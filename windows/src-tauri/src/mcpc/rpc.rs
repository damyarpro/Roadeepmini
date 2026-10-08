// JSON-RPC 2.0 framing for the MCP client, shared by both transports: building
// messages, sorting what a server sends into responses / requests /
// notifications, and the Server-Sent Events parser of the Streamable HTTP
// transport. No I/O here, so all of it is unit-tested.

use serde_json::{json, Map, Value};

/// What we ask for. A server that speaks another version answers with its own,
/// and `tools/list` / `tools/call` are the same in every published version.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const CLIENT_NAME: &str = "Roadeep Desktop";

/// JSON-RPC "method not found": our answer to any server→client request but ping.
pub const METHOD_NOT_FOUND: i64 = -32601;

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The answer to one of our requests (we only ever use numeric ids).
    Response { id: u64, result: Result<Value, RpcError> },
    /// The server asks us something; `id` is echoed back as is.
    Request { id: Value, method: String },
    Notification { method: String },
    /// Not JSON-RPC, or a response to an id we never sent.
    Invalid,
}

pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

pub fn notification(method: &str, params: Option<Value>) -> Value {
    let mut msg = Map::new();
    msg.insert("jsonrpc".into(), json!("2.0"));
    msg.insert("method".into(), json!(method));
    if let Some(p) = params {
        msg.insert("params".into(), p);
    }
    Value::Object(msg)
}

pub fn initialize_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        // We use tools only: no roots, sampling or elicitation offered.
        "capabilities": {},
        "clientInfo": { "name": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION") },
    })
}

/// Our answer to a server→client request: `ping` gets `{}`, anything else
/// (sampling, elicitation, roots…) "method not found" — we offer none of them.
pub fn reply_to(id: Value, method: &str) -> Value {
    if method == "ping" {
        json!({ "jsonrpc": "2.0", "id": id, "result": {} })
    } else {
        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": METHOD_NOT_FOUND, "message": "Method not found" } })
    }
}

fn response_id(id: &Value) -> Option<u64> {
    // Some servers echo ids as strings; ours are always plain numbers.
    id.as_u64().or_else(|| id.as_str().and_then(|s| s.parse().ok()))
}

pub fn classify(msg: &Value) -> Incoming {
    let Some(obj) = msg.as_object() else { return Incoming::Invalid };
    if let Some(method) = obj.get("method").and_then(Value::as_str) {
        let method = method.to_string();
        return match obj.get("id") {
            Some(id) if !id.is_null() => Incoming::Request { id: id.clone(), method },
            _ => Incoming::Notification { method },
        };
    }
    let Some(id) = obj.get("id").and_then(response_id) else { return Incoming::Invalid };
    if let Some(err) = obj.get("error") {
        let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
        let message = err.get("message").and_then(Value::as_str).unwrap_or("").to_string();
        return Incoming::Response { id, result: Err(RpcError { code, message }) };
    }
    match obj.get("result") {
        Some(result) => Incoming::Response { id, result: Ok(result.clone()) },
        None => Incoming::Invalid,
    }
}

/// A body or SSE `data` may hold one message or (older servers) a batch.
pub fn messages(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        other => vec![other],
    }
}

// ── Server-Sent Events ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

/// Incremental SSE parser (WHATWG rules: lines end in CRLF, LF or CR; `data`
/// lines are joined with LF; a blank line dispatches; `:` starts a comment).
/// Bytes are buffered until a line is complete, so a chunk boundary inside a
/// multi-byte character is harmless.
#[derive(Default)]
pub struct SseParser {
    line: Vec<u8>,
    /// The previous chunk ended on CR: a LF at the start of the next one belongs to it.
    after_cr: bool,
    event: String,
    data: String,
    has_data: bool,
}

