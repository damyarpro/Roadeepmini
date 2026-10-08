//! Shared provider contracts for the relay and native receiver. Unknown events fail closed.
use serde_json::{json, Value};

pub const PROVIDERS: &[&str] = &[
    "claude", "codex", "gemini", "cursor", "windsurf", "copilot", "vscode", "kiro", "opencode",
    "antigravity",
];

/// The tool through which Claude Code asks the user questions.
pub const QUESTION_TOOL: &str = "AskUserQuestion";
/// Bounds of a question set the island offers to answer (Claude Code asks 1–4
/// questions of 2–4 options). Beyond them the terminal answers.
pub const MAX_QUESTIONS: usize = 8;
pub const MAX_OPTIONS: usize = 16;

/// Claude Code's file-editing tools: their PostToolUse keeps the edit text, which
/// the island's live diff needs.
pub const DIFF_TOOLS: &[&str] = &["Edit", "MultiEdit", "Write"];
/// The `tool_input` keys holding the text being replaced or written.
pub const DIFF_FIELDS: &[&str] = &["old_string", "new_string", "content"];
/// Set on a payload whose edit text was cut or masked: its counts cannot be trusted.
pub const DIFF_TRUNCATED: &str = "roadeep_diff_truncated";
/// Most MultiEdit edits forwarded; any beyond are dropped and the diff flagged.
const MAX_EDITS: usize = 512;
pub fn events(provider: &str) -> &'static [&'static str] {
    match provider {
        "claude" => &[
            "SessionStart",
            "SessionEnd",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "PermissionRequest",
            "Notification",
            "Stop",
            "StopFailure",
            "SubagentStart",
            "SubagentStop",
        ],
        "codex" => &[
            "SessionStart",
            "SessionEnd",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Stop",
            "Interrupt",
            "SubagentStart",
            "SubagentStop",
            "PreCompact",
            "PostCompact",
        ],
        "gemini" => &[
            "SessionStart",
            "SessionEnd",
            "BeforeAgent",
            "AfterAgent",
            "BeforeTool",
            "AfterTool",
            "Notification",
            "PreCompress",
        ],
        "cursor" => &[
            "sessionStart",
            "sessionEnd",
            "beforeSubmitPrompt",
            "preToolUse",
            "postToolUse",
            "postToolUseFailure",
            "stop",
            "subagentStart",
            "subagentStop",
            "preCompact",
        ],
        "windsurf" => &[
            "pre_user_prompt",
            "pre_read_code",
            "post_read_code",
            "pre_write_code",
            "post_write_code",
            "pre_run_command",
            "post_run_command",
            "pre_mcp_tool_use",
            "post_mcp_tool_use",
            "post_cascade_response",
        ],
        "copilot" => &[
            "sessionStart",
            "sessionEnd",
            "userPromptSubmitted",
            "preToolUse",
            "postToolUse",
            "errorOccurred",
            "subagentStart",
            "subagentStop",
            "preCompact",
        ],
        "opencode" => &[
            "session.created",
            "session.deleted",
            "session.idle",
            "session.error",
            "session.prompt",
            "tool.execute.before",
            "tool.execute.after",
            "tool.execute.failed",
            "permission.asked",
        ],
        "kiro" => &[
            "SessionStart",
            "PreToolUse",
            "PostToolUse",
            "Stop",
            "PostFileSave",
        ],
        "vscode" => &[
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Stop",
            "SubagentStart",
            "SubagentStop",
            "PreCompact",
        ],
        // ~/.gemini/config/hooks.json: tool events and lifecycle events.
        "antigravity" => &[
            "PreToolUse",
            "PostToolUse",
            "PreInvocation",
            "PostInvocation",
            "Stop",
        ],
        _ => &[],
    }
}
pub fn normalize_event(provider: &str, event: &str) -> Option<&'static str> {
    if !events(provider).contains(&event) {
        return None;
    }
    Some(match event {
        "session.created" | "sessionStart" | "SessionStart" => "SessionStart",
        "session.deleted" | "sessionEnd" | "SessionEnd" => "SessionEnd",
        "session.prompt" | "beforeSubmitPrompt"
        | "userPromptSubmitted"
        | "BeforeAgent"
        | "pre_user_prompt"
        | "PreInvocation"
        | "UserPromptSubmit" => "UserPromptSubmit",
        "tool.execute.before"
        | "preToolUse"
        | "BeforeTool"
        | "pre_read_code"
        | "pre_write_code"
        | "pre_run_command"
        | "pre_mcp_tool_use"
        | "PreToolUse" => "PreToolUse",
        "tool.execute.after" | "PostFileSave" | "postToolUse" | "AfterTool" | "post_read_code"
        | "post_write_code" | "post_run_command" | "post_mcp_tool_use" | "PostInvocation"
        | "PostToolUse" => "PostToolUse",
        "tool.execute.failed" | "postToolUseFailure" | "PostToolUseFailure" => "PostToolUseFailure",
        "session.error" | "errorOccurred" | "StopFailure" => "StopFailure",
        "session.idle" | "stop" | "AfterAgent" | "post_cascade_response" | "Stop" => "Stop",
        "subagentStart" | "SubagentStart" => "SubagentStart",
        "subagentStop" | "SubagentStop" => "SubagentStop",
        "preCompact" | "PreCompress" | "PreCompact" => "PreCompact",
        "PostCompact" => "PostCompact",
        "PermissionRequest" => "PermissionRequest",
        "permission.asked" | "Notification" => "Notification",
        "Interrupt" => "Interrupt",
        _ => return None,
    })
}
pub fn permission(provider: &str, event: &str) -> bool {
    matches!(provider, "claude" | "codex") && event == "PermissionRequest"
}
fn field<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| input.get(*key))
        .filter(|v| !v.is_null())
}
/// Whether an event keeps its edit text for the live diff: a finished Edit,
/// MultiEdit or Write from Claude Code. `event` is the normalized name.
pub fn keeps_diff(provider: &str, event: &str, tool: Option<&str>) -> bool {
    provider == "claude" && event == "PostToolUse" && tool.is_some_and(|t| DIFF_TOOLS.contains(&t))
}

