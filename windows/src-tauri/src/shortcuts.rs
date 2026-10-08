// Global keyboard shortcuts: the customisable set behind Settings → Shortcuts,
// adapted from upstream's shortcuts.rs (the Mac's HotKeyCenter.swift and
// ShortcutLogic.swift).
//
// The chat shortcut is the first of them and the one Roadeep always had. Its
// keys stay in settings.shortcut ("" = off; shortcut.rs parses and spells
// them) and a press still reaches the island as tray → "chat", which opens the
// chat or puts it away again. The other actions are stored in
// settings.shortcuts, only the ones the user changed, and a press reaches the
// island as a `shortcut` event carrying the action id (src/island/shortcuts.ts).
//
// Those other actions are off until they are turned on in Settings. A global
// shortcut takes its keys away from every other app (Ctrl+Alt+←/→ move editors
// between groups in VS Code, Ctrl+Alt+S opens a JetBrains IDE's settings), so
// an update never takes one without asking.
//
// Ctrl+Alt is AltGr on Windows: AltGr+E types € on most European layouts and
// AltGr+A types ą on Polish. A Ctrl+Alt combination that types a character on
// one of the installed layouts is left unregistered and flagged in Settings,
// or that character could no longer be typed. The chat shortcut is exempt: it
// predates the check, and an update must not take a working shortcut away.
//
// Registration only happens on the main thread. The plugin runs every register
// and unregister there and blocks until it is done, so a lock held while
// waiting for it must never be wanted by the main thread. Doing all the work
// there, with the press handler only reading ROUTES, rules that out.

pub mod session_window;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{Code, GlobalShortcut, Modifiers, Shortcut};

use crate::errors;
use crate::island::WINDOW_LABEL;
use crate::log;

/// The chat shortcut's action id. Its keys are `settings.shortcut`.
pub const CHAT: &str = "openChat";

/// How each action went, sent to every window after a change.
pub const STATUS_EVENT: &str = "shortcuts-status";

/// The longest a recording in Settings keeps the shortcuts released: a window
/// closed mid-recording must not leave them off.
const SUSPEND_MAX: Duration = Duration::from_secs(30);

/// One global action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDef {
    pub id: &'static str,
    /// In shortcut.rs's spelling.
    pub default_keys: &'static str,
    pub enabled_by_default: bool,
}

const fn action(id: &'static str, keys: &'static str, on: bool) -> ActionDef {
    ActionDef { id, default_keys: keys, enabled_by_default: on }
}

/// Settings order; when two actions share a combination the first keeps it.
/// The ids are stored in settings.json: never rename one. The same table is in
/// src/core/bridge-shortcuts.ts, and a test there keeps the two in step.
pub const ACTIONS: &[ActionDef] = &[
    action("openChat", "Ctrl+Alt+KeyR", true),
    action("toggleIsland", "Ctrl+Alt+KeyN", false),
    action("goToAlert", "Ctrl+Alt+KeyA", false),
    action("jumpToTerminal", "Ctrl+Alt+KeyT", false),
    action("nextPill", "Ctrl+Alt+ArrowRight", false),
    action("prevPill", "Ctrl+Alt+ArrowLeft", false),
    action("muteToggle", "Ctrl+Alt+KeyS", false),
];

pub fn find(id: &str) -> Option<&'static ActionDef> {
    ACTIONS.iter().find(|a| a.id == id)
}

// ── Bindings ──────────────────────────────────────────────────────────────────

/// What the user chose for one action: its keys ("" for none) and whether it is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Binding {
    pub keys: String,
    pub enabled: bool,
}

impl Default for Binding {
    fn default() -> Self {
        Self { keys: String::new(), enabled: true }
    }
}

/// `settings.shortcuts`: the actions the user changed, by id.
pub type Bindings = BTreeMap<String, Binding>;

/// Reads `settings.shortcuts` without ever failing the whole file: an entry
/// that isn't a binding is skipped, and anything but an object reads as none.
pub fn lenient_bindings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Bindings, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    let Some(map) = value.as_object() else { return Ok(Bindings::new()) };
    Ok(map
        .iter()
        .filter_map(|(id, entry)| {
            let entry = entry.as_object()?;
            let keys = match entry.get("keys") {
                None => String::new(),
                Some(keys) => keys.as_str()?.to_string(),
            };
            let enabled = entry.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
            Some((id.clone(), Binding { keys, enabled }))
        })
        .collect())
}

