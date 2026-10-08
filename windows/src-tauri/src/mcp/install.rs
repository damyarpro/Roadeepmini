// Registering roadeep-mcp.exe with the coding apps (clients.rs lists them), and
// staging the exe itself.
//
// Those config files are the apps' own state — other MCP servers' tokens,
// projects, history — so the rules from hooks.rs apply, tightened, to every
// client and every format:
//   * only the `roadeep` entry is ever added, replaced or removed;
//   * the preview shows that entry alone; other servers appear by name only,
//     because their env/headers can hold secrets and must never reach the UI;
//   * a dated backup is taken, the write happens only after an explicit click,
//     and only if the file is byte-identical to what the preview was made from.
//     The apps rewrite these files often, so a stale preview is common: the
//     answer is "review again", never "write anyway";
//   * a file that can't be read with certainty (broken, JSON with comments, a
//     YAML shape the line editor doesn't know) is an error, never overwritten.
//
//   cfg_json.rs / cfg_toml.rs / cfg_yaml.rs — reading and editing each format
//   this file                              — bytes, backups, the atomic write

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use windows::Win32::System::SystemInformation::GetLocalTime;

use super::clients::{self, Client, Edit, Format, Found, Roots};
use super::{cfg_json, cfg_toml, cfg_yaml};
use crate::{errors, log, settings};

const EXE: &str = "roadeep-mcp.exe";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientStatus {
    pub id: &'static str,
    pub name: &'static str,
    /// Its config file or its config folder exists.
    pub detected: bool,
    /// A `roadeep` entry pointing at roadeep-mcp exists.
    pub installed: bool,
    /// A `roadeep` entry exists but is somebody else's; installing replaces it.
    pub conflict: bool,
    /// Our entry runs a relay from a previous folder (a build under the former
    /// name, or an interim one); installing updates it (`installed` is false
    /// until then).
    pub legacy_relay: bool,
    pub config_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub exe_ready: bool,
    pub exe_path: String,
    pub signed_in: bool,
    pub clients: Vec<ClientStatus>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPreview {
    pub diff: String,
    pub backup: String,
    pub config_path: String,
    /// The bytes this diff was computed from; handed back to `write`.
    pub fingerprint: String,
}

pub fn exe_path() -> PathBuf {
    settings::local_dir().join("bin").join(EXE)
}

/// The raw bytes, empty when the file does not exist.
fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    match std::fs::read(path) {
        Ok(b) => Ok(b),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(errors::coded(errors::CFG_UNREADABLE, &[&path.display().to_string(), &err.to_string()])),
    }
}

fn found(client: &Client, bytes: &[u8], path: &Path) -> Result<Found, String> {
    match client.format {
        Format::Json { container, shape } => cfg_json::found(bytes, path, container, shape),
        Format::Toml => cfg_toml::found(bytes, path),
        Format::Yaml => cfg_yaml::found(bytes, path),
    }
}

fn plan(client: &Client, bytes: &[u8], path: &Path, install: bool, command: &str) -> Result<Edit, String> {
    match client.format {
        Format::Json { container, shape } => cfg_json::edit(bytes, path, container, shape, install, command),
        Format::Toml => cfg_toml::edit(bytes, path, install, command),
        Format::Yaml => cfg_yaml::edit(bytes, path, install, command),
    }
}

/// FNV-1a over the exact bytes, as in hooks.rs: "is this still the file I showed?".
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

fn stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

/// Backups taken within the same millisecond get `-1`, `-2`, … after the stamp.
const MAX_BACKUP_SUFFIX: u32 = 999;

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("config").to_string()
}

fn backup_candidate(path: &Path, stamp: &str, n: u32) -> PathBuf {
    let suffix = if n == 0 { String::new() } else { format!("-{n}") };
    path.with_file_name(format!("{}.bak-roadeep-{stamp}{suffix}", file_name(path)))
}

/// Where the next backup would go (what the preview shows).
fn backup_path(path: &Path) -> PathBuf {
    let stamp = stamp();
    (0..=MAX_BACKUP_SUFFIX)
        .map(|n| backup_candidate(path, &stamp, n))
        .find(|p| !p.exists())
        .unwrap_or_else(|| backup_candidate(path, &stamp, 0))
}

