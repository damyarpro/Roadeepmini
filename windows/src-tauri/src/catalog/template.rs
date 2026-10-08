// Placeholders in manifest strings, and the host rules requests are held to.
//
// Only `{field.…}` and `{item.…}` start a placeholder; every other brace is
// literal text, so a GraphQL query in a POST body needs no escaping.
//
//     {field.<name>}          a field value (percent-encoded in a URL)
//     {field.<name>|base}     a url-kind field as a base URL, only at the very start
//     {item.<pointer>}        a JSON pointer into one list item (percent-encoded)
//     {item.<pointer>|url}    an item value that is itself a whole URL (list.url only)

use std::collections::HashMap;

use serde_json::Value;

/// Field name → the value the user saved (trimmed, already checked).
pub type Values = HashMap<String, String>;

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Lit(String),
    Field { name: String, base: bool },
    Item { pointer: String, url: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub parts: Vec<Part>,
}

/// Where a template sits decides what it may contain.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Context {
    /// request.url: fields; `|base` at the start.
    RequestUrl,
    /// A query value or a body string: fields only, inserted raw (the query
    /// encoder or the JSON serializer escapes them).
    Value,
    /// openUrl: fields; `|base` at the start.
    OpenUrl,
    /// list.url: fields and items; `|base` at the start; `{item.x|url}` alone.
    ItemUrl,
}

pub fn valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// "" (the whole value) or "/a/b" — RFC 6901, as serde_json reads it.
pub fn valid_pointer(pointer: &str) -> bool {
    (pointer.is_empty() || pointer.starts_with('/')) && pointer.len() <= 200 && !pointer.chars().any(char::is_control)
}

impl Template {
    pub fn parse(text: &str, context: Context) -> Result<Template, String> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut rest = text;
        while let Some(start) = next_placeholder(rest) {
            lit.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let end = after.find('}').ok_or_else(|| format!("unclosed placeholder in {text:?}"))?;
            let inner = &after[..end];
            let (head, modifier) = match inner.split_once('|') {
                Some((h, m)) => (h, Some(m)),
                None => (inner, None),
            };
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(&mut lit)));
            }
            if let Some(name) = head.strip_prefix("field.") {
                if !valid_field_name(name) {
                    return Err(format!("bad field name in {{{inner}}}"));
                }
                let base = match modifier {
                    None => false,
                    Some("base") => true,
                    Some(m) => return Err(format!("unknown modifier |{m} in {{{inner}}}")),
                };
                if base && (!parts.is_empty() || !matches!(context, Context::RequestUrl | Context::OpenUrl | Context::ItemUrl)) {
                    return Err(format!("{{{inner}}} is only allowed at the very start of a URL"));
                }
                parts.push(Part::Field { name: name.to_string(), base });
            } else if let Some(pointer) = head.strip_prefix("item.") {
                if context != Context::ItemUrl {
                    return Err(format!("{{{inner}}} is only allowed in list.url"));
                }
                if !valid_pointer(pointer) {
                    return Err(format!("bad JSON pointer in {{{inner}}}"));
                }
                let url = match modifier {
                    None => false,
                    Some("url") => true,
                    Some(m) => return Err(format!("unknown modifier |{m} in {{{inner}}}")),
                };
                parts.push(Part::Item { pointer: pointer.to_string(), url });
            }
            rest = &after[end + 1..];
        }
        lit.push_str(rest);
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        let template = Template { parts };
        let raw_items = template.parts.iter().filter(|p| matches!(p, Part::Item { url: true, .. })).count();
        if raw_items > 0 && template.parts.len() != 1 {
            return Err(format!("{{item.…|url}} must be the whole of {text:?}"));
        }
        Ok(template)
    }

    pub fn fields(&self) -> impl Iterator<Item = (&str, bool)> {
        self.parts.iter().filter_map(|p| match p {
            Part::Field { name, base } => Some((name.as_str(), *base)),
            _ => None,
        })
    }

    pub fn has_placeholders(&self) -> bool {
        self.parts.iter().any(|p| !matches!(p, Part::Lit(_)))
    }

    /// True when the whole template is one `{item.x|url}`.
    pub fn is_raw_item_url(&self) -> bool {
        matches!(self.parts.as_slice(), [Part::Item { url: true, .. }])
    }

    /// Fills the placeholders. `encode` percent-encodes field and item values
    /// as one path segment / query value (URLs); otherwise they go in raw.
    /// A missing item value fails (that item gets no link); a missing optional
    /// field is empty.
    pub fn expand(&self, values: &Values, item: Option<&Value>, encode: bool) -> Option<String> {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Lit(s) => out.push_str(s),
                Part::Field { name, base: true } => {
                    out.push_str(values.get(name).map(String::as_str).unwrap_or("").trim_end_matches('/'));
                }
                Part::Field { name, base: false } => {
                    let v = values.get(name).map(String::as_str).unwrap_or("");
                    if encode {
                        out.push_str(&percent_encode(v));
                    } else {
                        out.push_str(v);
                    }
                }
                Part::Item { pointer, url } => {
                    let v = scalar_text(item?.pointer(pointer)?)?;
                    if *url || !encode {
                        out.push_str(&v);
                    } else {
                        out.push_str(&percent_encode(&v));
                    }
                }
            }
        }
        Some(out)
    }
}

