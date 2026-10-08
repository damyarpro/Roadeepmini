// Conversation history for the island: list the account's chat threads, open
// one (it becomes the current thread, so the next message continues it), and
// delete one. Mirrors the mobile client (features/chat/threads.ts, api.ts,
// schemas.ts) and the web's header paging (network/rtk/chat/thread-paging.ts).
//
// What reaches the webview is bounded: a page is at most 30 threads, a thread
// at most the last 200 messages of at most 20 000 characters each.

use serde::Serialize;
use serde_json::Value;

use super::chat::tools::{self, ToolStepView, UserMessage};
use super::chat::{shown_char, ChatState, PERSIAN_SUFFIX};
use super::generation::valid_id;
use super::http::{codes, Call, RoadeepError};
use super::Roadeep;
use crate::log;

pub const MAX_PAGE: u32 = 30;
const MAX_OFFSET: u32 = 100_000;
const MAX_MESSAGES: usize = 200;
const MAX_MESSAGE_CHARS: usize = 20_000;
const MAX_TITLE_CHARS: usize = 120;
const MAX_PREVIEW_CHARS: usize = 140;
const MAX_MODEL_CHARS: usize = 200;
/// Web wave threads carry this historic prefix; they belong to that tool.
const WAVE_THREAD_PREFIX: &str = "wave-";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadSummary {
    pub id: String,
    /// "" when the thread has no title (the UI shows its own placeholder).
    pub title: String,
    /// The last message in one plain line; "" when there is none.
    pub preview: String,
    /// ISO 8601; "" when the server sent none.
    pub updated_at: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPage {
    pub items: Vec<ChatThreadSummary>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryMessage {
    pub id: String,
    /// "user" | "assistant".
    pub role: String,
    pub text: String,
    pub created_at: String,
    /// Assistant messages: the MCP tool steps that led to this answer (the
    /// app's tool messages themselves are folded away). An assistant message
    /// with no text holds the steps of a turn that ended without an answer.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_steps: Vec<ToolStepView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadHistory {
    pub id: String,
    pub title: String,
    pub model: Option<String>,
    pub messages: Vec<ChatHistoryMessage>,
}

fn id_of(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn text_of(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn one_line(text: &str, max: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().filter(|c| shown_char(*c)).take(max).collect()
}

fn clamp_block(text: &str, max: usize) -> String {
    text.chars().filter(|c| shown_char(*c) || matches!(c, '\n' | '\t')).take(max).collect()
}

/// Threads of standalone tools (wave, marketing…) are not chat history. A
/// missing `source` shows the thread: never hide a real conversation.
fn tool_scoped(record: &Value, id: &str) -> bool {
    id.starts_with(WAVE_THREAD_PREFIX) || text_of(record, "source").is_some_and(|s| s != "chat")
}

/// The first human string under a preferred key, for previews that are JSON.
fn human_string(v: &Value, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    match v {
        Value::Array(items) => items.iter().find_map(|i| human_string(i, depth + 1)),
        Value::Object(map) => ["reply", "title", "text", "content", "message"]
            .iter()
            .find_map(|k| map.get(*k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_string))
            .or_else(|| map.values().find_map(|v| human_string(v, depth + 1))),
        _ => None,
    }
}

/// One plain line: JSON previews (tool threads) reduced to their human text,
/// markdown marks dropped, whitespace flattened, cut short.
pub fn sanitize_preview(raw: &str) -> String {
    let mut trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // The app's tool messages: the user's text after a tool block, else nothing.
    if trimmed.starts_with("[Roadeep tools") || trimmed.to_ascii_lowercase().starts_with("<roadeep_tool") {
        match tools::split_block(trimmed) {
            (rest, Some(_)) => trimmed = rest.trim(),
            _ => return String::new(),
        }
    }
    let looks_json = trimmed.starts_with('{') || (trimmed.starts_with('[') && !trimmed.starts_with("[!") && !trimmed.contains("]("));
    let text = if looks_json {
        // Previews are cut server-side, so the JSON often does not parse.
        serde_json::from_str::<Value>(trimmed).ok().and_then(|v| human_string(&v, 0)).unwrap_or_default()
    } else {
        trimmed.to_string()
    };
    let plain: String = text
        .lines()
        .map(|line| {
            let l = line.trim_start();
            let l = l.trim_start_matches(['#', '>', '-', '*', '+', '•']).trim_start();
            l.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`' | '~' | '#' | '|'))
        .collect();
    one_line(&plain, MAX_PREVIEW_CHARS)
}

/// One page of `GET /v1/chat/threads/?type=chat`: a bare array (today), or
/// `{results|items, next, count}`. `X-Has-More` wins when the server sends it.
pub fn parse_threads(data: &Value, header_has_more: Option<bool>, offset: u32, limit: u32) -> ThreadPage {
    let (records, next, count, paginated) = match data {
        Value::Array(items) => (items.as_slice(), None, None, false),
        Value::Object(map) => {
            let list = ["results", "items"]
                .iter()
                .filter_map(|k| map.get(*k).and_then(Value::as_array))
                .find(|l| !l.is_empty())
                .or_else(|| map.get("items").and_then(Value::as_array))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            (list, map.get("next"), map.get("count").and_then(Value::as_u64), true)
        }
        _ => (&[][..], None, None, false),
    };
    let received = records.len() as u64;
    let has_more = match header_has_more {
        Some(more) => more,
        None if paginated && next.is_some() => !matches!(next, Some(Value::Null) | Some(Value::Bool(false))),
        None if paginated && count.is_some() => count.unwrap_or(0) > u64::from(offset) + received,
        // A bare array may or may not honour limit/offset: a full page means "maybe more".
        None => received >= u64::from(limit),
    };
    let items = records
        .iter()
        .filter_map(|r| {
            let id = id_of(r, "thread_id").or_else(|| id_of(r, "id")).filter(|id| valid_id(id))?;
            if tool_scoped(r, &id) {
                return None;
            }
            Some(ChatThreadSummary {
                title: text_of(r, "title").map(|t| one_line(&t, MAX_TITLE_CHARS)).unwrap_or_default(),
                preview: text_of(r, "last_message_preview").map(|p| sanitize_preview(&p)).unwrap_or_default(),
                updated_at: text_of(r, "updated_at").or_else(|| text_of(r, "created_at")).unwrap_or_default(),
                model: text_of(r, "model").map(|m| one_line(&m, MAX_MODEL_CHARS)),
                id,
            })
        })
        .take(MAX_PAGE as usize)
        .collect();
    ThreadPage { items, has_more }
}

/// What the island sent in a thread's first message besides the question: a
/// local agent's brief, the window context, the tool list (or its one-line
/// reminder, on any message), the Persian instruction. History shows what the
/// user typed.
pub fn display_user_text(text: &str) -> String {
    strip_extras(text).0
}

/// `display_user_text`, and the thread's tool state a tool block in the
/// message sets (tools::split_block).
fn strip_extras(text: &str) -> (String, Option<Option<String>>) {
    let mut rest = text;
    if let Some(after) = rest.strip_prefix(crate::clock::CHAT_LINE_PREFIX) {
        if let Some(end) = after.find("]\n\n") {
            rest = &after[end + 3..];
        }
    }
    if let Some(after) = rest.strip_prefix("[Agent instructions]\n") {
        if let Some(end) = after.find("\n[/Agent instructions]\n\n") {
            rest = &after[end + "\n[/Agent instructions]\n\n".len()..];
        }
    }
    if rest.starts_with("Context — App: ") {
        if let Some(end) = rest.find("\n\n") {
            rest = &rest[end + 2..];
        }
    }
    let (rest, tools_state) = tools::split_block(rest);
    (rest.strip_suffix(PERSIAN_SUFFIX).unwrap_or(rest).to_string(), tools_state)
}

/// `GET …/messages/` → (title, model, active user/assistant messages,
/// chronological, the last `MAX_MESSAGES`). Edited and regenerated turns keep
/// their old revisions with `is_active: false`; those are dropped.
#[cfg(test)]
pub fn parse_messages(data: &Value) -> (String, Option<String>, Vec<ChatHistoryMessage>) {
    let thread = parse_thread(data);
    (thread.title, thread.model, thread.messages)
}

/// What a thread's messages say, for the island.
pub struct ParsedThread {
    pub title: String,
    pub model: Option<String>,
    pub messages: Vec<ChatHistoryMessage>,
    /// The tool list the thread was last given (its fingerprint); None when it
    /// has none, or tools were turned off after it.
    pub tools: Option<String>,
}

/// One message as the server keeps it.
struct RawMessage {
    id: String,
    role: String,
    text: String,
    created_at: String,
}

pub fn parse_thread(data: &Value) -> ParsedThread {
    let list = data.get("messages").and_then(Value::as_array).or_else(|| data.as_array());
    let mut raw: Vec<RawMessage> = list
        .into_iter()
        .flatten()
        .filter(|m| m.get("is_active").and_then(Value::as_bool) != Some(false))
        .filter_map(|m| {
            let role = text_of(m, "role")?;
            if role != "user" && role != "assistant" {
                return None;
            }
            let text = m.get("content").or_else(|| m.get("message")).and_then(Value::as_str).unwrap_or("").to_string();
            Some(RawMessage { id: id_of(m, "id")?, role, text, created_at: text_of(m, "created_at").unwrap_or_default() })
        })
        .collect();
    // ISO timestamps of one server sort as text; a stable sort keeps the server's order on ties.
    if raw.iter().all(|m| !m.created_at.is_empty()) {
        raw.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    }
    let tools = raw
        .iter()
        .filter(|m| m.role == "user")
        .fold(None, |state, m| strip_extras(&m.text).1.unwrap_or(state));
    let mut messages = fold_tool_messages(raw);
    let skip = messages.len().saturating_sub(MAX_MESSAGES);
    messages.drain(..skip);
    let title = text_of(data, "title").map(|t| one_line(&t, MAX_TITLE_CHARS)).unwrap_or_default();
    let model = text_of(data, "model").map(|m| one_line(&m, MAX_MODEL_CHARS));
    ParsedThread { title, model, messages, tools }
}

/// A tool call waiting for its result in `fold_tool_messages`.
struct PendingStep {
    qualified: String,
    view: ToolStepView,
}

/// The tool-step rows of a fold, in order.
fn take_steps(pending: &mut Vec<PendingStep>) -> Vec<ToolStepView> {
    pending.drain(..).map(|p| p.view).collect()
}

fn history_message(id: String, role: &str, text: &str, created_at: String, tool_steps: Vec<ToolStepView>) -> Option<ChatHistoryMessage> {
    let text = clamp_block(text.trim(), MAX_MESSAGE_CHARS);
    // Media-only turns: nothing the island can show.
    if text.is_empty() && tool_steps.is_empty() {
        return None;
    }
    Some(ChatHistoryMessage { id, role: role.to_string(), text, created_at, tool_steps })
}

/// Steps of a turn that ended without an answer: an assistant message of their own.
fn orphan_steps(pending: &mut Vec<PendingStep>, created_at: String) -> Option<ChatHistoryMessage> {
    let first = pending.first()?;
    let id = format!("{}-steps", first.view.id);
    history_message(id, "assistant", "", created_at, take_steps(pending))
}

/// The island's view of a thread with MCP tool steps: the app's own messages
/// (tool results, notes) are hidden, a tool call becomes a step on the answer
/// that follows it, and a call the app sent a correction for is hidden too —
/// so a reopened thread reads like the live one.
fn fold_tool_messages(raw: Vec<RawMessage>) -> Vec<ChatHistoryMessage> {
    let mut out: Vec<ChatHistoryMessage> = Vec::new();
    let mut pending: Vec<PendingStep> = Vec::new();
    // An unusable call: hidden when a correction follows, shown as text otherwise.
    let mut held: Option<RawMessage> = None;
    let flush = |held: &mut Option<RawMessage>, pending: &mut Vec<PendingStep>, out: &mut Vec<ChatHistoryMessage>| {
        if let Some(m) = held.take() {
            out.extend(history_message(m.id, &m.role, &m.text, m.created_at, take_steps(pending)));
        }
    };
    for m in raw {
        if m.role == "assistant" {
            flush(&mut held, &mut pending, &mut out);
            match tools::parse_reply(&m.text) {
                tools::Parsed::Call { tool, arguments } => {
                    let (server, name) = tools::split_qualified(&tool);
                    pending.push(PendingStep {
                        qualified: tool,
                        view: ToolStepView {
                            id: m.id,
                            server,
                            tool: name,
                            // Until a result says otherwise: the turn stopped here.
                            state: "stopped",
                            arguments: tools::arguments_preview(&arguments),
                            result: None,
                            error: None,
                        },
                    });
                }
                tools::Parsed::Invalid(_) => held = Some(m),
                tools::Parsed::Answer => {
                    out.extend(history_message(m.id, &m.role, &m.text, m.created_at, take_steps(&mut pending)));
                }
            }
            continue;
        }
        match tools::classify_user(&m.text) {
            UserMessage::Note { invalid: true } => {
                // The call the correction was about never ran: hide it.
                if held.take().is_none() && pending.last().is_some_and(|p| p.view.result.is_none()) {
                    pending.pop();
                }
            }
            UserMessage::Note { invalid: false } => flush(&mut held, &mut pending, &mut out),
            UserMessage::Result { tool, ok, body } => {
                flush(&mut held, &mut pending, &mut out);
                let open = pending.iter_mut().rev().find(|p| p.qualified == tool && p.view.state == "stopped");
                if let Some(step) = open {
                    step.view.state = if ok {
                        "done"
                    } else if body == tools::DECLINED_TEXT {
                        "declined"
                    } else {
                        "error"
                    };
                    step.view.result = (step.view.state != "declined").then(|| tools::result_preview(&body));
                }
            }
            UserMessage::Plain => {
                flush(&mut held, &mut pending, &mut out);
                out.extend(orphan_steps(&mut pending, m.created_at.clone()));
                let text = display_user_text(&m.text);
                out.extend(history_message(m.id, &m.role, &text, m.created_at, Vec::new()));
            }
        }
    }
    flush(&mut held, &mut pending, &mut out);
    out.extend(orphan_steps(&mut pending, String::new()));
    out
}

fn check_thread_id(thread_id: &str) -> Result<(), RoadeepError> {
    if valid_id(thread_id) {
        Ok(())
    } else {
        Err(RoadeepError::validation("thread_id", "Unknown conversation."))
    }
}

pub async fn list(rd: &Roadeep, offset: u32, limit: u32) -> Result<ThreadPage, RoadeepError> {
    let limit = limit.clamp(1, MAX_PAGE);
    let offset = offset.min(MAX_OFFSET);
    let (data, has_more) = rd
        .request_paged(Call::get(format!("/v1/chat/threads/?type=chat&limit={limit}&offset={offset}")))
        .await?;
    Ok(parse_threads(&data, has_more, offset, limit))
}

pub async fn open(rd: &Roadeep, chat: &ChatState, thread_id: &str) -> Result<ThreadHistory, RoadeepError> {
    check_thread_id(thread_id)?;
    let data = rd.request(Call::get(format!("/v1/chat/threads/{thread_id}/messages/?type=chat"))).await?;
    if data.is_null() {
        return Err(RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected response."));
    }
    let ParsedThread { title, model, messages, tools } = parse_thread(&data);
    chat.open_thread(thread_id.to_string(), model.clone());
    // The next message only reminds the model of a tool list it already has.
    chat.note_thread_tools(tools);
    log::line(format!("roadeep: opened a conversation ({} messages)", messages.len()));
    Ok(ThreadHistory { id: thread_id.to_string(), title, model, messages })
}

pub async fn delete(rd: &Roadeep, chat: &ChatState, thread_id: &str) -> Result<(), RoadeepError> {
    check_thread_id(thread_id)?;
    match rd.request(Call::delete(format!("/v1/chat/threads/{thread_id}/"))).await {
        Ok(_) => {}
        // Already gone is what was asked for.
        Err(err) if err.status == Some(404) || err.code == "THREAD_NOT_FOUND" => {}
        Err(err) => return Err(err),
    }
    chat.forget_thread(thread_id);
    Ok(())
}

/// Best effort, for threads the app created for itself (agent drafts): a
/// failure is logged and never fails what it was cleaning up after.
pub async fn delete_quietly(rd: &Roadeep, thread_id: &str, why: &str) {
    if !valid_id(thread_id) {
        return;
    }
    match rd.request(Call::delete(format!("/v1/chat/threads/{thread_id}/"))).await {
        Ok(_) => {}
        Err(err) if err.status == Some(404) || err.code == "THREAD_NOT_FOUND" => {}
        Err(err) => log::line(format!(
            "roadeep: could not delete the {why} thread ({}) {}",
            err.code,
            err.request_id.as_deref().unwrap_or("")
        )),
    }
}

#[tauri::command]
pub async fn chat_threads(roadeep: tauri::State<'_, Roadeep>, offset: u32, limit: u32) -> Result<ThreadPage, RoadeepError> {
    list(&roadeep, offset, limit).await
}

/// Loads a conversation and makes it the current one: the next chat_send continues it.
#[tauri::command]
pub async fn chat_thread_open(
    roadeep: tauri::State<'_, Roadeep>,
    chat: tauri::State<'_, ChatState>,
    thread_id: String,
) -> Result<ThreadHistory, RoadeepError> {
    open(&roadeep, &chat, &thread_id).await
}

/// Deletes a conversation; the chat starts over when it was the current one.
#[tauri::command]
pub async fn chat_thread_delete(
    roadeep: tauri::State<'_, Roadeep>,
    chat: tauri::State<'_, ChatState>,
    thread_id: String,
) -> Result<(), RoadeepError> {
    delete(&roadeep, &chat, &thread_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roadeep::chat::compose_message;
    use serde_json::json;

    fn thread(id: &str) -> Value {
        json!({ "thread_id": id, "source": "chat", "title": format!("T {id}"), "last_message_preview": "**Hi** there",
                "updated_at": "2026-09-11T08:10:00Z", "created_at": "2026-09-11T08:00:00Z", "model": "m" })
    }

    #[test]
    fn thread_list_shapes_and_has_more() {
        // Today's bare array: a full page means "maybe more".
        let page = parse_threads(&json!([thread("a1"), thread("a2")]), None, 0, 2);
        assert!(page.has_more);
        assert_eq!(page.items[0], ChatThreadSummary {
            id: "a1".into(), title: "T a1".into(), preview: "Hi there".into(),
            updated_at: "2026-09-11T08:10:00Z".into(), model: Some("m".into()),
        });
        assert!(!parse_threads(&json!([thread("a1")]), None, 0, 30).has_more);
        // The header wins over any guess.
        assert!(!parse_threads(&json!([thread("a1"), thread("a2")]), Some(false), 0, 2).has_more);
        assert!(parse_threads(&json!([thread("a1")]), Some(true), 0, 30).has_more);
        // Envelopes: DRF `next`, or `count`.
        let drf = json!({ "results": [thread("b1")], "next": "https://x/?offset=30", "count": 99 });
        assert!(parse_threads(&drf, None, 0, 30).has_more);
        assert!(!parse_threads(&json!({ "results": [thread("b1")], "next": null }), None, 0, 30).has_more);
        assert!(parse_threads(&json!({ "items": [thread("c1")], "count": 40 }), None, 30, 9).has_more);
        assert!(!parse_threads(&json!({ "items": [thread("c1")], "count": 31 }), None, 30, 9).has_more);
        assert!(parse_threads(&json!("odd"), None, 0, 30).items.is_empty());
    }

    #[test]
    fn thread_list_filters_and_falls_back() {
        let page = parse_threads(&json!([
            thread("ok"),
            { "id": 42, "title": "", "created_at": "2026-01-01T00:00:00Z" },
            { "thread_id": "wave-123", "title": "wave" },
            { "thread_id": "m1", "source": "marketing" },
            { "thread_id": "../x" },
            { "title": "no id" }
        ]), None, 0, 30);
        let ids: Vec<&str> = page.items.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["ok", "42"]);
        assert_eq!(page.items[1].title, "", "untitled: the UI says so in its language");
        assert_eq!(page.items[1].updated_at, "2026-01-01T00:00:00Z", "created_at stands in");
        assert_eq!(page.items[1].model, None);
        let many: Vec<Value> = (0..50).map(|i| thread(&format!("t{i}"))).collect();
        assert_eq!(parse_threads(&Value::Array(many), None, 0, 30).items.len(), MAX_PAGE as usize);
    }

    #[test]
    fn previews_are_one_plain_line() {
        assert_eq!(sanitize_preview("  # Title\n- item one\n\n`code`  "), "Title item one code");
        assert_eq!(sanitize_preview(r#"{"reply": "سلام دنیا", "x": 1}"#), "سلام دنیا");
        assert_eq!(sanitize_preview(r#"[{"data": {"text": "deep"}}]"#), "deep");
        assert_eq!(sanitize_preview(r#"{"reply": "cut off"#), "", "unparseable JSON shows nothing rather than braces");
        assert_eq!(sanitize_preview("[link](https://x) after"), "[link](https://x) after");
        assert_eq!(sanitize_preview(&"a".repeat(500)).chars().count(), MAX_PREVIEW_CHARS);
        assert_eq!(sanitize_preview("   "), "");
    }

    #[test]
    fn messages_keep_active_revisions_in_order() {
        let data = json!({
            "thread_id": "t", "title": "Research", "model": "openrouter-openai-gpt-5",
            "messages": [
                { "id": 3, "role": "assistant", "content": "Second answer", "is_active": true, "created_at": "2026-09-11T08:36:00Z" },
                { "id": 1, "role": "user", "content": "Question", "created_at": "2026-09-11T08:35:00Z" },
                { "id": 2, "role": "assistant", "content": "Old answer", "is_active": false, "created_at": "2026-09-11T08:35:30Z" },
                { "id": 4, "role": "system", "content": "hidden", "created_at": "2026-09-11T08:37:00Z" },
                { "id": 5, "role": "assistant", "content": "", "created_at": "2026-09-11T08:38:00Z" },
                { "id": 6, "role": "user", "message": "legacy field", "is_active": null, "created_at": "2026-09-11T08:39:00Z" },
                { "role": "user", "content": "no id", "created_at": "2026-09-11T08:40:00Z" }
            ]
        });
        let (title, model, messages) = parse_messages(&data);
        assert_eq!(title, "Research");
        assert_eq!(model.as_deref(), Some("openrouter-openai-gpt-5"));
        let got: Vec<(&str, &str, &str)> = messages.iter().map(|m| (m.id.as_str(), m.role.as_str(), m.text.as_str())).collect();
        assert_eq!(got, [("1", "user", "Question"), ("3", "assistant", "Second answer"), ("6", "user", "legacy field")]);

        // Bounded: only the last MAX_MESSAGES, each cut to MAX_MESSAGE_CHARS.
        let many: Vec<Value> = (0..250)
            .map(|i| json!({ "id": i, "role": "user", "content": "x".repeat(30_000), "created_at": format!("2026-09-11T08:{:02}:{:02}Z", i / 60, i % 60) }))
            .collect();
        let (_, _, messages) = parse_messages(&json!({ "messages": many }));
        assert_eq!(messages.len(), MAX_MESSAGES);
        assert_eq!(messages[0].id, "50");
        assert_eq!(messages[0].text.chars().count(), MAX_MESSAGE_CHARS);
        assert!(parse_messages(&json!({})).2.is_empty());
    }

    #[test]
    fn first_message_extras_are_hidden_in_history() {
        use crate::roadeep::chat::ChatContext;
        let ctx = ChatContext::Window { app_name: "Code".into(), title: "t".into(), url: None };
        let sent = compose_message("چرا؟", Some(&ctx), true, "fa", Some("brief"), None);
        assert_eq!(display_user_text(&sent), "چرا؟");
        assert_eq!(display_user_text("plain"), "plain");
        let dated = format!("[Roadeep clock: now 2026-10-07 14:05+03:30 (Wednesday), Jalali 1405/07/15, source online]\n\n{sent}");
        assert_eq!(display_user_text(&dated), "چرا؟");
        assert_eq!(display_user_text("[Agent instructions]\nunterminated"), "[Agent instructions]\nunterminated");
    }

    #[test]
    fn bidi_controls_are_stripped_from_history() {
        let page = parse_threads(&json!([{ "thread_id": "a", "title": "x\u{202E}y\u{2067}z", "last_message_preview": "p\u{200F}q" }]), None, 0, 30);
        assert_eq!((page.items[0].title.as_str(), page.items[0].preview.as_str()), ("xyz", "pq"));
        let (_, _, messages) = parse_messages(&json!({ "messages": [
            { "id": 1, "role": "assistant", "content": "سلام\u{202B} دنیا\nمی\u{200C}خواهم", "created_at": "t" }
        ]}));
        assert_eq!(messages[0].text, "سلام دنیا\nمی\u{200C}خواهم", "ZWNJ and newlines stay");
    }

    #[test]
    fn thread_ids_are_checked() {
        assert!(check_thread_id("9c05a16a2f70413792b86fe9a54ac514").is_ok());
        for bad in ["", "../x", "a/b", "x?y", "a b"] {
            assert_eq!(check_thread_id(bad).unwrap_err().code, codes::VALIDATION, "{bad}");
        }
    }

    #[test]
    fn tool_steps_fold_into_the_answer_they_led_to() {
        use crate::roadeep::chat::tools::{correction_message, message_block, preamble, result_message, ToolEntry, DECLINED_TEXT};
        let list = preamble(&[ToolEntry { qualified: "github__search".into(), description: "Search.".into(), schema: json!({}) }]);
        let first = message_block(None, Some(&list)).0.unwrap();
        let reminder = message_block(Some(&list.fingerprint), Some(&list)).0.unwrap();
        let call = |tool: &str, args: Value| format!("<roadeep_tool_call>{}</roadeep_tool_call>", json!({ "tool": tool, "arguments": args }));
        let msg = |id: u32, role: &str, content: String| {
            json!({ "id": id, "role": role, "content": content, "created_at": format!("2026-09-11T08:00:{id:02}Z") })
        };
        let data = json!({ "messages": [
            msg(1, "user", format!("{first}find bugs")),
            msg(2, "assistant", format!("Let me look.\n{}", call("github__search", json!({ "q": "bug" })))),
            msg(3, "user", result_message("github__search", true, "3 issues")),
            msg(4, "assistant", call("github__create_issue", json!({ "title": "x", "token": "secret" }))),
            msg(5, "user", result_message("github__create_issue", false, DECLINED_TEXT)),
            msg(6, "assistant", "Found 3 issues.".into()),
            msg(7, "user", format!("{reminder}and more?")),
            msg(8, "assistant", "<roadeep_tool_call>{broken</roadeep_tool_call>".into()),
            msg(9, "user", correction_message("no JSON object in the call")),
            msg(10, "assistant", call("github__search", json!({ "q": "more" }))),
            msg(11, "user", result_message("github__search", false, "rate limited")),
            msg(12, "assistant", call("github__search", json!({ "q": "again" }))),
        ]});
        let thread = parse_thread(&data);
        assert_eq!(thread.tools.as_deref(), Some(list.fingerprint.as_str()), "the next message only needs a reminder");
        let shown: Vec<(&str, &str, &str, usize)> =
            thread.messages.iter().map(|m| (m.id.as_str(), m.role.as_str(), m.text.as_str(), m.tool_steps.len())).collect();
        assert_eq!(shown, [
            ("1", "user", "find bugs", 0),
            ("6", "assistant", "Found 3 issues.", 2),
            ("7", "user", "and more?", 0),
            ("10-steps", "assistant", "", 2),
        ]);
        let steps = &thread.messages[1].tool_steps;
        assert_eq!((steps[0].server.as_str(), steps[0].tool.as_str(), steps[0].state), ("github", "search", "done"));
        assert_eq!(steps[0].result.as_deref(), Some("3 issues"));
        assert_eq!((steps[1].tool.as_str(), steps[1].state, steps[1].result.as_deref()), ("create_issue", "declined", None));
        assert!(steps[1].arguments.contains("\"title\": \"x\"") && !steps[1].arguments.contains("secret"));
        // The broken call and its correction are gone; the turn ended mid-loop.
        let steps = &thread.messages[3].tool_steps;
        assert_eq!((steps[0].state, steps[0].result.as_deref()), ("error", Some("rate limited")));
        assert_eq!((steps[1].state, steps[1].result.as_deref()), ("stopped", None));
        let json = serde_json::to_value(&thread.messages[1]).unwrap();
        assert_eq!(json["toolSteps"][0]["state"], "done");
        assert!(serde_json::to_value(&thread.messages[0]).unwrap().get("toolSteps").is_none(), "only when there are steps");

        // Tools turned off later: the next message lists them again.
        let off = message_block(Some(&list.fingerprint), None).0.unwrap();
        let data = json!({ "messages": [msg(1, "user", format!("{first}a")), msg(2, "user", format!("{off}b"))] });
        let thread = parse_thread(&data);
        assert_eq!(thread.tools, None);
        assert_eq!(thread.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["a", "b"]);

        // A broken call the model was not corrected for stays as its text.
        let data = json!({ "messages": [msg(1, "user", "q".into()), msg(2, "assistant", "<roadeep_tool_call>{oops".into())] });
        assert_eq!(parse_thread(&data).messages[1].text, "<roadeep_tool_call>{oops");
    }

    #[test]
    fn tool_messages_do_not_leak_into_previews() {
        use crate::roadeep::chat::tools::{message_block, preamble, result_message, ToolEntry};
        let list = preamble(&[ToolEntry { qualified: "a__b".into(), description: "d".into(), schema: json!({}) }]);
        let reminder = message_block(Some(&list.fingerprint), Some(&list)).0.unwrap();
        assert_eq!(sanitize_preview(&format!("{reminder}hello")), "hello");
        assert_eq!(sanitize_preview(&result_message("a__b", true, "data")), "");
        assert_eq!(sanitize_preview("<roadeep_tool_call>{\"tool\":\"a__b\"}"), "");
        // A preamble cut short by the server shows nothing rather than the tool list.
        assert_eq!(sanitize_preview(&list.text[..40]), "");
        assert_eq!(sanitize_preview("[Roadeep tools note] The tool step limit"), "");
    }
}
