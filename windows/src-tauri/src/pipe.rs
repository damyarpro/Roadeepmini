// Named-pipe server for roadeep-hook.
//
// `\\.\pipe\roadeep-<sid>` — one instance per connection. Every hook event is
// forwarded to the island as a `hook` event. `PermissionRequest` is the only one
// that keeps its connection open: it waits for the island's decision and writes
// it back on the same pipe, which is how approving from the island works.
//
// Claude Code is never blocked by us. Three things guarantee it:
//   * roadeep-hook gives the connection 300 ms and exits cleanly if we are closed;
//   * we only wait for a human once the island has *confirmed* the card is on
//     screen, so a paused island or a webview that is not listening costs a few
//     hundred milliseconds, not two minutes;
//   * whatever happens we drop the connection after the decision timeout, and
//     the terminal takes over.
//
// What we write back is the bare word `allow` or `deny` — or, when Claude Code
// asked a question, `{"answers":[…]}` with what was picked on the island, by
// position. Turning either into the documented hookSpecificOutput JSON is
// roadeep-hook's job, so the wire format Claude Code expects lives in exactly
// one place.
//
// A card folded away on the island is still waiting: only a click, a decline
// or the timeout ends a request.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::mpsc;

use crate::coding_hooks::protocol;
use crate::island::WINDOW_LABEL;
use crate::log;

/// Slightly under roadeep-hook's own 110 s wait, so we always answer first.
const DECISION_TIMEOUT: Duration = Duration::from_secs(108);
/// How long the island gets to say "the card is up". This is the whole of B4:
/// without it, an island that is paused, hidden behind a crashed webview or
/// simply not listening would leave Claude Code staring at a prompt nobody can
/// see for nearly two minutes.
const ACK_TIMEOUT: Duration = Duration::from_millis(800);
const MAX_PAYLOAD: usize = 1 << 20;
static CONNECTIONS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(64);

/// What the island can say about a permission request.
pub enum Reply {
    /// The card is on screen and a human can act on it.
    Ack,
    /// A human clicked: `allow` or `deny`, or a question's `{"answers":[…]}` line.
    Decision(String),
    /// Nobody can act on it — paused, or another request already holds the card.
    Decline,
}

/// Permission requests the island has been told about.
#[derive(Default)]
pub struct Pending(pub Mutex<HashMap<String, mpsc::Sender<Reply>>>);

pub fn authorize_reply(window: &str, request: &str, decision: Option<&str>) -> Result<(), String> {
    if window != WINDOW_LABEL || request.is_empty() || request.len() > 64 || !request.bytes().all(|b|b.is_ascii_digit() || b==b'-') || decision.is_some_and(|d|!matches!(d,"allow"|"deny"|"always")) {
        log::line("hook outcome=invalid-reply");
        return Err("hook-invalid-reply".into());
    }
    Ok(())
}

/// The island's answers to Claude Code's question as the one line the relay
/// reads: `{"answers":[…]}`, one entry per question in order — an option index,
/// or a non-empty list of distinct indexes for a multi-select question. Only the
/// shape is checked here: whether the answers fit the questions is roadeep-hook's
/// call, as it holds the questions exactly as Claude Code asked them.
pub fn question_answers_line(answers: &Value) -> Result<String, String> {
    let index = |v: &Value| v.as_u64().is_some_and(|i| i < protocol::MAX_OPTIONS as u64);
    let fits = |a: &Value| match a {
        Value::Number(_) => index(a),
        Value::Array(list) => {
            let mut seen: Vec<u64> = list.iter().filter_map(Value::as_u64).collect();
            seen.sort_unstable();
            seen.dedup();
            !list.is_empty() && list.iter().all(index) && seen.len() == list.len()
        }
        _ => false,
    };
    match answers.as_array() {
        Some(list) if !list.is_empty() && list.len() <= protocol::MAX_QUESTIONS && list.iter().all(fits) => {
            Ok(json!({ "answers": list }).to_string())
        }
        _ => {
            log::line("hook outcome=invalid-answers");
            Err("hook-invalid-reply".into())
        }
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// The user SID (user name only if the SID can't be read): keeps two accounts
/// on the same machine from ever meeting on a pipe.
pub(crate) fn user_key() -> String {
    crate::win_user::current_user_sid()
        .unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_else(|_| "user".into()))
}

/// `\\.\pipe\roadeep-<sid>` — must match roadeep-hook's `pipe_path()` exactly.
pub fn pipe_name() -> String {
    format!(r"\\.\pipe\roadeep-{}", user_key())
}

/// Listens on the current pipe and, for the transition, on the one relays
/// staged by builds under the former name connect to (they may still be
/// registered in places the app can't see).
pub fn start(app: AppHandle) {
    let key = user_key();
    for (label, name) in [("relay", pipe_name()), ("relay-legacy", crate::migrate::legacy_hook_pipe(&key))] {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // first_pipe_instance also means we refuse to join a pipe somebody else
            // already owns under our name, rather than serving on top of it.
            let server = match ServerOptions::new().first_pipe_instance(true).create(&name) {
                Ok(s) => s,
                Err(err) => {
                    log::line(format!("{label}: cannot open the pipe: {err}"));
                    return;
                }
            };
            serve(label, server, || ServerOptions::new().create(&name), |connected| {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { handle(app, connected).await });
            })
            .await;
        });
    }
}

