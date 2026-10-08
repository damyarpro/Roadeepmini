// Chat over Roadeep: one thread per island conversation, files uploaded as
// attachments, replies produced by an async job that is followed live over the
// WebSocket (ws.rs) — or polled, when the socket is not there — until it ends.
// A job that pauses for a tool approval waits for the user's decision and then
// follows the resumed job. Also the model and agent catalogues for the
// settings window.
//
// Like the old Claude client, everything happens on the Rust side: file bytes
// and tokens never cross the IPC boundary.
//
// With MCP servers connected (crate::mcpc), a turn can also use their tools
// through a client-side loop: the protocol is in chat/tools.rs, the loop in
// `tool_loop` below. Each tool step is one more message in the same thread.

pub mod tools;

use std::path::{Path, PathBuf};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::{broadcast, Notify};
use tauri::Manager;

use super::generation::valid_id as valid_server_id;
use super::http::{codes, Call, Payload, RoadeepError, Upload};
use super::ws::{self, OpenError, Signal, TurnTracker, WsMsg};
use super::{threads, Balance, Roadeep, BALANCE_MAX_AGE};
use crate::agents::{self, LocalAgent};
use crate::errors;
use crate::mcpc;
use crate::settings::Settings;
use crate::{files, log};
use tools::{Parsed, ToolEntry, ToolStepView};

/// Uploads above this are refused before any byte leaves the machine.
pub const MAX_UPLOAD_BYTES: u64 = 20 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 32_000;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_START: Duration = Duration::from_millis(700);
const POLL_MAX: Duration = Duration::from_secs(2);
/// A job that shows no progress (no frame, no status change) for this long ends.
const JOB_TIMEOUT: Duration = Duration::from_secs(180);
/// However much it progresses, one job never runs longer than this.
const JOB_CAP: Duration = Duration::from_secs(15 * 60);
/// Deep research keeps its own, longer cap.
const DEEP_RESEARCH_CAP: Duration = Duration::from_secs(30 * 60);
/// Deep research legitimately runs for minutes.
const DEEP_RESEARCH_TIMEOUT: Duration = Duration::from_secs(600);
/// Transient poll failures tolerated in a row before giving up on the job.
const POLL_FAILURE_TOLERANCE: u32 = 2;
/// Streamed text reaches the island at most this often (the full text each time).
const DELTA_EVERY: Duration = Duration::from_millis(60);
/// While a job waits for a human, its status is read this often (a decision
/// made in another client, a cancel).
const APPROVAL_CHECK: Duration = Duration::from_secs(10);
/// An approval nobody answers ends the turn after this.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const APPROVAL_SUMMARY_CHARS: usize = 240;

/// Emitted to the island while a turn runs; see `StreamEvent`.
pub const STREAM_EVENT: &str = "chat-stream";

/// Roadeep has no system prompt, so a Persian reply is asked for in the first
/// message of a thread; the thread keeps answering in that language.
pub const PERSIAN_SUFFIX: &str = "\n\n(لطفاً به فارسی پاسخ بده.)";

pub mod chat_codes {
    pub const CANCELLED: &str = "CANCELLED";
    pub const AWAITING_APPROVAL: &str = "AWAITING_APPROVAL";
    pub const JOB_TIMEOUT: &str = "JOB_TIMEOUT";
    pub const EMPTY_RESPONSE: &str = "EMPTY_RESPONSE";
    pub const LLM_ERROR: &str = "LLM_ERROR";
    pub const FILE_TOO_LARGE: &str = "FILE_TOO_LARGE";
    pub const FILE_UNREADABLE: &str = "FILE_UNREADABLE";
    pub const UPLOAD_FAILED: &str = "UPLOAD_FAILED";
    pub const APPROVAL_ALREADY_DECIDED: &str = "APPROVAL_ALREADY_DECIDED";
    pub const APPROVAL_NOT_FOUND: &str = "APPROVAL_NOT_FOUND";
    /// A turn is already in flight; one at a time.
    pub const CHAT_BUSY: &str = "CHAT_BUSY";
    /// The server read a local agent's instructions as a "save to memory"
    /// request and answered with its canned memory reply instead of the model.
    pub const AGENT_MEMORY_TRIGGER: &str = "AGENT_MEMORY_TRIGGER";
    /// A tool result read by the server as a "save to memory" request: the
    /// model never saw it.
    pub const TOOL_MEMORY_TRIGGER: &str = "TOOL_MEMORY_TRIGGER";
    /// The model kept calling tools after the step limit and wrote no answer.
    pub const TOOL_STEP_LIMIT: &str = "TOOL_STEP_LIMIT";
    /// Nobody answered a tool approval in time.
    pub const TOOL_APPROVAL_TIMEOUT: &str = "TOOL_APPROVAL_TIMEOUT";
}

/// Approvals the app asks itself (MCP tools in `ask` mode): never confused
/// with the server's approval ids, which are UUIDs.
pub const LOCAL_APPROVAL_PREFIX: &str = "local-";

#[derive(Default)]
pub struct ChatState {
    thread_id: Mutex<Option<String>>,
    /// Model the current thread was started with; None = the server default.
    thread_model: Mutex<Option<String>>,
    job_id: Mutex<Option<String>>,
    /// Bumped by reset: an in-flight turn from before the reset stops polling
    /// and does not write its thread back.
    epoch: AtomicU64,
    /// The last `models()` catalogue, to drop a stale setting and to gate the
    /// features a model cannot do.
    known_models: Mutex<Vec<RoadeepModel>>,
    /// Plan locks from the last profile read; a locked feature is never sent.
    locks: Mutex<Option<PlanLocks>>,
    /// Stop was pressed. Checked between the upload, the submit and every poll,
    /// so a stop during the upload or the POST is not lost.
    cancel_requested: AtomicBool,
    /// Wakes the turn being followed: stop, reset, an approval decision.
    wake: Notify,
    /// The tool approval the turn is waiting on, and what became of it.
    approval: Mutex<ApprovalSlot>,
    /// The realtime socket, opened lazily by a turn.
    ws: ws::Hub,
    /// One turn at a time: a second send (a rebuilt UI…) must not clear a
    /// pending Stop, steal `job_id` or post to the same thread.
    busy: AtomicBool,
    /// The running turn's counter, for a UI that re-attaches to it.
    turn: Mutex<Option<u64>>,
    /// The island's tools chip for this conversation; None = settings.chat_tools.
    tools_choice: Mutex<Option<bool>>,
    /// Fingerprint of the tool list the current thread was given, so later
    /// messages only carry a one-line reminder. None = no list in the thread.
    tools_sent: Mutex<Option<String>>,
    /// The tool approval (MCP, `ask` mode) the turn waits on.
    local_approval: Mutex<LocalApproval>,
    local_seq: AtomicU64,
}

/// A local tool approval: waiting for the user, or decided.
#[derive(Debug, Clone, Default, PartialEq)]
enum LocalApproval {
    #[default]
    Idle,
    Waiting(String),
    Decided(String, &'static str),
}

impl LocalApproval {
    /// Err is the error code to answer with.
    fn decide(&mut self, id: &str, decision: &'static str) -> Result<(), &'static str> {
        match self {
            Self::Waiting(waiting) if waiting == id => {
                *self = Self::Decided(id.to_string(), decision);
                Ok(())
            }
            Self::Decided(decided, _) if decided == id => Err(chat_codes::APPROVAL_ALREADY_DECIDED),
            _ => Err(chat_codes::APPROVAL_NOT_FOUND),
        }
    }

    fn decision(&self, id: &str) -> Option<&'static str> {
        match self {
            Self::Decided(decided, decision) if decided == id => Some(decision),
            _ => None,
        }
    }
}

/// What `chat_turn_state` reports.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnState {
    pub busy: bool,
    pub turn: Option<u64>,
}

/// Marks a turn in flight; clears it however the turn ends.
struct Busy<'a>(&'a ChatState);

impl<'a> Busy<'a> {
    fn claim(chat: &'a ChatState, turn: u64) -> Option<Self> {
        if chat.busy.swap(true, Ordering::SeqCst) {
            return None;
        }
        *chat.turn.lock().unwrap() = Some(turn);
        Some(Self(chat))
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        *self.0.turn.lock().unwrap() = None;
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

impl ChatState {
    /// A model belongs to new threads. Reserve the idle chat while preferences
    /// commit so a concurrent send cannot race a successful conversation reset.
    pub fn change_model(
        &self,
        model: &str,
        settings: &Mutex<Settings>,
        persist: impl FnOnce(&Settings) -> Result<(), ()>,
    ) -> Result<Settings, RoadeepError> {
        if self.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(RoadeepError::new(chat_codes::CHAT_BUSY, "A reply is still running."));
        }
        struct Reservation<'a>(&'a AtomicBool);
        impl Drop for Reservation<'_> {
            fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); }
        }
        let _reservation = Reservation(&self.busy);
        if !model.is_empty() && !self.known_models.lock().unwrap().iter().any(|m| m.id == model) {
            return Err(RoadeepError::new("MODEL_UNAVAILABLE", "Choose an available text model."));
        }
        let mut current = settings.lock().unwrap();
        if current.model == model && self.current_thread().is_none() { return Ok(current.clone()); }
        let mut updated = current.clone();
        updated.model = model.to_string();
        persist(&updated).map_err(|_| {
            log::line("chat: model preference persistence failed");
            RoadeepError::new("SETTINGS_SAVE_FAILED", "Could not save the model choice.")
        })?;
        *current = updated.clone();
        self.reset();
        Ok(updated)
    }

    pub fn turn_state(&self) -> TurnState {
        let turn = *self.turn.lock().unwrap();
        TurnState { busy: self.busy.load(Ordering::SeqCst), turn }
    }

    pub fn reset(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        *self.thread_id.lock().unwrap() = None;
        *self.thread_model.lock().unwrap() = None;
        self.approval.lock().unwrap().clear();
        *self.local_approval.lock().unwrap() = LocalApproval::Idle;
        *self.tools_choice.lock().unwrap() = None;
        *self.tools_sent.lock().unwrap() = None;
        self.wake.notify_one();
    }

    /// The tool state an opened thread's messages show (threads.rs).
    pub fn note_thread_tools(&self, fingerprint: Option<String>) {
        *self.tools_sent.lock().unwrap() = fingerprint;
    }

    /// The island's tools chip: on or off for this conversation.
    pub fn set_tools_choice(&self, on: bool) {
        *self.tools_choice.lock().unwrap() = Some(on);
    }

    /// Sign-out or a new sign-in: nothing learnt about the previous account
    /// (thread, catalogue, plan locks, its event socket) may carry over to the next one.
    pub fn forget_account(&self) {
        self.reset();
        self.known_models.lock().unwrap().clear();
        *self.locks.lock().unwrap() = None;
        self.ws.close();
    }

    pub fn current_thread(&self) -> Option<String> {
        self.thread_id.lock().unwrap().clone()
    }

    /// Continues an existing conversation: the next turn goes to this thread
    /// and adds none of a new thread's extras (agent brief, context, Persian
    /// instruction) — they are already in its first message.
    pub fn open_thread(&self, thread_id: String, model: Option<String>) {
        self.reset();
        *self.thread_id.lock().unwrap() = Some(thread_id);
        *self.thread_model.lock().unwrap() = model;
    }

    /// A deleted thread that was the current one: start over.
    pub fn forget_thread(&self, thread_id: &str) -> bool {
        let current = self.thread_id.lock().unwrap().as_deref() == Some(thread_id);
        if current {
            self.reset();
        }
        current
    }

    fn cancelled(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }
}

fn cancelled_error() -> RoadeepError {
    RoadeepError::new(chat_codes::CANCELLED, "The request was cancelled.")
}

/// Job and thread ids come from the server and go back into URL paths.
fn server_id(data: &Value, key: &str) -> Result<Option<String>, RoadeepError> {
    match str_field(data, key) {
        Some(id) if valid_server_id(&id) => Ok(Some(id)),
        Some(_) => {
            log::line(format!("roadeep: refused a malformed {key} from the server"));
            Err(RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected response."))
        }
        None => Ok(None),
    }
}

/// Best effort: a job nobody will read should stop costing the user tokens.
async fn delete_job(rd: &Roadeep, job_id: &str, why: &str) {
    match rd.request(Call::delete(format!("/v1/chat/jobs/{job_id}/"))).await {
        Ok(_) => {}
        Err(err) if err.status == Some(409) || err.code == "JOB_ALREADY_FINISHED" || err.code == "JOB_NOT_FOUND" => {}
        Err(err) => log::line(format!("roadeep: could not cancel job after {why} ({})", err.code)),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatContext {
    File { name: String, path: String },
    Window { app_name: String, title: String, url: Option<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    pub text: String,
    /// The thread the turn went to (also the current one, unless reset meanwhile).
    pub thread_id: Option<String>,
    /// The sticky model ID when known; a server default stays unknown.
    pub model: Option<String>,
    /// The persisted assistant message, when the server said.
    pub message_id: Option<String>,
}

pub struct SendOptions {
    pub query: String,
    pub context: Option<ChatContext>,
    /// A Roadeep agent id, or "local:<id>" for a local agent (agents.rs).
    pub agent_id: Option<String>,
    /// settings.model; "" = server default.
    pub model: String,
    /// settings.language.
    pub language: String,
    pub tools: ToolFlags,
    /// settings.chat_tools: offer the enabled MCP servers' tools (the island's
    /// chip can turn them off for one conversation).
    pub use_tools: bool,
    /// Restricts voice turns; never grants arbitrary tool or approval authority.
    pub voice_task: bool,
    /// The island's turn counter, echoed in every "chat-stream" event.
    pub turn: u64,
}

impl SendOptions {
    pub fn from_settings(query: String, context: Option<ChatContext>, agent_id: Option<String>, settings: &Settings) -> Self {
        Self {
            query,
            context,
            agent_id,
            model: settings.model.clone(),
            language: settings.language.clone(),
            tools: ToolFlags::from_settings(settings),
            use_tools: settings.chat_tools,
            voice_task: false,
            turn: 0,
        }
    }
}

/// The chat features asked for in settings, before any gating.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolFlags {
    pub web_search: bool,
    pub reasoning: bool,
    pub reasoning_effort: String,
    pub deep_research: bool,
}

impl ToolFlags {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            web_search: s.chat_web_search,
            reasoning: s.chat_reasoning,
            reasoning_effort: s.chat_reasoning_effort.clone(),
            deep_research: s.chat_deep_research,
        }
    }

    pub fn any(&self) -> bool {
        self.web_search || self.reasoning || self.deep_research
    }
}

/// `GET /v1/user/profile/` plan locks. A missing or odd flag reads as unlocked:
/// the server still has the final word.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanLocks {
    pub chatbot: bool,
    pub agents: bool,
    pub web_search: bool,
    pub reasoning: bool,
    pub file_upload: bool,
}

/// What the account card shows besides the user.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub locks: PlanLocks,
    pub plan_name: Option<String>,
    /// Spendable tokens; None when the plan could not be read.
    pub wallet_units: Option<u64>,
}

/// Who answers: nobody in particular, an official Roadeep agent, or a local one.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentRef {
    None,
    Roadeep(String),
    Local(LocalAgent),
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

/// Resolves chat_send's agent parameter. `lookup` reads the local store.
pub fn resolve_agent(raw: Option<&str>, lookup: impl Fn(&str) -> Option<LocalAgent>) -> Result<AgentRef, RoadeepError> {
    let Some(raw) = raw.map(str::trim).filter(|a| !a.is_empty()) else { return Ok(AgentRef::None) };
    if let Some(local) = raw.strip_prefix(agents::LOCAL_PREFIX) {
        return match lookup(local) {
            Some(agent) => Ok(AgentRef::Local(agent)),
            None => Err(RoadeepError {
                field_errors: RoadeepError::validation("agent_id", "That agent no longer exists.").field_errors,
                ..RoadeepError::new(agents::agent_codes::NOT_FOUND, "That agent no longer exists.")
            }),
        };
    }
    if valid_agent_id(raw) {
        Ok(AgentRef::Roadeep(raw.to_string()))
    } else {
        Err(RoadeepError::validation("agent_id", "Unknown agent."))
    }
}

/// A missing capability flag means "the server does not say" and does not block,
/// except deep research, which is new and needs an explicit yes.
fn capability(m: &Value, caps: &[&str], key: &str, default: bool) -> bool {
    match m.get("capability_flags").and_then(|f| f.get(key)).and_then(Value::as_bool) {
        Some(v) => v,
        None => caps.contains(&key) || default,
    }
}

/// The wire fields for this turn's features. Whatever the model cannot do or
/// the plan locks is dropped, never sent (the server fails the turn otherwise).
/// Deep research turns web search on by itself and excludes reasoning; web
/// search and reasoning exclude each other, web search winning.
pub fn tool_fields(flags: &ToolFlags, model: Option<&RoadeepModel>, locks: Option<&PlanLocks>) -> Map<String, Value> {
    let locked = locks.cloned().unwrap_or_default();
    let web_ok = model.map_or(true, |m| m.web_search) && !locked.web_search;
    // Without a catalogue the setting is trusted, like `effective_model`.
    let deep_ok = model.map_or(true, |m| m.deep_research) && !locked.web_search;
    let reasoning_ok = model.map_or(true, |m| m.reasoning) && !locked.reasoning;

    let mut fields = Map::new();
    if flags.deep_research && deep_ok {
        fields.insert("deep_research".into(), Value::Bool(true));
    } else if flags.web_search && web_ok {
        fields.insert("web_search".into(), Value::Bool(true));
    } else if flags.reasoning && reasoning_ok {
        fields.insert("reasoning".into(), Value::Bool(true));
        let effort = flags.reasoning_effort.as_str();
        let effort = if crate::settings::REASONING_EFFORTS.contains(&effort) { effort } else { "low" };
        fields.insert("reasoning_effort".into(), Value::String(effort.to_string()));
    }
    fields
}

/// Delimits a local agent's instructions so the model reads them as the brief
/// for the conversation rather than as part of the question.
pub fn instructions_block(instructions: &str) -> String {
    format!("[Agent instructions]\n{}\n[/Agent instructions]\n\n", instructions.trim())
}

/// The model to send for a new thread, if any. Empty, legacy Anthropic ids and
/// ids the server no longer lists are dropped so the server default applies.
pub fn effective_model(setting: &str, known: &[String]) -> Option<String> {
    let model = setting.trim();
    if model.is_empty() || model.starts_with("claude-") {
        return None;
    }
    if !known.is_empty() && !known.iter().any(|k| k == model) {
        return None;
    }
    Some(model.to_string())
}

/// The text actually sent. A local agent's instructions, the window context and
/// the Persian instruction ride along with the first message of a thread only;
/// `tools` (tools::message_block: the tool list or a one-line reminder) right
/// before the question, on any message.
pub fn compose_message(
    query: &str,
    context: Option<&ChatContext>,
    new_thread: bool,
    language: &str,
    instructions: Option<&str>,
    tools: Option<&str>,
) -> String {
    let mut message = String::new();
    if new_thread {
        if let Some(text) = instructions.map(str::trim).filter(|s| !s.is_empty()) {
            message.push_str(&instructions_block(text));
        }
        if let Some(ChatContext::Window { app_name, title, url }) = context {
            message.push_str(&format!("Context — App: {app_name}, Window: {title}"));
            if let Some(url) = url.as_deref().filter(|u| !u.is_empty()) {
                message.push_str(&format!(", URL: {url}"));
            }
            message.push_str("\n\n");
        }
    }
    if let Some(block) = tools {
        message.push_str(block);
    }
    message.push_str(query);
    if new_thread && language == "fa" {
        message.push_str(PERSIAN_SUFFIX);
    }
    message
}

