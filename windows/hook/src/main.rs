//! roadeep-hook — the relay Claude Code runs on every hook event.
//!
//! Reads the hook JSON on stdin, adds a little terminal context, and hands it to
//! Roadeep over the named pipe `\\.\pipe\roadeep-<sid>`.
//!
//! Hard rule (docs/CLAUDE.md): **never block Claude Code.**
//! * If the pipe does not exist — Roadeep is closed — we exit 0 immediately with
//!   nothing on stdout (or only the "no decision" reply an agent needs, see
//!   `no_decision_reply`), and the session carries on untouched.
//! * Every step runs under a deadline enforced by the main thread, so a pipe that
//!   accepts the connection and then stops reading cannot wedge the session
//!   either: we abandon the worker and exit.
//! * Only `PermissionRequest` waits for an answer, because approving from the
//!   island is the whole point. No answer means empty stdout, and Claude Code
//!   asks in the terminal exactly as if Roadeep were not installed.
//!
//! Usage: `roadeep-hook <EventName>` (the name is also read from the JSON), or
//! `roadeep-hook --provider <agent> <event>` for the other coding agents.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

/// Budget for getting a pipe connection. Beyond this Claude Code wins, always.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Whole-run budget for an event nobody waits on: connect and write, no more.
const FIRE_AND_FORGET_BUDGET: Duration = Duration::from_secs(2);
/// How long a permission prompt may stay on screen before the terminal takes over.
const DECISION_BUDGET: Duration = Duration::from_secs(110);

/// `ERROR_PIPE_BUSY` — every instance is serving someone else right now. This is
/// the one error worth retrying: the server exists and a slot will free up.
const ERROR_PIPE_BUSY: i32 = 231;

/// Longest string forwarded for any single field; the island truncates to far
/// less than this anyway.
const MAX_FIELD_LEN: usize = 2_000;

/// The live diff needs the whole text of a finished edit, so the edit strings of
/// a PostToolUse of Edit / MultiEdit / Write (protocol::keeps_diff) are capped
/// far higher: per string, and for all of one event together.
const MAX_DIFF_FIELD_LEN: usize = 256 * 1024;
const MAX_DIFF_TOTAL: usize = 512 * 1024;
/// Longest line sent: the app reads at most 1 MiB from the pipe. An edit whose
/// JSON escaping would still pass this goes with the ordinary caps instead.
const MAX_LINE: usize = 1_000_000;

mod win;
mod protocol;

/// `\\.\pipe\roadeep-<sid>`. The SID keeps two accounts on the same machine from
/// ever meeting on the same pipe; the name falls back to the user name only if
/// the SID cannot be read at all, which should not happen.
fn pipe_path() -> String {
    let key = win::current_user_sid()
        .unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_else(|_| "user".into()));
    format!(r"\\.\pipe\roadeep-{key}")
}

/// Opens the pipe. Retries only while the server is busy: any other error means
/// there is nothing to talk to, and waiting would only delay Claude Code.
fn connect() -> Option<std::fs::File> {
    use std::os::windows::io::AsRawHandle;
    let path = pipe_path();
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => {
                let handle = windows::Win32::Foundation::HANDLE(file.as_raw_handle());
                // Somebody else's server on our pipe name gets nothing from us.
                return win::pipe_server_is_same_user(handle).then_some(file);
            }
            Err(err) => {
                if err.raw_os_error() != Some(ERROR_PIPE_BUSY) || Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (provider, argument) = if args.first().map(String::as_str) == Some("--provider") {
        (args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default())
    } else { ("claude".into(), args.first().cloned().unwrap_or_default()) };
    let exit_code = if provider == "cursor" && matches!(argument.as_str(), "preToolUse" | "subagentStart") { 1 } else { 0 };
    let Some(Event { line, name, question }) = read_event(&provider, &argument) else {
        // Nothing we could forward. An agent that needs a reply still gets its
        // "no decision" one.
        print(no_decision_reply(&provider, &argument));
        std::process::exit(exit_code)
    };

    let waits_for_answer = protocol::permission(&provider, &name);
    let budget = if waits_for_answer { DECISION_BUDGET } else { FIRE_AND_FORGET_BUDGET };

    // The worker owns every blocking call. If it overruns the budget we simply
    // stop listening and exit: the process dying takes the pipe handle with it.
    // (No catch_unwind here — the release profile is panic = "abort", so it would
    // be dead code. `talk` is written to have nothing to panic on instead.)
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let _ = tx.send(talk(&line, waits_for_answer));
    });

    let decision = rx.recv_timeout(budget).ok().flatten();
    // Only an agent the island answers for takes a decision. Nothing printed for
    // Claude Code: it asks in the terminal, as if we were not here.
    let reply = match decision {
        Some(d) if waits_for_answer => decision_json(&d, question.as_ref()),
        _ => None,
    };
    print(reply.or_else(|| no_decision_reply(&provider, &name).map(str::to_string)));
    std::process::exit(exit_code);
}