/// How long a failing `connect` waits before the next try, doubling up to the cap.
const CONNECT_BACKOFF_START: Duration = Duration::from_millis(200);
const CONNECT_BACKOFF_MAX: Duration = Duration::from_secs(5);

/// The accept loop both pipes share: wait for a client, hand the connected
/// instance to `on_client`, listen on a fresh one. Returns only when no new
/// instance can be created.
///
/// A failed `connect` leaves the instance in a state retrying cannot be trusted
/// to clear, so it is replaced rather than retried, with a backoff and one log
/// line per streak instead of one per attempt. (A client that came and went
/// before we accepted, ERROR_NO_DATA, is reported by mio as a connection; its
/// handler reads nothing and drops it.)
pub(crate) async fn serve(
    label: &str,
    mut server: NamedPipeServer,
    create: impl Fn() -> std::io::Result<NamedPipeServer>,
    mut on_client: impl FnMut(NamedPipeServer),
) {
    let mut failures: u32 = 0;
    let mut backoff = CONNECT_BACKOFF_START;
    loop {
        if let Err(err) = server.connect().await {
            failures += 1;
            if failures == 1 || failures.is_multiple_of(100) {
                log::line(format!("{label}: pipe connect failed ({failures} in a row): {err}"));
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(CONNECT_BACKOFF_MAX);
            // The new instance exists before the old one closes, so the name is
            // never free for somebody else to take.
            match create() {
                Ok(fresh) => server = fresh,
                Err(err) => {
                    log::line(format!("{label}: cannot recreate the pipe after a failed connect: {err}"));
                    return;
                }
            }
            continue;
        }
        if failures > 0 {
            log::line(format!("{label}: pipe accepting again after {failures} failed connect(s)"));
            failures = 0;
            backoff = CONNECT_BACKOFF_START;
        }
        // Hand the connected instance over and listen on a fresh one.
        let next = match create() {
            Ok(s) => s,
            Err(err) => {
                log::line(format!("{label}: cannot reopen the pipe: {err}"));
                return;
            }
        };
        on_client(std::mem::replace(&mut server, next));
    }
}

/// Writing the whole answer is bounded: a client that stops reading cannot pin the task.
const ANSWER_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the client gets to read the answer and hang up.
pub(crate) const CLIENT_CLOSE_WAIT: Duration = Duration::from_secs(5);

/// Writes `bytes` and closes the connection without losing any of them.
///
/// Never `DisconnectNamedPipe` after a write: it discards whatever the client
/// has not read yet, and `flush` on a tokio pipe does not wait for the reader.
/// Both clients (roadeep-hook, roadeep-mcp) hang up once they have read the
/// newline, so waiting for that — bounded — then dropping the handle delivers
/// every byte; a client that lingers is cut off after CLIENT_CLOSE_WAIT.
pub(crate) async fn answer_and_close(mut pipe: NamedPipeServer, bytes: &[u8]) -> std::io::Result<()> {
    let written = match tokio::time::timeout(ANSWER_WRITE_TIMEOUT, pipe.write_all(bytes)).await {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "the client stopped reading")),
    };
    if written.is_ok() {
        let mut sink = [0u8; 512];
        let wait = async {
            // Anything the client still sends is ignored; 0 or an error means it hung up.
            while let Ok(n) = pipe.read(&mut sink).await {
                if n == 0 {
                    break;
                }
            }
        };
        let _ = tokio::time::timeout(CLIENT_CLOSE_WAIT, wait).await;
    }
    drop(pipe);
    written
}

