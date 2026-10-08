use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingEvent {
    pub id: String,
    pub session_id: String,
    pub harness: &'static str,
    pub at: u64,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<RateUsage>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used_tokens: u64,
    pub limit_tokens: u64,
    pub used_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateWindow {
    pub used_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RateUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<RateWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<RateWindow>,
}
fn rate_window(v: &Value) -> Option<RateWindow> {
    let used_percent = v["used_percent"].as_f64().filter(|n| n.is_finite() && (0.0..=100.0).contains(n))?;
    Some(RateWindow {
        used_percent,
        resets_at: v["resets_at"].as_u64().filter(|n| *n <= 253_402_300_799).map(|n| n * 1000),
        window_minutes: v["window_minutes"].as_u64().filter(|n| *n <= 525_600),
    })
}

pub fn clean(text: &str, limit: usize) -> String {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| [
        r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)",
        r"(?i)\b(?:bearer\s+)[A-Za-z0-9._~+/=-]+",
        r"\b(?:sk-|ghp_|github_pat_|xox[baprs]-)[A-Za-z0-9_-]{8,}",
        r#"(?i)\b(?:[A-Z0-9_]*(?:api[_-]?key|token|secret|password|passwd)[A-Z0-9_]*)[\s"']*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,;"']+)"#,
        r"(?i)https?://[^\s/@]+:[^\s/@]+@",
    ].iter().map(|p| Regex::new(p).expect("fixed redaction regex")).collect());
    // Redact before clipping so a secret cannot straddle the display boundary.
    let mut out = text.to_owned();
    for pattern in patterns { out = pattern.replace_all(&out, "[redacted]").into_owned(); }
    let clipped: String = out.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(limit).collect();
    if out.chars().count() > limit { format!("{clipped}\n[truncated]") } else { clipped }
}

#[derive(Default)]
pub struct Parser {
    pub session: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
}

fn string<'a>(v: &'a Value, key: &str) -> Option<&'a str> { v.get(key)?.as_str() }