/// One question of an AskUserQuestion call: its text, chip label, whether
/// several options may be picked, and its options as (label, description).
pub struct Asked<'a> {
    pub question: &'a str,
    pub header: &'a str,
    pub multi: bool,
    pub options: Vec<(&'a str, &'a str)>,
}

/// The questions of an AskUserQuestion input, when the island can offer every
/// one of them: 1..=MAX_QUESTIONS questions with distinct non-empty texts, each
/// with 2..=MAX_OPTIONS options with distinct non-empty labels. Anything else is
/// None: the request then gets the ordinary card and no answer is taken for it.
pub fn asked_questions(input: &Value) -> Option<Vec<Asked<'_>>> {
    let items = input.get("questions")?.as_array()?;
    if items.is_empty() || items.len() > MAX_QUESTIONS {
        return None;
    }
    let mut out: Vec<Asked> = Vec::with_capacity(items.len());
    for item in items {
        let question = item.get("question")?.as_str().filter(|s| !s.trim().is_empty())?;
        if out.iter().any(|a| a.question == question) {
            return None;
        }
        let multi = match item.get("multiSelect") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return None,
        };
        let raw = item.get("options")?.as_array()?;
        if raw.len() < 2 || raw.len() > MAX_OPTIONS {
            return None;
        }
        let mut options: Vec<(&str, &str)> = Vec::with_capacity(raw.len());
        for option in raw {
            let label = option.get("label")?.as_str().filter(|s| !s.trim().is_empty())?;
            if options.iter().any(|(l, _)| *l == label) {
                return None;
            }
            options.push((label, option.get("description").and_then(Value::as_str).unwrap_or("")));
        }
        let header = item.get("header").and_then(Value::as_str).unwrap_or("");
        out.push(Asked { question, header, multi, options });
    }
    Some(out)
}

/// What the island is shown of the questions: same order, same fields.
fn questions_json(asked: &[Asked]) -> Value {
    Value::Array(
        asked
            .iter()
            .map(|a| {
                let options: Vec<Value> = a
                    .options
                    .iter()
                    .map(|(label, description)| json!({ "label": label, "description": description }))
                    .collect();
                json!({ "question": a.question, "header": a.header, "multiSelect": a.multi, "options": options })
            })
            .collect(),
    )
}

