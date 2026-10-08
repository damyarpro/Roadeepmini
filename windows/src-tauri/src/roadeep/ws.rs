// Roadeep's realtime WebSocket (`/ws/v1/events/`), used to stream island chat
// turns. Mirrors the mobile client (src/realtime/client.ts, parse.ts and
// features/chat/stream.ts) and the backend contract
// (Roadeep_front_Gitlab/docs/chatbot-frontend-integration.md §3–4):
// - the access token rides in the query string, so the URL is NEVER logged;
// - "open" after `connection.ready`, or a 3 s fallback;
// - close 4401 → one token refresh and one more try;
// - frames are filtered by `chat.` type and job id, and anything at or below
//   the last accepted `job_sequence` is dropped.
//
// The socket only exists while a chat turn wants it: it is opened lazily when
// a send starts and closes itself after a minute and a half without a turn
// listening, so nothing runs while the island is idle. Everything that reads a
// frame is a pure function and tested without a network.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{HeaderValue, USER_AGENT};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use super::chat::chat_codes::LLM_ERROR;
use super::chat::shown_char;
use super::generation::valid_id;
use super::http::{self, codes, RoadeepError};
use super::{Refresh, Roadeep};
use crate::log;

pub const EVENTS_PATH: &str = "/ws/v1/events/";
/// Server close code for a missing or expired token.
pub const CLOSE_UNAUTHORIZED: u16 = 4401;
const CONNECTION_READY: &str = "connection.ready";
/// TCP + TLS + upgrade; past this the turn simply polls.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Servers that never send `connection.ready` are treated as ready after this.
const READY_FALLBACK: Duration = Duration::from_secs(3);
/// The socket closes itself once no turn has listened for this long.
const IDLE_CLOSE: Duration = Duration::from_secs(90);
const IDLE_CHECK: Duration = Duration::from_secs(10);
/// After a failed connect, turns poll without trying again for this long, so
/// a network that blocks WebSockets does not cost every turn a timeout.
const DOWN_COOLDOWN: Duration = Duration::from_secs(300);
/// Frames waiting for a slow turn; a turn that lags behind catches up by REST.
const CHANNEL_CAPACITY: usize = 512;
const MAX_MESSAGE_BYTES: usize = 8 << 20;
/// Frames of an unknown job kept while a turn waits for an approval (the
/// resumed job can speak before the decision call has returned its id).
const MAX_BUFFERED: usize = 64;
const MAX_LABEL_CHARS: usize = 80;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

// ── URL ───────────────────────────────────────────────────────────────────────

/// `https://roadeep.com/api` → `wss://roadeep.com/ws/v1/events/?token=…`.
/// The socket lives next to the API, not under it (MOB config/endpoints.json).
pub fn events_url(base: &str, token: &str) -> Option<String> {
    let endpoint = events_endpoint(base)?;
    let token: String = url_encode(token);
    Some(format!("{endpoint}?token={token}"))
}

/// The URL without its query: the only form that may appear in a log.
pub fn events_endpoint(base: &str) -> Option<String> {
    let url = reqwest::Url::parse(base).ok()?;
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => return None,
    };
    let host = url.host_str()?;
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let prefix = url.path().trim_end_matches('/');
    let prefix = prefix.strip_suffix("/api").unwrap_or(prefix);
    Some(format!("{scheme}://{host}{port}{prefix}{EVENTS_PATH}"))
}

fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── Frames ────────────────────────────────────────────────────────────────────

