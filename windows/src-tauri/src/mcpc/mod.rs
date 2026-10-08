// MCP client ("mcpc"): the user's own MCP servers (GitHub, Notion, a local
// filesystem server…) whose tools the island chat may use through a
// client-side loop (roadeep/chat.rs). Separate from mcp/, which serves Roadeep
// itself AS an MCP server to the coding apps.
//
// One connection per server, opened lazily and reused; a per-server async
// mutex makes concurrent callers wait for the same handshake instead of racing
// it. Tool lists are cached per server (keyed by a fingerprint of its
// transport and auth) until the server says they changed. Idle local servers
// are stopped after 10 minutes. Every status change is broadcast as
// `mcpc-status` to both windows.

pub mod client;
pub mod directory;
mod http;
pub mod oauth;
mod rpc;
mod stdio;
pub mod store;
pub mod tools;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use client::{ServerInfo, Wire};
use store::{Auth, ServerConfig, ServerPatch, ServerSpec, Transport};
use tools::{RawTool, ToolView};

use crate::errors;
use crate::log;
use crate::planner;
use crate::secrets;

pub const STATUS_EVENT: &str = "mcpc-status";
/// How long a chat message waits for a server that isn't connected yet. The
/// connection carries on in the background, so a slow first start (npx
/// downloading a package) is ready for the next message.
const CHAT_WAIT: Duration = Duration::from_secs(15);
/// The settings window's "test connection" and tool list.
const TEST_WAIT: Duration = Duration::from_secs(90);
/// `initialize` of a local server: npx/uvx may download the package first.
const STDIO_INIT_TIMEOUT: Duration = Duration::from_secs(90);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// A tool call on a server that has to be (re)started first.
const CALL_CONNECT_WAIT: Duration = Duration::from_secs(30);
const IDLE_STOP: Duration = Duration::from_secs(10 * 60);
const REAP_EVERY: Duration = Duration::from_secs(60);
const MAX_SECRET_CHARS: usize = 16_384;

/// Whether the model may run a tool on its own, must ask the user first, or
/// never sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolMode {
    Auto,
    Ask,
    Off,
}

/// One tool offered to the model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub server_id: String,
    pub server_name: String,
    /// The server's own name for it (what `tools/call` takes).
    pub tool: String,
    /// `<slug>__<tool>`, `[A-Za-z0-9_-]{1,64}`, unique among the offered tools.
    pub qualified: String,
    /// One line, at most 300 characters, cleaned of control/bidi characters.
    pub description: String,
    /// Compacted: no `$schema`, examples or long descriptions.
    pub input_schema: Value,
    pub mode: ToolMode,
    pub read_only: bool,
    pub destructive: bool,
}

/// What a tool call gave back, as text for the model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutcome {
    pub is_error: bool,
    /// Text content joined, at most 16 000 characters (a cut is noted in the text).
    pub text: String,
    /// MIME types of content that isn't text (images, audio, binary resources).
    pub omitted: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Off,
    Idle,
    Connecting,
    Ready,
    NeedsAuth,
    NeedsApproval,
    Error,
}

#[derive(Clone, Serialize)]
struct StatusEvent {
    id: String,
    status: Status,
    error: Option<String>,
}

// ── Connections ───────────────────────────────────────────────────────────────

struct Live {
    wire: Wire,
    fingerprint: String,
}

#[derive(Default)]
struct SlotState {
    status: Option<(Status, Option<String>)>,
    /// (fingerprint, tools).
    tools: Option<(String, Vec<RawTool>)>,
    info: Option<ServerInfo>,
    last_used: Option<Instant>,
}

struct Slot {
    /// The per-server lock: held while connecting, so callers share one handshake.
    conn: tokio::sync::Mutex<Option<Arc<Live>>>,
    /// Interrupts a handshake in progress (disable, remove, edit).
    cancel: tokio::sync::Notify,
    /// The server said its tool list changed.
    dirty: Arc<AtomicBool>,
    state: Mutex<SlotState>,
}

static SLOTS: LazyLock<Mutex<HashMap<String, Arc<Slot>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn slot(id: &str) -> Arc<Slot> {
    SLOTS
        .lock()
        .unwrap()
        .entry(id.to_string())
        .or_insert_with(|| {
            Arc::new(Slot {
                conn: tokio::sync::Mutex::new(None),
                cancel: tokio::sync::Notify::new(),
                dirty: Arc::new(AtomicBool::new(false)),
                state: Mutex::new(SlotState::default()),
            })
        })
        .clone()
}

