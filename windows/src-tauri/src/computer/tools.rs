//! Computer tools join the existing chat approval loop; the agent binding is server-owned.
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use crate::mcpc::{ToolMode, ToolOutcome, ToolSpec};
use super::{ComputerState, error};
pub const SERVER_ID: &str = "builtin:computer";
const TOOLS: &[(&str,&str,bool)] = &[
    ("status","Read the explicitly started agent computer status.",true),
    ("navigate","Navigate its isolated browser to a public HTTP(S) URL. Returns visible text and interactive target coordinates.",false),
    ("screenshot","Capture the browser viewport. User sees the image; tool receives bounded visible text and target coordinates.",true),
    ("click","Click a browser viewport coordinate (1024 by 640).",false),
    ("type","Type text into the focused browser control.",false),
    ("key","Press an allowed browser keyboard key.",false),
    ("scroll","Scroll the isolated browser vertically.",false),
    ("list","List relative files within this computer's ephemeral workspace.",true),
    ("read","Read a bounded UTF-8 text file within this computer's workspace.",true),
    ("write","Write a bounded UTF-8 text file within this computer's workspace.",false),
    ("terminal","Run a shell command INSIDE this networkless container, never on the host. Deadline 15 seconds.",false),
];
pub fn mode(tool: &str) -> Result<ToolMode,String> {
    TOOLS.iter().find(|(name,..)| *name == tool).map(|(_,_,read)| if *read {ToolMode::Auto} else {ToolMode::Ask}).ok_or_else(|| error("computer-unknown-tool"))
}
pub fn specs() -> Vec<ToolSpec> {
    TOOLS.iter().map(|(name,description,read)| {
        let mut properties = serde_json::Map::new(); let mut required = Vec::new();
        for (key,kind) in match *name {
            "navigate" => vec![("url","string")], "click" => vec![("x","integer"),("y","integer")], "type" => vec![("text","string")],
            "key" => vec![("key","string")], "scroll" => vec![("deltaY","integer")], "list" | "read" => vec![("path","string")],
            "write" => vec![("path","string"),("text","string")], "terminal" => vec![("command","string")], _ => vec![],
        } { properties.insert(key.into(),json!({"type":kind})); if *name != "list" { required.push(key); } }
        ToolSpec { server_id:SERVER_ID.into(), server_name:"Agent computer".into(), tool:(*name).into(), qualified:format!("computer__{name}"), description:(*description).into(), input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),mode:if *read {ToolMode::Auto} else {ToolMode::Ask},read_only:*read,destructive:false }
    }).collect()
}
pub async fn call(app:&AppHandle,agent:&str,tool:&str,approved:bool,arguments:Value) -> Result<ToolOutcome,String> {
    if mode(tool)? == ToolMode::Ask && !approved { return Err(crate::errors::coded(crate::errors::MCPC_TOOL_ASK,&[])); }
    let mut args = arguments.as_object().cloned().ok_or_else(|| error("computer-invalid-operation"))?;
    if args.contains_key("action") || args.contains_key("agentId") || args.contains_key("agent_id") { return Err(error("computer-invalid-operation")); }
    args.insert("action".into(),json!(tool));
    let state = app.state::<ComputerState>();
    let result = state.operate(agent,Value::Object(args),false).await?;
    if let Some(image) = result.image {
        app.emit("computer-snapshot",json!({"agentId":agent,"image":image,"data":result.data})).map_err(|_| error("computer-snapshot-delivery"))?;
    }
    let text = serde_json::to_string(&result.data).map_err(|_| error("computer-runtime-protocol"))?;
    // Keep existing tool-result budgets even when a read returned a larger UI payload.
    let text: String = if text.chars().count() > 14_000 { format!("{}\n[truncated]",text.chars().take(14_000).collect::<String>()) } else { text };
    Ok(ToolOutcome {is_error:tool == "terminal" && result.data["exitCode"].as_i64().is_none_or(|code| code != 0),text,omitted:vec![]})
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn effects_always_ask_and_identity_never_in_schema() {
        for spec in specs() {
            assert_eq!(spec.mode,if ["status","screenshot","list","read"].contains(&spec.tool.as_str()) {ToolMode::Auto} else {ToolMode::Ask});
            assert!(!spec.input_schema.to_string().contains("agentId")); assert!(!spec.input_schema.to_string().contains("container"));
        }
        assert!(mode("start").is_err()); assert!(mode("host_exec").is_err());
    }
}
