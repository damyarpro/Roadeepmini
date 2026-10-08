// stdio transport: a local MCP server process speaking newline-delimited
// JSON-RPC on stdin/stdout.
//
// No shell is involved in finding the program: we walk %PATH% × %PATHEXT%
// ourselves (never the current directory). An .exe is started directly (Rust
// quotes its arguments). A .cmd/.bat (npx.cmd, uvx.cmd…) can only run through
// cmd.exe, which reads `%`, `^`, `&`, `|`, `<`, `>` and `"` as syntax even in
// places Rust's quoting can't protect — so for those launchers we build the
// command line ourselves and refuse any argument containing one of them.
//
// Every process goes into a Job object that kills its whole tree when the
// handle closes: npx's own node child dies with it, on disable, removal, app
// exit, or a crash of the app (the OS closes the handle then).

use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

use super::rpc::{self, Incoming, RpcError};
use super::tools::one_line;
use crate::errors;
use crate::log;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// One JSON-RPC message per line; a bigger line is a broken server.
const MAX_LINE: usize = 4 * 1024 * 1024;
const STDERR_LINES: usize = 50;
const STDERR_BYTES: usize = 2048;
const ALLOWED_EXTS: &[&str] = &["com", "exe", "bat", "cmd"];

// ── Finding the program ───────────────────────────────────────────────────────

fn lower_ext(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// Our own `where`: an absolute path, or a bare name searched on `path` with the
/// %PATHEXT% extensions we can run. Relative paths with a folder part are
/// refused (that would mean "relative to wherever the app was started").
pub fn resolve(command: &str, path: Option<&OsStr>, pathext: &str) -> Option<PathBuf> {
    let mut exts: Vec<String> = pathext
        .split(';')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| e.strip_prefix('.').is_some_and(|x| ALLOWED_EXTS.contains(&x)))
        .collect();
    if exts.is_empty() {
        exts = ALLOWED_EXTS.iter().map(|e| format!(".{e}")).collect();
    }
    let has_ext = lower_ext(Path::new(command)).is_some_and(|e| ALLOWED_EXTS.contains(&e.as_str()));
    let try_base = |base: PathBuf| -> Option<PathBuf> {
        if has_ext && base.is_file() {
            return Some(base);
        }
        exts.iter().find_map(|ext| {
            let mut name: OsString = base.clone().into_os_string();
            name.push(ext);
            let candidate = PathBuf::from(name);
            candidate.is_file().then_some(candidate)
        })
    };
    let as_path = Path::new(command);
    if as_path.is_absolute() {
        return try_base(as_path.to_path_buf());
    }
    if command.contains(['\\', '/', ':']) {
        return None;
    }
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .find_map(|dir| try_base(dir.join(command)))
}

/// What to start.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub program: PathBuf,
    /// Passed as separate arguments (Rust quotes them).
    pub args: Vec<String>,
    /// For cmd.exe: the command line after the program, used verbatim.
    pub raw: Option<String>,
}

/// One argument for a cmd.exe command line, or the coded refusal. Inside
/// double quotes cmd still expands `%VAR%`, and `"` would end the quoting, so
/// those (and the other metacharacters, for good measure) are refused. The
/// program behind the .cmd parses its own command line with the MSVC rules,
/// where backslashes before a closing quote must be doubled.
pub fn quote_for_cmd(arg: &str) -> Result<String, String> {
    if arg.chars().any(|c| matches!(c, '"' | '%' | '^' | '&' | '|' | '<' | '>') || c.is_control()) {
        return Err(errors::coded(errors::MCPC_UNSAFE_ARG, &[&one_line(arg, 80)]));
    }
    let trailing = arg.len() - arg.trim_end_matches('\\').len();
    Ok(format!("\"{arg}{}\"", "\\".repeat(trailing)))
}

