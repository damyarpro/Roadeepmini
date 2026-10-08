// Preferences, stored as plain JSON in %APPDATA%\Roadeep\settings.json.
// No secret ever lands here — API keys and the Roadeep session live in the
// Windows Credential Manager.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Pills next to the character besides VS Code: integrations and agents share the limit.
pub const MAX_ACTIVE_PILLS: usize = 4;
const MAX_AGENT_COLORS: usize = 200;

/// The character's look — bloub's customiser (windows/src/character/appearance.ts):
/// body shape, colour and resting expression, stored as bloub ids.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterAppearance {
    pub shape: String,
    pub color: String,
    pub expression: String,
}

const CHARACTER_SHAPES: [&str; 8] = ["cercle", "galet", "squircle", "capsule", "triangle", "hexagone", "nuage", "goutte"];
const CHARACTER_COLORS: [&str; 12] = ["encre", "creme", "brun", "rouge", "orange", "ambre", "vert", "turquoise", "bleu", "violet", "rose", "gris"];
const CHARACTER_EXPRESSIONS: [&str; 16] = [
    "neutre", "attentif", "surpris", "excite", "heureux", "hilare", "colere", "triste",
    "effraye", "mefiant", "confus", "curieux", "fier", "timide", "blase", "somnolent",
];

impl Default for CharacterAppearance {
    fn default() -> Self {
        // Cream, not bloub's ink: an ink body would vanish on the black island.
        Self { shape: "cercle".into(), color: "creme".into(), expression: "neutre".into() }
    }
}

impl<'de> Deserialize<'de> for CharacterAppearance {
    /// Never fails: unknown values fall back to the default, and a pre-bloub
    /// `{body, eyes, color, accessory}` is migrated to the closest bloub choice.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let text = |key: &str| raw.get(key).and_then(|v| v.as_str());
        let known = |key: &str, list: &[&str]| text(key).filter(|v| list.contains(v)).map(str::to_string);
        let def = Self::default();
        let legacy_shape = match text("body") { Some("round") => Some("cercle"), Some("softSquare") => Some("squircle"), _ => None };
        let legacy_color = match text("color") { Some("neutral") => Some("creme"), Some("sky") => Some("bleu"), Some("mint") => Some("turquoise"), _ => None };
        let legacy_expression = match text("eyes") { Some("pill") => Some("neutre"), Some("dot") => Some("attentif"), Some("wide") => Some("surpris"), _ => None };
        Ok(Self {
            shape: known("shape", &CHARACTER_SHAPES).or(legacy_shape.map(str::to_string)).unwrap_or(def.shape),
            color: known("color", &CHARACTER_COLORS).or(legacy_color.map(str::to_string)).unwrap_or(def.color),
            expression: known("expression", &CHARACTER_EXPRESSIONS).or(legacy_expression.map(str::to_string)).unwrap_or(def.expression),
        })
    }
}

