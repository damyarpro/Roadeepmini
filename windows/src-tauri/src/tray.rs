// Notification-area icon: Open, Settings, Pause, Quit — in the UI language.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::island::WINDOW_LABEL;

const TRAY_ID: &str = "roadeep";

/// The language the menu was last built in, so a save that leaves it alone
/// doesn't rebuild the menu.
static BUILT_FOR: Mutex<String> = Mutex::new(String::new());

/// Open, Settings, Pause, Quit and the tooltip, for "fa" or "en" (anything else
/// is Persian, the app's default).
pub fn labels(language: &str) -> [&'static str; 5] {
    match language {
        "en" => ["Open Roadeep", "Settings…", "Pause", "Quit", "Roadeep"],
        _ => ["باز کردن رودیپ", "تنظیمات…", "توقف موقت", "خروج", "رودیپ"],
    }
}

fn menu(app: &AppHandle, language: &str) -> tauri::Result<Menu<Wry>> {
    let [open, settings, pause, quit, _] = labels(language);
    let open = MenuItem::with_id(app, "open", open, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", settings, true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", pause, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&open, &sep1, &settings, &pause, &sep2, &quit])
}

fn current_language(app: &AppHandle) -> String {
    app.try_state::<crate::Shared>()
        .map(|shared| shared.settings.lock().unwrap().language.clone())
        .unwrap_or_default()
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let language = current_language(app);
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(labels(&language)[4])
        .menu(&menu(app, &language)?)
        .on_menu_event(|app: &AppHandle, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "settings" => crate::show_settings_window(app),
            id => {
                let _ = app.emit_to(WINDOW_LABEL, "tray", id.to_string());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    *BUILT_FOR.lock().unwrap() = language;
    Ok(())
}

/// Called on every settings save: rebuilds the menu when the language changed.
pub fn sync_language(app: &AppHandle, language: &str) {
    let mut built = BUILT_FOR.lock().unwrap();
    if *built == language {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let result = menu(app, language).and_then(|m| tray.set_menu(Some(m))).and_then(|_| tray.set_tooltip(Some(labels(language)[4])));
    match result {
        Ok(()) => *built = language.to_string(),
        Err(err) => crate::log::line(format!("tray: could not relabel the menu: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_language() {
        assert_eq!(labels("en"), ["Open Roadeep", "Settings…", "Pause", "Quit", "Roadeep"]);
        assert_eq!(labels("fa")[0], "باز کردن رودیپ");
        assert_eq!(labels("fa")[3], "خروج");
        // Persian is the default, as in settings.rs.
        assert_eq!(labels(""), labels("fa"));
        assert_eq!(labels("de"), labels("fa"));
        for l in labels("fa").iter().chain(labels("en").iter()) {
            assert!(!l.is_empty());
        }
    }
}