fn print(line: Option<impl std::fmt::Display>) {
    if let Some(line) = line {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

/// What an agent reads on stdout when nobody decided anything, for the agents
/// that need something there. Antigravity takes "{}" on PreToolUse as a denial,
/// so it gets "ask": its own prompt, and the user's Always Allow, stay in charge.
/// Roadeep never allows a tool by itself. Everyone else gets silence.
fn no_decision_reply(provider: &str, event: &str) -> Option<&'static str> {
    match provider {
        "antigravity" if event == "PreToolUse" => Some(r#"{"decision":"ask"}"#),
        "antigravity" => Some("{}"),
        _ => None,
    }
}

/// The documented PermissionRequest output. Anything we do not recognise prints
/// nothing at all rather than guessing — silence is the safe answer.
/// See https://code.claude.com/docs/en/hooks
///
/// `question` is the AskUserQuestion input, when that is what is being asked: the
/// island may then answer it (`{"answers":[…]}`, by position), which Claude Code
/// takes as the same input with an `answers` map added. Nothing else of the input
/// can be changed from the island, and no other tool's input can be changed at all.
fn decision_json(decision: &str, question: Option<&Value>) -> Option<String> {
    if decision.trim_start().starts_with('{') {
        let reply = serde_json::from_str::<Value>(decision.trim()).ok()?;
        let picks = reply.get("answers")?.as_array()?;
        let question = question?;
        let answers = answers_for(question, picks)?;
        let mut input = question.as_object()?.clone();
        input.insert("answers".into(), Value::Object(answers));
        return Some(
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow", "updatedInput": input },
                }
            })
            .to_string(),
        );
    }
    let behavior = match decision.trim() {
        // "always" still answers a plain allow; remembering it is the island's
        // business, not Claude Code's.
        "allow" | "always" => r#"{"behavior":"allow"}"#.to_string(),
        "deny" => r#"{"behavior":"deny","message":"Denied from Roadeep"}"#.to_string(),
        _ => return None,
    };
    Some(format!(
        r#"{{"hookSpecificOutput":{{"hookEventName":"PermissionRequest","decision":{behavior}}}}}"#
    ))
}

/// The `answers` map Claude Code takes, from the island's picks: one per question
/// asked, in order — an option index for a single-select question, a non-empty
/// list of distinct option indexes for a multi-select one. Keyed by the question
/// as Claude Code asked it, with its own labels: one label, or a list of them for
/// multi-select (Claude Code 2.1.136+, as the Mac sends it). None unless the
/// picks answer exactly the questions asked.
fn answers_for(question: &Value, picks: &[Value]) -> Option<Map<String, Value>> {
    let asked = protocol::asked_questions(question)?;
    if picks.len() != asked.len() {
        return None;
    }
    let mut answers = Map::new();
    for (q, pick) in asked.iter().zip(picks) {
        let label = |v: &Value| -> Option<Value> {
            let i = usize::try_from(v.as_u64()?).ok()?;
            q.options.get(i).map(|(label, _)| Value::String((*label).to_string()))
        };
        let answer = match pick {
            Value::Number(_) if !q.multi => label(pick)?,
            Value::Array(list) if q.multi => {
                let mut seen: Vec<u64> = list.iter().map(Value::as_u64).collect::<Option<_>>()?;
                seen.sort_unstable();
                seen.dedup();
                if list.is_empty() || seen.len() != list.len() {
                    return None;
                }
                Value::Array(list.iter().map(label).collect::<Option<_>>()?)
            }
            _ => return None,
        };
        answers.insert(q.question.to_string(), answer);
    }
    Some(answers)
}