/// Writes the backup to a name nobody holds yet. `create_new` makes the
/// "is it free?" check and the creation one step, so an earlier backup — the
/// user's original file, after an install and uninstall in the same instant —
/// is never overwritten.
fn write_backup(path: &Path, bytes: &[u8], stamp: &str) -> Result<PathBuf, String> {
    use std::io::Write;
    for n in 0..=MAX_BACKUP_SUFFIX {
        let candidate = backup_candidate(path, stamp, n);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(mut file) => {
                if let Err(err) = file.write_all(bytes).and_then(|_| file.sync_all()) {
                    drop(file);
                    let _ = std::fs::remove_file(&candidate);
                    return Err(errors::coded(errors::CFG_BACKUP, &[&err.to_string()]));
                }
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(errors::coded(errors::CFG_BACKUP, &[&err.to_string()])),
        }
    }
    Err(errors::coded(errors::CFG_BACKUP, &["too many backups with the same timestamp"]))
}

// ── Path-explicit core (what the tests drive) ─────────────────────────────────

/// (installed, conflict, stale relay). Read-only and must never fail loudly:
/// an unreadable file reads as "not installed". Our entry counts as installed
/// only when installing `command` would change nothing; otherwise it points at
/// a previous relay and is reported stale.
pub fn status_at(client: &Client, path: &Path, command: &str) -> (bool, bool, bool) {
    let Ok(bytes) = read_bytes(path) else { return (false, false, false) };
    match found(client, &bytes, path) {
        Ok(Found::Ours) => {
            let current = plan(client, &bytes, path, true, command).is_ok_and(|e| e.diff == errors::CFG_NO_CHANGE);
            (current, false, !current)
        }
        Ok(Found::Foreign) => (false, true, false),
        Ok(Found::Legacy) => (false, false, true),
        Ok(Found::Missing) | Err(_) => (false, false, false),
    }
}

pub fn preview_at(client: &Client, path: &Path, install: bool, command: &str) -> Result<McpPreview, String> {
    let bytes = read_bytes(path)?;
    let edit = plan(client, &bytes, path, install, command)?;
    Ok(McpPreview {
        diff: edit.diff,
        backup: backup_path(path).to_string_lossy().to_string(),
        config_path: path.to_string_lossy().to_string(),
        fingerprint: fingerprint(&bytes),
    })
}

/// Backs up, then writes beside the target and renames over it. Returns the
/// backup path ("" when there was no file to back up). The parent folder is
/// created only here — an explicit install for an app that isn't set up yet.
pub fn write_at(client: &Client, path: &Path, install: bool, expected: &str, command: &str) -> Result<String, String> {
    let bytes = read_bytes(path)?;
    if fingerprint(&bytes) != expected {
        return Err(errors::coded(errors::CFG_CHANGED, &[&path.display().to_string()]));
    }
    let edit = plan(client, &bytes, path, install, command)?;

    // The backup is the very bytes the preview was checked against, not a
    // second read that could already be newer.
    let backup = if bytes.is_empty() && !path.exists() {
        String::new()
    } else {
        write_backup(path, &bytes, &stamp())?.to_string_lossy().to_string()
    };

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| errors::coded(errors::CFG_WRITE, &[&e.to_string()]))?;
    }
    let temp = path.with_file_name(format!(".{}.roadeep-{}", file_name(path), std::process::id()));
    std::fs::write(&temp, &edit.bytes).map_err(|e| errors::coded(errors::CFG_WRITE, &[&e.to_string()]))?;
    if let Err(err) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(errors::coded(errors::CFG_WRITE, &[&err.to_string()]));
    }
    Ok(backup)
}

// ── Public API ────────────────────────────────────────────────────────────────

fn command() -> String {
    exe_path().to_string_lossy().to_string()
}

