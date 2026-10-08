// The chat shortcut, the global shortcut that opens the island chat from
// anywhere. One accelerator, stored in settings.json as e.g. "Ctrl+Alt+KeyR";
// "" switches it off. It is the first of the global shortcuts shortcuts.rs
// registers (Settings → Shortcuts); this file keeps how an accelerator is
// parsed and spelled, the plugin's handler and the entry points lib.rs calls.
// Another app may already own a combination: that is logged and shown in
// Settings, never fatal.

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcut, Modifiers, Shortcut, ShortcutEvent, ShortcutState};

use crate::errors;
use crate::log;
use crate::shortcuts::Status;

/// Not Ctrl+Alt+Space: the Claude desktop app already holds that one, and the
/// people who run this next to Claude Code usually have it installed.
pub const DEFAULT_ACCELERATOR: &str = "Ctrl+Alt+KeyR";

/// Ctrl, Alt or Win. Shift alone is not enough: Shift+A would take capital A
/// away from every other app.
const STRONG_MODIFIERS: Modifiers = Modifiers::CONTROL.union(Modifiers::ALT).union(Modifiers::SUPER);

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    /// What is configured ("" = off).
    pub accelerator: String,
    pub registered: bool,
    /// An `errors` code when the shortcut is configured but not working.
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCheck {
    pub ok: bool,
    /// The canonical spelling to store (e.g. "Ctrl+Alt+Space"), when it parses.
    pub accelerator: String,
    pub error: Option<String>,
    /// What the combination types on an installed keyboard layout, when
    /// Ctrl+Alt is AltGr there (shortcuts::typed_character).
    pub typed: Option<String>,
}

/// Parses an accelerator and checks it is one we accept. The error is an
/// `errors` code.
pub fn parse(accelerator: &str) -> Result<Shortcut, String> {
    let text = accelerator.trim();
    let shortcut: Shortcut = text.parse().map_err(|_| errors::coded(errors::SHORTCUT_INVALID, &[text]))?;
    if !shortcut.mods.intersects(STRONG_MODIFIERS) {
        return Err(errors::coded(errors::SHORTCUT_NO_MODIFIER, &[]));
    }
    Ok(shortcut)
}

/// One spelling per combination, so "ctrl+alt+space" and "Alt+Ctrl+Space"
/// compare (and display) the same.
pub fn canonical(shortcut: &Shortcut) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (flag, name) in [
        (Modifiers::CONTROL, "Ctrl"),
        (Modifiers::ALT, "Alt"),
        (Modifiers::SHIFT, "Shift"),
        (Modifiers::SUPER, "Super"),
    ] {
        if shortcut.mods.contains(flag) {
            parts.push(name.into());
        }
    }
    parts.push(shortcut.key.to_string());
    parts.join("+")
}

/// What settings.rs keeps: "" (off), or a valid accelerator in canonical form.
/// Anything else falls back to the default.
pub fn sanitize(accelerator: &str) -> String {
    if accelerator.trim().is_empty() {
        return String::new();
    }
    match parse(accelerator) {
        Ok(s) => canonical(&s),
        Err(_) => DEFAULT_ACCELERATOR.into(),
    }
}

/// The plugin's handler: a press goes to the action holding the combination
/// (the chat's still asks the island for the chat, as it always did).
pub fn on_event(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state() == ShortcutState::Pressed {
        crate::shortcuts::on_press(app, shortcut);
    }
}

/// Registers the chat shortcut `accelerator` and the other global shortcuts in
/// place of the current ones. A no-op when nothing changed, so `save_settings`
/// calls it on every save. The other actions' bindings come from the shared
/// settings, which `save_settings` (and the app's setup) has set by then.
pub fn apply(app: &AppHandle, accelerator: &str) {
    let stored = app
        .try_state::<crate::Shared>()
        .map(|shared| shared.settings.lock().unwrap_or_else(|e| e.into_inner()).shortcuts.clone())
        .unwrap_or_default();
    crate::shortcuts::apply(app, accelerator, &stored);
}