pub fn plan(command: &str, args: &[String], path: Option<&OsStr>, pathext: &str, cmd_exe: &Path) -> Result<Launch, String> {
    let program = resolve(command, path, pathext).ok_or_else(|| errors::coded(errors::MCPC_COMMAND_NOT_FOUND, &[&one_line(command, 120)]))?;
    match lower_ext(&program).as_deref() {
        Some("exe" | "com") => Ok(Launch { program, args: args.to_vec(), raw: None }),
        Some("cmd" | "bat") => {
            let mut line = quote_for_cmd(&program.to_string_lossy())?;
            for arg in args {
                line.push(' ');
                line.push_str(&quote_for_cmd(arg)?);
            }
            // /d: no AutoRun commands; /v:off: `!` stays literal even if delayed
            // expansion is on in the registry; /s /c "…": cmd strips exactly
            // the outer quotes and runs the rest as is.
            Ok(Launch { program: cmd_exe.to_path_buf(), args: Vec::new(), raw: Some(format!("/d /v:off /s /c \"{line}\"")) })
        }
        _ => Err(errors::coded(errors::MCPC_COMMAND_NOT_FOUND, &[&one_line(command, 120)])),
    }
}

/// %SystemRoot%\System32\cmd.exe, from the OS rather than %PATH% or %ComSpec%.
pub fn cmd_exe() -> PathBuf {
    let mut buf = [0u16; 260];
    let n = unsafe { windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut buf)) } as usize;
    let dir = if n > 0 && n < buf.len() { PathBuf::from(String::from_utf16_lossy(&buf[..n])) } else { PathBuf::from(r"C:\Windows\System32") };
    dir.join("cmd.exe")
}

/// The real environment's plan for a configured command.
pub fn plan_for(command: &str, args: &[String]) -> Result<Launch, String> {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    plan(command, args, std::env::var_os("PATH").as_deref(), &pathext, &cmd_exe())
}

// ── Job object ────────────────────────────────────────────────────────────────

struct Job(windows::Win32::Foundation::HANDLE);

// A job handle is a plain kernel handle, usable from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    fn new() -> Option<Job> {
        use windows::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?;
            let job = Job(handle);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(job)
        }
    }

    fn assign(&self, process: std::os::windows::io::RawHandle) -> bool {
        unsafe { windows::Win32::System::JobObjects::AssignProcessToJobObject(self.0, windows::Win32::Foundation::HANDLE(process)).is_ok() }
    }
}

impl Job {
    fn terminate(&self) {
        unsafe {
            let _ = windows::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

// ── The wire ──────────────────────────────────────────────────────────────────

type Waiter = oneshot::Sender<Result<Value, RpcError>>;

struct Inner {
    stdin: tokio::sync::Mutex<Option<Box<dyn AsyncWrite + Send + Unpin>>>,
    pending: Mutex<HashMap<u64, Waiter>>,
    next_id: AtomicU64,
    closed: AtomicBool,
    /// Set when we stop it on purpose: the exit is then not an error.
    stopping: AtomicBool,
    stderr: Mutex<VecDeque<String>>,
    /// `notifications/tools/list_changed` was seen.
    dirty: Arc<AtomicBool>,
}

impl Inner {
    fn stderr_tail(&self) -> String {
        let lines = self.stderr.lock().unwrap();
        let joined = lines.iter().cloned().collect::<Vec<_>>().join("\n");
        let start = joined.len().saturating_sub(STDERR_BYTES);
        let start = (start..joined.len()).find(|&i| joined.is_char_boundary(i)).unwrap_or(joined.len());
        joined[start..].to_string()
    }

    fn exited_error(&self) -> String {
        let tail = self.stderr_tail();
        let head = errors::coded(errors::MCPC_EXITED, &[]);
        if tail.is_empty() {
            head
        } else {
            format!("{head}\n{tail}")
        }
    }

    async fn send(&self, msg: &Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(msg).map_err(|e| errors::coded(errors::MCPC_PROTOCOL, &[&e.to_string()]))?;
        line.push(b'\n');
        let mut guard = self.stdin.lock().await;
        let Some(stdin) = guard.as_mut() else { return Err(self.exited_error()) };
        let written = async {
            stdin.write_all(&line).await?;
            stdin.flush().await
        };
        if written.await.is_err() {
            *guard = None;
            return Err(self.exited_error());
        }
        Ok(())
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let waiters: Vec<Waiter> = self.pending.lock().unwrap().drain().map(|(_, w)| w).collect();
        drop(waiters); // their receivers see the channel closed
    }
}

/// Removes the pending entry if the request future is dropped (cancel button,
/// timeout) and tells the server, as the spec asks.
struct PendingGuard {
    inner: Arc<Inner>,
    id: u64,
    cancel: bool,
    done: bool,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        self.inner.pending.lock().unwrap().remove(&self.id);
        if self.cancel && !self.inner.closed.load(Ordering::SeqCst) {
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let inner = self.inner.clone();
                let msg = rpc::notification("notifications/cancelled", Some(json!({ "requestId": self.id, "reason": "Cancelled by the user" })));
                rt.spawn(async move {
                    let _ = inner.send(&msg).await;
                });
            }
        }
    }
}

pub struct StdioWire {
    inner: Arc<Inner>,
    child: Mutex<Option<tokio::process::Child>>,
    job: Option<Job>,
}

pub type OnExit = Box<dyn FnOnce(String) + Send>;

impl StdioWire {
    /// Starts the process. `env` is added to the inherited environment.
    pub fn spawn(launch: &Launch, env: &[(String, String)], dirty: Arc<AtomicBool>, on_exit: OnExit) -> Result<StdioWire, String> {
        let mut std_cmd = std::process::Command::new(&launch.program);
        {
            use std::os::windows::process::CommandExt;
            std_cmd.args(&launch.args);
            if let Some(raw) = &launch.raw {
                std_cmd.raw_arg(raw);
            }
            std_cmd.creation_flags(CREATE_NO_WINDOW);
        }
        // A predictable working folder rather than wherever the app was started.
        if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from).filter(|p| p.is_dir()) {
            std_cmd.current_dir(home);
        }
        for (name, value) in env {
            std_cmd.env(name, value);
        }
        std_cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut cmd = tokio::process::Command::from(std_cmd);
        cmd.kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| errors::coded(errors::MCPC_SPAWN, &[&e.to_string()]))?;

