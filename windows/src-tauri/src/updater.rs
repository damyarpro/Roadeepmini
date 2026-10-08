// Auto-update from GitHub Releases (tauri-plugin-updater), opt-in at build time.
//
// The build turns it on by setting two environment variables, read here with
// `option_env!` (see windows/RELEASING.md):
//
//     ROADEEP_UPDATE_URL     https URL of latest.json, e.g.
//                            https://github.com/<owner>/<repo>/releases/latest/download/latest.json
//     ROADEEP_UPDATE_PUBKEY  the updater public key (`npx tauri signer generate`)
//
// Without both, every command answers `enabled: false` and nothing touches the
// network. With both, the app only ever CHECKS on its own (once, ~1 min after
// start, at most every 24 h, when Settings → autoUpdateCheck is on); a found
// update raises "update-available" and waits. Downloading and installing only
// happen from the explicit click that calls `update_install`.
//
// GitHub is reached the usual way, system proxy included — unlike Roadeep's own
// traffic (roadeep::http::proxy_policy), which goes direct.
//
// Installing runs the signed NSIS installer in passive mode with /UPDATE (the
// old version is not uninstalled first, so the NSIS uninstall hook that clears
// %LOCALAPPDATA%\com.roadeep.desktop\bin does not run) and /R (relaunch). The installer only
// writes into the install directory; the relays Claude Code may be running live
// in %LOCALAPPDATA%\com.roadeep.desktop\bin and are restaged at the next launch by
// ensure_hook_exe / ensure_mcp_exe, which keep the old copy when it is in use.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{errors, log, settings};

const BUILD_URL: Option<&str> = option_env!("ROADEEP_UPDATE_URL");
const BUILD_PUBKEY: Option<&str> = option_env!("ROADEEP_UPDATE_PUBKEY");

/// The automatic check waits this long after launch, so it never competes with start-up.
const STARTUP_DELAY: Duration = Duration::from_secs(60);
/// At most one automatic check per day (manual checks count too).
const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;
/// latest.json is tiny; a proxy that takes longer than this is not going to answer.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// The installer is ~10 MB and may come through a slow proxy (this is a total, not idle, timeout).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Release notes are shown as an excerpt; anything longer is cut here.
const MAX_NOTES_CHARS: usize = 4000;
const LAST_CHECK_FILE: &str = "update-check";

pub const AVAILABLE_EVENT: &str = "update-available";
pub const PROGRESS_EVENT: &str = "update-progress";

// ── Build-time configuration ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct BuildConfig {
    pub url: Url,
    pub pubkey: String,
}

