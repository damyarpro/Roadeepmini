// Streamable HTTP transport (MCP 2025-06-18): every message is a POST; the
// answer is either a JSON body or an SSE stream that carries it (and maybe a
// few server requests/notifications first). The session id the server hands
// out at `initialize` goes on every later request; a 404 for that session means
// it expired, and we initialize again once.
//
// Redirects are never followed and the URL is fixed, so the token only ever
// goes to the origin the user configured (store.rs makes changing the URL's
// origin forget the token). Never logged: the URL (it may carry a query), the
// token, any header.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE, WWW_AUTHENTICATE};
use serde_json::{json, Value};

use super::rpc::{self, Incoming, SseParser};
use super::stdio::rpc_error;
use super::tools::one_line;
use crate::errors;

const MAX_BODY: usize = 4 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const SESSION_HEADER: &str = "mcp-session-id";
const VERSION_HEADER: &str = "mcp-protocol-version";

/// A valid OAuth access token on demand (mcpc/oauth.rs, through mod.rs). A
/// closure rather than an AppHandle here keeps the transport testable without
/// a Tauri runtime.
pub type TokenSource = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

pub enum HttpAuth {
    None,
    /// The `token` slot, sent as `Authorization: Bearer …` or under a header of
    /// the user's choosing.
    Token { header: String, bearer: bool },
    /// mcpc/oauth.rs gives a valid access token (refreshing it when needed).
    OAuth(TokenSource),
    #[cfg(test)]
    Fixed(String, String),
}

pub struct HttpWire {
    server_id: String,
    url: reqwest::Url,
    auth: HttpAuth,
    client: reqwest::Client,
    session: Mutex<Option<String>>,
    version: Mutex<Option<String>>,
    next_id: AtomicU64,
    dirty: Arc<AtomicBool>,
    /// One re-initialization at a time.
    reinit: tokio::sync::Mutex<()>,
}

enum Fail {
    Coded(String),
    /// 404 while we had a session: it expired.
    SessionGone,
}

impl From<String> for Fail {
    fn from(e: String) -> Self {
        Fail::Coded(e)
    }
}

/// reqwest's own message stops short; the cause ("connection refused", "dns
/// error"…) is in the source chain. The URL is stripped, and any cause that
/// would mention one is skipped.
fn describe(e: reqwest::Error) -> String {
    if e.is_timeout() {
        return errors::coded(errors::MCPC_TIMEOUT, &[]);
    }
    let e = e.without_url();
    let mut parts = vec![e.to_string()];
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        let text = s.to_string();
        if !text.is_empty() && !parts.contains(&text) && !text.contains("://") {
            parts.push(text);
        }
        source = s.source();
    }
    errors::coded(errors::MCPC_NETWORK, &[&one_line(&parts.join(": "), 240)])
}