        let job = Job::new();
        match (&job, child.raw_handle()) {
            (Some(job), Some(handle)) if job.assign(handle) => {}
            _ => log::line("mcpc: could not put a server process in a job; its children may outlive it"),
        }
        let stdin = child.stdin.take().ok_or_else(|| errors::coded(errors::MCPC_SPAWN, &["stdin"]))?;
        let stdout = child.stdout.take().ok_or_else(|| errors::coded(errors::MCPC_SPAWN, &["stdout"]))?;
        let stderr = child.stderr.take();
        let inner = Self::start(Box::new(stdin), stdout, stderr, dirty, on_exit);
        Ok(StdioWire { inner, child: Mutex::new(Some(child)), job })
    }

    /// The protocol side, on any pipes (tests use in-memory ones).
    fn start(
        stdin: Box<dyn AsyncWrite + Send + Unpin>,
        stdout: impl AsyncRead + Send + Unpin + 'static,
        stderr: Option<impl AsyncRead + Send + Unpin + 'static>,
        dirty: Arc<AtomicBool>,
        on_exit: OnExit,
    ) -> Arc<Inner> {
        let inner = Arc::new(Inner {
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            stderr: Mutex::new(VecDeque::new()),
            dirty,
        });
        if let Some(stderr) = stderr {
            let sink = inner.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match (&mut reader).take(64 * 1024).read_until(b'\n', &mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = one_line(&String::from_utf8_lossy(&buf), 400);
                    if line.is_empty() {
                        continue;
                    }
                    let mut lines = sink.stderr.lock().unwrap();
                    lines.push_back(line);
                    while lines.len() > STDERR_LINES {
                        lines.pop_front();
                    }
                }
            });
        }
        let reader_inner = inner.clone();
        tokio::spawn(async move {
            read_loop(&reader_inner, stdout).await;
            reader_inner.close();
            if !reader_inner.stopping.load(Ordering::SeqCst) {
                // Give the stderr reader a moment to collect the last words.
                tokio::time::sleep(Duration::from_millis(100)).await;
                on_exit(reader_inner.exited_error());
            }
        });
        inner
    }

    #[cfg(test)]
    pub(crate) fn on_pipes(stdin: impl AsyncWrite + Send + Unpin + 'static, stdout: impl AsyncRead + Send + Unpin + 'static, dirty: Arc<AtomicBool>, on_exit: OnExit) -> StdioWire {
        let inner = Self::start(Box::new(stdin), stdout, None::<tokio::io::Empty>, dirty, on_exit);
        StdioWire { inner, child: Mutex::new(None), job: None }
    }

    pub fn alive(&self) -> bool {
        !self.inner.closed.load(Ordering::SeqCst)
    }

    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        if !self.alive() {
            return Err(self.inner.exited_error());
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().unwrap().insert(id, tx);
        // The spec forbids cancelling `initialize`.
        let mut guard = PendingGuard { inner: self.inner.clone(), id, cancel: method != "initialize", done: false };
        if let Err(e) = self.inner.send(&rpc::request(id, method, params)).await {
            guard.cancel = false;
            return Err(e);
        }
        let answer = tokio::time::timeout(timeout, rx).await;
        match answer {
            Err(_) => Err(errors::coded(errors::MCPC_TIMEOUT, &[])),
            Ok(Err(_)) => {
                guard.done = true;
                Err(self.inner.exited_error())
            }
            Ok(Ok(result)) => {
                guard.done = true;
                result.map_err(|e| rpc_error(&e))
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        self.inner.send(&rpc::notification(method, params)).await
    }

    pub fn stop(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.start_kill();
        }
        // The whole tree, now: a tool call still holding this wire must not
        // keep npx's node child alive after a disable.
        if let Some(job) = &self.job {
            job.terminate();
        }
    }
}