/// One event, ready to forward.
struct Event {
    /// The payload as one line of JSON.
    line: String,
    /// The agent's own event name.
    name: String,
    /// For Claude Code's AskUserQuestion, the question as it was asked.
    question: Option<Value>,
}

/// Reads stdin and prepares the event to forward.
fn read_event(provider: &str, arg_event: &str) -> Option<Event> {
    let mut raw = Vec::new();
    if std::io::stdin().take((1 << 20) + 1).read_to_end(&mut raw).is_err() || raw.is_empty() || raw.len() > (1 << 20) {
        return None;
    }
    let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string());
    prepare(&raw, provider, arg_event, &|var| std::env::var(var).ok(), cwd)
}

/// The event to forward, from the raw stdin bytes. `env` reads an environment
/// variable and `cwd` is the working directory, so tests stay pure.
fn prepare(
    raw: &[u8],
    provider: &str,
    arg_event: &str,
    env: &dyn Fn(&str) -> Option<String>,
    cwd: Option<String>,
) -> Option<Event> {
    // Some shells hand us a UTF-8 BOM; serde_json would choke on it.
    let raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);

    let input = serde_json::from_slice::<Value>(raw).ok()?;
    let event = if !arg_event.is_empty() { arg_event.to_string() } else {
        input.get("hook_event_name").and_then(|v| v.as_str()).unwrap_or_default().to_string()
    };
    let mut payload = protocol::normalize(&input, provider, &event)?;
    // Kept whole: what goes back to Claude Code must be its own input, not the
    // shortened copy the island is shown.
    let question = (payload.get("tool_input").is_some_and(|t| t.get("questions").is_some()))
        .then(|| input.get("tool_input").filter(|v| v.is_object()).cloned())
        .flatten();
    let keeps_diff = protocol::keeps_diff(
        provider,
        payload["hook_event_name"].as_str().unwrap_or_default(),
        payload["tool_name"].as_str(),
    );
    let map = payload.as_object_mut()?;

    let cwd_missing = map
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(str::is_empty)
        .unwrap_or(true);
    if cwd_missing {
        if let Some(cwd) = cwd {
            map.insert("cwd".into(), Value::String(cwd));
        }
    }

    // Which terminal the session runs in. Unlike macOS, Roadeep on Windows accepts
    // events from every terminal, so this is context only — never a filter.
    for (key, var) in [
        ("term_program", "TERM_PROGRAM"),
        ("wt_session", "WT_SESSION"),
        ("term_session_id", "TERM_SESSION_ID"),
        ("vscode_pid", "VSCODE_PID"),
        ("session_pid", "CLAUDE_CODE_SSE_PORT"),
    ] {
        if !map.contains_key(key) {
            let value = env(var).unwrap_or_default();
            map.insert(key.into(), Value::String(value));
        }
    }

    let whole = keeps_diff.then(|| payload.clone());
    truncate_payload(&mut payload, keeps_diff);
    let mut line = payload.to_string();
    if let Some(mut whole) = whole.filter(|_| line.len() > MAX_LINE) {
        // Escaping took a big edit past what the app reads: send it with the
        // ordinary caps, and say the diff cannot be trusted.
        truncate_payload(&mut whole, false);
        whole[protocol::DIFF_TRUNCATED] = Value::Bool(true);
        line = whole.to_string();
    }
    line.push('\n');
    Some(Event { line, name: event, question })
}

/// Caps the strings of a payload: every field to MAX_FIELD_LEN, except, when
/// `keeps_diff`, the edit strings of `tool_input`, which the live diff needs
/// whole (MAX_DIFF_FIELD_LEN each, MAX_DIFF_TOTAL together). If even those had
/// to be cut, the payload says so (protocol::DIFF_TRUNCATED).
fn truncate_payload(payload: &mut Value, keeps_diff: bool) {
    let input = if keeps_diff {
        payload.as_object_mut().and_then(|map| map.remove("tool_input"))
    } else {
        None
    };

    truncate_strings(payload);

    if let Some(mut input) = input {
        let mut budget = MAX_DIFF_TOTAL;
        let mut cut_any = false;
        cap_diff_strings(&mut input, &mut budget, &mut cut_any);
        if let Some(map) = payload.as_object_mut() {
            map.insert("tool_input".into(), input);
            if cut_any {
                map.insert(protocol::DIFF_TRUNCATED.into(), Value::Bool(true));
            }
        }
    }
}