/// One event frame, normalized. Unknown fields stay in `data`.
#[derive(Debug, Clone, PartialEq)]
pub struct WsEvent {
    /// e.g. "chat.delta".
    pub kind: String,
    pub job_id: Option<String>,
    pub thread_id: Option<String>,
    pub status: Option<String>,
    /// `job_sequence`: monotonic within one job.
    pub sequence: Option<i64>,
    pub snapshot: bool,
    /// `display_name`, the server's own short (Persian) label.
    pub label: Option<String>,
    pub delta: Option<String>,
    pub content_so_far: Option<String>,
    pub assistant_message_id: Option<String>,
    pub data: Value,
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Ids are numbers on some events and strings on others.
fn id(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn label(v: &Value) -> Option<String> {
    let raw = v.get("display_name").and_then(Value::as_str)?;
    let one_line: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one_line.chars().filter(|c| shown_char(*c)).take(MAX_LABEL_CHARS).collect();
    (!cut.is_empty()).then_some(cut)
}

/// JSON text → event, or None for anything that is not a typed object.
pub fn parse_frame(raw: &str) -> Option<WsEvent> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let obj = v.as_object()?;
    let kind = text(&v, "type")?;
    let data = match obj.get("data") {
        Some(d @ Value::Object(_)) => d.clone(),
        _ => Value::Object(Default::default()),
    };
    let sequence = v.get("job_sequence").or_else(|| v.get("sequence")).and_then(Value::as_i64);
    let snapshot = v.get("snapshot") == Some(&Value::Bool(true)) || data.get("snapshot") == Some(&Value::Bool(true));
    Some(WsEvent {
        job_id: id(&v, "job_id"),
        thread_id: id(&v, "thread_id"),
        status: text(&v, "status"),
        sequence,
        snapshot,
        label: label(&v),
        delta: v.get("delta").and_then(Value::as_str).map(str::to_string),
        content_so_far: v.get("content_so_far").and_then(Value::as_str).map(str::to_string),
        assistant_message_id: id(&v, "assistant_message_id").or_else(|| id(&data, "assistant_message_id")),
        kind,
        data,
    })
}

/// What the island shows while a turn runs ("chat-stream" `status`). None =
/// nothing worth showing (the text itself, usage counters, terminal events).
pub fn status_kind(event_type: &str, data: &Value) -> Option<&'static str> {
    let name = event_type.strip_prefix("chat.")?;
    let tool_kind = || {
        let tool = data.get("tool").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
        if tool.contains("search") || tool.contains("fetch") {
            "searching"
        } else {
            "tool"
        }
    };
    match name {
        "queued" | "started" => Some("queued"),
        "source_found" | "searching_rag" | "rag_result" | "searching_agent_knowledge" | "agent_knowledge_result" => {
            Some("searching")
        }
        "generation_queued" | "generation_waiting" | "tool_async_started" | "tool_async_settled" => Some("generating"),
        "streaming" | "delta" | "model_completed" | "usage_updated" | "loop_detected" | "done" | "error" | "cancelled"
        | "snapshot" | "tool_approval_required" => None,
        n if n.starts_with("tool_") || n.starts_with("server_tool_") || n.starts_with("subagent_") => Some(tool_kind()),
        // thinking, reasoning, memory_check, routing, loading_context, calling_model,
        // analyzing_files, coordination_planned, agent_*, and anything new.
        _ => Some("thinking"),
    }
}

/// A REST job status → the same status kinds, for turns that poll.
pub fn status_from_job(status: &str) -> Option<&'static str> {
    match status {
        "queued" | "pending" | "retry" => Some("queued"),
        "started" | "processing" => Some("thinking"),
        "generation_pending" => Some("generating"),
        _ => None,
    }
}

/// A failed job's error (`data.error` string or `{code, message}`), with every
/// "not enough credit" rejection under one code.
pub fn job_error(data: &Value, fallback_label: Option<&str>) -> RoadeepError {
    let default = "The model could not answer this one.";
    let (code, message) = match data.get("error") {
        Some(Value::String(s)) if !s.trim().is_empty() => (LLM_ERROR.to_string(), s.trim().to_string()),
        Some(Value::Object(o)) => (
            o.get("code").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(LLM_ERROR).to_string(),
            o.get("message")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .or_else(|| fallback_label.map(str::to_string))
                .unwrap_or_else(|| default.to_string()),
        ),
        _ => (LLM_ERROR.to_string(), fallback_label.unwrap_or(default).to_string()),
    };
    let status = data.get("status_code").and_then(Value::as_u64).and_then(|s| u16::try_from(s).ok());
    let code = if http::is_insufficient_credit(status, &code, &message) { codes::INSUFFICIENT_CREDITS.to_string() } else { code };
    RoadeepError { status, ..RoadeepError::new(&code, message) }
}