/// The server's canned "save to memory" replies (fa and en), whitespace- and
/// ZWNJ-normalised. The server runs a memory check on every user message, so a
/// local agent's instructions that sound like "remember this" get one of these
/// instead of the model's answer.
const MEMORY_REPLIES: &[&str] = &[
    "نتوانستم این مورد را در حافظه ذخیره کنم. لطفاً دوباره تلاش کنید.",
    "در حافظه ذخیره شد.",
    "saved to memory.",
    "ذخیره‌سازی حافظه در پلن فعلی شما فعال نیست.",
    "memory saving is not enabled on your current plan.",
];
/// The limit replies carry a number, so only their start is fixed.
const MEMORY_REPLY_PREFIXES: &[&str] = &["سقف حافظه پر شده است", "memory limit reached"];
/// A reply that never streamed and is this short may be a reworded canned reply.
const MEMORY_FALLBACK_MAX_CHARS: usize = 160;

fn normalize_reply(text: &str) -> String {
    text.replace('\u{200c}', " ").split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether a finished reply is the server's canned memory reply rather than an
/// answer. `streamed` is whether any text was streamed for the turn (canned
/// replies arrive whole). Exact matches first; the loose fallback only for an
/// unstreamed short reply that speaks of saving in/to memory.
pub fn is_memory_reply(text: &str, streamed: bool) -> bool {
    let reply = normalize_reply(text);
    if reply.is_empty() {
        return false;
    }
    if MEMORY_REPLIES.iter().any(|known| normalize_reply(known) == reply) {
        return true;
    }
    if MEMORY_REPLY_PREFIXES.iter().any(|p| reply.starts_with(&normalize_reply(p))) {
        return true;
    }
    if streamed || reply.chars().count() > MEMORY_FALLBACK_MAX_CHARS {
        return false;
    }
    (reply.contains("در حافظه") && reply.contains("ذخیره")) || reply.contains("to memory")
}

/// Agent ids are UUIDs; anything else is refused before it reaches a URL or body.
fn valid_agent_id(id: &str) -> bool {
    agents::valid_id(id)
}

fn mime_for(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// Only files that went through `ingest_file` (the inbox copy) can be uploaded,
/// and `ingest_file` only takes paths that were really dropped on the island
/// moments ago (lib.rs `DropRegistry`), so the front end cannot turn chat into
/// a reader of arbitrary paths.
fn check_upload_path(path: &str, inbox: &Path) -> Result<PathBuf, RoadeepError> {
    let unreadable = || RoadeepError::new(chat_codes::FILE_UNREADABLE, "Could not read the dropped file.");
    // Lexical checks first: canonicalising a UNC path (\\host\share, \\?\UNC\…)
    // would already make Windows contact that host.
    if path.starts_with("\\\\") || path.starts_with("//") || !Path::new(path).starts_with(inbox) {
        log::line("roadeep: refused to upload a file outside the inbox");
        return Err(unreadable());
    }
    let file = std::fs::canonicalize(path).map_err(|_| unreadable())?;
    let inbox = std::fs::canonicalize(inbox).map_err(|_| unreadable())?;
    if !file.starts_with(&inbox) {
        log::line("roadeep: refused to upload a file outside the inbox");
        return Err(unreadable());
    }
    let meta = std::fs::metadata(&file).map_err(|_| unreadable())?;
    if !meta.is_file() {
        return Err(unreadable());
    }
    if meta.len() > MAX_UPLOAD_BYTES {
        return Err(RoadeepError::new(
            chat_codes::FILE_TOO_LARGE,
            format!("That file is too large (max {} MB).", MAX_UPLOAD_BYTES / (1024 * 1024)),
        ));
    }
    Ok(file)
}

/// What one poll of `GET /v1/chat/jobs/{id}/` means.
#[derive(Debug, PartialEq)]
pub enum JobStep {
    Pending,
    Done(String),
    Failed(RoadeepError),
}

pub fn job_step(data: &Value) -> JobStep {
    let status = data.get("status").and_then(Value::as_str).unwrap_or("");
    match status {
        "done" => {
            let text = data.get("content").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if text.is_empty() {
                JobStep::Failed(RoadeepError::new(chat_codes::EMPTY_RESPONSE, "Roadeep returned an empty answer."))
            } else {
                JobStep::Done(text)
            }
        }
        "error" => JobStep::Failed(ws::job_error(data, None)),
        "cancelled" => JobStep::Failed(RoadeepError::new(chat_codes::CANCELLED, "The request was cancelled.")),
        "awaiting_approval" => JobStep::Failed(RoadeepError::new(
            chat_codes::AWAITING_APPROVAL,
            "This answer needs your approval. Approve it on roadeep.com, then ask again.",
        )),
        // queued | started | pending | retry | processing | generation_pending | unknown
        _ => JobStep::Pending,
    }
}

/// `job_step` for the island, which can wait for an approval instead of failing.
#[derive(Debug, PartialEq)]
pub enum JobState {
    Pending(String),
    /// The approval id, when the server sent a usable one.
    AwaitingApproval(Option<String>),
    Done { text: String, message_id: Option<String> },
    Failed(RoadeepError),
}

pub fn job_state(data: &Value) -> JobState {
    let status = data.get("status").and_then(Value::as_str).unwrap_or("");
    if status == "awaiting_approval" {
        return JobState::AwaitingApproval(str_field(data, "approval_id").filter(|id| valid_server_id(id)));
    }
    match job_step(data) {
        JobStep::Pending => JobState::Pending(status.to_string()),
        JobStep::Done(text) => JobState::Done { text, message_id: str_field(data, "assistant_message_id") },
        JobStep::Failed(err) => JobState::Failed(err),
    }
}

pub fn next_poll_delay(previous: Duration) -> Duration {
    (previous.mul_f32(1.5)).min(POLL_MAX)
}

fn is_transient(err: &RoadeepError) -> bool {
    err.code == codes::NETWORK || err.code == codes::TIMEOUT || err.status.map(|s| s >= 500).unwrap_or(false)
}

fn str_field(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

// ── Chat turn ─────────────────────────────────────────────────────────────────

async fn upload(rd: &Roadeep, ctx_name: &str, path: &str, thread_id: Option<String>) -> Result<String, RoadeepError> {
    let file = check_upload_path(path, &files::inbox_dir())?;
    let file_name = {
        let n = ctx_name.trim();
        let n = if n.is_empty() { file.file_name().and_then(|s| s.to_str()).unwrap_or("file") } else { n };
        n.chars().take(200).collect::<String>()
    };
    let mime = mime_for(&file).to_string();
    let read_path = file.clone();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(read_path))
        .await
        .map_err(|e| RoadeepError::new(chat_codes::FILE_UNREADABLE, format!("Could not read the dropped file: {e}")))?
        .map_err(|_| RoadeepError::new(chat_codes::FILE_UNREADABLE, "Could not read the dropped file."))?;
    log::line(format!("roadeep: uploading attachment ({} bytes, {mime})", bytes.len()));

    let call = Call {
        payload: Payload::Upload(Upload { file_name, mime, bytes, thread_id }),
        ..Call::post("/v1/chat/uploads/", Value::Null).timeout(UPLOAD_TIMEOUT)
    };
    let data = rd.request(call).await?;
    data.get("files")
        .and_then(Value::as_array)
        .and_then(|f| f.first())
        .and_then(|f| str_field(f, "id"))
        .ok_or_else(|| RoadeepError::new(chat_codes::UPLOAD_FAILED, "The file upload failed. Try again."))
}

/// One chat turn: optional upload, submit the async job, follow it until it
/// ends — and, when the model asks for MCP tools, the tool loop after it.
pub async fn send(rd: &Roadeep, chat: &ChatState, opts: SendOptions) -> Result<ChatReply, RoadeepError> {
    let query = opts.query.trim();
    if query.is_empty() {
        return Err(RoadeepError::validation("message", "Type a message first."));
    }
    if query.chars().count() > MAX_MESSAGE_CHARS {
        return Err(RoadeepError::validation("message", "That message is too long."));
    }
    validate_voice_context(opts.voice_task, opts.context.as_ref())?;
    let agent = if opts.voice_task { AgentRef::None } else { resolve_agent(opts.agent_id.as_deref(), agents::find)? };
    let Some(_busy) = Busy::claim(chat, opts.turn) else {
        return Err(RoadeepError::new(chat_codes::CHAT_BUSY, "A reply is already being written."));
    };
    chat.cancel_requested.store(false, Ordering::SeqCst);
    let mut tools = opts.tools.clone();
    let mut model_setting = opts.model.clone();
    let (agent_id, instructions) = match &agent {
        AgentRef::None => (None, None),
        AgentRef::Roadeep(id) => (Some(id.clone()), None),
        AgentRef::Local(local) => {
            if !local.model.trim().is_empty() {
                model_setting = local.model.clone();
            }
            // The agent asks for web search: that wins over reasoning.
            if local.web_search {
                tools.web_search = true;
                tools.reasoning = false;
            }
            (local.base_agent_id.clone(), Some(local.instructions.clone()))
        }
    };

    let epoch = chat.epoch.load(Ordering::SeqCst);
    let proposal_epoch = rd.app.get().map(|app|app.state::<crate::local_intelligence::assistant::VoiceAssistant>().generation()).unwrap_or(0);
    let thread_id = if opts.voice_task { None } else { chat.current_thread() };
    let new_thread = thread_id.is_none();

    // Gating needs the catalogue; load it once if nothing has asked for it yet.
    let wants_catalogue = !model_setting.trim().is_empty() || tools.any();
    if wants_catalogue &&chat.known_models.lock().unwrap().is_empty() {
        if let Err(err) = models(rd, chat).await {
            log::line(format!("roadeep: model catalogue unavailable ({}), features sent as set", err.code));
        }
    }
    let known = chat.known_models.lock().unwrap().clone();
    let known_ids: Vec<String> = known.iter().map(|m| m.id.clone()).collect();

    // The user's MCP tools, when this conversation uses them. Connecting a
    // server may take a few seconds; one that fails is simply left out.
    let computer_agent = match &agent {
        AgentRef::None => "default".to_string(),
        AgentRef::Roadeep(id) => id.clone(),
        AgentRef::Local(local) => format!("local:{}", local.id),
    };
    let offered = turn_tools(rd, chat, opts.use_tools, &computer_agent, opts.voice_task).await;
    if chat.cancelled() {
        return Err(cancelled_error());
    }
    let sent_list = if opts.voice_task { None } else { chat.tools_sent.lock().unwrap().clone() };
    let (tools_block, tools_state) = tools::message_block(sent_list.as_deref(), offered.as_ref().map(|(_, p)| p));

    let mut body = Map::new();
    if let Some(ChatContext::File { name, path }) = &opts.context {
        let id = upload(rd, name, path, thread_id.clone()).await?;
        body.insert("attachment_ids".into(), json!([id]));
    }
    // Today's date on every message (cached online clock; never waits on the network).
    let clock_line = rd.app.get().map(|app| crate::clock::chat_line(&crate::clock::now_cached(app))).unwrap_or_default();
    body.insert(
        "message".into(),
        Value::String(clock_line + &compose_message(
            query,
            opts.context.as_ref(),
            new_thread,
            &opts.language,
            instructions.as_deref(),
            tools_block.as_deref(),
        )),
    );
    let thread_model = match &thread_id {
        Some(id) => {
            body.insert("thread_id".into(), Value::String(id.clone()));
            chat.thread_model.lock().unwrap().clone()
        }
        None => {
            let chosen = effective_model(&model_setting, &known_ids);
            match &chosen {
                Some(model) => {
                    body.insert("model".into(), Value::String(model.clone()));
                }
                None if !model_setting.trim().is_empty() => {
                    log::line("roadeep: configured model not available, using the server default");
                }
                None => {}
            }
            chosen
        }
    };
    let model_info = match &thread_model {
        Some(id) => known.iter().find(|m| &m.id == id),
        None => known.iter().find(|m| m.is_default),
    };
    let locks = chat.locks.lock().unwrap().clone();
    let fields = tool_fields(&tools, model_info, locks.as_ref());
    let timeout = if fields.contains_key("deep_research") { DEEP_RESEARCH_TIMEOUT } else { JOB_TIMEOUT };
    // What every message of the turn carries (tool results go out like the question).
    let mut common = fields;
    if let Some(agent) = agent_id {
        common.insert("agent_id".into(), Value::String(agent));
    }
    common.insert("source".into(), Value::String("chat".into()));
    body.extend(common.clone());

    // Stop pressed during the upload: nothing has been asked yet.
    if chat.cancelled() {
        return Err(cancelled_error());
    }
    let mut turn_thread = thread_id;
    let accepted = |thread: &str| {
        accept_turn_thread(chat, opts.voice_task, thread, new_thread, thread_model.clone(), tools_state.clone());
    };
    let mut result =
        run_step(rd, chat, opts.turn, epoch, body, timeout, offered.is_some(), opts.voice_task, &mut turn_thread, accepted).await;
    // Only a new thread's first message carries the agent's instructions; a
    // memory reply anywhere else is what the user asked for.
    let briefed_agent = match &agent {
        AgentRef::Local(local) if new_thread && !local.instructions.trim().is_empty() => Some(local.name.clone()),
        _ => None,
    };
    if let (Some(agent_name), Ok(done)) = (briefed_agent, &result) {
        if is_memory_reply(&done.finished.text, done.streamed) {
            // The thread holds nothing but the brief and a canned reply.
            if let Some(thread) = &turn_thread {
                threads::delete_quietly(rd, thread, "memory-trigger").await;
                chat.forget_thread(thread);
            }
            result = Err(RoadeepError {
                agent_name: Some(agent_name),
                ..RoadeepError::new(
                    chat_codes::AGENT_MEMORY_TRIGGER,
                    "Roadeep read this agent's instructions as a request to save to memory and did not answer.",
                )
            });
        }
    }
    if let Some((set, _)) = &offered {
        result = match result {
            Ok(step) => {
                let mut ctx = LoopCtx { rd, chat, turn: opts.turn, epoch, timeout, common: &common, thread: &mut turn_thread, computer_agent: &computer_agent, voice_task: opts.voice_task, language: &opts.language, proposal_epoch };
                tool_loop(&mut ctx, set, step).await
            }
            Err(err) => Err(err),
        };
    }
    if opts.voice_task {
        if let Some(app) = rd.app.get() {
            match crate::local_intelligence::assistant::pending_question(app, &opts.language) {
                Ok(Some(question)) => if let Ok(done) = &mut result { done.finished.text = question; },
                Ok(None) => (),
                Err(code) => result = Err(RoadeepError::new(&code, "The voice proposal state could not be read.")),
            }
        }
    }
    chat.approval.lock().unwrap().clear();
    *chat.local_approval.lock().unwrap() = LocalApproval::Idle;
    match &result {
        // Credit was spent (or ran out): the balance on screen is stale.
        Ok(_) => rd.push_balance(),
        // A tool turn that ended early (stop, a failed step) may still have spent credit on its steps.
        Err(err)
            if err.code == codes::INSUFFICIENT_CREDITS
                || err.code == chat_codes::AGENT_MEMORY_TRIGGER
                || offered.is_some() =>
        {
            rd.push_balance()
        }
        Err(_) => {}
    }
    if let Err(err) = &result {
        log::line(format!("roadeep: chat job ended with {}", err.code));
    }
    result.map(|done| ChatReply { text: done.finished.text, thread_id: turn_thread,
        model: if opts.voice_task { thread_model } else { chat.thread_model.lock().unwrap().clone() }, message_id: done.finished.message_id })
}

/// One finished message of a turn.
struct Step {
    finished: Finished,
    /// Any text streamed for it (the server's canned replies stream none).
    streamed: bool,
}

/// Posts one message of a turn and follows its job to the end. `accepted` runs
/// once the server took the message — unless the conversation was reset
/// meanwhile — with the thread it went to; `thread` follows the server.
#[allow(clippy::too_many_arguments, clippy::result_large_err)]
async fn run_step(
    rd: &Roadeep,
    chat: &ChatState,
    turn: u64,
    epoch: u64,
    body: Map<String, Value>,
    timeout: Duration,
    hide_calls: bool,
    voice_task: bool,
    thread: &mut Option<String>,
    accepted: impl FnOnce(&str),
) -> Result<Step, RoadeepError> {
    if chat.cancelled() {
        return Err(cancelled_error());
    }
    // The socket opens before the job exists, so none of its frames are missed.
    let sub = match chat.ws.subscribe(rd, false).await {
        Ok(rx) => Some(rx),
        Err(OpenError::Expired { generation }) => return Err(rd.expired(generation, None)),
        Err(OpenError::Unavailable(why)) => {
            log::line(format!("roadeep: chat turn polls ({why})"));
            None
        }
    };
    if chat.cancelled() {
        return Err(cancelled_error());
    }
    let answer = rd.request(Call::post("/v1/chat/async/", Value::Object(body))).await?;
    let job_id = server_id(&answer, "job_id")?
        .ok_or_else(|| RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected response."))?;
    if let Some(sent_to) = server_id(&answer, "thread_id")? {
        *thread = Some(sent_to);
    }
    if let Some(sent_to) = thread.as_deref() {
        if chat.epoch.load(Ordering::SeqCst) == epoch {
            accepted(sent_to);
        }
    }
    *chat.job_id.lock().unwrap() = Some(job_id.clone());

    // Stop pressed while the POST was out: cancel() had no job id to send yet.
    let (result, last_job, streamed) = if chat.cancelled() {
        delete_job(rd, &job_id, "a stop during submit").await;
        (Err(cancelled_error()), job_id, false)
    } else {
        let mut follow = Follow::new(rd, chat, turn, epoch, &job_id, timeout, sub);
        follow.hide_calls = hide_calls;
        follow.voice_task = voice_task;
        let result = follow.run().await;
        (result, follow.tracker.job().to_string(), follow.streamed)
    };
    // Reset (new chat, another thread opened, sign-out) mid-turn: nobody reads
    // this job any more, so it should stop costing the user tokens.
    cancel_voice_block(&result, delete_job(rd, &last_job, "voice policy blocked approval")).await;
    if result.is_err() && chat.epoch.load(Ordering::SeqCst) != epoch {
        delete_job(rd, &last_job, "a reset").await;
    }
    chat.approval.lock().unwrap().clear();
    {
        let mut current = chat.job_id.lock().unwrap();
        if current.as_deref() == Some(last_job.as_str()) {
            *current = None;
        }
    }
    result.map(|finished| Step { finished, streamed })
}

// ── MCP tools ─────────────────────────────────────────────────────────────────

/// The tools offered in one turn (those that fit the preamble).
struct TurnTools {
    specs: Vec<mcpc::ToolSpec>,
}

impl TurnTools {
    /// The tool a call names: its qualified name (any case when unambiguous),
    /// or the server's own name when only one server has it.
    fn find(&self, name: &str) -> Option<&mcpc::ToolSpec> {
        fn unique<'s>(mut it: impl Iterator<Item = &'s mcpc::ToolSpec>) -> Option<&'s mcpc::ToolSpec> {
            let first = it.next();
            if it.next().is_some() {
                None
            } else {
                first
            }
        }
        let found = self
            .specs
            .iter()
            .find(|s| s.qualified == name)
            .or_else(|| unique(self.specs.iter().filter(|s| s.qualified.eq_ignore_ascii_case(name))))
            .or_else(|| unique(self.specs.iter().filter(|s| s.tool == name)));
        found.filter(|s| s.mode != mcpc::ToolMode::Off)
    }
}

/// The tools this turn may use: none when the conversation has them off, no
/// server is enabled, or no server answered (mcpc skips the failing ones).
async fn turn_tools(rd: &Roadeep, chat: &ChatState, wanted: bool, computer_agent: &str, voice_task: bool) -> Option<(TurnTools, tools::Preamble)> {
    let on = chat.tools_choice.lock().unwrap().unwrap_or(wanted);
    if !on && !voice_task {
        return None;
    }
    let app = rd.app.get()?;
    let started = Instant::now();
    let mut specs = if voice_task { voice_specs() } else {
        let mut specs = if mcpc::any_enabled() { mcpc::available_tools(app).await } else { crate::planner::tools::specs() };
        specs.extend(crate::desktop_actions::specs());
        specs
    };
    if !voice_task && app.state::<crate::computer::ComputerState>().agent_running(computer_agent) {
        // mcpc reserves the computer slug; configured servers retain their own tools under a suffixed slug.
        specs.splice(0..0, crate::computer::tools::specs());
    }
    if specs.is_empty() {
        log::line(format!("roadeep: no MCP tool available for this turn ({} ms)", started.elapsed().as_millis()));
        return None;
    }
    let (specs, mut preamble) = offer(specs);
    if voice_task { preamble.text.push_str("\nVoice policy: this turn is read-only; only the supplied read-only tools are available. Changes are done by the voice assistant's own approved tools, never here: do not attempt them or ask for approval. Never claim execution without a successful tool result."); }
    log::line(format!(
        "roadeep: {} MCP tools offered ({} chars, {} ms)",
        specs.len(),
        preamble.text.chars().count(),
        started.elapsed().as_millis()
    ));
    Some((TurnTools { specs }, preamble))
}

fn accept_turn_thread(chat: &ChatState, voice_task: bool, thread: &str, new_thread: bool, model: Option<String>, tools_state: Option<String>) {
    if voice_task { return; }
    *chat.thread_id.lock().unwrap() = Some(thread.to_string());
    if new_thread { *chat.thread_model.lock().unwrap() = model; }
    *chat.tools_sent.lock().unwrap() = tools_state;
}
fn validate_voice_context(voice_task: bool, context: Option<&ChatContext>) -> Result<(), RoadeepError> {
    if voice_task && matches!(context, Some(ChatContext::File { .. })) { Err(voice_blocked()) } else { Ok(()) }
}
/// Voice chat turns are strictly read-only: every change goes through the live voice approval card.
fn voice_specs() -> Vec<mcpc::ToolSpec> {
    crate::planner::tools::specs().into_iter().filter(voice_tool_allowed).collect()
}
fn voice_tool_allowed(spec: &mcpc::ToolSpec) -> bool {
    spec.mode == mcpc::ToolMode::Auto && spec.read_only && !spec.destructive
        && spec.server_id == crate::planner::tools::SERVER_ID
        && matches!(spec.tool.as_str(), "list_tasks" | "list_reminders" | "list_habits")
}
async fn cancel_voice_block<T>(result: &Result<T, RoadeepError>, cancel: impl std::future::Future<Output = ()>) {
    if result.as_ref().err().is_some_and(|e| e.code == "voice-sensitive-action-blocked") { cancel.await; }
}
fn voice_blocked() -> RoadeepError { RoadeepError::new("voice-sensitive-action-blocked", "Voice tasks cannot execute actions requiring approval.") }

/// The preamble for `specs`, and the tools it could list (the rest are cut).
fn offer(specs: Vec<mcpc::ToolSpec>) -> (Vec<mcpc::ToolSpec>, tools::Preamble) {
    let entries: Vec<ToolEntry> = specs
        .iter()
        .map(|s| ToolEntry { qualified: s.qualified.clone(), description: s.description.clone(), schema: s.input_schema.clone() })
        .collect();
    let preamble = tools::preamble(&entries);
    if preamble.listed < specs.len() {
        log::line(format!("roadeep: only {} of {} MCP tools fit in the tool list", preamble.listed, specs.len()));
    }
    let listed = preamble.listed;
    (specs.into_iter().take(listed).collect(), preamble)
}

/// What every step of the tool loop needs.
struct LoopCtx<'a, 'b> {
    rd: &'a Roadeep,
    chat: &'a ChatState,
    turn: u64,
    epoch: u64,
    timeout: Duration,
    common: &'a Map<String, Value>,
    thread: &'b mut Option<String>,
    computer_agent: &'a str,
    voice_task: bool,
    language: &'a str,
    proposal_epoch: u64,
}

impl LoopCtx<'_, '_> {
    fn emit(&self, body: StreamBody) {
        emit_stream(self.rd, self.turn, body);
    }

    fn stop(&self) -> Option<RoadeepError> {
        if self.chat.epoch.load(Ordering::SeqCst) != self.epoch {
            Some(RoadeepError::new(chat_codes::CANCELLED, "The conversation was reset."))
        } else if self.chat.cancelled() {
            Some(cancelled_error())
        } else {
            None
        }
    }

    /// The next message of the turn, in the same thread.
    #[allow(clippy::result_large_err)]
    async fn post(&mut self, message: String) -> Result<Step, RoadeepError> {
        if let Some(err) = self.stop() {
            return Err(err);
        }
        let Some(thread) = self.thread.clone() else {
            log::line("roadeep: tool loop has no thread to continue");
            return Err(RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected response."));
        };
        let mut body = self.common.clone();
        body.insert("message".into(), Value::String(message));
        body.insert("thread_id".into(), Value::String(thread));
        // The next reply replaces whatever the last one streamed.
        self.emit(StreamBody::Delta { text: String::new() });
        run_step(self.rd, self.chat, self.turn, self.epoch, body, self.timeout, true, self.voice_task, self.thread, |_| {}).await
    }
}

/// While the reply asks for a tool: run it (asking the user first when its
/// mode says so) and send the result back as the next message, until the
/// model answers. One correction for a call the app cannot run, then its text
/// stands; after MAX_TOOL_STEPS the model is asked to answer with what it has.
#[allow(clippy::result_large_err)] // RoadeepError, like every call in this file
async fn tool_loop(ctx: &mut LoopCtx<'_, '_>, set: &TurnTools, first: Step) -> Result<Step, RoadeepError> {
    let mut step = first;
    let mut steps = 0usize;
    let mut corrected = false;
    loop {
        let parsed = tools::parse_reply(&step.finished.text);
        let known = matches!(&parsed, Parsed::Call { tool, .. } if set.find(tool).is_some());
        match next_move(&parsed, known, steps, corrected) {
            Move::Answer => return Ok(step),
            Move::GiveUp(why) => {
                log::line(format!("roadeep: tool call unusable again ({why}), showing the reply"));
                return Ok(step);
            }
            Move::Correct(why) => {
                corrected = true;
                log::line(format!("roadeep: tool call unusable ({why}), asking once more"));
                step = ctx.post(tools::correction_message(why)).await?;
            }
            Move::Limit => {
                log::line("roadeep: tool step limit reached, asking for an answer");
                let mut last = ctx.post(tools::limit_message()).await?;
                let text = tools::strip_calls(&last.finished.text);
                if text.is_empty() {
                    return Err(RoadeepError::new(
                        chat_codes::TOOL_STEP_LIMIT,
                        "The assistant kept calling tools and did not answer. Try a narrower question.",
                    ));
                }
                last.finished.text = text;
                return Ok(last);
            }
            Move::Run => {
                let Parsed::Call { tool, arguments } = parsed else { return Ok(step) };
                let Some(spec) = set.find(&tool) else { return Ok(step) };
                steps += 1;
                ctx.emit(StreamBody::Delta { text: String::new() });
                let result = run_tool(ctx, spec, arguments, steps).await?;
                if ctx.voice_task {
                    if let Some(app) = ctx.rd.app.get() {
                        if let Some(question) = crate::local_intelligence::assistant::pending_question(app, ctx.language).map_err(|code| RoadeepError::new(&code, "The voice proposal state could not be read."))? {
                            return Ok(Step { finished: Finished { text: question, message_id: None }, streamed: false });
                        }
                    }
                }
                step = ctx.post(result).await?;
                if is_memory_reply(&step.finished.text, step.streamed) {
                    return Err(RoadeepError::new(
                        chat_codes::TOOL_MEMORY_TRIGGER,
                        "Roadeep took the tool's output for a request to save to memory and did not answer.",
                    ));
                }
            }
        }
    }
}

/// What the tool loop does with a reply.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Move {
    /// A normal answer: the turn is over.
    Answer,
    /// Run the call and send its result back.
    Run,
    /// The one correction of the turn.
    Correct(&'static str),
    /// A second unusable call: its text is the answer.
    GiveUp(&'static str),
    /// MAX_TOOL_STEPS done: ask for an answer instead of running another call.
    Limit,
}

/// `known`: the call names a tool that is offered. `steps`: tool steps done
/// (run or declined) for this user message; `corrected`: the correction is spent.
fn next_move(parsed: &Parsed, known: bool, steps: usize, corrected: bool) -> Move {
    let unusable = match parsed {
        Parsed::Answer => return Move::Answer,
        Parsed::Invalid(why) => *why,
        Parsed::Call { .. } if !known => "unknown tool name",
        Parsed::Call { .. } if steps >= tools::MAX_TOOL_STEPS => return Move::Limit,
        Parsed::Call { .. } => return Move::Run,
    };
    if corrected {
        Move::GiveUp(unusable)
    } else {
        Move::Correct(unusable)
    }
}

/// One tool step: approval when the tool asks for it, the call (stopped by
/// Stop or a reset), its row on the island. Returns the message for the model.
#[allow(clippy::result_large_err)] // RoadeepError, like every call in this file
async fn run_tool(
    ctx: &LoopCtx<'_, '_>,
    spec: &mcpc::ToolSpec,
    arguments: Value,
    n: usize,
) -> Result<String, RoadeepError> {
    if ctx.voice_task && !voice_tool_allowed(spec) { return Err(voice_blocked()); }
    let server = clamp_one_line(&spec.server_name, 60);
    let tool = clamp_one_line(&spec.tool, 64);
    let mut view = ToolStepView {
        id: format!("{}-{n}", ctx.turn),
        server: server.clone(),
        tool: tool.clone(),
        state: "running",
        arguments: tools::arguments_preview(&arguments),
        result: None,
        error: None,
    };
    let Some(app) = ctx.rd.app.get() else {
        return Err(RoadeepError::new(codes::INVALID_RESPONSE, "The app is not ready."));
    };
    // The mode as it is now, not as the turn listed it: the user may have
    // switched the tool to "ask" (or off) since. Calls go by the stable
    // identity (server id + the server's own tool name).
    let checked = tokio::select! {
        biased;
        err = until_stopped(ctx.chat, ctx.epoch) => {
            view.state = "stopped";
            ctx.emit(StreamBody::ToolStep { step: view });
            log::line(format!("roadeep: tool step {n} stopped ({})", spec.qualified));
            return Err(err);
        }
        mode = async {
            if spec.server_id == crate::desktop_actions::SERVER_ID { crate::desktop_actions::mode(&spec.tool) }
            else if spec.server_id == crate::computer::tools::SERVER_ID { crate::computer::tools::mode(&spec.tool) }
            else { mcpc::current_mode(app, &spec.server_id, &spec.tool).await }
        } => mode,
    };
    let mut ask = match checked {
        Ok(mode) => mode == mcpc::ToolMode::Ask,
        Err(coded) => return Ok(failed_step(ctx, view, spec, n, &coded, None)),
    };
    if ctx.voice_task && spec.server_id == crate::planner::tools::SERVER_ID && matches!(spec.tool.as_str(), "add_task" | "add_note") {
        let outcome = crate::local_intelligence::assistant::propose_tool(app, &spec.tool, &arguments, ctx.proposal_epoch);
        return match outcome {
            Ok(out) => { view.state = "done"; view.result = Some(tools::result_preview(&out.text)); ctx.emit(StreamBody::ToolStep { step: view }); Ok(tools::result_message(&spec.qualified, true, &out.text)) },
            Err(coded) => Ok(failed_step(ctx, view, spec, n, &coded, None)),
        };
    }
    let mut approved = false;
    loop {
        if ctx.voice_task && ask { return Err(voice_blocked()); }
        if ask && !approved {
            // The card shows the input in full; one too long for that is refused, never asked half-seen.
            let Some(shown) = tools::approval_arguments(&arguments) else {
                let coded = errors::coded(errors::MCPC_ARGS_TOO_LARGE, &[&tools::MAX_APPROVAL_ARGUMENTS.to_string()]);
                return Ok(failed_step(ctx, view, spec, n, &coded, None));
            };
            view.state = "waiting";
            ctx.emit(StreamBody::ToolStep { step: view.clone() });
            let label = if server.is_empty() { tool.clone() } else { format!("{server} · {tool}") };
            let call = LocalCall {
                server: spec.server_name.chars().filter(|c| shown_char(*c)).collect(),
                tool: spec.tool.chars().filter(|c| shown_char(*c)).collect(),
                arguments: shown,
            };
            match wait_local_approval(ctx, &label, &arguments, call).await {
                Ok(true) => approved = true,
                Ok(false) => {
                    view.state = "declined";
                    ctx.emit(StreamBody::ToolStep { step: view });
                    log::line(format!("roadeep: tool step {n} declined ({})", spec.qualified));
                    return Ok(tools::result_message(&spec.qualified, false, tools::DECLINED_TEXT));
                }
                Err(err) => {
                    view.state = "stopped";
                    ctx.emit(StreamBody::ToolStep { step: view });
                    return Err(err);
                }
            }
        }
        view.state = "running";
        ctx.emit(StreamBody::ToolStep { step: view.clone() });
        ctx.emit(StreamBody::Status { status: "tool", label: Some(tool.clone()) });
        let started = Instant::now();
        let outcome = tokio::select! {
            biased;
            err = until_stopped(ctx.chat, ctx.epoch) => {
                view.state = "stopped";
                ctx.emit(StreamBody::ToolStep { step: view });
                log::line(format!("roadeep: tool step {n} stopped ({})", spec.qualified));
                return Err(err);
            }
            outcome = async {
                if spec.server_id == crate::desktop_actions::SERVER_ID { crate::desktop_actions::call(&spec.tool, arguments.clone()) }
                else if spec.server_id == crate::computer::tools::SERVER_ID { crate::computer::tools::call(app, ctx.computer_agent, &spec.tool, approved, arguments.clone()).await }
                else { mcpc::call_tool(app, &spec.server_id, &spec.tool, approved, arguments.clone()).await }
            } => outcome,
        };
        let elapsed = started.elapsed().as_millis();
        match outcome {
            // Switched to "ask" between the check above and the call: ask now.
            Err(coded) if !approved && code_of(&coded) == errors::MCPC_TOOL_ASK => {
                log::line(format!("roadeep: tool step {n} now needs approval ({})", spec.qualified));
                ask = true;
            }
            Ok(out) => {
                let mut text = out.text;
                if !out.omitted.is_empty() {
                    let kinds: Vec<String> = out.omitted.iter().take(8).map(|m| clamp_one_line(m, 60)).collect();
                    text.push_str(&format!("\n[{} non-text item(s) not shown: {}]", out.omitted.len(), kinds.join(", ")));
                }
                view.state = if out.is_error { "error" } else { "done" };
                view.result = Some(tools::result_preview(&text));
                ctx.emit(StreamBody::ToolStep { step: view });
                log::line(format!(
                    "roadeep: tool step {n} {} ({}, {} chars, {elapsed} ms)",
                    spec.qualified,
                    if out.is_error { "tool error" } else { "ok" },
                    text.chars().count()
                ));
                return Ok(tools::result_message(&spec.qualified, !out.is_error, &text));
            }
            Err(coded) => return Ok(failed_step(ctx, view, spec, n, &coded, Some(elapsed))),
        }
    }
}

/// The code of a coded error (`CODE|arg…`, stderr lines after it).
fn code_of(coded: &str) -> &str {
    coded.split(['|', '\n']).next().unwrap_or("").trim()
}

/// A tool step that could not run: its row shows the coded error, the model
/// gets only the code (safe to log and to send; the arguments may hold a host
/// or a path).
fn failed_step(ctx: &LoopCtx<'_, '_>, mut view: ToolStepView, spec: &mcpc::ToolSpec, n: usize, coded: &str, elapsed: Option<u128>) -> String {
    let code = code_of(coded).to_string();
    view.state = "error";
    view.error = Some(coded.lines().next().unwrap_or("").chars().filter(|c| shown_char(*c)).take(300).collect());
    ctx.emit(StreamBody::ToolStep { step: view });
    let took = elapsed.map(|ms| format!(", {ms} ms")).unwrap_or_default();
    log::line(format!("roadeep: tool step {n} {} failed ({code}{took})", spec.qualified));
    tools::result_message(&spec.qualified, false, &format!("The tool could not run (error {code})."))
}

/// Resolves once the turn is stopped or the conversation reset.
async fn until_stopped(chat: &ChatState, epoch: u64) -> RoadeepError {
    loop {
        if chat.epoch.load(Ordering::SeqCst) != epoch {
            return RoadeepError::new(chat_codes::CANCELLED, "The conversation was reset.");
        }
        if chat.cancelled() {
            return cancelled_error();
        }
        // Stop and reset both wake the turn; a stale wake-up just loops once.
        chat.wake.notified().await;
    }
}

/// Shows an approval card for a tool in `ask` mode and waits for the click
/// (`chat_approval_decide` with the "local-" id). The card carries `call`, the
/// exact input in full. Ok(false) = declined.
#[allow(clippy::result_large_err)] // RoadeepError, like every call in this file
async fn wait_local_approval(ctx: &LoopCtx<'_, '_>, label: &str, arguments: &Value, call: LocalCall) -> Result<bool, RoadeepError> {
    let chat = ctx.chat;
    let id = format!("{LOCAL_APPROVAL_PREFIX}{}", chat.local_seq.fetch_add(1, Ordering::SeqCst) + 1);
    *chat.local_approval.lock().unwrap() = LocalApproval::Waiting(id.clone());
    let summary = approval_summary(label, Some(arguments));
    ctx.emit(StreamBody::Approval { approval: ApprovalCard { id: id.clone(), tool: label.to_string(), summary, call: Some(call) } });
    log::line("roadeep: chat turn waits for a tool approval (local)");
    let since = Instant::now();
    let outcome = loop {
        if let Some(err) = ctx.stop() {
            break Err(err);
        }
        if let Some(decision) = chat.local_approval.lock().unwrap().decision(&id) {
            break Ok(decision);
        }
        if since.elapsed() > APPROVAL_TIMEOUT {
            break Err(RoadeepError::new(chat_codes::TOOL_APPROVAL_TIMEOUT, "Nobody answered the tool approval in time."));
        }
        tokio::select! {
            _ = chat.wake.notified() => {}
            _ = tokio::time::sleep(APPROVAL_CHECK) => {}
        }
    };
    *chat.local_approval.lock().unwrap() = LocalApproval::Idle;
    if let Ok(decision) = &outcome {
        log::line(format!("roadeep: local tool approval {decision}"));
        ctx.emit(StreamBody::ApprovalDone { approval_id: id, decision });
    }
    outcome.map(|decision| decision == "approve")
}

/// Emits one "chat-stream" event to the island.
fn emit_stream(rd: &Roadeep, turn: u64, body: StreamBody) {
    rd.emit_to(Some(crate::island::WINDOW_LABEL), STREAM_EVENT, StreamEvent { turn, body });
}

/// What the island's tools chip shows.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatToolsState {
    /// At least one MCP server is enabled; without one there is no chip.
    pub available: bool,
    /// Tools are on for this conversation.
    pub on: bool,
    /// Tools the model would be offered; None when not counted.
    pub count: Option<usize>,
}

/// The chip's state. `default_on` is settings.chat_tools as the island has it;
/// `count` connects the enabled servers (lazily, ≤ 15 s each) to count their tools.
#[tauri::command]
#[allow(clippy::result_large_err)]
pub async fn chat_tools_state(
    app: tauri::AppHandle,
    chat: tauri::State<'_, ChatState>,
    default_on: bool,
    count: bool,
) -> Result<ChatToolsState, RoadeepError> {
    let available = mcpc::any_enabled();
    let on = chat.tools_choice.lock().unwrap().unwrap_or(default_on);
    let count = if available && on && count {
        let specs = mcpc::available_tools(&app).await;
        Some(if specs.is_empty() { 0 } else { offer(specs).0.len() })
    } else {
        None
    };
    Ok(ChatToolsState { available, on, count })
}

/// The chip: tools on or off for this conversation (a new chat starts from the setting).
#[tauri::command]
pub fn chat_tools_set(chat: tauri::State<'_, ChatState>, on: bool) {
    chat.set_tools_choice(on);
    log::line(format!("roadeep: chat tools {} for this conversation", if on { "on" } else { "off" }));
}

// ── Live turn ─────────────────────────────────────────────────────────────────

/// What the island receives as "chat-stream" while a turn runs.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamEvent {
    pub turn: u64,
    #[serde(flatten)]
    pub body: StreamBody,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StreamBody {
    /// The FULL reply so far (replace, don't append).
    Delta { text: String },
    /// "thinking" | "searching" | "tool" | "generating" | "queued".
    Status { status: &'static str, label: Option<String> },
    /// The turn waits for a human decision on a tool call.
    Approval { approval: ApprovalCard },
    #[serde(rename_all = "camelCase")]
    ApprovalDone { approval_id: String, decision: &'static str },
    /// An MCP tool step of the turn, sent again whenever its state changes (same `id`).
    ToolStep { step: ToolStepView },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ApprovalCard {
    pub id: String,
    pub tool: String,
    /// The proposed arguments in one short line.
    pub summary: String,
    /// Local (MCP, "local-" id) approvals only: exactly what will run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<LocalCall>,
}

/// The MCP call a local approval card shows in full.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LocalCall {
    /// The server's name as the user named it.
    pub server: String,
    /// The tool's own name on that server (what `tools/call` gets).
    pub tool: String,
    /// The arguments as pretty JSON, unmasked (tools::approval_arguments).
    pub arguments: String,
}

/// The approval a turn waits on. Decisions go through `begin` → `decided` so
/// a second click (or an approval of another turn) is refused, and a decision
/// that lands after the turn ended is known to have nobody following it.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ApprovalSlot {
    #[default]
    Idle,
    Waiting(String),
    Deciding(String),
    /// Posted; the turn has not picked the new job up yet.
    Decided { id: String, job: String, decision: &'static str },
    /// The turn follows the resumed job.
    Followed { id: String },
}

impl ApprovalSlot {
    fn id(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Waiting(id) | Self::Deciding(id) | Self::Decided { id, .. } | Self::Followed { id } => Some(id),
        }
    }

    pub fn wait(&mut self, approval: &str) {
        if self.id() != Some(approval) {
            *self = Self::Waiting(approval.to_string());
        }
    }

    /// The decision may be posted. Err is the error code to answer with.
    pub fn begin(&mut self, approval: &str) -> Result<(), &'static str> {
        match self {
            Self::Waiting(id) if id == approval => {
                *self = Self::Deciding(approval.to_string());
                Ok(())
            }
            _ if self.id() == Some(approval) => Err(chat_codes::APPROVAL_ALREADY_DECIDED),
            _ => Err(chat_codes::APPROVAL_NOT_FOUND),
        }
    }

    /// The server accepted the decision. False: no turn follows `job` any more.
    pub fn decided(&mut self, approval: &str, job: &str, decision: &'static str) -> bool {
        match self {
            Self::Deciding(id) if id == approval => {
                *self = Self::Decided { id: id.clone(), job: job.to_string(), decision };
                true
            }
            // The socket told the turn first.
            Self::Followed { id } => id == approval,
            _ => false,
        }
    }

    pub fn failed(&mut self, approval: &str) {
        if matches!(self, Self::Deciding(id) if id == approval) {
            *self = Self::Waiting(approval.to_string());
        }
    }

    /// A decision is out or made: the old job's own status no longer matters.
    pub fn in_progress(&self, approval: &str) -> bool {
        matches!(self, Self::Deciding(id) | Self::Decided { id, .. } if id == approval)
    }

    /// The turn picks up the resumed job.
    pub fn take_resumed(&mut self, approval: &str) -> Option<(String, &'static str)> {
        match std::mem::take(self) {
            Self::Decided { id, job, decision } if id == approval => {
                *self = Self::Followed { id };
                Some((job, decision))
            }
            other => {
                *self = other;
                None
            }
        }
    }

    /// The resumed job was announced on the socket (decided here or elsewhere).
    pub fn followed(&mut self, approval: &str) {
        *self = Self::Followed { id: approval.to_string() };
    }

    pub fn clear(&mut self) {
        *self = Self::Idle;
    }
}

/// Characters that may reach the island inside server or model text: no
/// control characters and no bidi overrides/isolates/marks, which could make
/// a label or summary read differently from what it is. ZWNJ and ZWJ stay:
/// Persian needs them.
pub(crate) fn shown_char(c: char) -> bool {
    !c.is_control() && !matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn clamp_one_line(text: &str, max: usize) -> String {
    let joined: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = joined.chars().filter(|c| shown_char(*c)).take(max).collect();
    if joined.chars().count() > max {
        out.push('…');
    }
    out
}

/// Argument names that never reach the island, whatever the server sent.
fn secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["token", "secret", "password", "passwd", "authorization", "cookie", "credential"].iter().any(|s| k.contains(s))
        || k == "key"
        || k.ends_with("_key")
        || k.ends_with("apikey")
}

/// A copy of tool arguments with secret-looking values masked, at any depth.
pub(crate) fn redact_secrets(v: &Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), if secret_key(k) { Value::String("•••".into()) } else { redact_secrets(v) }))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_secrets).collect()),
        other => other.clone(),
    }
}