fn existing_slot(id: &str) -> Option<Arc<Slot>> {
    SLOTS.lock().unwrap().get(id).cloned()
}

/// What a connection and a tool cache depend on.
fn fingerprint(server: &ServerConfig) -> String {
    let encoded = serde_json::to_vec(&(&server.transport, &server.auth)).unwrap_or_default();
    Sha256::digest(&encoded).iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// The first line of a coded error: what goes to the log (stderr lines a
/// server printed after it stay out).
fn head(err: &str) -> &str {
    err.lines().next().unwrap_or("")
}

fn status_of(err: &str) -> Status {
    let code = head(err).split('|').next().unwrap_or("");
    if code == errors::MCPC_NEEDS_AUTH {
        Status::NeedsAuth
    } else if code == errors::MCPC_NEEDS_APPROVAL {
        Status::NeedsApproval
    } else if code == errors::MCPC_DISABLED {
        Status::Off
    } else {
        Status::Error
    }
}

fn set_status(app: &AppHandle, id: &str, status: Status, error: Option<String>) {
    let slot = slot(id);
    let changed = {
        let mut state = slot.state.lock().unwrap();
        let next = Some((status, error.clone()));
        let changed = state.status != next;
        state.status = next;
        changed
    };
    if changed {
        match &error {
            Some(e) => log::line(format!("mcpc: {id} → {status:?} ({})", head(e))),
            None => log::line(format!("mcpc: {id} → {status:?}")),
        }
        let _ = app.emit(STATUS_EVENT, StatusEvent { id: id.to_string(), status, error });
    }
}

/// Broadcasts the status a server has right now (after an edit).
fn emit_current(app: &AppHandle, server: &ServerConfig) {
    let (status, error) = current_status(server);
    let _ = app.emit(STATUS_EVENT, StatusEvent { id: server.id.clone(), status, error });
}

fn token_present(id: &str) -> bool {
    secrets::mcpc_get(id, "token").is_some()
}

fn oauth_present(id: &str) -> bool {
    secrets::mcpc_get(id, "oauth").is_some()
}

/// What must hold before we even try: approval for a local command, a token
/// or a sign-in for HTTP auth.
fn precheck(server: &ServerConfig) -> Result<(), String> {
    if !store::command_approved(server) {
        return Err(errors::coded(errors::MCPC_NEEDS_APPROVAL, &[]));
    }
    let missing = match server.auth {
        Auth::None => false,
        Auth::Bearer | Auth::Header { .. } => !token_present(&server.id),
        Auth::Oauth => !oauth_present(&server.id),
    };
    if missing {
        return Err(errors::coded(errors::MCPC_NEEDS_AUTH, &[]));
    }
    Ok(())
}

fn current_status(server: &ServerConfig) -> (Status, Option<String>) {
    if !server.enabled {
        return (Status::Off, None);
    }
    if let Err(e) = precheck(server) {
        return (status_of(&e), None);
    }
    existing_slot(&server.id)
        .and_then(|s| s.state.lock().unwrap().status.clone())
        .filter(|(s, _)| !matches!(s, Status::Off | Status::NeedsApproval))
        .unwrap_or((Status::Idle, None))
}

fn http_auth(app: &AppHandle, server: &ServerConfig) -> http::HttpAuth {
    match &server.auth {
        Auth::None => http::HttpAuth::None,
        Auth::Bearer => http::HttpAuth::Token { header: "Authorization".into(), bearer: true },
        Auth::Header { name } => http::HttpAuth::Token { header: name.clone(), bearer: false },
        Auth::Oauth => {
            let (app, id) = (app.clone(), server.id.clone());
            http::HttpAuth::OAuth(Arc::new(move || {
                let (app, id) = (app.clone(), id.clone());
                Box::pin(async move { oauth::bearer(&app, &id).await })
            }))
        }
    }
}

async fn open(app: &AppHandle, server: &ServerConfig, dirty: Arc<AtomicBool>) -> Result<Live, String> {
    let (wire, timeout) = match &server.transport {
        Transport::Stdio { command, args, env } => {
            let launch = stdio::plan_for(command, args)?;
            let values: Vec<(String, String)> =
                env.iter().filter_map(|name| secrets::mcpc_get(&server.id, &format!("env:{name}")).map(|v| (name.clone(), v))).collect();
            let (app2, id) = (app.clone(), server.id.clone());
            let on_exit: stdio::OnExit = Box::new(move |err| set_status(&app2, &id, Status::Error, Some(err)));
            (Wire::Stdio(Box::new(stdio::StdioWire::spawn(&launch, &values, dirty, on_exit)?)), STDIO_INIT_TIMEOUT)
        }
        Transport::Http { url } => (Wire::Http(http::HttpWire::new(&server.id, url, http_auth(app, server), dirty)?), HTTP_TIMEOUT),
    };
    let info = client::handshake(&wire, timeout).await?;
    log::line(format!("mcpc: {} connected to {} {}", server.id, info.name, info.version));
    slot(&server.id).state.lock().unwrap().info = Some(info);
    Ok(Live { wire, fingerprint: fingerprint(server) })
}

fn touch(slot: &Slot) {
    slot.state.lock().unwrap().last_used = Some(Instant::now());
}

/// The live connection, opening one if needed. Holds the server's lock while
/// it connects.
async fn connect(app: AppHandle, server: ServerConfig) -> Result<Arc<Live>, String> {
    let slot = slot(&server.id);
    let fp = fingerprint(&server);
    let mut guard = slot.conn.lock().await;
    if let Some(live) = guard.as_ref() {
        if live.wire.alive() && live.fingerprint == fp {
            touch(&slot);
            return Ok(live.clone());
        }
    }
    *guard = None;
    if let Err(e) = precheck(&server) {
        set_status(&app, &server.id, status_of(&e), None);
        return Err(e);
    }
    set_status(&app, &server.id, Status::Connecting, None);
    let opened = tokio::select! {
        r = open(&app, &server, slot.dirty.clone()) => r,
        _ = slot.cancel.notified() => Err(errors::coded(errors::MCPC_DISABLED, &[])),
    };
    match opened {
        Ok(live) => {
            let live = Arc::new(live);
            *guard = Some(live.clone());
            touch(&slot);
            set_status(&app, &server.id, Status::Ready, None);
            Ok(live)
        }
        Err(e) => {
            set_status(&app, &server.id, status_of(&e), Some(e.clone()));
            Err(e)
        }
    }
}

/// Runs `work` as its own task and waits at most `wait` for it: on a timeout
/// the work goes on in the background (a connection that is still starting).
async fn bounded<T: Send + 'static>(wait: Duration, work: impl std::future::Future<Output = Result<T, String>> + Send + 'static) -> Result<T, String> {
    let task = tauri::async_runtime::spawn(work);
    match tokio::time::timeout(wait, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(errors::coded(errors::MCPC_PROTOCOL, &[&e.to_string()])),
        Err(_) => Err(errors::coded(errors::MCPC_TIMEOUT, &[])),
    }
}