/// Neither variable set = disabled (Ok(None)); anything half-set or unsafe is an
/// error, which also leaves the updater disabled but says why in the log.
pub fn validate(url: Option<&str>, pubkey: Option<&str>) -> Result<Option<BuildConfig>, String> {
    let url = url.map(str::trim).filter(|s| !s.is_empty());
    let pubkey = pubkey.map(str::trim).filter(|s| !s.is_empty());
    let (url, pubkey) = match (url, pubkey) {
        (None, None) => return Ok(None),
        (Some(u), Some(k)) => (u, k),
        (Some(_), None) => return Err("ROADEEP_UPDATE_URL is set but ROADEEP_UPDATE_PUBKEY is not".into()),
        (None, Some(_)) => return Err("ROADEEP_UPDATE_PUBKEY is set but ROADEEP_UPDATE_URL is not".into()),
    };
    if url.len() > 2048 {
        return Err("ROADEEP_UPDATE_URL is too long".into());
    }
    let parsed = Url::parse(url).map_err(|e| format!("ROADEEP_UPDATE_URL is not a URL: {e}"))?;
    if parsed.scheme() != "https" {
        return Err("ROADEEP_UPDATE_URL must use https".into());
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err("ROADEEP_UPDATE_URL has no host".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("ROADEEP_UPDATE_URL must not carry credentials".into());
    }
    // `tauri signer generate` prints the key as one base64 line.
    let base64 = pubkey.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='));
    if !base64 || !(40..=2048).contains(&pubkey.len()) {
        return Err("ROADEEP_UPDATE_PUBKEY is not a base64 updater public key".into());
    }
    Ok(Some(BuildConfig { url: parsed, pubkey: pubkey.to_string() }))
}

fn build_config() -> Option<BuildConfig> {
    validate(BUILD_URL, BUILD_PUBKEY).ok().flatten()
}

/// The plugin reads `plugins.updater` when it starts and refuses to start
/// without a `pubkey`. It is injected here rather than written in
/// tauri.conf.json, so a build without the variables needs no key at all.
pub fn with_config<R: tauri::Runtime>(mut context: tauri::Context<R>) -> tauri::Context<R> {
    let pubkey = build_config().map(|c| c.pubkey).unwrap_or_default();
    context.config_mut().plugins.0.insert("updater".into(), plugin_config(&pubkey));
    context
}

fn plugin_config(pubkey: &str) -> serde_json::Value {
    serde_json::json!({
        "pubkey": pubkey,
        // Every release is signed by a CLI that records its version in the
        // signature, so a tampered latest.json cannot pair a newer version
        // number with an older (genuinely signed) installer.
        "requireSignedVersion": true,
        "windows": { "installMode": "passive" },
    })
}

// ── Status (a small state machine, tested below) ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    pub version: String,
    /// Release notes as plain text (the UI renders them with textContent).
    pub notes: Option<String>,
    /// Publication date, Unix milliseconds.
    pub date: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub enabled: bool,
    pub current_version: String,
    pub checking: bool,
    pub available: Option<Available>,
    pub downloaded: bool,
    /// 0..=1 while downloading (and 1 once downloaded); None otherwise.
    pub progress: Option<f32>,
    /// Unix milliseconds of the last finished check.
    pub last_checked_at: Option<u64>,
    /// An E_UPDATE_* code (errors.rs).
    pub error: Option<String>,
}

impl UpdateStatus {
    fn new(enabled: bool, current_version: String) -> Self {
        Self {
            enabled,
            current_version,
            checking: false,
            available: None,
            downloaded: false,
            progress: None,
            last_checked_at: None,
            error: None,
        }
    }

    fn downloading(&self) -> bool {
        self.progress.is_some() && !self.downloaded
    }

    /// False when a check or a download is already running.
    fn begin_check(&mut self) -> bool {
        if !self.enabled || self.checking || self.downloading() {
            return false;
        }
        self.checking = true;
        self.error = None;
        true
    }

    fn finish_check(&mut self, result: Result<Option<Available>, String>, now_ms: u64) {
        self.checking = false;
        self.last_checked_at = Some(now_ms);
        match result {
            Ok(found) => {
                if found.as_ref().map(|a| &a.version) != self.available.as_ref().map(|a| &a.version) {
                    self.downloaded = false;
                    self.progress = None;
                }
                self.available = found;
            }
            // A failed check keeps what an earlier one found.
            Err(code) => self.error = Some(code),
        }
    }

    fn begin_download(&mut self) -> Result<(), String> {
        if !self.enabled {
            return Err(errors::UPDATE_DISABLED.into());
        }
        if self.available.is_none() {
            return Err(errors::UPDATE_NOTHING.into());
        }
        if self.checking || self.downloading() {
            return Err(errors::UPDATE_BUSY.into());
        }
        self.error = None;
        self.downloaded = false;
        self.progress = Some(0.0);
        Ok(())
    }

    /// Returns true when the visible progress moved by a whole percent (worth an event).
    fn advance(&mut self, done: u64, total: Option<u64>) -> bool {
        let Some(total) = total.filter(|t| *t > 0) else { return false };
        let next = (done as f64 / total as f64).clamp(0.0, 1.0) as f32;
        let before = self.progress.unwrap_or(0.0);
        self.progress = Some(next);
        (next * 100.0).floor() > (before * 100.0).floor()
    }