/// The next `{field.` or `{item.`, as a byte offset.
fn next_placeholder(s: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = s[from..].find('{') {
        let at = from + i;
        let tail = &s[at + 1..];
        if tail.starts_with("field.") || tail.starts_with("item.") {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// A JSON scalar as text: ids come as numbers from some APIs, flags as booleans.
pub fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Everything but RFC 3986 "unreserved" is escaped, so a value can never add
/// a path segment, a query parameter or a fragment.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ── Hosts ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum HostRule {
    /// "api.example.com" — that host, default port.
    Exact(String),
    /// "*.atlassian.net" — exactly one more label in front, default port.
    Wildcard(String),
    /// "{field.siteUrl}" — the host (and port) of that url-kind field's value.
    Field(String),
}

fn valid_hostname(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

impl HostRule {
    pub fn parse(entry: &str) -> Result<HostRule, String> {
        if let Some(inner) = entry.strip_prefix("{field.").and_then(|r| r.strip_suffix('}')) {
            return if valid_field_name(inner) {
                Ok(HostRule::Field(inner.to_string()))
            } else {
                Err(format!("bad host entry {entry:?}"))
            };
        }
        if let Some(suffix) = entry.strip_prefix("*.") {
            // "*.com" would allow the whole TLD.
            return if valid_hostname(suffix) && suffix.contains('.') {
                Ok(HostRule::Wildcard(suffix.to_string()))
            } else {
                Err(format!("bad wildcard host {entry:?} (use \"*.example.com\")"))
            };
        }
        if valid_hostname(entry) && (entry.contains('.') || entry == "localhost") {
            Ok(HostRule::Exact(entry.to_string()))
        } else {
            Err(format!("bad host {entry:?} (lowercase host name, no scheme, port or path)"))
        }
    }

    fn allows(&self, url: &reqwest::Url, values: &Values) -> bool {
        let Some(host) = url.host_str() else { return false };
        match self {
            HostRule::Exact(h) => url.port().is_none() && host == h,
            HostRule::Wildcard(suffix) => {
                url.port().is_none()
                    && host
                        .strip_suffix(suffix.as_str())
                        .and_then(|p| p.strip_suffix('.'))
                        .is_some_and(|label| !label.contains('.') && valid_hostname(label))
            }
            HostRule::Field(name) => {
                let Some(base) = values.get(name).and_then(|v| reqwest::Url::parse(v).ok()) else { return false };
                base.host_str().is_some_and(|h| h == host)
                    && base.port_or_known_default() == url.port_or_known_default()
                    && base.scheme() == url.scheme()
            }
        }
    }
}

pub fn is_local(url: &reqwest::Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// https (http only to this machine), no credentials, and a host the rules allow.
pub fn url_allowed(url: &reqwest::Url, rules: &[HostRule], values: &Values) -> bool {
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && is_local(url));
    scheme_ok
        && url.username().is_empty()
        && url.password().is_none()
        && rules.iter().any(|r| r.allows(url, values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(pairs: &[(&str, &str)]) -> Values {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn catalog_templates_parse_and_expand_with_encoding() {
        let t = Template::parse("https://api.x.com/v1/{field.org}/items?q={field.q}", Context::RequestUrl).unwrap();
        assert_eq!(t.fields().map(|(n, _)| n).collect::<Vec<_>>(), ["org", "q"]);
        let v = values(&[("org", "a b/../c?d#e"), ("q", "ü&x=1")]);
        assert_eq!(
            t.expand(&v, None, true).unwrap(),
            "https://api.x.com/v1/a%20b%2F..%2Fc%3Fd%23e/items?q=%C3%BC%26x%3D1"
        );
        // Raw for query values and bodies: their own encoder escapes them.
        let raw = Template::parse("{field.q}", Context::Value).unwrap();
        assert_eq!(raw.expand(&v, None, false).unwrap(), "ü&x=1");
    }

    #[test]
    fn catalog_templates_leave_other_braces_alone() {
        let body = "query { viewer { issues(first: 5) { nodes { id } } } } {field.team}";
        let t = Template::parse(body, Context::Value).unwrap();
        let out = t.expand(&values(&[("team", "\"x\"")]), None, false).unwrap();
        assert_eq!(out, "query { viewer { issues(first: 5) { nodes { id } } } } \"x\"");
    }

    #[test]
    fn catalog_base_only_at_the_start() {
        let t = Template::parse("{field.site|base}/wp-json/wc/v3/orders", Context::RequestUrl).unwrap();
        assert_eq!(t.expand(&values(&[("site", "https://shop.example.com/")]), None, true).unwrap(), "https://shop.example.com/wp-json/wc/v3/orders");
        assert!(Template::parse("https://x.com/{field.site|base}", Context::RequestUrl).is_err());
        assert!(Template::parse("{field.site|base}", Context::Value).is_err());
        assert!(Template::parse("{field.site|nope}", Context::RequestUrl).is_err());
        assert!(Template::parse("https://x.com/{field.site", Context::RequestUrl).is_err());
        assert!(Template::parse("https://x.com/{field.bad-name}", Context::RequestUrl).is_err());
    }

    #[test]
    fn catalog_item_placeholders() {
        let item = json!({ "id": 42, "key": "AB-1/2", "links": { "html": "https://x.com/a?b=1" } });
        let t = Template::parse("https://{field.site}.atlassian.net/browse/{item./key}", Context::ItemUrl).unwrap();
        assert_eq!(t.expand(&values(&[("site", "acme")]), Some(&item), true).unwrap(), "https://acme.atlassian.net/browse/AB-1%2F2");
        let raw = Template::parse("{item./links/html|url}", Context::ItemUrl).unwrap();
        assert!(raw.is_raw_item_url());
        assert_eq!(raw.expand(&Values::new(), Some(&item), true).unwrap(), "https://x.com/a?b=1");
        // A missing value gives no URL rather than a broken one.
        assert_eq!(t.expand(&values(&[("site", "acme")]), Some(&json!({})), true), None);
        assert!(Template::parse("https://x.com/{item./links/html|url}", Context::ItemUrl).is_err());
        assert!(Template::parse("https://x.com/{item./id}", Context::OpenUrl).is_err());
        assert!(Template::parse("https://x.com/{item.id}", Context::ItemUrl).is_err(), "pointers start with /");
    }

    #[test]
    fn catalog_hosts_exact_wildcard_and_field() {
        let rules = vec![
            HostRule::parse("api.example.com").unwrap(),
            HostRule::parse("*.atlassian.net").unwrap(),
            HostRule::parse("{field.siteUrl}").unwrap(),
        ];
        let v = values(&[("siteUrl", "https://shop.example.org:8443")]);
        let ok = |u: &str| url_allowed(&reqwest::Url::parse(u).unwrap(), &rules, &v);
        assert!(ok("https://api.example.com/v1"));
        assert!(ok("https://acme.atlassian.net/rest/api/3/search"));
        assert!(ok("https://shop.example.org:8443/wp-json"));
        assert!(!ok("http://api.example.com/v1"), "https only");
        assert!(!ok("https://api.example.com:444/v1"), "default port only");
        assert!(!ok("https://evil.com.atlassian.net/"), "one label only");
        assert!(!ok("https://atlassian.net/"));
        assert!(!ok("https://api.example.com.evil.com/"));
        assert!(!ok("https://shop.example.org/wp-json"), "the field's port");
        assert!(!ok("https://user:pw@api.example.com/"));
        assert!(!url_allowed(&reqwest::Url::parse("https://shop.example.org:8443/").unwrap(), &rules, &Values::new()));

        let local = vec![HostRule::parse("localhost").unwrap()];
        assert!(url_allowed(&reqwest::Url::parse("http://localhost/x").unwrap(), &local, &Values::new()));

        for bad in ["*.com", "*.", "https://x.com", "x.com/a", "X.com", "x.com:443", "{field.a-b}", "nodot", ""] {
            assert!(HostRule::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn catalog_percent_encoding_keeps_only_unreserved() {
        assert_eq!(percent_encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(percent_encode("a/b?c&d=e#f g%"), "a%2Fb%3Fc%26d%3De%23f%20g%25");
        assert_eq!(percent_encode("سلام").matches('%').count(), 8);
    }
}
