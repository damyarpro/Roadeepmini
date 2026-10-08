// The MCP tools, executed with the signed-in Roadeep session. Arguments come
// from whatever model is driving the MCP client, so every one is validated
// here, whatever the advertised JSON Schema says.

use std::time::{Duration, Instant};

use roadeep_mcp::tools as names;
use roadeep_mcp::wire::{self, WireError};
use serde_json::{json, Map, Value};

use super::confirm::{self, Answer};
use super::quotes;
use crate::log;
use crate::roadeep::chat::{self, JobStep};
use crate::roadeep::generation::{self, GenerationRequest};
use crate::roadeep::http::{codes, Call, RoadeepError};
use crate::roadeep::{auth, Roadeep};

/// Under the app's per-call deadline (230 s), which is under the relay's (240 s),
/// so the model always gets the real reason rather than a bare timeout.
const CHAT_TIMEOUT: Duration = Duration::from_secs(200);
const POLL_START: Duration = Duration::from_millis(700);
const POLL_FAILURE_TOLERANCE: u32 = 2;

pub const UNKNOWN_TOOL: &str = wire::codes::UNKNOWN_TOOL;

/// Roadeep's error, as the relay carries it. Signed-out states all read the
/// same: the fix is always "open Roadeep and sign in".
pub fn to_wire(err: RoadeepError) -> WireError {
    let signed_out = err.code == codes::NOT_SIGNED_IN || err.code == codes::SESSION_EXPIRED;
    WireError {
        message: if signed_out { wire::SIGN_IN_HINT.to_string() } else { err.message },
        code: if signed_out { wire::codes::NOT_SIGNED_IN.to_string() } else { err.code },
        status: err.status,
        retry_after: err.retry_after,
        request_id: err.request_id,
        details: err.field_errors.map(|f| *f),
    }
}

/// Refuses before any network call: an unknown tool, or nobody signed in.
fn precheck(tool: &str, signed_in: bool) -> Result<(), RoadeepError> {
    if !names::is_known(tool) {
        return Err(RoadeepError::new(UNKNOWN_TOOL, format!("Unknown tool: {tool}")));
    }
    if !signed_in {
        return Err(RoadeepError::new(codes::NOT_SIGNED_IN, wire::SIGN_IN_HINT));
    }
    Ok(())
}

pub async fn dispatch(rd: &Roadeep, tool: &str, args: &Value) -> Result<Value, RoadeepError> {
    precheck(tool, rd.session.has_session())?;
    match tool {
        names::WHOAMI => whoami(rd).await,
        names::LIST_MODELS => list_models(rd).await,
        names::LIST_AGENTS => {
            let agents = chat::agents(rd).await?;
            Ok(json!({ "agents": agents.iter().map(|a| json!({
                "id": a.id,
                "title": a.title,
                "short_description": a.short_description,
                "starter_prompts": a.starter_prompts,
            })).collect::<Vec<_>>() }))
        }
        names::CHAT => run_chat(rd, &ChatArgs::parse(args)?).await,
        names::LIST_SUBTYPES => generation::list_subtypes(rd).await,
        names::GET_SUBTYPE => generation::get_subtype(rd, &required_text(args, "slug")?).await,
        names::ESTIMATE_GENERATION => {
            let req = GenerationRequest::from_args(args)?;
            let quote = generation::estimate(rd, &req).await?;
            quotes::remember(&quote, &req);
            Ok(quote)
        }
        names::START_GENERATION => start_generation(rd, args).await,
        names::GENERATION_STATUS => generation::status(rd, &required_text(args, "id")?).await,
        names::CANCEL_GENERATION => generation::cancel(rd, &required_text(args, "id")?).await,
        _ => Err(RoadeepError::new(UNKNOWN_TOOL, format!("Unknown tool: {tool}"))),
    }
}

fn required_text(args: &Value, key: &str) -> Result<String, RoadeepError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| RoadeepError::validation(key, &format!("`{key}` is required.")))
}