/// What an `active_integrations` entry names.
#[derive(Debug, PartialEq)]
pub enum Pill<'a> {
    /// "integration_github" …
    Integration(&'a str),
    /// "agent:local:<id>" — a local agent (agents.rs).
    LocalAgent(&'a str),
    /// "agent:roadeep:<id>" — a Roadeep agent.
    RoadeepAgent(&'a str),
}

pub fn parse_pill(id: &str) -> Option<Pill<'_>> {
    if let Some(rest) = id.strip_prefix("agent:local:") {
        return crate::agents::valid_id(rest).then_some(Pill::LocalAgent(rest));
    }
    if let Some(rest) = id.strip_prefix("agent:roadeep:") {
        return crate::agents::valid_id(rest).then_some(Pill::RoadeepAgent(rest));
    }
    let slug = id.strip_prefix("integration_")?;
    // `-` for catalog ids ("better-stack"); `_` from the native ids' history.
    let ok = !slug.is_empty()
        && slug.len() <= 32
        && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    ok.then_some(Pill::Integration(id))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AdminVoiceMode { #[default] Manual, Always }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub admin_voice_mode: AdminVoiceMode,
    pub sound_enabled: bool,
    pub sound_volume: f64,
    pub auto_close_interval: f64,
    /// Seconds the compact island waits, untouched, before it shrinks to a line
    /// on its edge; 0 = never (Settings → General).
    #[serde(default = "default_absence", deserialize_with = "absence_from_file")]
    pub absence_interval: f64,
    pub active_integrations: Vec<String>,
    /// Services the user added ("integration_<id>", native or catalog): what
    /// Settings lists under "my services". Absent from files written before
    /// the catalog; `load` then fills it once (`adopt_added`).
    #[serde(default)]
    pub added_integrations: Vec<String>,
    /// "primary" = the main display, "cursor" = whichever display the mouse is on.
    pub screen: String,
    pub autostart: bool,
    pub hooks_installed: bool,
    /// Explicit opt-in to reading local Codex session logs. Never uploaded.
    #[serde(default)]
    pub observe_codex: bool,
    /// Explicit opt-in to bounded, redacted local coding evidence.
    #[serde(default)]
    pub retain_coding_history: bool,
    /// Roadeep model id for new chat threads; "" = the server's default model.
    /// Defaulted explicitly so a settings.json written by an older build still loads.
    #[serde(default)]
    pub model: String,
    /// UI and reply language: "fa" or "en".
    #[serde(default = "default_language")]
    pub language: String,
    /// Agent picked in the island chat: a Roadeep agent id, "local:<id>", or none.
    #[serde(default)]
    pub chat_agent: Option<String>,
    /// Chat features sent with every message (see `chat::ToolFlags`). Web search
    /// and reasoning exclude each other; deep research implies web search.
    #[serde(default)]
    pub chat_web_search: bool,
    #[serde(default)]
    pub chat_reasoning: bool,
    /// "low" | "medium" | "high".
    #[serde(default = "default_effort")]
    pub chat_reasoning_effort: String,
    #[serde(default)]
    pub chat_deep_research: bool,
    /// Offer the enabled MCP servers' tools in the island chat (mcpc); the
    /// chat's chip turns them off for one conversation.
    #[serde(default = "default_true")]
    pub chat_tools: bool,
    /// Pill colours the user picked for Roadeep agents (id → "#RRGGBB"); the
    /// others get one derived from their id.
    #[serde(default)]
    pub agent_colors: BTreeMap<String, String>,
    /// Global shortcut that opens the island chat, e.g. "Ctrl+Alt+Space"; "" = off.
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
    /// The other global shortcuts the user changed (Settings → Shortcuts), by
    /// action id; the rest keep their default (shortcuts.rs). A malformed
    /// value reads as none instead of failing the whole file.
    #[serde(default, deserialize_with = "crate::shortcuts::lenient_bindings")]
    pub shortcuts: crate::shortcuts::Bindings,
    /// Look for a new version once a day (only in builds with the updater, see updater.rs).
    #[serde(default = "default_true")]
    pub auto_update_check: bool,
    /// Where the island is docked. Only Rust changes it (drag, `dock_set`):
    /// `save_settings` keeps the stored value whatever a window sends.
    #[serde(default, deserialize_with = "lenient_dock")]
    pub dock: Dock,
    /// The set of built-in agents already added (default_agents.rs); 0 in files
    /// written before them. Only Rust changes it: `save_settings` keeps the
    /// stored value whatever a window sends.
    #[serde(default)]
    pub default_agents_version: u32,
    /// Planner focus timer, whole minutes; `migrate` holds them in `*_BOUNDS`.
    /// A value that isn't a number reads as 0 and becomes the default there,
    /// instead of failing the whole file.
    #[serde(default = "default_focus_minutes", deserialize_with = "lenient_count")]
    pub focus_minutes: u32,
    #[serde(default = "default_break_minutes", deserialize_with = "lenient_count")]
    pub break_minutes: u32,
    #[serde(default = "default_long_break_minutes", deserialize_with = "lenient_count")]
    pub long_break_minutes: u32,
    #[serde(default = "default_rounds", deserialize_with = "lenient_count")]
    pub rounds_before_long_break: u32,
    #[serde(default = "default_true")]
    pub focus_sound: bool,
    #[serde(default = "default_true")]
    pub reminder_sound: bool,
    #[serde(default = "default_true")]
    pub habit_nudges: bool,
    /// "normal" | "calm" | "still": how restless the character's idle eyes are.
    #[serde(default = "default_eye_motion")]
    pub eye_motion: String,
    #[serde(default = "default_true")]
    pub celebrations: bool,
    /// Only visual presets, never arbitrary assets, code or URLs.
    #[serde(default)]
    pub character_appearance: CharacterAppearance,
}

/// (min, max, default) of the planner durations; the settings window offers
/// the same bounds (state.ts PLANNER_BOUNDS).
pub const FOCUS_BOUNDS: (u32, u32, u32) = (5, 120, 25);
pub const BREAK_BOUNDS: (u32, u32, u32) = (1, 30, 5);
pub const LONG_BREAK_BOUNDS: (u32, u32, u32) = (5, 60, 15);
pub const ROUNDS_BOUNDS: (u32, u32, u32) = (2, 8, 4);
pub const EYE_MOTIONS: [&str; 3] = ["normal", "calm", "still"];

fn default_focus_minutes() -> u32 {
    FOCUS_BOUNDS.2
}
fn default_break_minutes() -> u32 {
    BREAK_BOUNDS.2
}
fn default_long_break_minutes() -> u32 {
    LONG_BREAK_BOUNDS.2
}
fn default_rounds() -> u32 {
    ROUNDS_BOUNDS.2
}
fn default_eye_motion() -> String {
    "normal".into()
}

/// Any JSON number, rounded; anything else is 0, which `migrate` turns into
/// the default (0 is never a valid duration or round count).
fn lenient_count<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(match value.as_f64() {
        Some(v) if v.is_finite() && v >= 0.0 => v.round().min(u32::MAX as f64) as u32,
        _ => 0,
    })
}

/// 0 (unreadable) → the default; anything else held within the bounds.
fn sanitize_count(value: u32, (min, max, default): (u32, u32, u32)) -> u32 {
    if value == 0 { default } else { value.clamp(min, max) }
}

/// The screen edge the island hugs and where along it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Dock {
    /// "top" | "left" | "right" (see dock::Edge).
    pub edge: String,
    /// Centre along that edge, 0…1 of the display's work area.
    pub pos: f64,
    /// The display the island was dragged to (dock::Screen::id); "" = the one
    /// the `screen` preference picks.
    pub monitor: String,
}

