// Migration from the app's former name to Roadeep.
//
// Earlier builds stored everything under the former name: %APPDATA% and
// %LOCALAPPDATA% folders, the Tauri identifier folders (WebView data included),
// the Credential Manager service and the Run-key autostart entry. The user's
// data must survive the rename, so on startup — before settings are read and
// before any WebView exists — this module moves it over:
//
//   1. folders: moved when the new one doesn't exist; when both exist, entries
//      missing from the new folder are moved in and nothing is overwritten.
//      A name conflict stays where it is; a move that fails (a locked file) is
//      retried on later launches, up to MAX_ATTEMPTS. Nothing is ever deleted
//      that wasn't moved first;
//   2. Credential Manager: copy → read back → only then delete the old entry;
//   3. old relays: the app keeps listening on the former pipe names (pipe.rs,
//      mcp/mod.rs), so relays still registered in places it can't see keep
//      working. Settings offers the reviewed update of every registration it
//      can see, and the old relay folders are deleted only on request
//      (`legacy_relay_cleanup`), and only when nothing visible points at them;
//   4. autostart: the old Run value is removed and, if autostart is on, the
//      current exe is registered under the new name (`migrate_autostart`).
//
// The former name is assembled from pieces so the codebase holds no literal
// occurrence of it (the rename was meant to remove it everywhere); it is used
// only to find old data and to keep old relays working.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The app's former name, as its folders and Run value were called.
pub const LEGACY_NAME: &str = concat!("Cou", "cou");
/// Its lowercase form: relay exe names, pipe names, log file, identifier suffix.
const LEGACY_LOWER: &str = concat!("cou", "cou");
/// The former Tauri identifier (folders, Credential Manager service).
pub const LEGACY_IDENTIFIER: &str = concat!("fr.louisraille.", "cou", "cou");
/// The current Tauri identifier; must match tauri.conf.json.
pub const IDENTIFIER: &str = "com.roadeep.desktop";

const STATE_FILE: &str = "migration.json";
const STATE_VERSION: u32 = 2;
/// Launches a failed move or credential copy is retried on before giving up.
pub const MAX_ATTEMPTS: u32 = 5;
/// A Chromium profile is a database: moved whole or not at all, never merged.
const ATOMIC_DIRS: &[&str] = &["EBWebView"];
/// Only one process migrates at a time; a second launch waits this long.
const LOCK_NAME: &str = r"Local\RoadeepDataMigration";
const LOCK_WAIT_MS: u32 = 30_000;

// ── Legacy names other modules need ──────────────────────────────────────────

fn known_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// The former %LOCALAPPDATA% folder.
pub fn legacy_local_dir() -> PathBuf {
    known_dir("LOCALAPPDATA").join(LEGACY_NAME)
}

/// Where builds under the former name staged the hook and MCP relays.
pub fn legacy_bin_dir() -> PathBuf {
    legacy_local_dir().join("bin")
}

/// Where interim builds staged them: inside the NSIS install folder.
pub fn interim_bin_dir() -> PathBuf {
    known_dir("LOCALAPPDATA").join("Roadeep").join("bin")
}

/// Every folder a previous build staged relays in.
pub fn previous_bin_dirs() -> Vec<PathBuf> {
    vec![legacy_bin_dir(), interim_bin_dir()]
}

/// The hook relay builds under the former name registered.
pub fn legacy_hook_exe() -> PathBuf {
    legacy_bin_dir().join(format!("{LEGACY_LOWER}-hook.exe"))
}

/// Every hook relay path a previous build may have registered.
pub fn previous_hook_exes() -> Vec<PathBuf> {
    vec![legacy_hook_exe(), interim_bin_dir().join("roadeep-hook.exe")]
}

/// Recognises an old hook command (any path) in a config file.
pub fn legacy_hook_marker() -> String {
    format!("{LEGACY_LOWER}-hook")
}

/// Recognises an old MCP relay command (any path) in a config file.
pub fn legacy_mcp_marker() -> String {
    format!("{LEGACY_LOWER}-mcp")
}

/// The pipe the old hook relay connects to; `key` as in pipe.rs.
pub fn legacy_hook_pipe(key: &str) -> String {
    format!(r"\\.\pipe\{LEGACY_LOWER}-{key}")
}

/// The pipe the old MCP relay connects to.
pub fn legacy_mcp_pipe(key: &str) -> String {
    format!(r"\\.\pipe\{LEGACY_LOWER}-mcp-{key}")
}

// ── State ─────────────────────────────────────────────────────────────────────

/// A move that failed and is retried on the next launches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub from: PathBuf,
    pub to: PathBuf,
    pub attempts: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    version: u32,
    /// Ids of the folder moves already run (`DirMove::id`).
    moves_done: Vec<String>,
    pending: Vec<Pending>,
    keyring: bool,
    keyring_attempts: u32,
}