/// The paid step: a quote this app issued for this exact body, then a human
/// click in the app's own dialog, and only then the submit.
async fn start_generation(rd: &Roadeep, args: &Value) -> Result<Value, RoadeepError> {
    let quote_id = required_text(args, "quote_id")?;
    let req = GenerationRequest::from_args(args)?;
    let quote = quotes::for_request(&quote_id, &req)?;
    let declined = |message: &str| Err(RoadeepError::new(confirm::USER_DECLINED, message));
    match confirm::ask(&quote).await {
        Answer::Approved => {}
        Answer::Busy => {
            return Err(RoadeepError::new(
                wire::codes::BUSY,
                "Another generation is waiting for the user's confirmation in the Roadeep app. Try again once it is answered.",
            ))
        }
        Answer::Declined => {
            return declined("The user declined this generation in the Roadeep app. Nothing was charged. Do not retry unless the user asks again.")
        }
        Answer::TimedOut => {
            return declined(&format!(
                "Nobody confirmed this generation in the Roadeep app within {} s, so it was not started. Nothing was charged.",
                confirm::DIALOG_TIMEOUT.as_secs()
            ))
        }
        Answer::Failed => {
            return declined("The Roadeep app could not show its confirmation dialog, so the generation was not started. Nothing was charged.")
        }
    }
    let result = generation::start(rd, &req, &quote_id).await;
    match &result {
        Ok(_) => quotes::forget(&quote_id),
        Err(err) if generation::QUOTE_RETRY_CODES.contains(&err.code.as_str()) => quotes::forget(&quote_id),
        Err(_) => {}
    }
    result
}

async fn whoami(rd: &Roadeep) -> Result<Value, RoadeepError> {
    let info = auth::session(rd).await;
    match info.user.filter(|_| info.signed_in) {
        Some(user) => Ok(json!({
            "signed_in": true,
            "user": { "id": user.id, "name": user.name, "email": user.email, "phone": user.phone }
        })),
        None => Err(RoadeepError::new(codes::NOT_SIGNED_IN, wire::SIGN_IN_HINT)),
    }
}

async fn list_models(rd: &Roadeep) -> Result<Value, RoadeepError> {
    let data = rd.request(Call::get("/v1/chat/models/")).await?;
    let models: Vec<Value> = chat::parse_models(&data)
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "display_name": m.display_name,
                "description": m.description,
                "provider": m.provider,
                "is_default": m.is_default,
                "vision": m.vision,
                "file_input": m.file_input,
            })
        })
        .collect();
    Ok(json!({ "models": models }))
}

// ── roadeep_chat ──────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub struct ChatArgs {
    pub message: String,
    pub thread_id: Option<String>,
    pub model: Option<String>,
    pub agent_id: Option<String>,
    pub web_search: Option<bool>,
}

/// A string or a number (thread ids have been both), as text.
fn id_arg(args: &Value, key: &str) -> Result<Option<String>, RoadeepError> {
    let raw = match args.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => return Ok(None),
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(_) => return Err(RoadeepError::validation(key, &format!("`{key}` must be a string."))),
    };
    if !generation::valid_id(&raw) {
        return Err(RoadeepError::validation(key, &format!("`{key}` is not a valid id.")));
    }
    Ok(Some(raw))
}

impl ChatArgs {
    pub fn parse(args: &Value) -> Result<Self, RoadeepError> {
        let message = match args.get("message") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            _ => return Err(RoadeepError::validation("message", "`message` is required.")),
        };
        if message.chars().count() > names::MAX_MESSAGE_CHARS {
            return Err(RoadeepError::validation("message", "That message is too long."));
        }
        let model = match args.get("model") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.trim().is_empty() => None,
            Some(Value::String(s)) if s.len() <= names::MAX_ID_LEN && !s.chars().any(char::is_control) => {
                Some(s.trim().to_string())
            }
            Some(_) => return Err(RoadeepError::validation("model", "That model id is not valid.")),
        };
        let web_search = match args.get("web_search") {
            None | Some(Value::Null) => None,
            Some(Value::Bool(b)) => Some(*b),
            Some(_) => return Err(RoadeepError::validation("web_search", "`web_search` must be true or false.")),
        };
        Ok(Self {
            message,
            thread_id: id_arg(args, "thread_id")?,
            model,
            agent_id: id_arg(args, "agent_id")?,
            web_search,
        })
    }

    /// `POST /v1/chat/async/`: only set fields; `model` only opens a new thread.
    pub fn body(&self) -> Value {
        let mut body = Map::new();
        body.insert("message".into(), json!(self.message));
        match &self.thread_id {
            Some(thread) => {
                body.insert("thread_id".into(), json!(thread));
            }
            None => {
                if let Some(model) = &self.model {
                    body.insert("model".into(), json!(model));
                }
            }
        }
        if let Some(agent) = &self.agent_id {
            body.insert("agent_id".into(), json!(agent));
        }
        if let Some(web) = self.web_search {
            body.insert("web_search".into(), json!(web));
        }
        body.insert("source".into(), json!("chat"));
        Value::Object(body)
    }
}