/// The string worth showing inside a nested argument (a prompt, a query…).
fn nested_text(v: &Value, depth: usize) -> Option<String> {
    let obj = v.as_object()?;
    for key in ["prompt", "text", "query", "message", "title", "url", "name"] {
        if let Some(s) = obj.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
            return Some(s.to_string());
        }
    }
    if depth == 0 {
        return None;
    }
    obj.iter().filter(|(k, _)| !secret_key(k)).find_map(|(_, v)| nested_text(v, depth - 1))
}

/// "model: flux · mode: text-to-image · input: A cinematic landscape" — the
/// proposed tool call in one line, so the decision is an informed one.
pub fn approval_summary(tool: &str, arguments: Option<&Value>) -> String {
    let mut parts = Vec::new();
    if let Some(Value::Object(args)) = arguments {
        for (key, value) in args.iter().filter(|(k, _)| !secret_key(k)).take(6) {
            let shown = match value {
                Value::String(s) if !s.trim().is_empty() => clamp_one_line(s, 80),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Array(items) if !items.is_empty() => format!("[{}]", items.len()),
                Value::Object(_) => match nested_text(value, 2) {
                    Some(text) => clamp_one_line(&text, 80),
                    None => continue,
                },
                _ => continue,
            };
            parts.push(format!("{}: {shown}", clamp_one_line(key, 30)));
        }
    }
    let summary = if parts.is_empty() { clamp_one_line(tool, 64) } else { parts.join(" · ") };
    clamp_one_line(&summary, APPROVAL_SUMMARY_CHARS)
}