/// What settings.rs keeps of `settings.shortcuts`: known actions other than
/// the chat (whose keys are `settings.shortcut`), with their keys in one
/// spelling. An entry whose keys are not a shortcut is dropped, so the action
/// falls back to its default.
pub fn sanitize(stored: &Bindings) -> Bindings {
    stored
        .iter()
        .filter_map(|(id, binding)| {
            let def = find(id).filter(|d| d.id != CHAT)?;
            let keys = if binding.keys.trim().is_empty() {
                String::new()
            } else {
                crate::shortcut::canonical(&crate::shortcut::parse(&binding.keys).ok()?)
            };
            Some((def.id.to_string(), Binding { keys, enabled: binding.enabled }))
        })
        .collect()
}

/// The binding in force for `def`: the chat's from `settings.shortcut`
/// (`chat`), the others as stored, or their default.
pub fn effective(def: &ActionDef, chat: &str, stored: &Bindings) -> Binding {
    if def.id == CHAT {
        let keys = chat.trim().to_string();
        let enabled = !keys.is_empty();
        return Binding { keys, enabled };
    }
    stored.get(def.id).cloned().unwrap_or_else(|| Binding {
        keys: def.default_keys.to_string(),
        enabled: def.enabled_by_default,
    })
}

// ── Status shown in Settings ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Registered: a press reaches the island.
    Active,
    /// Turned off, or no keys.
    Off,
    /// Another app holds the combination.
    InUse,
    /// An action earlier in the list already has the combination.
    Duplicate,
    /// Not a combination Windows can register.
    Invalid,
    /// Ctrl+Alt+key types a character on an installed keyboard layout.
    TypesCharacter,
    /// The shortcut plugin isn't running.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionStatus {
    pub id: &'static str,
    /// The keys this is about ("" for none).
    pub keys: String,
    pub status: Status,
    /// What a `TypesCharacter` combination types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed: Option<String>,
    /// An `errors` code, for `InUse` and `Invalid`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub actions: Vec<ActionStatus>,
    /// Settings is recording a combination: nothing is registered meanwhile.
    pub suspended: bool,
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

/// True when Windows reads the combination as AltGr: Ctrl and Alt held, no
/// Windows key.
pub fn is_altgr_like(shortcut: &Shortcut) -> bool {
    shortcut.mods.contains(Modifiers::CONTROL | Modifiers::ALT) && !shortcut.mods.intersects(Modifiers::SUPER)
}

/// The Windows virtual-key code registered for `code`, for the keys that can
/// type a character. The others (arrows, F-keys…) never do: `None`.
pub fn character_vk(code: Code) -> Option<u16> {
    use Code::*;
    let letters = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP,
        KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    if let Some(i) = letters.iter().position(|c| *c == code) {
        return Some(0x41 + i as u16);
    }
    let digits = [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];
    if let Some(i) = digits.iter().position(|c| *c == code) {
        return Some(0x30 + i as u16);
    }
    Some(match code {
        Space => 0x20,
        Semicolon => 0xBA,
        Equal => 0xBB,
        Comma => 0xBC,
        Minus => 0xBD,
        Period => 0xBE,
        Slash => 0xBF,
        Backquote => 0xC0,
        BracketLeft => 0xDB,
        Backslash => 0xDC,
        BracketRight => 0xDD,
        Quote => 0xDE,
        IntlBackslash => 0xE2,
        _ => return None,
    })
}

/// What each action should do before the OS is asked anything: the shortcut
/// to register, or the reason it won't be. `types` says what a combination
/// types, if anything (`typed_character`); the chat is never asked.
pub fn plan(
    chat: &str,
    stored: &Bindings,
    types: impl Fn(&Shortcut) -> Option<String>,
) -> Vec<(&'static ActionDef, Result<Shortcut, ActionStatus>)> {
    let mut taken: Vec<u32> = Vec::new();
    ACTIONS
        .iter()
        .map(|def| {
            let binding = effective(def, chat, stored);
            let refuse = |status: Status, typed: Option<String>, error: Option<String>| -> Result<Shortcut, ActionStatus> {
                Err(ActionStatus { id: def.id, keys: binding.keys.clone(), status, typed, error })
            };
            let outcome = if !binding.enabled || binding.keys.trim().is_empty() {
                refuse(Status::Off, None, None)
            } else {
                match crate::shortcut::parse(&binding.keys) {
                    Err(code) => refuse(Status::Invalid, None, Some(code)),
                    Ok(shortcut) => {
                        let typed = if def.id == CHAT { None } else { types(&shortcut) };
                        if typed.is_some() {
                            refuse(Status::TypesCharacter, typed, None)
                        } else if taken.contains(&shortcut.id()) {
                            refuse(Status::Duplicate, None, None)
                        } else {
                            taken.push(shortcut.id());
                            Ok(shortcut)
                        }
                    }
                }
            };
            (def, outcome)
        })
        .collect()
}