fn text_or_number(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn is_transient(err: &RoadeepError) -> bool {
    err.code == codes::NETWORK || err.code == codes::TIMEOUT || err.status.is_some_and(|s| s >= 500)
}

/// One stateless turn: submit the async job, poll it to the end. Unlike the
/// island's chat there is no remembered thread — the caller passes it back.
async fn run_chat(rd: &Roadeep, args: &ChatArgs) -> Result<Value, RoadeepError> {
    let accepted = rd.request(Call::post("/v1/chat/async/", args.body())).await?;
    let job_id = text_or_number(&accepted, "job_id")
        .filter(|id| generation::valid_id(id))
        .ok_or_else(|| RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected response."))?;
    let thread_id = text_or_number(&accepted, "thread_id").or_else(|| args.thread_id.clone());

    let path = format!("/v1/chat/jobs/{job_id}/");
    let started = Instant::now();
    let mut delay = POLL_START;
    let mut failures = 0;
    loop {
        tokio::time::sleep(delay).await;
        if started.elapsed() > CHAT_TIMEOUT {
            // Stop paying for an answer nobody will read; a finished job is fine.
            if let Err(err) = rd.request(Call::delete(path.clone())).await {
                log::line(format!("mcp: chat job cancel after timeout failed ({})", err.code));
            }
            return Err(RoadeepError::new(
                chat::chat_codes::JOB_TIMEOUT,
                "Roadeep is taking too long to answer. Try again in a moment.",
            ));
        }
        match rd.request(Call::get(path.clone())).await {
            Ok(data) => {
                failures = 0;
                match chat::job_step(&data) {
                    JobStep::Pending => {}
                    JobStep::Done(reply) => return Ok(json!({ "reply": reply, "thread_id": thread_id })),
                    JobStep::Failed(err) => return Err(err),
                }
            }
            Err(err) if is_transient(&err) && failures < POLL_FAILURE_TOLERANCE => failures += 1,
            Err(err) => return Err(err),
        }
        delay = chat::next_poll_delay(delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_arguments_are_validated() {
        let ok = ChatArgs::parse(&json!({ "message": "  salam  ", "thread_id": 42, "web_search": true })).unwrap();
        assert_eq!(ok.message, "salam");
        assert_eq!(ok.thread_id.as_deref(), Some("42"));
        assert_eq!(ok.web_search, Some(true));

        for bad in [
            json!({}),
            json!({ "message": "" }),
            json!({ "message": 5 }),
            json!({ "message": "x".repeat(names::MAX_MESSAGE_CHARS + 1) }),
            json!({ "message": "hi", "thread_id": "../../v1/auth" }),
            json!({ "message": "hi", "agent_id": "a b" }),
            json!({ "message": "hi", "agent_id": {} }),
            json!({ "message": "hi", "web_search": "yes" }),
            json!({ "message": "hi", "model": "x\u{0}y" }),
        ] {
            let err = ChatArgs::parse(&bad).unwrap_err();
            assert_eq!(err.code, codes::VALIDATION, "{bad}");
        }
    }

    #[test]
    fn chat_body_sends_only_set_fields_and_model_only_for_new_threads() {
        let new = ChatArgs::parse(&json!({ "message": "hi", "model": "gpt", "agent_id": "a-1" })).unwrap();
        assert_eq!(new.body(), json!({ "message": "hi", "model": "gpt", "agent_id": "a-1", "source": "chat" }));
        let cont = ChatArgs::parse(&json!({ "message": "and?", "model": "gpt", "thread_id": "t1", "web_search": false })).unwrap();
        assert_eq!(cont.body(), json!({ "message": "and?", "thread_id": "t1", "web_search": false, "source": "chat" }));
    }

    #[test]
    fn required_text_arguments() {
        assert_eq!(required_text(&json!({ "id": " g1 " }), "id").unwrap(), "g1");
        assert!(required_text(&json!({ "id": "" }), "id").is_err());
        assert!(required_text(&json!({ "id": 3 }), "id").is_err());
        assert!(required_text(&json!({}), "id").is_err());
    }

    #[test]
    fn signed_out_errors_point_at_the_app() {
        for code in [codes::NOT_SIGNED_IN, codes::SESSION_EXPIRED] {
            let w = to_wire(RoadeepError::new(code, "whatever"));
            assert_eq!(w.code, wire::codes::NOT_SIGNED_IN);
            assert_eq!(w.message, wire::SIGN_IN_HINT);
        }
        let mut throttled = RoadeepError::validation("prompt", "Prompt is required.");
        throttled.status = Some(400);
        let w = to_wire(throttled);
        assert_eq!(w.code, codes::VALIDATION);
        assert_eq!(w.details.unwrap()["prompt"], json!(["Prompt is required."]));
    }

    #[test]
    fn unknown_tools_and_signed_out_sessions_stop_before_the_network() {
        assert_eq!(precheck("rm_rf", true).unwrap_err().code, UNKNOWN_TOOL);
        for tool in names::NAMES {
            assert_eq!(precheck(tool, false).unwrap_err().code, codes::NOT_SIGNED_IN, "{tool}");
            assert!(precheck(tool, true).is_ok());
        }
    }
}