/// A session id as the spec allows it: visible ASCII only.
fn valid_session(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && id.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

impl HttpWire {
    pub fn new(server_id: &str, url: &str, auth: HttpAuth, dirty: Arc<AtomicBool>) -> Result<Arc<HttpWire>, String> {
        let url = super::store::check_url(url)?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent("Roadeep-Desktop");
        if loopback {
            // A local server is never behind the user's VPN/system proxy.
            builder = builder.no_proxy();
        } else {
            builder = builder.https_only(true);
        }
        let client = builder.build().map_err(|e| errors::coded(errors::MCPC_NETWORK, &[&one_line(&e.to_string(), 200)]))?;
        Ok(Arc::new(HttpWire {
            server_id: server_id.to_string(),
            url,
            auth,
            client,
            session: Mutex::new(None),
            version: Mutex::new(None),
            next_id: AtomicU64::new(1),
            dirty,
            reinit: tokio::sync::Mutex::new(()),
        }))
    }

    pub fn alive(&self) -> bool {
        true
    }

    async fn auth_header(&self) -> Result<Option<(HeaderName, HeaderValue)>, String> {
        let needs_auth = || errors::coded(errors::MCPC_NEEDS_AUTH, &[]);
        let (name, value) = match &self.auth {
            HttpAuth::None => return Ok(None),
            HttpAuth::Token { header, bearer } => {
                let token = crate::secrets::mcpc_get(&self.server_id, "token").ok_or_else(needs_auth)?;
                (header.clone(), if *bearer { format!("Bearer {token}") } else { token })
            }
            HttpAuth::OAuth(source) => {
                let token = source().await.ok_or_else(needs_auth)?;
                ("Authorization".to_string(), format!("Bearer {token}"))
            }
            #[cfg(test)]
            HttpAuth::Fixed(name, value) => (name.clone(), value.clone()),
        };
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| errors::coded(errors::MCPC_INVALID, &["header"]))?;
        let mut value = HeaderValue::from_str(&value).map_err(|_| errors::coded(errors::MCPC_INVALID, &["value"]))?;
        value.set_sensitive(true);
        Ok(Some((name, value)))
    }

    async fn post(&self, msg: &Value) -> Result<reqwest::Response, Fail> {
        let mut req = self
            .client
            .post(self.url.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream");
        let session = self.session.lock().unwrap().clone();
        if let Some(sid) = &session {
            req = req.header(SESSION_HEADER, sid);
        }
        if let Some(v) = self.version.lock().unwrap().clone() {
            req = req.header(VERSION_HEADER, v);
        }
        if let Some((name, value)) = self.auth_header().await? {
            req = req.header(name, value);
        }
        let body = serde_json::to_vec(msg).map_err(|e| errors::coded(errors::MCPC_PROTOCOL, &[&e.to_string()]))?;
        let resp = req.body(body).send().await.map_err(describe)?;
        let status = resp.status().as_u16();
        if status == 401 {
            let hint = resp.headers().get(WWW_AUTHENTICATE).and_then(|v| v.to_str().ok());
            super::oauth::on_unauthorized(&self.server_id, hint);
            return Err(Fail::Coded(errors::coded(errors::MCPC_NEEDS_AUTH, &[])));
        }
        if status == 404 && session.is_some() {
            return Err(Fail::SessionGone);
        }
        if !(200..300).contains(&status) {
            return Err(Fail::Coded(errors::coded(errors::MCPC_HTTP, &[&status.to_string()])));
        }
        Ok(resp)
    }

    /// Sends one message; for a request (`want`), reads the body or the SSE
    /// stream until its response.
    async fn exchange(self: &Arc<Self>, msg: &Value, want: Option<u64>) -> Result<Option<Value>, Fail> {
        let mut resp = self.post(msg).await?;
        if msg.get("method").and_then(Value::as_str) == Some("initialize") {
            let sid = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()).filter(|s| valid_session(s)).map(str::to_string);
            *self.session.lock().unwrap() = sid;
        }
        let Some(want) = want else { return Ok(None) };
        let sse = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("text/event-stream"));
        let mut total = 0usize;
        if sse {
            let mut parser = SseParser::default();
            while let Some(chunk) = resp.chunk().await.map_err(describe)? {
                total += chunk.len();
                if total > MAX_BODY {
                    return Err(Fail::Coded(errors::coded(errors::MCPC_TOO_LARGE, &[])));
                }
                for event in parser.push(&chunk) {
                    if !(event.event.is_empty() || event.event == "message") {
                        continue;
                    }
                    let Ok(value) = serde_json::from_str::<Value>(&event.data) else { continue };
                    if let Some(found) = self.handle(value, want) {
                        return found.map(Some).map_err(Fail::Coded);
                    }
                }
            }
            return Err(Fail::Coded(errors::coded(errors::MCPC_PROTOCOL, &["the stream ended without an answer"])));
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(describe)? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(Fail::Coded(errors::coded(errors::MCPC_TOO_LARGE, &[])));
            }
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body).map_err(|_| errors::coded(errors::MCPC_PROTOCOL, &["the answer is not JSON"]))?;
        match self.handle(value, want) {
            Some(found) => found.map(Some).map_err(Fail::Coded),
            None => Err(Fail::Coded(errors::coded(errors::MCPC_PROTOCOL, &["no answer to the request"]))),
        }
    }

    /// Server messages that arrive with a response: requests get an answer
    /// (posted separately), list changes are noted. Returns our response if
    /// it is among them.
    fn handle(self: &Arc<Self>, value: Value, want: u64) -> Option<Result<Value, String>> {
        for msg in rpc::messages(value) {
            match rpc::classify(&msg) {
                Incoming::Response { id, result } if id == want => return Some(result.map_err(|e| rpc_error(&e))),
                Incoming::Response { .. } | Incoming::Invalid => {}
                Incoming::Request { id, method } => {
                    let this = self.clone();
                    let reply = rpc::reply_to(id, &method);
                    tokio::spawn(async move {
                        let _ = this.post(&reply).await;
                    });
                }
                Incoming::Notification { method } => {
                    if method == "notifications/tools/list_changed" {
                        self.dirty.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
        None
    }

    async fn initialize(self: &Arc<Self>) -> Result<Value, Fail> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        *self.version.lock().unwrap() = None;
        let result = self.exchange(&rpc::request(id, "initialize", rpc::initialize_params()), Some(id)).await?.unwrap_or(Value::Null);
        if let Some(v) = result.get("protocolVersion").and_then(Value::as_str).filter(|v| v.len() <= 32 && v.is_ascii()) {
            *self.version.lock().unwrap() = Some(v.to_string());
        }
        Ok(result)
    }

    /// The session expired: a new one, unless another request already made it.
    async fn reinitialize(self: &Arc<Self>, stale: Option<String>) -> Result<(), String> {
        let _one = self.reinit.lock().await;
        let current = self.session.lock().unwrap().clone();
        if current.is_some() && current != stale {
            return Ok(());
        }
        *self.session.lock().unwrap() = None;
        match self.initialize().await {
            Ok(_) => {}
            Err(Fail::Coded(e)) => return Err(e),
            Err(Fail::SessionGone) => return Err(errors::coded(errors::MCPC_HTTP, &["404"])),
        }
        self.notify("notifications/initialized", None).await
    }

    pub async fn request(self: &Arc<Self>, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let work = async {
            let needs_auth = errors::coded(errors::MCPC_NEEDS_AUTH, &[]);
            let mut session_retried = false;
            // A refused OAuth token: oauth.rs refreshes it on the next `bearer`,
            // so one more try is worth it. A 401 means nothing ran.
            let mut auth_retried = !matches!(self.auth, HttpAuth::OAuth(_));
            loop {
                let stale = self.session.lock().unwrap().clone();
                let outcome = if method == "initialize" {
                    self.initialize().await
                } else {
                    let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                    let mut guard = CancelGuard { wire: self.clone(), id, done: false };
                    let outcome = self.exchange(&rpc::request(id, method, params.clone()), Some(id)).await;
                    guard.done = true;
                    outcome.map(|v| v.unwrap_or(Value::Null))
                };
                match outcome {
                    Ok(v) => return Ok(v),
                    Err(Fail::Coded(e)) if e == needs_auth && !auth_retried => auth_retried = true,
                    Err(Fail::Coded(e)) => return Err(e),
                    Err(Fail::SessionGone) if !session_retried && method != "initialize" => {
                        session_retried = true;
                        self.reinitialize(stale).await?;
                    }
                    Err(Fail::SessionGone) => return Err(errors::coded(errors::MCPC_HTTP, &["404"])),
                }
            }
        };
        tokio::time::timeout(timeout, work).await.unwrap_or_else(|_| Err(errors::coded(errors::MCPC_TIMEOUT, &[])))
    }

    pub async fn notify(self: &Arc<Self>, method: &str, params: Option<Value>) -> Result<(), String> {
        let msg = rpc::notification(method, params);
        let work = self.exchange(&msg, None);
        match tokio::time::timeout(Duration::from_secs(30), work).await {
            Err(_) => Err(errors::coded(errors::MCPC_TIMEOUT, &[])),
            Ok(Ok(_)) => Ok(()),
            Ok(Err(Fail::Coded(e))) => Err(e),
            Ok(Err(Fail::SessionGone)) => Err(errors::coded(errors::MCPC_HTTP, &["404"])),
        }
    }
}

/// A request dropped before its answer (cancel button, timeout): tell the
/// server, as the spec asks. Closing the connection alone doesn't mean "cancel".
struct CancelGuard {
    wire: Arc<HttpWire>,
    id: u64,
    done: bool,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            let wire = self.wire.clone();
            let msg = rpc::notification("notifications/cancelled", Some(json!({ "requestId": self.id, "reason": "Cancelled by the user" })));
            rt.spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(10), wire.post(&msg)).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Debug, Clone)]
    struct Req {
        headers: Vec<(String, String)>,
        body: Value,
    }

    impl Req {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }
        fn method(&self) -> &str {
            self.body["method"].as_str().unwrap_or("")
        }
    }

    type Route = Arc<dyn Fn(&Req, usize) -> String + Send + Sync>;

    /// A one-request-per-connection HTTP/1.1 server; `route` gets each request
    /// and its index and returns the raw response.
    async fn serve(route: Route) -> (String, Arc<Mutex<Vec<Req>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<Vec<Req>>> = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let route = route.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let head_end = loop {
                        let n = socket.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let headers: Vec<(String, String)> = head
                        .lines()
                        .skip(1)
                        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
                        .collect();
                    let len: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                    while buf.len() < head_end + len {
                        let n = socket.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let body = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap_or(Value::Null);
                    let req = Req { headers, body };
                    let index = {
                        let mut l = log.lock().unwrap();
                        l.push(req.clone());
                        l.len() - 1
                    };
                    let reply = route(&req, index);
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (format!("http://127.0.0.1:{port}/mcp"), seen)
    }

    fn json_reply(extra_headers: &str, body: &Value) -> String {
        let body = body.to_string();
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn status(code: u16, extra_headers: &str) -> String {
        format!("HTTP/1.1 {code} X\r\n{extra_headers}Content-Length: 0\r\nConnection: close\r\n\r\n")
    }

    fn result(req: &Req, result: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": req.body["id"], "result": result })
    }

    fn init_result() -> Value {
        json!({ "protocolVersion": "2025-03-26", "capabilities": { "tools": {} }, "serverInfo": { "name": "t", "version": "1" } })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn json_and_sse_answers_with_a_session() {
        let route: Route = Arc::new(|req: &Req, _| match req.method() {
            "initialize" => json_reply("Mcp-Session-Id: sess-1\r\n", &result(req, init_result())),
            "notifications/initialized" => status(202, ""),
            "tools/list" => {
                let ping = json!({ "jsonrpc": "2.0", "id": 77, "method": "ping" });
                let changed = json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" });
                let answer = result(req, json!({ "tools": [{ "name": "a" }] }));
                let body = format!(": hi\r\n\r\nevent: message\r\ndata: {ping}\r\n\r\ndata: {changed}\n\ndata: {answer}\n\n");
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
            }
            _ => status(202, ""),
        });
        let (url, seen) = serve(route).await;
        let dirty = Arc::new(AtomicBool::new(false));
        let wire = HttpWire::new("srv-1", &url, HttpAuth::Fixed("Authorization".into(), "Bearer t0k".into()), dirty.clone()).unwrap();
        let t = Duration::from_secs(5);
        let init = wire.request("initialize", rpc::initialize_params(), t).await.unwrap();
        assert_eq!(init["serverInfo"]["name"], "t");
        wire.notify("notifications/initialized", None).await.unwrap();
        let list = wire.request("tools/list", json!({}), t).await.unwrap();
        assert_eq!(list["tools"][0]["name"], "a");
        assert!(dirty.load(Ordering::SeqCst));
        // The ping reply is posted on its own.
        for _ in 0..50 {
            if seen.lock().unwrap().len() >= 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let reqs = seen.lock().unwrap().clone();
        assert_eq!(reqs[0].header(SESSION_HEADER), None);
        assert_eq!(reqs[0].header("accept"), Some("application/json, text/event-stream"));
        assert_eq!(reqs[0].header("authorization"), Some("Bearer t0k"));
        assert_eq!(reqs[0].header("user-agent"), Some("Roadeep-Desktop"));
        assert_eq!(reqs[1].header(SESSION_HEADER), Some("sess-1"));
        assert_eq!(reqs[1].header(VERSION_HEADER), Some("2025-03-26"), "the server's version is used");
        assert_eq!(reqs[2].header(SESSION_HEADER), Some("sess-1"));
        let pong = reqs.iter().find(|r| r.body["id"] == 77).expect("ping answered");
        assert_eq!(pong.body["result"], json!({}));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_expired_session_is_renewed_once() {
        let route: Route = Arc::new(|req: &Req, _| match (req.method(), req.header(SESSION_HEADER)) {
            ("initialize", _) => json_reply("Mcp-Session-Id: fresh\r\n", &result(req, init_result())),
            ("tools/list", Some("old")) => status(404, ""),
            ("tools/list", Some("fresh")) => json_reply("", &result(req, json!({ "tools": [] }))),
            _ => status(202, ""),
        });
        let (url, seen) = serve(route).await;
        let wire = HttpWire::new("srv-1", &url, HttpAuth::None, Arc::new(AtomicBool::new(false))).unwrap();
        *wire.session.lock().unwrap() = Some("old".into());
        let list = wire.request("tools/list", json!({}), Duration::from_secs(5)).await.unwrap();
        assert_eq!(list, json!({ "tools": [] }));
        let methods: Vec<String> = seen.lock().unwrap().iter().map(|r| r.method().to_string()).collect();
        assert_eq!(methods, ["tools/list", "initialize", "notifications/initialized", "tools/list"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn auth_redirect_size_and_errors() {
        let big = "x".repeat(MAX_BODY + 10);
        let route: Route = Arc::new(move |req: &Req, _| match req.params_name() {
            "unauthorized" => status(401, "WWW-Authenticate: Bearer resource_metadata=\"https://a/.well-known/oauth-protected-resource\"\r\n"),
            "redirect" => status(302, "Location: https://elsewhere.example.com/steal\r\n"),
            "big" => format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{big}", big.len()),
            "rpcerr" => json_reply("", &json!({ "jsonrpc": "2.0", "id": req.body["id"], "error": { "code": -32000, "message": "nope" } })),
            "notjson" => "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_string(),
            _ => status(500, ""),
        });
        let (url, _) = serve(route).await;
        let wire = HttpWire::new("srv-1", &url, HttpAuth::None, Arc::new(AtomicBool::new(false))).unwrap();
        let call = |name: &str| {
            let wire = wire.clone();
            let params = json!({ "name": name });
            async move { wire.request("tools/call", params, Duration::from_secs(10)).await }
        };
        assert_eq!(call("unauthorized").await.unwrap_err(), "E_MCPC_NEEDS_AUTH");
        assert_eq!(call("redirect").await.unwrap_err(), "E_MCPC_HTTP|302");
        assert_eq!(call("big").await.unwrap_err(), "E_MCPC_TOO_LARGE");
        assert_eq!(call("rpcerr").await.unwrap_err(), "E_MCPC_RPC|-32000|nope");
        assert!(call("notjson").await.unwrap_err().starts_with("E_MCPC_PROTOCOL|"));
        assert_eq!(call("other").await.unwrap_err(), "E_MCPC_HTTP|500");
    }

    #[test]
    fn only_https_or_loopback_http() {
        let dirty = || Arc::new(AtomicBool::new(false));
        assert!(HttpWire::new("a1", "http://example.com/mcp", HttpAuth::None, dirty()).is_err());
        assert!(HttpWire::new("a1", "https://example.com/mcp", HttpAuth::None, dirty()).is_ok());
        assert!(HttpWire::new("a1", "http://localhost:3000/mcp", HttpAuth::None, dirty()).is_ok());
        assert!(valid_session("abc-123") && !valid_session("a b") && !valid_session(""));
    }

    impl Req {
        fn params_name(&self) -> &str {
            self.body["params"]["name"].as_str().unwrap_or("")
        }
    }
}