async fn handle(app: AppHandle, mut pipe: NamedPipeServer) {
    let Ok(_slot)=CONNECTIONS.try_acquire() else {return;};
    let deadline=tokio::time::Instant::now()+Duration::from_secs(2);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match tokio::time::timeout_at(deadline,pipe.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&b'\n') || buf.len() > MAX_PAYLOAD {
                    break;
                }
            }
            _ => return,
        }
    }
    if buf.len() > MAX_PAYLOAD { return; }
    let line = match buf.iter().position(|b| *b == b'\n') {
        Some(i) => &buf[..i],
        None => &buf[..],
    };
    let Ok(mut payload) = serde_json::from_slice::<Value>(line) else { return };
    if !payload.is_object() {
        return;
    }

    let provider = payload.get("provider").and_then(Value::as_str).unwrap_or("claude").to_string();
    let original = payload.get("original_event").or_else(||payload.get("hook_event_name")).and_then(Value::as_str).unwrap_or_default().to_string();
    let Some(normalized) = protocol::normalize(&payload, &provider, &original) else { return; };
    payload = normalized;
    redact_payload(&mut payload);
    let event = payload["hook_event_name"].as_str().unwrap_or_default().to_string();
    crate::shortcuts::session_window::note(&pipe, &payload, &event);

    if event != "PermissionRequest" {
        log::line(format!("hook provider={provider} event={event}"));
        let _ = app.emit_to(WINDOW_LABEL, "hook", payload);
        let _ = pipe.disconnect();
        return;
    }

    let id = format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let (tx, mut rx) = mpsc::channel::<Reply>(4);
    {
        let pending = app.state::<Pending>();
        let Ok(mut slots)=pending.0.lock() else { return; };
        if slots.len() >= 32 { log::line("hook outcome=queue-full"); return; }
        slots.insert(id.clone(), tx);
    }
    payload["request_id"] = json!(id);
    log::line(format!("hook provider={provider} event=PermissionRequest id={id}"));
    let _ = app.emit_to(WINDOW_LABEL, "hook", payload);

    let decision = wait_for_decision(&id, &mut rx).await;
    app.state::<Pending>().0.lock().unwrap().remove(&id);

    // No decision: say nothing at all. roadeep-hook then writes nothing to stdout
    // and Claude Code asks in the terminal, exactly as if Roadeep were closed.
    match decision {
        Some(d) => {
            if let Err(err) = answer_and_close(pipe, format!("{d}\n").as_bytes()).await {
                log::line(format!("hook id={id} decision not delivered: {err}"));
            }
        }
        None => {
            let _ = pipe.disconnect();
        }
    }
}

/// Longest edit string the island gets for the live diff, in characters (the
/// relay already caps them at 256 KB).
const MAX_DIFF_FIELD_CHARS: usize = 256 * 1024;

/// Masks secrets and caps every string before the island sees it. The edit text
/// of a finished Claude Code edit keeps its length, as the live diff needs it
/// whole; when masking or the cap changed it, the payload says the diff cannot
/// be trusted (protocol::DIFF_TRUNCATED).
fn redact_payload(payload: &mut Value) {
    let keeps_diff = protocol::keeps_diff(
        payload["provider"].as_str().unwrap_or_default(),
        payload["hook_event_name"].as_str().unwrap_or_default(),
        payload["tool_name"].as_str(),
    );
    let input = if keeps_diff {
        payload.as_object_mut().and_then(|map| map.remove("tool_input"))
    } else {
        None
    };
    redact(payload);
    if let Some(mut input) = input {
        let mut changed = false;
        redact_diff(&mut input, &mut changed);
        if let Some(map) = payload.as_object_mut() {
            map.insert("tool_input".into(), input);
            if changed {
                map.insert(protocol::DIFF_TRUNCATED.into(), Value::Bool(true));
            }
        }
    }
}