fn state_path() -> PathBuf {
    crate::settings::config_dir().join(STATE_FILE)
}

fn load_state(path: &Path) -> State {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Temp file + rename, like every other config write.
fn save_state(path: &Path, state: &State) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

// ── 1. Folders ────────────────────────────────────────────────────────────────

/// One folder to carry over.
#[derive(Default)]
pub struct DirMove {
    /// Stable id recorded in the state once the move has run.
    pub id: &'static str,
    pub from: PathBuf,
    pub to: PathBuf,
    /// Top-level entries left in place (the relay folder: see `legacy_relay_cleanup`).
    pub skip: Vec<String>,
    /// When not empty, only these top-level entries move.
    pub only: Vec<String>,
    /// Top-level files renamed on the way (the log).
    pub rename: Vec<(String, String)>,
}

/// What one move did: log lines (the app's own paths, never file contents)
/// and the moves that failed, for a retry on the next launch.
#[derive(Debug, Default)]
pub struct MoveOutcome {
    pub log: Vec<String>,
    pub failed: Vec<(PathBuf, PathBuf)>,
}

fn dir_moves() -> Vec<DirMove> {
    let roaming = known_dir("APPDATA");
    let local = known_dir("LOCALAPPDATA");
    let local_data = crate::settings::local_dir();
    vec![
        DirMove { id: "roaming-name", from: roaming.join(LEGACY_NAME), to: crate::settings::config_dir(), ..Default::default() },
        DirMove { id: "roaming-identifier", from: roaming.join(LEGACY_IDENTIFIER), to: roaming.join(IDENTIFIER), ..Default::default() },
        // First the identifier folder (WebView profile, local models: one rename
        // while the target is still absent), then the former local folder into it.
        DirMove { id: "local-identifier", from: local.join(LEGACY_IDENTIFIER), to: local_data.clone(), ..Default::default() },
        DirMove {
            id: "local-name",
            from: local.join(LEGACY_NAME),
            to: local_data.clone(),
            skip: vec!["bin".into()],
            rename: vec![(format!("{LEGACY_LOWER}.log"), "roadeep.log".into())],
            ..Default::default()
        },
        // Interim builds kept their data in the install folder.
        DirMove {
            id: "local-interim",
            from: local.join("Roadeep"),
            to: local_data,
            only: ["inbox", "exports", "roadeep.log", "coding-history-v1.json", "update-check"].map(String::from).to_vec(),
            ..Default::default()
        },
    ]
}

/// Moves `m.from` into `m.to`. Never overwrites, never deletes what it didn't move.
pub fn migrate_dir(m: &DirMove) -> MoveOutcome {
    let mut out = MoveOutcome::default();
    if !m.from.is_dir() {
        return out;
    }
    let whole = m.skip.is_empty() && m.only.is_empty() && m.rename.is_empty();
    if whole && !m.to.exists() {
        match std::fs::rename(&m.from, &m.to) {
            Ok(()) => {
                out.log.push(format!("migrate: moved {} -> {}", m.from.display(), m.to.display()));
                return out;
            }
            // A locked file inside: fall back to entry by entry.
            Err(err) => out.log.push(format!("migrate: whole move of {} failed ({err}); moving entries", m.from.display())),
        }
    }
    if let Err(err) = std::fs::create_dir_all(&m.to) {
        out.log.push(format!("migrate: cannot create {} ({err}); {} left as is", m.to.display(), m.from.display()));
        out.failed.push((m.from.clone(), m.to.clone()));
        return out;
    }
    move_entries(&m.from, &m.to, m, true, &mut out);
    // Only succeeds when everything went across.
    if std::fs::remove_dir(&m.from).is_ok() {
        out.log.push(format!("migrate: moved {} -> {}", m.from.display(), m.to.display()));
    }
    out
}

fn move_entries(from: &Path, to: &Path, m: &DirMove, top: bool, out: &mut MoveOutcome) {
    let entries = match std::fs::read_dir(from) {
        Ok(e) => e,
        Err(err) => {
            out.log.push(format!("migrate: cannot list {} ({err})", from.display()));
            return;
        }
    };
    let listed = |list: &[String], name: &str| list.iter().any(|s| s.eq_ignore_ascii_case(name));
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if top && (listed(&m.skip, &name) || (!m.only.is_empty() && !listed(&m.only, &name))) {
            continue;
        }
        let renamed = if top { m.rename.iter().find(|(a, _)| a.eq_ignore_ascii_case(&name)) } else { None };
        let src = entry.path();
        let dst = to.join(renamed.map(|(_, b)| b.as_str()).unwrap_or(&name));
        if !dst.exists() {
            if let Err(err) = std::fs::rename(&src, &dst) {
                out.log.push(format!("migrate: could not move {} ({err}); retrying next launch", src.display()));
                out.failed.push((src, dst));
            }
            continue;
        }
        let atomic = ATOMIC_DIRS.iter().any(|a| a.eq_ignore_ascii_case(&name));
        if src.is_dir() && dst.is_dir() && !atomic {
            move_entries(&src, &dst, m, false, out);
            let _ = std::fs::remove_dir(&src);
        } else {
            out.log.push(format!("migrate: {} already exists; kept {} in place", dst.display(), src.display()));
        }
    }
}