/// On exit: hand every combination back to the system.
pub fn release(app: &AppHandle) {
    crate::shortcuts::release(app);
}

/// The chat shortcut's state, as Settings → General used to show it.
#[tauri::command]
pub fn shortcut_status() -> ShortcutStatus {
    let Some(chat) = crate::shortcuts::status().actions.into_iter().find(|s| s.id == crate::shortcuts::CHAT) else {
        return ShortcutStatus::default();
    };
    let registered = chat.status == Status::Active;
    ShortcutStatus { accelerator: chat.keys, registered, error: if registered { None } else { chat.error } }
}

/// For the recorder in Settings: is this combination usable right now? One
/// that types a character with AltGr is refused; a free one is tried for real
/// (registered and released at once), since only the OS knows whether another
/// app holds it. One Roadeep already holds is fine: Settings itself spots two
/// actions on the same keys.
#[tauri::command]
pub fn shortcut_check(app: AppHandle, accelerator: String) -> ShortcutCheck {
    let shortcut = match parse(&accelerator) {
        Ok(s) => s,
        Err(code) => return ShortcutCheck { ok: false, accelerator: String::new(), error: Some(code), typed: None },
    };
    let name = canonical(&shortcut);
    let verdict = |ok: bool, error: Option<String>, typed: Option<String>| ShortcutCheck {
        ok,
        accelerator: name.clone(),
        error,
        typed,
    };
    if crate::shortcuts::holder(&shortcut).is_some() {
        return verdict(true, None, None);
    }
    if let Some(typed) = crate::shortcuts::typed_character(&shortcut) {
        return verdict(false, None, Some(typed));
    }
    let Some(gs) = app.try_state::<GlobalShortcut<tauri::Wry>>() else {
        return verdict(false, Some(errors::coded(errors::SHORTCUT_TAKEN, &["unavailable"])), None);
    };
    match gs.register(shortcut) {
        Ok(()) => {
            if let Err(err) = gs.unregister(shortcut) {
                log::line(format!("shortcut: could not release the trial of {name}: {err}"));
            }
            verdict(true, None, None)
        }
        Err(err) => verdict(false, Some(errors::coded(errors::SHORTCUT_TAKEN, &[&err.to_string()])), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses_and_is_canonical() {
        let s = parse(DEFAULT_ACCELERATOR).unwrap();
        assert_eq!(canonical(&s), DEFAULT_ACCELERATOR);
    }

    #[test]
    fn spellings_collapse_to_one_form() {
        for raw in ["ctrl+alt+space", "Alt+Control+Space", " Ctrl + Alt + SPACE "] {
            assert_eq!(sanitize(raw), "Ctrl+Alt+Space", "{raw}");
        }
        assert_eq!(sanitize("Ctrl+Shift+KeyK"), "Ctrl+Shift+KeyK");
        assert_eq!(sanitize("Super+Digit1"), "Super+Digit1");
        assert_eq!(sanitize("Alt+F5"), "Alt+F5");
    }

    #[test]
    fn a_strong_modifier_is_required() {
        assert_eq!(parse("Space").unwrap_err(), errors::SHORTCUT_NO_MODIFIER);
        assert_eq!(parse("Shift+KeyA").unwrap_err(), errors::SHORTCUT_NO_MODIFIER);
        assert!(parse("Ctrl+Shift+KeyA").is_ok());
    }

    #[test]
    fn invalid_falls_back_and_empty_means_off() {
        assert!(parse("Ctrl+Nope").unwrap_err().starts_with(errors::SHORTCUT_INVALID));
        assert!(parse("Ctrl+").is_err());
        assert_eq!(sanitize("Ctrl+Nope"), DEFAULT_ACCELERATOR);
        assert_eq!(sanitize("KeyA"), DEFAULT_ACCELERATOR);
        assert_eq!(sanitize(""), "");
        assert_eq!(sanitize("   "), "");
    }
}