pub fn status(signed_in: bool) -> McpStatus {
    let roots = Roots::from_env();
    let clients = clients::CLIENTS
        .iter()
        .map(|c| {
            let path = c.config_path(&roots);
            let (installed, conflict, legacy_relay) = status_at(c, &path, &command());
            ClientStatus {
                id: c.id,
                name: c.name,
                detected: c.detected(&roots),
                installed,
                conflict,
                legacy_relay,
                config_path: path.to_string_lossy().to_string(),
            }
        })
        .collect();
    let exe = exe_path();
    McpStatus { exe_ready: exe.exists(), exe_path: exe.to_string_lossy().to_string(), signed_in, clients }
}

pub fn preview(client: &str, install: bool) -> Result<McpPreview, String> {
    let client = clients::find(client)?;
    preview_at(client, &client.config_path(&Roots::from_env()), install, &command())
}

pub fn write(client: &str, install: bool, fingerprint: &str) -> Result<String, String> {
    let client = match clients::find(client) {
        Ok(c) => c,
        Err(err) => {
            log::line(format!("mcp: refused a write for an unknown client: {err}"));
            return Err(err);
        }
    };
    if install && !exe_path().exists() {
        return Err(errors::coded(errors::MCP_EXE_MISSING, &[EXE]));
    }
    let action = if install { "installed" } else { "removed" };
    let result = write_at(client, &client.config_path(&Roots::from_env()), install, fingerprint, &command());
    match &result {
        Ok(_) => log::line(format!("mcp: {} entry {action}", client.id)),
        // The first line only: a manual-edit refusal carries a snippet after it.
        Err(err) => log::line(format!("mcp: {} entry not {action}: {}", client.id, err.lines().next().unwrap_or(""))),
    }
    result
}

/// Copies roadeep-mcp.exe into %LOCALAPPDATA%\com.roadeep.desktop\bin on launch, from the
/// app resources (installed) or next to roadeep.exe / the release build (dev).
///
/// Unlike the hook relay, this exe stays running for as long as a Claude Code
/// session is open, so the old copy is usually locked. Windows lets a running
/// exe be renamed, so it is moved aside and replaced; the leftovers are swept
/// on the next launch once nothing runs them.
pub fn ensure_mcp_exe(app: &AppHandle) {
    let dest = exe_path();
    let Some(dir) = dest.parent() else { return };
    if let Err(err) = std::fs::create_dir_all(dir) {
        log::line(format!("mcp: cannot create {}: {err}", dir.display()));
        return;
    }
    sweep_old_copies(dir);

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = app.path().resolve(EXE, tauri::path::BaseDirectory::Resource) {
        candidates.push(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(EXE));
            candidates.push(parent.join("../release").join(EXE));
            candidates.push(parent.join("_up_/target/release").join(EXE));
        }
    }
    let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
    let Some(src) = candidates.into_iter().find(|p| p.exists()) else {
        log::line(format!("mcp: {EXE} not found — the Roadeep MCP server cannot run. Looked in: {}", tried.join(", ")));
        return;
    };

    let same = match (std::fs::metadata(&src), std::fs::metadata(&dest)) {
        (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() == b.modified().ok(),
        _ => false,
    };
    if same {
        return;
    }
    if std::fs::copy(&src, &dest).is_ok() {
        return;
    }
    // Probably running under a Claude Code session: move it aside, then copy.
    let aside = dir.join(format!("{EXE}.old-{}", std::process::id()));
    if let Err(err) = std::fs::rename(&dest, &aside) {
        log::line(format!("mcp: could not update {EXE} (in use, rename failed: {err}); keeping the current copy"));
        return;
    }
    if let Err(err) = std::fs::copy(&src, &dest) {
        log::line(format!("mcp: could not install {EXE}: {err}; restoring the previous copy"));
        if let Err(err) = std::fs::rename(&aside, &dest) {
            log::line(format!("mcp: could not restore {EXE}: {err}"));
        }
    }
}

