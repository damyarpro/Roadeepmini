// The MCP conversation itself, the same over both transports: initialize →
// notifications/initialized → tools/list (following nextCursor) → tools/call.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use super::http::HttpWire;
use super::stdio::StdioWire;
use super::tools::{self, one_line, RawTool};
use super::ToolOutcome;
use crate::errors;

/// More than any real server has; a server that keeps paging is cut here.
const MAX_TOOLS: usize = 300;
const MAX_PAGES: usize = 20;

pub enum Wire {
    Stdio(Box<StdioWire>),
    Http(Arc<HttpWire>),
}

impl Wire {
    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        match self {
            Wire::Stdio(w) => w.request(method, params, timeout).await,
            Wire::Http(w) => w.request(method, params, timeout).await,
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        match self {
            Wire::Stdio(w) => w.notify(method, params).await,
            Wire::Http(w) => w.notify(method, params).await,
        }
    }

    pub fn alive(&self) -> bool {
        match self {
            Wire::Stdio(w) => w.alive(),
            Wire::Http(w) => w.alive(),
        }
    }

    pub fn is_stdio(&self) -> bool {
        matches!(self, Wire::Stdio(_))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

pub async fn handshake(wire: &Wire, timeout: Duration) -> Result<ServerInfo, String> {
    let result = wire.request("initialize", super::rpc::initialize_params(), timeout).await?;
    let version = result.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    // Any dated version is fine: tools/list and tools/call haven't changed.
    if version.is_empty() || version.len() > 32 {
        return Err(errors::coded(errors::MCPC_PROTOCOL, &["no protocol version"]));
    }
    let info = result.get("serverInfo");
    let text = |key: &str| one_line(info.and_then(|i| i.get(key)).and_then(Value::as_str).unwrap_or(""), 80);
    wire.notify("notifications/initialized", None).await?;
    Ok(ServerInfo { name: text("name"), version: text("version") })
}

pub async fn list_tools(wire: &Wire, timeout: Duration) -> Result<Vec<RawTool>, String> {
    let mut out: Vec<RawTool> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let params = match &cursor {
            Some(c) => json!({ "cursor": c }),
            None => json!({}),
        };
        let page = wire.request("tools/list", params, timeout).await?;
        let Some(items) = page.get("tools").and_then(Value::as_array) else {
            return Err(errors::coded(errors::MCPC_PROTOCOL, &["tools/list without tools"]));
        };
        for item in items {
            if out.len() >= MAX_TOOLS {
                return Ok(out);
            }
            if let Some(tool) = tools::parse_tool(item) {
                if !out.iter().any(|t| t.name == tool.name) {
                    out.push(tool);
                }
            }
        }
        cursor = page.get("nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty() && c.len() <= 1024).map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    Ok(out)
}

pub async fn call(wire: &Wire, tool: &str, arguments: Value, timeout: Duration) -> Result<ToolOutcome, String> {
    let result = wire.request("tools/call", json!({ "name": tool, "arguments": arguments }), timeout).await?;
    Ok(tools::outcome(&result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// The whole conversation against a scripted server on in-memory pipes.
    #[tokio::test(flavor = "current_thread")]
    async fn handshake_paged_listing_and_a_call() {
        let (client_in, server_in) = tokio::io::duplex(64 * 1024);
        let (mut server_out, client_out) = tokio::io::duplex(64 * 1024);
        let wire = Wire::Stdio(Box::new(StdioWire::on_pipes(client_in, client_out, Arc::new(AtomicBool::new(false)), Box::new(|_| {}))));
        let server = tokio::spawn(async move {
            let mut lines = BufReader::new(server_in).lines();
            let mut seen = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                let msg: Value = serde_json::from_str(&line).unwrap();
                let method = msg["method"].as_str().unwrap_or("").to_string();
                seen.push(method.clone());
                let result = match method.as_str() {
                    "initialize" => json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": { "name": "Demo\u{202E}", "version": "2.0" } }),
                    "tools/list" if msg["params"]["cursor"].is_null() => json!({ "tools": [{ "name": "b" }, { "name": "" }], "nextCursor": "p2" }),
                    "tools/list" => json!({ "tools": [{ "name": "a", "annotations": { "readOnlyHint": true } }, { "name": "b" }] }),
                    "tools/call" => json!({ "content": [{ "type": "text", "text": format!("hi {}", msg["params"]["arguments"]["who"]) }] }),
                    _ => continue,
                };
                let reply = json!({ "jsonrpc": "2.0", "id": msg["id"], "result": result });
                server_out.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                if method == "tools/call" {
                    break;
                }
            }
            seen
        });
        let t = Duration::from_secs(5);
        let info = handshake(&wire, t).await.unwrap();
        assert_eq!(info, ServerInfo { name: "Demo".into(), version: "2.0".into() });
        let tools = list_tools(&wire, t).await.unwrap();
        assert_eq!(tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(tools[1].read_only);
        let out = call(&wire, "a", json!({ "who": "you" }), t).await.unwrap();
        assert_eq!(out.text, "hi \"you\"");
        assert_eq!(server.await.unwrap(), ["initialize", "notifications/initialized", "tools/list", "tools/list", "tools/call"]);
    }
}
