//! Transient engines never receive shell commands or prompt text in process arguments.
use std::{
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

#[cfg(windows)]
struct Job(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
impl Job {
    fn new() -> Result<Self, String> {
        use windows::Win32::System::JobObjects::*;
        unsafe {
            let job = Self(
                CreateJobObjectW(None, windows::core::PCWSTR::null())
                    .map_err(|_| "local-containment")?,
            );
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            info.BasicLimitInformation.ActiveProcessLimit = 4;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|_| "local-containment")?;
            Ok(job)
        }
    }
    fn assign(&self, child: &tokio::process::Child) -> Result<(), String> {
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtResumeProcess(process: *mut std::ffi::c_void) -> i32;
        }
        let handle = child.raw_handle().ok_or("local-containment")?;
        unsafe {
            AssignProcessToJobObject(self.0, windows::Win32::Foundation::HANDLE(handle))
                .map_err(|_| "local-containment")?;
            if NtResumeProcess(handle) < 0 {
                return Err("local-containment".into());
            }
        }
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        if unsafe { windows::Win32::Foundation::CloseHandle(self.0) }.is_err() {
            crate::log::line("local engine: containment cleanup failed");
        }
    }
}
async fn bounded<R: AsyncRead + Unpin>(mut stream: R, limit: usize) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|_| "local-output-read")?;
        if n == 0 {
            return Ok(result);
        }
        if result.len() + n > limit {
            return Err("local-output-limit".into());
        }
        result.extend_from_slice(&buffer[..n]);
    }
}
pub async fn run(
    exe: &Path,
    args: &[String],
    input: &[u8],
    cancelled: Arc<AtomicBool>,
    deadline: Duration,
) -> Result<Vec<u8>, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("local-cancelled".into());
    }
    #[cfg(windows)]
    let job = Job::new()?;
    let mut command = Command::new(exe);
    command
        .current_dir(exe.parent().ok_or("local-runtime-path")?)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000 | 0x0000_0004);
    // A configured environment must not redirect model loads or enable engine telemetry.
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if name.starts_with("LLAMA_") || name.starts_with("GGML_") || name.starts_with("PIPER_") {
            command.env_remove(key);
        }
    }
    let mut child = command.spawn().map_err(|_| "local-engine-start")?;
    #[cfg(windows)]
    if let Err(error) = job.assign(&child) {
        let _ = child.start_kill();
        return Err(error);
    }
    let mut stdin = child.stdin.take().ok_or("local-engine-input")?;
    let stdout = child.stdout.take().ok_or("local-engine-output")?;
    let stderr = child.stderr.take().ok_or("local-engine-output")?;
    let work = async {
        let write = async move {
            stdin
                .write_all(input)
                .await
                .map_err(|_| "local-engine-input".to_string())?;
            stdin
                .shutdown()
                .await
                .map_err(|_| "local-engine-input".to_string())?;
            // ChildStdin shutdown alone does not close the Windows pipe. Piper reads until EOF.
            drop(stdin);
            Ok::<(), String>(())
        };
        let (out, _, _, status) = tokio::try_join!(
            bounded(stdout, 128 * 1024),
            bounded(stderr, 128 * 1024),
            write,
            async {
                child
                    .wait()
                    .await
                    .map_err(|_| "local-engine-wait".to_string())
            }
        )?;
        if !status.success() {
            return Err("local-engine-failed".into());
        }
        Ok(out)
    };
    let cancel = async {
        loop {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let outcome = tokio::select! {
        result = tokio::time::timeout(deadline, work) => result.unwrap_or_else(|_| Err("local-timeout".to_string())),
        _ = cancel => Err("local-cancelled".into()),
    };
    if outcome.is_err() {
        #[cfg(windows)]
        drop(job);
        // Wait for handle cleanup before the operation removes its prompt/audio workspace.
        let cleanup = async {
            let _ = child.start_kill();
            child.wait().await
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(3), cleanup).await,
            Ok(Ok(_))
        ) {
            crate::log::line("local engine: process cleanup incomplete");
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn bounded_read_rejects_overflow() {
        assert_eq!(
            super::bounded(&b"abc"[..], 2).await.unwrap_err(),
            "local-output-limit"
        );
        assert_eq!(super::bounded(&b"abc"[..], 3).await.unwrap(), b"abc");
    }
    #[tokio::test]
    async fn cancelled_never_launches() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        assert_eq!(
            super::run(
                std::path::Path::new("missing.exe"),
                &[],
                &[],
                flag,
                std::time::Duration::from_secs(1)
            )
            .await
            .unwrap_err(),
            "local-cancelled"
        );
    }
}
