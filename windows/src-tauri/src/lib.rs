// Roadeep for Windows — app wiring and the commands the island calls.

mod agents;
mod catalog;
mod clock;
mod coding;
mod coding_hooks;
mod computer;
mod default_agents;
mod dock;
mod desktop_actions;
mod dock_motion;
mod errors;
mod files;
mod hooks;
mod integrations;
mod island;
mod log;
mod mcp;
mod mcpc;
mod migrate;
mod pipe;
mod planner;
mod roadeep;
mod secrets;
mod settings;
mod shortcut;
mod shortcuts;
mod tray;
mod updater;
mod util;
mod voice;
mod local_runtime;
mod local_intelligence;
mod win_user;

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

use files::DroppedFile;
use hooks::{HookPreview, HookStatus};
use dock::Layout;
use dock_motion::Motion;
use island::{PollGate, ScreenInfo};
use pipe::Pending;
use roadeep::auth::SessionInfo;
use roadeep::chat::{ChatContext, ChatReply, ChatState, DraftState, RoadeepAgent, RoadeepModel, SendOptions};
use roadeep::http::RoadeepError;
use roadeep::Roadeep;
use settings::Settings;

/// Keeps spawned helpers from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Shared {
    pub settings: Mutex<Settings>,
    pub gate: Arc<PollGate>,
    pub drops: DropRegistry,
    /// Drags, dock changes and the maximise slide (dock_motion.rs).
    pub motion: Arc<Motion>,
}

/// How long a dropped path stays claimable by `ingest_file`.
const DROP_TTL: Duration = Duration::from_secs(120);
/// How long `ingest_file` waits for the native drop event to be recorded: the
/// webview's copy of the event can reach the page a moment before ours runs.
const DROP_WAIT: Duration = Duration::from_millis(600);
/// More paths than anyone drops at once; bounds the list.
const MAX_DROPS: usize = 32;

/// Paths the OS really dropped on the island, recorded on the Rust side from the
/// window's own drag-and-drop event. `ingest_file` only copies one of these, so
/// the page cannot use it (and, through the inbox, chat uploads) to read any
/// file it likes.
#[derive(Default)]
pub struct DropRegistry {
    paths: Mutex<Vec<(PathBuf, Instant)>>,
}

impl DropRegistry {
    fn record(&self, paths: &[PathBuf]) {
        let mut list = self.paths.lock().unwrap();
        list.retain(|(_, at)| at.elapsed() < DROP_TTL);
        for p in paths {
            list.push((p.clone(), Instant::now()));
        }
        let excess = list.len().saturating_sub(MAX_DROPS);
        list.drain(..excess);
    }

