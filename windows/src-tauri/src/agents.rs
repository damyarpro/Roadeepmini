// Local custom agents: a name, instructions and a few chat options, stored on
// this PC in %APPDATA%\Roadeep\agents.json. Roadeep has no system prompt, so the
// instructions ride along with the first message of a new thread (chat.rs).
//
// The file is small (≤ 50 agents), so every call reads it fresh and every change
// rewrites it atomically: temp file + rename. A file that does not parse, or
// holds entries that fail validation, is moved aside as agents.json.bad-<time>
// before anything is written over it — a user's agents are never lost silently.
// A file that exists but cannot be read right now (another process holds it,
// access denied) is an error, never "no agents": writing then would replace it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::log;
use crate::roadeep::http::RoadeepError;

pub const MAX_AGENTS: usize = 50;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_INSTRUCTIONS_CHARS: usize = 8000;
pub const MAX_DESCRIPTION_CHARS: usize = 200;
pub const MAX_STARTER_PROMPTS: usize = 5;
pub const MAX_STARTER_PROMPT_CHARS: usize = 200;
const MAX_MODEL_CHARS: usize = 200;
/// What chat_send's agent parameter starts with when it names a local agent.
pub const LOCAL_PREFIX: &str = "local:";
const FILE_VERSION: u32 = 1;
/// Broadcast to every window after a save or delete, so the island's pills and
/// chat picker follow edits made in the settings window.
pub const CHANGED_EVENT: &str = "local-agents-changed";

pub mod agent_codes {
    pub const LIMIT: &str = "LOCAL_AGENT_LIMIT";
    pub const NOT_FOUND: &str = "LOCAL_AGENT_NOT_FOUND";
    pub const STORE: &str = "LOCAL_AGENT_STORE_ERROR";
}

/// Serializes read-modify-write cycles; the file is the only state.
static FILE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalAgent {
    pub id: String,
    pub name: String,
    pub instructions: String,
    /// Roadeep model id for threads started with this agent; "" = settings.model.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub web_search: bool,
    /// Official Roadeep agent this one builds on (sent as agent_id), if any.
    #[serde(default)]
    pub base_agent_id: Option<String>,
    /// One line shown under the name. Absent in files from older builds.
    #[serde(default)]
    pub description: String,
    /// Pill colour, "#RRGGBB"; "" = derived from the id by the front end.
    #[serde(default)]
    pub color: String,
    /// Quick chips shown in an empty chat with this agent.
    #[serde(default)]
    pub starter_prompts: Vec<String>,
    /// Unix milliseconds.
    pub created_at: u64,
    pub updated_at: u64,
}

/// What the settings window sends: no id = create.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub instructions: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub web_search: bool,
    #[serde(default)]
    pub base_agent_id: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub starter_prompts: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    agents: Vec<Value>,
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Roadeep agent ids are UUIDs; local ids are generated here in the same shape.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_model(model: &str) -> bool {
    model.len() <= MAX_MODEL_CHARS
        && model.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'))
}

fn has_control(text: &str, allow_newlines: bool) -> bool {
    text.chars().any(|c| c.is_control() && !(allow_newlines && matches!(c, '\n' | '\r' | '\t')))
}

/// "#RRGGBB" → upper-cased; anything else (named colours, #RGB, alpha) → None.
pub fn normalize_color(color: &str) -> Option<String> {
    let c = color.trim();
    let hex = c.strip_prefix('#')?;
    (hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit())).then(|| format!("#{}", hex.to_ascii_uppercase()))
}

/// Trimmed, blanks dropped; None when there are too many or one is too long.
fn clean_prompts(prompts: &[String]) -> Option<Vec<String>> {
    let cleaned: Vec<String> = prompts.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    let fits = cleaned.len() <= MAX_STARTER_PROMPTS
        && cleaned.iter().all(|p| p.chars().count() <= MAX_STARTER_PROMPT_CHARS && !has_control(p, false));
    fits.then_some(cleaned)
}