impl Default for Dock {
    fn default() -> Self {
        Self { edge: "top".into(), pos: 0.5, monitor: String::new() }
    }
}

/// A malformed `dock` must not cost the user every other preference: it falls
/// back to the default instead of failing the whole file.
fn lenient_dock<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Dock, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Idle seconds before the island becomes a line, for new installs.
pub const DEFAULT_ABSENCE: f64 = 300.0;
/// `absenceInterval` was in settings.json long before anything used it, with
/// this default, and no window ever offered it.
const LEGACY_ABSENCE: f64 = 180.0;
/// Shorter, and the island would fold away while being read; a day at most.
const MIN_ABSENCE: f64 = 60.0;
const MAX_ABSENCE: f64 = 86_400.0;

fn default_absence() -> f64 {
    DEFAULT_ABSENCE
}

/// A 180 in a file can only be that old, never-shown default — the setting
/// offers off, 1, 2, 5, 10, 15 and 30 minutes — so it reads as today's
/// default. Saved back, the file then holds 300 and this never fires again.
/// It is done here rather than in `migrate` because the value has no other
/// mark of its age. Anything that isn't a number falls back to the default
/// instead of failing the whole file.
fn absence_from_file<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(match value.as_f64() {
        Some(v) if v == LEGACY_ABSENCE => DEFAULT_ABSENCE,
        Some(v) => v,
        None => DEFAULT_ABSENCE,
    })
}

/// 0 (off) stays; other values are held between a minute and a day.
pub fn sanitize_absence(seconds: f64) -> f64 {
    if !seconds.is_finite() || seconds < 0.0 {
        DEFAULT_ABSENCE
    } else if seconds == 0.0 {
        0.0
    } else {
        seconds.clamp(MIN_ABSENCE, MAX_ABSENCE)
    }
}

const MAX_MONITOR_ID: usize = 128;
/// More services than any catalog holds; bounds the list.
const MAX_ADDED: usize = 300;

/// A pill id of a native service or of a service in the loaded catalog.
fn known_service(id: &str) -> bool {
    crate::integrations::SERVICES.iter().any(|(pill, _)| *pill == id) || crate::catalog::get().by_pill(id).is_some()
}

/// For a settings.json from before the catalog: every native service with a
/// saved key, and every integration pill already on, counts as added — the
/// user's setup looks the same after the update. `has_key` asks the
/// Credential Manager (a stub in tests).
pub fn adopt_added(settings: &mut Settings, has_key: impl Fn(&str) -> bool) {
    let mut added: Vec<String> = Vec::new();
    for (pill, keys) in crate::integrations::SERVICES {
        if keys.iter().any(|k| has_key(k)) {
            added.push((*pill).to_string());
        }
    }
    for id in &settings.active_integrations {
        if matches!(parse_pill(id), Some(Pill::Integration(_))) && !added.contains(id) {
            added.push(id.clone());
        }
    }
    settings.added_integrations = added;
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "fa".into()
}

fn default_effort() -> String {
    "low".into()
}

fn default_shortcut() -> String {
    crate::shortcut::DEFAULT_ACCELERATOR.into()
}

pub const REASONING_EFFORTS: [&str; 3] = ["low", "medium", "high"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            sound_enabled: true,
            sound_volume: 0.12,
            auto_close_interval: 15.0,
            absence_interval: DEFAULT_ABSENCE,
            // Built-in agents, not services: a service only shows once it is set up.
            active_integrations: crate::default_agents::island_pills(),
            added_integrations: Vec::new(),
            screen: "primary".into(),
            autostart: false,
            hooks_installed: false,
            observe_codex: false,
            retain_coding_history: false,
            model: String::new(),
            language: default_language(),
            chat_agent: None,
            chat_web_search: false,
            chat_reasoning: false,
            chat_reasoning_effort: default_effort(),
            chat_deep_research: false,
            chat_tools: true,
            agent_colors: BTreeMap::new(),
            shortcut: default_shortcut(),
            shortcuts: crate::shortcuts::Bindings::new(),
            auto_update_check: true,
            dock: Dock::default(),
            default_agents_version: 0,
            focus_minutes: FOCUS_BOUNDS.2,
            break_minutes: BREAK_BOUNDS.2,
            long_break_minutes: LONG_BREAK_BOUNDS.2,
            rounds_before_long_break: ROUNDS_BOUNDS.2,
            focus_sound: true,
            reminder_sound: true,
            habit_nudges: true,
            eye_motion: default_eye_motion(),
            celebrations: true,
            character_appearance: CharacterAppearance::default(),
            admin_voice_mode: AdminVoiceMode::Manual,
        }
    }
}