/// Approval details (`GET /v1/chat/approvals/`): `{items: [...]}` or a bare array.
pub fn find_approval(data: &Value, approval: &str) -> Option<(Option<String>, Option<Value>)> {
    let items = data.get("items").and_then(Value::as_array).or_else(|| data.as_array())?;
    let item = items.iter().find(|a| str_field(a, "id").as_deref() == Some(approval))?;
    Some((opt_text(item, "tool"), item.get("arguments").cloned()))
}

struct Finished {
    text: String,
    message_id: Option<String>,
}

enum Wake {
    Nudge,
    Ws(Result<WsMsg, broadcast::error::RecvError>),
    Timer,
}

/// The next frame, or never when the turn polls.
async fn next_frame(sub: &mut Option<broadcast::Receiver<WsMsg>>) -> Result<WsMsg, broadcast::error::RecvError> {
    match sub {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Follows one turn to its end: socket frames when there is a socket, REST
/// checks of the job always (a recovery path with a socket, the only path
/// without one). The turn completes either way.
struct Follow<'a> {
    rd: &'a Roadeep,
    chat: &'a ChatState,
    turn: u64,
    epoch: u64,
    timeout: Duration,
    tracker: TurnTracker,
    sub: Option<broadcast::Receiver<WsMsg>>,
    ws_since: Instant,
    frames_seen: bool,
    reconnected: bool,
    /// When the current job started (a resumed job gets its own budget): the cap.
    started: Instant,
    cap: Duration,
    /// Last sign of life (a frame about the job, a new REST status): the idle timeout.
    last_progress: Instant,
    last_rest_status: Option<String>,
    next_check: Instant,
    poll_delay: Duration,
    failures: u32,
    awaiting: Option<(String, Instant)>,
    queue: VecDeque<Signal>,
    last_status: Option<(&'static str, Option<String>)>,
    pending_delta: Option<String>,
    last_delta: Option<Instant>,
    /// Any reply text streamed for this turn (canned server replies stream none).
    streamed: bool,
    /// MCP tools are offered: a tool call must not stream onto the island.
    hide_calls: bool,
    voice_task: bool,
}

impl<'a> Follow<'a> {
    fn new(
        rd: &'a Roadeep,
        chat: &'a ChatState,
        turn: u64,
        epoch: u64,
        job: &str,
        timeout: Duration,
        sub: Option<broadcast::Receiver<WsMsg>>,
    ) -> Self {
        let now = Instant::now();
        let first_check = if sub.is_some() { ws::SILENCE } else { POLL_START };
        Self {
            rd,
            chat,
            turn,
            epoch,
            timeout,
            tracker: TurnTracker::new(job),
            sub,
            ws_since: now,
            frames_seen: false,
            reconnected: false,
            started: now,
            cap: if timeout >= DEEP_RESEARCH_TIMEOUT { DEEP_RESEARCH_CAP } else { JOB_CAP },
            last_progress: now,
            last_rest_status: None,
            next_check: now + first_check,
            poll_delay: POLL_START,
            failures: 0,
            awaiting: None,
            queue: VecDeque::new(),
            last_status: None,
            pending_delta: None,
            last_delta: None,
            streamed: false,
            hide_calls: false,
            voice_task: false,
        }
    }

    fn emit(&self, body: StreamBody) {
        self.rd.emit_to(Some(crate::island::WINDOW_LABEL), STREAM_EVENT, StreamEvent { turn: self.turn, body });
    }

    fn stop(&self) -> Option<RoadeepError> {
        if self.chat.epoch.load(Ordering::SeqCst) != self.epoch {
            Some(RoadeepError::new(chat_codes::CANCELLED, "The conversation was reset."))
        } else if self.chat.cancelled() {
            Some(cancelled_error())
        } else {
            None
        }
    }

    async fn run(&mut self) -> Result<Finished, RoadeepError> {
        loop {
            if let Some(err) = self.stop() {
                return Err(err);
            }
            while let Some(signal) = self.queue.pop_front() {
                if let Some(out) = self.on_signal(signal).await {
                    return out;
                }
            }
            let resumed = self.awaiting.as_ref().and_then(|(id, _)| self.chat.approval.lock().unwrap().take_resumed(id));
            if let Some((job, decision)) = resumed {
                self.resume(job, decision);
                continue;
            }
            match &self.awaiting {
                None if self.last_progress.elapsed() > self.timeout || self.started.elapsed() > self.cap => {
                    return Err(self.time_out().await)
                }
                Some((_, since)) if since.elapsed() > APPROVAL_TIMEOUT => return Err(self.time_out().await),
                _ => {}
            }
            let mut wake_at = self.next_check;
            if self.pending_delta.is_some() {
                let due = self.last_delta.map_or_else(Instant::now, |at| at + DELTA_EVERY);
                wake_at = wake_at.min(due);
            }
            let wake = tokio::select! {
                biased;
                _ = self.chat.wake.notified() => Wake::Nudge,
                msg = next_frame(&mut self.sub) => Wake::Ws(msg),
                _ = tokio::time::sleep_until(wake_at.into()) => Wake::Timer,
            };
            match wake {
                Wake::Nudge => {}
                Wake::Ws(msg) => {
                    if let Some(out) = self.on_ws(msg).await {
                        return out;
                    }
                }
                Wake::Timer => {
                    self.flush_delta_if_due();
                    if Instant::now() >= self.next_check {
                        if let Some(out) = self.check_job().await {
                            return out;
                        }
                    }
                }
            }
        }
    }

    async fn on_ws(&mut self, msg: Result<WsMsg, broadcast::error::RecvError>) -> Option<Result<Finished, RoadeepError>> {
        match msg {
            Ok(WsMsg::Event(ev)) => {
                if ev.job_id.as_deref() == Some(self.tracker.job()) {
                    self.frames_seen = true;
                    self.last_progress = Instant::now();
                    if self.awaiting.is_none() {
                        self.next_check = Instant::now() + ws::SILENCE;
                    }
                }
                let signal = self.tracker.apply(&ev)?;
                self.on_signal(signal).await
            }
            Ok(WsMsg::Closed(code)) => self.on_drop(code).await,
            Err(broadcast::error::RecvError::Closed) => self.on_drop(None).await,
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                // content_so_far repairs the text; a REST check repairs the rest.
                log::line(format!("roadeep: chat turn skipped {missed} socket frames"));
                self.next_check = Instant::now();
                None
            }
        }
    }

    /// The socket dropped mid-turn: one reconnect (4401 refreshes inside), then
    /// polling. Either way the job is read once right away, since the drop may
    /// have hidden its end.
    async fn on_drop(&mut self, code: Option<u16>) -> Option<Result<Finished, RoadeepError>> {
        self.sub = None;
        self.next_check = Instant::now();
        if self.reconnected {
            log::line("roadeep: chat socket dropped again, polling");
            return None;
        }
        self.reconnected = true;
        log::line(format!(
            "roadeep: chat socket dropped{}, reconnecting",
            code.map(|c| format!(" ({c})")).unwrap_or_default()
        ));
        match self.chat.ws.subscribe(self.rd, true).await {
            Ok(rx) => {
                self.sub = Some(rx);
                self.ws_since = Instant::now();
            }
            Err(OpenError::Expired { generation }) => return Some(Err(self.rd.expired(generation, None))),
            Err(OpenError::Unavailable(why)) => log::line(format!("roadeep: chat turn polls ({why})")),
        }
        None
    }

    async fn on_signal(&mut self, signal: Signal) -> Option<Result<Finished, RoadeepError>> {
        match signal {
            Signal::Delta(text) => {
                self.streamed = true;
                self.pending_delta = Some(if self.hide_calls { tools::visible_stream_text(&text) } else { text });
                self.flush_delta_if_due();
                None
            }
            Signal::Status { kind, label } => {
                self.status(kind, label);
                None
            }
            Signal::Approval { id, tool } => {
                if self.voice_task { return Some(Err(voice_blocked())); }
                self.enter_approval(id, tool).await;
                None
            }
            Signal::Resumed { job, decision } => {
                if self.voice_task {
                    // An out-of-band server decision cannot grant a voice job
                    // authority. Track its replacement so run_step cancels it.
                    self.tracker.switch_job(&job);
                    *self.chat.job_id.lock().unwrap() = Some(job);
                    return Some(Err(voice_blocked()));
                }
                if let Some((id, _)) = &self.awaiting {
                    self.chat.approval.lock().unwrap().followed(id);
                }
                self.resume(job, decision);
                None
            }
            Signal::Done { reply, message_id } => Some(self.finish(reply, message_id).await),
            Signal::Failed(err) => Some(Err(err)),
            Signal::Cancelled => Some(Err(cancelled_error())),
        }
    }

    fn status(&mut self, kind: &'static str, label: Option<String>) {
        if self.awaiting.is_some() {
            return;
        }
        let next = Some((kind, label.clone()));
        if self.last_status != next {
            self.last_status = next;
            self.emit(StreamBody::Status { status: kind, label });
        }
    }

    fn flush_delta(&mut self) {
        if let Some(text) = self.pending_delta.take() {
            self.last_delta = Some(Instant::now());
            self.emit(StreamBody::Delta { text });
        }
    }

    fn flush_delta_if_due(&mut self) {
        if self.last_delta.map_or(true, |at| at.elapsed() >= DELTA_EVERY) {
            self.flush_delta();
        }
    }

    async fn enter_approval(&mut self, id: String, tool: Option<String>) {
        if self.awaiting.as_ref().is_some_and(|(current, _)| *current == id) {
            return;
        }
        self.flush_delta();
        self.chat.approval.lock().unwrap().wait(&id);
        self.awaiting = Some((id.clone(), Instant::now()));
        self.tracker.set_awaiting(true);
        self.last_status = None;
        // The event names the tool; the arguments only come with the list.
        let (tool, arguments) = match self.rd.request(Call::get("/v1/chat/approvals/").timeout(Duration::from_secs(10))).await {
            Ok(data) => match find_approval(&data, &id) {
                Some((listed, arguments)) => (listed.or(tool), arguments),
                None => (tool, None),
            },
            Err(err) => {
                log::line(format!("roadeep: approval details unavailable ({})", err.code));
                (tool, None)
            }
        };
        let tool = tool.map(|t| clamp_one_line(&t, 64)).unwrap_or_else(|| "tool".to_string());
        log::line("roadeep: chat turn waits for a tool approval");
        let summary = approval_summary(&tool, arguments.as_ref());
        self.emit(StreamBody::Approval { approval: ApprovalCard { id, tool, summary, call: None } });
        self.next_check = Instant::now() + APPROVAL_CHECK;
    }

    /// Same turn, new job: follow it with a fresh budget, replaying whatever it
    /// already said on the socket.
    fn resume(&mut self, job: String, decision: &'static str) {
        let approval = self.awaiting.take().map(|(id, _)| id);
        log::line(format!("roadeep: approval {decision}, following the resumed job"));
        *self.chat.job_id.lock().unwrap() = Some(job.clone());
        if let Some(approval_id) = approval {
            self.emit(StreamBody::ApprovalDone { approval_id, decision });
        }
        let now = Instant::now();
        self.started = now;
        self.last_progress = now;
        self.last_rest_status = None;
        self.ws_since = now;
        self.frames_seen = false;
        self.poll_delay = POLL_START;
        self.failures = 0;
        self.last_status = None;
        self.next_check = now + if self.sub.is_some() { ws::SILENCE } else { POLL_START };
        let replay = self.tracker.switch_job(&job);
        self.queue.extend(replay);
    }

    /// `chat.done`: its reply, else the streamed text, else the job itself.
    async fn finish(&mut self, reply: Option<String>, message_id: Option<String>) -> Result<Finished, RoadeepError> {
        self.flush_delta();
        let streamed = Some(self.tracker.text().trim().to_string()).filter(|t| !t.is_empty());
        if let Some(text) = reply.or(streamed) {
            return Ok(Finished { text, message_id });
        }
        let data = self.rd.request(Call::get(format!("/v1/chat/jobs/{}/", self.tracker.job()))).await?;
        match job_state(&data) {
            JobState::Done { text, message_id: listed } => Ok(Finished { text, message_id: message_id.or(listed) }),
            JobState::Failed(err) => Err(err),
            JobState::Pending(_) | JobState::AwaitingApproval(_) => {
                Err(RoadeepError::new(chat_codes::EMPTY_RESPONSE, "Roadeep returned an empty answer."))
            }
        }
    }

    async fn check_job(&mut self) -> Option<Result<Finished, RoadeepError>> {
        let path = format!("/v1/chat/jobs/{}/", self.tracker.job());
        match self.rd.request(Call::get(path)).await {
            Ok(data) => {
                self.failures = 0;
                let deciding = self
                    .awaiting
                    .as_ref()
                    .is_some_and(|(id, _)| self.chat.approval.lock().unwrap().in_progress(id));
                let state = job_state(&data);
                if self.voice_task && matches!(state, JobState::AwaitingApproval(_)) { return Some(Err(voice_blocked())); }
                match state {
                    // A decision is out: the paused job's own state is old news.
                    _ if deciding => {}
                    JobState::Pending(status) => {
                        if self.last_rest_status.as_deref() != Some(status.as_str()) {
                            self.last_progress = Instant::now();
                            self.last_rest_status = Some(status.clone());
                        }
                        if self.sub.is_some() && ws::stalled(self.frames_seen, self.ws_since.elapsed()) {
                            log::line("roadeep: chat socket silent about this job, polling");
                            self.sub = None;
                        }
                        if self.sub.is_none() {
                            if let Some(kind) = ws::status_from_job(&status) {
                                self.status(kind, None);
                            }
                        }
                    }
                    JobState::AwaitingApproval(Some(id)) => self.enter_approval(id, None).await,
                    JobState::AwaitingApproval(None) => {
                        return Some(Err(RoadeepError::new(
                            chat_codes::AWAITING_APPROVAL,
                            "This answer needs your approval. Approve it on roadeep.com, then ask again.",
                        )))
                    }
                    JobState::Done { text, message_id } => {
                        self.flush_delta();
                        return Some(Ok(Finished { text, message_id }));
                    }
                    JobState::Failed(err) => return Some(Err(err)),
                }
            }
            Err(err) if is_transient(&err) && self.failures < POLL_FAILURE_TOLERANCE => self.failures += 1,
            Err(err) => return Some(Err(err)),
        }
        let now = Instant::now();
        self.next_check = if self.awaiting.is_some() {
            now + APPROVAL_CHECK
        } else if self.sub.is_some() {
            now + ws::SILENCE
        } else {
            self.poll_delay = next_poll_delay(self.poll_delay);
            now + self.poll_delay
        };
        None
    }

    /// A job that times out is cancelled on the server rather than left running unread.
    async fn time_out(&mut self) -> RoadeepError {
        delete_job(self.rd, self.tracker.job(), "a timeout").await;
        RoadeepError::new(chat_codes::JOB_TIMEOUT, "Roadeep is taking too long to answer. Try again in a moment.")
    }
}

/// Posts the user's decision on the approval the current turn waits for; that
/// turn then follows the resumed job (a new job id, same turn).
pub async fn decide_approval(rd: &Roadeep, chat: &ChatState, approval_id: &str, decision: &str) -> Result<(), RoadeepError> {
    let decision: &'static str = match decision {
        "approve" => "approve",
        "reject" => "reject",
        _ => return Err(RoadeepError::validation("decision", "Unknown decision.")),
    };
    let not_found = || RoadeepError::new(chat_codes::APPROVAL_NOT_FOUND, "That approval is no longer waiting.");
    // An MCP tool the app itself asked about: decided here, nothing to post.
    if approval_id.starts_with(LOCAL_APPROVAL_PREFIX) {
        let outcome = chat.local_approval.lock().unwrap().decide(approval_id, decision);
        return match outcome {
            Ok(()) => {
                chat.wake.notify_one();
                Ok(())
            }
            Err(code) if code == chat_codes::APPROVAL_ALREADY_DECIDED => {
                Err(RoadeepError::new(code, "That approval was already decided."))
            }
            Err(_) => Err(not_found()),
        };
    }
    if !valid_server_id(approval_id) {
        return Err(not_found());
    }
    if let Err(code) = chat.approval.lock().unwrap().begin(approval_id) {
        return Err(if code == chat_codes::APPROVAL_ALREADY_DECIDED {
            RoadeepError::new(code, "That approval was already decided.")
        } else {
            not_found()
        });
    }
    let posted = rd
        .request(Call::post(format!("/v1/chat/approvals/{approval_id}/"), json!({ "decision": decision })))
        .await
        .and_then(|data| {
            server_id(&data, "job_id")?
                .ok_or_else(|| RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected response."))
        });
    match posted {
        Ok(job) => {
            let followed = chat.approval.lock().unwrap().decided(approval_id, &job, decision);
            if followed && !chat.cancelled() {
                *chat.job_id.lock().unwrap() = Some(job);
                chat.wake.notify_one();
            } else {
                // The turn ended meanwhile (stop, reset): nobody reads this job.
                delete_job(rd, &job, "a decision after the turn ended").await;
            }
            log::line(format!("roadeep: approval decision sent ({decision})"));
            Ok(())
        }
        Err(err) => {
            chat.approval.lock().unwrap().failed(approval_id);
            log::line(format!("roadeep: approval decision failed ({})", err.code));
            Err(err)
        }
    }
}

/// Whether a turn is in flight, and which: a rebuilt island re-attaches to it
/// (its "chat-stream" events carry that turn).
#[tauri::command]
pub fn chat_turn_state(chat: tauri::State<'_, ChatState>) -> TurnState {
    chat.turn_state()
}

#[tauri::command]
pub async fn chat_approval_decide(
    roadeep: tauri::State<'_, Roadeep>,
    chat: tauri::State<'_, ChatState>,
    approval_id: String,
    decision: String,
) -> Result<(), RoadeepError> {
    decide_approval(&roadeep, &chat, &approval_id, &decision).await
}

/// Polls a job until it ends, `stop` says so, or `timeout` passes. A job that
/// times out is cancelled on the server rather than left running unread.
async fn poll_job(
    rd: &Roadeep,
    job_id: &str,
    timeout: Duration,
    stop: impl Fn() -> Option<RoadeepError>,
) -> Result<String, RoadeepError> {
    let path = format!("/v1/chat/jobs/{job_id}/");
    let started = Instant::now();
    let mut delay = POLL_START;
    let mut failures = 0;
    loop {
        tokio::time::sleep(delay).await;
        if let Some(err) = stop() {
            return Err(err);
        }
        if started.elapsed() > timeout {
            delete_job(rd, job_id, "a timeout").await;
            return Err(RoadeepError::new(
                chat_codes::JOB_TIMEOUT,
                "Roadeep is taking too long to answer. Try again in a moment.",
            ));
        }
        match rd.request(Call::get(path.clone())).await {
            Ok(data) => {
                failures = 0;
                match job_step(&data) {
                    JobStep::Pending => {}
                    JobStep::Done(text) => return Ok(text),
                    JobStep::Failed(err) => return Err(err),
                }
            }
            Err(err) if is_transient(&err) && failures < POLL_FAILURE_TOLERANCE => failures += 1,
            Err(err) => return Err(err),
        }
        delay = next_poll_delay(delay);
    }
}

/// Cancels the in-flight turn. Before the job exists (upload or submit still
/// out) the flag alone stops it; send() deletes the job as soon as it has one.
/// A job that already finished is fine.
pub async fn cancel(rd: &Roadeep, chat: &ChatState) -> Result<(), RoadeepError> {
    chat.cancel_requested.store(true, Ordering::SeqCst);
    chat.wake.notify_one();
    let Some(job_id) = chat.job_id.lock().unwrap().clone() else { return Ok(()) };
    match rd.request(Call::delete(format!("/v1/chat/jobs/{job_id}/"))).await {
        Ok(_) => Ok(()),
        Err(err) if err.status == Some(409) || err.code == "JOB_ALREADY_FINISHED" || err.code == "JOB_NOT_FOUND" => Ok(()),
        Err(err) => Err(err),
    }
}

// ── Catalogues ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoadeepModel {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub provider: Option<String>,
    /// The server's global fallback model (`default: true`).
    pub is_default: bool,
    pub vision: bool,
    pub file_input: bool,
    /// Feature gates (see `capability`): unknown counts as yes, except deep research.
    pub reasoning: bool,
    /// native_web_search or openrouter_web_search.
    pub web_search: bool,
    pub deep_research: bool,
    /// Function calling.
    pub tools: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoadeepAgent {
    pub id: String,
    pub title: String,
    pub short_description: Option<String>,
    pub icon_url: Option<String>,
    pub starter_prompts: Vec<String>,
    /// "public" | "private" as the server says it; None when it does not.
    pub visibility: Option<String>,
}

/// Roadeep's agents split the way the settings window lists them: the ones
/// only this account can use, and everyone's.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoadeepAgentCatalog {
    pub exclusive: Vec<RoadeepAgent>,
    pub public: Vec<RoadeepAgent>,
}