/// Trims and checks a draft. The error names the field, like the server does.
pub fn validate(draft: &AgentDraft) -> Result<AgentDraft, RoadeepError> {
    let name = draft.name.trim().to_string();
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_CHARS || has_control(&name, false) {
        return Err(RoadeepError::validation("name", "Give the agent a name of 1 to 60 characters."));
    }
    let instructions = draft.instructions.trim().replace("\r\n", "\n");
    let count = instructions.chars().count();
    if count == 0 || count > MAX_INSTRUCTIONS_CHARS || has_control(&instructions, true) {
        return Err(RoadeepError::validation("instructions", "Instructions must be 1 to 8000 characters."));
    }
    let model = draft.model.trim().to_string();
    if !valid_model(&model) {
        return Err(RoadeepError::validation("model", "Unknown model."));
    }
    let base_agent_id = match draft.base_agent_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) if valid_id(id) => Some(id.to_string()),
        Some(_) => return Err(RoadeepError::validation("baseAgentId", "Unknown Roadeep agent.")),
        None => None,
    };
    let id = match draft.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) if valid_id(id) => Some(id.to_string()),
        Some(_) => return Err(RoadeepError::validation("id", "Unknown agent.")),
        None => None,
    };
    let description = draft.description.trim().to_string();
    if description.chars().count() > MAX_DESCRIPTION_CHARS || has_control(&description, false) {
        return Err(RoadeepError::validation("description", "The description can be up to 200 characters."));
    }
    let color = match draft.color.trim() {
        "" => String::new(),
        c => normalize_color(c).ok_or_else(|| RoadeepError::validation("color", "Pick a colour like #7C5CFF."))?,
    };
    let Some(starter_prompts) = clean_prompts(&draft.starter_prompts) else {
        return Err(RoadeepError::validation("starterPrompts", "Up to 5 starter prompts of 200 characters each."));
    };
    Ok(AgentDraft {
        id,
        name,
        instructions,
        model,
        web_search: draft.web_search,
        base_agent_id,
        description,
        color,
        starter_prompts,
    })
}

/// A stored agent must still pass today's rules, or the file is suspect.
fn check_stored(value: &Value) -> Option<LocalAgent> {
    let agent: LocalAgent = serde_json::from_value(value.clone()).ok()?;
    let draft = AgentDraft {
        id: Some(agent.id.clone()),
        name: agent.name.clone(),
        instructions: agent.instructions.clone(),
        model: agent.model.clone(),
        web_search: agent.web_search,
        base_agent_id: agent.base_agent_id.clone(),
        description: agent.description.clone(),
        color: agent.color.clone(),
        starter_prompts: agent.starter_prompts.clone(),
    };
    let clean = validate(&draft).ok()?;
    (clean.id.as_deref() == Some(agent.id.as_str())).then_some(LocalAgent {
        name: clean.name,
        instructions: clean.instructions,
        model: clean.model,
        base_agent_id: clean.base_agent_id,
        description: clean.description,
        color: clean.color,
        starter_prompts: clean.starter_prompts,
        ..agent
    })
}

// ── Ids and time ──────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A random 64-bit value from the std hasher's per-instance random keys — no
/// extra crate for what is only a local, non-secret identifier.
fn random_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    hasher.finish()
}

/// UUID v4 layout (8-4-4-4-12 hex).
pub fn new_id() -> String {
    let (a, b) = (random_u64(), random_u64());
    let hi = (a & 0xffff_ffff_ffff_0fff) | 0x4000;
    let lo = (b & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        hi >> 32,
        (hi >> 16) & 0xffff,
        hi & 0xffff,
        lo >> 48,
        lo & 0xffff_ffff_ffff
    )
}

// ── File ──────────────────────────────────────────────────────────────────────

pub fn store_path() -> PathBuf {
    crate::settings::config_dir().join("agents.json")
}

fn store_error(context: &str, err: impl std::fmt::Display) -> RoadeepError {
    log::line(format!("agents: {context}: {err}"));
    RoadeepError::new(agent_codes::STORE, "Could not save your agents on this PC.")
}

/// Moves a suspect file aside so the next write cannot destroy it.
fn quarantine(path: &Path, why: &str) {
    let backup = path.with_file_name(format!("agents.json.bad-{}", now_ms()));
    match std::fs::rename(path, &backup) {
        Ok(()) => log::line(format!("agents: {why}; kept the old file as {}", backup.display())),
        Err(err) => log::line(format!("agents: {why}; could not move it aside: {err}")),
    }
}