    /// Takes a recorded drop: each one can be ingested once.
    fn claim(&self, path: &str) -> bool {
        let wanted = PathBuf::from(path);
        let mut list = self.paths.lock().unwrap();
        list.retain(|(_, at)| at.elapsed() < DROP_TTL);
        match list.iter().position(|(p, _)| *p == wanted) {
            Some(i) => {
                list.remove(i);
                true
            }
            None => false,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootInfo {
    settings: Settings,
    screen: ScreenInfo,
    /// How the island is laid out in its window for the current dock and display.
    layout: Layout,
    version: String,
    hook_path: String,
}

#[tauri::command]
fn boot(app: AppHandle, shared: State<Shared>) -> BootInfo {
    let mut settings = shared.settings.lock().unwrap().clone();
    // The real state of ~/.claude/settings.json wins over whatever we stored.
    settings.hooks_installed = hooks::status().installed;
    let screen = island::screen_info(&app, &settings);
    let layout = island::current_layout(&app, &settings);
    BootInfo {
        settings,
        screen,
        layout,
        version: env!("CARGO_PKG_VERSION").to_string(),
        hook_path: settings::hook_exe_path().to_string_lossy().to_string(),
    }
}

#[tauri::command]
async fn save_settings(app: AppHandle, shared: State<'_, Shared>, mut settings: Settings) -> Result<(), String> {
    // A window built before the Roadeep switch may still send a claude-* model.
    settings::migrate(&mut settings);
    let settings_app = app.clone();
    let (screen_changed, autostart_changed, settings) = tauri::async_runtime::spawn_blocking(move || {
        let settings_shared = settings_app.state::<Shared>();
        let mut current = settings_shared.settings.lock().map_err(|_| "settings-lock-failed".to_string())?;
        let screen_changed = current.screen != settings.screen;
        let autostart_changed = current.autostart != settings.autostart;
        // The dock belongs to Rust (drag, dock_set): a window holding an older
        // copy must not move the island back. Picking a display preference
        // forgets the display the island was dragged to.
        settings.dock = current.dock.clone();
        // Same for the built-in agents' version: a window must not reset it.
        settings.default_agents_version = current.default_agents_version;
        if screen_changed {
            settings.dock.monitor.clear();
        }
        settings_app.state::<coding::history::CodingHistory>().commit_preference(settings.retain_coding_history, || {
            settings::save(&settings).map_err(|_| {
                log::line("settings: could not persist preferences");
                "settings-save-failed".to_string()
            })
        })?;
        *current = settings.clone();
        Ok::<_, String>((screen_changed, autostart_changed, settings))
    }).await.map_err(|_| { log::line("settings: preference worker failed"); "settings-worker-failed".to_string() })??;
    if autostart_changed {
        let manager = app.autolaunch();
        let result = if settings.autostart { manager.enable() } else { manager.disable() };
        if let Err(err) = result {
            eprintln!("[roadeep] autostart: {err}");
        }
    }
    if screen_changed && !shared.motion.is_active() {
        let collapsed = shared.gate.collapsed.load(Ordering::Relaxed);
        let maximized = shared.motion.maximized.load(Ordering::Relaxed);
        island::apply_geometry(&app, &settings, collapsed, maximized);
    }
    app.state::<coding::CodingObserver>().set_enabled(settings.observe_codex);
    let _ = app.emit("coding-status", app.state::<coding::CodingObserver>().status());
    tray::sync_language(&app, &settings.language);
    shortcut::apply(&app, &settings.shortcut);
    // Keep the other window in step (island ⇄ settings window).
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

/// Hidden island → shrink the window to the wake strip (its line) and park the
/// cursor poll; anything else → full panel and 60 Hz polling.
#[tauri::command]
fn set_collapsed(app: AppHandle, shared: State<Shared>, collapsed: bool) {
    let settings = shared.settings.lock().unwrap().clone();
    shared.gate.collapsed.store(collapsed, Ordering::Relaxed);
    if collapsed {
        // Maximised is a mode of the open chat; a hidden island is never in it.
        shared.motion.maximized.store(false, Ordering::Relaxed);
    }
    // A move in progress places the window itself when it lands (it checks
    // `collapsed` then).
    if !shared.motion.is_active() {
        let maximized = shared.motion.maximized.load(Ordering::Relaxed);
        island::apply_geometry(&app, &settings, collapsed, maximized);
    }
    // The wake strip must always take the mouse, and a resize invalidates the flag.
    island::set_ignore_cursor(&app, false);
    shared.gate.forget_ignore_state();
    shared.gate.set_active(!collapsed);
    // Collapsed, the cursor poll sleeps and never re-arms the drop target on a
    // press; a file dragged onto the wake strip must still be taken.
    if collapsed {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || island::unblock_webview_drops(&handle));
    }
}

/// The front end pushes the island shape; Rust decides click-through from it.
#[tauri::command]
fn set_island_rect(shared: State<Shared>, x: f64, y: f64, width: f64, height: f64) {
    shared.gate.set_rect(island::IslandRect { x, y, w: width, h: height });
}

#[tauri::command]
fn focus_window(app: AppHandle, focused: bool) {
    let Some(win) = island::window(&app) else { return };
    island::set_activating(&win, focused);
    if focused {
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn reposition(app: AppHandle, shared: State<Shared>) {
    if shared.motion.is_active() {
        return;
    }
    let settings = shared.settings.lock().unwrap().clone();
    let collapsed = shared.gate.collapsed.load(Ordering::Relaxed);
    let maximized = shared.motion.maximized.load(Ordering::Relaxed);
    island::apply_geometry(&app, &settings, collapsed, maximized);
}

#[tauri::command]
fn open_url(url: String) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let _ = Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// "Open terminal" opens the working folder in VS Code when `code` is on PATH,
/// and falls back to Explorer otherwise.
#[tauri::command]
fn open_in_vscode(path: Option<String>) -> bool {
    // No `cmd /C` anywhere near this. The path is a project folder chosen by
    // whoever is using Claude Code, and cmd would happily read `&`, `^` and `%`
    // in a folder name as syntax. Finding the launcher ourselves and handing the
    // path over as a separate argument keeps it a path.
    if let Some(code) = find_on_path("code") {
        let mut cmd = Command::new(code);
        if let Some(p) = path.as_deref().filter(|p| !p.is_empty()) {
            cmd.arg(p);
        }
        if cmd.creation_flags(CREATE_NO_WINDOW).spawn().is_ok() {
            return true;
        }
    }
    if let Some(p) = path.as_deref().filter(|p| !p.is_empty()) {
        let _ = Command::new("explorer").arg(p).spawn();
    }
    false
}

/// Our own `where`: walks %PATH% against %PATHEXT%, no shell involved.
/// Rust quotes arguments correctly for `.cmd`/`.bat` targets since 1.77, so
/// spawning `code.cmd` directly is safe.
fn find_on_path(stem: &str) -> Option<std::path::PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let dirs = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&dirs) {
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let candidate = dir.join(format!("{stem}{}", ext.to_lowercase()));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Tray → Pause. Paused means paused: the pollers stop talking to the network,
/// not just the island stopping showing things.
#[tauri::command]
fn set_paused(paused: bool) {
    integrations::set_paused(paused);
}

// ── Roadeep MCP server for the coding apps ────────────────────────────────────

#[tauri::command]
fn mcp_status(roadeep: State<Roadeep>) -> mcp::install::McpStatus {
    mcp::install::status(roadeep.session.has_session())
}

/// The diff the user has to look at before that app's config is touched.
/// `client` is an id from mcp::clients; anything else is refused.
#[tauri::command]
fn mcp_preview(client: String, install: bool) -> Result<mcp::install::McpPreview, String> {
    mcp::install::preview(&client, install)
}

/// Only ever called from an explicit click in the settings window, with the
/// fingerprint of the preview the user saw.
#[tauri::command]
fn mcp_apply(client: String, install: bool, fingerprint: String) -> Result<String, String> {
    mcp::install::write(&client, install, &fingerprint)
}

// ── Claude Code hooks ─────────────────────────────────────────────────────────

#[tauri::command]
fn hooks_status() -> HookStatus {
    hooks::status()
}

/// Returns the diff the user has to look at before anything is written.
#[tauri::command]
fn hooks_preview(install: bool) -> Result<HookPreview, String> {
    hooks::preview(install)
}

/// Only ever called from an explicit click in the settings window.
#[tauri::command]
fn hooks_apply(
    app: AppHandle,
    shared: State<Shared>,
    install: bool,
    fingerprint: String,
) -> Result<String, String> {
    // The fingerprint comes from the preview the user actually looked at, so a
    // settings.json that changed in between is refused rather than overwritten.
    let backup = hooks::write(install, &fingerprint)?;
    let updated = {
        let mut current = shared.settings.lock().unwrap();
        current.hooks_installed = install;
        let _ = settings::save(&current);
        current.clone()
    };
    let _ = app.emit("settings-changed", updated);
    Ok(backup)
}

#[tauri::command]
fn approval_decision(window: tauri::Window, app: AppHandle, request_id: String, decision: String) -> Result<(), String> {
    pipe::authorize_reply(window.label(), &request_id, Some(&decision))?;
    pipe::answer(&app, &request_id, &decision);
    Ok(())
}

/// Answers to a question Claude Code asked, picked on the island: one per
/// question, by position (pipe::question_answers_line). Answers of the wrong
/// shape hand the request back to the terminal at once.
#[tauri::command]
fn approval_answer(window: tauri::Window, app: AppHandle, request_id: String, answers: serde_json::Value) -> Result<(), String> {
    pipe::authorize_reply(window.label(), &request_id, None)?;
    match pipe::question_answers_line(&answers) {
        Ok(line) => {
            pipe::answer_question(&app, &request_id, line);
            Ok(())
        }
        Err(err) => {
            pipe::decline(&app, &request_id);
            Err(err)
        }
    }
}

/// The island has the card on screen, so the long wait for a human may begin.
/// Until this arrives the relay only waits a few hundred milliseconds, which is
/// what stops a paused or unresponsive island from freezing Claude Code.
#[tauri::command]
fn approval_ack(window: tauri::Window, app: AppHandle, request_id: String) -> Result<(), String> {
    pipe::authorize_reply(window.label(), &request_id, None)?;
    pipe::acknowledge(&app, &request_id);
    Ok(())
}

/// Nobody can act on this request — the island is paused, or another card is
/// already up. Claude Code falls back to asking in the terminal immediately.
#[tauri::command]
fn approval_decline(window: tauri::Window, app: AppHandle, request_id: String) -> Result<(), String> {
    pipe::authorize_reply(window.label(), &request_id, None)?;
    pipe::decline(&app, &request_id);
    Ok(())
}

// ── Chat, files and secrets ───────────────────────────────────────────────────

/// One chat turn over Roadeep. Tokens and file bytes stay on the Rust side.
/// The error is a serialized `RoadeepError` ({code, message, status, ...}).
/// While it runs, "chat-stream" events tagged with `turn` go to the island.
#[tauri::command]
async fn chat_send(
    shared: State<'_, Shared>,
    roadeep: State<'_, Roadeep>,
    chat: State<'_, ChatState>,
    query: String,
    context: Option<ChatContext>,
    agent_id: Option<String>,
    turn: Option<u64>,
    voice_task: Option<bool>,
) -> Result<ChatReply, RoadeepError> {
    let settings = shared.settings.lock().unwrap().clone();
    let mut opts = SendOptions::from_settings(query, context, agent_id, &settings);
    opts.turn = turn.unwrap_or(0);
    opts.voice_task = voice_task.unwrap_or(false);
    roadeep::chat::send(&roadeep, &chat, opts).await
}

#[tauri::command]
fn chat_reset(chat: State<ChatState>) {
    chat.reset();
}

/// Stops the reply being generated, if any.
#[tauri::command]
async fn chat_cancel(roadeep: State<'_, Roadeep>, chat: State<'_, ChatState>) -> Result<(), RoadeepError> {
    roadeep::chat::cancel(&roadeep, &chat).await
}

// ── Roadeep account and catalogues ────────────────────────────────────────────

/// A new sign-in may be a different account: nothing of the last one carries over.
#[tauri::command]
async fn roadeep_login(
    roadeep: State<'_, Roadeep>,
    chat: State<'_, ChatState>,
    email: String,
    password: String,
) -> Result<SessionInfo, RoadeepError> {
    let info = roadeep::auth::login(&roadeep, &email, &password).await?;
    chat.forget_account();
    Ok(info)
}

#[tauri::command]
async fn roadeep_otp_send(roadeep: State<'_, Roadeep>, phone: String) -> Result<(), RoadeepError> {
    roadeep::auth::otp_send(&roadeep, &phone).await
}

#[tauri::command]
async fn roadeep_otp_verify(
    roadeep: State<'_, Roadeep>,
    chat: State<'_, ChatState>,
    phone: String,
    otp: String,
) -> Result<SessionInfo, RoadeepError> {
    let info = roadeep::auth::otp_verify(&roadeep, &phone, &otp).await?;
    chat.forget_account();
    Ok(info)
}

#[tauri::command]
fn roadeep_logout(roadeep: State<Roadeep>, chat: State<ChatState>) {
    chat.forget_account();
    roadeep::auth::logout(&roadeep);
}

#[tauri::command]
async fn roadeep_session(roadeep: State<'_, Roadeep>) -> Result<SessionInfo, RoadeepError> {
    Ok(roadeep::auth::session(&roadeep).await)
}

#[tauri::command]
async fn roadeep_models(roadeep: State<'_, Roadeep>, chat: State<'_, ChatState>) -> Result<Vec<RoadeepModel>, RoadeepError> {
    roadeep::chat::models(&roadeep, &chat).await
}

#[tauri::command]
async fn roadeep_agents(roadeep: State<'_, Roadeep>) -> Result<Vec<RoadeepAgent>, RoadeepError> {
    roadeep::chat::agents(&roadeep).await
}

/// Copies a dropped file into the inbox and reports its name back. Only a path
/// the island has just received from a real drop is accepted (`DropRegistry`).
#[tauri::command]
async fn ingest_file(shared: State<'_, Shared>, path: String) -> Result<DroppedFile, String> {
    let waited = Instant::now();
    while !shared.drops.claim(&path) {
        if waited.elapsed() >= DROP_WAIT {
            log::line("files: refused to ingest a path that was not dropped on the island");
            return Err(errors::coded(errors::FILE_NOT_DROPPED, &[]));
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    tauri::async_runtime::spawn_blocking(move || files::ingest(&path))
        .await
        .map_err(|e| errors::coded(errors::FILE_COPY, &[&e.to_string()]))?
}

/// The island may only ask whether a key exists — never read it.
#[tauri::command]
fn secret_present(key: String) -> bool {
    secrets::present(&key)
}

/// Only for values that are settings rather than secrets (secrets::PUBLIC_KEYS).
#[tauri::command]
fn secret_public_value(key: String) -> Option<String> {
    secrets::public_value(&key)
}

#[tauri::command]
fn secret_set(key: String, value: String) -> Result<(), String> {
    secrets::set(&key, &value)?;
    integrations::key_changed(&key);
    Ok(())
}

#[tauri::command]
fn secret_clear(key: String) -> Result<(), String> {
    secrets::clear(&key)?;
    integrations::key_changed(&key);
    Ok(())
}

/// Opens the configured n8n instance — the URL lives in the Credential Manager.
#[tauri::command]
fn open_n8n() {
    if let Some(url) = secrets::get("n8n-url") {
        open_url(url);
    }
}

/// Refresh buttons in the integration cards.
#[tauri::command]
async fn refresh_integration(app: AppHandle, id: String) {
    // The card shows the outcome itself; poll_once has recorded and logged it.
    let _ = integrations::poll_once(app, &id).await;
}

/// "Test connection" in Settings → Integrations: one poll now, and its outcome.
#[tauri::command]
async fn test_integration(app: AppHandle, id: String) -> Result<integrations::PollStatus, String> {
    integrations::test(app, &id).await
}

/// When each service last answered, and its current failure, for Settings.
#[tauri::command]
fn integration_status() -> Vec<integrations::PollStatus> {
    integrations::statuses()
}

/// The bundled catalog services (sorted by name) for Settings → the market.
#[tauri::command]
fn catalog_list() -> Vec<catalog::CatalogEntry> {
    catalog::get().entries()
}

/// Lets the island write to the same log as the Rust side.
#[tauri::command]
fn log_line(message: String) {
    log::line(format!("ui  {message}"));
}

// ── Settings window ───────────────────────────────────────────────────────────

/// WebView2 allows exactly one browser environment per app, and its options are
/// fixed by whichever webview is created first. Every window must therefore ask
/// for the *same* arguments as the island (see `additionalBrowserArgs` in
/// tauri.conf.json) — a mismatch makes the second window come up blank, with no
/// error anywhere.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

/// In a dev build the pages are served by Vite, so the second window needs the
/// absolute dev URL; a bundled build resolves it inside the app bundle.
fn settings_page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/settings.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("settings.html".into())
}

/// The settings window is created hidden at launch and only ever shown and
/// hidden afterwards. A WebView2 window created later — on the main thread or
/// not — silently comes up blank in this app, so the window that works is the
/// one that exists before the island's webview does.
fn create_settings_window(app: &AppHandle) {
    let url = settings_page_url(app);
    match WebviewWindowBuilder::new(app, "settings", url)
        .additional_browser_args(BROWSER_ARGS)
        .title("Settings — Roadeep")
        .inner_size(560.0, 680.0)
        .min_inner_size(460.0, 480.0)
        .resizable(true)
        .visible(false)
        .center()
        .build()
    {
        Ok(win) => {
            // Closing it must only hide it, or it could never be reopened.
            let hidden = win.clone();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = hidden.hide();
                }
            });
        }
        Err(err) => log::line(format!("settings window failed: {err}")),
    }
}

pub fn show_settings_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window("settings") else {
        log::line("settings window missing");
        return;
    };
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

/// `section` (e.g. "account") asks the settings window to jump to that section.
#[tauri::command]
fn open_settings_window(app: AppHandle, section: Option<String>) {
    show_settings_window(&app);
    if let Some(section) = section.filter(|s| !s.is_empty() && s.len() <= 32) {
        let _ = app.emit_to("settings", "settings-focus", section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dropped_paths_can_be_claimed_once() {
        let drops = DropRegistry::default();
        assert!(!drops.claim(r"C:\Users\me\.ssh\id_rsa"), "nothing dropped yet");
        drops.record(&[PathBuf::from(r"C:\Users\me\Desktop\a.pdf")]);
        assert!(!drops.claim(r"C:\Users\me\Desktop\b.pdf"));
        assert!(drops.claim(r"C:\Users\me\Desktop\a.pdf"));
        assert!(!drops.claim(r"C:\Users\me\Desktop\a.pdf"), "each drop is claimed once");

        let many: Vec<PathBuf> = (0..MAX_DROPS + 5).map(|i| PathBuf::from(format!(r"C:\f{i}"))).collect();
        drops.record(&many);
        assert!(!drops.claim(r"C:\f0"), "the oldest entries go first");
        assert!(drops.claim(&format!(r"C:\f{}", MAX_DROPS + 4)));
    }
}

pub fn run() {
    if let Some(code) = coding_hooks::run_cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        std::process::exit(code);
    }
    // Data from builds under the former name, before anything reads it and
    // before the WebView profile is opened.
    migrate::run_early();
    let loaded = settings::load();
    let gate = Arc::new(PollGate::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let _ = app.emit_to(island::WINDOW_LABEL, "tray", "open".to_string());
        }))
        // The Run-key value name. The entry under the former name is removed at
        // startup (migrate::migrate_autostart).
        .plugin(tauri_plugin_autostart::Builder::new().app_name("Roadeep").build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(shortcut::on_event).build())
        // Inert unless the build enabled it (updater.rs); its JS commands are not granted.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateState::default())
        .manage(Shared {
            settings: Mutex::new(loaded.clone()),
            gate: gate.clone(),
            drops: DropRegistry::default(),
            motion: Arc::new(Motion::new()),
        })
        .manage(coding::CodingObserver::new(loaded.observe_codex))
        .manage(coding::history::CodingHistory::new(loaded.retain_coding_history))
        .manage(Pending::default())
        .manage(ChatState::default())
        .manage(DraftState::default())
        .manage(local_runtime::RuntimeState::default())
        .manage(local_intelligence::LocalIntelligence::default())
        .manage(local_intelligence::assistant::VoiceAssistant::default())
        .manage(local_intelligence::speaker::SpeakerState::default())
        .manage(voice::VoiceHub::default())
        .manage(computer::ComputerState::default())
        .manage(Roadeep::new())
        .manage(planner::Planner::open(planner::Planner::default_path()))
        // The OS-level drop, seen by Rust itself: the only source ingest_file trusts.
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                if window.label() == island::WINDOW_LABEL {
                    window.state::<Shared>().drops.record(paths);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            local_runtime::local_runtime_status,
            local_runtime::local_runtime_install,
            local_runtime::local_runtime_cancel,
            local_runtime::local_runtime_enable,
            local_intelligence::local_brain_chat,
            local_intelligence::local_brain_cancel,
            local_intelligence::local_speech_transcribe,
            local_intelligence::local_speech_speak,
            local_intelligence::speaker::speaker_status,
            local_intelligence::speaker::speaker_enrollment_begin,
            local_intelligence::speaker::speaker_enrollment_end,
            local_intelligence::speaker::speaker_enroll,
            local_intelligence::speaker::speaker_verify,
            local_intelligence::speaker::speaker_clear,
            voice::voice_status,
            voice::voice_configure,
            voice::voice_clear_key,
            voice::voice_start,
            voice::voice_end,
            voice::voice_app_tool,
            clock::clock_now,
            voice::memory::memory_get,
            voice::memory::memory_remember,
            voice::memory::memory_forget,
            voice::memory::memory_delete,
            voice::memory::memory_clear,
            voice::memory::memory_set_enabled,
            voice::memory::memory_record,
            computer::computer_setup,
            computer::computer_status,
            computer::computer_start,
            computer::computer_pause,
            computer::computer_resume,
            computer::computer_stop,
            computer::computer_reset,
            computer::computer_takeover,
            computer::computer_operate,
            boot,
            coding::coding_snapshot,
            coding::coding_clear,
            coding::coding_status,
            coding::export::coding_export,
            coding::history::coding_history_snapshot,
            coding::history::coding_history_save,
            coding::history::coding_history_clear,
            coding::git::coding_git_inspect,
            coding::handoff::coding_handoff_agents,
            coding::handoff::coding_handoff,
            save_settings,
            set_collapsed,
            set_island_rect,
            focus_window,
            reposition,
            dock_motion::drag_start,
            dock_motion::dock_layout_ready,
            dock_motion::dock_set,
            dock_motion::set_maximized,
            open_url,
            open_in_vscode,
            quit_app,
            coding_hooks::coding_hooks_status,
            coding_hooks::coding_hooks_preview,
            coding_hooks::coding_hooks_apply,
            migrate::legacy_relay_cleanup,
            hooks_status,
            hooks_preview,
            hooks_apply,
            mcp_status,
            mcp_preview,
            mcp_apply,
            approval_decision,
            approval_answer,
            approval_ack,
            approval_decline,
            log_line,
            chat_send,
            desktop_actions::desktop_voice_try,
            local_intelligence::assistant::voice_assistant_analyze,
            local_intelligence::assistant::voice_proposal_pending,
            local_intelligence::assistant::voice_proposal_decide,
            local_intelligence::assistant::voice_proposal_clear,
            chat_reset,
            chat_cancel,
            roadeep_login,
            roadeep_otp_send,
            roadeep_otp_verify,
            roadeep_logout,
            roadeep_session,
            roadeep_models,
            roadeep_agents,
            roadeep::chat::roadeep_profile_locks,
            roadeep::chat::roadeep_agent_catalog,
            roadeep::chat::agent_draft,
            roadeep::chat::agent_draft_cancel,
            roadeep::chat::chat_approval_decide,
            roadeep::chat::chat_turn_state,
            roadeep::model_choice::chat_model_set,
            roadeep::chat::chat_tools_state,
            roadeep::chat::chat_tools_set,
            roadeep::chat::roadeep_balance,
            roadeep::threads::chat_threads,
            roadeep::threads::chat_thread_open,
            roadeep::threads::chat_thread_delete,
            agents::local_agents_list,
            agents::local_agent_save,
            agents::local_agent_delete,
            ingest_file,
            secret_present,
            secret_public_value,
            secret_set,
            secret_clear,
            refresh_integration,
            test_integration,
            integration_status,
            catalog_list,
            open_n8n,
            open_settings_window,
            set_paused,
            shortcut::shortcut_status,
            shortcut::shortcut_check,
            shortcuts::shortcuts_status,
            shortcuts::shortcuts_suspend,
            shortcuts::session_window::open_session,
            updater::update_status,
            updater::update_check,
            updater::update_install,
            mcpc::mcpc_list,
            mcpc::mcpc_add,
            mcpc::mcpc_update,
            mcpc::mcpc_remove,
            mcpc::mcpc_set_secret,
            mcpc::mcpc_clear_secret,
            mcpc::mcpc_approve_command,
            mcpc::mcpc_connect,
            mcpc::mcpc_tools,
            mcpc::mcpc_set_tool_mode,
            mcpc::oauth::mcpc_oauth_start,
            mcpc::oauth::mcpc_oauth_signout,
            mcpc::directory::mcpc_directory,
            planner::planner_get,
            planner::planner_task_add,
            planner::planner_task_update,
            planner::planner_task_delete,
            planner::planner_note_add,
            planner::planner_note_update,
            planner::planner_note_delete,
            planner::planner_reminder_add,
            planner::planner_reminder_update,
            planner::planner_reminder_delete,
            planner::planner_reminder_snooze,
            planner::planner_habit_add,
            planner::planner_habit_update,
            planner::planner_habit_delete,
            planner::planner_habit_check,
            planner::focus_start,
            planner::focus_pause,
            planner::focus_resume,
            planner::focus_skip,
            planner::focus_stop,
            planner::focus_get,
            planner::planner_pending_fires,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            tray::build(&handle)?;
            shortcut::apply(&handle, &loaded.shortcut);
            // Before the island: see create_settings_window.
            create_settings_window(&handle);

            if let Some(win) = island::window(&handle) {
                island::make_non_activating(&win);
                island::apply_geometry(&handle, &loaded, false, false);
                let _ = win.show();
            }
            gate.collapsed.store(false, Ordering::Relaxed);
            gate.set_active(true);
            island::spawn_cursor_poll(handle.clone(), gate.clone());

            log::line(format!("--- Roadeep {} started ---", env!("CARGO_PKG_VERSION")));
            handle.state::<Roadeep>().attach(handle.clone());
            for key in secrets::purge_legacy() {
                log::line(format!("secrets: removed legacy credential {key}"));
            }
            hooks::ensure_hook_exe(&handle);
            pipe::start(handle.clone());
            mcp::install::ensure_mcp_exe(&handle);
            for line in migrate::migrate_autostart(&migrate::SystemStartup(&handle), loaded.autostart) {
                log::line(line);
            }
            mcp::start(handle.clone());
            mcpc::start(handle.clone());
            voice::start_prefetch(&handle);
            planner::start(handle.clone());
            coding::start(handle.clone());
            coding::history::start(handle.clone());
            updater::start(&handle);
            integrations::start(handle.clone());
            local_runtime::prepare_bundled(handle.clone());
            Ok(())
        })
        .build(updater::with_config(tauri::generate_context!()))
        .expect("error while running Roadeep")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<local_runtime::RuntimeState>().shutdown();
                app.state::<local_intelligence::LocalIntelligence>().shutdown();
                shortcut::release(app);
                mcpc::shutdown();
                let voice = app.state::<voice::VoiceHub>();
                let computer = app.state::<computer::ComputerState>();
                tauri::async_runtime::block_on(async {
                    let cleanup = async {
                        let (voice_result, ()) = tokio::join!(voice.shutdown(), computer.shutdown());
                        if voice_result.is_err() { log::line("voice shutdown failed"); }
                    };
                    if tokio::time::timeout(Duration::from_secs(3), cleanup).await.is_err() {
                        log::line("assistant shutdown deadline elapsed; remote cleanup could not be confirmed");
                    }
                });
            }
        });
}

/// Internal child process path: no Tauri bootstrap or network is initialized.
pub fn internal_speaker_worker() -> i32 {local_intelligence::internal_speaker_worker()}