impl Parser {
    pub fn parse(&mut self, line: &[u8], offset: u64, observed_at: u64) -> Result<Option<CodingEvent>, ()> {
        let value: Value = serde_json::from_slice(line).map_err(|_| ())?;
        let p = &value["payload"];
        let typ = string(&value, "type").ok_or(())?;
        if typ == "session_meta" {
            let id = string(p, "id").or_else(|| string(p, "session_id")).ok_or(())?;
            if id.is_empty() || id.len() > 128 || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) { return Err(()); }
            self.session = Some(format!("codex:{id}"));
            self.cwd = string(p, "cwd").map(|s| clean(s, 512));
        } else if typ == "turn_context" {
            if let Some(cwd) = string(p, "cwd") { self.cwd = Some(clean(cwd, 512)); }
            self.model = string(p, "model").map(|s| clean(s, 160));
            return Ok(None);
        }
        let Some(session) = self.session.clone() else { return Ok(None); };
        let at = string(&value, "timestamp").and_then(timestamp).unwrap_or(observed_at);
        let mut e = CodingEvent { id: format!("{session}:{offset}"), session_id: session, harness: "codex", at, kind: "session", cwd: self.cwd.clone(), title: None, tool: None, call_id: None, phase: None, command: None, output: None, exit_code: None, files: None, patch: None, context: None, usage: None };
        match (typ, string(p, "type").unwrap_or("")) {
            ("session_meta", _) => e.title = Some("Codex session".into()),
            ("event_msg", "token_count") => {
                e.kind = "usage";
                let info = &p["info"];
                let used = info["last_token_usage"]["input_tokens"].as_u64().filter(|n| *n <= 1_000_000_000);
                let limit = info["model_context_window"].as_u64().filter(|n| *n > 0 && *n <= 1_000_000_000);
                if let (Some(used_tokens), Some(limit_tokens)) = (used, limit) {
                    if used_tokens <= limit_tokens { e.context = Some(ContextUsage { used_tokens, limit_tokens, used_percent: used_tokens as f64 / limit_tokens as f64 * 100.0, model: self.model.clone() }); }
                }
                let primary = rate_window(&p["rate_limits"]["primary"]);
                let secondary = rate_window(&p["rate_limits"]["secondary"]);
                if primary.is_some() || secondary.is_some() { e.usage = Some(RateUsage { primary, secondary }); }
                if e.context.is_none() && e.usage.is_none() { return Ok(None); }
            }
            ("event_msg", "user_message" | "task_started" | "turn_started") => { e.kind = "prompt"; e.title = Some("New task".into()); }
            ("event_msg", "task_complete" | "task_completed" | "turn_complete") => { e.kind = if p.get("error").is_some_and(|v| !v.is_null()) { "error" } else { "finished" }; e.title = Some(if e.kind == "error" { "Task reported an error" } else { "Task finished" }.into()); }
            ("event_msg", "turn_aborted") => {
                if !matches!(string(p, "reason"), Some("interrupted" | "replaced" | "review_ended" | "budget_limited")) { return Ok(None); }
                let failed = match p.get("error").filter(|v| !v.is_null()) {
                    Some(error) => affects_turn_status(error).ok_or(())?,
                    None => false,
                };
                e.kind = if failed { "error" } else { "cancelled" };
                e.title = Some(if failed { "Task reported an error" } else { "Task interrupted" }.into());
            }
            ("event_msg", "error") => {
                if !affects_turn_status(p).ok_or(())? { return Ok(None); }
                e.kind = "error"; e.title = Some("Task reported an error".into());
            }
            ("response_item", "function_call" | "custom_tool_call") => {
                let name = string(p, "name").ok_or(())?;
                e.kind = "tool"; e.phase = Some("started"); e.tool = Some(clean(name, 100)); e.title = e.tool.clone();
                e.call_id = string(p, "call_id").map(|s| clean(s, 160));
                let args: Value = string(p, "arguments").and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null);
                let short = name.rsplit('.').next().unwrap_or(name);
                if matches!(short, "exec_command" | "shell" | "local_shell") {
                    e.command = string(&args, "cmd").or_else(|| string(&args, "command")).map(|s| clean(s, 4000));
                }
                let raw = string(p, "input").or_else(|| string(&args, "input")).or_else(|| string(&args, "patch"));
                if short == "apply_patch" {
                    if let Some(raw) = raw { attach_patch(&mut e, raw); }
                } else if short == "exec" {
                    if let Some(raw) = raw { extract_script(&mut e, raw); }
                }
            }
            ("response_item", "function_call_output" | "custom_tool_call_output") => {
                e.kind = "tool"; e.phase = Some("completed");
                e.call_id = string(p, "call_id").map(|s| clean(s, 160));
                let output = p.get("output").unwrap_or(&Value::Null);
                let (text, code, failed) = decode_output(output, 0);
                e.exit_code = code.or_else(|| exit_code(&text));
                if e.exit_code.is_some_and(|v| v != 0) || failed { e.phase = Some("failed"); }
                e.output = Some(clean(&text, 6000));
            }
            _ => return Ok(None),
        }
        Ok(Some(e))
    }
}

// Matches the installed 0.160.0 protocol's replay semantics without retaining error text.
fn affects_turn_status(error: &Value) -> Option<bool> {
    error.get("message")?.as_str()?;
    let classification = &error["codex_error_info"];
    let nonterminal = classification.as_str() == Some("thread_rollback_failed")
        || classification.get("active_turn_not_steerable").is_some();
    Some(!nonterminal)
}

fn decode_output(value: &Value, depth: usize) -> (String, Option<i32>, bool) {
    if depth >= 3 { return (value.as_str().unwrap_or("").to_owned(), None, false); }
    if let Some(raw) = value.as_str() {
        if let Ok(decoded) = serde_json::from_str::<Value>(raw) {
            if decoded.is_object() { return decode_output(&decoded, depth + 1); }
        }
        return (raw.to_owned(), None, false);
    }
    let code = value.get("exit_code").and_then(Value::as_i64).and_then(|v| i32::try_from(v).ok());
    let failed = value.get("isError").and_then(Value::as_bool) == Some(true);
    if let Some(output) = value.get("output") {
        let (text, nested_code, nested_failed) = decode_output(output, depth + 1);
        return (text, code.or(nested_code), failed || nested_failed);
    }
    if let Some(blocks) = value.get("content").and_then(Value::as_array) {
        let text = blocks.iter().take(16).filter_map(|block| string(block, "text")).collect::<Vec<_>>().join("\n");
        let (text, nested_code, nested_failed) = decode_output(&Value::String(text), depth + 1);
        return (text, code.or(nested_code), failed || nested_failed);
    }
    (value.to_string(), code, failed)
}