/// What a frame means for the turn following it.
#[derive(Debug, PartialEq)]
pub enum Signal {
    /// The full reply so far.
    Delta(String),
    Status { kind: &'static str, label: Option<String> },
    /// The job paused for a human decision on a tool call.
    Approval { id: String, tool: Option<String> },
    /// Someone decided (here or in another client): follow this job now.
    Resumed { job: String, decision: &'static str },
    Done { reply: Option<String>, message_id: Option<String> },
    Failed(RoadeepError),
    Cancelled,
}

/// Follows one turn's job through the user-wide stream.
#[derive(Debug)]
pub struct TurnTracker {
    job: String,
    last_sequence: Option<i64>,
    text: String,
    awaiting: bool,
    buffered: Vec<WsEvent>,
}

impl TurnTracker {
    pub fn new(job: &str) -> Self {
        Self { job: job.to_string(), last_sequence: None, text: String::new(), awaiting: false, buffered: Vec::new() }
    }

    pub fn job(&self) -> &str {
        &self.job
    }

    /// The reply as streamed so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_awaiting(&mut self, awaiting: bool) {
        self.awaiting = awaiting;
        if !awaiting {
            self.buffered.clear();
        }
    }

    /// The decision resumed the turn under a new job: follow it, replaying
    /// whatever it already said while we did not know its id.
    pub fn switch_job(&mut self, job: &str) -> Vec<Signal> {
        self.job = job.to_string();
        self.last_sequence = None;
        self.awaiting = false;
        let buffered = std::mem::take(&mut self.buffered);
        buffered.iter().filter_map(|ev| self.apply(ev)).collect()
    }

    pub fn apply(&mut self, ev: &WsEvent) -> Option<Signal> {
        if !ev.kind.starts_with("chat.") {
            return None;
        }
        let job = ev.job_id.as_deref()?;
        if job != self.job {
            if self.awaiting && self.buffered.len() < MAX_BUFFERED {
                self.buffered.push(ev.clone());
            }
            return None;
        }
        if let Some(seq) = ev.sequence {
            if self.last_sequence.is_some_and(|last| seq <= last) {
                return None;
            }
            self.last_sequence = Some(seq);
        }
        match ev.kind.as_str() {
            "chat.delta" => {
                match (&ev.content_so_far, &ev.delta) {
                    (Some(full), _) => self.text = full.clone(),
                    (None, Some(delta)) => self.text.push_str(delta),
                    (None, None) => return None,
                }
                (!self.text.is_empty()).then(|| Signal::Delta(self.text.clone()))
            }
            "chat.done" => Some(Signal::Done {
                reply: ev.data.get("reply").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
                message_id: ev.assistant_message_id.clone(),
            }),
            "chat.error" => Some(Signal::Failed(job_error(&ev.data, ev.label.as_deref()))),
            "chat.cancelled" => Some(Signal::Cancelled),
            "chat.tool_approval_required" => {
                id(&ev.data, "approval_id").filter(|a| valid_id(a)).map(|id| Signal::Approval { id, tool: text(&ev.data, "tool") })
            }
            "chat.tool_approved" | "chat.tool_rejected" => {
                let decision = if ev.kind == "chat.tool_approved" { "approve" } else { "reject" };
                match id(&ev.data, "job_id").filter(|j| valid_id(j) && *j != self.job) {
                    Some(job) => Some(Signal::Resumed { job, decision }),
                    None => Some(Signal::Status { kind: "tool", label: ev.label.clone() }),
                }
            }
            "chat.snapshot" => {
                // The job's current state after a reconnect, not a new operation.
                if ev.status.as_deref() == Some("awaiting_approval") {
                    if let Some(id) = id(&ev.data, "approval_id").filter(|a| valid_id(a)) {
                        return Some(Signal::Approval { id, tool: text(&ev.data, "tool") });
                    }
                }
                if let Some(full) = ev.content_so_far.as_ref().filter(|t| !t.is_empty()) {
                    self.text = full.clone();
                    return Some(Signal::Delta(self.text.clone()));
                }
                let kind = text(&ev.data, "last_event_type")
                    .and_then(|t| status_kind(&t, &ev.data))
                    .or_else(|| ev.status.as_deref().and_then(status_from_job))?;
                Some(Signal::Status { kind, label: ev.label.clone() })
            }
            other => status_kind(other, &ev.data).map(|kind| Signal::Status { kind, label: ev.label.clone() }),
        }
    }
}

/// A turn that has heard nothing at all about its job on the socket for this
/// long stops trusting it and polls instead.
pub const SILENCE: Duration = Duration::from_secs(15);

pub fn stalled(frames_seen: bool, waited: Duration) -> bool {
    !frames_seen && waited >= SILENCE
}

// ── Connection ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum WsMsg {
    Event(Arc<WsEvent>),
    /// The socket is gone; the close code when the server sent one.
    Closed(Option<u16>),
}

#[derive(Debug)]
pub enum OpenError {
    /// The refresh token was rejected: the session is over.
    Expired { generation: u64 },
    /// No socket this time (network, proxy opt-in, second 4401…): poll instead.
    Unavailable(&'static str),
}

struct Live {
    tx: broadcast::Sender<WsMsg>,
    alive: Arc<AtomicBool>,
    stop: Arc<Notify>,
    generation: u64,
}

impl Live {
    fn new(generation: u64) -> (Self, broadcast::Receiver<WsMsg>) {
        let (tx, rx) = broadcast::channel(CHANNEL_CAPACITY);
        let live = Self { tx, alive: Arc::new(AtomicBool::new(true)), stop: Arc::new(Notify::new()), generation };
        (live, rx)
    }

    fn usable(&self, generation: u64) -> bool {
        self.alive.load(Ordering::SeqCst) && self.generation == generation
    }

    /// The only way a Live goes away: its reader task is told to close the
    /// socket, so no authenticated connection is ever left behind.
    fn stop(self) {
        self.alive.store(false, Ordering::SeqCst);
        self.stop.notify_one();
    }
}

/// The one shared socket. Consumers subscribe; the reader task fans frames out.
#[derive(Default)]
pub struct Hub {
    live: Mutex<Option<Live>>,
    /// Single flight: one connect at a time, and every subscriber after it
    /// reuses what it opened.
    opening: tokio::sync::Mutex<()>,
    /// Bumped by `close()`: a connect that was in flight then is thrown away.
    closes: AtomicU64,
    /// Until when, and for which session, connecting is not worth a try.
    down_until: Mutex<Option<(Instant, u64)>>,
}

impl Hub {
    /// A receiver on a ready socket, connecting if there is none. Subscribe
    /// BEFORE submitting the job, so its first frames are not missed.
    ///
    /// A socket that is alive (and of this session) is always reused, so
    /// `fresh` — "mine just dropped" — can only ever replace a dead one, never
    /// another consumer's new socket. It only skips the connect cooldown.
    pub async fn subscribe(&self, rd: &Roadeep, fresh: bool) -> Result<broadcast::Receiver<WsMsg>, OpenError> {
        // Signed out: the REST call that follows says so (NOT_SIGNED_IN).
        if !rd.session.has_session() {
            return Err(OpenError::Unavailable("not signed in"));
        }
        let generation = rd.session.generation();
        self.attach(generation, fresh, || async move {
            let (socket, early) = open(rd).await?;
            let (live, rx) = Live::new(generation);
            for ev in early {
                let _ = live.tx.send(WsMsg::Event(Arc::new(ev)));
            }
            tauri::async_runtime::spawn(pump(socket, live.tx.clone(), live.alive.clone(), live.stop.clone()));
            Ok((live, rx))
        })
        .await
    }