// ── Registration ──────────────────────────────────────────────────────────────

/// The OS side of registration: the plugin in the app, a fake in the tests.
trait Keys {
    fn register_key(&self, shortcut: Shortcut) -> Result<(), String>;
    fn unregister_key(&self, shortcut: Shortcut) -> Result<(), String>;
}

impl<R: Runtime> Keys for GlobalShortcut<R> {
    fn register_key(&self, shortcut: Shortcut) -> Result<(), String> {
        self.register(shortcut).map_err(|e| e.to_string())
    }
    fn unregister_key(&self, shortcut: Shortcut) -> Result<(), String> {
        self.unregister(shortcut).map_err(|e| e.to_string())
    }
}

/// What the settings ask for, cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Wanted {
    chat: String,
    stored: Bindings,
}

struct Engine {
    /// What the settings last asked for.
    wanted: Option<Wanted>,
    /// What `held` and `report` were built from; None makes the next apply run.
    applied: Option<Wanted>,
    /// The combinations we hold, and the action each one runs.
    held: Vec<(Shortcut, &'static str)>,
    report: Vec<ActionStatus>,
    /// The recording that has the shortcuts released, by number.
    suspended: Option<u64>,
    recordings: u64,
}

impl Engine {
    const fn new() -> Self {
        Self { wanted: None, applied: None, held: Vec::new(), report: Vec::new(), suspended: None, recordings: 0 }
    }

    /// Holds what `wanted` asks for: lets go of what it no longer wants, keeps
    /// what it still does, and asks the OS for the rest.
    fn sync(&mut self, keys: Option<&dyn Keys>, wanted: Wanted, types: impl Fn(&Shortcut) -> Option<String>) {
        let plan = plan(&wanted.chat, &wanted.stored, types);
        let wanted_ids: Vec<u32> = plan.iter().filter_map(|(_, o)| o.as_ref().ok().map(Shortcut::id)).collect();
        let mut kept: Vec<Shortcut> = Vec::new();
        for (shortcut, _) in std::mem::take(&mut self.held) {
            if wanted_ids.contains(&shortcut.id()) {
                kept.push(shortcut);
            } else {
                let_go(keys, shortcut);
            }
        }
        let mut report = Vec::with_capacity(plan.len());
        for (def, outcome) in plan {
            let shortcut = match outcome {
                Ok(shortcut) => shortcut,
                Err(status) => {
                    report.push(status);
                    continue;
                }
            };
            let entry = |status: Status, error: Option<String>| ActionStatus {
                id: def.id,
                keys: crate::shortcut::canonical(&shortcut),
                status,
                typed: None,
                error,
            };
            let result = if kept.iter().any(|s| s.id() == shortcut.id()) {
                Ok(())
            } else {
                match keys {
                    Some(keys) => keys.register_key(shortcut).map_err(Some),
                    None => Err(None),
                }
            };
            report.push(match result {
                Ok(()) => {
                    self.held.push((shortcut, def.id));
                    entry(Status::Active, None)
                }
                Err(Some(err)) => {
                    // Most often another app already holds the combination.
                    log::line(format!("shortcuts: {} not registered: {err}", def.id));
                    entry(Status::InUse, Some(errors::coded(errors::SHORTCUT_TAKEN, &[&err])))
                }
                Err(None) => entry(Status::Unavailable, None),
            });
        }
        self.report = report;
        self.applied = Some(wanted);
    }

    /// Lets go of every combination; the next apply registers them again.
    fn release_all(&mut self, keys: Option<&dyn Keys>) {
        for (shortcut, _) in std::mem::take(&mut self.held) {
            let_go(keys, shortcut);
        }
        self.applied = None;
    }