    fn finish_download(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.downloaded = true;
                self.progress = Some(1.0);
            }
            Err(code) => {
                self.downloaded = false;
                self.progress = None;
                self.error = Some(code);
            }
        }
    }

    /// The installer could not be started: the download is kept, the click can be retried.
    fn install_failed(&mut self, code: String) {
        self.error = Some(code);
    }
}

/// Whether the automatic check should run now.
pub fn auto_check_due(setting_on: bool, last_checked_secs: Option<u64>, now_secs: u64) -> bool {
    if !setting_on {
        return false;
    }
    match last_checked_secs {
        None => true,
        // A clock that went backwards must not silence the check for good.
        Some(last) if last > now_secs => true,
        Some(last) => now_secs - last >= CHECK_EVERY_SECS,
    }
}

/// When to wake for the single automatic check of this session: a minute after
/// launch, or later if the last check is less than a day old.
pub fn auto_check_delay(last_checked_secs: Option<u64>, now_secs: u64) -> Duration {
    let due_in = match last_checked_secs {
        Some(last) if last <= now_secs => (last + CHECK_EVERY_SECS).saturating_sub(now_secs),
        _ => 0,
    };
    Duration::from_secs(due_in).max(STARTUP_DELAY)
}

// ── Runtime ───────────────────────────────────────────────────────────────────

pub struct UpdateState {
    status: Mutex<UpdateStatus>,
    /// The update found by the last check, kept for the install click.
    pending: Mutex<Option<Update>>,
    /// The verified installer, kept so a failed install start can be retried without downloading again.
    bytes: Mutex<Option<Vec<u8>>>,
    config: Option<BuildConfig>,
}

impl Default for UpdateState {
    fn default() -> Self {
        let config = build_config();
        Self {
            status: Mutex::new(UpdateStatus::new(config.is_some(), env!("CARGO_PKG_VERSION").to_string())),
            pending: Mutex::new(None),
            bytes: Mutex::new(None),
            config,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    downloaded: u64,
    total: Option<u64>,
    progress: Option<f32>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn last_check_path() -> std::path::PathBuf {
    settings::local_dir().join(LAST_CHECK_FILE)
}

fn read_last_check() -> Option<u64> {
    std::fs::read_to_string(last_check_path()).ok()?.trim().parse().ok()
}

fn write_last_check(secs: u64) {
    if let Err(err) = std::fs::write(last_check_path(), secs.to_string()) {
        log::line(format!("update: could not record the check time: {err}"));
    }
}

/// Maps a plugin error to one of our codes. Only the error's kind is logged by
/// the callers, never a URL beyond the configured one.
fn code_for(err: &tauri_plugin_updater::Error) -> &'static str {
    use tauri_plugin_updater::Error as E;
    match err {
        E::Reqwest(e) if e.is_timeout() => errors::UPDATE_TIMEOUT,
        E::Reqwest(_) | E::Network(_) => errors::UPDATE_NETWORK,
        E::ReleaseNotFound => errors::UPDATE_NO_RELEASE,
        E::Serialization(_) | E::Semver(_) | E::UrlParse(_) | E::TargetNotFound(_) | E::TargetsNotFound(_) => {
            errors::UPDATE_MANIFEST
        }
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => errors::UPDATE_SIGNATURE,
        _ => errors::UPDATE_FAILED,
    }
}

fn notes_excerpt(body: Option<&str>) -> Option<String> {
    let body = body?.trim();
    if body.is_empty() {
        return None;
    }
    Some(body.chars().take(MAX_NOTES_CHARS).collect())
}

/// Logs why the updater is off (a half-set or unsafe build) and schedules the
/// one automatic check of this session. One sleeping task, no interval.
pub fn start(app: &AppHandle) {
    match validate(BUILD_URL, BUILD_PUBKEY) {
        Ok(None) => return,
        Err(reason) => {
            log::line(format!("update: disabled — {reason}"));
            return;
        }
        Ok(Some(_)) => {}
    }
    let delay = auto_check_delay(read_last_check(), now_secs());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let setting_on = app.state::<crate::Shared>().settings.lock().unwrap().auto_update_check;
        if !auto_check_due(setting_on, read_last_check(), now_secs()) {
            return;
        }
        run_check(&app).await;
    });
}