/// Anthropic model ids from the pre-Roadeep build mean nothing to Roadeep.
fn is_legacy_model(model: &str) -> bool {
    model.trim().starts_with("claude-")
}

/// Brings values written by an older build (or sent by a stale window) into
/// range. Returns true when something changed.
pub fn migrate(settings: &mut Settings) -> bool {
    let mut changed = false;
    if is_legacy_model(&settings.model) {
        settings.model = String::new();
        changed = true;
    }
    if settings.language != "fa" && settings.language != "en" {
        settings.language = default_language();
        changed = true;
    }
    if settings.chat_agent.as_deref().is_some_and(|a| a.trim().is_empty()) {
        settings.chat_agent = None;
        changed = true;
    }
    if !REASONING_EFFORTS.contains(&settings.chat_reasoning_effort.as_str()) {
        settings.chat_reasoning_effort = default_effort();
        changed = true;
    }
    // Deep research searches the web and cannot reason; web search cannot either.
    if settings.chat_deep_research && (!settings.chat_web_search || settings.chat_reasoning) {
        settings.chat_web_search = true;
        settings.chat_reasoning = false;
        changed = true;
    }
    if settings.chat_web_search && settings.chat_reasoning {
        settings.chat_reasoning = false;
        changed = true;
    }
    // Pills: known shapes only, each once, within the shared limit.
    let mut pills: Vec<String> = Vec::new();
    for id in &settings.active_integrations {
        if parse_pill(id).is_some() && !pills.contains(id) && pills.len() < MAX_ACTIVE_PILLS {
            pills.push(id.clone());
        }
    }
    if pills != settings.active_integrations {
        settings.active_integrations = pills;
        changed = true;
    }
    // Added services: known ones only (native or in this build's catalog), each once.
    let mut added: Vec<String> = Vec::new();
    for id in &settings.added_integrations {
        if known_service(id) && !added.contains(id) && added.len() < MAX_ADDED {
            added.push(id.clone());
        }
    }
    if added != settings.added_integrations {
        settings.added_integrations = added;
        changed = true;
    }
    let colors: BTreeMap<String, String> = settings
        .agent_colors
        .iter()
        .filter(|(id, _)| crate::agents::valid_id(id))
        .filter_map(|(id, c)| crate::agents::normalize_color(c).map(|c| (id.clone(), c)))
        .take(MAX_AGENT_COLORS)
        .collect();
    if colors != settings.agent_colors {
        settings.agent_colors = colors;
        changed = true;
    }
    // "" (off) or a valid accelerator, one spelling; anything else → the default.
    let shortcut = crate::shortcut::sanitize(&settings.shortcut);
    if shortcut != settings.shortcut {
        settings.shortcut = shortcut;
        changed = true;
    }
    // Known actions only (not the chat: that is `shortcut`), keys in one spelling.
    let shortcuts = crate::shortcuts::sanitize(&settings.shortcuts);
    if shortcuts != settings.shortcuts {
        settings.shortcuts = shortcuts;
        changed = true;
    }
    let absence = sanitize_absence(settings.absence_interval);
    if absence != settings.absence_interval {
        settings.absence_interval = absence;
        changed = true;
    }
    let dock = sanitize_dock(&settings.dock);
    if dock != settings.dock {
        settings.dock = dock;
        changed = true;
    }
    for (value, bounds) in [
        (&mut settings.focus_minutes, FOCUS_BOUNDS),
        (&mut settings.break_minutes, BREAK_BOUNDS),
        (&mut settings.long_break_minutes, LONG_BREAK_BOUNDS),
        (&mut settings.rounds_before_long_break, ROUNDS_BOUNDS),
    ] {
        let clean = sanitize_count(*value, bounds);
        if clean != *value {
            *value = clean;
            changed = true;
        }
    }
    if !EYE_MOTIONS.contains(&settings.eye_motion.as_str()) {
        settings.eye_motion = default_eye_motion();
        changed = true;
    }
    changed
}

/// A known edge, a finite position within 0…1, a plausible monitor id.
pub fn sanitize_dock(dock: &Dock) -> Dock {
    let edge = match crate::dock::Edge::parse(&dock.edge) {
        Some(e) => e.as_str().to_string(),
        None => Dock::default().edge,
    };
    let pos = if dock.pos.is_finite() { dock.pos.clamp(0.0, 1.0) } else { 0.5 };
    let monitor = if dock.monitor.len() <= MAX_MONITOR_ID && !dock.monitor.chars().any(char::is_control) {
        dock.monitor.clone()
    } else {
        String::new()
    };
    Dock { edge, pos, monitor }
}

