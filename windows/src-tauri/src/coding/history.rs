//! Bounded opt-in structural evidence. No commands, outputs, patches or prompt titles.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::State;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_EVENTS: usize = 512;
const AGE: u64 = 7 * 86_400_000;
#[derive(Clone)]
pub struct CodingHistory { inner: Arc<Mutex<Inner>>, path: PathBuf }
struct Inner { enabled: bool, revision: u64 }
#[derive(Serialize)]
pub struct Snapshot { pub revision: u64, pub events: Vec<Value> }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope { version: u32, events: Vec<Value> }
fn fail(code: &'static str) -> String { crate::log::line(format!("coding: {code}")); code.into() }
fn bounded_text(s: &str, max: usize) -> String {
    let redacted = super::parser::clean(s, 2048);
    let mut units = 0;
    redacted.chars().take_while(|c| { units += c.len_utf16(); units <= max }).collect()
}
fn text(v: &Value, key: &str, max: usize, out: &mut Map<String, Value>) -> Result<(), String> {
    if let Some(value) = v.get(key) {
        let s = value.as_str().filter(|s| s.encode_utf16().count() <= max && !s.contains('\0')).ok_or_else(|| fail("history-invalid"))?;
        out.insert(key.into(), Value::String(bounded_text(s, max)));
    }
    Ok(())
}
fn normalize(events: Vec<Value>, at: u64) -> Result<Vec<Value>, String> {
    if events.len() > MAX_EVENTS || serde_json::to_vec(&events).map_err(|_| fail("history-invalid"))?.len() > MAX_BYTES { return Err(fail("history-invalid")); }
    let mut out = Vec::new();
    for v in events {
        let object = v.as_object().ok_or_else(|| fail("history-invalid"))?;
        let id = object.get("id").and_then(Value::as_str).ok_or_else(|| fail("history-invalid"))?;
        let session = v["sessionId"].as_str().ok_or_else(|| fail("history-invalid"))?;
        if [id, session].iter().any(|s| s.is_empty() || s.encode_utf16().count() > 200 || s.chars().any(char::is_control)) || !matches!(v["harness"].as_str(), Some("codex" | "claude" | "gemini" | "cursor" | "windsurf" | "copilot" | "vscode" | "kiro" | "opencode")) { return Err(fail("history-invalid")); }
        let kind = v["kind"].as_str().filter(|s| matches!(*s, "session" | "prompt" | "tool" | "finished" | "cancelled" | "error" | "usage")).ok_or_else(|| fail("history-invalid"))?;
        let time = v["at"].as_u64().filter(|n| *n <= at + 60_000).ok_or_else(|| fail("history-invalid"))?;
        if time < at.saturating_sub(AGE) { continue; }
        let mut item = Map::new();
        for key in ["id", "sessionId", "harness", "kind", "at"] { item.insert(key.into(), v[key].clone()); }
        for (key, max) in [("cwd", 512), ("tool", 512), ("callId", 200)] { text(&v, key, max, &mut item)?; }
        if v.get("callId").and_then(Value::as_str).is_some_and(|s| s.chars().any(char::is_control)) { return Err(fail("history-invalid")); }
        if let Some(phase) = v.get("phase") {
            if !matches!(phase.as_str(), Some("started" | "completed" | "failed")) { return Err(fail("history-invalid")); }
            item.insert("phase".into(), phase.clone());
        }
        item.insert("title".into(), json!(match kind { "session" => match v["harness"].as_str() { Some("claude")=>"Claude session",Some("codex")=>"Codex session",Some("gemini")=>"Gemini session",Some("cursor")=>"Cursor session",Some("windsurf")=>"Windsurf session",Some("copilot")=>"Copilot session",Some("kiro")=>"Kiro session",Some("opencode")=>"OpenCode session",_=>"VS Code session" }, "prompt" => "New task", "tool" => "Observed tool", "finished" => "Task finished", "cancelled" => "Task interrupted", "error" => "Task reported an error", _ => "Observed usage" }));
        if let Some(code) = v.get("exitCode") { if code.as_i64().and_then(|n| i32::try_from(n).ok()).is_none() { return Err(fail("history-invalid")); } item.insert("exitCode".into(), code.clone()); }
        if let Some(files) = v.get("files") {
            let files = files.as_array().filter(|a| a.len() <= 60).ok_or_else(|| fail("history-invalid"))?;
            let mut paths = Vec::new();
            for file in files { let s = file.as_str().filter(|s| s.encode_utf16().count() <= 512).ok_or_else(|| fail("history-invalid"))?; paths.push(bounded_text(s, 512)); }
            item.insert("files".into(), json!(paths));
        }
        if let Some(summary) = v.get("testSummary") {
            if kind != "tool" || !matches!(v["phase"].as_str(), Some("completed" | "failed")) { return Err(fail("history-invalid")); }
            let mut counts = Map::new();
            for key in ["passed", "failed", "skipped"] {
                let number = summary[key].as_u64().filter(|n| *n <= 1_000_000_000).ok_or_else(|| fail("history-invalid"))?;
                counts.insert(key.into(), json!(number));
            }
            let verdict = summary["verdict"].as_str().filter(|s| matches!(*s, "passed" | "failed" | "unknown" | "skipped")).ok_or_else(|| fail("history-invalid"))?;
            let failed = counts["failed"].as_u64().unwrap() > 0 || v["exitCode"].as_i64().is_some_and(|n| n != 0) || v["phase"] == "failed";
            let passed = counts["passed"].as_u64().unwrap() > 0 && v["exitCode"] == 0;
            counts.insert("verdict".into(), json!(if failed { "failed" } else if verdict == "passed" && !passed { "unknown" } else { verdict }));
            item.insert("testSummary".into(), Value::Object(counts));
        }
        for key in ["context", "usage"] { if let Some(value) = v.get(key) { item.insert(key.into(), normalize_usage(key, value)?); } }
        if !out.iter().any(|e: &Value| e["id"] == v["id"]) { out.push(Value::Object(item)); }
    }
    Ok(out)
}
fn normalize_usage(key: &str, value: &Value) -> Result<Value, String> {
    let obj = value.as_object().ok_or_else(|| fail("history-invalid"))?;
    let mut out = Map::new();
    if key == "usage" {
        for key in ["primary", "secondary"] { if let Some(window) = obj.get(key) { out.insert(key.into(), normalize_usage("window", window)?); } }
    } else {
        let fields: &[(&str, f64)] = if key == "context" { &[("usedTokens", 1e9), ("limitTokens", 1e9), ("usedPercent", 100.0)] } else { &[("usedPercent", 100.0), ("resetsAt", 8_640_000_000_000_000.0), ("windowMinutes", 525_600.0)] };
        for (field, max) in fields {
            if let Some(v) = obj.get(*field) { if !v.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0 && n <= *max) { return Err(fail("history-invalid")); } out.insert((*field).into(), v.clone()); }
        }
        if key == "window" && !out.contains_key("usedPercent") { return Err(fail("history-invalid")); }
        if key == "context" { text(value, "model", 132, &mut out)?; }
    }
    Ok(Value::Object(out))
}
fn read_envelope(path: &Path) -> Result<Option<Envelope>, String> {
    let file = match fs::File::open(path) { Ok(file) => file, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(_) => return Err(fail("history-read-failed")) };
    let mut bytes = Vec::new(); file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|_| fail("history-read-failed"))?;
    if bytes.len() > MAX_BYTES { return Err(fail("history-invalid")); }
    let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| fail("history-invalid"))?;
    if envelope.version != 1 { return Err(fail("history-invalid")); }
    Ok(Some(envelope))
}
impl CodingHistory {
    pub fn new(enabled: bool) -> Self { Self::at(enabled, crate::settings::local_dir().join("coding-history-v1.json")) }
    fn at(enabled: bool, path: PathBuf) -> Self { Self { inner: Arc::new(Mutex::new(Inner { enabled, revision: 0 })), path } }
    fn check_path(&self) -> Result<(), String> { if self.path.is_absolute() { Ok(()) } else { Err(fail("history-storage-unavailable")) } }
    #[cfg(test)]
    fn set_enabled(&self, enabled: bool) -> Result<(), String> { self.commit_preference(enabled, || Ok(())) }
    pub fn commit_preference<F>(&self, enabled: bool, persist: F) -> Result<(), String> where F: FnOnce() -> Result<(), String> {
        self.check_path()?;
        let mut inner = self.inner.lock().map_err(|_| fail("history-lock-failed"))?;
        if !enabled { remove(&self.path)?; }
        persist()?;
        if inner.enabled != enabled { inner.revision += 1; inner.enabled = enabled; }
        Ok(())
    }
    fn snapshot(&self) -> Result<Snapshot, String> {
        self.check_path()?;
        let inner = self.inner.lock().map_err(|_| fail("history-lock-failed"))?;
        let events = if inner.enabled {
            match read_envelope(&self.path)? {
                Some(envelope) => {
                    let previous_count = envelope.events.len();
                    let events = normalize(envelope.events, super::now())?;
                    if previous_count != events.len() { write_envelope(&self.path, &events)?; }
                    events
                }
                None => Vec::new(),
            }
        } else { Vec::new() };
        Ok(Snapshot { revision: inner.revision, events })
    }
    fn save(&self, events: Vec<Value>, revision: u64) -> Result<bool, String> {
        self.check_path()?;
        let inner = self.inner.lock().map_err(|_| fail("history-lock-failed"))?;
        if !inner.enabled || inner.revision != revision { return Ok(false); }
        // Corrupt/unsupported content is preserved until explicit clear.
        if let Some(envelope) = read_envelope(&self.path)? { normalize(envelope.events, super::now())?; }
        let events = normalize(events, super::now())?;
        write_envelope(&self.path, &events)?;
        Ok(true)
    }
    pub fn clear(&self) -> Result<Snapshot, String> {
        self.check_path()?;
        let mut inner = self.inner.lock().map_err(|_| fail("history-lock-failed"))?;
        // A failed deletion must still invalidate outstanding saves.
        inner.revision += 1; remove(&self.path)?;
        Ok(Snapshot { revision: inner.revision, events: Vec::new() })
    }
}
fn write_envelope(path: &Path, events: &[Value]) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Envelope { version: 1, events: events.to_vec() }).map_err(|_| fail("history-invalid"))?;
    if bytes.len() > MAX_BYTES { return Err(fail("history-invalid")); }
    atomic_write(path, &bytes)
}
fn remove(path: &Path) -> Result<(), String> { match fs::remove_file(path) { Ok(()) => Ok(()), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(_) => Err(fail("history-delete-failed")) } }
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let directory = path.parent().ok_or_else(|| fail("history-write-failed"))?;
    fs::create_dir_all(directory).map_err(|_| fail("history-write-failed"))?;
    let temp = directory.join(format!(".history-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|_| fail("history-write-failed"))?;
        file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|_| fail("history-write-failed"))?; drop(file);
        replace(&temp, path).map_err(|_| fail("history-write-failed"))
    })();
    if result.is_err() { if let Err(e) = fs::remove_file(&temp) { if e.kind() != std::io::ErrorKind::NotFound { crate::log::line("coding: history temporary cleanup failed"); } } }
    result
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32; }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Valid NUL-terminated pointers remain alive; paths share a directory.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> { fs::rename(from, to) }
/// Expire local evidence without requiring a new coding event.
pub fn start(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let history = app.state::<CodingHistory>().inner().clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let worker = history.clone(); let result = tauri::async_runtime::spawn_blocking(move || worker.snapshot()).await;
            let code = match result { Ok(Ok(_)) => None, Ok(Err(code)) => Some(code), Err(_) => Some(fail("history-worker-failed")) };
            if let Some(code) = code { let _ = app.emit("coding-history-error", code); }
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}
#[tauri::command]
pub async fn coding_history_snapshot(history: State<'_, CodingHistory>) -> Result<Snapshot, String> { let history = history.inner().clone(); tauri::async_runtime::spawn_blocking(move || history.snapshot()).await.map_err(|_| fail("history-worker-failed"))? }
#[tauri::command]
pub async fn coding_history_save(history: State<'_, CodingHistory>, events: Vec<Value>, revision: u64) -> Result<bool, String> { let history = history.inner().clone(); tauri::async_runtime::spawn_blocking(move || history.save(events, revision)).await.map_err(|_| fail("history-worker-failed"))? }
#[tauri::command]
pub async fn coding_history_clear(history: State<'_, CodingHistory>) -> Result<Snapshot, String> { let history = history.inner().clone(); tauri::async_runtime::spawn_blocking(move || history.clear()).await.map_err(|_| fail("history-worker-failed"))? }
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> CodingHistory { CodingHistory::at(true, std::env::temp_dir().join(format!("roadeep-history-{}.json", uuid::Uuid::new_v4()))) }
    fn event() -> Value { json!({"id":"codex:fixture:1","sessionId":"codex:fixture","harness":"codex","at":super::super::now(),"kind":"tool","title":"private prompt","output":"API_KEY=private-value","command":"secret transcript","patch":"raw patch","context":{"usedTokens":5,"limitTokens":10,"usedPercent":50}}) }
    #[test]
    fn every_documented_hook_provider_round_trips_without_transcript() {
        for harness in ["claude","codex","gemini","cursor","windsurf","copilot","vscode","kiro","opencode"] {
            let input=json!({"id":format!("{harness}:fixture:event"),"sessionId":"shared-id","harness":harness,"at":super::super::now(),"kind":"session","title":"Private prompt","output":"private"});
            let out=normalize(vec![input],super::super::now()).unwrap();
            assert_eq!(out[0]["harness"],harness);assert!(out[0].get("output").is_none());assert!(!out[0]["title"].as_str().unwrap().contains("Private"));
        }
    }
    #[test]
    fn history_is_bounded_redacted_structural_atomic_and_revision_safe() {
        let h = fixture(); assert!(h.save(vec![event()], 0).unwrap()); assert!(h.save(vec![event()], 0).unwrap());
        let disk = fs::read_to_string(&h.path).unwrap(); for private in ["private prompt", "private-value", "secret transcript", "raw patch"] { assert!(!disk.contains(private)); }
        assert_eq!(h.snapshot().unwrap().events.len(), 1);
        let barrier = h.clear().unwrap().revision; assert_eq!(barrier, 1); assert!(!h.save(vec![event()], 0).unwrap()); assert!(!h.path.exists());
        assert!(h.save(vec![event()], barrier).unwrap()); h.set_enabled(false).unwrap(); assert!(!h.path.exists()); assert!(!h.save(vec![event()], barrier).unwrap()); assert!(h.snapshot().unwrap().events.is_empty());
    }
    #[test]
    fn invalid_history_and_expired_events_fail_safely() {
        let h = fixture(); let mut old = event(); old["at"] = json!(0); assert!(h.save(vec![old], 0).unwrap()); assert!(h.snapshot().unwrap().events.is_empty());
        assert!(h.save(vec![event(); MAX_EVENTS + 1], 0).is_err()); let mut bad = event(); bad["context"]["usedPercent"] = json!(101); assert!(h.save(vec![bad], 0).is_err());
        fs::write(&h.path, b"{broken").unwrap(); assert!(h.snapshot().is_err()); assert!(h.save(vec![event()], 0).is_err()); assert_eq!(fs::read(&h.path).unwrap(), b"{broken"); h.clear().unwrap();
    }
    #[test]
    fn late_save_cannot_resurrect_clear_or_disable() {
        let h = fixture(); let late = h.clone(); let generation = h.snapshot().unwrap().revision;
        h.clear().unwrap(); let join = std::thread::spawn(move || late.save(vec![event()], generation)); assert!(!join.join().unwrap().unwrap()); assert!(!h.path.exists());
        h.set_enabled(false).unwrap(); h.set_enabled(true).unwrap(); assert!(!h.save(vec![event()], generation).unwrap());
    }
    #[test]
    fn claude_hook_history_round_trips_with_generic_ids_and_safe_titles() {
        let h = fixture();
        let input = json!({"id":uuid::Uuid::new_v4().to_string(),"sessionId":uuid::Uuid::new_v4().to_string(),"harness":"claude","kind":"tool","phase":"completed","at":super::super::now(),"title":"Private prompt","files":["src/main.ts"],"callId":"fixture","testSummary":{"verdict":"passed","passed":2,"failed":0,"skipped":0},"exitCode":0});
        assert!(h.save(vec![input], 0).unwrap()); let events = h.snapshot().unwrap().events;
        assert_eq!(events[0]["harness"], "claude"); assert_eq!(events[0]["testSummary"]["verdict"], "passed"); assert_eq!(events[0]["title"], "Observed tool");
        for bad_id in ["bad\nID".to_string(), "a".repeat(201)] { let mut bad = event(); bad["id"] = json!(bad_id); assert!(h.save(vec![bad], 0).is_err()); }
        h.clear().unwrap();
    }
    #[test]
    fn expiration_prunes_disk_and_failed_disable_does_not_commit_preference() {
        let h = fixture(); let mut expired = event(); expired["at"] = json!(0);
        fs::write(&h.path, serde_json::to_vec(&Envelope { version: 1, events: vec![expired] }).unwrap()).unwrap(); assert!(h.snapshot().unwrap().events.is_empty());
        let disk: Envelope = serde_json::from_slice(&fs::read(&h.path).unwrap()).unwrap(); assert!(disk.events.is_empty()); h.clear().unwrap();
        fs::create_dir(&h.path).unwrap(); let persisted = std::sync::atomic::AtomicBool::new(false);
        assert!(h.commit_preference(false, || { persisted.store(true, std::sync::atomic::Ordering::Relaxed); Ok(()) }).is_err()); assert!(!persisted.load(std::sync::atomic::Ordering::Relaxed)); assert!(h.inner.lock().unwrap().enabled);
        fs::remove_dir(&h.path).unwrap(); h.clear().unwrap();
    }
    #[test]
    fn unicode_wire_boundaries_use_frontend_utf16_units() {
        let h = fixture(); let mut unicode = event(); let character = "\u{754c}";
        let path = format!("{}\n[truncated]", character.repeat(500)); unicode["files"] = json!([path]); unicode["cwd"] = json!(character.repeat(500)); unicode["tool"] = json!(character.repeat(500)); unicode["context"]["model"] = json!(character.repeat(120));
        assert!(h.save(vec![unicode], 0).unwrap()); let saved = h.snapshot().unwrap().events; assert_eq!(saved[0]["context"]["model"].as_str().unwrap().chars().count(), 120); assert_eq!(saved[0]["files"][0].as_str().unwrap().encode_utf16().count(), 512);
        let mut bad = event(); bad["files"] = json!([character.repeat(513)]); assert!(h.save(vec![bad], 0).is_err()); h.clear().unwrap();
    }
    #[test]
    fn cancellation_history_retains_terminal_kind_without_private_error_text() {
        let mut input = event(); input["kind"] = json!("cancelled");
        input["title"] = json!("private abort detail");
        let normalized = normalize(vec![input], super::super::now()).unwrap();
        assert_eq!(normalized[0]["kind"], "cancelled");
        assert_eq!(normalized[0]["title"], "Task interrupted");
        assert!(normalized[0].get("output").is_none());
    }

}