fn opt_text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// `data.items[]` (today) or a bare array. Malformed items are skipped.
pub fn parse_models(data: &Value) -> Vec<RoadeepModel> {
    let items = data.get("items").and_then(Value::as_array).or_else(|| data.as_array());
    items
        .into_iter()
        .flatten()
        .filter_map(|m| {
            // This is the chat catalogue. Unknown metadata remains compatible;
            // only an explicit server restriction removes an entry.
            if m.get("enabled").and_then(Value::as_bool) == Some(false)
                || m.get("available").and_then(Value::as_bool) == Some(false)
                || m.get("capability_flags").and_then(|f| f.get("text")).and_then(Value::as_bool) == Some(false)
            { return None; }
            let id = opt_text(m, "id")?;
            let caps: Vec<&str> = m
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let flag = |k: &str| {
                m.get("capability_flags").and_then(|f| f.get(k)).and_then(Value::as_bool).unwrap_or(false)
                    || caps.contains(&k)
            };
            Some(RoadeepModel {
                display_name: opt_text(m, "display_name").unwrap_or_else(|| id.clone()),
                description: opt_text(m, "description"),
                provider: opt_text(m, "provider"),
                is_default: m.get("default").and_then(Value::as_bool).unwrap_or(false),
                vision: flag("vision"),
                file_input: flag("file_input"),
                reasoning: capability(m, &caps, "reasoning", true),
                web_search: capability(m, &caps, "native_web_search", true)
                    || capability(m, &caps, "openrouter_web_search", true),
                deep_research: capability(m, &caps, "deep_research", false),
                tools: capability(m, &caps, "tools", true),
                id,
            })
        })
        .collect()
}

fn parse_agent(a: &Value) -> Option<RoadeepAgent> {
    let id = str_field(a, "id").filter(|id| valid_agent_id(id))?;
    Some(RoadeepAgent {
        title: opt_text(a, "title").or_else(|| opt_text(a, "slug")).unwrap_or_else(|| id.clone()),
        short_description: opt_text(a, "short_description"),
        icon_url: opt_text(a, "icon_url"),
        starter_prompts: a
            .get("starter_prompts")
            .and_then(Value::as_array)
            .map(|p| p.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
            .unwrap_or_default(),
        visibility: opt_text(a, "visibility").map(|v| v.to_ascii_lowercase()),
        id,
    })
}

/// `data.agents[]` (today) or a bare array. Malformed items are skipped.
pub fn parse_agents(data: &Value) -> Vec<RoadeepAgent> {
    let items = data.get("agents").and_then(Value::as_array).or_else(|| data.as_array());
    items.into_iter().flatten().filter_map(parse_agent).collect()
}

/// A group or visibility name → exclusive? Anything not clearly private is public.
fn is_exclusive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("priv") || n.contains("exclusive")
}

/// Adds agents to the catalogue, first group wins on a duplicate id.
fn add_to_catalog(cat: &mut RoadeepAgentCatalog, agents: Vec<RoadeepAgent>, group: Option<&str>) {
    for agent in agents {
        if cat.exclusive.iter().chain(cat.public.iter()).any(|a| a.id == agent.id) {
            continue;
        }
        let exclusive = group.or(agent.visibility.as_deref()).is_some_and(is_exclusive);
        if exclusive {
            cat.exclusive.push(agent);
        } else {
            cat.public.push(agent);
        }
    }
}

/// The agents inside one group value: a bare array, or `{agents|items|results: [...]}`.
fn group_agents(v: &Value) -> Vec<RoadeepAgent> {
    let list = v
        .as_array()
        .or_else(|| ["agents", "items", "results"].iter().find_map(|k| v.get(*k).and_then(Value::as_array)));
    list.into_iter().flatten().filter_map(parse_agent).collect()
}

/// `GET /v1/agents/catalog/` → `data.groups`, grouped by public / private.
/// The doc names the grouping but not its exact shape, so both an object keyed
/// by visibility and an array of `{visibility|key|slug|name, agents}` are read.
/// None = no recognisable groups (the caller falls back to the plain list).
pub fn parse_catalog(data: &Value) -> Option<RoadeepAgentCatalog> {
    let groups = data.get("groups")?;
    let mut cat = RoadeepAgentCatalog::default();
    match groups {
        Value::Object(map) => {
            for (key, v) in map {
                add_to_catalog(&mut cat, group_agents(v), Some(key));
            }
        }
        Value::Array(items) => {
            for g in items {
                let name = ["visibility", "key", "slug", "name", "id"].iter().find_map(|k| opt_text(g, k));
                add_to_catalog(&mut cat, group_agents(g), name.as_deref());
            }
        }
        _ => return None,
    }
    Some(cat)
}

/// The plain list split by each agent's own `visibility`.
pub fn catalog_from_list(agents: Vec<RoadeepAgent>) -> RoadeepAgentCatalog {
    let mut cat = RoadeepAgentCatalog::default();
    add_to_catalog(&mut cat, agents, None);
    cat
}

pub async fn models(rd: &Roadeep, chat: &ChatState) -> Result<Vec<RoadeepModel>, RoadeepError> {
    let generation = rd.session.generation();
    let data = rd.request(Call::get("/v1/chat/models/")).await?;
    let list = parse_models(&data);
    // A sign-out or sign-in meanwhile: this catalogue belongs to the previous session.
    if rd.session.generation() == generation {
        *chat.known_models.lock().unwrap() = list.clone();
    }
    Ok(list)
}

pub async fn agents(rd: &Roadeep) -> Result<Vec<RoadeepAgent>, RoadeepError> {
    let data = rd.request(Call::get("/v1/agents/")).await?;
    Ok(parse_agents(&data))
}

/// The catalogue endpoint when the server has it, else the plain list split by
/// visibility. Throttling and sign-in errors are not worth a second request.
pub async fn agent_catalog(rd: &Roadeep) -> Result<RoadeepAgentCatalog, RoadeepError> {
    match rd.request(Call::get("/v1/agents/catalog/")).await {
        Ok(data) => {
            if let Some(cat) = parse_catalog(&data) {
                return Ok(cat);
            }
            log::line("roadeep: agent catalogue had no groups, using the plain list");
        }
        Err(err) if err.code == codes::THROTTLED || err.code == codes::NOT_SIGNED_IN || err.code == codes::SESSION_EXPIRED => {
            return Err(err);
        }
        Err(err) => log::line(format!("roadeep: agent catalogue unavailable ({}), using the plain list", err.code)),
    }
    Ok(catalog_from_list(agents(rd).await?))
}

#[tauri::command]
pub async fn roadeep_agent_catalog(roadeep: tauri::State<'_, Roadeep>) -> Result<RoadeepAgentCatalog, RoadeepError> {
    agent_catalog(&roadeep).await
}

pub fn parse_locks(data: &Value) -> PlanLocks {
    let lock = |k: &str| data.get(k).and_then(Value::as_bool) == Some(true);
    PlanLocks {
        chatbot: lock("chatbot_locked"),
        agents: lock("agents_locked"),
        web_search: lock("web_search_locked"),
        reasoning: lock("reasoning_locked"),
        file_upload: lock("file_upload_locked"),
    }
}

/// `GET /v1/users/plan/me/` → (plan name, wallet units). Counters were strings
/// on older deployments.
pub fn parse_plan(data: &Value) -> (Option<String>, Option<u64>) {
    let units = match data.get("wallet_units") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|n| n.is_finite())
    .map(|n| n.max(0.0).floor() as u64);
    (opt_text(data, "plan_name"), units)
}

/// Plan locks (required) and the plan/balance (best effort: the card simply
/// leaves the balance out when it cannot be read).
pub async fn account_status(rd: &Roadeep, chat: &ChatState) -> Result<AccountStatus, RoadeepError> {
    let generation = rd.session.generation();
    let profile = rd.request(Call::get("/v1/user/profile/")).await?;
    let locks = parse_locks(&profile);
    *chat.locks.lock().unwrap() = Some(locks.clone());
    let (plan_name, wallet_units) = match rd.request(Call::get("/v1/users/plan/me/")).await {
        Ok(plan) => {
            let (plan_name, units) = parse_plan(&plan);
            rd.store_balance(generation, &Balance { units, plan: plan_name.clone() });
            (plan_name, units)
        }
        Err(err) => {
            log::line(format!("roadeep: plan unavailable ({}) {}", err.code, err.request_id.as_deref().unwrap_or("")));
            (None, None)
        }
    };
    Ok(AccountStatus { locks, plan_name, wallet_units })
}

// ── Agent builder ─────────────────────────────────────────────────────────────
//
// The settings window's wizard asks the model the user picked to draft a local
// agent. Each draft is its own one-off thread — never the island's — and the
// reply must be a single JSON object, which is read defensively: models wrap
// JSON in fences or prose, and whatever they send is clamped to the limits of
// the local agent store before the user ever sees it.

const MAX_GOAL_CHARS: usize = 4000;
const MAX_FEEDBACK_CHARS: usize = 2000;
const DRAFT_TIMEOUT: Duration = Duration::from_secs(180);
const DRAFT_STARTER_PROMPTS: usize = 3;
/// JSON candidates tried in one reply before giving up (bounds the work).
const MAX_JSON_CANDIDATES: usize = 64;
const DRAFT_NUDGE: &str = "Return only the JSON object described above. No code fences, no explanation.";

pub mod draft_codes {
    /// The model answered twice without a usable JSON draft.
    pub const INVALID: &str = "AGENT_DRAFT_INVALID";
    /// A draft is already being written.
    pub const BUSY: &str = "AGENT_DRAFT_BUSY";
}

/// One draft at a time; Stop cancels its job.
#[derive(Default)]
pub struct DraftState {
    job_id: Mutex<Option<String>>,
    cancel_requested: AtomicBool,
    running: AtomicBool,
}

/// Clears `running` however the draft ends.
struct Running<'a>(&'a AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// What the model proposes, already within the local agent limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSuggestion {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub starter_prompts: Vec<String>,
    pub web_search: bool,
    #[serde(default)]
    pub suggested_color: Option<String>,
}

/// Single line, no control characters, at most `max` characters.
fn clamp_line(text: &str, max: usize) -> String {
    let joined: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// Multi-line text: newlines and tabs kept, other control characters dropped.
fn clamp_block(text: &str, max: usize) -> String {
    let normalized = text.replace("\r\n", "\n");
    let kept: String = normalized.trim().chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
    kept.chars().take(max).collect::<String>().trim_end().to_string()
}

fn first_key<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| v.get(*k))
}

/// Finds the first complete JSON object in a reply: skips fences, prose and
/// anything before or after; respects braces inside strings.
pub fn extract_json_object(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    let mut tried = 0;
    for (start, _) in text.match_indices('{') {
        if tried >= MAX_JSON_CANDIDATES {
            break;
        }
        tried += 1;
        let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
        for (offset, &b) in bytes[start..].iter().enumerate() {
            if in_string {
                match b {
                    _ if escaped => escaped = false,
                    b'\\' => escaped = true,
                    b'"' => in_string = false,
                    _ => {}
                }
                continue;
            }
            match b {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let slice = &text[start..start + offset + 1];
                        if let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(slice) {
                            return Some(value);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Reads a drafted agent out of the model's JSON. Over-long fields are cut to
/// the store's limits; a missing name or instructions makes it unusable.
pub fn parse_suggestion(v: &Value) -> Result<AgentSuggestion, &'static str> {
    let text = |keys: &[&str]| first_key(v, keys).and_then(Value::as_str).unwrap_or("").to_string();
    let name = clamp_line(&text(&["name", "title"]), agents::MAX_NAME_CHARS);
    if name.is_empty() {
        return Err("no name");
    }
    let instructions = match first_key(v, &["instructions", "systemPrompt", "system_prompt"]) {
        Some(Value::String(s)) => s.clone(),
        // Some models answer with a list of rules.
        Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    };
    let instructions = clamp_block(&instructions, agents::MAX_INSTRUCTIONS_CHARS);
    if instructions.is_empty() {
        return Err("no instructions");
    }
    let starter_prompts = first_key(v, &["starterPrompts", "starter_prompts"])
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|p| clamp_line(p, agents::MAX_STARTER_PROMPT_CHARS))
                .filter(|p| !p.is_empty())
                .take(DRAFT_STARTER_PROMPTS)
                .collect()
        })
        .unwrap_or_default();
    let web_search = match first_key(v, &["webSearch", "web_search"]) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.trim().eq_ignore_ascii_case("true"),
        _ => false,
    };
    let suggested_color = first_key(v, &["suggestedColor", "suggested_color", "color"])
        .and_then(Value::as_str)
        .and_then(agents::normalize_color);
    Ok(AgentSuggestion {
        name,
        description: clamp_line(&text(&["description", "shortDescription", "short_description"]), agents::MAX_DESCRIPTION_CHARS),
        instructions,
        starter_prompts,
        web_search,
        suggested_color,
    })
}