    fn routes(&self) -> Vec<(u32, &'static str)> {
        self.held.iter().map(|(s, action)| (s.id(), *action)).collect()
    }

    fn report(&self) -> Report {
        Report { actions: self.report.clone(), suspended: self.suspended.is_some() }
    }
}

fn let_go(keys: Option<&dyn Keys>, shortcut: Shortcut) {
    if let Some(Err(err)) = keys.map(|k| k.unregister_key(shortcut)) {
        log::line(format!("shortcuts: could not unregister {}: {err}", crate::shortcut::canonical(&shortcut)));
    }
}

static ENGINE: Mutex<Engine> = Mutex::new(Engine::new());
/// Hot-key id → action, for the press handler. The plugin calls the handler
/// with its own lock held, so this is only ever held for a lookup or a swap.
static ROUTES: Mutex<Vec<(u32, &'static str)>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn on_main<R: Runtime>(app: &AppHandle<R>, task: impl FnOnce() + Send + 'static) {
    if let Err(err) = app.run_on_main_thread(task) {
        log::line(format!("shortcuts: main thread unavailable: {err}"));
    }
}

/// Registers `wanted`, publishes the routes and tells the windows. Main thread.
fn sync_now<R: Runtime>(app: &AppHandle<R>, engine: &mut Engine, wanted: Wanted) {
    let plugin = app.try_state::<GlobalShortcut<R>>();
    engine.sync(plugin.as_ref().map(|p| p.inner() as &dyn Keys), wanted, typed_character);
    *lock(&ROUTES) = engine.routes();
    let _ = app.emit(STATUS_EVENT, engine.report());
}

/// Registers what the settings ask for, the chat shortcut `chat` and the other
/// actions in `stored`, in place of what is registered now. A no-op when
/// nothing changed (Settings saves on every step of the volume slider, and a
/// taken combination is not retried each time), and held back while Settings
/// records a combination.
pub fn apply<R: Runtime>(app: &AppHandle<R>, chat: &str, stored: &Bindings) {
    let wanted = Wanted { chat: crate::shortcut::sanitize(chat), stored: sanitize(stored) };
    let handle = app.clone();
    on_main(app, move || {
        let mut engine = lock(&ENGINE);
        engine.wanted = Some(wanted.clone());
        if engine.suspended.is_some() || engine.applied.as_ref() == Some(&wanted) {
            return;
        }
        sync_now(&handle, &mut engine, wanted);
    });
}

/// Settings is recording a combination (`on`), or is done. While it records,
/// every shortcut is let go, so the keys reach the recorder instead of running
/// an action. A recording ends by itself after SUSPEND_MAX.
pub fn suspend<R: Runtime>(app: &AppHandle<R>, on: bool) {
    let handle = app.clone();
    on_main(app, move || {
        if !on {
            resume(&handle, None);
            return;
        }
        let mut engine = lock(&ENGINE);
        engine.recordings += 1;
        let ticket = engine.recordings;
        engine.suspended = Some(ticket);
        let plugin = handle.try_state::<GlobalShortcut<R>>();
        engine.release_all(plugin.as_ref().map(|p| p.inner() as &dyn Keys));
        lock(&ROUTES).clear();
        let _ = handle.emit(STATUS_EVENT, engine.report());
        let later = handle.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SUSPEND_MAX).await;
            let again = later.clone();
            on_main(&later, move || resume(&again, Some(ticket)));
        });
    });
}

/// Takes the shortcuts back after a recording; with `ticket`, only if that
/// recording is the one still going. Main thread.
fn resume<R: Runtime>(app: &AppHandle<R>, ticket: Option<u64>) {
    let mut engine = lock(&ENGINE);
    let Some(current) = engine.suspended else { return };
    if ticket.is_some_and(|t| t != current) {
        return;
    }
    if ticket.is_some() {
        log::line("shortcuts: the recording never ended; shortcuts back on");
    }
    engine.suspended = None;
    match engine.wanted.clone() {
        Some(wanted) => sync_now(app, &mut engine, wanted),
        None => {
            let _ = app.emit(STATUS_EVENT, engine.report());
        }
    }
}

/// On exit: hands every combination back to the system.
pub fn release<R: Runtime>(app: &AppHandle<R>) {
    let mut engine = lock(&ENGINE);
    let plugin = app.try_state::<GlobalShortcut<R>>();
    engine.release_all(plugin.as_ref().map(|p| p.inner() as &dyn Keys));
    lock(&ROUTES).clear();
}