    /// `subscribe` without the network: `connect` opens a socket and returns
    /// its Live with a first receiver.
    async fn attach<F, Fut>(&self, generation: u64, fresh: bool, connect: F) -> Result<broadcast::Receiver<WsMsg>, OpenError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(Live, broadcast::Receiver<WsMsg>), OpenError>>,
    {
        let _single = self.opening.lock().await;
        {
            let mut slot = self.live.lock().unwrap();
            match slot.take() {
                Some(live) if live.usable(generation) => {
                    let rx = live.tx.subscribe();
                    *slot = Some(live);
                    return Ok(rx);
                }
                // Dead, or the previous session's: closed before anything replaces it.
                Some(stale) => stale.stop(),
                None => {}
            }
        }
        if !fresh && self.down_until.lock().unwrap().is_some_and(|(until, g)| g == generation && Instant::now() < until) {
            return Err(OpenError::Unavailable("recently unreachable"));
        }
        let closes = self.closes.load(Ordering::SeqCst);
        let (live, rx) = match connect().await {
            Ok(opened) => opened,
            Err(err) => {
                if matches!(err, OpenError::Unavailable(_)) {
                    *self.down_until.lock().unwrap() = Some((Instant::now() + DOWN_COOLDOWN, generation));
                }
                return Err(err);
            }
        };
        *self.down_until.lock().unwrap() = None;
        // Signed out (or in) while connecting: this socket belongs to nobody.
        if self.closes.load(Ordering::SeqCst) != closes {
            live.stop();
            return Err(OpenError::Unavailable("closed while connecting"));
        }
        if let Some(previous) = self.live.lock().unwrap().replace(live) {
            previous.stop();
        }
        Ok(rx)
    }

    /// A socket without a network, for tests of what owns the hub.
    #[cfg(test)]
    pub(crate) async fn attach_test_socket(&self, generation: u64) {
        let _ = self.attach(generation, false, || async move { Ok(Live::new(generation)) }).await;
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self) -> bool {
        self.live.lock().unwrap().as_ref().is_some_and(|l| l.alive.load(Ordering::SeqCst))
    }

    /// Closes the socket now, and any being opened (sign-out, a new account).
    pub fn close(&self) {
        self.closes.fetch_add(1, Ordering::SeqCst);
        if let Some(live) = self.live.lock().unwrap().take() {
            live.stop();
        }
    }
}

/// Reads frames until the socket closes, is stopped, or has had no turn
/// listening for `IDLE_CLOSE`.
async fn pump(mut socket: Socket, tx: broadcast::Sender<WsMsg>, alive: Arc<AtomicBool>, stop: Arc<Notify>) {
    let mut tick = tokio::time::interval(IDLE_CHECK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut unwatched_since: Option<Instant> = None;
    let mut code = None;
    let why = loop {
        tokio::select! {
            _ = stop.notified() => {
                let _ = socket.close(None).await;
                break "closed by the app";
            }
            frame = socket.next() => match frame {
                Some(Ok(Message::Text(raw))) => {
                    if let Some(ev) = parse_frame(raw.as_str()).filter(|ev| ev.kind.starts_with("chat.")) {
                        // No receiver is fine: nobody is following a turn right now.
                        let _ = tx.send(WsMsg::Event(Arc::new(ev)));
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    code = frame.map(|f| u16::from(f.code));
                    break "closed by the server";
                }
                Some(Ok(_)) => {} // ping/pong (answered by tungstenite), binary
                Some(Err(err)) => break error_kind(&err),
                None => break "stream ended",
            },
            _ = tick.tick() => {
                if tx.receiver_count() > 0 {
                    unwatched_since = None;
                } else if unwatched_since.get_or_insert_with(Instant::now).elapsed() >= IDLE_CLOSE {
                    let _ = socket.close(None).await;
                    break "idle";
                }
            }
        }
    };
    alive.store(false, Ordering::SeqCst);
    log::line(format!(
        "roadeep ws: disconnected ({why}{})",
        code.map(|c| format!(", code {c}")).unwrap_or_default()
    ));
    let _ = tx.send(WsMsg::Closed(code));
}

/// Never Display a tungstenite error: some variants can carry request details.
fn error_kind(err: &WsError) -> &'static str {
    match err {
        WsError::ConnectionClosed | WsError::AlreadyClosed => "closed",
        WsError::Io(_) => "io error",
        WsError::Tls(_) => "tls error",
        WsError::Capacity(_) => "frame too large",
        WsError::Protocol(_) => "protocol error",
        WsError::Http(_) => "http error",
        WsError::HttpFormat(_) | WsError::Url(_) => "bad request",
        _ => "error",
    }
}

fn tls_config() -> Option<Arc<rustls::ClientConfig>> {
    static CONFIG: OnceLock<Option<Arc<rustls::ClientConfig>>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|err| log::line(format!("roadeep ws: tls setup failed ({err})")))
                .ok()?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Some(Arc::new(config))
        })
        .clone()
}