/// The inputs of one draft request, trimmed and checked.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftInput {
    pub goal: String,
    pub model: String,
    pub language: String,
    pub previous: Option<AgentSuggestion>,
    pub feedback: Option<String>,
}

pub fn check_draft_input(
    goal: &str,
    model: &str,
    language: &str,
    previous: Option<AgentSuggestion>,
    feedback: Option<&str>,
) -> Result<DraftInput, RoadeepError> {
    let goal = goal.trim().replace("\r\n", "\n");
    if goal.is_empty() || goal.chars().count() > MAX_GOAL_CHARS {
        return Err(RoadeepError::validation("goal", "Describe the agent in 1 to 4000 characters."));
    }
    let model = model.trim().to_string();
    let model_ok = model.len() <= 200
        && model.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'));
    if !model_ok {
        return Err(RoadeepError::validation("model", "Unknown model."));
    }
    let language = if language == "en" { "en" } else { "fa" }.to_string();
    let feedback = feedback.map(|f| f.trim().replace("\r\n", "\n")).filter(|f| !f.is_empty());
    if let Some(f) = &feedback {
        if f.chars().count() > MAX_FEEDBACK_CHARS {
            return Err(RoadeepError::validation("feedback", "Keep the feedback under 2000 characters."));
        }
        if previous.is_none() {
            return Err(RoadeepError::validation("feedback", "There is no draft to refine yet."));
        }
    }
    Ok(DraftInput { goal, model, language, previous, feedback })
}

/// The server reads every user message, local agents' instructions included,
/// for "save this for later" wording and answers such a message with a canned
/// reply instead of the model. This prompt is itself a user message, so it
/// describes that wording without quoting any of it.
const DRAFT_NO_STORAGE_VERBS: &str = "In the instructions, never tell the agent to retain, store or hold on to \
     anything for later, in any language: the platform reads such verbs as a command to its own storage feature \
     and then does not answer. State the rules plainly instead (for example \"Always answer in short bullet points.\").";

/// The one message the model gets. English instructions work best across
/// models; the language line decides what the agent itself is written in.
pub fn draft_prompt(input: &DraftInput) -> String {
    let language = if input.language == "fa" { "Persian (Farsi), natural and fluent" } else { "English" };
    let mut prompt = format!(
        "You design custom AI assistants (\"agents\"). From the user's goal below, write the agent's configuration.\n\
         Reply with ONLY one JSON object, no code fences and no other text, with exactly these keys:\n\
         {{\"name\": string (a short name, at most 40 characters), \
         \"description\": string (one sentence, at most 160 characters), \
         \"instructions\": string (the agent's instructions, addressed to the agent as \"you\", 80 to 400 words: its role, tone, how it answers, what it must avoid), \
         \"starterPrompts\": array of exactly 3 short example requests a user might send it (each at most 100 characters), \
         \"webSearch\": boolean (true only if the agent needs current information from the web), \
         \"suggestedColor\": string (a hex colour like \"#7C5CFF\" that suits the agent)}}\n\
         Write name, description, instructions and starterPrompts in {language}.\n\
         {DRAFT_NO_STORAGE_VERBS}\n\n\
         User's goal:\n<<<\n{}\n>>>",
        input.goal
    );
    if let (Some(previous), Some(feedback)) = (&input.previous, &input.feedback) {
        let current = serde_json::to_string_pretty(previous).unwrap_or_default();
        prompt.push_str(&format!(
            "\n\nCurrent draft:\n{current}\n\n\
             Revise the draft according to this feedback. Keep everything the feedback does not mention:\n<<<\n{feedback}\n>>>"
        ));
    }
    prompt
}

/// Submits one turn and polls it, honouring Stop. The draft's thread id is
/// noted in `thread` as soon as the server names it, so it can be deleted
/// however the draft ends.
async fn draft_turn(
    rd: &Roadeep,
    state: &DraftState,
    body: Map<String, Value>,
    thread: &mut Option<String>,
) -> Result<String, RoadeepError> {
    if state.cancel_requested.load(Ordering::SeqCst) {
        return Err(cancelled_error());
    }
    let accepted = rd.request(Call::post("/v1/chat/async/", Value::Object(body))).await?;
    if thread.is_none() {
        *thread = server_id(&accepted, "thread_id")?;
    }
    let job_id = server_id(&accepted, "job_id")?
        .ok_or_else(|| RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected response."))?;
    *state.job_id.lock().unwrap() = Some(job_id.clone());
    let result = if state.cancel_requested.load(Ordering::SeqCst) {
        delete_job(rd, &job_id, "a stop during submit").await;
        Err(cancelled_error())
    } else {
        let stop = || state.cancel_requested.load(Ordering::SeqCst).then(cancelled_error);
        poll_job(rd, &job_id, DRAFT_TIMEOUT, stop).await
    };
    *state.job_id.lock().unwrap() = None;
    result
}

/// Drafts (or, with `previous` + `feedback`, revises) a local agent. One retry
/// with a "JSON only" nudge, in the same thread, when the reply is unusable.
pub async fn draft_agent(rd: &Roadeep, chat: &ChatState, state: &DraftState, input: DraftInput) -> Result<AgentSuggestion, RoadeepError> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(RoadeepError::new(draft_codes::BUSY, "A draft is already being written."));
    }
    let _running = Running(&state.running);
    state.cancel_requested.store(false, Ordering::SeqCst);
    let started = Instant::now();
    log::line(format!(
        "agents: draft started (goal {} chars, {}, lang {})",
        input.goal.chars().count(),
        if input.feedback.is_some() { "refine" } else { "new" },
        input.language
    ));

    let known: Vec<String> = chat.known_models.lock().unwrap().iter().map(|m| m.id.clone()).collect();
    let prompt = draft_prompt(&input);
    let mut body = Map::new();
    body.insert("message".into(), Value::String(prompt.clone()));
    if let Some(model) = effective_model(&input.model, &known) {
        body.insert("model".into(), Value::String(model));
    }
    body.insert("source".into(), Value::String("chat".into()));

    // Each draft is a one-off thread; it goes once the draft is over.
    let mut thread: Option<String> = None;
    let outcome = async {
        let reply = draft_turn(rd, state, body.clone(), &mut thread).await?;
        let first = extract_json_object(&reply).ok_or("no JSON object").and_then(|v| parse_suggestion(&v));
        let why = match first {
            Ok(s) => return Ok(s),
            Err(why) => why,
        };
        log::line(format!("agents: draft reply unusable ({why}, {} chars), asking again", reply.chars().count()));
        let mut retry = Map::new();
        match thread.clone() {
            Some(thread) => {
                retry.insert("message".into(), Value::String(DRAFT_NUDGE.into()));
                retry.insert("thread_id".into(), Value::String(thread));
            }
            None => {
                retry = body;
                retry.insert("message".into(), Value::String(format!("{prompt}\n\n{DRAFT_NUDGE}")));
            }
        }
        retry.insert("source".into(), Value::String("chat".into()));
        let reply = draft_turn(rd, state, retry, &mut thread).await?;
        extract_json_object(&reply).ok_or("no JSON object").and_then(|v| parse_suggestion(&v)).map_err(|why| {
            log::line(format!("agents: draft reply unusable again ({why})"));
            RoadeepError::new(draft_codes::INVALID, "The model did not return a usable draft. Try again or pick another model.")
        })
    }
    .await;
    if let Some(thread) = thread {
        threads::delete_quietly(rd, &thread, "agent draft").await;
    }
    match &outcome {
        Ok(_) => log::line(format!("agents: draft ready in {:.1}s", started.elapsed().as_secs_f32())),
        Err(err) => log::line(format!(
            "agents: draft ended with {} {}",
            err.code,
            err.request_id.as_deref().unwrap_or("")
        )),
    }
    outcome
}

/// Stops the draft being written, if any. A job that already ended is fine.
pub async fn cancel_draft(rd: &Roadeep, state: &DraftState) {
    state.cancel_requested.store(true, Ordering::SeqCst);
    let job = state.job_id.lock().unwrap().clone();
    if let Some(job_id) = job {
        delete_job(rd, &job_id, "a stop").await;
    }
}

#[tauri::command]
pub async fn agent_draft(
    roadeep: tauri::State<'_, Roadeep>,
    chat: tauri::State<'_, ChatState>,
    draft: tauri::State<'_, DraftState>,
    goal: String,
    model: String,
    language: String,
    previous: Option<AgentSuggestion>,
    feedback: Option<String>,
) -> Result<AgentSuggestion, RoadeepError> {
    let input = check_draft_input(&goal, &model, &language, previous, feedback.as_deref())?;
    draft_agent(&roadeep, &chat, &draft, input).await
}

#[tauri::command]
pub async fn agent_draft_cancel(roadeep: tauri::State<'_, Roadeep>, draft: tauri::State<'_, DraftState>) -> Result<(), RoadeepError> {
    cancel_draft(&roadeep, &draft).await;
    Ok(())
}

/// Plan locks and balance for the settings window's account card.
#[tauri::command]
pub async fn roadeep_profile_locks(
    roadeep: tauri::State<'_, Roadeep>,
    chat: tauri::State<'_, ChatState>,
) -> Result<AccountStatus, RoadeepError> {
    account_status(&roadeep, &chat).await
}