fn redact(v: &mut Value) {
    match v {
        Value::String(s) => *s = crate::coding::clean(s, 2000),
        Value::Array(a) => a.iter_mut().for_each(redact),
        Value::Object(m) => m.values_mut().for_each(redact),
        _ => {}
    }
}

/// `tool_input` of a finished edit: the edit strings are masked like the rest
/// but keep their length; anything else gets the ordinary cap.
fn redact_diff(v: &mut Value, changed: &mut bool) {
    match v {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                match value {
                    Value::String(s) if protocol::DIFF_FIELDS.contains(&key.as_str()) => {
                        let cleaned = crate::coding::clean(s, MAX_DIFF_FIELD_CHARS);
                        // clean() also drops control characters, the \r of CRLF
                        // files among them; that leaves the lines as they were.
                        let kept: String =
                            s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
                        if cleaned != kept {
                            *changed = true;
                        }
                        *s = cleaned;
                    }
                    _ => redact_diff(value, changed),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| redact_diff(item, changed)),
        Value::String(s) => *s = crate::coding::clean(s, 2000),
        _ => {}
    }
}

/// Two waits: a short one for "the card is up", then the long one for a human.
async fn wait_for_decision(id: &str, rx: &mut mpsc::Receiver<Reply>) -> Option<String> {
    match tokio::time::timeout(ACK_TIMEOUT, rx.recv()).await {
        Ok(Some(Reply::Ack)) => {}
        // A click that beats the ack is still a click.
        Ok(Some(Reply::Decision(d))) => {
            log::line(format!("hook id={id} answered {}", loggable(&d)));
            return Some(d);
        }
        Ok(Some(Reply::Decline)) => {
            log::line(format!("hook id={id} not shown — terminal takes over"));
            return None;
        }
        Ok(None) => return None,
        Err(_) => {
            log::line(format!("hook id={id} island never acknowledged — terminal takes over"));
            return None;
        }
    }

    match tokio::time::timeout(DECISION_TIMEOUT, rx.recv()).await {
        Ok(Some(Reply::Decision(d))) => {
            log::line(format!("hook id={id} answered {}", loggable(&d)));
            Some(d)
        }
        Ok(Some(Reply::Decline)) => {
            log::line(format!("hook id={id} released without a decision"));
            None
        }
        _ => {
            log::line(format!("hook id={id} timed out — terminal takes over"));
            None
        }
    }
}

/// The log says a question was answered, never with what.
fn loggable(decision: &str) -> &str {
    if decision.starts_with('{') { "a question" } else { decision }
}

fn send(app: &AppHandle, request_id: &str, reply: Reply, keep: bool) {
    let sender = {
        let pending = app.state::<Pending>();
        let mut map = pending.0.lock().unwrap();
        if keep { map.get(request_id).cloned() } else { map.remove(request_id) }
    };
    match sender {
        Some(tx) => {
            let _ = tx.try_send(reply);
        }
        None => log::line(format!("reply for id={request_id} — no pending request")),
    }
}

/// The island has the card on screen; the long wait may begin.
pub fn acknowledge(app: &AppHandle, request_id: &str) {
    send(app, request_id, Reply::Ack, true);
}

/// Nobody can act on this one — paused, or another card already holds the view.
pub fn decline(app: &AppHandle, request_id: &str) {
    log::line(format!("decline id={request_id}"));
    send(app, request_id, Reply::Decline, false);
}

/// Called by the island's Allow / Deny buttons. Only ever a bare word: turning
/// it into Claude Code's JSON is roadeep-hook's job.
pub fn answer(app: &AppHandle, request_id: &str, decision: &str) {
    let word = match decision {
        "allow" | "always" => "allow",
        _ => "deny",
    };
    log::line(format!("decision id={request_id} {word}"));
    send(app, request_id, Reply::Decision(word.to_string()), false);
}

