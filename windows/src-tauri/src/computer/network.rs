//! Networkless containers receive only validated, DNS-pinned public HTTP(S) responses.
use std::{net::IpAddr, time::Duration};
use serde_json::{json, Value};
use super::encoding;
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a,b,c,_] = ip.octets();
            !(a == 0 || a == 10 || a == 127 || a >= 224 || (a == 169 && b == 254) || (a == 172 && (16..=31).contains(&b)) || (a == 192 && (b == 168 || b == 0 || (b == 88 && c == 99))) || (a == 100 && (64..=127).contains(&b)) || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100))) || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() { return public_ip(IpAddr::V4(v4)); }
            let s = ip.segments();
            // Only global unicast, excluding documentation and transition ranges.
            (s[0] & 0xe000) == 0x2000 && s[0] != 0x2002 && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
        }
    }
}
pub fn url(raw: &str) -> Result<reqwest::Url, &'static str> {
    if raw.len() > 4096 { return Err("computer-invalid-url"); }
    let parsed = reqwest::Url::parse(raw).map_err(|_| "computer-invalid-url")?;
    if !matches!(parsed.scheme(), "http" | "https") || !parsed.username().is_empty() || parsed.password().is_some() || !matches!(parsed.port_or_known_default(), Some(80 | 443)) { return Err("computer-invalid-url"); }
    let host = parsed.host_str().ok_or("computer-invalid-url")?;
    let trimmed = host.trim_matches(['[', ']']);
    if let Ok(ip) = trimmed.parse::<IpAddr>() { if !public_ip(ip) { return Err("computer-private-network"); } }
    if !host.contains('.') && !host.contains(':') { return Err("computer-private-network"); }
    if host.ends_with(".localhost") || host.ends_with(".local") || host == "localhost" { return Err("computer-private-network"); }
    Ok(parsed)
}
pub async fn fetch(frame: &Value) -> Value {
    let Some(id) = frame["id"].as_u64().filter(|n| *n < 1_000_000) else { return json!({"kind":"networkResult","id":0,"error":"computer-network-refused"}); };
    match tokio::time::timeout(Duration::from_secs(12), request(frame)).await {
        Ok(Ok(result)) => json!({"kind":"networkResult", "id":id, "response":result}),
        _ => json!({"kind":"networkResult", "id":id, "error":"computer-network-refused"}),
    }
}
async fn request(frame: &Value) -> Result<Value, ()> {
    let url = url(frame["url"].as_str().ok_or(())?).map_err(|_| ())?;
    let host = url.host_str().ok_or(())?.trim_matches(['[',']']).to_string();
    let port = url.port_or_known_default().ok_or(())?;
    let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), port)).await.map_err(|_| ())?.collect();
    if addresses.is_empty() || addresses.len() > 32 || addresses.iter().any(|a| !public_ip(a.ip())) { return Err(()); }
    let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
        .resolve(&host, addresses[0]).timeout(Duration::from_secs(10)).build().map_err(|_| ())?;
    let method = frame["method"].as_str().ok_or(())?;
    if !matches!(method, "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS") { return Err(()); }
    let mut request = client.request(reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| ())?, url);
    if let Some(headers) = frame["headers"].as_object() {
        if headers.len() > 64 { return Err(()); }
        if headers.iter().map(|(k,v)| k.len() + v.as_str().map(str::len).unwrap_or(99999)).sum::<usize>() > 24 * 1024 { return Err(()); }
        for (key, value) in headers {
            // Host/DNS and hop-by-hop behavior stay server-owned. Browser cookies remain browser-owned.
            if matches!(key.to_ascii_lowercase().as_str(), "host" | "connection" | "content-length" | "accept-encoding" | "proxy-authorization" | "proxy-connection" | "upgrade") { continue; }
            let value = value.as_str().ok_or(())?;
            if key.len() > 128 || value.len() > 8192 { return Err(()); }
            request = request.header(key, value);
        }
    }
    if let Some(body) = frame["body"].as_str() { if body.len() > 128 * 1024 { return Err(()); } request = request.body(body.to_owned()); }
    let mut response = request.send().await.map_err(|_| ())?;
    if response.content_length().is_some_and(|n| n > 1024 * 1024) { return Err(()); }
    let status = response.status().as_u16();
    let mut headers = serde_json::Map::new();
    for (key, value) in response.headers() {
        if !matches!(key.as_str(), "transfer-encoding" | "content-length" | "connection") {
            if let Ok(text) = value.to_str() { if text.len() < 8192 { headers.insert(key.to_string(), json!(text)); } }
        }
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? { if body.len() + chunk.len() > 1024 * 1024 { return Err(()); } body.extend_from_slice(&chunk); }
    Ok(json!({"status":status, "headers":headers, "body":encoding::encode(&body)}))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};
    #[test] fn denies_private_metadata_and_credential_urls() {
        for u in ["http://localhost", "http://127.0.0.1", "http://169.254.169.254", "http://10.1.2.3", "http://[::1]", "http://[::ffff:127.0.0.1]", "http://service.local", "file:///etc/passwd", "https://u:p@example.com", "http://example.com:2375"] { assert!(url(u).is_err(), "{u}"); }
        assert!(url("https://example.com/path").is_ok());
    }
    #[test] fn address_boundaries() {
        for ip in [Ipv4Addr::new(172,16,0,1), Ipv4Addr::new(100,64,0,1), Ipv4Addr::new(192,0,2,1), Ipv4Addr::new(198,18,0,1)] { assert!(!public_ip(ip.into())); }
        assert!(public_ip(Ipv4Addr::new(8,8,8,8).into()));
        assert!(!public_ip(Ipv6Addr::LOCALHOST.into()));
    }
}