enum Dial {
    Unauthorized,
    Failed(&'static str),
}

/// Connect with the current access token; 4401 → one refresh and one retry.
/// A second 4401 just means "no socket": the REST calls decide about the session.
async fn open(rd: &Roadeep) -> Result<(Socket, Vec<WsEvent>), OpenError> {
    if http::proxy_opt_in() {
        return Err(OpenError::Unavailable("proxy opt-in"));
    }
    let generation = rd.session.generation();
    let mut token = match rd.session.access() {
        Some(token) => token,
        None => match rd.refresh(None).await {
            Refresh::Ok(token) => token,
            Refresh::Invalid => return Err(OpenError::Expired { generation }),
            Refresh::Retriable => return Err(OpenError::Unavailable("no access token")),
        },
    };
    let base = rd.transport.base();
    for attempt in 0..2 {
        match dial(base, &token).await {
            Ok(opened) => return Ok(opened),
            Err(Dial::Unauthorized) if attempt == 0 => {
                log::line("roadeep ws: token refused, refreshing once");
                match rd.refresh(Some(&token)).await {
                    Refresh::Ok(fresh) => token = fresh,
                    Refresh::Invalid => return Err(OpenError::Expired { generation }),
                    Refresh::Retriable => return Err(OpenError::Unavailable("refresh failed")),
                }
            }
            Err(Dial::Unauthorized) => return Err(OpenError::Unavailable("token refused twice")),
            Err(Dial::Failed(why)) => {
                log::line(format!("roadeep ws: connect failed ({why})"));
                return Err(OpenError::Unavailable(why));
            }
        }
    }
    Err(OpenError::Unavailable("token refused twice"))
}

async fn dial(base: &str, token: &str) -> Result<(Socket, Vec<WsEvent>), Dial> {
    let endpoint = events_endpoint(base).ok_or(Dial::Failed("bad base url"))?;
    let url = events_url(base, token).ok_or(Dial::Failed("bad base url"))?;
    let mut request = url.into_client_request().map_err(|_| Dial::Failed("bad request"))?;
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static(concat!("Roadeep-Windows/", env!("CARGO_PKG_VERSION"))));
    let connector = match endpoint.starts_with("wss://") {
        true => Some(Connector::Rustls(tls_config().ok_or(Dial::Failed("tls setup"))?)),
        false => None,
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let started = Instant::now();
    // tokio-tungstenite has no proxy support at all: the socket always goes direct.
    // NEVER install a trace-level `log` subscriber for tungstenite: at trace it
    // logs the whole handshake request, `?token=<access token>` included.
    let connect = tokio_tungstenite::connect_async_tls_with_config(request, Some(config), true, connector);
    let mut socket = match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Err(_) => return Err(Dial::Failed("connect timeout")),
        Ok(Err(WsError::Http(response))) if matches!(response.status().as_u16(), 401 | 403) => {
            return Err(Dial::Unauthorized)
        }
        Ok(Err(err)) => return Err(Dial::Failed(error_kind(&err))),
        Ok(Ok((socket, _))) => socket,
    };