fn cached_tools(server: &ServerConfig) -> Option<Vec<RawTool>> {
    let slot = existing_slot(&server.id)?;
    if slot.dirty.load(Ordering::SeqCst) {
        return None;
    }
    let state = slot.state.lock().unwrap();
    let (fp, tools) = state.tools.as_ref()?;
    (*fp == fingerprint(server)).then(|| tools.clone())
}

/// A server's tools: from the cache, or by connecting and listing (at most
/// `wait`; the attempt finishes in the background).
async fn tools_for(app: &AppHandle, server: &ServerConfig, wait: Duration) -> Result<Vec<RawTool>, String> {
    precheck(server)?;
    if let Some(tools) = cached_tools(server) {
        return Ok(tools);
    }
    let (app, server) = (app.clone(), server.clone());
    bounded(wait, async move {
        let live = connect(app.clone(), server.clone()).await?;
        let slot = slot(&server.id);
        slot.dirty.store(false, Ordering::SeqCst);
        match client::list_tools(&live.wire, HTTP_TIMEOUT).await {
            Ok(tools) => {
                slot.state.lock().unwrap().tools = Some((live.fingerprint.clone(), tools.clone()));
                Ok(tools)
            }
            Err(e) => {
                set_status(&app, &server.id, status_of(&e), Some(e.clone()));
                Err(e)
            }
        }
    })
    .await
}

