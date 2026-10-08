//! MCP over stdio: JSON-RPC 2.0, one message per line.
//!
//! Everything here is pure. `handle_line` answers what it can on the spot
//! (initialize, ping, tools/list, errors) and hands `tools/call` back to the
//! caller, which relays it to the app on a worker thread so a slow generation
//! never blocks a ping.

use serde_json::{json, Map, Value};

use crate::tools;
use crate::wire::{codes, Outcome, WireError};

/// Newest first. A client asking for one of these gets it echoed back; anything
/// else gets the newest, and the client decides whether it can live with that.
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// Longest stdin line accepted. Arguments are prompts and small JSON.
pub const MAX_LINE: usize = 4 << 20;

const INSTRUCTIONS: &str = "Roadeep tools served by the Roadeep desktop app, using the Roadeep account signed in there. \
The app must be running and signed in. Generation costs credits: call roadeep_estimate_generation, show the user the cost, \
and only then call roadeep_start_generation with the same body and the quote_id.";

#[derive(Debug, PartialEq)]
pub enum Incoming {
    /// Write this response.
    Reply(Value),
    /// A notification or a stray response: nothing to say.
    Ignore,
    /// Relay to the app, then answer `id` with `tool_result`.
    ToolCall { id: Value, name: String, arguments: Value },
}

pub fn handle_line(line: &[u8]) -> Incoming {
    let line = line.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(line);
    if line.iter().all(u8::is_ascii_whitespace) {
        return Incoming::Ignore;
    }
    if line.len() > MAX_LINE {
        return Incoming::Reply(error(Value::Null, INVALID_REQUEST, "Message too large"));
    }
    let message: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(_) => return Incoming::Reply(error(Value::Null, PARSE_ERROR, "Parse error")),
    };
    handle_message(message)
}

fn handle_message(message: Value) -> Incoming {
    let Some(obj) = message.as_object() else {
        // Batches were dropped from MCP in 2025-06-18; we never accepted them.
        return Incoming::Reply(error(Value::Null, INVALID_REQUEST, "Invalid Request"));
    };
    let id = obj.get("id").cloned();
    let valid_id = matches!(id, Some(Value::String(_)) | Some(Value::Number(_)));

    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response to a request we never sent, or garbage with an id.
        return match id {
            Some(id) if !obj.contains_key("result") && !obj.contains_key("error") => {
                Incoming::Reply(error(id, INVALID_REQUEST, "Invalid Request"))
            }
            _ => Incoming::Ignore,
        };
    };
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return match id {
            Some(id) => Incoming::Reply(error(id, INVALID_REQUEST, "Invalid Request: jsonrpc must be \"2.0\"")),
            None => Incoming::Ignore,
        };
    }

    // Notifications (no id) never get an answer, not even an error.
    let Some(id) = id else { return Incoming::Ignore };
    if !valid_id {
        return Incoming::Reply(error(Value::Null, INVALID_REQUEST, "Invalid Request: id must be a string or a number"));
    }
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Incoming::Reply(result(id, initialize_result(&params))),
        "ping" => Incoming::Reply(result(id, json!({}))),
        "tools/list" => Incoming::Reply(result(id, json!({ "tools": tools::definitions() }))),
        "tools/call" => tool_call(id, &params),
        _ => Incoming::Reply(error(id, METHOD_NOT_FOUND, &format!("Method not found: {method}"))),
    }
}

pub fn negotiate_version(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|r| SUPPORTED_VERSIONS.iter().find(|v| **v == r))
        .copied()
        .unwrap_or(SUPPORTED_VERSIONS[0])
}

fn initialize_result(params: &Value) -> Value {
    let version = negotiate_version(params.get("protocolVersion").and_then(Value::as_str));
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "roadeep",
            "title": "Roadeep desktop",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": INSTRUCTIONS
    })
}

fn tool_call(id: Value, params: &Value) -> Incoming {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Incoming::Reply(error(id, INVALID_PARAMS, "Invalid params: `name` is required"));
    };
    if !tools::is_known(name) {
        return Incoming::Reply(error(id, INVALID_PARAMS, &format!("Unknown tool: {name}")));
    }
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return Incoming::Reply(error(id, INVALID_PARAMS, "Invalid params: `arguments` must be an object")),
    };
    Incoming::ToolCall { id, name: name.to_string(), arguments }
}

/// A tool result. Roadeep failures are results with `isError: true`, not
/// protocol errors, so the model sees them and can react.
pub fn tool_result(id: Value, outcome: &Outcome) -> Value {
    let (text, is_error) = match outcome {
        Outcome::Ok(value) => (serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()), false),
        Outcome::Err(err) => (error_text(err), true),
    };
    result(id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
}