async fn run_check(app: &AppHandle) -> UpdateStatus {
    let state = app.state::<UpdateState>();
    let Some(config) = state.config.clone() else {
        return state.status.lock().unwrap().clone();
    };
    if !state.status.lock().unwrap().begin_check() {
        return state.status.lock().unwrap().clone();
    }

    let result = match app
        .updater_builder()
        .endpoints(vec![config.url.clone()])
        .map(|b| b.pubkey(config.pubkey.clone()).timeout(CHECK_TIMEOUT))
        .and_then(|b| b.build())
    {
        Ok(updater) => updater.check().await,
        Err(err) => Err(err),
    };

    let now = now_secs();
    write_last_check(now);
    let outcome = match result {
        Ok(Some(update)) => {
            let found = Available {
                version: update.version.clone(),
                notes: notes_excerpt(update.body.as_deref()),
                date: update.date.map(|d| d.unix_timestamp() * 1000),
            };
            log::line(format!("update: version {} is available", found.version));
            let changed = state.pending.lock().unwrap().as_ref().map(|u| u.version != update.version).unwrap_or(true);
            if changed {
                *state.bytes.lock().unwrap() = None;
            }
            *state.pending.lock().unwrap() = Some(update);
            Ok(Some(found))
        }
        Ok(None) => {
            *state.pending.lock().unwrap() = None;
            *state.bytes.lock().unwrap() = None;
            log::line("update: up to date");
            Ok(None)
        }
        Err(err) => {
            let code = code_for(&err);
            log::line(format!("update: check failed ({code}): {err}"));
            Err(code.to_string())
        }
    };

    let status = {
        let mut status = state.status.lock().unwrap();
        status.finish_check(outcome, now * 1000);
        status.clone()
    };
    if let Some(available) = &status.available {
        let _ = app.emit(AVAILABLE_EVENT, available.clone());
    }
    status
}

