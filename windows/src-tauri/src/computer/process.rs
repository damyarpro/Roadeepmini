//! Docker is invoked directly, with an explicit local endpoint and contained process tree.
use std::{path::{Path, PathBuf}, process::Stdio, time::Duration};
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader}, process::Command};
use serde_json::Value;
use super::{error, network};
const FRAME_CAP: usize = 3 * 1024 * 1024;
pub fn executable() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths).filter(|p| p.is_absolute() && !p.to_string_lossy().starts_with(r"\\")) {
        let file = dir.join(if cfg!(windows) { "docker.exe" } else { "docker" });
        if file.is_file() { return Some(file); }
    }
    None
}
#[cfg(windows)]
struct Job(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
impl Job {
    fn new() -> Result<Self, String> {
        use windows::Win32::System::JobObjects::*;
        unsafe {
            let job = Self(CreateJobObjectW(None, windows::core::PCWSTR::null()).map_err(|_| error("computer-containment"))?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            info.BasicLimitInformation.ActiveProcessLimit = 8;
            SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, &info as *const _ as *const std::ffi::c_void, std::mem::size_of_val(&info) as u32).map_err(|_| error("computer-containment"))?;
            Ok(job)
        }
    }
    fn assign(&self, child: &tokio::process::Child) -> Result<(), String> {
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        #[link(name = "ntdll")]
        unsafe extern "system" { fn NtResumeProcess(process: *mut std::ffi::c_void) -> i32; }
        let handle = child.raw_handle().ok_or_else(|| error("computer-containment"))?;
        unsafe {
            AssignProcessToJobObject(self.0, windows::Win32::Foundation::HANDLE(handle)).map_err(|_| error("computer-containment"))?;
            if NtResumeProcess(handle) < 0 { return Err(error("computer-containment")); }
        }
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) { if unsafe { windows::Win32::Foundation::CloseHandle(self.0) }.is_err() { crate::log::line("computer: containment cleanup failed"); } }
}
fn command(exe: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000 | 0x0000_0004);
    cmd.kill_on_drop(true).current_dir(std::env::temp_dir())
        .args(["--host", if cfg!(windows) { "npipe:////./pipe/docker_engine" } else { "unix:///var/run/docker.sock" }]).args(args)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, _) in std::env::vars_os() { if key.to_string_lossy().to_ascii_uppercase().starts_with("DOCKER_") { cmd.env_remove(key); } }
    cmd
}
async fn bounded<R: tokio::io::AsyncRead + Unpin>(mut read: R, cap: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new(); let mut buf = [0; 8192];
    loop {
        let n = read.read(&mut buf).await.map_err(|_| error("computer-process-read"))?;
        if n == 0 { return Ok(out); }
        if out.len() + n > cap { return Err(error("computer-output-limit")); }
        out.extend_from_slice(&buf[..n]);
    }
}
pub async fn run(exe: &Path, args: &[String], operation: Option<&Value>, duration: Duration) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    let job = Job::new()?;
    let mut child = command(exe, args).spawn().map_err(|_| error("computer-process-start"))?;
    #[cfg(windows)]
    if let Err(code) = job.assign(&child) { let _ = child.start_kill(); return Err(code); }
    let mut input = child.stdin.take().ok_or_else(|| error("computer-process-read"))?;
    let output = child.stdout.take().ok_or_else(|| error("computer-process-read"))?;
    let stderr = child.stderr.take().ok_or_else(|| error("computer-process-read"))?;
    let work = async {
        if let Some(operation) = operation {
            input.write_all(format!("{}\n", operation).as_bytes()).await.map_err(|_| error("computer-process-write"))?;
        } else { input.shutdown().await.map_err(|_| error("computer-process-write"))?; }
        let read = async {
            if operation.is_none() { return bounded(output, 128 * 1024).await; }
            let mut reader = BufReader::new(output); let mut requests = 0;
            loop {
                let mut frame = Vec::new();
                // take() bounds allocation even if the runtime never writes a newline.
                let n = (&mut reader).take((FRAME_CAP + 1) as u64).read_until(b'\n', &mut frame).await.map_err(|_| error("computer-process-read"))?;
                if n == 0 || n > FRAME_CAP { return Err(error("computer-output-limit")); }
                let value: Value = serde_json::from_slice(&frame).map_err(|_| error("computer-runtime-protocol"))?;
                if value["kind"] == "network" {
                    requests += 1;
                    if requests > 80 { return Err(error("computer-network-limit")); }
                    let reply = network::fetch(&value).await;
                    input.write_all(format!("{}\n", reply).as_bytes()).await.map_err(|_| error("computer-process-write"))?;
                } else if value["kind"] == "result" {
                    input.shutdown().await.map_err(|_| error("computer-process-write"))?;
                    return Ok(frame);
                } else { return Err(error("computer-runtime-protocol")); }
            }
        };
        let (result, _, exit) = tokio::try_join!(read, bounded(stderr, 8192), async { child.wait().await.map_err(|_| error("computer-process-wait")) })?;
        if !exit.success() { return Err(error("computer-runtime-unavailable")); }
        Ok(result)
    };
    match tokio::time::timeout(duration, work).await {
        Ok(result) => result,
        Err(_) => { let _ = child.kill().await; Err(error("computer-timeout")) }
    }
}