impl SseParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        for &b in bytes {
            if self.after_cr {
                self.after_cr = false;
                if b == b'\n' {
                    continue;
                }
            }
            match b {
                b'\n' => self.end_line(&mut out),
                b'\r' => {
                    self.after_cr = true;
                    self.end_line(&mut out);
                }
                _ => self.line.push(b),
            }
        }
        out
    }

    fn end_line(&mut self, out: &mut Vec<SseEvent>) {
        let line = String::from_utf8_lossy(&std::mem::take(&mut self.line)).into_owned();
        if line.is_empty() {
            if self.has_data {
                let mut data = std::mem::take(&mut self.data);
                if data.ends_with('\n') {
                    data.pop();
                }
                out.push(SseEvent { event: std::mem::take(&mut self.event), data });
            }
            self.event.clear();
            self.data.clear();
            self.has_data = false;
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.find(':') {
            Some(i) => {
                let v = &line[i + 1..];
                (&line[..i], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (line.as_str(), ""),
        };
        match field {
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
                self.has_data = true;
            }
            "event" => self.event = value.to_string(),
            // `id` and `retry` only matter for resuming a stream, which we don't do.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_notifications_are_well_formed() {
        assert_eq!(
            request(7, "tools/list", json!({ "cursor": "c" })),
            json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list", "params": { "cursor": "c" } })
        );
        assert_eq!(notification("notifications/initialized", None), json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        let init = initialize_params();
        assert_eq!(init["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(init["clientInfo"]["name"], "Roadeep Desktop");
        // One line on the wire: stdio framing depends on it.
        let line = serde_json::to_string(&request(1, "x", json!({ "t": "a\nb" }))).unwrap();
        assert!(!line.contains('\n'));
    }

    #[test]
    fn incoming_messages_are_classified() {
        assert_eq!(
            classify(&json!({ "jsonrpc": "2.0", "id": 3, "result": { "ok": true } })),
            Incoming::Response { id: 3, result: Ok(json!({ "ok": true })) }
        );
        assert_eq!(
            classify(&json!({ "jsonrpc": "2.0", "id": "4", "error": { "code": -32602, "message": "bad" } })),
            Incoming::Response { id: 4, result: Err(RpcError { code: -32602, message: "bad".into() }) }
        );
        assert_eq!(
            classify(&json!({ "jsonrpc": "2.0", "id": "abc", "method": "ping" })),
            Incoming::Request { id: json!("abc"), method: "ping".into() }
        );
        assert_eq!(
            classify(&json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" })),
            Incoming::Notification { method: "notifications/tools/list_changed".into() }
        );
        for bad in [json!(1), json!({ "id": 1 }), json!({ "id": "x", "result": {} }), json!({ "result": {} })] {
            assert_eq!(classify(&bad), Incoming::Invalid, "{bad}");
        }
        assert_eq!(messages(json!([1, 2])).len(), 2);
        assert_eq!(messages(json!({ "a": 1 })).len(), 1);
    }

    #[test]
    fn server_requests_get_ping_or_method_not_found() {
        assert_eq!(reply_to(json!(9), "ping"), json!({ "jsonrpc": "2.0", "id": 9, "result": {} }));
        let other = reply_to(json!("s1"), "sampling/createMessage");
        assert_eq!(other["id"], "s1");
        assert_eq!(other["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn sse_events_are_parsed_across_chunks_and_line_endings() {
        let mut p = SseParser::default();
        assert!(p.push(b": keep-alive\n\nevent: mess").is_empty());
        assert!(p.push(b"age\r\ndata: {\"a\":").is_empty());
        let events = p.push(b"1}\r\n\r\ndata:x\rdata: y\r");
        assert_eq!(events, vec![SseEvent { event: "message".into(), data: "{\"a\":1}".into() }]);
        let events = p.push(b"\nid: 5\n\n");
        assert_eq!(events, vec![SseEvent { event: String::new(), data: "x\ny".into() }]);
        // A blank line without data dispatches nothing.
        assert!(p.push(b"event: ping\n\n").is_empty());
        // UTF-8 split between chunks.
        let bytes = "data: سلام\n\n".as_bytes();
        assert!(p.push(&bytes[..8]).is_empty());
        assert_eq!(p.push(&bytes[8..])[0].data, "سلام");
    }
}