/// A registered combination was pressed (shortcut.rs's handler).
pub fn on_press<R: Runtime>(app: &AppHandle<R>, shortcut: &Shortcut) {
    let action = lock(&ROUTES).iter().find(|(id, _)| *id == shortcut.id()).map(|(_, action)| *action);
    if let Some(action) = action {
        dispatch(app, action);
    }
}

/// Hands an action to the island. The chat keeps the way it always came.
pub fn dispatch<R: Runtime>(app: &AppHandle<R>, action: &str) {
    log::line(format!("shortcut {action}"));
    let _ = if action == CHAT {
        app.emit_to(WINDOW_LABEL, "tray", "chat".to_string())
    } else {
        app.emit_to(WINDOW_LABEL, "shortcut", action.to_string())
    };
}

/// The action holding `shortcut` right now, if one of ours does.
pub fn holder(shortcut: &Shortcut) -> Option<&'static str> {
    lock(&ENGINE).held.iter().find(|(s, _)| s.id() == shortcut.id()).map(|(_, action)| *action)
}

pub fn status() -> Report {
    lock(&ENGINE).report()
}

/// The character a Ctrl+Alt combination types on one of the installed
/// keyboard layouts, if any (see the top of the file).
pub fn typed_character(shortcut: &Shortcut) -> Option<String> {
    if !is_altgr_like(shortcut) {
        return None;
    }
    let vk = character_vk(shortcut.key)?;
    ctrl_alt_types(vk, shortcut.mods.contains(Modifiers::SHIFT))
}

fn ctrl_alt_types(vk: u16, shift: bool) -> Option<String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayoutList, MapVirtualKeyExW, ToUnicodeEx, HKL, MAPVK_VK_TO_VSC, VK_CONTROL, VK_MENU, VK_SHIFT,
    };
    let mut layouts = [HKL::default(); 32];
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts[..])) };
    let count = usize::try_from(count).unwrap_or(0).min(layouts.len());

    let mut state = [0u8; 256];
    state[VK_CONTROL.0 as usize] = 0x80;
    state[VK_MENU.0 as usize] = 0x80;
    if shift {
        state[VK_SHIFT.0 as usize] = 0x80;
    }
    for &layout in &layouts[..count] {
        let scan = unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC, Some(layout)) };
        let mut buf = [0u16; 8];
        // Flag 0x4: leave the keyboard state alone, so a dead key met here
        // doesn't change what the user types next (Windows 10 1607 and later).
        let n = unsafe { ToUnicodeEx(u32::from(vk), scan, &state, &mut buf, 0x4, Some(layout)) };
        let typed = match n {
            // A dead key (´ ^ ¨…) is still something the user types with it.
            n if n < 0 => String::from_utf16_lossy(&buf[..1]),
            0 => continue,
            n => String::from_utf16_lossy(&buf[..(n as usize).min(buf.len())]),
        };
        if typed.chars().any(|c| !c.is_control()) {
            return Some(typed);
        }
    }
    None
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// How each global shortcut went the last time they were registered.
#[tauri::command]
pub fn shortcuts_status() -> Report {
    status()
}

