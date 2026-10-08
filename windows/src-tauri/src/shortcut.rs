// The global shortcut that opens the island chat from anywhere (Settings →
// General). One accelerator, stored in settings.json as e.g. "Ctrl+Alt+Space";
// "" switches it off. Another app may already own the combination: that is
// logged and shown in Settings, never fatal.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState};

use crate::errors;
use crate::island::WINDOW_LABEL;
use crate::log;

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
}

struct Current {
    status: ShortcutStatus,
    shortcut: Option<Shortcut>,
}

static CURRENT: Mutex<Current> = Mutex::new(Current {
    status: ShortcutStatus { accelerator: String::new(), registered: false, error: None },
    shortcut: None,
});

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

/// The plugin's handler: every press of our shortcut asks the island for the chat.
pub fn on_event(app: &AppHandle, _shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state() == ShortcutState::Pressed {
        let _ = app.emit_to(WINDOW_LABEL, "tray", "chat".to_string());
    }
}

/// Registers `accelerator` in place of whatever was registered before. A no-op
/// when it did not change, so `save_settings` calls it on every save (the volume
/// slider saves on every step: a taken combination is not retried each time).
pub fn apply(app: &AppHandle, accelerator: &str) {
    let wanted = sanitize(accelerator);
    let mut current = CURRENT.lock().unwrap();
    if current.status.accelerator == wanted && (current.status.registered || current.status.error.is_some() || wanted.is_empty()) {
        return;
    }
    if let Some(old) = current.shortcut.take() {
        if let Err(err) = app.global_shortcut().unregister(old) {
            log::line(format!("shortcut: could not unregister {}: {err}", canonical(&old)));
        }
    }
    current.status = ShortcutStatus { accelerator: wanted.clone(), registered: false, error: None };
    if wanted.is_empty() {
        log::line("shortcut: off");
        return;
    }
    let shortcut = match parse(&wanted) {
        Ok(s) => s,
        Err(code) => {
            current.status.error = Some(code);
            return;
        }
    };
    match app.global_shortcut().register(shortcut) {
        Ok(()) => {
            log::line(format!("shortcut: registered {wanted}"));
            current.status.registered = true;
            current.shortcut = Some(shortcut);
        }
        Err(err) => {
            // Most often another app already owns the combination.
            log::line(format!("shortcut: could not register {wanted}: {err}"));
            current.status.error = Some(errors::coded(errors::SHORTCUT_TAKEN, &[&err.to_string()]));
        }
    }
}

/// On exit: hand the combination back to the system.
pub fn release(app: &AppHandle) {
    let mut current = CURRENT.lock().unwrap();
    if let Some(old) = current.shortcut.take() {
        let _ = app.global_shortcut().unregister(old);
    }
    current.status.registered = false;
}

#[tauri::command]
pub fn shortcut_status() -> ShortcutStatus {
    CURRENT.lock().unwrap().status.clone()
}

/// For the recorder in Settings: is this combination usable right now? A free
/// combination is tried for real (registered and released at once), since only
/// the OS knows whether another app holds it.
#[tauri::command]
pub fn shortcut_check(app: AppHandle, accelerator: String) -> ShortcutCheck {
    let shortcut = match parse(&accelerator) {
        Ok(s) => s,
        Err(code) => return ShortcutCheck { ok: false, accelerator: String::new(), error: Some(code) },
    };
    let name = canonical(&shortcut);
    {
        let current = CURRENT.lock().unwrap();
        if current.status.registered && current.status.accelerator == name {
            return ShortcutCheck { ok: true, accelerator: name, error: None };
        }
    }
    let gs = app.global_shortcut();
    match gs.register(shortcut) {
        Ok(()) => {
            if let Err(err) = gs.unregister(shortcut) {
                log::line(format!("shortcut: could not release the trial of {name}: {err}"));
            }
            ShortcutCheck { ok: true, accelerator: name, error: None }
        }
        Err(err) => ShortcutCheck {
            ok: false,
            accelerator: name,
            error: Some(errors::coded(errors::SHORTCUT_TAKEN, &[&err.to_string()])),
        },
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
