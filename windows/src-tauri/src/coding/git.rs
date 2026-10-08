//! Explicit, read-only working-tree inspection. Never executes a repository hook/tool.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Semaphore;

static INSPECTIONS: Semaphore = Semaphore::const_new(2);
const OUTPUT_CAP: usize = 1024 * 1024;
const PATCH_CAP: usize = 64 * 1024;
#[derive(Serialize)]
pub struct GitFile { path: String, status: String }
#[derive(Serialize)]
pub struct GitInspection { root: String, head: Option<String>, at: u64, files: Vec<GitFile>, patch: String, truncated: bool }
pub(super) fn local_directory(cwd: &str) -> Result<PathBuf, String> {
    if cwd.is_empty() || cwd.len() > 4096 || cwd.chars().any(char::is_control) || cwd.starts_with("\\\\") || cwd.starts_with("//") { return Err(error("coding-invalid-cwd")); }
    let path = Path::new(cwd);
    if !path.is_absolute() { return Err(error("coding-invalid-cwd")); }
    check_local_drive(path)?;
    if !path.is_dir() { return Err(error("coding-invalid-cwd")); }
    let path = std::fs::canonicalize(path).map_err(|_| error("coding-invalid-cwd"))?;
    let raw = path.to_string_lossy();
    if raw.starts_with(r"\\?\UNC\") || (raw.starts_with(r"\\") && !raw.starts_with(r"\\?\")) { return Err(error("coding-invalid-cwd")); }
    // Canonical local Windows paths use an extended prefix that shells do not accept.
    let path = if let Some(raw) = raw.strip_prefix(r"\\?\") { PathBuf::from(raw) } else { path };
    check_local_drive(&path)?;
    Ok(path)
}
fn check_local_drive(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::path::{Component, Prefix};
        #[link(name = "kernel32")]
        unsafe extern "system" { fn GetDriveTypeW(root: *const u16) -> u32; }
        let drive = match path.components().next() { Some(Component::Prefix(prefix)) => match prefix.kind() { Prefix::Disk(drive) => drive, _ => return Err(error("coding-invalid-cwd")) }, _ => return Err(error("coding-invalid-cwd")) };
        let root: Vec<u16> = std::ffi::OsString::from(format!("{}:\\", drive as char)).encode_wide().chain(Some(0)).collect();
        // The drive root remains valid and NUL-terminated throughout the query.
        if !matches!(unsafe { GetDriveTypeW(root.as_ptr()) }, 2 | 3 | 6) { return Err(error("coding-invalid-cwd")); }
    }
    Ok(())
}
pub(super) fn on_path(stem: &str, extensions: &[&str]) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    find_on_path(&paths, stem, extensions)
}
fn find_on_path(paths: &std::ffi::OsStr, stem: &str, extensions: &[&str]) -> Option<PathBuf> {
    for directory in std::env::split_paths(paths).filter(|p| p.is_absolute() && !p.to_string_lossy().starts_with(r"\\")) {
        for extension in extensions { let candidate = directory.join(format!("{stem}{extension}")); if candidate.is_file() { return Some(candidate); } }
    }
    None
}
fn git_executable() -> Option<PathBuf> {
    let executable = on_path("git", &[".exe"])?;
    let directory = executable.parent()?;
    // Git for Windows' cmd/bin git.exe is a dispatcher that spawns the native
    // binary. Run the installed native sibling directly so containment permits
    // only Git itself, never a general dispatcher/helper process.
    if directory.file_name().is_some_and(|name| name.eq_ignore_ascii_case("cmd") || name.eq_ignore_ascii_case("bin")) {
        if let Some(installation) = directory.parent() {
            for architecture in ["mingw64", "mingw32"] {
                let native = installation.join(architecture).join("bin").join("git.exe");
                if native.is_file() { return Some(native); }
            }
        }
    }
    Some(executable)
}
pub(super) fn error(code: &'static str) -> String { crate::log::line(format!("coding: {code}")); code.into() }
async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(mut reader: R, cap: usize) -> Result<Vec<u8>, String> {
    let mut result = Vec::new(); let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer).await.map_err(|_| error("git-read-failed"))?;
        if count == 0 { return Ok(result); }
        if result.len() + count > cap { return Err(error("git-output-limit")); }
        result.extend_from_slice(&buffer[..count]);
    }
}
/// Each Git invocation is assigned before it runs. Even a concurrently changed
/// repository config cannot start a helper; timeout/drop kills the contained process.
struct InspectionJob(windows::Win32::Foundation::HANDLE);
// This uniquely owned kernel handle is valid when the async task moves threads.
unsafe impl Send for InspectionJob {}
impl InspectionJob {
    fn new() -> Result<Self, String> {
        use windows::Win32::System::JobObjects::*;
        // No pointers escape: Windows copies the supplied limit structure.
        unsafe {
            let job = Self(CreateJobObjectW(None, windows::core::PCWSTR::null()).map_err(|_| error("git-containment-failed"))?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            info.BasicLimitInformation.ActiveProcessLimit = 1;
            SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, &info as *const _ as *const std::ffi::c_void, std::mem::size_of_val(&info) as u32).map_err(|_| error("git-containment-failed"))?;
            Ok(job)
        }
    }
    fn contain_and_resume(&self, child: &tokio::process::Child) -> Result<(), String> {
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        #[link(name = "ntdll")]
        unsafe extern "system" { fn NtResumeProcess(process: *mut std::ffi::c_void) -> i32; }
        let handle = child.raw_handle().ok_or_else(|| error("git-containment-failed"))?;
        // Child owns a valid process handle. Assignment happens while its primary
        // thread is suspended, closing the helper-execution race before resume.
        unsafe {
            AssignProcessToJobObject(self.0, windows::Win32::Foundation::HANDLE(handle)).map_err(|_| error("git-containment-failed"))?;
            if NtResumeProcess(handle) < 0 { return Err(error("git-containment-failed")); }
        }
        Ok(())
    }
}
impl Drop for InspectionJob {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE also applies when the inspection future is cancelled.
        if unsafe { windows::Win32::Foundation::CloseHandle(self.0) }.is_err() { crate::log::line("coding: Git containment cleanup failed"); }
    }
}