fn attach_patch(e: &mut CodingEvent, raw: &str) {
    let files: Vec<_> = raw.lines().filter_map(|line| ["*** Update File: ", "*** Add File: ", "*** Delete File: ", "*** Move to: "].iter().find_map(|p| line.strip_prefix(p))).take(40).map(|s| clean(s.trim(), 512)).collect();
    if !files.is_empty() { e.files = Some(files); }
    e.patch = Some(clean(raw, 8000));
}

// Decode literal JSON strings only. Never evaluate JavaScript from a transcript.
fn extract_script(e: &mut CodingEvent, raw: &str) {
    static LITERALS: OnceLock<Regex> = OnceLock::new();
    let literals = LITERALS.get_or_init(|| Regex::new(r#""(?:\\.|[^"\\])*""#).expect("fixed literal regex"));
    for m in literals.find_iter(raw).take(100) {
        if let Ok(decoded) = serde_json::from_str::<String>(m.as_str()) {
            if decoded.starts_with("*** Begin Patch") && raw.contains("apply_patch") { attach_patch(e, &decoded); }
        }
    }
    if e.patch.is_none() && raw.contains("apply_patch") {
        if let Some(start) = raw.find("*** Begin Patch") {
            if let Some(end) = raw[start..].find("*** End Patch") { attach_patch(e, &raw[start..start + end + 15]); }
        }
    }
    if raw.matches("exec_command").count() == 1 && e.patch.is_none() {
        static COMMAND: OnceLock<Regex> = OnceLock::new();
        let pattern = COMMAND.get_or_init(|| Regex::new(r#"(?:"(?:cmd|command)"|\b(?:cmd|command))\s*:\s*("(?:\\.|[^"\\])*")"#).expect("fixed command regex"));
        if let Some(c) = pattern.captures(raw) { e.command = serde_json::from_str::<String>(&c[1]).ok().map(|s| clean(&s, 4000)); }
    }
}

fn exit_code(text: &str) -> Option<i32> {
    static EXIT: OnceLock<Regex> = OnceLock::new();
    let re = EXIT.get_or_init(|| Regex::new(r#"(?i)(?:process exited with code|exit_code["\s]*:|exit code:)\s*(-?\d+)"#).expect("fixed exit regex"));
    // A mixed output carrying several different process results is ambiguous.
    let codes: Vec<i32> = re.captures_iter(text).filter_map(|c| c[1].parse().ok()).collect();
    let first = *codes.first()?;
    codes.iter().all(|c| *c == first).then_some(first)
}

pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * m + 2) / 5 + day - 1 - 719468
}

fn timestamp(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') || b.get(10) != Some(&b'T') || !s.ends_with('Z') { return None; }
    let n = |a, z| s.get(a..z)?.parse::<i64>().ok();
    let (y,m,d,h,min,sec) = (n(0,4)?,n(5,7)?,n(8,10)?,n(11,13)?,n(14,16)?,n(17,19)?);
    if !(1970..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(0..24).contains(&h) || !(0..60).contains(&min) || !(0..60).contains(&sec) { return None; }
    let fraction = s.get(19..s.len()-1)?;
    let ms = if fraction.is_empty() {0} else { let digits = fraction.strip_prefix('.')?; if !digits.bytes().all(|b| b.is_ascii_digit()) { return None; } format!("{digits:0<3}").get(..3)?.parse::<i64>().ok()? };
    u64::try_from((days_from_civil(y,m,d)*86400+h*3600+min*60+sec)*1000+ms).ok()
}