    // The server registers the receiver after the upgrade and says so; chat
    // frames before that are kept, anything else is ignored.
    let deadline = tokio::time::Instant::now() + READY_FALLBACK;
    let mut early = Vec::new();
    let ready = loop {
        match tokio::time::timeout_at(deadline, socket.next()).await {
            Err(_) => break "fallback",
            Ok(Some(Ok(Message::Text(raw)))) => match parse_frame(raw.as_str()) {
                Some(ev) if ev.kind == CONNECTION_READY => break "ready",
                Some(ev) if ev.kind.starts_with("chat.") => early.push(ev),
                _ => {}
            },
            Ok(Some(Ok(Message::Close(frame)))) => {
                return match frame.map(|f| u16::from(f.code)) {
                    Some(CLOSE_UNAUTHORIZED) => Err(Dial::Unauthorized),
                    _ => Err(Dial::Failed("closed before ready")),
                };
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(err))) => return Err(Dial::Failed(error_kind(&err))),
            Ok(None) => return Err(Dial::Failed("closed before ready")),
        }
    };
    log::line(format!("roadeep ws: connected to {endpoint} ({ready}, {}ms)", started.elapsed().as_millis()));
    Ok((socket, early))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(v: Value) -> WsEvent {
        parse_frame(&v.to_string()).expect("frame")
    }

    #[test]
    fn url_is_derived_from_the_api_base_and_logged_without_the_token() {
        let token = "eyJ.a+b/c=";
        let url = events_url("https://roadeep.com/api", token).unwrap();
        assert_eq!(url, "wss://roadeep.com/ws/v1/events/?token=eyJ.a%2Bb%2Fc%3D");
        let logged = events_endpoint("https://roadeep.com/api").unwrap();
        assert_eq!(logged, "wss://roadeep.com/ws/v1/events/");
        assert!(!logged.contains("token") && !logged.contains("eyJ"));
        assert_eq!(events_endpoint("https://test.roadeep.com/api/").unwrap(), "wss://test.roadeep.com/ws/v1/events/");
        assert_eq!(events_endpoint("http://localhost:8000/api").unwrap(), "ws://localhost:8000/ws/v1/events/");
        assert_eq!(events_endpoint("https://x.dev/sub/api").unwrap(), "wss://x.dev/sub/ws/v1/events/");
        assert_eq!(events_endpoint("ftp://roadeep.com/api"), None);
        assert_eq!(events_url("not a url", "t"), None);
        // The URL that reaches the wire still parses and carries the token only in the query.
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert_eq!(parsed.path(), "/ws/v1/events/");
        assert_eq!(parsed.query_pairs().next().unwrap().1, token);
    }

    #[test]
    fn frames_parse_with_tolerance() {
        let ev = frame(json!({
            "type": "chat.delta", "job_id": "j1", "thread_id": 7, "status": "streaming",
            "delta": "سلام", "content_so_far": "سلام", "job_sequence": 12, "display_name": "  در حال\nنوشتن  ",
            "data": {}
        }));
        assert_eq!(ev.kind, "chat.delta");
        assert_eq!(ev.thread_id.as_deref(), Some("7"));
        assert_eq!(ev.sequence, Some(12));
        assert_eq!(ev.label.as_deref(), Some("در حال نوشتن"));
        assert!(!ev.snapshot);
        assert!(frame(json!({ "type": "chat.snapshot", "data": { "snapshot": true } })).snapshot);
        assert_eq!(frame(json!({ "type": "x", "data": "odd" })).data, json!({}));
        for bad in ["", "nope", "[1]", "{\"no_type\":1}", "{\"type\":\"\"}"] {
            assert!(parse_frame(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn deltas_replace_or_append_and_old_sequences_are_dropped() {
        let mut t = TurnTracker::new("j1");
        let delta = |seq: i64, delta: &str, so_far: Option<&str>| {
            let mut v = json!({ "type": "chat.delta", "job_id": "j1", "job_sequence": seq, "delta": delta });
            if let Some(s) = so_far {
                v["content_so_far"] = json!(s);
            }
            frame(v)
        };
        assert_eq!(t.apply(&delta(1, "He", None)), Some(Signal::Delta("He".into())));
        assert_eq!(t.apply(&delta(2, "llo", None)), Some(Signal::Delta("Hello".into())));
        assert_eq!(t.apply(&delta(2, "XX", None)), None, "same sequence twice");
        assert_eq!(t.apply(&delta(1, "late", None)), None, "out of order");
        assert_eq!(t.apply(&delta(3, "!", Some("Hello!"))), Some(Signal::Delta("Hello!".into())), "content_so_far wins");
        assert_eq!(t.text(), "Hello!");
        // other jobs and non-chat frames are not ours
        assert_eq!(t.apply(&frame(json!({ "type": "chat.delta", "job_id": "j2", "delta": "x" }))), None);
        assert_eq!(t.apply(&frame(json!({ "type": "generation.done", "job_id": "j1" }))), None);
        assert_eq!(t.apply(&frame(json!({ "type": "chat.delta", "delta": "x" }))), None, "no job id");
        // frames without a sequence are accepted
        assert!(t.apply(&frame(json!({ "type": "chat.delta", "job_id": "j1", "delta": "?" }))).is_some());
    }

    #[test]
    fn terminal_events() {
        let mut t = TurnTracker::new("j");
        let done = frame(json!({ "type": "chat.done", "job_id": "j", "assistant_message_id": 3702, "data": { "reply": " Final " } }));
        assert_eq!(t.apply(&done), Some(Signal::Done { reply: Some("Final".into()), message_id: Some("3702".into()) }));
        let mut t = TurnTracker::new("j");
        let empty = frame(json!({ "type": "chat.done", "job_id": "j", "data": { "assistant_message_id": "m" } }));
        assert_eq!(t.apply(&empty), Some(Signal::Done { reply: None, message_id: Some("m".into()) }));

        let mut t = TurnTracker::new("j");
        let err = frame(json!({ "type": "chat.error", "job_id": "j", "data": { "error": { "code": "MODEL_UNAVAILABLE", "message": "off" } } }));
        let Some(Signal::Failed(e)) = t.apply(&err) else { panic!() };
        assert_eq!((e.code.as_str(), e.message.as_str()), ("MODEL_UNAVAILABLE", "off"));
        let mut t = TurnTracker::new("j");
        let err = frame(json!({ "type": "chat.error", "job_id": "j", "display_name": "خطا", "data": { "status_code": 402 } }));
        let Some(Signal::Failed(e)) = t.apply(&err) else { panic!() };
        assert_eq!((e.code.as_str(), e.message.as_str(), e.status), (codes::INSUFFICIENT_CREDITS, "خطا", Some(402)));
        let mut t = TurnTracker::new("j");
        let err = frame(json!({ "type": "chat.error", "job_id": "j", "data": { "error": "اعتبار شما کافی نیست" } }));
        let Some(Signal::Failed(e)) = t.apply(&err) else { panic!() };
        assert_eq!(e.code, codes::INSUFFICIENT_CREDITS);

        let mut t = TurnTracker::new("j");
        assert_eq!(t.apply(&frame(json!({ "type": "chat.cancelled", "job_id": "j" }))), Some(Signal::Cancelled));
    }

    #[test]
    fn status_events_map_to_the_contract_kinds() {
        let cases = [
            ("chat.queued", json!({}), Some("queued")),
            ("chat.started", json!({}), Some("queued")),
            ("chat.thinking", json!({}), Some("thinking")),
            ("chat.reasoning_summary", json!({}), Some("thinking")),
            ("chat.calling_model", json!({}), Some("thinking")),
            ("chat.agent_step_started", json!({}), Some("thinking")),
            ("chat.source_found", json!({}), Some("searching")),
            ("chat.searching_rag", json!({}), Some("searching")),
            ("chat.tool_started", json!({ "tool": "start_generation" }), Some("tool")),
            ("chat.tool_started", json!({ "tool": "web_fetch" }), Some("searching")),
            ("chat.subagent_progress", json!({}), Some("tool")),
            ("chat.generation_waiting", json!({}), Some("generating")),
            ("chat.tool_async_settled", json!({}), Some("generating")),
            ("chat.something_new", json!({}), Some("thinking")),
            ("chat.delta", json!({}), None),
            ("chat.usage_updated", json!({}), None),
            ("chat.done", json!({}), None),
            ("generation.done", json!({}), None),
        ];
        for (kind, data, want) in cases {
            assert_eq!(status_kind(kind, &data), want, "{kind}");
        }
        let mut t = TurnTracker::new("j");
        let ev = frame(json!({ "type": "chat.calling_model", "job_id": "j", "display_name": "فراخوانی مدل" }));
        assert_eq!(t.apply(&ev), Some(Signal::Status { kind: "thinking", label: Some("فراخوانی مدل".into()) }));
        assert_eq!(status_from_job("processing"), Some("thinking"));
        assert_eq!(status_from_job("generation_pending"), Some("generating"));
        assert_eq!(status_from_job("done"), None);
    }

    #[test]
    fn snapshots_restore_state_after_a_reconnect() {
        let mut t = TurnTracker::new("j");
        let snap = frame(json!({ "type": "chat.snapshot", "job_id": "j", "status": "calling_model", "snapshot": true,
            "job_sequence": 10, "display_name": "در حال پردازش", "data": { "last_event_type": "chat.calling_model" } }));
        assert_eq!(t.apply(&snap), Some(Signal::Status { kind: "thinking", label: Some("در حال پردازش".into()) }));
        let snap = frame(json!({ "type": "chat.snapshot", "job_id": "j", "status": "awaiting_approval", "job_sequence": 11,
            "data": { "approval_id": "9c31cbd5-f15f", "tool": "start_generation" } }));
        assert_eq!(t.apply(&snap), Some(Signal::Approval { id: "9c31cbd5-f15f".into(), tool: Some("start_generation".into()) }));
        let snap = frame(json!({ "type": "chat.snapshot", "job_id": "j", "status": "streaming", "job_sequence": 12, "content_so_far": "abc" }));
        assert_eq!(t.apply(&snap), Some(Signal::Delta("abc".into())));
        // the sequence still applies to snapshots
        let stale = frame(json!({ "type": "chat.snapshot", "job_id": "j", "status": "queued", "job_sequence": 9 }));
        assert_eq!(t.apply(&stale), None);
    }

    #[test]
    fn approvals_and_resumed_jobs() {
        let mut t = TurnTracker::new("old");
        let ask = frame(json!({ "type": "chat.tool_approval_required", "job_id": "old", "job_sequence": 5,
            "data": { "approval_id": "a-1", "tool": "start_generation" } }));
        assert_eq!(t.apply(&ask), Some(Signal::Approval { id: "a-1".into(), tool: Some("start_generation".into()) }));
        let bad = frame(json!({ "type": "chat.tool_approval_required", "job_id": "old", "data": { "approval_id": "../x" } }));
        assert_eq!(t.apply(&bad), None, "an id that cannot go in a URL is ignored");

        // While waiting, the resumed job may speak before we know its id.
        t.set_awaiting(true);
        let early = frame(json!({ "type": "chat.delta", "job_id": "new", "job_sequence": 1, "content_so_far": "Hi" }));
        assert_eq!(t.apply(&early), None);
        let replay = t.switch_job("new");
        assert_eq!(replay, vec![Signal::Delta("Hi".into())]);
        assert_eq!(t.job(), "new");

        // A decision made elsewhere names the new job in its data.
        let mut t = TurnTracker::new("old");
        let approved = frame(json!({ "type": "chat.tool_approved", "job_id": "old", "data": { "job_id": "next" } }));
        assert_eq!(t.apply(&approved), Some(Signal::Resumed { job: "next".into(), decision: "approve" }));
        let mut t = TurnTracker::new("old");
        let rejected = frame(json!({ "type": "chat.tool_rejected", "job_id": "old", "data": {} }));
        assert_eq!(t.apply(&rejected), Some(Signal::Status { kind: "tool", label: None }));

        // Not waiting: foreign frames are not buffered.
        let mut t = TurnTracker::new("a");
        t.apply(&frame(json!({ "type": "chat.delta", "job_id": "b", "delta": "x" })));
        assert!(t.switch_job("b").is_empty());
    }

    /// Live smoke check: `cargo test -p roadeep -- --ignored live_ws`. A bogus
    /// token must be refused by the server (4401 / HTTP 401), not fail on TLS
    /// or a wrong path.
    #[test]
    #[ignore]
    fn live_ws_refuses_a_bogus_token() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        match rt.block_on(dial(http::base_url(), "not-a-token")) {
            Err(Dial::Unauthorized) => eprintln!("live: socket refused the bogus token as expected"),
            Err(Dial::Failed(why)) => panic!("socket failed before auth: {why}"),
            Ok(_) => panic!("a bogus token was accepted"),
        }
    }

    fn fake(generation: u64, opened: &AtomicU64) -> Result<(Live, broadcast::Receiver<WsMsg>), OpenError> {
        opened.fetch_add(1, Ordering::SeqCst);
        Ok(Live::new(generation))
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn concurrent_subscribers_share_one_socket() {
        let hub = Hub::default();
        let opened = AtomicU64::new(0);
        let connect = || async {
            tokio::task::yield_now().await; // let the other subscriber run meanwhile
            fake(1, &opened)
        };
        let (a, b) = rt().block_on(async { tokio::join!(hub.attach(1, false, connect), hub.attach(1, false, connect)) });
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(opened.load(Ordering::SeqCst), 1, "single flight");
        assert_eq!(hub.live.lock().unwrap().as_ref().unwrap().tx.receiver_count(), 2);
    }

    #[test]
    fn a_fresh_subscribe_never_closes_another_consumers_socket() {
        let hub = Hub::default();
        let opened = AtomicU64::new(0);
        let rt = rt();
        rt.block_on(hub.attach(1, false, || async { fake(1, &opened) })).unwrap();
        let first = hub.live.lock().unwrap().as_ref().unwrap().alive.clone();
        first.store(false, Ordering::SeqCst); // the socket drops under both consumers

        // B reconnects first and gets a new socket…
        rt.block_on(hub.attach(1, true, || async { fake(1, &opened) })).unwrap();
        let second = hub.live.lock().unwrap().as_ref().unwrap().alive.clone();
        // …and A's "fresh" reconnect reuses it instead of replacing it.
        rt.block_on(hub.attach(1, true, || async { fake(1, &opened) })).unwrap();
        assert_eq!(opened.load(Ordering::SeqCst), 2);
        assert!(second.load(Ordering::SeqCst), "B's socket is still up");

        // Another session's socket is never reused: closed, then replaced.
        rt.block_on(hub.attach(2, false, || async { fake(2, &opened) })).unwrap();
        assert!(!second.load(Ordering::SeqCst));
        assert_eq!(opened.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn close_stops_the_socket_and_one_being_opened() {
        let hub = Hub::default();
        let opened = AtomicU64::new(0);
        let rt = rt();
        rt.block_on(hub.attach(1, false, || async { fake(1, &opened) })).unwrap();
        let alive = hub.live.lock().unwrap().as_ref().unwrap().alive.clone();
        hub.close();
        assert!(!alive.load(Ordering::SeqCst));
        assert!(hub.live.lock().unwrap().is_none());

        // A sign-out while a connect is in flight: the new socket is stopped, not stored.
        let mut opening = None;
        let result = rt.block_on(hub.attach(1, false, || async {
            let (live, rx) = fake(1, &opened)?;
            opening = Some(live.alive.clone());
            hub.close();
            Ok((live, rx))
        }));
        assert!(matches!(result, Err(OpenError::Unavailable(_))));
        assert!(!opening.unwrap().load(Ordering::SeqCst));
        assert!(hub.live.lock().unwrap().is_none());

    }

    #[test]
    fn fallback_decision() {
        assert!(!stalled(false, Duration::from_secs(5)));
        assert!(stalled(false, SILENCE));
        assert!(!stalled(true, Duration::from_secs(600)), "a socket that spoke is trusted; REST checks cover long silences");
    }
}