async fn git(executable: &Path, cwd: &Path, args: &[&str], filters: &[String]) -> Result<(bool, Vec<u8>), String> {
    let job = InspectionJob::new()?;
    let mut command = Command::new(executable);
    command.creation_flags(0x0800_0000 | 0x0000_0004).kill_on_drop(true).current_dir(std::env::temp_dir())
        .args(["--no-pager", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=NUL", "-c", "diff.external=", "-c", "core.pager=cat", "-c", "submodule.recurse=false", "-c", "protocol.allow=never", "-C"])
        .arg(cwd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    // Inherited Git overrides must not redirect the requested repository or execute helpers.
    for (key, _) in std::env::vars_os() { if key.to_string_lossy().to_ascii_uppercase().starts_with("GIT_") { command.env_remove(key); } }
    for driver in filters {
        for property in ["clean", "smudge", "process"] { command.args(["-c", &format!("filter.{driver}.{property}=")]); }
        command.args(["-c", &format!("filter.{driver}.required=false")]);
    }
    command.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "NUL");
    command.args(args);
    let mut child = command.spawn().map_err(|_| error("git-launch-failed"))?;
    if let Err(code) = job.contain_and_resume(&child) { let _ = child.start_kill(); return Err(code); }
    let stdout = child.stdout.take().ok_or_else(|| error("git-read-failed"))?;
    let stderr = child.stderr.take().ok_or_else(|| error("git-read-failed"))?;
    let result = tokio::try_join!(async { child.wait().await.map_err(|_| error("git-wait-failed")) }, read_bounded(stdout, OUTPUT_CAP), read_bounded(stderr, 8192));
    match result {
        Ok((status, output, _)) => Ok((status.success(), output)),
        Err(code) => { let _ = child.kill().await; Err(code) }
    }
}
fn status_files(output: &[u8]) -> (Vec<GitFile>, bool) {
    let mut pieces = output.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut files = Vec::new(); let mut truncated = false;
    while let Some(record) = pieces.next() {
        if record.len() < 4 { continue; }
        if record[0] == b'R' || record[0] == b'C' || record[1] == b'R' || record[1] == b'C' { let _ = pieces.next(); }
        if files.len() == 500 { truncated = true; break; }
        files.push(GitFile { status: String::from_utf8_lossy(&record[..2]).into_owned(), path: super::parser::clean(&String::from_utf8_lossy(&record[3..]), 2048) });
    }
    (files, truncated)
}
fn patch_text(bytes: &[u8]) -> (String, bool) {
    let mut patch = super::parser::clean(&String::from_utf8_lossy(bytes), OUTPUT_CAP);
    let truncated = patch.len() > PATCH_CAP;
    if truncated { let mut boundary = PATCH_CAP - 12; while !patch.is_char_boundary(boundary) { boundary -= 1; } patch.truncate(boundary); patch.push_str("\n[truncated]"); }
    (patch, truncated)
}
async fn inspect(executable: &Path, cwd: &Path) -> Result<GitInspection, String> {
    let (_, filter_keys) = git(executable, cwd, &["config", "--null", "--name-only", "--get-regexp", "^filter\\..*\\.(clean|smudge|process|required)$"], &[]).await?;
    let mut filters = Vec::new();
    for key in filter_keys.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let key = std::str::from_utf8(key).map_err(|_| error("git-unsafe-filter-config"))?;
        let driver = key.strip_prefix("filter.").and_then(|s| s.rsplit_once('.').map(|p| p.0)).ok_or_else(|| error("git-unsafe-filter-config"))?;
        if driver.len() > 128 || !driver.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) || filters.len() > 512 { return Err(error("git-unsafe-filter-config")); }
        if !filters.iter().any(|v| v == driver) { filters.push(driver.to_string()); }
    }
    let (success, root) = git(executable, cwd, &["rev-parse", "--show-toplevel"], &filters).await?;
    if !success { return Err(error("git-not-repository")); }
    let root = super::parser::clean(String::from_utf8_lossy(&root).trim(), 4096);
    let (head_ok, head) = git(executable, cwd, &["rev-parse", "--verify", "HEAD"], &filters).await?;
    let head = head_ok.then(|| String::from_utf8_lossy(&head).trim().to_owned()).filter(|s| matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let (status_ok, status) = git(executable, cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=normal", "--ignore-submodules=all"], &filters).await?;
    if !status_ok { return Err(error("git-inspection-failed")); }
    let (staged_ok, staged) = git(executable, cwd, &["diff", "--cached", "--no-ext-diff", "--no-textconv", "--no-renames", "--ignore-submodules=all", "--no-color", "--"], &filters).await?;
    let (unstaged_ok, unstaged) = git(executable, cwd, &["diff", "--no-ext-diff", "--no-textconv", "--no-renames", "--ignore-submodules=all", "--no-color", "--"], &filters).await?;
    if !staged_ok || !unstaged_ok { return Err(error("git-inspection-failed")); }
    if staged.len() + unstaged.len() + 40 > OUTPUT_CAP { return Err(error("git-output-limit")); }
    let mut diff = Vec::new();
    if !staged.is_empty() { diff.extend_from_slice(b"# Staged changes\n"); diff.extend_from_slice(&staged); }
    if !unstaged.is_empty() { diff.extend_from_slice(b"# Unstaged changes\n"); diff.extend_from_slice(&unstaged); }
    let (files, files_truncated) = status_files(&status); let (patch, patch_truncated) = patch_text(&diff);
    Ok(GitInspection { root, head, at: super::now(), files, patch, truncated: files_truncated || patch_truncated })
}
#[tauri::command]
pub async fn coding_git_inspect(cwd: String) -> Result<GitInspection, String> {
    let _permit = INSPECTIONS.try_acquire().map_err(|_| error("git-busy"))?;
    let cwd = tauri::async_runtime::spawn_blocking(move || local_directory(&cwd)).await.map_err(|_| error("git-worker-failed"))??;
    let executable = git_executable().ok_or_else(|| error("git-unavailable"))?;
    let request = uuid::Uuid::new_v4(); let started = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(10), inspect(&executable, &cwd)).await.map_err(|_| error("git-timeout"))?;
    crate::log::line(format!("coding: git inspection request={request} durationMs={} success={}", started.elapsed().as_millis(), result.is_ok()));
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    #[test]
    fn cwd_path_search_status_and_patch_are_bounded() {
        for invalid in ["relative", "\\\\server\\share", "//server/share", "C:\\nul\0bad", "C:\\bad\n"] { assert!(local_directory(invalid).is_err()); }
        let dir = std::env::temp_dir().join(format!("roadeep-git-path-{}", uuid::Uuid::new_v4())); std::fs::create_dir_all(&dir).unwrap(); std::fs::write(dir.join("git.exe"), "fixture").unwrap();
        let paths = std::env::join_paths([PathBuf::from("relative"), dir.clone()]).unwrap(); assert_eq!(find_on_path(&paths, "git", &[".exe"]), Some(dir.join("git.exe")));
        let status = b" M file one.txt\0R  new.txt\0old.txt\0?? untracked.txt\0"; let (files, truncated) = status_files(status); assert_eq!(files.len(), 3); assert_eq!(files[1].path, "new.txt"); assert!(!truncated);
        let many = b" M x\0".repeat(501); assert_eq!(status_files(&many).0.len(), 500); assert!(status_files(&many).1);
        let (patch, truncated) = patch_text(format!("API_KEY=private-value\n{}", "a".repeat(PATCH_CAP + 10)).as_bytes()); assert!(!patch.contains("private-value")); assert!(patch.len() <= PATCH_CAP); assert!(truncated);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn repository_fixture_returns_working_tree_without_external_diff() {
        let Some(exe) = git_executable() else { panic!("Git required for repository fixture"); };
        let dir = std::env::temp_dir().join(format!("roadeep-git-test-{}", uuid::Uuid::new_v4())); std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| { let output = std::process::Command::new(&exe).current_dir(&dir).args(args).creation_flags(0x0800_0000).output().unwrap(); assert!(output.status.success(), "fixture setup"); };
        run(&["init", "--quiet"]); run(&["config", "user.name", "Fixture"]); run(&["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(dir.join("tracked.txt"), "original\n").unwrap(); run(&["add", "--", "tracked.txt"]); run(&["commit", "--quiet", "-m", "fixture"]);
        run(&["config", "diff.external", "this-command-must-never-run"]); run(&["config", "core.fsmonitor", "this-command-must-never-run"]);
        run(&["config", "filter.fixture.clean", "this-command-must-never-run"]); run(&["config", "filter.fixture.process", "this-command-must-never-run"]); run(&["config", "filter.fixture.required", "true"]);
        std::fs::write(dir.join(".gitattributes"), "tracked.txt filter=fixture\n").unwrap();
        std::fs::write(dir.join("tracked.txt"), "API_KEY=private-value\nchanged\n").unwrap(); std::fs::write(dir.join("untracked.txt"), "untracked contents must not appear").unwrap();
        let evidence = inspect(&exe, &dir).await.unwrap(); assert!(evidence.head.is_some()); assert_eq!(evidence.files.len(), 3); assert!(evidence.patch.contains("changed")); assert!(!evidence.patch.contains("private-value")); assert!(!evidence.patch.contains("untracked contents"));
        let marker = dir.join("helper-must-not-run.txt");
        run(&["config", "filter.fixture.clean", &format!("cmd.exe /d /c echo forbidden > \"{}\"", marker.display())]);
        run(&["config", "--unset", "filter.fixture.process"]);
        // Deliberately omit filter overrides: the process Job is the final guard.
        let (success, _) = git(&exe, &dir, &["diff", "--no-ext-diff", "--no-textconv", "--", "tracked.txt"], &[]).await.unwrap();
        assert!(!success); assert!(!marker.exists(), "Git must never create a helper process");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn unborn_staged_and_unstaged_evidence_does_not_change_index() {
        let exe = git_executable().unwrap();
        let dir = std::env::temp_dir().join(format!("roadeep-unborn-test-{}", uuid::Uuid::new_v4())); std::fs::create_dir_all(&dir).unwrap();
        for args in [vec!["init", "--quiet"], vec!["config", "core.autocrlf", "false"]] { assert!(std::process::Command::new(&exe).args(args).current_dir(&dir).creation_flags(0x0800_0000).status().unwrap().success()); }
        std::fs::write(dir.join("file.txt"), "staged content\n").unwrap(); assert!(std::process::Command::new(&exe).args(["add", "--", "file.txt"]).current_dir(&dir).creation_flags(0x0800_0000).status().unwrap().success());
        let index = std::fs::read(dir.join(".git/index")).unwrap();
        std::fs::write(dir.join("file.txt"), "unstaged content\n").unwrap(); let evidence = inspect(&exe, &dir).await.unwrap();
        assert!(evidence.head.is_none()); assert!(evidence.patch.contains("staged content")); assert!(evidence.patch.contains("unstaged content")); assert_eq!(std::fs::read(dir.join(".git/index")).unwrap(), index); assert!(!dir.join(".git/index.lock").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn stream_output_cap_and_timeout_stop_unbounded_reads() {
        let (mut writer, reader) = tokio::io::duplex(64); tokio::spawn(async move { use tokio::io::AsyncWriteExt; writer.write_all(&[0; 32]).await.unwrap(); });
        assert_eq!(read_bounded(reader, 10).await.unwrap_err(), "git-output-limit");
        let (_writer, reader) = tokio::io::duplex(64);
        assert!(tokio::time::timeout(Duration::from_millis(10), read_bounded(reader, 10)).await.is_err());
    }
}