/// %APPDATA%\Roadeep
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Roadeep")
}

/// %LOCALAPPDATA%\com.roadeep.desktop — where roadeep-hook.exe and the log live.
pub fn local_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    // The app's own local data folder (shared with the WebView profile), never
    // the install folder %LOCALAPPDATA%\Roadeep that NSIS manages.
    base.join(crate::migrate::IDENTIFIER)
}

pub fn hook_exe_path() -> PathBuf {
    local_dir().join("bin").join("roadeep-hook.exe")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// The settings in a file, and whether it already has `addedIntegrations`.
/// `None` when it isn't valid settings JSON.
fn parse_file(bytes: &[u8]) -> Option<(Settings, bool)> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if !matches!(value.get("adminVoiceMode").and_then(|v| v.as_str()), Some("manual" | "always")) { value.as_object_mut()?.insert("adminVoiceMode".into(), serde_json::json!("manual")); }
    let has_added = value.get("addedIntegrations").is_some();
    Some((serde_json::from_value(value).ok()?, has_added))
}

pub fn load() -> Settings {
    // `writable`: the file parsed, or there is none yet. One that is there but
    // cannot be read or parsed is never written over on load.
    let (mut settings, adopt, mut changed, writable) = match std::fs::read(settings_path()) {
        Ok(bytes) => match parse_file(&bytes) {
            // Written once, so the adoption never runs again.
            Some((settings, has_added)) => (settings, !has_added, !has_added, true),
            None => (Settings::default(), true, false, false),
        },
        // First launch: written below once the built-in agents are in.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Settings::default(), true, false, true),
        Err(_) => (Settings::default(), true, false, false),
    };
    if adopt {
        adopt_added(&mut settings, crate::secrets::present);
        crate::log::line(format!("settings: {} services marked as added", settings.added_integrations.len()));
    }
    changed |= migrate(&mut settings);
    // Without the stored version it is unknown which built-in agents the user
    // already had (and maybe deleted), so they are only added with a good file.
    if writable {
        match crate::default_agents::apply(&mut settings, &crate::agents::store_path(), crate::secrets::present) {
            Ok(Some(done)) => {
                crate::log::line(format!(
                    "settings: built-in agents v{}: {} added, {} already there, {} over the limit; \
                     {} keyless services removed, {} agent pills added",
                    settings.default_agents_version,
                    done.seeded.added,
                    done.seeded.present,
                    done.seeded.over_limit,
                    done.services_removed,
                    done.pills_added,
                ));
                changed = true;
            }
            Ok(None) => {}
            Err(why) => crate::log::line(format!("settings: built-in agents not added yet ({why}); retrying next launch")),
        }
    }
    if changed && writable {
        if let Err(err) = save(&settings) {
            crate::log::line(format!("settings: could not save migrated settings: {err}"));
        }
    }
    settings
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(settings_path(), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_is_backward_compatible_tolerant_and_round_trips() {
        let mut json = serde_json::to_value(Settings::default()).unwrap();
        json.as_object_mut().unwrap().remove("characterAppearance");
        let old: Settings = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(old.character_appearance, CharacterAppearance::default());
        json["characterAppearance"] = serde_json::json!({"shape":"nuage","color":"violet","expression":"curieux"});
        let current: Settings = serde_json::from_value(json).unwrap();
        let saved = serde_json::to_value(&current).unwrap();
        assert_eq!(saved["characterAppearance"], serde_json::json!({"shape":"nuage","color":"violet","expression":"curieux"}));
        assert_eq!(serde_json::from_value::<Settings>(saved).unwrap().character_appearance, current.character_appearance);
        for bad in [serde_json::Value::Null, serde_json::json!([]), serde_json::json!({"shape":7,"color":"url(x)","expression":"<script>"})] {
            assert_eq!(serde_json::from_value::<CharacterAppearance>(bad).unwrap(), CharacterAppearance::default());
        }
    }

    #[test]
    fn appearance_migrates_pre_bloub_values() {
        let migrated: CharacterAppearance = serde_json::from_value(serde_json::json!(
            {"body":"softSquare","eyes":"wide","color":"mint","accessory":"sprout"})).unwrap();
        assert_eq!(migrated, CharacterAppearance { shape: "squircle".into(), color: "turquoise".into(), expression: "surpris".into() });
        let original: CharacterAppearance = serde_json::from_value(serde_json::json!(
            {"body":"round","eyes":"pill","color":"neutral","accessory":"none"})).unwrap();
        assert_eq!(original, CharacterAppearance::default());
        // A new value wins over a stale legacy one; the legacy keys are not written back.
        let mixed: CharacterAppearance = serde_json::from_value(serde_json::json!({"shape":"goutte","body":"softSquare","color":"sky"})).unwrap();
        assert_eq!((mixed.shape.as_str(), mixed.color.as_str()), ("goutte", "bleu"));
        let saved = serde_json::to_value(&mixed).unwrap();
        assert!(saved.get("body").is_none() && saved.get("accessory").is_none());
    }

    #[test]
    fn old_settings_file_loads_with_defaults_and_migrates() {
        let old = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false,
            "model":"claude-opus-5"}"#;
        let mut s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.language, "fa");
        assert!(migrate(&mut s));
        assert_eq!(s.model, "");
        assert!(!migrate(&mut s), "migration is idempotent");
    }

    #[test]
    fn idle_line_delay_reads_the_old_default_as_five_minutes() {
        let file = |v: &str| {
            format!(
                r#"{{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":{v},
                "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}}"#
            )
        };
        assert_eq!(Settings::default().absence_interval, 300.0, "new installs");
        // The never-shown 180 → 300; written back, it stays 300.
        let mut s: Settings = serde_json::from_str(&file("180")).unwrap();
        assert_eq!(s.absence_interval, 300.0);
        assert!(!migrate(&mut s));
        let again: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(again.absence_interval, 300.0);
        // Choices are kept, 0 included (off).
        for v in [0.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0] {
            let mut s: Settings = serde_json::from_str(&file(&v.to_string())).unwrap();
            assert!(!migrate(&mut s), "{v}");
            assert_eq!(s.absence_interval, v);
        }
        // Out of range or not a number.
        for (raw, want) in [("-5", 300.0), ("10", 60.0), ("1e9", 86_400.0), ("\"x\"", 300.0), ("null", 300.0)] {
            let mut s: Settings = serde_json::from_str(&file(raw)).unwrap();
            migrate(&mut s);
            assert_eq!(s.absence_interval, want, "{raw}");
        }
        // Missing entirely.
        let none: Settings = serde_json::from_str(r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#).unwrap();
        assert_eq!(none.absence_interval, 300.0);
    }

    #[test]
    fn model_field_may_be_missing_entirely() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let s: Settings = serde_json::from_str(older).unwrap();
        assert_eq!(s.model, "");
        assert_eq!(s.language, "fa");
    }

    #[test]
    fn roadeep_models_and_valid_languages_are_kept() {
        let mut s = Settings { model: "openrouter-openai-gpt-5".into(), language: "en".into(), ..Default::default() };
        assert!(!migrate(&mut s));
        assert_eq!(s.model, "openrouter-openai-gpt-5");
        assert_eq!(s.language, "en");

        s.language = "de".into();
        assert!(migrate(&mut s));
        assert_eq!(s.language, "fa");
    }

    #[test]
    fn chat_options_default_and_stay_consistent() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let mut s: Settings = serde_json::from_str(older).unwrap();
        assert_eq!(s.chat_agent, None);
        assert!(!s.chat_web_search && !s.chat_reasoning && !s.chat_deep_research);
        assert!(s.chat_tools, "MCP tools are on unless turned off");
        assert_eq!(s.chat_reasoning_effort, "low");
        assert!(!migrate(&mut s));

        s.chat_agent = Some(" ".into());
        s.chat_reasoning_effort = "xhigh".into();
        s.chat_web_search = true;
        s.chat_reasoning = true;
        assert!(migrate(&mut s));
        assert_eq!(s.chat_agent, None);
        assert_eq!(s.chat_reasoning_effort, "low");
        assert!(s.chat_web_search && !s.chat_reasoning, "web search wins over reasoning");

        s.chat_web_search = false;
        s.chat_reasoning = true;
        s.chat_deep_research = true;
        assert!(migrate(&mut s));
        assert!(s.chat_deep_research && s.chat_web_search && !s.chat_reasoning);
        assert!(!migrate(&mut s));
    }

    #[test]
    fn pill_ids() {
        assert_eq!(parse_pill("integration_github"), Some(Pill::Integration("integration_github")));
        assert_eq!(parse_pill("agent:local:3f1c2b9e-0000-4000-8000-123456789abc"), Some(Pill::LocalAgent("3f1c2b9e-0000-4000-8000-123456789abc")));
        assert_eq!(parse_pill("agent:roadeep:a-1"), Some(Pill::RoadeepAgent("a-1")));
        for bad in ["", "github", "integration_", "integration_Git Hub", "agent:local:", "agent:local:../x", "agent:roadeep:a b", "agent:other:x", "local:abc"] {
            assert_eq!(parse_pill(bad), None, "{bad}");
        }
    }

    #[test]
    fn pills_are_cleaned_and_share_the_limit() {
        let mut s = Settings {
            active_integrations: vec![
                "integration_github".into(),
                "agent:local:abc".into(),
                "integration_github".into(),
                "agent:roadeep:../../x".into(),
                "agent:roadeep:r-1".into(),
                "integration_stripe".into(),
                "integration_vercel".into(),
            ],
            ..Default::default()
        };
        assert!(migrate(&mut s));
        assert_eq!(s.active_integrations, ["integration_github", "agent:local:abc", "agent:roadeep:r-1", "integration_stripe"]);
        assert!(!migrate(&mut s));
    }

    #[test]
    fn agent_colours_load_default_and_are_validated() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let mut s: Settings = serde_json::from_str(older).unwrap();
        assert!(s.agent_colors.is_empty());
        s.agent_colors.insert("r-1".into(), "#7c5cff".into());
        s.agent_colors.insert("r-2".into(), "red".into());
        s.agent_colors.insert("../x".into(), "#FFFFFF".into());
        assert!(migrate(&mut s));
        assert_eq!(s.agent_colors.len(), 1);
        assert_eq!(s.agent_colors["r-1"], "#7C5CFF");
    }

    #[test]
    fn dock_defaults_to_todays_top_centre() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let mut s: Settings = serde_json::from_str(older).unwrap();
        assert_eq!(s.dock, Dock { edge: "top".into(), pos: 0.5, monitor: String::new() });
        assert!(!migrate(&mut s));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["dock"], serde_json::json!({"edge": "top", "pos": 0.5, "monitor": ""}));
    }

    #[test]
    fn dock_is_validated_and_a_broken_one_costs_nothing_else() {
        let partial = r#"{"soundEnabled":false,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false,
            "dock":{"edge":"left"}}"#;
        let s: Settings = serde_json::from_str(partial).unwrap();
        assert_eq!(s.dock, Dock { edge: "left".into(), pos: 0.5, monitor: String::new() });

        let broken = r#"{"soundEnabled":false,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false,
            "dock":{"edge":5,"pos":"x"}}"#;
        let s: Settings = serde_json::from_str(broken).unwrap();
        assert!(!s.sound_enabled, "the other preferences survive");
        assert_eq!(s.dock, Dock::default());

        let mut s = Settings {
            dock: Dock { edge: "bottom".into(), pos: 3.0, monitor: "x".repeat(MAX_MONITOR_ID + 1) },
            ..Default::default()
        };
        assert!(migrate(&mut s));
        assert_eq!(s.dock, Dock { edge: "top".into(), pos: 1.0, monitor: String::new() });
        s.dock = Dock { edge: "right".into(), pos: -0.2, monitor: r"\\.\DISPLAY2".into() };
        assert!(migrate(&mut s));
        assert_eq!(s.dock.edge, "right");
        assert_eq!(s.dock.pos, 0.0);
        assert!(!migrate(&mut s));
    }

    #[test]
    fn catalog_pill_ids_may_hold_a_dash() {
        assert_eq!(parse_pill("integration_better-stack"), Some(Pill::Integration("integration_better-stack")));
        assert_eq!(parse_pill("integration_a/b"), None);
    }

    #[test]
    fn added_services_are_adopted_from_keys_and_pills_once() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":["integration_github","agent:local:abc","integration_sentry"],
            "screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let (mut s, has_added) = parse_file(older.as_bytes()).unwrap();
        assert!(!has_added);
        assert!(s.added_integrations.is_empty());
        let saved = ["stripe-api-key", "n8n-url", "github-token"];
        adopt_added(&mut s, |k| saved.contains(&k));
        // Natives with a key (in service order), then pills already on; agents aren't services.
        assert_eq!(s.added_integrations, ["integration_stripe", "integration_github", "integration_n8n", "integration_sentry"]);

        let json = serde_json::to_string(&s).unwrap();
        let (again, has_added) = parse_file(json.as_bytes()).unwrap();
        assert!(has_added, "written once, the field is there next time");
        assert_eq!(again.added_integrations, s.added_integrations);

        assert!(parse_file(b"not json").is_none());
        let (_, has_added) = parse_file(br#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"addedIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#).unwrap();
        assert!(has_added, "an empty list is a choice, not a missing field");
    }

    #[test]
    fn unknown_added_services_are_dropped() {
        let mut s = Settings {
            added_integrations: vec![
                "integration_github".into(),
                "integration_github".into(),
                "integration_not-a-service".into(),
                "agent:local:abc".into(),
                "integration_linear".into(),
            ],
            ..Default::default()
        };
        assert!(migrate(&mut s));
        assert_eq!(s.added_integrations, ["integration_github", "integration_linear"]);
        assert!(!migrate(&mut s));
    }

    #[test]
    fn planner_settings_default_and_are_held_in_range() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":300,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let mut s: Settings = serde_json::from_str(older).unwrap();
        assert_eq!((s.focus_minutes, s.break_minutes, s.long_break_minutes, s.rounds_before_long_break), (25, 5, 15, 4));
        assert!(s.focus_sound && s.reminder_sound && s.habit_nudges && s.celebrations);
        assert_eq!(s.eye_motion, "normal");
        assert!(!migrate(&mut s));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["focusMinutes"], 25);
        assert_eq!(json["roundsBeforeLongBreak"], 4);
        assert_eq!(json["eyeMotion"], "normal");

        // A stale or broken window: floats round, junk falls back, the rest is clamped.
        let odd = r#"{"soundEnabled":false,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":300,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false,
            "focusMinutes":49.6,"breakMinutes":"x","longBreakMinutes":500,"roundsBeforeLongBreak":-3,
            "eyeMotion":"wild","celebrations":false}"#;
        let mut s: Settings = serde_json::from_str(odd).unwrap();
        assert!(!s.sound_enabled, "the other preferences survive");
        assert!(migrate(&mut s));
        assert_eq!((s.focus_minutes, s.break_minutes, s.long_break_minutes, s.rounds_before_long_break), (50, 5, 60, 4));
        assert_eq!(s.eye_motion, "normal");
        assert!(!s.celebrations);
        assert!(!migrate(&mut s));

        for (motion, keep) in [("calm", true), ("still", true), ("", false), ("Normal", false)] {
            s.eye_motion = motion.into();
            assert_eq!(migrate(&mut s), !keep, "{motion}");
        }
        s.focus_minutes = 1;
        s.break_minutes = 99;
        assert!(migrate(&mut s));
        assert_eq!((s.focus_minutes, s.break_minutes), (5, 30));
    }

    #[test]
    fn shortcut_defaults_and_is_validated() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let mut s: Settings = serde_json::from_str(older).unwrap();
        assert_eq!(s.shortcut, crate::shortcut::DEFAULT_ACCELERATOR);
        assert!(!migrate(&mut s));

        s.shortcut = String::new();
        assert!(!migrate(&mut s), "empty means off and is kept");

        s.shortcut = "alt+ctrl+keyk".into();
        assert!(migrate(&mut s));
        assert_eq!(s.shortcut, "Ctrl+Alt+KeyK");

        for bad in ["KeyK", "Shift+KeyK", "Ctrl+Banana", "Ctrl+"] {
            s.shortcut = bad.into();
            assert!(migrate(&mut s), "{bad}");
            assert_eq!(s.shortcut, crate::shortcut::DEFAULT_ACCELERATOR, "{bad}");
        }
    }

    #[test]
    fn the_other_shortcuts_load_from_any_file_and_are_cleaned() {
        use crate::shortcuts::Binding;
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":300,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let mut s: Settings = serde_json::from_str(older).unwrap();
        assert!(s.shortcuts.is_empty(), "a file from before them: every action keeps its default");
        assert!(!migrate(&mut s));

        // A malformed value costs the shortcuts, never the other preferences.
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["soundEnabled"] = serde_json::json!(false);
        for wrong in [serde_json::json!("Ctrl+Alt+A"), serde_json::json!([1]), serde_json::Value::Null] {
            value["shortcuts"] = wrong;
            let loaded: Settings = serde_json::from_value(value.clone()).unwrap();
            assert!(loaded.shortcuts.is_empty());
            assert!(!loaded.sound_enabled);
        }

        value["shortcuts"] = serde_json::json!({
            "goToAlert": {"keys": "alt+ctrl+g", "enabled": true},
            "openChat": {"keys": "Ctrl+Alt+KeyQ", "enabled": true},
            "nope": {"keys": "Ctrl+Alt+KeyX", "enabled": true},
            "muteToggle": {"keys": "Shift+KeyM", "enabled": true}
        });
        let mut s: Settings = serde_json::from_value(value).unwrap();
        assert!(migrate(&mut s));
        let kept: Vec<(&str, &Binding)> = s.shortcuts.iter().map(|(id, b)| (id.as_str(), b)).collect();
        assert_eq!(kept, [("goToAlert", &Binding { keys: "Ctrl+Alt+KeyG".into(), enabled: true })]);
        assert!(!migrate(&mut s), "idempotent");
        let saved = serde_json::to_value(&s).unwrap();
        assert_eq!(saved["shortcuts"], serde_json::json!({"goToAlert": {"keys": "Ctrl+Alt+KeyG", "enabled": true}}));
    }
    #[test]
    fn voice_mode_defaults_and_invalid_saved_value_fails_safe() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value.as_object_mut().unwrap().remove("adminVoiceMode");
        assert_eq!(serde_json::from_value::<Settings>(value.clone()).unwrap().admin_voice_mode, AdminVoiceMode::Manual);
        value["adminVoiceMode"] = serde_json::json!("unsafe");
        assert!(serde_json::from_value::<Settings>(value.clone()).is_err(), "updates must reject unknown mode");
        assert_eq!(parse_file(&serde_json::to_vec(&value).unwrap()).unwrap().0.admin_voice_mode, AdminVoiceMode::Manual);
        value["adminVoiceMode"] = serde_json::json!("always");
        assert_eq!(parse_file(&serde_json::to_vec(&value).unwrap()).unwrap().0.admin_voice_mode, AdminVoiceMode::Always);
    }

}