fn sweep_old_copies(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(&format!("{EXE}.old-")) {
            // Still running somewhere: it goes next time.
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const CMD: &str = r"C:\Users\x\AppData\Local\com.roadeep.desktop\bin\roadeep-mcp.exe";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roadeep-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn client(id: &str) -> &'static Client {
        clients::find(id).unwrap()
    }

    fn sample() -> Value {
        json!({
            "numStartups": 12,
            "projects": { "D:/a": { "allowedTools": [] }, "d:/a": { "history": [] } },
            "mcpServers": {
                "github": { "type": "http", "url": "https://x", "headers": { "Authorization": "Bearer SECRET" } }
            }
        })
    }

    #[test]
    fn writing_backs_up_preserves_and_refuses_a_changed_file() {
        let cc = client("claude-code");
        let dir = temp_dir("write");
        let path = dir.join(".claude.json");
        // Shaped like Claude Code's: 2-space JSON, no trailing newline, keys that
        // differ only by case (which PowerShell's parser chokes on).
        let original = serde_json::to_string_pretty(&sample()).unwrap();
        std::fs::write(&path, &original).unwrap();

        let plan = preview_at(cc, &path, true, CMD).unwrap();
        assert!(plan.diff.contains("roadeep-mcp.exe"));
        let backup = write_at(cc, &path, true, &plan.fingerprint, CMD).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original, "backup holds the original bytes");
        assert_eq!(status_at(cc, &path, CMD), (true, false, false));

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.ends_with('\n'), "keeps the original's lack of a trailing newline");
        let after: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(after["projects"], sample()["projects"]);
        assert_eq!(after["mcpServers"]["github"], sample()["mcpServers"]["github"]);
        assert_eq!(after["mcpServers"]["roadeep"], json!({ "type": "stdio", "command": CMD, "args": [], "env": {} }));

        // Installing again changes nothing.
        assert_eq!(preview_at(cc, &path, true, CMD).unwrap().diff, errors::CFG_NO_CHANGE);

        // A file that moved since the preview is refused and left alone.
        let stale = preview_at(cc, &path, false, CMD).unwrap();
        let mut edited = after.clone();
        edited["numStartups"] = json!(13);
        let edited_text = serde_json::to_string_pretty(&edited).unwrap();
        std::fs::write(&path, &edited_text).unwrap();
        let err = write_at(cc, &path, false, &stale.fingerprint, CMD).unwrap_err();
        assert!(err.starts_with(errors::CFG_CHANGED), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited_text);

        // Uninstall from a fresh preview gives back the original content.
        let plan = preview_at(cc, &path, false, CMD).unwrap();
        write_at(cc, &path, false, &plan.fingerprint, CMD).unwrap();
        let restored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut expected = sample();
        expected["numStartups"] = json!(13);
        assert_eq!(restored, expected);

        // Unparseable content is refused before anything is written.
        std::fs::write(&path, b"{ broken").unwrap();
        assert!(preview_at(cc, &path, true, CMD).is_err());
        assert!(write_at(cc, &path, true, "whatever", CMD).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ broken");
        assert_eq!(status_at(cc, &path, CMD), (false, false, false));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_and_folder_are_created_without_a_backup() {
        let dir = temp_dir("missing");
        let cursor = client("cursor");
        let path = dir.join(".cursor").join("mcp.json");
        let plan = preview_at(cursor, &path, true, CMD).unwrap();
        assert!(!dir.join(".cursor").exists(), "a preview creates nothing");
        assert_eq!(write_at(cursor, &path, true, &plan.fingerprint, CMD).unwrap(), "");
        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(after, json!({ "mcpServers": { "roadeep": { "command": CMD, "args": [] } } }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_client_round_trips_through_the_file_layer() {
        let dir = temp_dir("all");
        for c in clients::CLIENTS {
            let path = dir.join(c.id).join("config");
            let plan = preview_at(c, &path, true, CMD).unwrap();
            write_at(c, &path, true, &plan.fingerprint, CMD).unwrap();
            assert_eq!(status_at(c, &path, CMD), (true, false, false), "{}", c.id);
            let plan = preview_at(c, &path, false, CMD).unwrap();
            assert!(plan.diff.contains("roadeep"), "{}: {}", c.id, plan.diff);
            let backup = write_at(c, &path, false, &plan.fingerprint, CMD).unwrap();
            assert!(!backup.is_empty(), "{}", c.id);
            assert_eq!(status_at(c, &path, CMD), (false, false, false), "{}", c.id);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_from_the_former_name_is_flagged_and_updated_in_every_format() {
        let dir = temp_dir("legacy");
        let old = format!(r"C:\Users\x\AppData\Local\Old\bin\{}.exe", crate::migrate::legacy_mcp_marker());
        for c in clients::CLIENTS {
            let path = dir.join(c.id).join("config");
            let plan = preview_at(c, &path, true, &old).unwrap();
            write_at(c, &path, true, &plan.fingerprint, &old).unwrap();
            assert_eq!(status_at(c, &path, CMD), (false, false, true), "{}", c.id);
            // An interim relay path (our marker, previous folder) is stale too.
            let interim = r"C:\Users\x\AppData\Local\Roadeep\bin\roadeep-mcp.exe";
            let plan = preview_at(c, &path, true, interim).unwrap();
            write_at(c, &path, true, &plan.fingerprint, interim).unwrap();
            assert_eq!(status_at(c, &path, CMD), (false, false, true), "{}", c.id);
            assert_eq!(status_at(c, &path, interim), (true, false, false), "{}", c.id);
            // Installing replaces the old relay path in place.
            let plan = preview_at(c, &path, true, CMD).unwrap();
            write_at(c, &path, true, &plan.fingerprint, CMD).unwrap();
            assert_eq!(status_at(c, &path, CMD), (true, false, false), "{}", c.id);
            // And an old entry can be removed like any of ours.
            let plan = preview_at(c, &path, true, &old).unwrap();
            write_at(c, &path, true, &plan.fingerprint, &old).unwrap();
            let plan = preview_at(c, &path, false, CMD).unwrap();
            write_at(c, &path, false, &plan.fingerprint, CMD).unwrap();
            assert_eq!(status_at(c, &path, CMD), (false, false, false), "{}", c.id);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_fingerprints_are_refused_for_toml_and_yaml_too() {
        let dir = temp_dir("stale");
        for (id, text, edited) in [
            ("codex", "model = \"o3\"\n", "model = \"o4\"\n"),
            ("hermes", "model:\n  default: a\n", "model:\n  default: b\n"),
        ] {
            let c = client(id);
            let path = dir.join(id);
            std::fs::write(&path, text).unwrap();
            let plan = preview_at(c, &path, true, CMD).unwrap();
            std::fs::write(&path, edited).unwrap();
            let err = write_at(c, &path, true, &plan.fingerprint, CMD).unwrap_err();
            assert!(err.starts_with(errors::CFG_CHANGED), "{id}: {err}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backups_in_the_same_instant_never_overwrite_each_other() {
        let dir = temp_dir("backup");
        let path = dir.join(".claude.json");
        // Install then uninstall within one timestamp: the first backup is the
        // user's original file and must survive the second.
        let first = write_backup(&path, b"original", "20261001-120000-123").unwrap();
        let second = write_backup(&path, b"after install", "20261001-120000-123").unwrap();
        let third = write_backup(&path, b"third", "20261001-120000-123").unwrap();
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert!(first.to_string_lossy().ends_with(".claude.json.bak-roadeep-20261001-120000-123"), "{first:?}");
        assert!(second.to_string_lossy().ends_with("-123-1"), "{second:?}");
        assert!(third.to_string_lossy().ends_with("-123-2"), "{third:?}");
        assert_eq!(std::fs::read(&first).unwrap(), b"original");
        assert_eq!(std::fs::read(&second).unwrap(), b"after install");

        // The preview names the next free slot, and real stamps carry milliseconds.
        assert_eq!(backup_candidate(&path, "s", 0), dir.join(".claude.json.bak-roadeep-s"));
        let stamp = stamp();
        assert_eq!(stamp.len(), "20261001-120000-123".len(), "{stamp}");

        // End to end: two writes back to back keep two distinct backups.
        let cc = client("claude-code");
        std::fs::write(&path, br#"{"a":1}"#).unwrap();
        let plan = preview_at(cc, &path, true, CMD).unwrap();
        let b1 = write_at(cc, &path, true, &plan.fingerprint, CMD).unwrap();
        let plan = preview_at(cc, &path, false, CMD).unwrap();
        let b2 = write_at(cc, &path, false, &plan.fingerprint, CMD).unwrap();
        assert_ne!(b1, b2);
        assert_eq!(std::fs::read(&b1).unwrap(), br#"{"a":1}"#, "the original survives the uninstall");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