/// `tool_input` of a finished edit: the edit strings share `budget`, each capped
/// at MAX_DIFF_FIELD_LEN; any other string gets the ordinary cap.
fn cap_diff_strings(value: &mut Value, budget: &mut usize, cut_any: &mut bool) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                match v {
                    Value::String(s) if protocol::DIFF_FIELDS.contains(&key.as_str()) => {
                        if cut(s, MAX_DIFF_FIELD_LEN.min(*budget)) {
                            *cut_any = true;
                        }
                        *budget = budget.saturating_sub(s.len());
                    }
                    _ => cap_diff_strings(v, budget, cut_any),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                cap_diff_strings(item, budget, cut_any);
            }
        }
        Value::String(s) => {
            cut(s, MAX_FIELD_LEN);
        }
        _ => {}
    }
}

/// Caps every string in the payload. A single Write can carry a whole file.
fn truncate_strings(value: &mut Value) {
    match value {
        Value::String(s) => {
            cut(s, MAX_FIELD_LEN);
        }
        Value::Array(items) => items.iter_mut().for_each(truncate_strings),
        Value::Object(map) => map.values_mut().for_each(truncate_strings),
        _ => {}
    }
}

/// Shortens `s` to at most `max` bytes plus an ellipsis; true if it was cut.
fn cut(s: &mut String, max: usize) -> bool {
    if s.len() <= max {
        return false;
    }
    // Cut on a char boundary; a lone byte index can split UTF-8.
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push('…');
    true
}