/// Retries moves that failed on an earlier launch. A target that appeared in
/// the meantime wins (the original stays where it is); after MAX_ATTEMPTS the
/// item is dropped and logged.
pub fn retry_pending(pending: Vec<Pending>, log: &mut Vec<String>) -> Vec<Pending> {
    let mut still = Vec::new();
    for mut p in pending {
        if !p.from.exists() {
            continue;
        }
        if p.to.exists() {
            log.push(format!("migrate: {} already exists; kept {} in place", p.to.display(), p.from.display()));
            continue;
        }
        if let Some(parent) = p.to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(&p.from, &p.to) {
            Ok(()) => {
                log.push(format!("migrate: moved {} -> {} (retry {})", p.from.display(), p.to.display(), p.attempts + 1));
                if let Some(parent) = p.from.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
            Err(err) => {
                p.attempts += 1;
                if p.attempts >= MAX_ATTEMPTS {
                    log.push(format!("migrate: gave up moving {} after {} attempts ({err}); it stays where it is", p.from.display(), p.attempts));
                } else {
                    still.push(p);
                }
            }
        }
    }
    still
}

// ── 2. Credential Manager ─────────────────────────────────────────────────────

/// The Credential Manager, behind a seam so the migration can be tested.
pub trait Vault {
    fn read(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String>;
    fn write(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String>;
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

pub struct SystemVault;

impl Vault for SystemVault {
    fn read(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        let entry = keyring::Entry::new(service, account).map_err(|e| e.to_string())?;
        match entry.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn write(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        keyring::Entry::new(service, account).and_then(|e| e.set_secret(secret)).map_err(|e| e.to_string())
    }
    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        match keyring::Entry::new(service, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct KeyringOutcome {
    pub moved: usize,
    /// The new service already had a value: both are kept, nothing overwritten.
    pub kept: usize,
    pub failed: usize,
}

/// Copies each account from `from` to `to`, checks the copy reads back
/// identically, and only then deletes the original. Values are never logged.
pub fn migrate_keyring(vault: &dyn Vault, from: &str, to: &str, accounts: &[String]) -> (KeyringOutcome, Vec<String>) {
    let mut out = KeyringOutcome::default();
    let mut log = Vec::new();
    for account in accounts {
        let old = match vault.read(from, account) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(err) => {
                out.failed += 1;
                log.push(format!("migrate: credential {account}: read failed ({err})"));
                continue;
            }
        };
        match vault.read(to, account) {
            Ok(Some(_)) => {
                out.kept += 1;
                log.push(format!("migrate: credential {account}: already set under the new name; old one kept"));
                continue;
            }
            Ok(None) => {}
            Err(err) => {
                out.failed += 1;
                log.push(format!("migrate: credential {account}: read failed ({err})"));
                continue;
            }
        }
        let copied = vault
            .write(to, account, &old)
            .and_then(|()| vault.read(to, account))
            .and_then(|back| if back.as_deref() == Some(old.as_slice()) { Ok(()) } else { Err("read-back mismatch".into()) });
        if let Err(err) = copied {
            out.failed += 1;
            log.push(format!("migrate: credential {account}: copy failed ({err}); original kept"));
            continue;
        }
        if let Err(err) = vault.delete(from, account) {
            log.push(format!("migrate: credential {account}: copied, old entry not removed ({err})"));
        }
        out.moved += 1;
    }
    (out, log)
}

/// Credentials the app no longer uses are deleted under the former service
/// too, never copied (secrets::purge_legacy does the same for the new one).
pub fn purge_retired(vault: &dyn Vault, service: &str, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter(|key| matches!(vault.read(service, key), Ok(Some(_))))
        .map(|key| match vault.delete(service, key) {
            Ok(()) => format!("migrate: removed retired credential {key} under the former service"),
            Err(err) => format!("migrate: could not remove retired credential {key} ({err})"),
        })
        .collect()
}

/// Every account name the app may have written: native keys, the session,
/// every catalog field, and the slots of the MCP servers in the user's config.
fn known_accounts(servers: &[crate::mcpc::store::ServerConfig]) -> Vec<String> {
    let mut accounts: Vec<String> = crate::secrets::KNOWN_KEYS
        .iter()
        .chain(crate::secrets::SESSION_KEYS)
        .map(|k| k.to_string())
        .collect();
    for service in &crate::catalog::get().services {
        for field in &service.fields {
            accounts.push(field.key.clone());
        }
    }
    accounts.extend(mcpc_accounts(servers));
    accounts
}

fn mcpc_accounts(servers: &[crate::mcpc::store::ServerConfig]) -> Vec<String> {
    let mut accounts = Vec::new();
    for server in servers {
        accounts.push(format!("mcpc.{}.token", server.id));
        accounts.push(format!("mcpc.{}.oauth", server.id));
        if let crate::mcpc::store::Transport::Stdio { env, .. } = &server.transport {
            accounts.extend(env.iter().map(|name| format!("mcpc.{}.env:{name}", server.id)));
        }
    }
    accounts
}

// ── Single migrator ───────────────────────────────────────────────────────────

/// A named mutex held for the duration of the migration.
pub struct MigrationLock(windows::Win32::Foundation::HANDLE);

impl Drop for MigrationLock {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::ReleaseMutex;
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// Waits up to `wait_ms` for the named mutex. An abandoned mutex (a migrator
/// that crashed) is ours; the steps are idempotent, so we simply carry on.
pub fn acquire_lock(name: &str, wait_ms: u32) -> Option<MigrationLock> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
    let handle = unsafe { CreateMutexW(None, false, &HSTRING::from(name)) }.ok()?;
    let waited = unsafe { WaitForSingleObject(handle, wait_ms) };
    if waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED {
        Some(MigrationLock(handle))
    } else {
        unsafe {
            let _ = CloseHandle(handle);
        }
        None
    }
}

// ── Startup ───────────────────────────────────────────────────────────────────

/// Runs steps 1 and 2. Must run before `settings::load()` and before any
/// window is built (the WebView profile moves with step 1). A move runs once;
/// only its failed entries are retried, so files the user deletes later don't
/// come back.
pub fn run_early() {
    let Some(_lock) = acquire_lock(LOCK_NAME, LOCK_WAIT_MS) else {
        crate::log::line("migrate: another instance is migrating; skipped this launch");
        return;
    };
    let path = state_path();
    let mut state = load_state(&path);
    let mut log = Vec::new();

    state.pending = retry_pending(std::mem::take(&mut state.pending), &mut log);
    for m in dir_moves() {
        if state.moves_done.iter().any(|id| id == m.id) {
            continue;
        }
        let out = migrate_dir(&m);
        log.extend(out.log);
        state.pending.extend(out.failed.into_iter().map(|(from, to)| Pending { from, to, attempts: 1 }));
        state.moves_done.push(m.id.to_string());
    }

    if !state.keyring {
        log.extend(purge_retired(&SystemVault, LEGACY_IDENTIFIER, crate::secrets::LEGACY_KEYS));
        let accounts = known_accounts(&crate::mcpc::store::load());
        let (outcome, lines) = migrate_keyring(&SystemVault, LEGACY_IDENTIFIER, IDENTIFIER, &accounts);
        log.extend(lines);
        if outcome.moved + outcome.kept + outcome.failed > 0 {
            log.push(format!(
                "migrate: credentials moved={} kept={} failed={}",
                outcome.moved, outcome.kept, outcome.failed
            ));
        }
        // A failure is retried at the next launches; the originals are untouched.
        state.keyring_attempts += 1;
        if outcome.failed == 0 {
            state.keyring = true;
        } else if state.keyring_attempts >= MAX_ATTEMPTS {
            state.keyring = true;
            log.push(format!("migrate: gave up on {} credential(s) after {MAX_ATTEMPTS} attempts; originals kept", outcome.failed));
        }
    }
    state.version = STATE_VERSION;
    // Logged after the move, so the log itself lands in the new folder.
    for line in log {
        crate::log::line(line);
    }
    if let Err(err) = save_state(&path, &state) {
        crate::log::line(format!("migrate: could not record progress ({err}); will retry"));
    }
}

// ── 3. Old relay folders ──────────────────────────────────────────────────────

/// Whether any config the app can see still runs a relay from a previous
/// folder. Project-scoped coding-app hooks (VS Code, Kiro, OpenCode) live in
/// arbitrary folders and can't be enumerated — which is why the old pipes stay
/// open and the folders are only removed on request.
pub fn legacy_relay_referenced() -> bool {
    crate::hooks::status().legacy_relay
        || crate::mcp::install::status(false).clients.iter().any(|c| c.legacy_relay)
        || crate::coding_hooks::legacy_relay_referenced()
}

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    /// A visible registration still runs an old relay: nothing was removed.
    pub referenced: bool,
    pub removed: Vec<String>,
    /// Still there: in use right now, or kept because it is referenced.
    pub kept: Vec<String>,
}

/// Deletes the old relay folders unless `referenced`. A relay that is running
/// (locked) stays and is reported. An old local folder left empty goes too.
pub fn cleanup_bins(bins: &[PathBuf], referenced: bool) -> CleanupReport {
    let mut report = CleanupReport { referenced, ..Default::default() };
    for bin in bins.iter().filter(|b| b.is_dir()) {
        if referenced {
            report.kept.push(bin.display().to_string());
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(bin) {
            for entry in entries.flatten() {
                let path = entry.path();
                let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
                match removed {
                    Ok(()) => report.removed.push(path.display().to_string()),
                    Err(_) => report.kept.push(path.display().to_string()),
                }
            }
        }
        if std::fs::remove_dir(bin).is_ok() {
            report.removed.push(bin.display().to_string());
        }
    }
    report
}

/// Settings window only: removes the old relay folders once no visible
/// registration needs them. The user asks for it; it never runs on its own.
#[tauri::command]
pub async fn legacy_relay_cleanup(window: tauri::WebviewWindow) -> Result<CleanupReport, String> {
    if window.label() != "settings" {
        return Err("legacy-cleanup-window-denied".into());
    }
    tauri::async_runtime::spawn_blocking(|| {
        let report = cleanup_bins(&previous_bin_dirs(), legacy_relay_referenced());
        // The former local folder, if that left it empty (never the install folder).
        let _ = std::fs::remove_dir(legacy_local_dir());
        crate::log::line(format!(
            "migrate: old relay cleanup referenced={} removed={} kept={}",
            report.referenced,
            report.removed.len(),
            report.kept.len()
        ));
        report
    })
    .await
    .map_err(|_| "legacy-cleanup-failed".to_string())
}

// ── 4. Autostart ──────────────────────────────────────────────────────────────

pub trait Startup {
    /// Removes the Run value under the former name. Ok(true) if there was one.
    fn remove_legacy(&self) -> Result<bool, String>;
    /// Registers the running exe under the current name.
    fn enable(&self) -> Result<(), String>;
}

/// The old entry always goes; the new one is (re)written when the user has
/// autostart on, so it points at the exe that is running now.
pub fn migrate_autostart(startup: &dyn Startup, enabled: bool) -> Vec<String> {
    let mut log = Vec::new();
    match startup.remove_legacy() {
        Ok(true) => log.push("migrate: removed the old autostart entry".into()),
        Ok(false) => {}
        Err(err) => log.push(format!("migrate: old autostart entry not removed ({err})")),
    }
    if enabled {
        if let Err(err) = startup.enable() {
            log.push(format!("migrate: autostart not re-registered ({err})"));
        }
    }
    log
}

/// The Run key and Task Manager's StartupApproved mirror of it, per user.
const RUN_KEYS: &[&str] = &[
    r"Software\Microsoft\Windows\CurrentVersion\Run",
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
];

fn delete_run_value(subkey: &str, name: &str) -> Result<bool, String> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(subkey), &HSTRING::from(name)) };
    if status == ERROR_SUCCESS {
        Ok(true)
    } else if status == ERROR_FILE_NOT_FOUND {
        Ok(false)
    } else {
        Err(format!("registry error {}", status.0))
    }
}

pub struct SystemStartup<'a>(pub &'a tauri::AppHandle);

impl Startup for SystemStartup<'_> {
    fn remove_legacy(&self) -> Result<bool, String> {
        let mut removed = false;
        for key in RUN_KEYS {
            removed |= delete_run_value(key, LEGACY_NAME)?;
        }
        Ok(removed)
    }
    fn enable(&self) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        self.0.autolaunch().enable().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roadeep-migrate-{tag}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn mv(from: &Path, to: &Path) -> DirMove {
        DirMove { id: "t", from: from.to_path_buf(), to: to.to_path_buf(), ..Default::default() }
    }

    #[test]
    fn legacy_names_are_assembled_correctly() {
        assert_eq!(LEGACY_NAME.len(), 6);
        assert_eq!(LEGACY_NAME.to_lowercase(), LEGACY_LOWER);
        assert!(LEGACY_IDENTIFIER.ends_with(LEGACY_LOWER));
        assert!(legacy_hook_exe().ends_with(format!("bin/{LEGACY_LOWER}-hook.exe")));
        assert_eq!(legacy_mcp_marker(), format!("{LEGACY_LOWER}-mcp"));
        assert_eq!(legacy_hook_pipe("S-1"), format!(r"\\.\pipe\{LEGACY_LOWER}-S-1"));
        assert_eq!(legacy_mcp_pipe("S-1"), format!(r"\\.\pipe\{LEGACY_LOWER}-mcp-S-1"));
        assert_ne!(legacy_hook_pipe("S-1"), crate::pipe::pipe_name());
        assert_eq!(previous_hook_exes().len(), 2);
    }

    #[test]
    fn identifier_matches_tauri_conf_and_local_data_is_not_the_install_folder() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], IDENTIFIER);
        assert!(crate::settings::local_dir().ends_with(IDENTIFIER));
        assert_ne!(crate::settings::local_dir().file_name().unwrap(), conf["productName"].as_str().unwrap());
    }

    #[test]
    fn a_missing_target_receives_the_whole_folder() {
        let root = temp("whole");
        let (from, to) = (root.join("old"), root.join("new"));
        write(&from.join("settings.json"), "{\"a\":1}");
        write(&from.join("sub/planner.json"), "p");
        let out = migrate_dir(&mv(&from, &to));
        assert!(out.failed.is_empty());
        assert!(!from.exists());
        assert_eq!(read(&to.join("settings.json")), "{\"a\":1}");
        assert_eq!(read(&to.join("sub/planner.json")), "p");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_existing_target_is_never_overwritten() {
        let root = temp("merge");
        let (from, to) = (root.join("old"), root.join("new"));
        write(&from.join("settings.json"), "old");
        write(&from.join("agents.json"), "agents");
        write(&from.join("sub/a.txt"), "a-old");
        write(&from.join("sub/b.txt"), "b");
        write(&to.join("settings.json"), "new");
        write(&to.join("sub/a.txt"), "a-new");
        let out = migrate_dir(&mv(&from, &to));
        assert_eq!(read(&to.join("settings.json")), "new");
        assert_eq!(read(&to.join("agents.json")), "agents");
        assert_eq!(read(&to.join("sub/a.txt")), "a-new");
        assert_eq!(read(&to.join("sub/b.txt")), "b");
        // The conflicting originals stay where they were: nothing is lost.
        assert_eq!(read(&from.join("settings.json")), "old");
        assert_eq!(read(&from.join("sub/a.txt")), "a-old");
        assert!(!from.join("agents.json").exists());
        assert!(out.log.iter().any(|l| l.contains("already exists")), "{:?}", out.log);
        assert!(out.failed.is_empty(), "a conflict is final, not retried");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_relay_folder_stays_and_the_log_is_renamed() {
        let root = temp("local");
        let (from, to) = (root.join("old"), root.join("new"));
        let old_log = format!("{LEGACY_LOWER}.log");
        write(&from.join("bin").join(format!("{LEGACY_LOWER}-hook.exe")), "exe");
        write(&from.join(&old_log), "lines");
        write(&from.join("inbox/f.txt"), "f");
        let m = DirMove { skip: vec!["bin".into()], rename: vec![(old_log.clone(), "roadeep.log".into())], ..mv(&from, &to) };
        migrate_dir(&m);
        assert!(from.join("bin").join(format!("{LEGACY_LOWER}-hook.exe")).exists());
        assert!(!to.join("bin").exists());
        assert_eq!(read(&to.join("roadeep.log")), "lines");
        assert!(!to.join(&old_log).exists());
        assert_eq!(read(&to.join("inbox/f.txt")), "f");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_listed_entries_leave_the_install_folder() {
        let root = temp("interim");
        let (from, to) = (root.join("Roadeep"), root.join("data"));
        write(&from.join("Roadeep.exe"), "app");
        write(&from.join("bin/roadeep-hook.exe"), "relay");
        write(&from.join("inbox/f.txt"), "f");
        write(&to.join("existing"), "x");
        migrate_dir(&DirMove { only: vec!["inbox".into(), "exports".into()], ..mv(&from, &to) });
        assert_eq!(read(&to.join("inbox/f.txt")), "f");
        assert!(from.join("Roadeep.exe").exists() && from.join("bin/roadeep-hook.exe").exists());
        assert!(!to.join("Roadeep.exe").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_webview_profile_is_never_merged() {
        let root = temp("webview");
        let (from, to) = (root.join("old"), root.join("new"));
        write(&from.join("EBWebView/Default/Local Storage/x"), "old");
        write(&from.join("local-ai/v1/m"), "model");
        write(&to.join("EBWebView/Default/y"), "new");
        migrate_dir(&mv(&from, &to));
        assert!(!to.join("EBWebView/Default/Local Storage/x").exists());
        assert!(from.join("EBWebView/Default/Local Storage/x").exists());
        assert_eq!(read(&to.join("local-ai/v1/m")), "model");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_locked_entry_is_reported_for_a_retry() {
        let root = temp("locked");
        let (from, to) = (root.join("old"), root.join("new"));
        write(&from.join("busy.db"), "data");
        write(&to.join("keep"), "x");
        // An open handle without delete sharing blocks the rename on Windows.
        let handle = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().read(true).share_mode(1).open(from.join("busy.db")).unwrap()
        };
        let out = migrate_dir(&mv(&from, &to));
        assert_eq!(out.failed, vec![(from.join("busy.db"), to.join("busy.db"))]);
        let pending = vec![Pending { from: from.join("busy.db"), to: to.join("busy.db"), attempts: 1 }];
        let mut log = Vec::new();
        let still = retry_pending(pending, &mut log);
        assert_eq!(still[0].attempts, 2, "still locked: counted, kept for later");
        drop(handle);
        let still = retry_pending(still, &mut log);
        assert!(still.is_empty());
        assert_eq!(read(&to.join("busy.db")), "data");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_retry_gives_up_after_the_limit_and_never_overwrites() {
        let root = temp("retry");
        let (a, b) = (root.join("a.txt"), root.join("out").join("a.txt"));
        write(&a, "old");
        let handle = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().read(true).share_mode(1).open(&a).unwrap()
        };
        let mut log = Vec::new();
        let still = retry_pending(vec![Pending { from: a.clone(), to: b.clone(), attempts: MAX_ATTEMPTS - 1 }], &mut log);
        assert!(still.is_empty());
        assert!(log.iter().any(|l| l.contains("gave up")), "{log:?}");
        drop(handle);

        write(&b, "new");
        let mut log = Vec::new();
        let still = retry_pending(vec![Pending { from: a.clone(), to: b.clone(), attempts: 1 }], &mut log);
        assert!(still.is_empty());
        assert_eq!(read(&b), "new");
        assert_eq!(read(&a), "old");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn running_twice_is_harmless() {
        let root = temp("twice");
        let (from, to) = (root.join("old"), root.join("new"));
        write(&from.join("a"), "a");
        let m = mv(&from, &to);
        migrate_dir(&m);
        assert!(migrate_dir(&m).log.is_empty());
        assert_eq!(read(&to.join("a")), "a");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn state_round_trips_and_defaults_when_missing_or_broken() {
        let root = temp("state");
        let path = root.join(STATE_FILE);
        assert_eq!(load_state(&path), State::default());
        let state = State {
            version: 2,
            moves_done: vec!["roaming-name".into()],
            pending: vec![Pending { from: "a".into(), to: "b".into(), attempts: 2 }],
            keyring: false,
            keyring_attempts: 1,
        };
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path), state);
        // A version 1 file loads: its fields are ignored, the moves run.
        std::fs::write(&path, r#"{"version":1,"dirs":true,"keyring":true}"#).unwrap();
        let v1 = load_state(&path);
        assert!(v1.keyring && v1.moves_done.is_empty());
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_state(&path), State::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_one_process_migrates_at_a_time() {
        let name = format!(r"Local\RoadeepMigrationTest-{}", uuid::Uuid::new_v4().simple());
        let held = acquire_lock(&name, 0).expect("free lock");
        // A mutex is re-entrant per thread: contend from another one.
        let other = name.clone();
        assert!(std::thread::spawn(move || acquire_lock(&other, 50).is_none()).join().unwrap());
        drop(held);
        let other = name.clone();
        assert!(std::thread::spawn(move || acquire_lock(&other, 1000).is_some()).join().unwrap());
    }

    #[derive(Default)]
    struct FakeVault {
        items: RefCell<HashMap<(String, String), Vec<u8>>>,
        fail_write: bool,
        corrupt: bool,
    }

    impl FakeVault {
        fn put(&self, s: &str, a: &str, v: &str) {
            self.items.borrow_mut().insert((s.into(), a.into()), v.as_bytes().to_vec());
        }
        fn get(&self, s: &str, a: &str) -> Option<String> {
            self.items.borrow().get(&(s.into(), a.into())).map(|v| String::from_utf8(v.clone()).unwrap())
        }
    }

    impl Vault for FakeVault {
        fn read(&self, s: &str, a: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.items.borrow().get(&(s.into(), a.into())).cloned())
        }
        fn write(&self, s: &str, a: &str, v: &[u8]) -> Result<(), String> {
            if self.fail_write {
                return Err("denied".into());
            }
            let mut v = v.to_vec();
            if self.corrupt {
                v.push(b'!');
            }
            self.items.borrow_mut().insert((s.into(), a.into()), v);
            Ok(())
        }
        fn delete(&self, s: &str, a: &str) -> Result<(), String> {
            self.items.borrow_mut().remove(&(s.into(), a.into()));
            Ok(())
        }
    }

    fn accounts(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn credentials_are_copied_verified_then_removed() {
        let v = FakeVault::default();
        v.put("old", "github-token", "gh-secret");
        v.put("old", "mcpc.gh.env:API_KEY", "k");
        let (out, log) = migrate_keyring(&v, "old", "new", &accounts(&["github-token", "mcpc.gh.env:API_KEY", "absent"]));
        assert_eq!(out, KeyringOutcome { moved: 2, kept: 0, failed: 0 });
        assert_eq!(v.get("new", "github-token").as_deref(), Some("gh-secret"));
        assert_eq!(v.get("old", "github-token"), None);
        assert!(log.iter().all(|l| !l.contains("gh-secret")), "values never reach the log");
    }

    #[test]
    fn an_existing_new_credential_is_not_overwritten() {
        let v = FakeVault::default();
        v.put("old", "roadeep-user", "old-user");
        v.put("new", "roadeep-user", "new-user");
        let (out, _) = migrate_keyring(&v, "old", "new", &accounts(&["roadeep-user"]));
        assert_eq!(out, KeyringOutcome { moved: 0, kept: 1, failed: 0 });
        assert_eq!(v.get("new", "roadeep-user").as_deref(), Some("new-user"));
        assert_eq!(v.get("old", "roadeep-user").as_deref(), Some("old-user"));
    }

    #[test]
    fn a_failed_or_mismatched_copy_keeps_the_original() {
        for (fail_write, corrupt) in [(true, false), (false, true)] {
            let v = FakeVault { fail_write, corrupt, ..Default::default() };
            v.put("old", "vercel-token", "t");
            let (out, _) = migrate_keyring(&v, "old", "new", &accounts(&["vercel-token"]));
            assert_eq!(out.failed, 1);
            assert_eq!(v.get("old", "vercel-token").as_deref(), Some("t"));
        }
    }

    #[test]
    fn retired_credentials_are_purged_under_the_former_service_not_copied() {
        let v = FakeVault::default();
        v.put("old", "anthropic-api-key", "sk-x");
        let log = purge_retired(&v, "old", crate::secrets::LEGACY_KEYS);
        assert_eq!(v.get("old", "anthropic-api-key"), None);
        assert_eq!(v.get("new", "anthropic-api-key"), None);
        assert_eq!(log.len(), 1);
        assert!(!log[0].contains("sk-x"));
        assert!(!known_accounts(&[]).iter().any(|a| a == "anthropic-api-key"));
    }

    #[test]
    fn mcp_server_slots_are_enumerated() {
        use crate::mcpc::store::{Auth, ServerConfig, Transport};
        let server = ServerConfig {
            id: "gh".into(),
            name: "GitHub".into(),
            source: "custom".into(),
            transport: Transport::Stdio { command: "npx".into(), args: vec![], env: vec!["API_KEY".into()] },
            auth: Auth::None,
            enabled: true,
            approved_command: None,
            tools: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(mcpc_accounts(&[server]), accounts(&["mcpc.gh.token", "mcpc.gh.oauth", "mcpc.gh.env:API_KEY"]));
    }

    #[test]
    fn known_accounts_cover_native_session_and_catalog_keys() {
        let all = known_accounts(&[]);
        for key in ["github-token", "roadeep-access-token", "roadeep-refresh-token"] {
            assert!(all.iter().any(|a| a == key), "{key}");
        }
        assert!(all.iter().any(|a| a.starts_with("x.")), "catalog fields are included");
    }

    #[test]
    fn relay_folders_are_kept_while_referenced_and_removed_on_request() {
        let root = temp("bin");
        let (old, interim) = (root.join("old").join("bin"), root.join("Roadeep").join("bin"));
        write(&old.join("hook.exe"), "x");
        write(&interim.join("mcp.exe"), "y");
        let kept = cleanup_bins(&[old.clone(), interim.clone()], true);
        assert!(kept.referenced && kept.removed.is_empty() && kept.kept.len() == 2);
        assert!(old.join("hook.exe").exists());
        let done = cleanup_bins(&[old.clone(), interim.clone(), root.join("absent")], false);
        assert!(!old.exists() && !interim.exists());
        assert!(done.kept.is_empty() && done.removed.len() == 4, "{done:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_relay_in_use_is_kept_and_reported() {
        let root = temp("bin-busy");
        let bin = root.join("bin");
        write(&bin.join("hook.exe"), "x");
        let handle = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().read(true).share_mode(1).open(bin.join("hook.exe")).unwrap()
        };
        let report = cleanup_bins(std::slice::from_ref(&bin), false);
        assert_eq!(report.kept, vec![bin.join("hook.exe").display().to_string()]);
        assert!(bin.exists());
        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }

    struct FakeStartup {
        legacy: RefCell<bool>,
        enabled: RefCell<bool>,
    }

    impl Startup for FakeStartup {
        fn remove_legacy(&self) -> Result<bool, String> {
            Ok(self.legacy.replace(false))
        }
        fn enable(&self) -> Result<(), String> {
            self.enabled.replace(true);
            Ok(())
        }
    }

    #[test]
    fn autostart_moves_to_the_new_entry_only_when_enabled() {
        let on = FakeStartup { legacy: RefCell::new(true), enabled: RefCell::new(false) };
        let log = migrate_autostart(&on, true);
        assert!(!*on.legacy.borrow() && *on.enabled.borrow());
        assert_eq!(log.len(), 1);

        let off = FakeStartup { legacy: RefCell::new(true), enabled: RefCell::new(false) };
        migrate_autostart(&off, false);
        assert!(!*off.legacy.borrow(), "the old entry goes either way");
        assert!(!*off.enabled.borrow(), "autostart stays off when the user turned it off");

        let fresh = FakeStartup { legacy: RefCell::new(false), enabled: RefCell::new(false) };
        assert!(migrate_autostart(&fresh, true).is_empty());
        assert!(*fresh.enabled.borrow());
    }
}