#[tauri::command]
pub fn update_status(state: State<UpdateState>) -> UpdateStatus {
    state.status.lock().unwrap().clone()
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateStatus {
    run_check(&app).await
}

/// Only ever called from an explicit click: downloads the installer, verifies
/// its signature, then starts it and quits (the installer relaunches the app).
/// Resolves only with an error code; on success the process is gone.
#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, UpdateState>) -> Result<(), String> {
    let Some(mut update) = state.pending.lock().unwrap().clone() else {
        let code = if state.config.is_some() { errors::UPDATE_NOTHING } else { errors::UPDATE_DISABLED };
        return Err(code.into());
    };

    let cached = state.bytes.lock().unwrap().clone();
    let bytes = match cached {
        Some(bytes) => bytes,
        None => {
            state.status.lock().unwrap().begin_download()?;
            log::line(format!("update: downloading {}", update.version));
            update.timeout = Some(DOWNLOAD_TIMEOUT);
            let mut done: u64 = 0;
            let result = update
                .download(
                    |chunk, total| {
                        done += chunk as u64;
                        let mut status = state.status.lock().unwrap();
                        if status.advance(done, total) {
                            let progress = status.progress;
                            drop(status);
                            let _ = app.emit(PROGRESS_EVENT, ProgressEvent { downloaded: done, total, progress });
                        }
                    },
                    || {},
                )
                .await;
            match result {
                Ok(bytes) => {
                    state.status.lock().unwrap().finish_download(Ok(()));
                    let _ = app.emit(PROGRESS_EVENT, ProgressEvent { downloaded: done, total: Some(done), progress: Some(1.0) });
                    *state.bytes.lock().unwrap() = Some(bytes.clone());
                    bytes
                }
                Err(err) => {
                    let code = code_for(&err);
                    log::line(format!("update: download failed ({code}): {err}"));
                    state.status.lock().unwrap().finish_download(Err(code.to_string()));
                    return Err(code.into());
                }
            }
        }
    };

    log::line(format!("update: starting the installer for {}", update.version));
    // On Windows this launches the installer and exits the process.
    if let Err(err) = update.install(bytes) {
        log::line(format!("update: the installer could not start: {err}"));
        state.status.lock().unwrap().install_failed(errors::UPDATE_INSTALL.into());
        return Err(errors::UPDATE_INSTALL.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEFCQ0RFRgpSV1FBQkNERUY=";

    fn found(v: &str) -> Option<Available> {
        Some(Available { version: v.into(), notes: None, date: None })
    }

    #[test]
    fn nothing_set_means_disabled_without_error() {
        assert_eq!(validate(None, None), Ok(None));
        assert_eq!(validate(Some("  "), Some("")), Ok(None));
    }

    #[test]
    fn both_set_and_https_enables() {
        let url = "https://github.com/acme/roadeep/releases/latest/download/latest.json";
        let cfg = validate(Some(url), Some(KEY)).unwrap().unwrap();
        assert_eq!(cfg.url.as_str(), url);
        assert_eq!(cfg.pubkey, KEY);
    }

    #[test]
    fn half_set_or_unsafe_urls_are_refused() {
        assert!(validate(Some("https://example.com/latest.json"), None).is_err());
        assert!(validate(None, Some(KEY)).is_err());
        assert!(validate(Some("http://example.com/latest.json"), Some(KEY)).is_err(), "http");
        assert!(validate(Some("file:///C:/latest.json"), Some(KEY)).is_err(), "file");
        assert!(validate(Some("not a url"), Some(KEY)).is_err());
        assert!(validate(Some("https://user:pw@example.com/latest.json"), Some(KEY)).is_err(), "credentials");
        assert!(validate(Some("https://example.com/latest.json"), Some("not base64 !")).is_err());
        assert!(validate(Some("https://example.com/latest.json"), Some("abc")).is_err(), "too short");
    }

    #[test]
    fn disabled_status_never_checks_or_downloads() {
        let mut s = UpdateStatus::new(false, "0.1.1".into());
        assert!(!s.begin_check());
        assert_eq!(s.begin_download(), Err(errors::UPDATE_DISABLED.to_string()));
    }

    #[test]
    fn check_then_download_then_install_failure() {
        let mut s = UpdateStatus::new(true, "0.1.1".into());
        assert_eq!(s.begin_download(), Err(errors::UPDATE_NOTHING.to_string()), "nothing found yet");
        assert!(s.begin_check());
        assert!(!s.begin_check(), "one check at a time");
        s.finish_check(Ok(found("0.2.0")), 1_000);
        assert!(!s.checking);
        assert_eq!(s.last_checked_at, Some(1_000));
        assert_eq!(s.available.as_ref().unwrap().version, "0.2.0");

        s.begin_download().unwrap();
        assert!(s.downloading());
        assert!(!s.begin_check(), "no check while downloading");
        assert_eq!(s.begin_download(), Err(errors::UPDATE_BUSY.to_string()));
        assert!(s.advance(50, Some(100)));
        assert!(!s.advance(50, Some(100)), "same percent, no event");
        assert!(!s.advance(10, None), "unknown size, no progress");
        s.finish_download(Ok(()));
        assert!(s.downloaded && !s.downloading());
        assert_eq!(s.progress, Some(1.0));

        s.install_failed(errors::UPDATE_INSTALL.into());
        assert_eq!(s.error.as_deref(), Some(errors::UPDATE_INSTALL));
        assert!(s.downloaded, "the verified download is kept for a retry");
        assert!(s.begin_check(), "a new check clears the error");
        assert_eq!(s.error, None);
    }

    #[test]
    fn failed_download_and_failed_check() {
        let mut s = UpdateStatus::new(true, "0.1.1".into());
        assert!(s.begin_check());
        s.finish_check(Ok(found("0.2.0")), 1);
        s.begin_download().unwrap();
        s.finish_download(Err(errors::UPDATE_SIGNATURE.into()));
        assert_eq!(s.progress, None);
        assert!(!s.downloaded);
        assert_eq!(s.error.as_deref(), Some(errors::UPDATE_SIGNATURE));

        assert!(s.begin_check());
        s.finish_check(Err(errors::UPDATE_NETWORK.into()), 2);
        assert_eq!(s.available.as_ref().unwrap().version, "0.2.0", "a failed check keeps the last result");
        assert_eq!(s.error.as_deref(), Some(errors::UPDATE_NETWORK));
    }

    #[test]
    fn a_newer_release_resets_the_download() {
        let mut s = UpdateStatus::new(true, "0.1.1".into());
        s.begin_check();
        s.finish_check(Ok(found("0.2.0")), 1);
        s.begin_download().unwrap();
        s.finish_download(Ok(()));
        s.begin_check();
        s.finish_check(Ok(found("0.2.0")), 2);
        assert!(s.downloaded, "same version keeps the download");
        s.begin_check();
        s.finish_check(Ok(found("0.3.0")), 3);
        assert!(!s.downloaded && s.progress.is_none());
        s.begin_check();
        s.finish_check(Ok(None), 4);
        assert_eq!(s.available, None);
    }

    #[test]
    fn automatic_check_runs_at_most_once_a_day() {
        let now = 1_700_000_000;
        assert!(!auto_check_due(false, None, now), "setting off");
        assert!(auto_check_due(true, None, now), "never checked");
        assert!(!auto_check_due(true, Some(now - 3600), now));
        assert!(!auto_check_due(true, Some(now - CHECK_EVERY_SECS + 1), now));
        assert!(auto_check_due(true, Some(now - CHECK_EVERY_SECS), now));
        assert!(auto_check_due(true, Some(now + 3600), now), "clock went backwards");
    }

    #[test]
    fn automatic_check_waits_a_minute_or_until_a_day_has_passed() {
        let now = 1_700_000_000;
        assert_eq!(auto_check_delay(None, now), STARTUP_DELAY);
        assert_eq!(auto_check_delay(Some(now - 2 * CHECK_EVERY_SECS), now), STARTUP_DELAY);
        assert_eq!(auto_check_delay(Some(now - 3600), now), Duration::from_secs(CHECK_EVERY_SECS - 3600));
        assert_eq!(auto_check_delay(Some(now - CHECK_EVERY_SECS + 10), now), STARTUP_DELAY);
        assert_eq!(auto_check_delay(Some(now + 50), now), STARTUP_DELAY);
    }

    #[test]
    fn injected_plugin_config_always_loads() {
        // The plugin refuses to start on a config it cannot read, which would take the app down.
        for key in ["", KEY] {
            let cfg: tauri_plugin_updater::Config = serde_json::from_value(plugin_config(key)).unwrap();
            assert_eq!(cfg.pubkey, key);
            assert!(cfg.require_signed_version);
            assert!(cfg.endpoints.is_empty(), "endpoints are given per check");
            let mode = format!("{:?}", cfg.windows.unwrap().install_mode);
            assert_eq!(mode, "Passive");
        }
    }

    #[test]
    fn notes_are_trimmed_and_capped() {
        assert_eq!(notes_excerpt(None), None);
        assert_eq!(notes_excerpt(Some("  \n ")), None);
        assert_eq!(notes_excerpt(Some(" Fixes ")).as_deref(), Some("Fixes"));
        let long = "x".repeat(MAX_NOTES_CHARS + 50);
        assert_eq!(notes_excerpt(Some(&long)).unwrap().chars().count(), MAX_NOTES_CHARS);
    }
}
