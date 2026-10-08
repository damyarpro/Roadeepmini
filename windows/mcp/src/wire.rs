//! What travels over `\\.\pipe\roadeep-mcp-<sid>`: one JSON line each way, one
//! request per connection.
//!
//!   → {"v":1,"tool":"roadeep_chat","arguments":{...}}
//!   ← {"ok":true,"result":{...}}
//!   ← {"ok":false,"error":{"code":"…","message":"…","status":429,"retryAfter":60,…}}

use serde_json::{json, Map, Value};

pub const VERSION: u64 = 1;
const PIPE_PREFIX: &str = r"\\.\pipe\roadeep-mcp-";

/// Largest request the app reads. Tool arguments are prompts and small JSON.
pub const MAX_REQUEST: usize = 1 << 20;
/// Largest response the relay reads. A subtype with every member's schema can be big.
pub const MAX_RESPONSE: usize = 8 << 20;

pub mod codes {
    pub const ROADEEP_NOT_RUNNING: &str = "ROADEEP_NOT_RUNNING";
    pub const NOT_SIGNED_IN: &str = "NOT_SIGNED_IN";
    pub const UNTRUSTED_PIPE: &str = "UNTRUSTED_PIPE";
    pub const TIMEOUT: &str = "TIMEOUT";
    pub const BUSY: &str = "BUSY";
    pub const BAD_REQUEST: &str = "BAD_REQUEST";
    pub const UNKNOWN_TOOL: &str = "UNKNOWN_TOOL";
    pub const INVALID_RESPONSE: &str = "INVALID_RESPONSE";
    pub const PIPE_ERROR: &str = "PIPE_ERROR";
}

/// The message every "you need the app" error carries.
pub const SIGN_IN_HINT: &str = "Open the Roadeep app and sign in, then try again.";

/// `\\.\pipe\roadeep-mcp-<key>`. The key is the user SID (user name only if the
/// SID cannot be read), so two accounts on one machine never share a pipe.
pub fn pipe_name(user_key: &str) -> String {
    format!("{PIPE_PREFIX}{user_key}")
}

#[derive(Debug, Clone, PartialEq)]
pub struct WireError {
    pub code: String,
    pub message: String,
    pub status: Option<u16>,
    pub retry_after: Option<u64>,
    pub request_id: Option<String>,
    /// `{field: [messages]}` from validation errors, so the model can fix its input.
    pub details: Option<Map<String, Value>>,
}

impl WireError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), status: None, retry_after: None, request_id: None, details: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Ok(Value),
    Err(WireError),
}

pub fn encode_request(tool: &str, arguments: &Value) -> String {
    let mut line = json!({ "v": VERSION, "tool": tool, "arguments": arguments }).to_string();
    line.push('\n');
    line
}

/// `(tool, arguments)`. Arguments default to `{}` and must be an object.
pub fn decode_request(line: &[u8]) -> Result<(String, Value), WireError> {
    let bad = |m: &str| WireError::new(codes::BAD_REQUEST, m);
    let value: Value = serde_json::from_slice(trim_line(line)).map_err(|_| bad("The request is not valid JSON."))?;
    if value.get("v").and_then(Value::as_u64) != Some(VERSION) {
        return Err(bad("Unsupported relay version. Update the Roadeep app."));
    }
    let tool = value
        .get("tool")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| bad("The request names no tool."))?
        .to_string();
    let arguments = match value.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return Err(bad("Tool arguments must be an object.")),
    };
    Ok((tool, arguments))
}

pub fn encode_response(outcome: &Outcome) -> String {
    let value = match outcome {
        Outcome::Ok(result) => json!({ "ok": true, "result": result }),
        Outcome::Err(err) => {
            let mut e = Map::new();
            e.insert("code".into(), json!(err.code));
            e.insert("message".into(), json!(err.message));
            if let Some(s) = err.status {
                e.insert("status".into(), json!(s));
            }
            if let Some(r) = err.retry_after {
                e.insert("retryAfter".into(), json!(r));
            }
            if let Some(id) = &err.request_id {
                e.insert("requestId".into(), json!(id));
            }
            if let Some(d) = &err.details {
                e.insert("details".into(), Value::Object(d.clone()));
            }
            json!({ "ok": false, "error": e })
        }
    };
    let mut line = value.to_string();
    line.push('\n');
    line
}

/// Anything we cannot read is an error result, never a panic.
pub fn decode_response(line: &[u8]) -> Outcome {
    let invalid = || Outcome::Err(WireError::new(codes::INVALID_RESPONSE, "The Roadeep app sent an unreadable answer."));
    let Ok(value) = serde_json::from_slice::<Value>(trim_line(line)) else { return invalid() };
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) => Outcome::Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        Some(false) => {
            let Some(e) = value.get("error") else { return invalid() };
            let text = |k: &str| e.get(k).and_then(Value::as_str).map(str::to_string);
            Outcome::Err(WireError {
                code: text("code").unwrap_or_else(|| codes::INVALID_RESPONSE.into()),
                message: text("message").unwrap_or_else(|| "Roadeep request failed.".into()),
                status: e.get("status").and_then(Value::as_u64).and_then(|s| u16::try_from(s).ok()),
                retry_after: e.get("retryAfter").and_then(Value::as_u64),
                request_id: text("requestId"),
                details: e.get("details").and_then(Value::as_object).cloned(),
            })
        }
        None => invalid(),
    }
}

fn trim_line(line: &[u8]) -> &[u8] {
    let line = line.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(line);
    let end = line.iter().position(|b| *b == b'\n').unwrap_or(line.len());
    let line = &line[..end];
    line.strip_suffix(b"\r").unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        let line = encode_request("roadeep_chat", &json!({ "message": "salam" }));
        assert!(line.ends_with('\n'));
        let (tool, args) = decode_request(line.as_bytes()).unwrap();
        assert_eq!(tool, "roadeep_chat");
        assert_eq!(args["message"], "salam");
    }

    #[test]
    fn bad_requests_are_refused() {
        assert!(decode_request(b"not json").is_err());
        assert!(decode_request(br#"{"tool":"x"}"#).is_err(), "missing version");
        assert!(decode_request(br#"{"v":2,"tool":"x"}"#).is_err());
        assert!(decode_request(br#"{"v":1}"#).is_err());
        assert!(decode_request(br#"{"v":1,"tool":"x","arguments":[1]}"#).is_err());
        let (_, args) = decode_request(br#"{"v":1,"tool":"x"}"#).unwrap();
        assert_eq!(args, json!({}));
    }

    #[test]
    fn responses_round_trip_including_errors() {
        let ok = Outcome::Ok(json!({ "reply": "hi" }));
        assert_eq!(decode_response(encode_response(&ok).as_bytes()), ok);

        let mut details = Map::new();
        details.insert("prompt".into(), json!(["required"]));
        let err = Outcome::Err(WireError {
            code: "THROTTLED".into(),
            message: "slow down".into(),
            status: Some(429),
            retry_after: Some(30),
            request_id: Some("r1".into()),
            details: Some(details),
        });
        assert_eq!(decode_response(encode_response(&err).as_bytes()), err);
    }

    #[test]
    fn unreadable_responses_become_errors() {
        for bad in [&b""[..], b"{", b"{}", br#"{"ok":false}"#] {
            let Outcome::Err(e) = decode_response(bad) else { panic!("{bad:?}") };
            assert_eq!(e.code, codes::INVALID_RESPONSE);
        }
    }

    #[test]
    fn pipe_name_is_per_user() {
        assert_eq!(pipe_name("S-1-5-21-1"), r"\\.\pipe\roadeep-mcp-S-1-5-21-1");
    }
}