/// Stops a server's connection now, a handshake in progress included, and
/// forgets its tools and status.
async fn disconnect(id: &str) {
    let Some(slot) = existing_slot(id) else { return };
    slot.cancel.notify_waiters();
    let old = slot.conn.lock().await.take();
    if let Some(live) = old {
        if let Wire::Stdio(w) = &live.wire {
            w.stop();
        }
    }
    let mut state = slot.state.lock().unwrap();
    state.tools = None;
    state.status = None;
    state.info = None;
}

/// oauth.rs: the server was signed in or out. The connection is rebuilt with
/// the new tokens on next use.
pub fn auth_changed(app: &AppHandle, id: &str) {
    let (app, id) = (app.clone(), id.to_string());
    tauri::async_runtime::spawn(async move {
        disconnect(&id).await;
        if let Ok(server) = store::get(&id) {
            emit_current(&app, &server);
        }
    });
}

fn enabled_servers() -> Vec<ServerConfig> {
    store::load().into_iter().filter(|s| s.enabled).collect()
}

/// Cheap: whether the chat has any tool to offer. The planner's built-in
/// tools are always there, so this is true even with no server switched on.
pub fn any_enabled() -> bool {
    true
}

/// The built-in planner tools first (they must survive the preamble budget),
/// then the tools of the enabled servers, for the next chat message. Servers
/// that fail (or aren't ready within 15 s) are left out; their status says why.
pub async fn available_tools(app: &AppHandle) -> Vec<ToolSpec> {
    let servers = enabled_servers();
    let lists = futures_util::future::join_all(servers.iter().map(|s| tools_for(app, s, CHAT_WAIT))).await;
    let pairs: Vec<(ServerConfig, Vec<RawTool>)> = servers.into_iter().zip(lists).filter_map(|(s, r)| r.ok().map(|t| (s, t))).collect();
    let mut offered = planner::tools::specs();
    offered.extend(tools::offered(&pairs));
    offered
}

/// The server and the mode of one of its tools as they are NOW: the server
/// must still exist and be enabled, still list the tool (cache, or a fresh
/// `tools/list`), and the user must not have switched the tool off. Looked up
/// by the stable identity (server id + the server's own tool name), never by
/// the qualified name, whose suffixes shift as servers come and go.
async fn resolve(app: &AppHandle, server_id: &str, tool: &str) -> Result<(ServerConfig, ToolMode), String> {
    let server = store::get(server_id)?;
    if !server.enabled {
        return Err(errors::coded(errors::MCPC_DISABLED, &[]));
    }
    let listed = tools_for(app, &server, CALL_CONNECT_WAIT).await?;
    let mode = mode_now(&server, &listed, tool)?;
    Ok((server, mode))
}

/// The mode of `tool` given the server's current settings and tool list; Err
/// when it isn't listed any more or is switched off.
fn mode_now(server: &ServerConfig, listed: &[RawTool], tool: &str) -> Result<ToolMode, String> {
    let raw = listed.iter().find(|t| t.name == tool).ok_or_else(|| errors::coded(errors::MCPC_UNKNOWN_TOOL, &[&tools::one_line(tool, 80)]))?;
    match tools::mode_for(server, raw) {
        ToolMode::Off => Err(errors::coded(errors::MCPC_TOOL_OFF, &[&tools::one_line(tool, 80)])),
        mode => Ok(mode),
    }
}

/// A call may go ahead in `mode` only with the user's click when it asks.
fn may_run(mode: ToolMode, approved: bool, tool: &str) -> Result<(), String> {
    match mode {
        ToolMode::Auto => Ok(()),
        ToolMode::Ask if approved => Ok(()),
        ToolMode::Ask => Err(errors::coded(errors::MCPC_TOOL_ASK, &[&tools::one_line(tool, 80)])),
        ToolMode::Off => Err(errors::coded(errors::MCPC_TOOL_OFF, &[&tools::one_line(tool, 80)])),
    }
}

/// The mode a tool has right now (the user may have changed it since the turn
/// listed the tools). Errors are coded: gone, disabled, switched off.
pub async fn current_mode(app: &AppHandle, server_id: &str, tool: &str) -> Result<ToolMode, String> {
    if server_id == planner::tools::SERVER_ID {
        return planner::tools::mode(tool);
    }
    resolve(app, server_id, tool).await.map(|(_, mode)| mode)
}