/// Settings records a new combination: let go of ours meanwhile, so the keys
/// reach the recorder instead of running an action. `false` takes them back.
#[tauri::command]
pub fn shortcuts_suspend(app: AppHandle, suspended: bool) {
    suspend(&app, suspended);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn never(_: &Shortcut) -> Option<String> {
        None
    }

    fn keys(text: &str) -> Shortcut {
        crate::shortcut::parse(text).unwrap()
    }

    fn bound(keys: &str, enabled: bool) -> Binding {
        Binding { keys: keys.into(), enabled }
    }

    /// The OS, with some combinations held by other apps.
    #[derive(Default)]
    struct FakeKeys {
        taken: Vec<u32>,
        registered: RefCell<Vec<u32>>,
        calls: RefCell<Vec<String>>,
    }

    impl Keys for FakeKeys {
        fn register_key(&self, shortcut: Shortcut) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("+{}", crate::shortcut::canonical(&shortcut)));
            if self.taken.contains(&shortcut.id()) || self.registered.borrow().contains(&shortcut.id()) {
                return Err("already registered".into());
            }
            self.registered.borrow_mut().push(shortcut.id());
            Ok(())
        }
        fn unregister_key(&self, shortcut: Shortcut) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("-{}", crate::shortcut::canonical(&shortcut)));
            self.registered.borrow_mut().retain(|id| *id != shortcut.id());
            Ok(())
        }
    }

    fn wanted(chat: &str, stored: &[(&str, Binding)]) -> Wanted {
        Wanted { chat: chat.into(), stored: stored.iter().map(|(id, b)| (id.to_string(), b.clone())).collect() }
    }

    fn status_of(engine: &Engine, id: &str) -> Status {
        engine.report.iter().find(|s| s.id == id).unwrap().status
    }

    #[test]
    fn the_chat_comes_first_with_the_shortcut_roadeep_always_had() {
        assert_eq!(ACTIONS[0].id, CHAT);
        assert_eq!(ACTIONS[0].default_keys, crate::shortcut::DEFAULT_ACCELERATOR);
        let ids: Vec<_> = ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(ids, ["openChat", "toggleIsland", "goToAlert", "jumpToTerminal", "nextPill", "prevPill", "muteToggle"]);
    }

    #[test]
    fn every_default_is_canonical_ctrl_alt_and_unique() {
        let mut seen = Vec::new();
        for def in ACTIONS {
            let shortcut = keys(def.default_keys);
            assert_eq!(crate::shortcut::canonical(&shortcut), def.default_keys, "{}", def.id);
            assert!(is_altgr_like(&shortcut), "{} default is not Ctrl+Alt", def.id);
            assert!(!seen.contains(&shortcut.id()), "{} duplicates another default", def.id);
            seen.push(shortcut.id());
        }
    }

    #[test]
    fn only_the_chat_is_on_until_the_user_turns_another_on() {
        for def in ACTIONS {
            assert_eq!(def.enabled_by_default, def.id == CHAT, "{}", def.id);
        }
        let plan = plan(crate::shortcut::DEFAULT_ACCELERATOR, &Bindings::new(), never);
        for (def, outcome) in plan {
            match outcome {
                Ok(_) => assert_eq!(def.id, CHAT),
                Err(s) => assert_eq!(s.status, Status::Off, "{}", def.id),
            }
        }
    }

    #[test]
    fn the_chat_binding_is_settings_shortcut() {
        let chat = find(CHAT).unwrap();
        assert_eq!(effective(chat, "Ctrl+Alt+KeyK", &Bindings::new()), bound("Ctrl+Alt+KeyK", true));
        assert_eq!(effective(chat, "", &Bindings::new()), bound("", false));
        // A stored entry for the chat is never read.
        let stored: Bindings = [("openChat".to_string(), bound("Ctrl+Alt+KeyQ", true))].into();
        assert_eq!(effective(chat, "Ctrl+Alt+KeyK", &stored).keys, "Ctrl+Alt+KeyK");
        let alert = find("goToAlert").unwrap();
        assert_eq!(effective(alert, "", &Bindings::new()), bound("Ctrl+Alt+KeyA", false));
        let stored: Bindings = [("goToAlert".to_string(), bound("Ctrl+Shift+KeyG", true))].into();
        assert_eq!(effective(alert, "", &stored), bound("Ctrl+Shift+KeyG", true));
    }

    #[test]
    fn sanitize_keeps_known_actions_in_one_spelling() {
        let stored: Bindings = [
            ("goToAlert".to_string(), bound("alt+ctrl+g", true)),
            ("nextPill".to_string(), bound("", false)),
            ("muteToggle".to_string(), bound("Shift+KeyM", true)),
            ("prevPill".to_string(), bound("Ctrl+Banana", true)),
            ("openChat".to_string(), bound("Ctrl+Alt+KeyQ", true)),
            ("rm -rf".to_string(), bound("Ctrl+Alt+KeyX", true)),
        ]
        .into();
        let clean = sanitize(&stored);
        let expected: Bindings = [
            ("goToAlert".to_string(), bound("Ctrl+Alt+KeyG", true)),
            ("nextPill".to_string(), bound("", false)),
        ]
        .into();
        assert_eq!(clean, expected);
        assert_eq!(sanitize(&clean), clean, "idempotent");
    }

    #[test]
    fn a_malformed_entry_never_fails_the_settings_file() {
        let read = |json: &str| lenient_bindings(&mut serde_json::Deserializer::from_str(json)).unwrap();
        assert!(read(r#""Ctrl+Alt+A""#).is_empty());
        assert!(read("[1, 2]").is_empty());
        assert!(read("null").is_empty());
        let mixed = read(r#"{"goToAlert":{"keys":"Ctrl+Alt+KeyA"},"nextPill":"x","prevPill":{"keys":7},"muteToggle":{"enabled":false}}"#);
        let expected: Bindings = [
            ("goToAlert".to_string(), bound("Ctrl+Alt+KeyA", true)),
            ("muteToggle".to_string(), bound("", false)),
        ]
        .into();
        assert_eq!(mixed, expected);
    }

    #[test]
    fn bindings_round_trip_through_json() {
        let stored: Bindings = [("goToAlert".to_string(), bound("Ctrl+Alt+KeyA", false))].into();
        let json = serde_json::to_string(&stored).unwrap();
        assert_eq!(json, r#"{"goToAlert":{"keys":"Ctrl+Alt+KeyA","enabled":false}}"#);
        assert_eq!(lenient_bindings(&mut serde_json::Deserializer::from_str(&json)).unwrap(), stored);
    }

    #[test]
    fn a_combination_used_twice_goes_to_the_first_action_and_the_chat_comes_first() {
        let stored: Bindings = [
            ("goToAlert".to_string(), bound("Ctrl+Alt+KeyR", true)),
            ("jumpToTerminal".to_string(), bound("Ctrl+Alt+KeyT", true)),
            ("muteToggle".to_string(), bound("Ctrl+Alt+KeyT", true)),
        ]
        .into();
        let plan = plan("Ctrl+Alt+KeyR", &stored, never);
        let outcome = |id: &str| plan.iter().find(|(d, _)| d.id == id).unwrap().1.clone();
        assert!(outcome(CHAT).is_ok());
        assert_eq!(outcome("goToAlert").unwrap_err().status, Status::Duplicate);
        assert!(outcome("jumpToTerminal").is_ok());
        assert_eq!(outcome("muteToggle").unwrap_err().status, Status::Duplicate);
        // Same key, other modifiers: no clash. A disabled action holds nothing.
        let stored: Bindings = [
            ("goToAlert".to_string(), bound("Ctrl+Shift+KeyR", true)),
            ("jumpToTerminal".to_string(), bound("Ctrl+Alt+KeyR", false)),
        ]
        .into();
        let plan = super::plan("Ctrl+Alt+KeyR", &stored, never);
        assert!(plan.iter().find(|(d, _)| d.id == "goToAlert").unwrap().1.is_ok());
        assert_eq!(plan.iter().find(|(d, _)| d.id == "jumpToTerminal").unwrap().1.as_ref().unwrap_err().status, Status::Off);
    }

    #[test]
    fn a_combination_that_types_a_character_is_refused_except_the_chat() {
        let ogonek = |s: &Shortcut| (s.key == Code::KeyA || s.key == Code::KeyR).then(|| "ą".to_string());
        let stored: Bindings = [("goToAlert".to_string(), bound("Ctrl+Alt+KeyA", true))].into();
        let plan = plan("Ctrl+Alt+KeyR", &stored, ogonek);
        assert!(plan[0].1.is_ok(), "the chat shortcut keeps working");
        let refused = plan.iter().find(|(d, _)| d.id == "goToAlert").unwrap().1.clone().unwrap_err();
        assert_eq!((refused.status, refused.typed.as_deref()), (Status::TypesCharacter, Some("ą")));
        assert_eq!(refused.keys, "Ctrl+Alt+KeyA");
    }

    #[test]
    fn only_ctrl_alt_without_the_windows_key_reads_as_altgr() {
        assert!(is_altgr_like(&keys("Ctrl+Alt+KeyE")));
        assert!(is_altgr_like(&keys("Ctrl+Alt+Shift+KeyE")));
        assert!(!is_altgr_like(&keys("Ctrl+Shift+KeyE")));
        assert!(!is_altgr_like(&keys("Ctrl+Alt+Super+KeyE")));
        assert_eq!(typed_character(&keys("Ctrl+Shift+KeyE")), None);
        assert_eq!(typed_character(&keys("Ctrl+Alt+ArrowLeft")), None, "an arrow never types");
    }

    #[test]
    fn virtual_keys_match_the_ones_registered() {
        assert_eq!(character_vk(Code::KeyA), Some(0x41));
        assert_eq!(character_vk(Code::KeyZ), Some(0x5A));
        assert_eq!(character_vk(Code::Digit0), Some(0x30));
        assert_eq!(character_vk(Code::Digit9), Some(0x39));
        assert_eq!(character_vk(Code::BracketRight), Some(0xDD));
        assert_eq!(character_vk(Code::Space), Some(0x20));
        assert_eq!(character_vk(Code::ArrowLeft), None);
        assert_eq!(character_vk(Code::F5), None);
    }

    #[test]
    fn the_engine_registers_what_is_asked_and_reports_the_rest() {
        let os = FakeKeys { taken: vec![keys("Ctrl+Alt+KeyT").id()], ..Default::default() };
        let mut engine = Engine::new();
        let asked = wanted(
            "Ctrl+Alt+KeyR",
            &[("goToAlert", bound("Ctrl+Alt+KeyA", true)), ("jumpToTerminal", bound("Ctrl+Alt+KeyT", true))],
        );
        engine.sync(Some(&os), asked.clone(), never);
        assert_eq!(status_of(&engine, CHAT), Status::Active);
        assert_eq!(status_of(&engine, "goToAlert"), Status::Active);
        assert_eq!(status_of(&engine, "jumpToTerminal"), Status::InUse);
        assert_eq!(status_of(&engine, "muteToggle"), Status::Off);
        let taken = engine.report.iter().find(|s| s.id == "jumpToTerminal").unwrap();
        assert!(taken.error.as_deref().unwrap().starts_with(errors::SHORTCUT_TAKEN));
        let mut routes = engine.routes();
        routes.sort();
        let mut expected = vec![(keys("Ctrl+Alt+KeyR").id(), CHAT), (keys("Ctrl+Alt+KeyA").id(), "goToAlert")];
        expected.sort();
        assert_eq!(routes, expected);
        assert_eq!(engine.applied, Some(asked));
    }

    #[test]
    fn a_change_keeps_what_stays_and_lets_go_of_what_left() {
        let os = FakeKeys::default();
        let mut engine = Engine::new();
        engine.sync(Some(&os), wanted("Ctrl+Alt+KeyR", &[("goToAlert", bound("Ctrl+Alt+KeyA", true))]), never);
        os.calls.borrow_mut().clear();
        // The alert moves to the chat's old keys; the chat gets new ones.
        engine.sync(Some(&os), wanted("Ctrl+Alt+KeyK", &[("goToAlert", bound("Ctrl+Alt+KeyR", true))]), never);
        assert_eq!(*os.calls.borrow(), ["-Ctrl+Alt+KeyA", "+Ctrl+Alt+KeyK"]);
        assert_eq!(holder_in(&engine, "Ctrl+Alt+KeyR"), Some("goToAlert"));
        assert_eq!(holder_in(&engine, "Ctrl+Alt+KeyK"), Some(CHAT));
        engine.release_all(Some(&os));
        assert!(os.registered.borrow().is_empty());
        assert!(engine.routes().is_empty());
        assert_eq!(engine.applied, None, "the next apply registers them again");
    }

    #[test]
    fn without_the_plugin_nothing_is_active() {
        let mut engine = Engine::new();
        engine.sync(None, wanted("Ctrl+Alt+KeyR", &[]), never);
        assert_eq!(status_of(&engine, CHAT), Status::Unavailable);
        assert!(engine.routes().is_empty());
    }

    #[test]
    fn the_report_names_every_action_once_in_settings_order() {
        let mut engine = Engine::new();
        engine.sync(Some(&FakeKeys::default()), wanted("", &[]), never);
        let ids: Vec<_> = engine.report().actions.iter().map(|s| s.id).collect();
        assert_eq!(ids, ACTIONS.iter().map(|a| a.id).collect::<Vec<_>>());
        assert_eq!(status_of(&engine, CHAT), Status::Off);
        let json = serde_json::to_value(engine.report()).unwrap();
        assert_eq!(json["suspended"], false);
        assert_eq!(json["actions"][0], serde_json::json!({"id": "openChat", "keys": "", "status": "off"}));
    }

    fn holder_in(engine: &Engine, text: &str) -> Option<&'static str> {
        let id = keys(text).id();
        engine.held.iter().find(|(s, _)| s.id() == id).map(|(_, action)| *action)
    }
}