/// Called when the island answers a question Claude Code asked. `line` comes
/// from `question_answers_line`; roadeep-hook checks it against the questions.
pub fn answer_question(app: &AppHandle, request_id: &str, line: String) {
    log::line(format!("decision id={request_id} answered a question"));
    send(app, request_id, Reply::Decision(line), false);
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn replies_require_island_and_bounded_known_decisions() {
        assert!(authorize_reply("island","123-4",Some("allow")).is_ok());
        for (window,id,decision) in [("settings","123-4","allow"),("island","token=private","allow"),("island","123-4","arbitrary")] {assert!(authorize_reply(window,id,Some(decision)).is_err());}
    }
    #[tokio::test] async fn decline_or_dropped_ui_never_decides() {
        let (tx,mut rx)=mpsc::channel(4);tx.send(Reply::Decline).await.unwrap();assert!(wait_for_decision("fixture",&mut rx).await.is_none());
        let (tx,mut rx)=mpsc::channel(4);drop(tx);assert!(wait_for_decision("fixture",&mut rx).await.is_none());
    }
    #[tokio::test] async fn unacknowledged_card_releases_to_provider() {
        let (_tx,mut rx)=mpsc::channel(4);
        assert!(wait_for_decision("fixture",&mut rx).await.is_none());
    }
    #[tokio::test] async fn acknowledgement_only_does_not_grant_and_explicit_click_does() {
        let (tx,mut rx)=mpsc::channel(4);tx.send(Reply::Ack).await.unwrap();drop(tx);assert!(wait_for_decision("fixture",&mut rx).await.is_none());
        let (tx,mut rx)=mpsc::channel(4);tx.send(Reply::Ack).await.unwrap();tx.send(Reply::Decision("deny".into())).await.unwrap();assert_eq!(wait_for_decision("fixture",&mut rx).await,Some("deny".into()));
    }

    #[test]
    fn question_answers_go_back_as_one_line_by_position() {
        assert_eq!(question_answers_line(&json!([1, [0, 2]])).unwrap(), r#"{"answers":[1,[0,2]]}"#);
        for bad in [
            json!([]),
            json!({ "Which one?": "A" }),
            json!(["A"]),
            json!([-1]),
            json!([1.5]),
            json!([protocol::MAX_OPTIONS]),
            json!([[]]),
            json!([[1, 1]]),
            json!([[1, "2"]]),
            json!([null]),
            json!(vec![0; protocol::MAX_QUESTIONS + 1]),
        ] {
            assert!(question_answers_line(&bad).is_err(), "{bad}");
        }
        // The log never carries the answers.
        assert_eq!(loggable(r#"{"answers":[1]}"#), "a question");
        assert_eq!(loggable("deny"), "deny");
    }

    #[tokio::test] async fn an_answered_question_is_a_decision() {
        let line = question_answers_line(&json!([0])).unwrap();
        let (tx,mut rx)=mpsc::channel(4);tx.send(Reply::Ack).await.unwrap();tx.send(Reply::Decision(line.clone())).await.unwrap();
        assert_eq!(wait_for_decision("fixture",&mut rx).await,Some(line));
    }

    fn edit(input: Value) -> Value {
        protocol::normalize(&json!({ "tool_name": "Edit", "tool_input": input, "cwd": "C:/p" }), "claude", "PostToolUse").unwrap()
    }

    #[test]
    fn a_finished_edit_keeps_its_text_and_says_when_it_was_masked() {
        let crlf = "fn a() {}\r\n".repeat(1_000);
        let mut v = edit(json!({ "file_path": "a.rs", "old_string": crlf, "new_string": "x" }));
        redact_payload(&mut v);
        assert_eq!(v["tool_input"]["old_string"].as_str().unwrap(), "fn a() {}\n".repeat(1_000));
        assert!(v.get(protocol::DIFF_TRUNCATED).is_none(), "dropping \\r changes no line");

        let mut v = edit(json!({ "old_string": "token = \"abcdefgh12345\"", "new_string": "y" }));
        redact_payload(&mut v);
        assert!(v["tool_input"]["old_string"].as_str().unwrap().contains("[redacted]"));
        assert_eq!(v[protocol::DIFF_TRUNCATED], true);

        // Before it happens, the same text is not even forwarded; other strings keep the cap.
        let mut v = protocol::normalize(&json!({ "tool_name": "Bash", "tool_input": { "command": "x".repeat(5_000) } }), "claude", "PostToolUse").unwrap();
        redact_payload(&mut v);
        assert!(v["tool_input"]["command"].as_str().unwrap().chars().count() <= 2_000 + "\n[truncated]".len());
    }
}
