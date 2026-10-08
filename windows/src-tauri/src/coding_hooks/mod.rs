//! Provider-specific hook configuration, preview-before-write and shared relay contracts.
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::Window;

#[path = "../../../hook/src/protocol.rs"]
pub mod protocol;
const MAX_CONFIG: u64 = 2 * 1024 * 1024;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    provider: String,
    label: String,
    supported: bool,
    scope: &'static str,
    installed: bool,
    /// Entries from a build under the app's former name still run the old
    /// relay; applying replaces them.
    legacy_relay: bool,
    detected: bool,
    hook_ready: bool,
    settings_path: String,
    hook_path: String,
    events: Vec<String>,
    permission: bool,
    reason: Option<&'static str>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    provider: String,
    settings_path: String,
    backup: String,
    diff: String,
    fingerprint: String,
}
fn fail(code: &str) -> String {
    crate::log::line(format!("coding-hooks outcome={code}"));
    code.into()
}

/// Explicit local maintenance commands use the same reviewed merge and fingerprint gate as settings.
/// They run before the single-instance plugin so an existing island cannot swallow installation.
pub fn run_cli(args: &[String]) -> Option<i32> {
    let flag = args.first()?.as_str();
    if !matches!(
        flag,
        "--coding-hooks-preview"
            | "--coding-hooks-preview-remove"
            | "--coding-hooks-apply"
            | "--coding-hooks-remove"
            | "--coding-hooks-status"
    ) {
        return None;
    }
    let result = (|| -> Result<Value, String> {
        let base = home()?;
        if flag == "--coding-hooks-status" {
            if args.len() != 1 {
                return Err(fail("hook-cli-invalid"));
            }
            return serde_json::to_value(
                protocol::PROVIDERS
                    .iter()
                    .map(|p| status_in(&base, p, None))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| fail("hook-cli-invalid"));
        }
        let provider = args.get(1).ok_or_else(|| fail("hook-cli-invalid"))?;
        let exe = crate::settings::hook_exe_path();
        let write = !matches!(
            flag,
            "--coding-hooks-preview" | "--coding-hooks-preview-remove"
        );
        let project = args.get(if write { 3 } else { 2 }).map(String::as_str);
        if args.len() > if write { 4 } else { 3 } {
            return Err(fail("hook-cli-invalid"));
        }
        let path = path_in(&base, provider, project)?;
        let remove = matches!(
            flag,
            "--coding-hooks-remove" | "--coding-hooks-preview-remove"
        );
        if write {
            let expected = args.get(2).ok_or_else(|| fail("hook-cli-invalid"))?;
            if !remove && !exe.is_file() {
                return Err(fail("hook-relay-unavailable"));
            }
            write_in(&path, provider, &exe, expected, remove)?;
            serde_json::to_value(status_in(&base, provider, project))
                .map_err(|_| fail("hook-cli-invalid"))
        } else {
            serde_json::to_value(preview_in(&path, provider, &exe, remove)?)
                .map_err(|_| fail("hook-cli-invalid"))
        }
    })();
    match result {
        Ok(value) => {
            println!("{value}");
            Some(0)
        }
        Err(code) => {
            eprintln!("{code}");
            Some(2)
        }
    }
}
fn authorize(window: &Window) -> Result<(), String> {
    if matches!(window.label(), "island" | "settings") {
        Ok(())
    } else {
        Err(fail("hook-window-denied"))
    }
}
fn home() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| fail("hook-home-unavailable"))
}
fn path_in(base: &Path, provider: &str, project: Option<&str>) -> Result<PathBuf, String> {
    Ok(match provider {
        "claude" => base.join(".claude/settings.json"),
        "codex" => {
            let root = std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| base.join(".codex"));
            if !root.is_absolute() {
                return Err(fail("hook-invalid-path"));
            }
            root.join("hooks.json")
        }
        "gemini" => base.join(".gemini/settings.json"),
        "cursor" => base.join(".cursor/hooks.json"),
        "windsurf" => base.join(".codeium/windsurf/hooks.json"),
        "copilot" => {
            let directory = std::env::var_os("COPILOT_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| base.join(".copilot"));
            if !directory.is_absolute() {
                return Err(fail("hook-invalid-path"));
            }
            directory.join("hooks/roadeep.json")
        }
        "vscode" | "kiro" | "opencode" => {
            let raw = project.ok_or_else(|| fail("hook-project-required"))?;
            if raw.len() > 2048 || raw.chars().any(char::is_control) {
                return Err(fail("hook-invalid-path"));
            }
            let root = PathBuf::from(raw);
            if !root.is_absolute()
                || !root.is_dir()
                || root
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(fail("hook-invalid-path"));
            }
            check_path(&root)?;
            let root = fs::canonicalize(root).map_err(|_| fail("hook-invalid-path"))?;
            root.join(if provider == "opencode" {
                ".opencode/plugins/roadeep/index.ts"
            } else if provider == "kiro" {
                ".kiro/hooks/roadeep.json"
            } else {
                ".github/hooks/roadeep-vscode.json"
            })
        }
        _ => return Err(fail("hook-unsupported-provider")),
    })
}
fn check_path(path: &Path) -> Result<(), String> {
    let mut current = Some(path);
    while let Some(p) = current {
        if let Ok(meta) = fs::symlink_metadata(p) {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(fail("hook-reparse-path"));
            }
        }
        current = p.parent();
    }
    Ok(())
}
fn atomic_replace(temp: &Path, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return fs::rename(temp, path).map_err(|_| fail("hook-config-write"));
    }
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    // ReplaceFile preserves the original target's security descriptor on Windows.
    if unsafe {
        ReplaceFileW(
            target.as_ptr(),
            source.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(fail("hook-config-write"));
    }
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    check_path(path)?;
    match fs::File::open(path) {
        Ok(file) => {
            let mut b = Vec::new();
            file.take(MAX_CONFIG + 1)
                .read_to_end(&mut b)
                .map_err(|_| fail("hook-config-unreadable"))?;
            if b.len() as u64 > MAX_CONFIG {
                return Err(fail("hook-config-too-large"));
            }
            Ok(b)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(fail("hook-config-unreadable")),
    }
}
fn parse(bytes: &[u8]) -> Result<Value, String> {
    let b = bytes.strip_prefix(&[239, 187, 191]).unwrap_or(bytes);
    if b.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    let v: Value = serde_json::from_slice(b).map_err(|_| fail("hook-config-invalid"))?;
    if !v.is_object() {
        return Err(fail("hook-config-invalid"));
    }
    Ok(v)
}
fn hash(bytes: &[u8], provider: &str, path: &Path, remove: bool) -> String {
    let mut h = Sha256::new();
    h.update(provider);
    h.update(path.as_os_str().to_string_lossy().as_bytes());
    h.update([remove as u8]);
    h.update(bytes);
    format!("{:x}", h.finalize())
}
fn command(exe: &Path, provider: &str, event: &str) -> Result<String, String> {
    let path = exe.to_string_lossy().replace('\\', "/");
    // Shell commands contain only the fixed relay and allowlisted arguments. Reject shell metacharacters in installation paths.
    if path.chars().any(|c| {
        matches!(
            c,
            '"' | '\'' | '`' | '$' | '%' | '\n' | '\r' | '&' | '|' | '<' | '>' | ';'
        )
    }) {
        return Err(fail("hook-relay-path-unsafe"));
    }
    let prefix = if matches!(provider, "windsurf" | "vscode" | "kiro") {
        "& "
    } else {
        ""
    };
    Ok(format!("{prefix}\"{path}\" --provider {provider} {event}"))
}
fn handler(exe: &Path, provider: &str, event: &str) -> Result<Value, String> {
    let cmd = command(exe, provider, event)?;
    Ok(match provider {
        "copilot" => {
            json!({"type":"command","exec":exe,"args":["--provider",provider,event],"timeoutSec":10})
        }
        "kiro" => {
            json!({"name":format!("Roadeep {event}"),"trigger":event,"action":{"type":"command","command":cmd},"timeout":10})
        }
        "vscode" => json!({"type":"command","windows":cmd,"timeout":10}),
        "windsurf" => json!({"powershell":cmd,"show_output":false}),
        _ => {
            json!({"type":"command","command":cmd,"timeout":if protocol::permission(provider,event){120}else if provider=="gemini"{10000}else if provider=="codex"&&matches!(event,"SessionEnd"|"Interrupt"){3}else{10}})
        }
    })
}
fn ours(handler: &Value, exe: &Path, provider: &str, event: &str) -> bool {
    // Exact equality of our executable/argument command prevents deleting another integration merely containing the relay name.
    let expected = handler_for_ownership(exe, provider, event);
    expected.is_some_and(|expected| {
        ["command", "powershell", "windows"]
            .iter()
            .any(|key| expected.get(*key).is_some() && handler.get(*key) == expected.get(*key))
            || (provider == "copilot"
                && handler.get("exec") == expected.get("exec")
                && handler.get("args") == expected.get("args"))
            || (provider == "claude"
                && handler["command"].as_str()
                    == Some(
                        format!("\"{}\" {event}", exe.to_string_lossy().replace('\\', "/"))
                            .as_str(),
                    ))
    })
}
fn handler_for_ownership(exe: &Path, provider: &str, event: &str) -> Option<Value> {
    handler(exe, provider, event).ok()
}
/// Relays previous builds registered (under the app's former name, or staged
/// in the install folder by interim builds).
fn previous_exes() -> Vec<PathBuf> {
    crate::migrate::previous_hook_exes()
}
/// Ours with the current relay or a previous one: both are replaced on apply and removed on remove.
fn ours_any(handler: &Value, exe: &Path, provider: &str, event: &str) -> bool {
    ours(handler, exe, provider, event) || previous_exes().iter().any(|p| ours(handler, p, provider, event))
}
fn kiro_ours(h: &Value, exe: &Path, event: &str) -> bool {
    std::iter::once(exe.to_path_buf()).chain(previous_exes()).any(|exe| {
        command(&exe, "kiro", event)
            .ok()
            .is_some_and(|cmd| h["trigger"] == *event && h["action"]["command"] == cmd)
    })
}
/// The generated OpenCode plugin of a previous relay.
fn previous_plugin(bytes: &[u8]) -> bool {
    !bytes.is_empty() && previous_exes().iter().any(|p| plugin_source(p).is_ok_and(|s| s == bytes))
}
/// Any entry still runs a previous relay.
fn references_previous(root: &Value, provider: &str) -> bool {
    previous_exes().iter().any(|p| references(root, provider, p))
}
fn merge(mut root: Value, provider: &str, exe: &Path, remove: bool) -> Result<Value, String> {
    if provider == "kiro" {
        if root.get("version").is_some_and(|v| v != "v1") {
            return Err(fail("hook-config-version"));
        }
        let mut hooks = match root.get("hooks") {
            Some(v) => v
                .as_array()
                .cloned()
                .ok_or_else(|| fail("hook-config-invalid"))?,
            None => vec![],
        };
        hooks.retain(|h| !protocol::events(provider).iter().any(|event| kiro_ours(h, exe, event)));
        if !remove {
            for event in protocol::events(provider) {
                hooks.push(handler(exe, provider, event)?);
            }
            root["version"] = json!("v1");
        }
        if hooks.is_empty() {
            root.as_object_mut().unwrap().remove("hooks");
        } else {
            root["hooks"] = json!(hooks);
        }
        return Ok(root);
    }
    if let Some(version) = root.get("version") {
        if matches!(provider, "cursor" | "copilot") && version != 1 {
            return Err(fail("hook-config-version"));
        }
    }
    if root.get("hooks").is_some_and(|v| !v.is_object()) {
        return Err(fail("hook-config-invalid"));
    }
    if !remove && matches!(provider, "cursor" | "copilot") {
        root["version"] = json!(1);
    }
    if root.get("hooks").is_none() {
        if remove {
            return Ok(root);
        }
        root["hooks"] = json!({});
    }
    for event in protocol::events(provider) {
        let mut entries = match root["hooks"].get(*event) {
            Some(v) => v
                .as_array()
                .cloned()
                .ok_or_else(|| fail("hook-config-invalid"))?,
            None => Vec::new(),
        };
        let grouped = matches!(provider, "claude" | "codex" | "gemini");
        if grouped {
            let mut kept = Vec::new();
            for mut entry in entries {
                if let Some(handlers) = entry.get_mut("hooks") {
                    let list = handlers
                        .as_array_mut()
                        .ok_or_else(|| fail("hook-config-invalid"))?;
                    let before = list.len();
                    list.retain(|h| !ours_any(h, exe, provider, event));
                    if before > 0 && list.is_empty() {
                        continue;
                    }
                }
                kept.push(entry);
            }
            entries = kept;
            if !remove {
                entries.push(json!({"matcher":"*","hooks":[handler(exe,provider,event)?]}));
            }
        } else {
            entries.retain(|h| !ours_any(h, exe, provider, event));
            if !remove {
                entries.push(handler(exe, provider, event)?);
            }
        }
        if entries.is_empty() {
            root["hooks"].as_object_mut().unwrap().remove(*event);
        } else {
            root["hooks"][*event] = json!(entries);
        }
    }
    if root["hooks"].as_object().is_some_and(|m| m.is_empty()) {
        root.as_object_mut().unwrap().remove("hooks");
    }
    Ok(root)
}
fn installed(root: &Value, provider: &str, exe: &Path) -> bool {
    if provider == "kiro" {
        return protocol::events(provider).iter().all(|event| {
            root["hooks"].as_array().is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    command(exe, provider, event)
                        .ok()
                        .is_some_and(|cmd| h["trigger"] == *event && h["action"]["command"] == cmd)
                })
            })
        });
    }
    protocol::events(provider).iter().all(|event| {
        root["hooks"][*event].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                if matches!(provider, "claude" | "codex" | "gemini") {
                    entry["hooks"]
                        .as_array()
                        .is_some_and(|h| h.iter().any(|h| ours(h, exe, provider, event)))
                } else {
                    ours(entry, exe, provider, event)
                }
            })
        })
    })
}
/// Any entry (not necessarily every event) still runs `exe`.
fn references(root: &Value, provider: &str, exe: &Path) -> bool {
    protocol::events(provider).iter().any(|event| {
        if provider == "kiro" {
            return root["hooks"].as_array().is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    command(exe, provider, event)
                        .ok()
                        .is_some_and(|cmd| h["trigger"] == *event && h["action"]["command"] == cmd)
                })
            });
        }
        root["hooks"][*event].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                if matches!(provider, "claude" | "codex" | "gemini") {
                    entry["hooks"]
                        .as_array()
                        .is_some_and(|h| h.iter().any(|h| ours(h, exe, provider, event)))
                } else {
                    ours(entry, exe, provider, event)
                }
            })
        })
    })
}
/// Whether a user-scope provider config still runs a previous relay. Project-scoped
/// ones (VS Code, Kiro, OpenCode) live in folders we can't enumerate.
pub fn legacy_relay_referenced() -> bool {
    let Ok(base) = home() else { return false };
    protocol::PROVIDERS
        .iter()
        .filter(|p| !matches!(**p, "vscode" | "kiro" | "opencode"))
        .filter_map(|p| path_in(&base, p, None).ok().map(|path| (*p, path)))
        .any(|(provider, path)| {
            read(&path)
                .and_then(|b| parse(&b))
                .is_ok_and(|root| references_previous(&root, provider))
        })
}
fn plugin_source(exe: &Path) -> Result<Vec<u8>, String> {
    if !exe.is_absolute() {
        return Err(fail("hook-relay-path-unsafe"));
    }
    let literal = serde_json::to_string(&exe.to_string_lossy())
        .map_err(|_| fail("hook-relay-path-unsafe"))?;
    Ok(include_str!("../../../hook/opencode/index.template.ts")
        .replace("\"__ROADEEP_RELAY__\"", &literal)
        .into_bytes())
}
fn source_plan(bytes: &[u8], provider: &str, exe: &Path, remove: bool) -> Result<Vec<u8>, String> {
    if provider == "opencode" {
        let source = plugin_source(exe)?;
        if !bytes.is_empty() && bytes != source && !previous_plugin(bytes) {
            return Err(fail("hook-plugin-conflict"));
        }
        return Ok(if remove { Vec::new() } else { source });
    }
    let current = parse(bytes)?;
    let next = merge(current, provider, exe, remove)?;
    serde_json::to_vec_pretty(&next).map_err(|_| fail("hook-config-write"))
}
fn status_in(base: &Path, provider: &str, project: Option<&str>) -> ProviderStatus {
    let exe = crate::settings::hook_exe_path();
    let path = path_in(base, provider, project);
    let plugin_state = if provider == "opencode" {
        path.as_ref().ok().and_then(|p| read(p).ok()).map(|bytes| {
            let expected = plugin_source(&exe).unwrap_or_default();
            (
                bytes == expected && !bytes.is_empty(),
                !bytes.is_empty() && bytes != expected && !previous_plugin(&bytes),
            )
        })
    } else {
        None
    };
    let (config, reason) = match &path {
        Ok(_) if provider == "opencode" => (
            json!({}),
            if plugin_state.is_some_and(|(_, conflict)| conflict) {
                Some("hook-plugin-conflict")
            } else if plugin_state.is_none() {
                Some("hook-config-invalid")
            } else {
                None
            },
        ),
        Ok(path) => match read(path).and_then(|b| parse(&b)) {
            Ok(v) => (v, None),
            Err(_) => (json!({}), Some("hook-config-invalid")),
        },
        Err(_) => (json!({}), Some("hook-project-required")),
    };
    let label = match provider {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "gemini" => "Gemini CLI",
        "cursor" => "Cursor",
        "windsurf" => "Windsurf / Devin",
        "copilot" => "GitHub Copilot CLI",
        "vscode" => "VS Code Local",
        "kiro" => "Kiro",
        "opencode" => "OpenCode",
        _ => provider,
    };
    ProviderStatus {
        provider: provider.into(),
        label: label.into(),
        supported: true,
        scope: if matches!(provider, "vscode" | "kiro" | "opencode") {
            "project"
        } else {
            "user"
        },
        installed: if provider == "opencode" {
            plugin_state.is_some_and(|(installed, _)| installed)
        } else {
            installed(&config, provider, &exe)
        },
        legacy_relay: if provider == "opencode" {
            path.as_ref().ok().and_then(|p| read(p).ok()).is_some_and(|b| previous_plugin(&b))
        } else {
            references_previous(&config, provider)
        },
        detected: path
            .as_ref()
            .ok()
            .and_then(|p| p.parent())
            .is_some_and(Path::exists),
        hook_ready: exe.is_file(),
        settings_path: path.map(|p| p.to_string_lossy().into()).unwrap_or_default(),
        hook_path: exe.to_string_lossy().into(),
        events: protocol::events(provider)
            .iter()
            .map(|s| s.to_string())
            .collect(),
        permission: matches!(provider, "claude" | "codex"),
        reason: reason.or(if provider == "codex" {
            Some("codex-hook-trust-required")
        } else if provider == "vscode" {
            Some("vscode-local-preview")
        } else if provider == "opencode" {
            Some("opencode-v2-required")
        } else if provider == "kiro" {
            Some("kiro-version-required")
        } else {
            None
        }),
    }
}
#[tauri::command]
pub async fn coding_hooks_status(window: Window) -> Result<Vec<ProviderStatus>, String> {
    authorize(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let base = home()?;
        let mut result: Vec<_> = protocol::PROVIDERS
            .iter()
            .map(|p| status_in(&base, p, None))
            .collect();
        for (provider, label, reason) in [
            ("cline", "Cline", "cline-adapter-unavailable"),
            ("zed", "Zed", "hook-contract-unavailable"),
        ] {
            result.push(ProviderStatus {
                provider: provider.into(),
                label: label.into(),
                supported: false,
                scope: "unavailable",
                installed: false,
                legacy_relay: false,
                detected: base.join(format!(".{provider}")).exists(),
                hook_ready: false,
                settings_path: String::new(),
                hook_path: String::new(),
                events: vec![],
                permission: false,
                reason: Some(reason),
            });
        }
        Ok(result)
    })
    .await
    .map_err(|_| fail("hook-worker-failed"))?
}
fn preview_in(path: &Path, provider: &str, exe: &Path, remove: bool) -> Result<Preview, String> {
    let bytes = read(path)?;
    let next = source_plan(&bytes, provider, exe, remove)?;
    let current_text = if provider == "opencode" {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        serde_json::to_string_pretty(&parse(&bytes)?).unwrap()
    };
    let next_text = String::from_utf8_lossy(&next);
    let backup = path.with_file_name(format!(
        "{}.roadeep-backup-{}",
        path.file_name().unwrap().to_string_lossy(),
        format!(
            "{}-{}",
            {
                let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
                format!("{:04}{:02}{:02}", t.wYear, t.wMonth, t.wDay)
            },
            &hash(&bytes, provider, path, remove)[..16]
        )
    ));
    Ok(Preview {
        provider: provider.into(),
        settings_path: path.to_string_lossy().into(),
        backup: backup.to_string_lossy().into(),
        diff: format!(
            "--- current\n{}\n+++ proposed\n{}\n",
            current_text, next_text
        ),
        fingerprint: hash(&bytes, provider, path, remove),
    })
}
#[tauri::command]
pub async fn coding_hooks_preview(
    window: Window,
    provider: String,
    project_path: Option<String>,
    remove: Option<bool>,
) -> Result<Preview, String> {
    authorize(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = path_in(&home()?, &provider, project_path.as_deref())?;
        preview_in(
            &path,
            &provider,
            &crate::settings::hook_exe_path(),
            remove.unwrap_or(false),
        )
    })
    .await
    .map_err(|_| fail("hook-worker-failed"))?
}
fn write_in(
    path: &Path,
    provider: &str,
    exe: &Path,
    expected: &str,
    remove: bool,
) -> Result<(), String> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| fail("hook-state-unavailable"))?;
    let bytes = read(path)?;
    if hash(&bytes, provider, path, remove) != expected {
        return Err(fail("hook-config-changed"));
    }
    let next = source_plan(&bytes, provider, exe, remove)?;
    if next == bytes || (provider != "opencode" && parse(&next)? == parse(&bytes)?) {
        return Ok(());
    }
    // Reject symlinks/reparse paths rather than following a project folder outside the reviewed scope.
    check_path(path)?;
    fs::create_dir_all(path.parent().ok_or_else(|| fail("hook-invalid-path"))?)
        .map_err(|_| fail("hook-config-write"))?;
    let preview = preview_in(path, provider, exe, remove)?;
    if path.exists() {
        let mut backup = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&preview.backup)
            .map_err(|_| fail("hook-config-backup"))?;
        backup
            .write_all(&bytes)
            .and_then(|_| backup.sync_all())
            .map_err(|_| fail("hook-config-backup"))?;
    }
    if provider == "opencode" && remove {
        if hash(&read(path)?, provider, path, remove) != expected {
            return Err(fail("hook-config-changed"));
        }
        fs::remove_file(path).map_err(|_| fail("hook-config-write"))?;
        return Ok(());
    }
    let temp = path.with_extension(format!("json.roadeep-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|_| fail("hook-config-write"))?;
        f.write_all(&next)
            .and_then(|_| f.sync_all())
            .map_err(|_| fail("hook-config-write"))?;
        drop(f);
        if hash(&read(path)?, provider, path, remove) != expected {
            return Err(fail("hook-config-changed"));
        }
        atomic_replace(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
#[tauri::command]
pub async fn coding_hooks_apply(
    window: Window,
    provider: String,
    project_path: Option<String>,
    fingerprint: String,
    remove: Option<bool>,
) -> Result<ProviderStatus, String> {
    authorize(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let base = home()?;
        let path = path_in(&base, &provider, project_path.as_deref())?;
        let exe = crate::settings::hook_exe_path();
        if !remove.unwrap_or(false) && !exe.is_file() {
            return Err(fail("hook-relay-unavailable"));
        }
        write_in(
            &path,
            &provider,
            &exe,
            &fingerprint,
            remove.unwrap_or(false),
        )?;
        crate::log::line(format!("coding-hooks provider={provider} outcome=applied"));
        Ok(status_in(&base, &provider, project_path.as_deref()))
    })
    .await
    .map_err(|_| fail("hook-worker-failed"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn exe() -> PathBuf {
        PathBuf::from("C:/Tools/roadeep-hook.exe")
    }
    #[test]
    fn every_supported_provider_installs_idempotently_and_preserves_foreign_entries() {
        for p in protocol::PROVIDERS
            .iter()
            .filter(|p| !matches!(**p, "kiro" | "opencode"))
        {
            let e = protocol::events(p)[0];
            let foreign = if matches!(*p, "claude" | "codex" | "gemini") {
                json!({"hooks":[{"command":"foreign","type":"command"}]})
            } else {
                json!({"command":"foreign"})
            };
            let original = json!({"other":42,"hooks":{e:[foreign]}});
            let a = merge(original.clone(), p, &exe(), false).unwrap();
            assert!(installed(&a, p, &exe()), "{p}");
            assert_eq!(a, merge(a.clone(), p, &exe(), false).unwrap());
            let removed = merge(a, p, &exe(), true).unwrap();
            assert_eq!(removed["other"], original["other"]);
            assert_eq!(removed["hooks"], original["hooks"]);
        }
    }
    #[test]
    fn opencode_exact_generated_file_only_and_native_round_trip() {
        let dir = std::env::temp_dir().join(format!("roadeep-oc-hook-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("index.ts");
        let p = preview_in(&path, "opencode", &exe(), false).unwrap();
        write_in(&path, "opencode", &exe(), &p.fingerprint, false).unwrap();
        assert_eq!(read(&path).unwrap(), plugin_source(&exe()).unwrap());
        let p = preview_in(&path, "opencode", &exe(), true).unwrap();
        write_in(&path, "opencode", &exe(), &p.fingerprint, true).unwrap();
        assert!(!path.exists());
        fs::write(&path, b"// foreign plugin").unwrap();
        assert_eq!(
            preview_in(&path, "opencode", &exe(), false).unwrap_err(),
            "hook-plugin-conflict"
        );
        assert_eq!(read(&path).unwrap(), b"// foreign plugin");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn entries_of_the_former_relay_are_replaced_and_flagged() {
        for old in previous_exes() {
        for p in protocol::PROVIDERS.iter().filter(|p| **p != "opencode") {
            let legacy = merge(json!({"other":1}), p, &old, false).unwrap();
            assert!(references(&legacy, p, &old) && references_previous(&legacy, p), "{p}");
            assert!(!installed(&legacy, p, &exe()), "{p}");
            let updated = merge(legacy.clone(), p, &exe(), false).unwrap();
            assert!(installed(&updated, p, &exe()), "{p}");
            assert!(!references(&updated, p, &old), "{p}: the old relay entry is gone");
            let removed = merge(legacy, p, &exe(), true).unwrap();
            assert!(!references(&removed, p, &old), "{p}: remove takes the old entry too");
            assert_eq!(removed["other"], 1);
        }
        let old_plugin = plugin_source(&old).unwrap();
        assert!(previous_plugin(&old_plugin));
        assert_eq!(source_plan(&old_plugin, "opencode", &exe(), false).unwrap(), plugin_source(&exe()).unwrap());
        }
    }
    #[test]
    fn kiro_array_preserves_foreign_entries() {
        let v = json!({"version":"v1","hooks":[{"name":"foreign","trigger":"Stop","action":{"type":"command","command":"echo foreign"}}]});
        let next = merge(v.clone(), "kiro", &exe(), false).unwrap();
        assert!(installed(&next, "kiro", &exe()));
        assert_eq!(next, merge(next.clone(), "kiro", &exe(), false).unwrap());
        assert_eq!(v, merge(next, "kiro", &exe(), true).unwrap());
    }
    #[test]
    fn mixed_group_preserves_foreign_handler() {
        let v = json!({"hooks":{"SessionStart":[{"matcher":"startup","hooks":[{"command":"foreign"},handler(&exe(),"codex","SessionStart").unwrap()]}]}});
        let out = merge(v, "codex", &exe(), true).unwrap();
        assert_eq!(
            out["hooks"]["SessionStart"][0]["hooks"],
            json!([{"command":"foreign"}])
        );
    }
    #[test]
    fn malformed_config_and_unknown_provider_rejected() {
        assert!(merge(json!({"hooks":{"SessionStart":3}}), "codex", &exe(), false).is_err());
        assert!(parse(b"[]").is_err());
        assert!(path_in(Path::new("C:/test"), "unknown", None).is_err());
        assert!(path_in(Path::new("C:/test"), "vscode", Some("../test")).is_err());
    }
    #[test]
    fn exact_ownership_does_not_delete_name_mentions() {
        let v = json!({"hooks":{"SessionStart":[{"hooks":[{"command":"echo roadeep-hook"}]}]}});
        assert_eq!(v, merge(v.clone(), "codex", &exe(), true).unwrap());
    }
    #[test]
    fn stale_preview_never_writes() {
        let dir = std::env::temp_dir().join(format!("roadeep-hooks-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("hooks.json");
        fs::write(&path, b"{}").unwrap();
        let p = preview_in(&path, "codex", &exe(), false).unwrap();
        fs::write(&path, b"{\"other\":1}").unwrap();
        assert_eq!(
            write_in(&path, "codex", &exe(), &p.fingerprint, false).unwrap_err(),
            "hook-config-changed"
        );
        assert_eq!(read(&path).unwrap(), b"{\"other\":1}");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn temp_apply_backups_and_remove_preserve_data() {
        let dir = std::env::temp_dir().join(format!("roadeep-hooks-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("hooks.json");
        fs::write(&path, b"{\"other\":42}").unwrap();
        let p = preview_in(&path, "codex", &exe(), false).unwrap();
        write_in(&path, "codex", &exe(), &p.fingerprint, false).unwrap();
        assert!(installed(
            &parse(&read(&path).unwrap()).unwrap(),
            "codex",
            &exe()
        ));
        let p = preview_in(&path, "codex", &exe(), true).unwrap();
        write_in(&path, "codex", &exe(), &p.fingerprint, true).unwrap();
        assert_eq!(parse(&read(&path).unwrap()).unwrap(), json!({"other":42}));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
        fs::remove_dir_all(dir).unwrap();
    }
}