/// Runs `tool` (the server's own name) on `server_id`. `approved`: the user
/// clicked Allow for exactly this call. The mode is checked again here, so a
/// tool switched to `ask` after the caller looked is refused with
/// MCPC_TOOL_ASK (the caller then asks), and one switched `off` is refused.
/// Errors are coded (errors.rs). Dropping the future cancels the call (the
/// server is told).
pub async fn call_tool(app: &AppHandle, server_id: &str, tool: &str, approved: bool, arguments: Value) -> Result<ToolOutcome, String> {
    let arguments = match arguments {
        Value::Null => Value::Object(Default::default()),
        Value::Object(_) => arguments,
        _ => return Err(errors::coded(errors::MCPC_INVALID, &["arguments"])),
    };
    // The planner's own tools: local, no server behind them.
    if server_id == planner::tools::SERVER_ID {
        return planner::tools::call(app, tool, &arguments);
    }
    let (server, mode) = resolve(app, server_id, tool).await?;
    if let Err(e) = may_run(mode, approved, tool) {
        log::line(format!("mcpc: {} {} not run ({})", server.id, tools::one_line(tool, 80), head(&e)));
        return Err(e);
    }
    let live = {
        let (app, server) = (app.clone(), server.clone());
        bounded(CALL_CONNECT_WAIT, connect(app, server)).await?
    };
    let slot = slot(&server.id);
    touch(&slot);
    log::line(format!("mcpc: {} runs {}", server.id, tools::one_line(tool, 80)));
    let started = Instant::now();
    let result = client::call(&live.wire, tool, arguments, CALL_TIMEOUT).await;
    touch(&slot);
    match &result {
        Ok(out) => log::line(format!("mcpc: {} {} done in {} ms{}", server.id, tools::one_line(tool, 80), started.elapsed().as_millis(), if out.is_error { " (tool error)" } else { "" })),
        Err(e) => log::line(format!("mcpc: {} {} failed: {}", server.id, tools::one_line(tool, 80), head(e))),
    }
    result
}

/// Stops local servers nobody used for 10 minutes. The next use starts them again.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(REAP_EVERY);
        loop {
            tick.tick().await;
            let slots: Vec<(String, Arc<Slot>)> = SLOTS.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            for (id, slot) in slots {
                let idle = slot.state.lock().unwrap().last_used.is_some_and(|t| t.elapsed() >= IDLE_STOP);
                if !idle {
                    continue;
                }
                let Ok(mut guard) = slot.conn.try_lock() else { continue };
                // strong_count 1: no call is using it right now.
                let stop = guard.as_ref().is_some_and(|live| live.wire.is_stdio() && Arc::strong_count(live) == 1);
                if stop {
                    *guard = None;
                    drop(guard);
                    log::line(format!("mcpc: {id} idle for 10 minutes, stopped"));
                    set_status(&app, &id, Status::Idle, None);
                }
            }
        }
    });
}

/// App exit: every local server goes now (the job objects would take them
/// with the process anyway).
pub fn shutdown() {
    let slots: Vec<Arc<Slot>> = SLOTS.lock().unwrap().values().cloned().collect();
    for slot in slots {
        if let Ok(mut guard) = slot.conn.try_lock() {
            if let Some(live) = guard.take() {
                if let Wire::Stdio(w) = &live.wire {
                    w.stop();
                }
            }
        }
    }
}