impl Drop for StdioWire {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn rpc_error(e: &RpcError) -> String {
    errors::coded(errors::MCPC_RPC, &[&e.code.to_string(), &one_line(&e.message, 200)])
}

async fn read_loop(inner: &Arc<Inner>, stdout: impl AsyncRead + Unpin) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.len() > MAX_LINE {
            log::line("mcpc: a stdio server sent a message over 4 MB; stopping it");
            return;
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        // Servers that print banners or logs on stdout: not ours to read.
        let Ok(value) = serde_json::from_str::<Value>(text) else { continue };
        for msg in rpc::messages(value) {
            match rpc::classify(&msg) {
                Incoming::Response { id, result } => {
                    if let Some(waiter) = inner.pending.lock().unwrap().remove(&id) {
                        let _ = waiter.send(result);
                    }
                }
                Incoming::Request { id, method } => {
                    let _ = inner.send(&rpc::reply_to(id, &method)).await;
                }
                Incoming::Notification { method } => {
                    if method == "notifications/tools/list_changed" {
                        inner.dirty.store(true, Ordering::SeqCst);
                    }
                }
                Incoming::Invalid => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcpc::store::tests::temp_dir;
    use tokio::io::AsyncWriteExt;

    fn touch(path: &Path) {
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn programs_are_found_on_path_with_pathext() {
        let dir = temp_dir("path");
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        touch(&a.join("npx.cmd"));
        touch(&b.join("npx.exe"));
        touch(&b.join("tool.ps1"));
        touch(&b.join("uvx.EXE"));
        let path = std::env::join_paths([a.clone(), b.clone()]).unwrap();
        let ext = ".COM;.EXE;.BAT;.CMD;.VBS;.PS1";
        // Folder order wins over extension order, as with `where`.
        assert_eq!(resolve("npx", Some(&path), ext), Some(a.join("npx.cmd")));
        let path_ba = std::env::join_paths([b.clone(), a.clone()]).unwrap();
        assert_eq!(resolve("npx", Some(&path_ba), ext), Some(b.join("npx.exe")));
        assert_eq!(resolve("npx.cmd", Some(&path), ext), Some(a.join("npx.cmd")));
        assert!(resolve("uvx", Some(&path), ext).is_some());
        assert_eq!(resolve("tool", Some(&path), ext), None, "only programs we can start");
        assert_eq!(resolve("missing", Some(&path), ext), None);
        assert_eq!(resolve(r".\npx", Some(&path), ext), None, "never relative to the current folder");
        assert_eq!(resolve("a/npx", Some(&path), ext), None);
        let abs = a.join("npx");
        assert_eq!(resolve(&abs.to_string_lossy(), None, ext), Some(a.join("npx.cmd")));
        assert_eq!(resolve("npx", None, ext), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cmd_launchers_get_a_quoted_command_line_and_refuse_metacharacters() {
        let dir = temp_dir("plan");
        touch(&dir.join("npx.cmd"));
        touch(&dir.join("node.exe"));
        let path = dir.clone().into_os_string();
        let cmd = Path::new(r"C:\Windows\System32\cmd.exe");
        let args: Vec<String> = ["-y", "@modelcontextprotocol/server-filesystem", r"C:\Users\me\My Docs\"].iter().map(|s| s.to_string()).collect();
        let launch = plan("npx", &args, Some(&path), ".EXE;.CMD", cmd).unwrap();
        assert_eq!(launch.program, cmd);
        let npx = dir.join("npx.cmd").to_string_lossy().to_string();
        assert_eq!(
            launch.raw.as_deref().unwrap(),
            format!(r#"/d /v:off /s /c ""{npx}" "-y" "@modelcontextprotocol/server-filesystem" "C:\Users\me\My Docs\\"""#)
        );
        for bad in ["a\"b", "%PATH%", "a^b", "a&calc", "a|b", "<x", ">x", "a\nb"] {
            let err = plan("npx", &[bad.to_string()], Some(&path), ".EXE;.CMD", cmd).unwrap_err();
            assert!(err.starts_with("E_MCPC_UNSAFE_ARG|"), "{bad}: {err}");
        }
        // An .exe takes them as separate arguments: Rust quotes them itself.
        let launch = plan("node", &["a&b".into()], Some(&path), ".EXE;.CMD", cmd).unwrap();
        assert_eq!((launch.program, launch.args, launch.raw), (dir.join("node.exe"), vec!["a&b".to_string()], None));
        assert_eq!(plan("nope", &[], Some(&path), ".EXE", cmd).unwrap_err(), "E_MCPC_COMMAND_NOT_FOUND|nope");
        assert_eq!(quote_for_cmd("").unwrap(), "\"\"");
        assert_eq!(quote_for_cmd(r"a\b\\").unwrap(), r#""a\b\\\\""#);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A server on in-memory pipes: framing, responses out of order, a ping from
    /// the server, notifications, junk lines, and the end of the stream.
    #[tokio::test(flavor = "current_thread")]
    async fn requests_match_responses_and_server_requests_are_answered() {
        let (client_in, mut server_in) = tokio::io::duplex(64 * 1024);
        let (mut server_out, client_out) = tokio::io::duplex(64 * 1024);
        let dirty = Arc::new(AtomicBool::new(false));
        let (exit_tx, exit_rx) = oneshot::channel::<String>();
        let wire = StdioWire::on_pipes(client_in, client_out, dirty.clone(), Box::new(move |e| {
            let _ = exit_tx.send(e);
        }));

        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(&mut server_in);
            let mut first = String::new();
            reader.read_line(&mut first).await.unwrap();
            let mut second = String::new();
            reader.read_line(&mut second).await.unwrap();
            let a: Value = serde_json::from_str(&first).unwrap();
            let b: Value = serde_json::from_str(&second).unwrap();
            assert_eq!(a["method"], "one");
            assert_eq!(b["method"], "two");
            let out = format!(
                "not json at all\n{}\n{}\n{}\n",
                json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }),
                json!({ "jsonrpc": "2.0", "id": "srv-1", "method": "ping" }),
                json!({ "jsonrpc": "2.0", "id": b["id"], "error": { "code": -32602, "message": "bad\nparams" } }),
            );
            server_out.write_all(out.as_bytes()).await.unwrap();
            let mut pong = String::new();
            reader.read_line(&mut pong).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&pong).unwrap(), json!({ "jsonrpc": "2.0", "id": "srv-1", "result": {} }));
            server_out.write_all(format!("{}\n", json!({ "jsonrpc": "2.0", "id": a["id"], "result": { "n": 1 } })).as_bytes()).await.unwrap();
            // Then the server goes away.
            drop(server_out);
        });

        let t = Duration::from_secs(5);
        let (one, two) = tokio::join!(wire.request("one", json!({}), t), wire.request("two", json!({}), t));
        assert_eq!(one.unwrap(), json!({ "n": 1 }));
        assert_eq!(two.unwrap_err(), "E_MCPC_RPC|-32602|bad params");
        assert!(dirty.load(Ordering::SeqCst));
        server.await.unwrap();
        let exit = tokio::time::timeout(t, exit_rx).await.unwrap().unwrap();
        assert!(exit.starts_with("E_MCPC_EXITED"));
        assert!(!wire.alive());
        assert!(wire.request("three", json!({}), t).await.unwrap_err().starts_with("E_MCPC_EXITED"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_silent_server_times_out_and_the_request_is_cancelled() {
        let (client_in, mut server_in) = tokio::io::duplex(64 * 1024);
        let (_server_out, client_out) = tokio::io::duplex(1024);
        let wire = StdioWire::on_pipes(client_in, client_out, Arc::new(AtomicBool::new(false)), Box::new(|_| {}));
        let err = wire.request("tools/call", json!({}), Duration::from_millis(50)).await.unwrap_err();
        assert_eq!(err, "E_MCPC_TIMEOUT");
        assert!(wire.inner.pending.lock().unwrap().is_empty());
        let mut reader = BufReader::new(&mut server_in);
        let mut call = String::new();
        reader.read_line(&mut call).await.unwrap();
        let mut cancel = String::new();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut cancel)).await.unwrap().unwrap();
        let cancel: Value = serde_json::from_str(&cancel).unwrap();
        assert_eq!(cancel["method"], "notifications/cancelled");
        assert_eq!(cancel["params"]["requestId"], 1);
    }

    /// The real thing: this test binary started as an MCP server through a .cmd
    /// launcher (cmd.exe /d /s /c with our quoting), inside a job object.
    #[tokio::test(flavor = "current_thread")]
    async fn a_real_process_through_a_cmd_launcher() {
        let dir = temp_dir("proc");
        let exe = std::env::current_exe().unwrap();
        std::fs::write(dir.join("fake-mcp.cmd"), format!("@\"{}\" %*\r\n", exe.display())).unwrap();
        let tricky = vec![
            "--exact".to_string(),
            "mcpc::stdio::tests::fake_stdio_server".to_string(),
            "--ignored".to_string(),
            "--nocapture".to_string(),
            "a b".to_string(),
            r"C:\dir with space\".to_string(),
            "x=y!".to_string(),
        ];
        let launch = plan("fake-mcp", &tricky, Some(dir.as_os_str()), ".EXE;.CMD", &cmd_exe()).unwrap();
        let env = vec![("ROADEEP_MCPC_FAKE".to_string(), "1".to_string())];
        let wire = StdioWire::spawn(&launch, &env, Arc::new(AtomicBool::new(false)), Box::new(|_| {})).unwrap();
        let t = Duration::from_secs(30);
        let init = wire.request("initialize", rpc::initialize_params(), t).await.unwrap();
        assert_eq!(init["serverInfo"]["name"], "fake");
        let args = wire.request("tools/call", json!({ "name": "argv" }), t).await.unwrap();
        let argv: Vec<String> = serde_json::from_value(args["argv"].clone()).unwrap();
        for expected in ["a b", r"C:\dir with space\", "x=y!"] {
            assert!(argv.iter().any(|a| a == expected), "{expected:?} not in {argv:?}");
        }
        let err = wire.request("tools/call", json!({ "name": "exit" }), t).await.unwrap_err();
        assert!(err.starts_with("E_MCPC_EXITED"), "{err}");
        assert!(err.contains("fake server says hello"), "stderr tail: {err}");
        drop(wire);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Not a test: the fake server `a_real_process_through_a_cmd_launcher` starts.
    #[test]
    #[ignore]
    fn fake_stdio_server() {
        if std::env::var("ROADEEP_MCPC_FAKE").is_err() {
            return;
        }
        use std::io::{BufRead, Write};
        eprintln!("fake server says hello");
        let stdin = std::io::stdin();
        let mut out = std::io::stdout();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            let id = msg["id"].clone();
            let reply = match msg["method"].as_str() {
                Some("initialize") => json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} }, "serverInfo": { "name": "fake", "version": "1" } }),
                Some("tools/call") if msg["params"]["name"] == "argv" => json!({ "argv": std::env::args().collect::<Vec<_>>() }),
                Some("tools/call") if msg["params"]["name"] == "exit" => std::process::exit(3),
                _ => continue,
            };
            writeln!(out, "{}", json!({ "jsonrpc": "2.0", "id": id, "result": reply })).unwrap();
            out.flush().unwrap();
        }
    }
}