/// Units and plan, at most a minute old ("roadeep-balance" pushes fresh ones).
#[tauri::command]
pub async fn roadeep_balance(roadeep: tauri::State<'_, Roadeep>) -> Result<Balance, RoadeepError> {
    roadeep.balance(BALANCE_MAX_AGE).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn existing_preference_choice_resets_a_different_historical_thread() {
        let chat = ChatState::default();
        chat.open_thread("historical".into(), Some("historical-model".into()));
        *chat.known_models.lock().unwrap() = parse_models(&json!([{"id":"preferred"}]));
        let mut initial = Settings::default();
        initial.model = "preferred".into();
        let settings = Mutex::new(initial);
        chat.change_model("preferred", &settings, |_| Ok(())).unwrap();
        assert!(chat.current_thread().is_none());
        assert_eq!(settings.lock().unwrap().model, "preferred");
    }

    #[test]
    fn model_choice_commits_then_starts_new_thread() {
        let chat = ChatState::default();
        chat.open_thread("existing".into(), Some("old".into()));
        *chat.known_models.lock().unwrap() = parse_models(&json!([{"id":"next"}]));
        let settings = Mutex::new(Settings::default());
        let updated = chat.change_model("next", &settings, |_| Ok(())).unwrap();
        assert_eq!(updated.model, "next");
        assert_eq!(settings.lock().unwrap().model, "next");
        assert_eq!(chat.current_thread(), None);
        assert!(!chat.turn_state().busy);
    }

    #[test]
    fn model_choice_failure_preserves_thread_and_settings() {
        let chat = ChatState::default();
        chat.open_thread("existing".into(), Some("old".into()));
        *chat.known_models.lock().unwrap() = parse_models(&json!([{"id":"next"}]));
        let settings = Mutex::new(Settings::default());
        let before = settings.lock().unwrap().model.clone();
        assert_eq!(chat.change_model("next", &settings, |_| Err(())).unwrap_err().code, "SETTINGS_SAVE_FAILED");
        assert_eq!(settings.lock().unwrap().model, before);
        assert_eq!(chat.current_thread().as_deref(), Some("existing"));
        assert_eq!(chat.thread_model.lock().unwrap().as_deref(), Some("old"));
        assert!(!chat.turn_state().busy);
    }

    #[test]
    fn model_choice_busy_and_unknown_ids_cannot_mutate_or_cancel() {
        let chat = ChatState::default();
        chat.open_thread("existing".into(), Some("old".into()));
        let settings = Mutex::new(Settings::default());
        let running = Busy::claim(&chat, 7).unwrap();
        assert_eq!(chat.change_model("", &settings, |_| panic!("must not save")).unwrap_err().code, chat_codes::CHAT_BUSY);
        assert_eq!(chat.current_thread().as_deref(), Some("existing"));
        assert_eq!(chat.turn_state().turn, Some(7));
        assert!(!chat.cancel_requested.load(Ordering::SeqCst));
        drop(running);
        assert_eq!(chat.change_model("unlisted", &settings, |_| panic!("must not save")).unwrap_err().code, "MODEL_UNAVAILABLE");
        assert_eq!(chat.current_thread().as_deref(), Some("existing"));
        assert!(!chat.turn_state().busy);
    }

    #[test]
    fn model_catalogue_respects_explicit_restrictions_without_name_guesses() {
        let models = parse_models(&json!([
            {"id":"arbitrary/image-name"},
            {"id":"disabled", "enabled": false},
            {"id":"locked", "available": false},
            {"id":"nontext", "capability_flags": {"text": false}},
            {"id":"text", "capability_flags": {"text": true}}
        ]));
        assert_eq!(models.iter().map(|m|m.id.as_str()).collect::<Vec<_>>(), vec!["arbitrary/image-name", "text"]);
    }

    use super::*;

    #[test]
    fn persian_suffix_only_on_the_first_turn_in_persian() {
        assert_eq!(compose_message("salam", None, true, "fa", None, None), format!("salam{PERSIAN_SUFFIX}"));
        assert_eq!(compose_message("salam", None, false, "fa", None, None), "salam");
        assert_eq!(compose_message("hi", None, true, "en", None, None), "hi");
    }

    #[test]
    fn window_context_prefixes_the_first_message_only() {
        let ctx = ChatContext::Window { app_name: "Code".into(), title: "main.rs".into(), url: Some("https://x".into()) };
        assert_eq!(
            compose_message("why?", Some(&ctx), true, "en", None, None),
            "Context — App: Code, Window: main.rs, URL: https://x\n\nwhy?"
        );
        assert_eq!(compose_message("and?", Some(&ctx), false, "en", None, None), "and?");
        let ctx = ChatContext::Window { app_name: "Code".into(), title: "t".into(), url: None };
        assert_eq!(
            compose_message("q", Some(&ctx), true, "fa", None, None),
            format!("Context — App: Code, Window: t\n\nq{PERSIAN_SUFFIX}")
        );
        // a file context adds no text: the attachment carries it
        let file = ChatContext::File { name: "a.pdf".into(), path: "x".into() };
        assert_eq!(compose_message("q", Some(&file), true, "en", None, None), "q");
    }

    fn local(id: &str) -> LocalAgent {
        LocalAgent {
            id: id.into(),
            name: "Editor".into(),
            instructions: "Answer like a strict editor.".into(),
            model: String::new(),
            web_search: false,
            base_agent_id: None,
            description: String::new(),
            color: String::new(),
            starter_prompts: Vec::new(),
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn local_agent_instructions_lead_the_first_message_only() {
        let first = compose_message("fix this", None, true, "en", Some("  Answer like a strict editor.\n"), None);
        assert_eq!(first, "[Agent instructions]\nAnswer like a strict editor.\n[/Agent instructions]\n\nfix this");
        assert_eq!(compose_message("again", None, false, "en", Some("x"), None), "again");
        assert_eq!(compose_message("q", None, true, "en", Some("   "), None), "q", "blank instructions add nothing");

        // Order on a new thread: instructions, window context, question, Persian suffix.
        let ctx = ChatContext::Window { app_name: "Code".into(), title: "t".into(), url: None };
        assert_eq!(
            compose_message("q", Some(&ctx), true, "fa", Some("brief"), None),
            format!("[Agent instructions]\nbrief\n[/Agent instructions]\n\nContext — App: Code, Window: t\n\nq{PERSIAN_SUFFIX}")
        );
    }

    #[test]
    fn agent_parameter_resolution() {
        let store = |id: &str| (id == "abc-1").then(|| local("abc-1"));
        assert_eq!(resolve_agent(None, store).unwrap(), AgentRef::None);
        assert_eq!(resolve_agent(Some("  "), store).unwrap(), AgentRef::None);
        assert_eq!(
            resolve_agent(Some("3f1c2b9e-0000-4000-8000-123456789abc"), store).unwrap(),
            AgentRef::Roadeep("3f1c2b9e-0000-4000-8000-123456789abc".into())
        );
        assert_eq!(resolve_agent(Some("local:abc-1"), store).unwrap(), AgentRef::Local(local("abc-1")));
        assert_eq!(resolve_agent(Some("local:gone"), store).unwrap_err().code, agents::agent_codes::NOT_FOUND);
        assert_eq!(resolve_agent(Some("../x"), store).unwrap_err().code, codes::VALIDATION);
    }

    fn model(web: bool, reasoning: bool, deep: bool) -> RoadeepModel {
        RoadeepModel {
            id: "m".into(),
            display_name: "m".into(),
            description: None,
            provider: None,
            is_default: true,
            vision: false,
            file_input: false,
            reasoning,
            web_search: web,
            deep_research: deep,
            tools: true,
        }
    }

    #[test]
    fn feature_fields_respect_exclusivity_capabilities_and_locks() {
        let flags = |web, reasoning, deep| ToolFlags {
            web_search: web,
            reasoning,
            reasoning_effort: "medium".into(),
            deep_research: deep,
        };
        let keys = |m: Map<String, Value>| m.keys().cloned().collect::<Vec<_>>();
        let all = model(true, true, true);

        assert!(tool_fields(&flags(false, false, false), Some(&all), None).is_empty());
        assert_eq!(keys(tool_fields(&flags(true, false, false), Some(&all), None)), ["web_search"]);
        let r = tool_fields(&flags(false, true, false), Some(&all), None);
        assert_eq!(r.get("reasoning"), Some(&Value::Bool(true)));
        assert_eq!(r.get("reasoning_effort"), Some(&Value::String("medium".into())));
        // exclusivity: web search beats reasoning, deep research beats both
        assert_eq!(keys(tool_fields(&flags(true, true, false), Some(&all), None)), ["web_search"]);
        assert_eq!(keys(tool_fields(&flags(true, true, true), Some(&all), None)), ["deep_research"]);

        // capabilities: a model without deep research falls back to web search
        assert_eq!(keys(tool_fields(&flags(true, false, true), Some(&model(true, true, false)), None)), ["web_search"]);
        assert!(tool_fields(&flags(true, false, false), Some(&model(false, true, false)), None).is_empty());
        assert!(tool_fields(&flags(false, true, false), Some(&model(true, false, false)), None).is_empty());
        // no catalogue: trust the settings
        assert_eq!(keys(tool_fields(&flags(false, false, true), None, None)), ["deep_research"]);

        // plan locks
        let locked = PlanLocks { web_search: true, ..Default::default() };
        assert!(tool_fields(&flags(true, false, true), Some(&all), Some(&locked)).is_empty());
        let locked = PlanLocks { reasoning: true, ..Default::default() };
        assert!(tool_fields(&flags(false, true, false), Some(&all), Some(&locked)).is_empty());

        // an unknown effort never reaches the wire
        let mut odd = flags(false, true, false);
        odd.reasoning_effort = "xhigh".into();
        assert_eq!(tool_fields(&odd, None, None).get("reasoning_effort"), Some(&Value::String("low".into())));
    }

    #[test]
    fn locks_and_plan_parsing() {
        let locks = parse_locks(&json!({ "web_search_locked": true, "reasoning_locked": "yes", "agents_locked": false }));
        assert_eq!(locks, PlanLocks { web_search: true, ..Default::default() });
        assert_eq!(parse_plan(&json!({ "plan_name": " Pro ", "wallet_units": 42850.7 })), (Some("Pro".into()), Some(42850)));
        assert_eq!(parse_plan(&json!({ "wallet_units": "120" })), (None, Some(120)));
        assert_eq!(parse_plan(&json!({ "wallet_units": -5 })), (None, Some(0)));
        assert_eq!(parse_plan(&json!({})), (None, None));
    }

    #[test]
    fn model_selection() {
        let known = vec!["openrouter-openai-gpt-5".to_string()];
        assert_eq!(effective_model("", &known), None);
        assert_eq!(effective_model("  ", &[]), None);
        assert_eq!(effective_model("claude-opus-5", &[]), None);
        assert_eq!(effective_model("openrouter-openai-gpt-5", &known).as_deref(), Some("openrouter-openai-gpt-5"));
        assert_eq!(effective_model("gone-model", &known), None);
        // catalogue not loaded yet: trust the setting
        assert_eq!(effective_model("some-model", &[]).as_deref(), Some("some-model"));
    }

    #[test]
    fn job_steps() {
        assert_eq!(job_step(&json!({ "status": "queued" })), JobStep::Pending);
        assert_eq!(job_step(&json!({ "status": "generation_pending" })), JobStep::Pending);
        assert_eq!(job_step(&json!({ "status": "done", "content": " Hi \n" })), JobStep::Done("Hi".into()));
        let JobStep::Failed(e) = job_step(&json!({ "status": "done", "content": "" })) else { panic!() };
        assert_eq!(e.code, chat_codes::EMPTY_RESPONSE);
        let JobStep::Failed(e) = job_step(&json!({ "status": "error", "error": "quota" })) else { panic!() };
        assert_eq!((e.code.as_str(), e.message.as_str()), (chat_codes::LLM_ERROR, "quota"));
        let JobStep::Failed(e) =
            job_step(&json!({ "status": "error", "error": { "code": "MODEL_UNAVAILABLE", "message": "off" } }))
        else {
            panic!()
        };
        assert_eq!((e.code.as_str(), e.message.as_str()), ("MODEL_UNAVAILABLE", "off"));
        let JobStep::Failed(e) = job_step(&json!({ "status": "error" })) else { panic!() };
        assert_eq!(e.code, chat_codes::LLM_ERROR);
        let JobStep::Failed(e) = job_step(&json!({ "status": "cancelled" })) else { panic!() };
        assert_eq!(e.code, chat_codes::CANCELLED);
        let JobStep::Failed(e) = job_step(&json!({ "status": "awaiting_approval", "approval_id": "x" })) else { panic!() };
        assert_eq!(e.code, chat_codes::AWAITING_APPROVAL);
    }

    #[test]
    fn job_states_for_the_island() {
        assert_eq!(job_state(&json!({ "status": "processing" })), JobState::Pending("processing".into()));
        assert_eq!(
            job_state(&json!({ "status": "awaiting_approval", "approval_id": "9c31cbd5-f15f" })),
            JobState::AwaitingApproval(Some("9c31cbd5-f15f".into()))
        );
        assert_eq!(job_state(&json!({ "status": "awaiting_approval", "approval_id": "../x" })), JobState::AwaitingApproval(None));
        assert_eq!(
            job_state(&json!({ "status": "done", "content": "Hi", "assistant_message_id": 3702 })),
            JobState::Done { text: "Hi".into(), message_id: Some("3702".into()) }
        );
        let JobState::Failed(e) = job_state(&json!({ "status": "error", "error": { "code": "X_INSUFFICIENT_CREDITS", "message": "m" } }))
        else {
            panic!()
        };
        assert_eq!(e.code, codes::INSUFFICIENT_CREDITS);
    }

    #[test]
    fn forgetting_the_account_closes_the_socket() {
        let chat = ChatState::default();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(chat.ws.attach_test_socket(1));
        assert!(chat.ws.is_open());
        chat.forget_account();
        assert!(!chat.ws.is_open());
    }

    #[test]
    fn one_turn_at_a_time() {
        let chat = ChatState::default();
        assert_eq!(chat.turn_state(), TurnState { busy: false, turn: None });
        {
            let _busy = Busy::claim(&chat, 7).expect("first turn");
            assert_eq!(chat.turn_state(), TurnState { busy: true, turn: Some(7) });
            assert!(Busy::claim(&chat, 8).is_none(), "a second send is refused");
            assert_eq!(chat.turn_state().turn, Some(7), "and does not take over the turn id");
        }
        assert_eq!(chat.turn_state(), TurnState { busy: false, turn: None }, "cleared however the turn ends");
        assert!(Busy::claim(&chat, 9).is_some());
        assert_eq!(
            serde_json::to_value(TurnState { busy: true, turn: Some(3) }).unwrap(),
            json!({ "busy": true, "turn": 3 })
        );
    }

    #[test]
    fn bidi_controls_never_reach_the_island() {
        let sneaky = "ab\u{202E}cd\u{2066}e\u{200F}f\u{061C}g\u{200E}h\u{2069}i\u{202A}j";
        assert_eq!(clamp_one_line(sneaky, 100), "abcdefghij");
        // Persian joiners stay
        assert_eq!(clamp_one_line("می\u{200C}خواهم\u{200D}", 100), "می\u{200C}خواهم\u{200D}");
        assert_eq!(approval_summary("t", Some(&json!({ "p": "x\u{202E}y" }))), "p: xy");
    }

    #[test]
    fn unc_paths_are_refused_before_any_lookup() {
        let inbox = std::env::temp_dir().join("roadeep-unc-test-inbox");
        for bad in [r"\\evil-host\share\a.txt", r"\\?\UNC\evil-host\share\a.txt", "//evil-host/share/a.txt", r"\\.\pipe\x"] {
            assert_eq!(check_upload_path(bad, &inbox).unwrap_err().code, chat_codes::FILE_UNREADABLE, "{bad}");
        }
        // lexically outside the inbox: refused without touching the disk
        assert!(check_upload_path(r"C:\Windows\win.ini", &inbox).is_err());
    }

    #[test]
    fn approval_slot_state_machine() {
        let mut slot = ApprovalSlot::default();
        assert_eq!(slot.begin("a"), Err(chat_codes::APPROVAL_NOT_FOUND), "nothing waits");
        slot.wait("a");
        assert_eq!(slot.begin("b"), Err(chat_codes::APPROVAL_NOT_FOUND), "another approval");
        assert_eq!(slot.begin("a"), Ok(()));
        assert!(slot.in_progress("a"));
        assert_eq!(slot.begin("a"), Err(chat_codes::APPROVAL_ALREADY_DECIDED), "double click");
        // the server refused: the user may try again
        slot.failed("a");
        assert_eq!(slot, ApprovalSlot::Waiting("a".into()));
        assert_eq!(slot.begin("a"), Ok(()));
        assert!(slot.decided("a", "job-2", "approve"));
        assert_eq!(slot.begin("a"), Err(chat_codes::APPROVAL_ALREADY_DECIDED));
        assert_eq!(slot.take_resumed("b"), None);
        assert_eq!(slot.take_resumed("a"), Some(("job-2".into(), "approve")));
        assert_eq!(slot.take_resumed("a"), None, "picked up once");
        assert_eq!(slot.begin("a"), Err(chat_codes::APPROVAL_ALREADY_DECIDED));
        // the same approval announced again (a snapshot) does not reopen it
        slot.wait("a");
        assert_eq!(slot, ApprovalSlot::Followed { id: "a".into() });

        // The socket announced the resumed job before the POST returned.
        let mut slot = ApprovalSlot::Waiting("x".into());
        slot.begin("x").unwrap();
        slot.followed("x");
        assert!(slot.decided("x", "job", "reject"), "still followed");

        // The turn ended (stop/reset) while the decision was out.
        let mut slot = ApprovalSlot::Waiting("y".into());
        slot.begin("y").unwrap();
        slot.clear();
        assert!(!slot.decided("y", "job", "approve"), "nobody follows the resumed job");
    }

    #[test]
    fn approval_summary_is_short_and_keeps_secrets_out() {
        let args = json!({
            "model": "flux", "mode": "text-to-image", "input": { "prompt": "A cinematic\nlandscape" },
            "api_key": "sk-123", "access_token": "t", "count": 2, "draft": true, "tags": ["a", "b"], "none": null
        });
        let s = approval_summary("start_generation", Some(&args));
        assert_eq!(s, "model: flux · mode: text-to-image · input: A cinematic landscape · count: 2 · draft: true · tags: [2]");
        assert!(!s.contains("sk-123") && !s.contains("access_token"));
        assert_eq!(approval_summary("start_generation", None), "start_generation");
        assert_eq!(approval_summary("web_fetch", Some(&json!({}))), "web_fetch");
        let long = approval_summary("t", Some(&json!({ "a": "x".repeat(500), "b": "y".repeat(500), "c": "z".repeat(500), "d": "w".repeat(500) })));
        assert!(long.chars().count() <= APPROVAL_SUMMARY_CHARS + 1);

        let listed = json!({ "items": [{ "id": "a-1", "tool": "start_generation", "arguments": { "model": "flux" } }] });
        assert_eq!(find_approval(&listed, "a-1"), Some((Some("start_generation".into()), Some(json!({ "model": "flux" })))));
        assert_eq!(find_approval(&listed, "zz"), None);
        assert!(find_approval(&json!([{ "id": "b" }]), "b").is_some());
    }

    #[test]
    fn stream_events_serialize_per_the_contract() {
        let ev = |body| serde_json::to_value(StreamEvent { turn: 3, body }).unwrap();
        assert_eq!(ev(StreamBody::Delta { text: "Hi".into() }), json!({ "turn": 3, "kind": "delta", "text": "Hi" }));
        assert_eq!(
            ev(StreamBody::Status { status: "searching", label: None }),
            json!({ "turn": 3, "kind": "status", "status": "searching", "label": null })
        );
        assert_eq!(
            ev(StreamBody::Approval { approval: ApprovalCard { id: "a".into(), tool: "t".into(), summary: "s".into(), call: None } }),
            json!({ "turn": 3, "kind": "approval", "approval": { "id": "a", "tool": "t", "summary": "s" } })
        );
        let call = LocalCall { server: "GitHub".into(), tool: "create_issue".into(), arguments: "{}".into() };
        assert_eq!(
            ev(StreamBody::Approval { approval: ApprovalCard { id: "local-1".into(), tool: "t".into(), summary: "s".into(), call: Some(call) } }),
            json!({ "turn": 3, "kind": "approval", "approval": {
                "id": "local-1", "tool": "t", "summary": "s",
                "call": { "server": "GitHub", "tool": "create_issue", "arguments": "{}" }
            } })
        );
        assert_eq!(
            ev(StreamBody::ApprovalDone { approval_id: "a".into(), decision: "reject" }),
            json!({ "turn": 3, "kind": "approvalDone", "approvalId": "a", "decision": "reject" })
        );
        let reply = ChatReply { text: "t".into(), thread_id: Some("th".into()), model: None, message_id: None };
        assert_eq!(serde_json::to_value(reply).unwrap(), json!({ "text": "t", "threadId": "th", "model": null, "messageId": null }));
    }

    #[test]
    fn opening_and_deleting_threads_moves_the_current_one() {
        let chat = ChatState::default();
        chat.open_thread("t1".into(), Some("m".into()));
        assert_eq!(chat.current_thread().as_deref(), Some("t1"));
        assert_eq!(chat.thread_model.lock().unwrap().as_deref(), Some("m"));
        let epoch = chat.epoch.load(Ordering::SeqCst);
        assert!(!chat.forget_thread("t2"));
        assert_eq!(chat.current_thread().as_deref(), Some("t1"));
        assert!(chat.forget_thread("t1"));
        assert_eq!(chat.current_thread(), None);
        assert!(chat.epoch.load(Ordering::SeqCst) > epoch, "an in-flight turn stops following");
        // an opened thread is an existing one: no first-turn extras
        chat.open_thread("t3".into(), None);
        let new_thread = chat.current_thread().is_none();
        assert_eq!(compose_message("q", None, new_thread, "fa", Some("brief"), None), "q");
    }

    #[test]
    fn poll_backoff_caps_at_two_seconds() {
        let mut d = POLL_START;
        let mut seen = vec![];
        for _ in 0..6 {
            seen.push(d.as_millis());
            d = next_poll_delay(d);
        }
        assert_eq!(seen[0], 700);
        assert!(seen.windows(2).all(|w| w[1] >= w[0]));
        assert_eq!(*seen.last().unwrap(), 2000);
    }

    #[test]
    fn catalogue_parsing() {
        let models = parse_models(&json!({ "items": [
            { "id": "m1", "display_name": "Model 1", "capability_flags": { "vision": true }, "default": true },
            { "id": "m2", "capabilities": ["file_input"], "is_default": true },
            { "display_name": "no id" }
        ]}));
        assert_eq!(models.len(), 2);
        assert!(models[0].is_default && models[0].vision && !models[0].file_input);
        assert_eq!(models[1].display_name, "m2");
        assert!(!models[1].is_default, "only `default` marks the global fallback");
        assert!(models[1].file_input);
        // unknown capabilities do not block; deep research needs an explicit yes
        assert!(models[0].reasoning && models[0].web_search && models[0].tools && !models[0].deep_research);
        let gated = parse_models(&json!([{ "id": "g", "capability_flags": {
            "reasoning": false, "native_web_search": false, "openrouter_web_search": false, "deep_research": true
        }}]));
        assert!(!gated[0].reasoning && !gated[0].web_search && gated[0].deep_research);
        let either = parse_models(&json!([{ "id": "e", "capability_flags": { "native_web_search": false } }]));
        assert!(either[0].web_search, "openrouter search is still possible");

        let agents = parse_agents(&json!({ "agents": [
            { "id": "a-1", "title": "Writer", "short_description": "writes", "icon_url": "https://i", "starter_prompts": ["x", "", 3] },
            { "id": 7, "slug": "slugged" },
            { "title": "no id" }
        ]}));
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].starter_prompts, vec!["x".to_string()]);
        assert_eq!(agents[1].id, "7");
        assert_eq!(agents[1].title, "slugged");
        assert_eq!(parse_agents(&json!([{ "id": "b" }])).len(), 1);
    }

    #[test]
    fn agent_catalogue_shapes() {
        let a = |id: &str, vis: &str| json!({ "id": id, "title": id, "visibility": vis });
        // Object keyed by visibility.
        let cat = parse_catalog(&json!({ "groups": {
            "private": [a("p-1", "private")],
            "public": { "agents": [a("u-1", "public"), a("p-1", "public")] }
        }}))
        .unwrap();
        assert_eq!(cat.exclusive.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["p-1"]);
        assert_eq!(cat.public.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["u-1"], "duplicates kept once");

        // Array of groups, with whichever name key the server uses.
        let cat = parse_catalog(&json!({ "groups": [
            { "visibility": "public", "agents": [a("u-1", "public")] },
            { "key": "Private", "items": [a("p-1", "private"), { "id": "../bad" }] }
        ]}))
        .unwrap();
        assert_eq!(cat.public.len(), 1);
        assert_eq!(cat.exclusive.len(), 1, "an id that cannot go in a URL is dropped");
        assert_eq!(cat.exclusive[0].visibility.as_deref(), Some("private"));

        assert_eq!(parse_catalog(&json!({ "agents": [] })), None, "no groups → fall back");
        assert_eq!(parse_catalog(&json!({ "groups": "x" })), None);

        // Fallback: the plain list split by each agent's visibility.
        let cat = catalog_from_list(parse_agents(&json!({ "agents": [a("p", "PRIVATE"), a("u", "public"), { "id": "n" }] })));
        assert_eq!(cat.exclusive.len(), 1);
        assert_eq!(cat.public.len(), 2, "no visibility = public");
    }

    #[test]
    fn draft_json_is_found_in_fences_and_prose() {
        let plain = r#"{"name":"A","instructions":"B"}"#;
        assert_eq!(extract_json_object(plain).unwrap()["name"], "A");

        let fenced = "```json\n{\"name\": \"Writer\", \"instructions\": \"Write {well}.\"}\n```";
        let v = extract_json_object(fenced).unwrap();
        assert_eq!(v["instructions"], "Write {well}.", "braces inside strings do not end the object");

        let prose = "Sure! Here is your agent:\n{\"name\": \"Chef\", \"instructions\": \"Cook \\\"well\\\".\"}\nEnjoy {it}.";
        assert_eq!(extract_json_object(prose).unwrap()["name"], "Chef");

        // A broken first candidate does not hide a good one after it.
        let two = "{not json} then {\"name\": \"B\", \"instructions\": \"x\"}";
        assert_eq!(extract_json_object(two).unwrap()["name"], "B");

        for bad in ["", "no json here", "{\"name\": \"cut", "[1, 2]", "{ unbalanced"] {
            assert_eq!(extract_json_object(bad), None, "{bad}");
        }
    }

    #[test]
    fn draft_fields_are_clamped_and_checked() {
        let v = json!({
            "name": format!("  {}\nTail  ", "N".repeat(70)),
            "description": "d".repeat(300),
            "instructions": format!("Be kind.\r\n{}", "i".repeat(9000)),
            "starterPrompts": ["one", "", "two\nlines", 4, "three", "four"],
            "webSearch": "true",
            "suggestedColor": "#a78bfa"
        });
        let s = parse_suggestion(&v).unwrap();
        assert_eq!(s.name.chars().count(), agents::MAX_NAME_CHARS);
        assert!(!s.name.contains('\n'));
        assert_eq!(s.description.chars().count(), agents::MAX_DESCRIPTION_CHARS);
        assert!(s.instructions.starts_with("Be kind.\ni"));
        assert_eq!(s.instructions.chars().count(), agents::MAX_INSTRUCTIONS_CHARS);
        assert_eq!(s.starter_prompts, vec!["one", "two lines", "three"]);
        assert!(s.web_search);
        assert_eq!(s.suggested_color.as_deref(), Some("#A78BFA"));

        // snake_case keys and a list of instructions are accepted too
        let s = parse_suggestion(&json!({
            "name": "نویسنده", "instructions": ["Rule 1", "Rule 2"], "starter_prompts": ["x"],
            "web_search": false, "color": "blue"
        }))
        .unwrap();
        assert_eq!(s.instructions, "Rule 1\nRule 2");
        assert_eq!(s.starter_prompts, vec!["x"]);
        assert_eq!(s.suggested_color, None, "only #RRGGBB survives");
        assert_eq!(s.description, "");

        assert_eq!(parse_suggestion(&json!({ "instructions": "x" })), Err("no name"));
        assert_eq!(parse_suggestion(&json!({ "name": "x", "instructions": "   " })), Err("no instructions"));

        // Whatever the model sends, the result fits the local agent store.
        let draft = agents::AgentDraft {
            id: None,
            name: s.name,
            instructions: s.instructions,
            model: String::new(),
            web_search: s.web_search,
            base_agent_id: None,
            description: s.description,
            color: s.suggested_color.unwrap_or_default(),
            starter_prompts: s.starter_prompts,
        };
        assert!(agents::validate(&draft).is_ok());
    }

    #[test]
    fn draft_input_and_prompt() {
        let field = |e: RoadeepError| e.field_errors.and_then(|f| f.keys().next().cloned()).unwrap_or_default();
        assert_eq!(field(check_draft_input("  ", "", "fa", None, None).unwrap_err()), "goal");
        assert_eq!(field(check_draft_input(&"g".repeat(4001), "", "fa", None, None).unwrap_err()), "goal");
        assert_eq!(field(check_draft_input("goal", "bad model", "fa", None, None).unwrap_err()), "model");
        assert_eq!(field(check_draft_input("goal", "", "fa", None, Some("shorter")).unwrap_err()), "feedback");

        let input = check_draft_input(" Summarise my emails ", "m-1", "xx", None, Some("  ")).unwrap();
        assert_eq!(input.goal, "Summarise my emails");
        assert_eq!(input.language, "fa", "unknown language falls back to Persian");
        assert_eq!(input.feedback, None, "blank feedback is no feedback");
        let prompt = draft_prompt(&input);
        assert!(prompt.contains("Persian (Farsi)"));
        assert!(prompt.contains("<<<\nSummarise my emails\n>>>"));
        assert!(!prompt.contains("Current draft"));

        let previous = AgentSuggestion {
            name: "Mail".into(),
            description: String::new(),
            instructions: "Summarise.".into(),
            starter_prompts: vec![],
            web_search: false,
            suggested_color: None,
        };
        let input = check_draft_input("Summarise", "", "en", Some(previous), Some("Make it shorter")).unwrap();
        let prompt = draft_prompt(&input);
        assert!(prompt.contains("in English"));
        assert!(prompt.contains("Current draft:") && prompt.contains("\"name\": \"Mail\""));
        assert!(prompt.ends_with("<<<\nMake it shorter\n>>>"));
    }

    #[test]
    fn draft_prompt_warns_off_storage_verbs_without_using_them() {
        let input = check_draft_input("Summarise my emails", "", "fa", None, None).unwrap();
        let prompt = draft_prompt(&input);
        assert!(prompt.contains("never tell the agent to retain, store or hold on to"));
        // The prompt is a user message too: none of the server's trigger
        // phrases may appear in the app's own text.
        let own = prompt.split("User's goal:").next().unwrap().to_lowercase();
        for trigger in [
            "remember", "memorize", "memorise", "save this", "store this", "note this", "keep this for later",
            "add this to memory", "حفظ", "به خاطر بسپار", "یادت باشه", "ذخیره", "فراموش نکن", "حافظه",
        ] {
            assert!(!own.contains(trigger), "{trigger}");
        }
    }

    #[test]
    fn memory_replies_are_recognised_exactly() {
        for reply in [
            "نتوانستم این مورد را در حافظه ذخیره کنم. لطفاً دوباره تلاش کنید.",
            "در حافظه ذخیره شد.",
            "Saved to memory.",
            "ذخیره‌سازی حافظه در پلن فعلی شما فعال نیست.",
            "Memory saving is not enabled on your current plan.",
            "سقف حافظه پر شده است (حداکثر 50 مورد). لطفاً موارد قدیمی را حذف کنید.",
            "Memory limit reached (max 50). Delete old items first.",
        ] {
            assert!(is_memory_reply(reply, true), "streamed: {reply}");
            assert!(is_memory_reply(reply, false), "unstreamed: {reply}");
        }
        // Whitespace, line breaks and a missing or extra ZWNJ do not matter.
        assert!(is_memory_reply("  در   حافظه\nذخیره شد.  ", true));
        assert!(is_memory_reply("ذخیره سازی حافظه در پلن فعلی شما فعال نیست.", true));
        assert!(is_memory_reply("saved TO memory.", true));
    }

    #[test]
    fn memory_fallback_is_narrow() {
        // A reworded canned reply that streamed nothing.
        assert!(is_memory_reply("این مورد در حافظه ذخیره نشد.", false));
        assert!(is_memory_reply("Couldn't save that to memory. Try again.", false));
        // Streamed text is a real answer unless it matches exactly.
        assert!(!is_memory_reply("Couldn't save that to memory. Try again.", true));
        assert!(!is_memory_reply("این مورد در حافظه ذخیره نشد.", true));
        // Normal short answers about memory.
        for reply in [
            "حافظه رم لپ‌تاپ شما ۱۶ گیگابایت است.",
            "داده‌ها در حافظه رم نگه داشته می‌شوند.",
            "Your laptop has 16 GB of memory.",
            "Memory leaks happen when references are never released.",
            "سلام! چطور می‌توانم کمکتان کنم؟",
            "Saved.",
            "",
        ] {
            assert!(!is_memory_reply(reply, false), "{reply}");
        }
        // Long unstreamed replies never hit the fallback.
        let long = format!("{} در حافظه ذخیره می‌شود.", "متن ".repeat(60));
        assert!(!is_memory_reply(&long, false));
    }

    #[test]
    fn server_ids_are_checked_before_they_reach_a_url() {
        assert_eq!(server_id(&json!({ "job_id": "a-1_B" }), "job_id").unwrap().as_deref(), Some("a-1_B"));
        assert_eq!(server_id(&json!({ "job_id": 42 }), "job_id").unwrap().as_deref(), Some("42"));
        assert_eq!(server_id(&json!({}), "thread_id").unwrap(), None);
        for bad in ["../../admin", "a/b", "x?y=1", "a b", "%2e%2e"] {
            assert_eq!(server_id(&json!({ "job_id": bad }), "job_id").unwrap_err().code, codes::INVALID_RESPONSE, "{bad}");
        }
    }

    #[test]
    fn forgetting_the_account_drops_locks_and_catalogue() {
        let chat = ChatState::default();
        *chat.thread_id.lock().unwrap() = Some("t".into());
        chat.known_models.lock().unwrap().push(model(true, true, true));
        *chat.locks.lock().unwrap() = Some(PlanLocks { web_search: true, ..Default::default() });
        chat.forget_account();
        assert!(chat.thread_id.lock().unwrap().is_none());
        assert!(chat.known_models.lock().unwrap().is_empty());
        assert!(chat.locks.lock().unwrap().is_none());
    }

    #[test]
    fn agent_ids_are_restricted() {
        assert!(valid_agent_id("3f1c2b9e-0000-4000-8000-123456789abc"));
        assert!(!valid_agent_id(""));
        assert!(!valid_agent_id("../x"));
        assert!(!valid_agent_id("a b"));
    }

    #[test]
    fn uploads_only_from_the_inbox_and_within_the_cap() {
        let root = std::env::temp_dir().join(format!("roadeep-upload-test-{}", std::process::id()));
        let inbox = root.join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let inside = inbox.join("a.txt");
        std::fs::write(&inside, b"hi").unwrap();
        let outside = root.join("b.txt");
        std::fs::write(&outside, b"hi").unwrap();

        assert!(check_upload_path(inside.to_str().unwrap(), &inbox).is_ok());
        let err = check_upload_path(outside.to_str().unwrap(), &inbox).unwrap_err();
        assert_eq!(err.code, chat_codes::FILE_UNREADABLE);
        let sneaky = inbox.join("..").join("b.txt");
        assert!(check_upload_path(sneaky.to_str().unwrap(), &inbox).is_err());
        assert!(check_upload_path(inbox.join("missing").to_str().unwrap(), &inbox).is_err());

        let big = inbox.join("big.bin");
        std::fs::File::create(&big).unwrap().set_len(MAX_UPLOAD_BYTES + 1).unwrap();
        assert_eq!(check_upload_path(big.to_str().unwrap(), &inbox).unwrap_err().code, chat_codes::FILE_TOO_LARGE);

        assert_eq!(mime_for(Path::new("x.PDF")), "application/pdf");
        assert_eq!(mime_for(Path::new("x")), "application/octet-stream");
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── MCP tool loop ──

    fn spec(qualified: &str, tool: &str, mode: mcpc::ToolMode) -> mcpc::ToolSpec {
        mcpc::ToolSpec {
            server_id: "gh-1".into(),
            server_name: "GitHub".into(),
            tool: tool.into(),
            qualified: qualified.into(),
            description: "d".into(),
            input_schema: json!({ "type": "object" }),
            mode,
            read_only: false,
            destructive: false,
        }
    }

    #[test]
    fn tool_calls_name_offered_tools_only() {
        let set = TurnTools {
            specs: vec![
                spec("github__search", "search", mcpc::ToolMode::Auto),
                spec("github__create_issue", "create_issue", mcpc::ToolMode::Ask),
                spec("notion__search", "search", mcpc::ToolMode::Auto),
                spec("github__delete_repo", "delete_repo", mcpc::ToolMode::Off),
            ],
        };
        assert_eq!(set.find("github__search").unwrap().qualified, "github__search");
        assert_eq!(set.find("GitHub__Create_Issue").unwrap().qualified, "github__create_issue", "any case");
        assert_eq!(set.find("create_issue").unwrap().qualified, "github__create_issue", "bare name, one server has it");
        assert!(set.find("search").is_none(), "bare name two servers have: ambiguous");
        assert!(set.find("github__delete_repo").is_none(), "off is never run");
        assert!(set.find("nope").is_none());
    }

    #[test]
    fn the_loop_corrects_once_and_stops_at_the_step_limit() {
        let call = Parsed::Call { tool: "a__b".into(), arguments: json!({}) };
        assert_eq!(next_move(&Parsed::Answer, false, 0, false), Move::Answer);
        assert_eq!(next_move(&call, true, 0, false), Move::Run);
        assert_eq!(next_move(&call, false, 0, false), Move::Correct("unknown tool name"));
        assert_eq!(next_move(&call, false, 0, true), Move::GiveUp("unknown tool name"));
        assert_eq!(next_move(&Parsed::Invalid("x"), true, 3, false), Move::Correct("x"));
        assert_eq!(next_move(&Parsed::Invalid("x"), true, 3, true), Move::GiveUp("x"));
        assert_eq!(next_move(&call, true, tools::MAX_TOOL_STEPS - 1, false), Move::Run);
        assert_eq!(next_move(&call, true, tools::MAX_TOOL_STEPS, false), Move::Limit);

        // A model that never stops calling: messages the turn sends after the
        // question, and tools it runs.
        let (mut steps, mut corrected, mut messages) = (0, false, 0);
        loop {
            match next_move(&call, true, steps, corrected) {
                Move::Run => {
                    steps += 1;
                    messages += 1;
                }
                Move::Limit => {
                    messages += 1;
                    break;
                }
                Move::Correct(_) => {
                    corrected = true;
                    messages += 1;
                }
                Move::Answer | Move::GiveUp(_) => break,
            }
        }
        assert_eq!((steps, messages), (tools::MAX_TOOL_STEPS, tools::MAX_TOOL_STEPS + 1), "8 results + 1 limit note");

        // A model that keeps sending broken calls: one correction, then its text stands.
        let mut corrected = false;
        let mut sent = 0;
        while let Move::Correct(_) = next_move(&Parsed::Invalid("bad"), true, 0, corrected) {
            corrected = true;
            sent += 1;
        }
        assert_eq!(sent, 1);
    }

    #[test]
    fn local_tool_approvals_are_decided_once() {
        let mut slot = LocalApproval::default();
        assert_eq!(slot.decide("local-1", "approve"), Err(chat_codes::APPROVAL_NOT_FOUND), "nothing waits");
        slot = LocalApproval::Waiting("local-1".into());
        assert_eq!(slot.decide("local-2", "approve"), Err(chat_codes::APPROVAL_NOT_FOUND), "another approval");
        assert_eq!(slot.decision("local-1"), None);
        assert_eq!(slot.decide("local-1", "reject"), Ok(()));
        assert_eq!(slot.decision("local-1"), Some("reject"));
        assert_eq!(slot.decide("local-1", "approve"), Err(chat_codes::APPROVAL_ALREADY_DECIDED), "double click");

        // A new conversation forgets it, and the per-chat tool choice and list.
        let chat = ChatState::default();
        *chat.local_approval.lock().unwrap() = LocalApproval::Waiting("local-3".into());
        chat.set_tools_choice(false);
        chat.note_thread_tools(Some("abcd1234".into()));
        chat.reset();
        assert_eq!(*chat.local_approval.lock().unwrap(), LocalApproval::Idle);
        assert_eq!(*chat.tools_choice.lock().unwrap(), None);
        assert_eq!(*chat.tools_sent.lock().unwrap(), None);
        // Server approval ids are UUIDs: they can never take the local path.
        assert!(!"3f1c2b9e-0000-4000-8000-123456789abc".starts_with(LOCAL_APPROVAL_PREFIX));
    }

    #[test]
    fn the_tool_block_goes_right_before_the_question() {
        let ctx = ChatContext::Window { app_name: "Code".into(), title: "t".into(), url: None };
        let msg = compose_message("q", Some(&ctx), true, "fa", Some("brief"), Some("[Roadeep tools #ab active: x]\n\n"));
        assert_eq!(
            msg,
            format!(
                "[Agent instructions]\nbrief\n[/Agent instructions]\n\nContext — App: Code, Window: t\n\n[Roadeep tools #ab active: x]\n\nq{PERSIAN_SUFFIX}"
            )
        );
        // Later messages carry the block too (a reminder), nothing else.
        assert_eq!(compose_message("q", None, false, "fa", Some("x"), Some("[T]\n\n")), "[T]\n\nq");
        assert_eq!(threads::display_user_text(&msg), "q");
    }

    #[test]
    fn tool_steps_serialize_per_the_contract() {
        let step = ToolStepView {
            id: "3-1".into(),
            server: "GitHub".into(),
            tool: "create_issue".into(),
            state: "running",
            arguments: "{}".into(),
            result: None,
            error: None,
        };
        assert_eq!(
            serde_json::to_value(StreamEvent { turn: 3, body: StreamBody::ToolStep { step } }).unwrap(),
            json!({ "turn": 3, "kind": "toolStep", "step": {
                "id": "3-1", "server": "GitHub", "tool": "create_issue", "state": "running",
                "arguments": "{}", "result": null, "error": null
            }})
        );
        assert_eq!(
            serde_json::to_value(ChatToolsState { available: true, on: false, count: None }).unwrap(),
            json!({ "available": true, "on": false, "count": null })
        );
    }

    #[test]
    fn secrets_are_masked_at_any_depth() {
        let v = json!({ "token": "t", "list": [{ "api_key": "k", "name": "n" }], "q": "x" });
        assert_eq!(redact_secrets(&v), json!({ "token": "•••", "list": [{ "api_key": "•••", "name": "n" }], "q": "x" }));
    }

    #[test]
    fn offered_tools_follow_the_preamble() {
        let specs: Vec<mcpc::ToolSpec> = (0..3).map(|i| spec(&format!("s__t{i}"), &format!("t{i}"), mcpc::ToolMode::Auto)).collect();
        let (kept, preamble) = offer(specs);
        assert_eq!(kept.len(), 3);
        assert_eq!(preamble.listed, 3);
        assert!(preamble.text.contains("- s__t2: d — {}"));
    }
    #[test]
    fn voice_tools_are_local_explicit_and_restrictive() {
        let specs = voice_specs();
        let names: Vec<&str> = specs.iter().map(|s| s.tool.as_str()).collect();
        assert_eq!(names, ["list_tasks", "list_reminders", "list_habits"]);
        assert!(specs.iter().all(|s| s.read_only && voice_tool_allowed(s)));
        // Every mutating planner tool and desktop open is neither offered nor callable.
        for spec in crate::planner::tools::specs().into_iter().chain(crate::desktop_actions::specs()) {
            let read_only = spec.server_id == crate::planner::tools::SERVER_ID && spec.read_only;
            assert_eq!(voice_tool_allowed(&spec), read_only, "{}", spec.tool);
            assert_eq!(specs.iter().any(|s| s.qualified == spec.qualified), read_only, "{}", spec.tool);
        }
        for tool in ["add_task", "add_note", "complete_task", "add_reminder", "start_focus", "stop_focus", "log_habit"] {
            let mut forged = specs[0].clone(); forged.tool = tool.into(); forged.read_only = true;
            assert!(!voice_tool_allowed(&forged), "{tool}");
        }
        let mut writable = specs[0].clone(); writable.read_only = false;
        assert!(!voice_tool_allowed(&writable));
        let mut open = specs[0].clone(); open.server_id = crate::desktop_actions::SERVER_ID.into(); open.tool = "open".into();
        assert!(!voice_tool_allowed(&open));
        let mut spoof = specs[0].clone();
        spoof.server_id = "remote-mcp".into();
        assert!(!voice_tool_allowed(&spoof));
        spoof.server_id = crate::planner::tools::SERVER_ID.into(); spoof.tool = "delete_task".into();
        assert!(!voice_tool_allowed(&spoof));
        spoof = specs[0].clone(); spoof.mode = mcpc::ToolMode::Ask;
        assert!(!voice_tool_allowed(&spoof));
        spoof.server_id = crate::computer::tools::SERVER_ID.into();
        assert!(!voice_tool_allowed(&spoof));
        assert!(!SendOptions::from_settings("test".into(), None, None, &Settings::default()).voice_task);
    }

    #[tokio::test]
    async fn voice_approval_jobs_stop_without_cards_and_cancel_each_step() {
        let rd = Roadeep {
            transport: crate::roadeep::http::Transport::new(), session: crate::roadeep::auth::Session::empty(),
            refresh_lock: tokio::sync::Mutex::new(()), app: std::sync::OnceLock::new(), balance: Mutex::new(None),
        };
        let chat = ChatState::default();
        chat.open_thread("typed-thread".into(), Some("typed-model".into()));
        chat.note_thread_tools(Some("typed-fingerprint".into()));
        let cancelled = std::sync::atomic::AtomicUsize::new(0);
        for job in ["first-voice-job", "continued-tool-job"] {
            accept_turn_thread(&chat, true, job, true, Some("voice-model".into()), Some("voice-fingerprint".into()));
            let mut follow = Follow::new(&rd, &chat, 1, chat.epoch.load(Ordering::SeqCst), job, JOB_TIMEOUT, None);
            follow.voice_task = true;
            follow.queue.push_back(Signal::Approval { id: "approval-1".into(), tool: Some("payment".into()) });
            let result = follow.run().await;
            assert_eq!(result.as_ref().err().unwrap().code, "voice-sensitive-action-blocked");
            assert!(follow.awaiting.is_none());
            assert!(chat.approval.lock().unwrap().id().is_none());
            cancel_voice_block(&result, async { cancelled.fetch_add(1, Ordering::SeqCst); }).await;
            assert_eq!(chat.current_thread().as_deref(), Some("typed-thread"));
            assert_eq!(chat.thread_model.lock().unwrap().as_deref(), Some("typed-model"));
            assert_eq!(chat.tools_sent.lock().unwrap().as_deref(), Some("typed-fingerprint"));
        }
        let mut resumed = Follow::new(&rd, &chat, 1, chat.epoch.load(Ordering::SeqCst), "old-job", JOB_TIMEOUT, None);
        resumed.voice_task = true;
        assert_eq!(resumed.on_signal(Signal::Resumed { job: "replacement-job".into(), decision: "approve" }).await.unwrap().err().unwrap().code, "voice-sensitive-action-blocked");
        assert_eq!(resumed.tracker.job(), "replacement-job");
        assert_eq!(cancelled.load(Ordering::SeqCst), 2);
        accept_turn_thread(&chat, false, "new-typed-thread", true, Some("new-typed-model".into()), Some("new-typed-tools".into()));
        assert_eq!(chat.current_thread().as_deref(), Some("new-typed-thread"));
        assert_eq!(chat.thread_model.lock().unwrap().as_deref(), Some("new-typed-model"));
        cancel_voice_block(&Ok::<_, RoadeepError>(()), async { panic!("success must not cancel") }).await;
        let mut typed = Follow::new(&rd, &chat, 2, chat.epoch.load(Ordering::SeqCst), "typed-job", JOB_TIMEOUT, None);
        assert!(typed.on_signal(Signal::Approval { id: "typed-approval".into(), tool: Some("tool".into()) }).await.is_none());
        assert!(typed.awaiting.is_some());
        assert_eq!(chat.approval.lock().unwrap().id(), Some("typed-approval"));
    }

    #[test]
    fn voice_rejects_file_upload_before_submit() {
        let file = ChatContext::File { name: "test.txt".into(), path: "test.txt".into() };
        assert_eq!(validate_voice_context(true, Some(&file)).unwrap_err().code, "voice-sensitive-action-blocked");
        assert!(validate_voice_context(false, Some(&file)).is_ok());
        assert!(validate_voice_context(true, None).is_ok());
    }

}
