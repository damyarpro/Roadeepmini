//! The app's own tool registry (MCP servers, desktop actions, the agent computer) offered to a
//! live voice session, and the island-only dispatcher that runs them through the chat's paths.
use crate::mcpc::{self, ToolMode, ToolSpec};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Total function tools in one live session (tools.json + app tools).
pub const MAX_SESSION_TOOLS: usize = 60;
const MAX_NAME: usize = 64;
const PREFIX: &str = "app__";
const MAX_DESCRIPTION: usize = 400;
const MAX_TITLE: usize = 80;
pub const MAX_ARGUMENTS: usize = 16 * 1024;
pub const MAX_OUTPUT: usize = 6000;
/// Voice turns have no chat agent; they use the default agent computer, like an agent-less chat.
pub const COMPUTER_AGENT: &str = "default";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTool {
    pub name: String,
    pub title: String,
    pub server: String,
    /// Runs without the voice approval card: read-only AND the user's mode is `auto`.
    pub read_only: bool,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub server_id: String,
    pub tool: String,
    pub meta: AppTool,
}
pub type Registry = HashMap<String, Entry>;

/// The MCP tool list is fetched in the background (app start, then whenever it is stale or a
/// server changed) so voice_start never waits on a server. A stale list is still safe to offer:
/// every call re-checks the server, the tool and its current mode (mcpc::call_tool).
const MCP_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
#[derive(Default)]
struct McpCache {
    at: Option<std::time::Instant>,
    specs: Vec<ToolSpec>,
    dirty: bool,
}
impl McpCache {
    /// The cached list and whether a background refresh is due.
    fn read(&self, now: std::time::Instant) -> (Vec<ToolSpec>, bool) {
        let stale = self.dirty || self.at.is_none_or(|at| now.duration_since(at) >= MCP_TTL);
        (self.specs.clone(), stale)
    }
    fn store(&mut self, specs: Vec<ToolSpec>, now: std::time::Instant) {
        *self = Self { at: Some(now), specs, dirty: false };
    }
}
static MCP: std::sync::LazyLock<std::sync::Mutex<McpCache>> = std::sync::LazyLock::new(Default::default);
static REFRESHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn refresh_in_background(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if REFRESHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        let specs = if mcpc::any_enabled() { mcpc::available_tools(&app).await } else { Vec::new() };
        let count = specs.len();
        if let Ok(mut cache) = MCP.lock() {
            cache.store(specs, std::time::Instant::now());
        }
        REFRESHING.store(false, Ordering::SeqCst);
        crate::log::line(format!("voice: app tools prefetched ({count} MCP tools, {} ms)", started.elapsed().as_millis()));
    });
}
/// App start: fetch once, and mark the list stale whenever an MCP server's status changes.
pub fn start_prefetch(app: &tauri::AppHandle) {
    use tauri::Listener;
    refresh_in_background(app);
    // Warms the online clock too, so the first voice_start already has it.
    crate::clock::now_cached(app);
    app.listen(mcpc::STATUS_EVENT, |_| {
        if let Ok(mut cache) = MCP.lock() {
            cache.dirty = true;
        }
    });
}
/// Everything chat could offer, minus the planner (tools.json covers it) and tools switched off.
/// Never waits: the MCP part comes from the prefetch cache (refreshed in the background).
pub fn collect(app: &tauri::AppHandle) -> Vec<ToolSpec> {
    use tauri::Manager;
    let mut specs = Vec::new();
    if app
        .state::<crate::computer::ComputerState>()
        .agent_running(COMPUTER_AGENT)
    {
        specs.extend(crate::computer::tools::specs());
    }
    specs.extend(crate::desktop_actions::specs());
    let (cached, stale) = MCP.lock().map(|c| c.read(std::time::Instant::now())).unwrap_or_default();
    if stale {
        refresh_in_background(app);
    }
    specs.extend(cached);
    specs
}
fn clean_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}
fn fit(base: &str, suffix: &str) -> String {
    let room = MAX_NAME - PREFIX.len() - suffix.len();
    format!("{PREFIX}{}{suffix}", &base[..base.len().min(room)])
}
/// Deterministic, unique, OpenAI-safe names; read-only tools are kept first when the cap cuts.
/// Returns the registry (in offer order) and the qualified names that were cut.
pub fn select(specs: Vec<ToolSpec>, room: usize) -> (Vec<Entry>, Vec<(Value, String)>, Vec<String>) {
    let mut specs: Vec<ToolSpec> = specs
        .into_iter()
        .filter(|s| s.server_id != crate::planner::tools::SERVER_ID && s.mode != ToolMode::Off)
        .collect();
    specs.sort_by_key(|s| !(s.read_only && s.mode == ToolMode::Auto));
    let cut = specs.split_off(specs.len().min(room)).into_iter().map(|s| s.qualified).collect();
    let mut names = HashSet::new();
    let mut entries = Vec::new();
    let mut functions = Vec::new();
    for spec in specs {
        let base = clean_name(&spec.qualified);
        let mut name = fit(&base, "");
        let mut n = 2;
        while names.contains(&name) {
            name = fit(&base, &format!("_{n}"));
            n += 1;
        }
        names.insert(name.clone());
        let server = mcpc::tools::one_line(&spec.server_name, MAX_TITLE);
        let title = mcpc::tools::one_line(&spec.tool, MAX_TITLE);
        let description: String = mcpc::tools::one_line(
            &format!("[{server}] {}", spec.description),
            MAX_DESCRIPTION,
        );
        let mut parameters = mcpc::tools::compact_schema(&spec.input_schema);
        if parameters.get("type").and_then(Value::as_str) != Some("object") {
            parameters = serde_json::json!({"type": "object", "properties": {}});
        }
        functions.push((parameters, description));
        entries.push(Entry {
            server_id: spec.server_id,
            tool: spec.tool,
            meta: AppTool {
                name,
                title,
                server,
                read_only: spec.read_only && spec.mode == ToolMode::Auto && !spec.destructive,
            },
        });
    }
    (entries, functions, cut)
}
pub fn function(entry: &Entry, parameters: Value, description: String) -> Value {
    serde_json::json!({
        "type": "function",
        "name": entry.meta.name,
        "description": description,
        "parameters": parameters,
    })
}
/// The checks that need no app: argument shape and the approval rule.
pub fn admit(entry: &Entry, arguments: &Value, approved: bool) -> Result<(), String> {
    if !arguments.is_object() {
        return Err("Invalid tool arguments".into());
    }
    if serde_json::to_string(arguments).map(|s| s.len()).unwrap_or(usize::MAX) > MAX_ARGUMENTS {
        return Err("Tool arguments too large".into());
    }
    if !entry.meta.read_only {
        if !approved {
            return Err("This action requires the user's approval".into());
        }
        // Same rule as chat: an action is only approved when its input can be shown in full.
        if crate::roadeep::chat::tools::approval_arguments(arguments).is_none() {
            return Err("Tool arguments too large to approve".into());
        }
    }
    Ok(())
}
fn clamp(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.into();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}
/// The code of a coded error; its arguments may hold a host or a path and stay out.
fn code_of(coded: &str) -> &str {
    coded.split(['|', '\n']).next().unwrap_or("").trim()
}
pub async fn dispatch(
    app: &tauri::AppHandle,
    entry: &Entry,
    approved: bool,
    arguments: Value,
) -> Result<String, String> {
    let outcome = if entry.server_id == crate::desktop_actions::SERVER_ID {
        crate::desktop_actions::call(&entry.tool, arguments)
    } else if entry.server_id == crate::computer::tools::SERVER_ID {
        crate::computer::tools::call(app, COMPUTER_AGENT, &entry.tool, approved, arguments).await
    } else {
        mcpc::call_tool(app, &entry.server_id, &entry.tool, approved, arguments).await
    };
    match outcome {
        Ok(out) => {
            let mut text = out.text;
            if !out.omitted.is_empty() {
                text.push_str(&format!("\n[{} non-text item(s) not shown]", out.omitted.len()));
            }
            if out.is_error {
                text = format!("Tool error: {text}");
            }
            Ok(clamp(&text, MAX_OUTPUT))
        }
        Err(coded) => Err(format!("The tool could not run (error {})", clamp(code_of(&coded), 80))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec(server_id: &str, qualified: &str, mode: ToolMode, read_only: bool) -> ToolSpec {
        ToolSpec {
            server_id: server_id.into(),
            server_name: "Server".into(),
            tool: qualified.rsplit("__").next().unwrap().into(),
            qualified: qualified.into(),
            description: "d".repeat(900),
            input_schema: serde_json::json!({"type":"object","properties":{"q":{"type":"string"}}}),
            mode,
            read_only,
            destructive: false,
        }
    }
    #[test]
    fn names_are_safe_unique_bounded_and_deterministic() {
        let long = format!("srv__{}", "x".repeat(80));
        let specs = || {
            vec![
                spec("s1", "srv__tool.name/ü", ToolMode::Auto, true),
                spec("s1", "srv__tool_name__", ToolMode::Auto, true),
                spec("s2", &long, ToolMode::Ask, false),
                spec("s3", &long, ToolMode::Ask, false),
            ]
        };
        let (a, functions, cut) = select(specs(), 60);
        let (b, ..) = select(specs(), 60);
        assert!(cut.is_empty());
        let names: Vec<&str> = a.iter().map(|e| e.meta.name.as_str()).collect();
        assert_eq!(names, b.iter().map(|e| e.meta.name.as_str()).collect::<Vec<_>>());
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len());
        for name in &names {
            assert!(name.starts_with("app__") && name.len() <= 64, "{name}");
            assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'), "{name}");
        }
        for (params, description) in &functions {
            assert_eq!(params["type"], "object");
            assert!(description.chars().count() <= MAX_DESCRIPTION);
        }
    }
    #[test]
    fn off_and_planner_are_excluded_and_cap_prefers_read_only() {
        let mut specs = crate::planner::tools::specs();
        specs.push(spec("s1", "srv__off", ToolMode::Off, true));
        for i in 0..50 {
            specs.push(spec("s1", &format!("srv__write{i}"), ToolMode::Ask, false));
        }
        for i in 0..50 {
            specs.push(spec("s1", &format!("srv__read{i}"), ToolMode::Auto, true));
        }
        let (entries, functions, cut) = select(specs, 41);
        assert_eq!(entries.len(), 41);
        assert_eq!(functions.len(), 41);
        assert_eq!(cut.len(), 59);
        assert!(entries.iter().all(|e| e.server_id == "s1" && e.tool != "off"));
        assert_eq!(entries.iter().filter(|e| e.meta.read_only).count(), 41);
        let (entries, ..) = select(crate::desktop_actions::specs(), 60);
        assert_eq!(entries[0].meta.name, "app__desktop__open");
        assert!(!entries[0].meta.read_only);
    }
    #[test]
    fn mutating_tools_need_approval_and_arguments_are_bounded() {
        let (entries, ..) = select(
            vec![
                spec("s1", "srv__read", ToolMode::Auto, true),
                spec("s1", "srv__ask_read", ToolMode::Ask, true),
                spec("s1", "srv__write", ToolMode::Auto, false),
            ],
            60,
        );
        let by = |t: &str| entries.iter().find(|e| e.tool == t).unwrap();
        let args = serde_json::json!({"q": "x"});
        assert!(admit(by("read"), &args, false).is_ok());
        assert!(admit(by("ask_read"), &args, false).is_err());
        assert!(admit(by("write"), &args, false).is_err());
        assert!(admit(by("write"), &args, true).is_ok());
        assert!(admit(by("read"), &serde_json::json!(["x"]), true).is_err());
        let big = serde_json::json!({"q": "x".repeat(MAX_ARGUMENTS)});
        assert!(admit(by("read"), &big, true).is_err());
        // Fits the 16 KB transport limit but not the chat approval card once hidden characters
        // are spelled out: read-only still runs, an approved action is refused.
        let hidden = serde_json::json!({"q": "\u{202E}".repeat(3500)});
        assert!(serde_json::to_string(&hidden).unwrap().len() <= MAX_ARGUMENTS);
        assert!(admit(by("read"), &hidden, false).is_ok());
        assert_eq!(
            admit(by("write"), &hidden, true).unwrap_err(),
            "Tool arguments too large to approve"
        );
    }
    #[test]
    fn prefetch_cache_is_used_and_marked_stale_by_age_or_change() {
        let t0 = std::time::Instant::now();
        let mut cache = McpCache::default();
        assert!(cache.read(t0).1, "nothing fetched yet");
        cache.store(vec![spec("s1", "srv__read", ToolMode::Auto, true)], t0);
        let (specs, stale) = cache.read(t0 + std::time::Duration::from_secs(60));
        assert_eq!((specs.len(), stale), (1, false));
        let (specs, stale) = cache.read(t0 + MCP_TTL);
        assert_eq!((specs.len(), stale), (1, true), "stale lists are still offered while refreshing");
        cache.store(specs, t0);
        cache.dirty = true;
        assert!(cache.read(t0).1);
    }
    #[test]
    fn coded_error_arguments_never_leave() {
        assert_eq!(code_of("MCPC_FAILED|https://secret.host/path\nstderr"), "MCPC_FAILED");
        assert_eq!(clamp(&"a".repeat(7000), MAX_OUTPUT).chars().count(), MAX_OUTPUT);
    }
}