/// Only selected fields cross the pipe. Outputs, patches, transcripts and file
/// content are omitted, except two things the island acts on: the questions of
/// Claude Code's AskUserQuestion, and the edit text of a finished Claude Code
/// Edit / MultiEdit / Write (the live diff).
pub fn normalize(input: &Value, provider: &str, event: &str) -> Option<Value> {
    input.as_object()?;
    let normalized = normalize_event(provider, event)?;
    let mut out = json!({"provider":provider,"hook_event_name":normalized,"original_event":event});
    for (target, aliases) in [
        (
            "session_id",
            &[
                "session_id",
                "sessionId",
                "conversation_id",
                "conversationId",
                "trajectory_id",
            ] as &[&str],
        ),
        ("cwd", &["cwd"]),
        ("tool_name", &["tool_name", "toolName"]),
        (
            "tool_use_id",
            &["tool_use_id", "toolUseId", "tool_call_id", "call_id"],
        ),
        ("turn_id", &["turn_id", "turnId"]),
        ("agent_id", &["agent_id", "agentId"]),
        ("agent_type", &["agent_type", "agentType"]),
        ("notification_type", &["notification_type"]),
        ("prompt", &["prompt", "initialPrompt"]),
        ("message", &["message"]),
        // Claude Code's Stop: the turn's final line, shown on the island.
        ("last_assistant_message", &["last_assistant_message"]),
        ("reason", &["reason"]),
        ("term_program", &["term_program"]),
        ("wt_session", &["wt_session"]),
        ("term_session_id", &["term_session_id"]),
        ("vscode_pid", &["vscode_pid"]),
        ("session_pid", &["session_pid"]),
        ("source", &["source"]),
    ] {
        if let Some(v) = field(input, aliases).filter(|v| v.is_string()) {
            out[target] = v.clone();
        }
    }
    // Antigravity (and Gemini-style payloads): the tool is `toolCall.name` /
    // `toolCall.args`, the folder the first of `workspacePaths`.
    let tool_call = input.get("toolCall").filter(|v| v.is_object());
    if out.get("tool_name").is_none() {
        if let Some(v) = tool_call.and_then(|c| c.get("name")).filter(|v| v.is_string()) {
            out["tool_name"] = v.clone();
        }
    }
    if out.get("cwd").is_none() {
        if let Some(v) = input
            .get("workspacePaths")
            .and_then(|w| w.get(0))
            .filter(|v| v.is_string())
        {
            out["cwd"] = v.clone();
        }
    }
    let tool = field(input, &["tool_input", "toolInput", "toolArgs", "tool_info"])
        .or_else(|| tool_call.and_then(|c| c.get("args")).filter(|v| v.is_object()));
    if let Some(tool) = tool {
        let parsed = tool
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok());
        let tool = parsed.as_ref().unwrap_or(tool);
        let mut selected = json!({});
        for (key, aliases) in [
            ("command", &["command", "command_line", "CommandLine"] as &[&str]),
            ("file_path", &["file_path", "path", "FilePath", "Path"]),
            ("description", &["description"]),
            ("url", &["url", "Url"]),
            ("query", &["query", "Query"]),
            ("pattern", &["pattern", "Pattern"]),
            ("prompt", &["prompt"]),
            ("recipient", &["recipient"]),
            ("subject", &["subject"]),
        ] {
            if let Some(v) = field(tool, aliases).filter(|v| v.is_string()) {
                selected[key] = v.clone();
            }
        }
        if out.get("cwd").is_none() {
            if let Some(v) = tool.get("cwd").filter(|v| v.is_string()) {
                out["cwd"] = v.clone();
            }
        }
        if out.get("prompt").is_none() {
            if let Some(v) = tool.get("user_prompt").filter(|v| v.is_string()) {
                out["prompt"] = v.clone();
            }
        }
        let tool_name = out.get("tool_name").and_then(Value::as_str);
        // Claude Code asking a question: the island shows it and answers by position.
        if provider == "claude" && normalized == "PermissionRequest" && tool_name == Some(QUESTION_TOOL) {
            if let Some(asked) = asked_questions(tool) {
                selected["questions"] = questions_json(&asked);
            }
        }
        if keeps_diff(provider, normalized, tool_name) {
            for key in DIFF_FIELDS {
                if let Some(v) = tool.get(*key).filter(|v| v.is_string()) {
                    selected[*key] = v.clone();
                }
            }
            if let Some(edits) = tool.get("edits").and_then(Value::as_array) {
                let kept: Vec<Value> = edits
                    .iter()
                    .take(MAX_EDITS)
                    .filter_map(|e| {
                        let old = e.get("old_string")?.as_str()?;
                        let new = e.get("new_string")?.as_str()?;
                        Some(json!({ "old_string": old, "new_string": new }))
                    })
                    .collect();
                if edits.len() > MAX_EDITS {
                    out[DIFF_TRUNCATED] = json!(true);
                }
                selected["edits"] = Value::Array(kept);
            }
            // The relay's flag survives the app's own pass over the payload.
            if input.get(DIFF_TRUNCATED) == Some(&Value::Bool(true)) {
                out[DIFF_TRUNCATED] = json!(true);
            }
        }
        out["tool_input"] = selected;
    }
    if provider == "kiro" && event == "PostFileSave" {
        out["tool_name"] = json!("Write");
    }
    if provider == "windsurf" && out.get("tool_name").is_none() {
        out["tool_name"] = json!(event);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn events_only_normalize_documented_contracts() {
        for provider in PROVIDERS {
            for event in events(provider) {
                assert!(normalize_event(provider, event).is_some());
            }
        }
        assert!(normalize_event("codex", "StopFailure").is_none());
        assert!(normalize_event("cursor", "PermissionRequest").is_none());
        assert!(!permission("vscode", "PermissionRequest"));
    }
    #[test]
    fn identities_survive_and_private_outputs_are_omitted() {
        let v=normalize(&json!({"sessionId":"s","toolName":"Shell","toolArgs":"{\"command\":\"echo ok\",\"content\":\"secret\"}","output":"secret","transcript_path":"private"}),"copilot","preToolUse").unwrap();
        assert_eq!(v["session_id"], "s");
        assert_eq!(v["tool_input"]["command"], "echo ok");
        assert!(v.get("output").is_none());
        assert!(v["tool_input"].get("content").is_none());
        let stop = normalize(&json!({"session_id":"s","last_assistant_message":"Done.","transcript_path":"private"}),"claude","Stop").unwrap();
        assert_eq!(stop["last_assistant_message"], "Done.");
        assert!(stop.get("transcript_path").is_none());
    }

    #[test]
    fn antigravity_tool_calls_become_tool_name_and_input() {
        let v = normalize(
            &json!({
                "conversationId": "conv-1",
                "workspacePaths": ["C:\\Projects\\test"],
                "toolCall": { "name": "run_command", "args": { "CommandLine": "cargo check", "Cwd": "x" } }
            }),
            "antigravity",
            "PreToolUse",
        )
        .unwrap();
        assert_eq!(v["hook_event_name"], "PreToolUse");
        assert_eq!(v["tool_name"], "run_command");
        assert_eq!(v["tool_input"], json!({ "command": "cargo check" }));
        assert_eq!(v["session_id"], "conv-1");
        assert_eq!(v["cwd"], "C:\\Projects\\test");
        let file = normalize(&json!({ "toolCall": { "name": "view_file", "args": { "FilePath": "a.rs" } } }), "antigravity", "PostToolUse").unwrap();
        assert_eq!(file["tool_input"]["file_path"], "a.rs");
        assert_eq!(normalize_event("antigravity", "PreInvocation"), Some("UserPromptSubmit"));
        assert_eq!(normalize_event("antigravity", "PostInvocation"), Some("PostToolUse"));
        assert!(normalize_event("antigravity", "PermissionRequest").is_none());
        assert!(!permission("antigravity", "PreToolUse"));
    }

    fn ask(questions: Value) -> Value {
        json!({ "session_id": "s", "tool_name": "AskUserQuestion", "tool_input": { "questions": questions } })
    }

    #[test]
    fn a_question_is_forwarded_for_the_island_and_survives_a_second_pass() {
        let raw = ask(json!([
            { "question": "Which one?", "header": "Pick", "options": [{ "label": "A", "description": "first" }, { "label": "B" }], "extra": 1 },
            { "question": "Extras?", "multiSelect": true, "options": [{ "label": "Tests" }, { "label": "Docs" }] }
        ]));
        let v = normalize(&raw, "claude", "PermissionRequest").unwrap();
        let expected = json!([
            { "question": "Which one?", "header": "Pick", "multiSelect": false,
              "options": [{ "label": "A", "description": "first" }, { "label": "B", "description": "" }] },
            { "question": "Extras?", "header": "", "multiSelect": true,
              "options": [{ "label": "Tests", "description": "" }, { "label": "Docs", "description": "" }] }
        ]);
        assert_eq!(v["tool_input"]["questions"], expected);
        // The app normalizes the relay's payload again: nothing changes.
        let again = normalize(&v, "claude", "PermissionRequest").unwrap();
        assert_eq!(again["tool_input"]["questions"], expected);
        // Only Claude Code's permission request carries them.
        assert!(normalize(&raw, "claude", "PreToolUse").unwrap()["tool_input"].get("questions").is_none());
        assert!(normalize(&raw, "codex", "PermissionRequest").unwrap()["tool_input"].get("questions").is_none());
    }

    #[test]
    fn questions_the_island_cannot_offer_whole_are_left_to_the_terminal() {
        let two = json!([{ "label": "A" }, { "label": "B" }]);
        for questions in [
            json!([]),
            json!([{ "question": "", "options": two }]),
            json!([{ "question": "Q?", "options": [{ "label": "A" }] }]),
            json!([{ "question": "Q?", "options": [{ "label": "A" }, { "label": "A" }] }]),
            json!([{ "question": "Q?", "options": [{ "label": "A" }, { "label": 2 }] }]),
            json!([{ "question": "Q?", "options": two }, { "question": "Q?", "options": two }]),
            json!([{ "question": "Q?", "multiSelect": "yes", "options": two }]),
            json!(vec![json!({ "question": "Q?", "options": two }); MAX_QUESTIONS + 1]),
        ] {
            let v = normalize(&ask(questions.clone()), "claude", "PermissionRequest").unwrap();
            assert!(v["tool_input"].get("questions").is_none(), "{questions}");
        }
        let many: Vec<Value> = (0..=MAX_OPTIONS).map(|i| json!({ "label": format!("o{i}") })).collect();
        assert!(asked_questions(&json!({ "questions": [{ "question": "Q?", "options": many }] })).is_none());
    }

    #[test]
    fn a_finished_claude_edit_keeps_its_text_for_the_live_diff() {
        let edit = json!({ "tool_name": "Edit", "tool_input": { "file_path": "a.ts", "old_string": "x", "new_string": "y", "replace_all": true } });
        let v = normalize(&edit, "claude", "PostToolUse").unwrap();
        assert_eq!(v["tool_input"], json!({ "file_path": "a.ts", "old_string": "x", "new_string": "y" }));
        let write = json!({ "tool_name": "Write", "tool_input": { "file_path": "a.ts", "content": "hello" } });
        assert_eq!(normalize(&write, "claude", "PostToolUse").unwrap()["tool_input"]["content"], "hello");
        let multi = json!({ "tool_name": "MultiEdit", "tool_input": { "file_path": "a.ts",
            "edits": [{ "old_string": "a", "new_string": "b" }, { "old_string": 1 }] } });
        let v = normalize(&multi, "claude", "PostToolUse").unwrap();
        assert_eq!(v["tool_input"]["edits"], json!([{ "old_string": "a", "new_string": "b" }]));
        assert!(v.get(DIFF_TRUNCATED).is_none());
        // Before it happens, from another agent, or another tool: no text.
        assert!(normalize(&write, "claude", "PreToolUse").unwrap()["tool_input"].get("content").is_none());
        assert!(normalize(&write, "vscode", "PostToolUse").unwrap()["tool_input"].get("content").is_none());
        let bash = json!({ "tool_name": "Bash", "tool_input": { "command": "ls", "content": "x" } });
        assert!(normalize(&bash, "claude", "PostToolUse").unwrap()["tool_input"].get("content").is_none());
    }

    #[test]
    fn the_cut_flag_survives_and_too_many_edits_set_it() {
        let flagged = json!({ "tool_name": "Write", "tool_input": { "content": "x" }, DIFF_TRUNCATED: true });
        assert_eq!(normalize(&flagged, "claude", "PostToolUse").unwrap()[DIFF_TRUNCATED], true);
        assert!(normalize(&flagged, "claude", "PreToolUse").unwrap().get(DIFF_TRUNCATED).is_none());
        let edits: Vec<Value> = (0..MAX_EDITS + 1).map(|_| json!({ "old_string": "a", "new_string": "b" })).collect();
        let multi = json!({ "tool_name": "MultiEdit", "tool_input": { "edits": edits } });
        let v = normalize(&multi, "claude", "PostToolUse").unwrap();
        assert_eq!(v["tool_input"]["edits"].as_array().unwrap().len(), MAX_EDITS);
        assert_eq!(v[DIFF_TRUNCATED], true);
    }
}