/// Connect, send, and — for a permission request — wait for the island's word.
fn talk(payload: &str, waits_for_answer: bool) -> Option<String> {
    let mut pipe = connect()?;

    if pipe.write_all(payload.as_bytes()).is_err() {
        return None;
    }
    let _ = pipe.flush();

    if !waits_for_answer {
        return None;
    }

    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&b'\n') || buf.len() > 4096 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let answer = String::from_utf8_lossy(&buf).trim().to_string();
    (!answer.is_empty()).then_some(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_json_matches_the_documented_shape() {
        assert_eq!(
            decision_json("allow", None).unwrap(),
            r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#
        );
        assert_eq!(
            decision_json("deny", None).unwrap(),
            r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":"Denied from Roadeep"}}}"#
        );
        // "always" is an island concept; Claude Code just gets an allow.
        assert!(decision_json("always", None).unwrap().contains(r#""behavior":"allow""#));
    }

    #[test]
    fn anything_unrecognised_prints_nothing() {
        assert!(decision_json("", None).is_none());
        assert!(decision_json("maybe", None).is_none());
        // The shape the app used to send must not be mistaken for a decision.
        assert!(decision_json(r#"{"permissionDecision":"allow"}"#, None).is_none());
    }

    fn question() -> Value {
        json!({ "questions": [
            { "question": "Which one?", "header": "Pick", "options": [{ "label": "A" }, { "label": "B" }] },
            { "question": "Extras?", "multiSelect": true,
              "options": [{ "label": "Tests" }, { "label": "Docs" }, { "label": "Lint" }] }
        ], "metadata": { "source": "x" } })
    }

    #[test]
    fn an_answered_question_goes_back_as_the_same_input_plus_answers() {
        let q = question();
        let out = decision_json(r#"{"answers":[1,[2,0]]}"#, Some(&q)).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let decision = &v["hookSpecificOutput"]["decision"];
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
        assert_eq!(decision["behavior"], "allow");
        assert_eq!(decision["updatedInput"]["questions"], q["questions"]);
        assert_eq!(decision["updatedInput"]["metadata"], q["metadata"]);
        assert_eq!(
            decision["updatedInput"]["answers"],
            json!({ "Which one?": "B", "Extras?": ["Lint", "Tests"] })
        );
    }

    #[test]
    fn answers_must_answer_exactly_the_questions_asked() {
        let q = question();
        let ok = |a: &str| decision_json(a, Some(&q)).is_some();
        assert!(ok(r#"{"answers":[0,[1]]}"#));
        // Out of range, a question left out or one too many.
        assert!(!ok(r#"{"answers":[2,[1]]}"#));
        assert!(!ok(r#"{"answers":[0,[3]]}"#));
        assert!(!ok(r#"{"answers":[0]}"#));
        assert!(!ok(r#"{"answers":[0,[1],0]}"#));
        // Shapes: single-select is one index, multi-select a non-empty list of distinct ones.
        assert!(!ok(r#"{"answers":[[0],[1]]}"#));
        assert!(!ok(r#"{"answers":[0,1]}"#));
        assert!(!ok(r#"{"answers":[0,[]]}"#));
        assert!(!ok(r#"{"answers":[0,[1,1]]}"#));
        assert!(!ok(r#"{"answers":[-1,[1]]}"#));
        assert!(!ok(r#"{"answers":[0.5,[1]]}"#));
        assert!(!ok(r#"{"answers":["A",[1]]}"#));
        // The text-keyed shape, or anything else, is no answer.
        assert!(!ok(r#"{"answers":{"Which one?":"A","Extras?":["Tests"]}}"#));
        assert!(!ok(r#"{"command":"rm -rf /"}"#));
        // No question asked: the island cannot rewrite any other tool's input.
        assert!(decision_json(r#"{"answers":[0,[1]]}"#, None).is_none());
        // A question set the island was never offered is never answered.
        let odd = json!({ "questions": [{ "question": "Q?", "options": [{ "label": "A" }] }] });
        assert!(decision_json(r#"{"answers":[0]}"#, Some(&odd)).is_none());
    }

    #[test]
    fn antigravity_is_never_allowed_by_us() {
        assert_eq!(no_decision_reply("antigravity", "PreToolUse"), Some(r#"{"decision":"ask"}"#));
        for event in ["PostToolUse", "PreInvocation", "PostInvocation", "Stop", ""] {
            assert_eq!(no_decision_reply("antigravity", event), Some("{}"));
        }
        for provider in protocol::PROVIDERS.iter().filter(|p| **p != "antigravity") {
            for event in ["PreToolUse", "PermissionRequest", "Stop"] {
                assert_eq!(no_decision_reply(provider, event), None, "{provider} {event}");
            }
        }
        // Antigravity never waits for the island, so no decision can reach it.
        assert!(protocol::events("antigravity").iter().all(|e| !protocol::permission("antigravity", e)));
    }

    fn run(raw: &str, provider: &str, event: &str) -> (Value, Event) {
        let ev = prepare(raw.as_bytes(), provider, event, &|_| None, Some("C:\\here".into())).expect("forwarded");
        let v = serde_json::from_str(ev.line.trim_end()).unwrap();
        (v, ev)
    }

    #[test]
    fn a_question_is_kept_whole_and_only_for_ask_user_question() {
        let long = "x".repeat(3000);
        let raw = format!(
            r#"{{"hook_event_name":"PermissionRequest","tool_name":"AskUserQuestion","tool_input":{{"questions":[{{"question":"{long}","options":[{{"label":"A"}},{{"label":"B"}}]}}]}}}}"#
        );
        let (v, ev) = run(&raw, "claude", "PermissionRequest");
        assert_eq!(ev.question.unwrap()["questions"][0]["question"].as_str().unwrap().len(), 3000);
        assert!(v["tool_input"]["questions"][0]["question"].as_str().unwrap().ends_with('…'));
        assert_eq!(v["cwd"], "C:\\here");
        let (_, ev) = run(r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#, "claude", "PermissionRequest");
        assert!(ev.question.is_none());
        // Nothing the island could offer: no question kept, no answer possible.
        let (v, ev) = run(r#"{"tool_name":"AskUserQuestion","tool_input":{"questions":[]}}"#, "claude", "PermissionRequest");
        assert!(ev.question.is_none() && v["tool_input"].get("questions").is_none());
    }

    #[test]
    fn what_cannot_be_read_is_not_forwarded() {
        for raw in ["", "not json", "[1,2]"] {
            assert!(prepare(raw.as_bytes(), "claude", "Stop", &|_| None, None).is_none());
        }
        assert!(prepare(br#"{"x":1}"#, "claude", "NotAnEvent", &|_| None, None).is_none());
        // A BOM is not a reason to drop the event.
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(br#"{"hook_event_name":"Stop"}"#);
        assert!(prepare(&bom, "claude", "", &|_| None, None).is_some());
    }

    #[test]
    fn a_finished_edit_keeps_its_text_whole_for_the_live_diff() {
        let big = "line\\n".repeat(4_000);
        let raw = format!(
            r#"{{"tool_name":"Edit","cwd":"{cwd}","tool_input":{{"file_path":"a.ts","old_string":"{big}","new_string":"{big}"}}}}"#,
            cwd = "c".repeat(4_000)
        );
        let (v, _) = run(&raw, "claude", "PostToolUse");
        assert_eq!(v["tool_input"]["old_string"].as_str().unwrap().len(), 20_000);
        assert_eq!(v["tool_input"]["new_string"].as_str().unwrap().len(), 20_000);
        // Everything else keeps the ordinary cap, and nothing says "cut".
        assert!(v["cwd"].as_str().unwrap().len() <= MAX_FIELD_LEN + 4);
        assert!(v.get(protocol::DIFF_TRUNCATED).is_none());
        // The same edit before it happens, or from another agent: no text, or capped.
        let (v, _) = run(&raw, "claude", "PreToolUse");
        assert!(v["tool_input"].get("old_string").is_none());
        let multi = format!(r#"{{"tool_name":"MultiEdit","tool_input":{{"edits":[{{"old_string":"{big}","new_string":"x"}}]}}}}"#);
        let (v, _) = run(&multi, "claude", "PostToolUse");
        assert_eq!(v["tool_input"]["edits"][0]["old_string"].as_str().unwrap().len(), 20_000);
    }

    #[test]
    fn an_edit_beyond_the_budget_is_cut_and_flagged() {
        let mut v = json!({ "tool_name": "Write", "tool_input": { "content": "x".repeat(MAX_DIFF_FIELD_LEN + 10) } });
        truncate_payload(&mut v, true);
        assert!(v["tool_input"]["content"].as_str().unwrap().len() <= MAX_DIFF_FIELD_LEN + 4);
        assert_eq!(v[protocol::DIFF_TRUNCATED], true);

        // Together, the edit strings never pass the shared budget.
        let half = "y".repeat(MAX_DIFF_FIELD_LEN - 1);
        let edits: Vec<Value> = (0..4).map(|_| json!({ "old_string": half, "new_string": half })).collect();
        let mut multi = json!({ "tool_name": "MultiEdit", "tool_input": { "edits": edits } });
        truncate_payload(&mut multi, true);
        assert!(multi.to_string().len() < MAX_DIFF_TOTAL + 64 * 1024);
        assert_eq!(multi[protocol::DIFF_TRUNCATED], true);

        // Without the diff rule every string gets the ordinary cap.
        let mut plain = json!({ "tool_input": { "content": "é".repeat(4000) } });
        truncate_payload(&mut plain, false);
        assert!(plain["tool_input"]["content"].as_str().unwrap().len() <= MAX_FIELD_LEN + 4);
        assert!(plain.get(protocol::DIFF_TRUNCATED).is_none());
    }

    #[test]
    fn an_edit_that_escaping_would_take_past_the_pipe_limit_goes_capped_and_flagged() {
        // Control characters escape to six bytes each.
        let raw = json!({ "tool_name": "Write", "tool_input": { "file_path": "a.bin", "content": "\u{1}".repeat(200_000) } });
        let (v, _) = run(&raw.to_string(), "claude", "PostToolUse");
        assert!(v["tool_input"]["content"].as_str().unwrap().len() <= MAX_FIELD_LEN + 4);
        assert_eq!(v[protocol::DIFF_TRUNCATED], true);
        assert_eq!(v["tool_input"]["file_path"], "a.bin");
    }

    #[test]
    fn long_strings_are_cut_on_a_char_boundary() {
        let mut v = serde_json::json!({ "tool_input": { "content": "é".repeat(4000) } });
        truncate_strings(&mut v);
        let s = v["tool_input"]["content"].as_str().unwrap();
        assert!(s.len() <= MAX_FIELD_LEN + 4);
        assert!(s.ends_with('…'));
    }
}