// ── Commands (the settings window) ────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvView {
    name: String,
    present: bool,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TransportView {
    Http { url: String },
    Stdio { command: String, args: Vec<String>, env: Vec<EnvView> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthView {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<String>,
    /// A token or OAuth tokens are stored.
    present: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    id: String,
    name: String,
    source: String,
    enabled: bool,
    transport: TransportView,
    auth: AuthView,
    status: Status,
    error: Option<String>,
    tool_count: usize,
    server_info: Option<ServerInfo>,
    /// stdio: the hash of the command shown (store::command_hash). The
    /// approval sends it back, so only what was on screen can be approved.
    command_hash: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResult {
    server_info: Option<ServerInfo>,
    tools: Vec<ToolView>,
}

fn view(server: &ServerConfig) -> ServerView {
    let transport = match &server.transport {
        Transport::Http { url } => TransportView::Http { url: url.clone() },
        Transport::Stdio { command, args, env } => TransportView::Stdio {
            command: command.clone(),
            args: args.clone(),
            env: env.iter().map(|n| EnvView { name: n.clone(), present: secrets::mcpc_get(&server.id, &format!("env:{n}")).is_some() }).collect(),
        },
    };
    let auth = match &server.auth {
        Auth::None => AuthView { kind: "none", header: None, present: false },
        Auth::Bearer => AuthView { kind: "bearer", header: None, present: token_present(&server.id) },
        Auth::Header { name } => AuthView { kind: "header", header: Some(name.clone()), present: token_present(&server.id) },
        Auth::Oauth => AuthView { kind: "oauth", header: None, present: oauth_present(&server.id) },
    };
    let (status, error) = current_status(server);
    let (tool_count, server_info) = existing_slot(&server.id)
        .map(|s| {
            let state = s.state.lock().unwrap();
            let fp = fingerprint(server);
            (state.tools.as_ref().filter(|(f, _)| *f == fp).map_or(0, |(_, t)| t.len()), state.info.clone())
        })
        .unwrap_or((0, None));
    ServerView {
        id: server.id.clone(),
        name: server.name.clone(),
        source: server.source.clone(),
        enabled: server.enabled,
        transport,
        auth,
        status,
        error,
        tool_count,
        server_info,
        command_hash: store::current_command_hash(server),
    }
}

#[tauri::command]
pub async fn mcpc_list() -> Result<Vec<ServerView>, String> {
    Ok(store::load_strict()?.iter().map(view).collect())
}

#[tauri::command]
pub async fn mcpc_add(app: AppHandle, spec: ServerSpec) -> Result<String, String> {
    let server = store::add(&spec)?;
    let kind = if matches!(server.transport, Transport::Stdio { .. }) { "stdio" } else { "http" };
    log::line(format!("mcpc: added {} ({kind}, {})", server.id, server.source));
    emit_current(&app, &server);
    Ok(server.id)
}

fn origin(transport: &Transport) -> Option<String> {
    match transport {
        Transport::Http { url } => reqwest::Url::parse(url).ok().map(|u| u.origin().ascii_serialization()),
        Transport::Stdio { .. } => None,
    }
}

fn env_names(transport: &Transport) -> &[String] {
    match transport {
        Transport::Stdio { env, .. } => env,
        Transport::Http { .. } => &[],
    }
}

#[tauri::command]
pub async fn mcpc_update(app: AppHandle, id: String, patch: ServerPatch) -> Result<(), String> {
    let (before, after) = store::update(&id, &patch)?;
    // Secrets that no longer have a use go. A token or sign-in made for one
    // origin must never be sent to another.
    for name in env_names(&before.transport) {
        if !env_names(&after.transport).contains(name) {
            secrets::mcpc_clear(&id, &format!("env:{name}"));
        }
    }
    let moved = origin(&before.transport) != origin(&after.transport);
    if moved || !matches!(after.auth, Auth::Bearer | Auth::Header { .. }) {
        secrets::mcpc_clear(&id, "token");
    }
    if moved || after.auth != Auth::Oauth {
        // Under oauth's epoch lock: a refresh in flight can't store tokens back.
        oauth::invalidate(&id, || secrets::mcpc_clear(&id, "oauth"));
        oauth::forget(&id);
    }
    if before.transport != after.transport || before.auth != after.auth || before.enabled != after.enabled {
        disconnect(&id).await;
    }
    emit_current(&app, &after);
    Ok(())
}

#[tauri::command]
pub async fn mcpc_remove(app: AppHandle, id: String) -> Result<(), String> {
    let server = store::remove(&id)?;
    disconnect(&id).await;
    for name in env_names(&server.transport) {
        secrets::mcpc_clear(&id, &format!("env:{name}"));
    }
    // Under oauth's epoch lock: a refresh in flight can't store tokens back.
    oauth::invalidate(&id, || secrets::mcpc_clear_all(&id));
    oauth::forget(&id);
    SLOTS.lock().unwrap().remove(&id);
    log::line(format!("mcpc: removed {id}"));
    let _ = app.emit(STATUS_EVENT, StatusEvent { id, status: Status::Off, error: None });
    Ok(())
}

/// The slots the settings window may write for this server: `token` for HTTP
/// token auth, `env:<NAME>` for a variable the stdio server declares. Never
/// `oauth` (oauth.rs owns it).
fn writable_slot(server: &ServerConfig, slot: &str) -> bool {
    match slot.strip_prefix("env:") {
        Some(name) => env_names(&server.transport).iter().any(|n| n == name),
        None => slot == "token" && matches!(server.auth, Auth::Bearer | Auth::Header { .. }),
    }
}

#[tauri::command]
pub async fn mcpc_set_secret(app: AppHandle, id: String, slot: String, value: String) -> Result<(), String> {
    let server = store::get(&id)?;
    if !writable_slot(&server, &slot) {
        return Err(errors::coded(errors::MCPC_INVALID, &["slot"]));
    }
    let value = value.trim();
    if value.chars().count() > MAX_SECRET_CHARS || value.chars().any(char::is_control) {
        return Err(errors::coded(errors::MCPC_INVALID, &["value"]));
    }
    secrets::mcpc_set(&id, &slot, value).map_err(|e| {
        log::line(format!("mcpc: could not store {slot} of {id}: {e}"));
        errors::coded(errors::MCPC_STORE, &[&e])
    })?;
    disconnect(&id).await;
    emit_current(&app, &server);
    Ok(())
}

#[tauri::command]
pub async fn mcpc_clear_secret(app: AppHandle, id: String, slot: String) -> Result<(), String> {
    let server = store::get(&id)?;
    if slot == "oauth" || !secrets::mcpc_slot_valid(&slot) {
        return Err(errors::coded(errors::MCPC_INVALID, &["slot"]));
    }
    secrets::mcpc_clear(&id, &slot);
    disconnect(&id).await;
    emit_current(&app, &server);
    Ok(())
}

#[tauri::command]
pub async fn mcpc_approve_command(app: AppHandle, id: String, hash: String) -> Result<(), String> {
    let server = store::approve(&id, &hash).inspect_err(|e| {
        if head(e) == errors::MCPC_COMMAND_CHANGED {
            log::line(format!("mcpc: approval of {id} refused: the command changed after it was shown"));
        }
    })?;
    log::line(format!("mcpc: the user approved the command of {id}"));
    disconnect(&id).await;
    emit_current(&app, &server);
    Ok(())
}

/// Lists a server's tools for the settings window; a switched-off server is
/// stopped again afterwards.
async fn tools_for_settings(app: &AppHandle, server: &ServerConfig) -> Result<Vec<RawTool>, String> {
    let result = tools_for(app, server, TEST_WAIT).await;
    if !server.enabled {
        // Keep the tool list, drop the process.
        let cache = existing_slot(&server.id).and_then(|s| s.state.lock().unwrap().tools.clone());
        disconnect(&server.id).await;
        if let (Some(slot), Some(cache)) = (existing_slot(&server.id), cache) {
            slot.state.lock().unwrap().tools = Some(cache);
        }
        emit_current(app, server);
    }
    if let Err(e) = &result {
        if !server.enabled {
            set_status(app, &server.id, status_of(e), Some(e.clone()));
        }
    }
    result
}

/// "Test connection": a fresh connection and tool list.
#[tauri::command]
pub async fn mcpc_connect(app: AppHandle, id: String) -> Result<ConnectResult, String> {
    let server = store::get(&id)?;
    disconnect(&id).await;
    let tools = tools_for_settings(&app, &server).await?;
    let server_info = existing_slot(&id).and_then(|s| s.state.lock().unwrap().info.clone());
    Ok(ConnectResult { server_info, tools: tools::views(&server, &tools) })
}

#[tauri::command]
pub async fn mcpc_tools(app: AppHandle, id: String) -> Result<Vec<ToolView>, String> {
    let server = store::get(&id)?;
    let tools = match cached_tools(&server) {
        Some(t) => t,
        None => tools_for_settings(&app, &server).await?,
    };
    Ok(tools::views(&server, &tools))
}

#[tauri::command]
pub async fn mcpc_set_tool_mode(id: String, tool: String, mode: ToolMode) -> Result<(), String> {
    store::set_tool_mode(&id, &tool, mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn server(transport: Transport, auth: Auth) -> ServerConfig {
        ServerConfig {
            id: "t-1".into(),
            name: "T".into(),
            source: "custom".into(),
            transport,
            auth,
            enabled: true,
            approved_command: None,
            tools: BTreeMap::new(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn statuses_come_from_the_error_code() {
        assert_eq!(status_of("E_MCPC_NEEDS_AUTH"), Status::NeedsAuth);
        assert_eq!(status_of("E_MCPC_NEEDS_APPROVAL"), Status::NeedsApproval);
        assert_eq!(status_of("E_MCPC_DISABLED"), Status::Off);
        assert_eq!(status_of("E_MCPC_EXITED\nE_MCPC_NEEDS_AUTH"), Status::Error);
        assert_eq!(status_of("E_MCPC_HTTP|500"), Status::Error);
        assert_eq!(serde_json::to_value(Status::NeedsApproval).unwrap(), "needs_approval");
        assert_eq!(head("E_MCPC_EXITED\nsecret stderr"), "E_MCPC_EXITED");
    }

    #[test]
    fn local_commands_need_approval_first() {
        let mut s = server(Transport::Stdio { command: "npx".into(), args: vec!["-y".into(), "x".into()], env: vec![] }, Auth::None);
        assert_eq!(precheck(&s).unwrap_err(), "E_MCPC_NEEDS_APPROVAL");
        assert_eq!(current_status(&s).0, Status::NeedsApproval);
        s.approved_command = Some(store::command_hash("npx", &["-y".into(), "x".into()], &[]));
        assert!(precheck(&s).is_ok());
        s.enabled = false;
        assert_eq!(current_status(&s), (Status::Off, None));
        let http = server(Transport::Http { url: "https://x.com/mcp".into() }, Auth::None);
        assert!(precheck(&http).is_ok());
        assert_eq!(current_status(&http).0, Status::Idle);
    }

    #[test]
    fn the_ui_may_write_only_the_slots_a_server_declares() {
        let stdio = server(Transport::Stdio { command: "npx".into(), args: vec![], env: vec!["API_KEY".into()] }, Auth::None);
        assert!(writable_slot(&stdio, "env:API_KEY"));
        assert!(!writable_slot(&stdio, "env:PATH"));
        assert!(!writable_slot(&stdio, "token"));
        assert!(!writable_slot(&stdio, "oauth"));
        let bearer = server(Transport::Http { url: "https://x.com/mcp".into() }, Auth::Bearer);
        assert!(writable_slot(&bearer, "token"));
        assert!(!writable_slot(&bearer, "oauth"));
        let oauth = server(Transport::Http { url: "https://x.com/mcp".into() }, Auth::Oauth);
        assert!(!writable_slot(&oauth, "token") && !writable_slot(&oauth, "oauth"));
    }

    #[test]
    fn calls_check_the_mode_the_tool_has_now() {
        let raw = |name: &str, read_only: bool| RawTool {
            name: name.into(),
            title: None,
            description: String::new(),
            input_schema: serde_json::json!({}),
            read_only,
            destructive: false,
            open_world: false,
        };
        let listed = vec![raw("search", true), raw("create_issue", false)];
        let mut s = server(Transport::Http { url: "https://x.com/mcp".into() }, Auth::None);
        assert_eq!(mode_now(&s, &listed, "search"), Ok(ToolMode::Auto));
        assert_eq!(mode_now(&s, &listed, "create_issue"), Ok(ToolMode::Ask));
        assert_eq!(mode_now(&s, &listed, "gone").unwrap_err(), "E_MCPC_UNKNOWN_TOOL|gone");
        // The user changes their mind mid-turn: the setting of now wins.
        s.tools.insert("search".into(), ToolMode::Ask);
        assert_eq!(mode_now(&s, &listed, "search"), Ok(ToolMode::Ask));
        s.tools.insert("search".into(), ToolMode::Off);
        assert_eq!(mode_now(&s, &listed, "search").unwrap_err(), "E_MCPC_TOOL_OFF|search");

        assert!(may_run(ToolMode::Auto, false, "t").is_ok());
        assert!(may_run(ToolMode::Ask, true, "t").is_ok());
        assert_eq!(may_run(ToolMode::Ask, false, "t").unwrap_err(), "E_MCPC_TOOL_ASK|t");
        assert_eq!(may_run(ToolMode::Off, true, "t").unwrap_err(), "E_MCPC_TOOL_OFF|t");
    }

    #[test]
    fn fingerprints_follow_transport_and_auth_only() {
        let a = server(Transport::Http { url: "https://x.com/mcp".into() }, Auth::None);
        let mut b = a.clone();
        b.name = "Other".into();
        b.tools.insert("x".into(), ToolMode::Off);
        assert_eq!(fingerprint(&a), fingerprint(&b));
        b.auth = Auth::Bearer;
        assert_ne!(fingerprint(&a), fingerprint(&b));
        assert_eq!(origin(&a.transport).as_deref(), Some("https://x.com"));
    }
}