/// Reads the store. Missing file = no agents. A damaged file is set aside (and
/// logged) and whatever was valid in it is kept. A file that is there but cannot
/// be read is an error: callers that write must not go on as if it were empty.
pub fn load_from(path: &Path) -> std::io::Result<Vec<LocalAgent>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            log::line(format!("agents: could not read {}: {err}", path.display()));
            return Err(err);
        }
    };
    let file: StoreFile = match serde_json::from_slice(&bytes) {
        Ok(file) => file,
        Err(err) => {
            quarantine(path, &format!("agents.json is not valid ({err})"));
            return Ok(Vec::new());
        }
    };
    let total = file.agents.len();
    let mut agents: Vec<LocalAgent> = Vec::new();
    for value in &file.agents {
        if let Some(agent) = check_stored(value) {
            if agents.len() < MAX_AGENTS && !agents.iter().any(|a| a.id == agent.id) {
                agents.push(agent);
            }
        }
    }
    if agents.len() != total || file.version != FILE_VERSION {
        quarantine(path, &format!("agents.json had {} unusable entries (format v{})", total - agents.len(), file.version));
        if let Err(err) = save_to(path, &agents) {
            log::line(format!("agents: could not rewrite the cleaned file: {err}"));
        }
    }
    Ok(agents)
}

/// Atomic: the old file stays intact until the new one is completely on disk.
pub fn save_to(path: &Path, agents: &[LocalAgent]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let values = agents.iter().map(serde_json::to_value).collect::<Result<Vec<_>, _>>()?;
    let json = serde_json::to_vec_pretty(&StoreFile { version: FILE_VERSION, agents: values })?;
    let tmp = path.with_file_name("agents.json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

// ── Operations (path-explicit so tests never touch %APPDATA%) ─────────────────

/// The store as it is now, or the STORE error when it cannot be read.
fn load_for_write(path: &Path) -> Result<Vec<LocalAgent>, RoadeepError> {
    load_from(path).map_err(|e| store_error("refusing to write over a file that could not be read", e))
}

pub fn save_in(path: &Path, draft: AgentDraft) -> Result<LocalAgent, RoadeepError> {
    let draft = validate(&draft)?;
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut agents = load_for_write(path)?;
    let now = now_ms();
    let saved = match draft.id {
        Some(id) => {
            let Some(agent) = agents.iter_mut().find(|a| a.id == id) else {
                return Err(RoadeepError::new(agent_codes::NOT_FOUND, "That agent no longer exists."));
            };
            agent.name = draft.name;
            agent.instructions = draft.instructions;
            agent.model = draft.model;
            agent.web_search = draft.web_search;
            agent.base_agent_id = draft.base_agent_id;
            agent.description = draft.description;
            agent.color = draft.color;
            agent.starter_prompts = draft.starter_prompts;
            agent.updated_at = now.max(agent.created_at);
            agent.clone()
        }
        None => {
            if agents.len() >= MAX_AGENTS {
                return Err(RoadeepError::new(agent_codes::LIMIT, "You can keep up to 50 agents."));
            }
            let agent = LocalAgent {
                id: new_id(),
                name: draft.name,
                instructions: draft.instructions,
                model: draft.model,
                web_search: draft.web_search,
                base_agent_id: draft.base_agent_id,
                description: draft.description,
                color: draft.color,
                starter_prompts: draft.starter_prompts,
                created_at: now,
                updated_at: now,
            };
            agents.push(agent.clone());
            agent
        }
    };
    save_to(path, &agents).map_err(|e| store_error("save failed", e))?;
    Ok(saved)
}

pub fn delete_in(path: &Path, id: &str) -> Result<(), RoadeepError> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut agents = load_for_write(path)?;
    let before = agents.len();
    agents.retain(|a| a.id != id);
    if agents.len() == before {
        return Err(RoadeepError::new(agent_codes::NOT_FOUND, "That agent no longer exists."));
    }
    save_to(path, &agents).map_err(|e| store_error("delete failed", e))
}

/// What adding the built-in agents did (default_agents.rs).
#[derive(Debug, Default, PartialEq)]
pub struct SeedReport {
    pub added: usize,
    /// Already in the store under the same id: left exactly as they are.
    pub present: usize,
    /// Left out because the store was full.
    pub over_limit: usize,
    /// Every agent id in the store afterwards.
    pub ids: Vec<String>,
}

/// The agents of a store file that passes every check `load_from` makes,
/// or None when `load_from` would set it aside or clean it.
fn parse_clean(bytes: &[u8]) -> Option<Vec<LocalAgent>> {
    let file: StoreFile = serde_json::from_slice(bytes).ok()?;
    if file.version != FILE_VERSION || file.agents.len() > MAX_AGENTS {
        return None;
    }
    let mut agents: Vec<LocalAgent> = Vec::new();
    for value in &file.agents {
        let agent = check_stored(value)?;
        if agents.iter().any(|a| a.id == agent.id) {
            return None;
        }
        agents.push(agent);
    }
    Some(agents)
}

/// Adds each of `defaults` whose id is not in the store yet, within
/// MAX_AGENTS, after the user's own agents; never changes or duplicates one.
/// Only a store that reads cleanly is written: a missing file counts as empty,
/// an unreadable or damaged one is an error and stays exactly as it is (the
/// next `load_from` deals with it as usual).
pub fn seed_in(path: &Path, defaults: &[LocalAgent]) -> Result<SeedReport, String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut agents = match std::fs::read(path) {
        Ok(bytes) => parse_clean(&bytes).ok_or("agents.json is not valid")?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(format!("agents.json could not be read: {err}")),
    };
    let now = now_ms();
    let mut report = SeedReport::default();
    for default in defaults {
        if agents.iter().any(|a| a.id == default.id) {
            report.present += 1;
        } else if agents.len() >= MAX_AGENTS {
            report.over_limit += 1;
        } else {
            agents.push(LocalAgent { created_at: now, updated_at: now, ..default.clone() });
            report.added += 1;
        }
    }
    if report.added > 0 {
        save_to(path, &agents).map_err(|e| format!("agents.json could not be written: {e}"))?;
    }
    report.ids = agents.into_iter().map(|a| a.id).collect();
    Ok(report)
}