/// One readable paragraph plus the machine bits the model can act on.
pub fn error_text(err: &WireError) -> String {
    let mut text = match err.code.as_str() {
        codes::ROADEEP_NOT_RUNNING | codes::NOT_SIGNED_IN => err.message.clone(),
        _ => format!("Roadeep error {}: {}", err.code, err.message),
    };
    if let Some(after) = err.retry_after {
        text.push_str(&format!(" Retry after {after} s."));
    }
    if let Some(details) = err.details.as_ref().filter(|d| !d.is_empty()) {
        text.push_str(&format!("\nDetails: {}", Value::Object(details.clone())));
    }
    if let Some(status) = err.status {
        text.push_str(&format!("\n(HTTP {status}"));
        if let Some(rid) = &err.request_id {
            text.push_str(&format!(", request {rid}"));
        }
        text.push(')');
    }
    text
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error(id: Value, code: i64, message: &str) -> Value {
    let mut e = Map::new();
    e.insert("code".into(), json!(code));
    e.insert("message".into(), json!(message));
    json!({ "jsonrpc": "2.0", "id": id, "error": e })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(line: &str) -> Value {
        match handle_line(line.as_bytes()) {
            Incoming::Reply(v) => v,
            other => panic!("expected a reply to {line}, got {other:?}"),
        }
    }

    #[test]
    fn initialize_echoes_a_supported_version() {
        let r = reply(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#);
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "roadeep");
        assert!(r["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_offers_the_latest_for_an_unknown_version() {
        let r = reply(r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#);
        assert_eq!(r["id"], "a");
        assert_eq!(r["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);
        assert_eq!(negotiate_version(None), SUPPORTED_VERSIONS[0]);
    }

    #[test]
    fn tools_list_returns_every_tool() {
        let r = reply(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), tools::NAMES.len());
    }

    #[test]
    fn ping_is_answered_with_an_empty_result() {
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#)["result"], json!({}));
    }

    #[test]
    fn unknown_method_is_32601() {
        let r = reply(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#);
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(r["id"], 4);
    }

    #[test]
    fn malformed_json_is_32700_with_a_null_id() {
        let r = reply("{not json");
        assert_eq!(r["error"]["code"], PARSE_ERROR);
        assert_eq!(r["id"], Value::Null);
    }

    #[test]
    fn invalid_requests_are_32600() {
        assert_eq!(reply("[1,2]")["error"]["code"], INVALID_REQUEST);
        assert_eq!(reply(r#"{"id":5,"method":"ping"}"#)["error"]["code"], INVALID_REQUEST);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":{"x":1},"method":"ping"}"#)["error"]["code"], INVALID_REQUEST);
        assert_eq!(reply(r#"{"jsonrpc":"2.0","id":6}"#)["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        for line in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
            r#"{"jsonrpc":"2.0","method":"no/such/notification"}"#,
            r#"{"jsonrpc":"2.0","id":9,"result":{}}"#,
            "",
            "   ",
        ] {
            assert_eq!(handle_line(line.as_bytes()), Incoming::Ignore, "{line}");
        }
    }

    #[test]
    fn tool_calls_are_handed_back_for_relaying() {
        let line = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"roadeep_chat","arguments":{"message":"hi"}}}"#;
        match handle_line(line.as_bytes()) {
            Incoming::ToolCall { id, name, arguments } => {
                assert_eq!(id, 7);
                assert_eq!(name, "roadeep_chat");
                assert_eq!(arguments["message"], "hi");
            }
            other => panic!("{other:?}"),
        }
        // Arguments default to {}.
        let line = r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"roadeep_whoami"}}"#;
        assert!(matches!(handle_line(line.as_bytes()), Incoming::ToolCall { arguments, .. } if arguments == json!({})));
    }

    #[test]
    fn bad_tool_calls_are_32602() {
        for line in [
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rm_rf"}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"roadeep_chat","arguments":"hi"}}"#,
        ] {
            assert_eq!(reply(line)["error"]["code"], INVALID_PARAMS, "{line}");
        }
    }

    #[test]
    fn a_bom_is_tolerated() {
        let mut line = vec![0xEF, 0xBB, 0xBF];
        line.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        assert!(matches!(handle_line(&line), Incoming::Reply(v) if v["result"] == json!({})));
    }

    #[test]
    fn tool_results_carry_is_error() {
        let ok = tool_result(json!(1), &Outcome::Ok(json!({ "reply": "salam" })));
        assert_eq!(ok["result"]["isError"], false);
        assert!(ok["result"]["content"][0]["text"].as_str().unwrap().contains("salam"));

        let mut err = WireError::new("THROTTLED", "Too many requests.");
        err.status = Some(429);
        err.retry_after = Some(60);
        let bad = tool_result(json!(2), &Outcome::Err(err));
        assert_eq!(bad["result"]["isError"], true);
        let text = bad["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("THROTTLED") && text.contains("Retry after 60 s") && text.contains("HTTP 429"), "{text}");

        let closed = tool_result(json!(3), &Outcome::Err(WireError::new(codes::ROADEEP_NOT_RUNNING, crate::wire::SIGN_IN_HINT)));
        assert!(closed["result"]["content"][0]["text"].as_str().unwrap().starts_with("Open the Roadeep app"));
    }
}