pub fn list() -> Result<Vec<LocalAgent>, RoadeepError> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load_from(&store_path()).map_err(|e| store_error("read failed", e))
}

/// For chat: the agent behind a "local:<id>" reference. An unreadable store
/// reads as "not found" here; list() has already logged why.
pub fn find(id: &str) -> Option<LocalAgent> {
    if !valid_id(id) {
        return None;
    }
    list().ok()?.into_iter().find(|a| a.id == id)
}

fn announce_change(app: &AppHandle) {
    if let Err(err) = app.emit(CHANGED_EVENT, ()) {
        log::line(format!("agents: could not emit {CHANGED_EVENT}: {err}"));
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn local_agents_list() -> Result<Vec<LocalAgent>, RoadeepError> {
    list()
}

/// Creates (no id) or updates an agent; returns it as stored.
#[tauri::command]
pub fn local_agent_save(app: AppHandle, agent: AgentDraft) -> Result<LocalAgent, RoadeepError> {
    let creating = agent.id.is_none();
    let saved = save_in(&store_path(), agent)?;
    log::line(format!("agents: {} local agent {}", if creating { "created" } else { "updated" }, saved.id));
    announce_change(&app);
    Ok(saved)
}

#[tauri::command]
pub fn local_agent_delete(app: AppHandle, id: String) -> Result<(), RoadeepError> {
    if !valid_id(&id) {
        return Err(RoadeepError::validation("id", "Unknown agent."));
    }
    delete_in(&store_path(), &id)?;
    log::line(format!("agents: deleted local agent {id}"));
    announce_change(&app);
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roadeep-agents-{tag}-{}-{}", std::process::id(), random_u64()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn draft(name: &str, instructions: &str) -> AgentDraft {
        AgentDraft {
            id: None,
            name: name.into(),
            instructions: instructions.into(),
            model: String::new(),
            web_search: false,
            base_agent_id: None,
            description: String::new(),
            color: String::new(),
            starter_prompts: Vec::new(),
        }
    }

    fn field_of(err: &RoadeepError) -> String {
        err.field_errors.as_ref().and_then(|f| f.keys().next().cloned()).unwrap_or_default()
    }

    fn load(path: &Path) -> Vec<LocalAgent> {
        load_from(path).expect("store readable")
    }

    #[test]
    fn validation_rules() {
        let ok = validate(&draft("  Writer  ", " Be brief.\r\nAlways. ")).unwrap();
        assert_eq!(ok.name, "Writer");
        assert_eq!(ok.instructions, "Be brief.\nAlways.");

        assert_eq!(field_of(&validate(&draft("", "x")).unwrap_err()), "name");
        assert_eq!(field_of(&validate(&draft("   ", "x")).unwrap_err()), "name");
        assert_eq!(field_of(&validate(&draft(&"n".repeat(61), "x")).unwrap_err()), "name");
        assert!(validate(&draft(&"ن".repeat(60), "x")).is_ok(), "limits count characters, not bytes");
        assert_eq!(field_of(&validate(&draft("a\u{0007}", "x")).unwrap_err()), "name");
        assert_eq!(field_of(&validate(&draft("a", "")).unwrap_err()), "instructions");
        assert_eq!(field_of(&validate(&draft("a", &"i".repeat(8001))).unwrap_err()), "instructions");
        assert!(validate(&draft("a", &"i".repeat(8000))).is_ok());

        let mut d = draft("a", "b");
        d.model = "bad model".into();
        assert_eq!(field_of(&validate(&d).unwrap_err()), "model");
        d.model = "openrouter/openai-gpt-5".into();
        assert!(validate(&d).is_ok());
        d.base_agent_id = Some("../etc".into());
        assert_eq!(field_of(&validate(&d).unwrap_err()), "baseAgentId");
        d.base_agent_id = Some("  ".into());
        assert_eq!(validate(&d).unwrap().base_agent_id, None);
        d.id = Some("x y".into());
        assert_eq!(field_of(&validate(&d).unwrap_err()), "id");
    }

    #[test]
    fn description_and_starter_prompt_rules() {
        let mut d = draft("a", "b");
        d.description = format!("  {}  ", "د".repeat(200));
        assert_eq!(validate(&d).unwrap().description.chars().count(), 200);
        d.description = "d".repeat(201);
        assert_eq!(field_of(&validate(&d).unwrap_err()), "description");
        d.description = "line\nbreak".into();
        assert_eq!(field_of(&validate(&d).unwrap_err()), "description");
        d.description = String::new();

        d.starter_prompts = vec!["  one ".into(), "".into(), "   ".into(), "two".into()];
        assert_eq!(validate(&d).unwrap().starter_prompts, vec!["one".to_string(), "two".to_string()]);
        d.starter_prompts = (0..6).map(|i| format!("p{i}")).collect();
        assert_eq!(field_of(&validate(&d).unwrap_err()), "starterPrompts");
        d.starter_prompts = vec!["p".repeat(201)];
        assert_eq!(field_of(&validate(&d).unwrap_err()), "starterPrompts");
        d.starter_prompts = vec!["پ".repeat(200); 5];
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn colour_rules() {
        assert_eq!(normalize_color("#7c5cff").as_deref(), Some("#7C5CFF"));
        assert_eq!(normalize_color(" #22C55E ").as_deref(), Some("#22C55E"));
        for bad in ["7C5CFF", "#7C5CF", "#7C5CFF00", "#GGGGGG", "red", "", "#"] {
            assert_eq!(normalize_color(bad), None, "{bad}");
        }
        let mut d = draft("a", "b");
        d.color = "#a78bfa".into();
        assert_eq!(validate(&d).unwrap().color, "#A78BFA");
        d.color = "  ".into();
        assert_eq!(validate(&d).unwrap().color, "", "blank = derived from the id");
        d.color = "javascript:alert(1)".into();
        assert_eq!(field_of(&validate(&d).unwrap_err()), "color");
    }

    #[test]
    fn ids_look_like_uuids_and_differ() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(valid_id(&a));
    }

    #[test]
    fn save_load_round_trip_update_and_delete() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("agents.json");
        assert!(load(&path).is_empty(), "missing file = no agents");

        let mut d = draft("Translator", "Translate to French.");
        d.web_search = true;
        d.base_agent_id = Some("3f1c2b9e-0000-4000-8000-123456789abc".into());
        d.color = "#38bdf8".into();
        d.description = "French, formal".into();
        d.starter_prompts = vec!["Translate this email".into()];
        let created = save_in(&path, d).unwrap();
        assert!(valid_id(&created.id));
        assert_eq!(created.created_at, created.updated_at);
        assert_eq!(created.color, "#38BDF8");

        let loaded = load(&path);
        assert_eq!(loaded, vec![created.clone()]);
        assert!(!dir.join("agents.json.tmp").exists(), "temp file is renamed into place");

        let mut edit = draft("Translator FR", "Translate to French, formally.");
        edit.id = Some(created.id.clone());
        edit.model = "m-1".into();
        let updated = save_in(&path, edit).unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.created_at, created.created_at);
        assert!(!updated.web_search && updated.base_agent_id.is_none());
        assert!(updated.color.is_empty() && updated.starter_prompts.is_empty() && updated.description.is_empty());
        assert_eq!(load(&path), vec![updated.clone()]);

        let mut missing = draft("x", "y");
        missing.id = Some(new_id());
        assert_eq!(save_in(&path, missing).unwrap_err().code, agent_codes::NOT_FOUND);

        delete_in(&path, &created.id).unwrap();
        assert!(load(&path).is_empty());
        assert_eq!(delete_in(&path, &created.id).unwrap_err().code, agent_codes::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_from_older_builds_still_load() {
        let dir = temp_dir("compat");
        let path = dir.join("agents.json");
        // Exactly what the previous build wrote: no description, colour or prompts.
        let old = r#"{ "version": 1, "agents": [ {
            "id": "3f1c2b9e-0000-4000-8000-123456789abc", "name": "Editor",
            "instructions": "Be strict.", "model": "", "webSearch": false,
            "baseAgentId": null, "createdAt": 1, "updatedAt": 2 } ] }"#;
        std::fs::write(&path, old).unwrap();
        let agents = load(&path);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "Editor");
        assert!(agents[0].color.is_empty() && agents[0].description.is_empty() && agents[0].starter_prompts.is_empty());
        assert!(backups(&dir).is_empty(), "an old but valid file is not quarantined");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old, "and not rewritten either");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_store_is_never_written_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp_dir("locked");
        let path = dir.join("agents.json");
        save_in(&path, draft("Keep me", "x")).unwrap();
        let before = std::fs::read(&path).unwrap();
        {
            // Another process (antivirus, OneDrive) holding the file with no sharing.
            let _held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
            assert!(load_from(&path).is_err(), "a sharing violation is not an empty store");
            assert_eq!(save_in(&path, draft("New", "y")).unwrap_err().code, agent_codes::STORE);
            assert_eq!(delete_in(&path, "anything").unwrap_err().code, agent_codes::STORE);
        }
        assert_eq!(std::fs::read(&path).unwrap(), before, "file untouched");
        assert_eq!(load(&path).len(), 1);
        assert!(backups(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limit_of_fifty() {
        let dir = temp_dir("limit");
        let path = dir.join("agents.json");
        for i in 0..MAX_AGENTS {
            save_in(&path, draft(&format!("a{i}"), "x")).unwrap();
        }
        assert_eq!(save_in(&path, draft("one more", "x")).unwrap_err().code, agent_codes::LIMIT);
        assert_eq!(load(&path).len(), MAX_AGENTS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn backups(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("agents.json.bad-"))
            .collect()
    }

    #[test]
    fn corrupt_file_is_backed_up_not_lost() {
        let dir = temp_dir("corrupt");
        let path = dir.join("agents.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(load(&path).is_empty());
        let saved = backups(&dir);
        assert_eq!(saved.len(), 1);
        assert_eq!(std::fs::read(&saved[0]).unwrap(), b"{ not json");
        assert!(!path.exists());

        // A save after that starts a fresh file and leaves the backup alone.
        save_in(&path, draft("a", "b")).unwrap();
        assert_eq!(load(&path).len(), 1);
        assert_eq!(backups(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_entries_are_dropped_with_a_backup() {
        let dir = temp_dir("partial");
        let path = dir.join("agents.json");
        let good = LocalAgent {
            id: new_id(),
            name: "Good".into(),
            instructions: "Fine".into(),
            model: String::new(),
            web_search: false,
            base_agent_id: None,
            description: String::new(),
            color: "#22C55E".into(),
            starter_prompts: vec!["Hi".into()],
            created_at: 1,
            updated_at: 2,
        };
        let raw = serde_json::json!({ "version": 1, "agents": [
            serde_json::to_value(&good).unwrap(),
            { "id": "bad id", "name": "x", "instructions": "y", "createdAt": 1, "updatedAt": 1 },
            { "id": new_id(), "name": "x", "instructions": "y", "color": "url(x)", "createdAt": 1, "updatedAt": 1 },
            { "name": "no id" }
        ]});
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(load(&path), vec![good.clone()]);
        assert_eq!(backups(&dir).len(), 1);
        // The cleaned file is valid now: loading again changes nothing.
        assert_eq!(load(&path), vec![good]);
        assert_eq!(backups(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
