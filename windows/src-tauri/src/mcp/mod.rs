// Local Roadeep MCP server, app side.
//
// roadeep-mcp.exe (the process Claude Code and other MCP clients spawn) holds no
// credentials. It relays each tools/call over `\\.\pipe\roadeep-mcp-<sid>` to
// this server, which runs it with the signed-in Roadeep session and writes the
// answer back on the same connection. One request per connection, like the
// hook relay (pipe.rs).
//
// Who may talk to it:
//   * the pipe's DACL grants access to the current user's SID only, and remote
//     clients are rejected — nobody else can even open it;
//   * the client process must run as the same user, checked per connection;
//   * the relay checks the reverse (that the server runs as the user).
//
// Every call is logged as name, outcome and duration. Arguments, prompts and
// replies never are, and tokens never leave the roadeep module.
//
//   tools.rs   — tool dispatch and argument validation
//   quotes.rs  — the cost quotes this app has handed out, for the paid step
//   confirm.rs — the native "spend credits?" dialog a human must click
//   install.rs — the `roadeep` entry in each coding app's config, staging the exe
//   clients.rs — the apps it can be registered with, and where their configs live
//   cfg_json.rs, cfg_toml.rs, cfg_yaml.rs — editing each config format

mod cfg_json;
mod cfg_toml;
mod cfg_yaml;
pub mod clients;
pub mod confirm;
pub mod install;
pub mod quotes;
pub mod tools;

use std::sync::Arc;
use std::time::{Duration, Instant};

use roadeep_mcp::wire::{self, codes, Outcome, WireError};
use tauri::{AppHandle, Manager};
use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::Semaphore;

use crate::log;
use crate::roadeep::Roadeep;

/// Calls executed at once; more get an immediate BUSY rather than a queue.
const MAX_CONCURRENT: usize = 4;
/// A request line arrives right after connecting; a client that dawdles is dropped.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Under the relay's 240 s, so the client always gets our answer, not its own timeout.
const CALL_DEADLINE: Duration = Duration::from_secs(230);

/// `\\.\pipe\roadeep-mcp-<sid>` — must match roadeep-mcp's `win::user_key()`.
pub fn pipe_name() -> String {
    wire::pipe_name(&crate::pipe::user_key())
}

/// The current pipe, plus (for the transition) the one the relay staged by
/// builds under the former name connects to: same wire format, same answers.
/// Both share one concurrency limit.
pub fn start(app: AppHandle) {
    let limiter = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let legacy = crate::migrate::legacy_mcp_pipe(&crate::pipe::user_key());
    for (label, name) in [("mcp", pipe_name()), ("mcp-legacy", legacy)] {
        let app = app.clone();
        let limiter = limiter.clone();
        tauri::async_runtime::spawn(async move {
            let Some(security) = secure::PipeSecurity::current_user_only() else {
                log::line(format!("{label}: cannot build the pipe security descriptor; not started"));
                return;
            };
            // first_pipe_instance: never serve on top of a pipe somebody else owns.
            let server = match security.create(&name, true) {
                Ok(s) => s,
                Err(err) => {
                    log::line(format!("{label}: cannot open the pipe: {err}"));
                    return;
                }
            };
            log::line(format!("{label}: server listening"));
            crate::pipe::serve(label, server, || security.create(&name, false), |connected| {
                let app = app.clone();
                let limiter = limiter.clone();
                tauri::async_runtime::spawn(async move { handle(app, connected, limiter).await });
            })
            .await;
        });
    }
}

async fn read_request(pipe: &mut NamedPipeServer) -> Result<Vec<u8>, WireError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let read = async {
        loop {
            match pipe.read(&mut chunk).await {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.contains(&b'\n') {
                        return Ok(());
                    }
                    if buf.len() > wire::MAX_REQUEST {
                        return Err(WireError::new(codes::BAD_REQUEST, "The request is too large."));
                    }
                }
                Err(err) => return Err(WireError::new(codes::PIPE_ERROR, format!("read failed: {err}"))),
            }
        }
    };
    match tokio::time::timeout(READ_TIMEOUT, read).await {
        Ok(Ok(())) => Ok(buf),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(WireError::new(codes::TIMEOUT, "No request arrived.")),
    }
}

async fn handle(app: AppHandle, mut pipe: NamedPipeServer, limiter: Arc<Semaphore>) {
    if !secure::client_is_same_user(&pipe) {
        log::line("mcp: refused a connection from another account");
        let _ = pipe.disconnect();
        return;
    }

    let outcome = match read_request(&mut pipe).await {
        Err(err) => {
            log::line(format!("mcp: bad request ({})", err.code));
            Outcome::Err(err)
        }
        Ok(line) => match wire::decode_request(&line) {
            Err(err) => {
                log::line(format!("mcp: bad request ({})", err.code));
                Outcome::Err(err)
            }
            Ok((tool, args)) => run(&app, &tool, &args, &limiter).await,
        },
    };

    respond(pipe, &outcome).await;
}

/// The answer, delivered in full however large: see `pipe::answer_and_close`.
async fn respond(pipe: NamedPipeServer, outcome: &Outcome) {
    if let Err(err) = crate::pipe::answer_and_close(pipe, wire::encode_response(outcome).as_bytes()).await {
        log::line(format!("mcp: could not answer the relay: {err}"));
    }
}

async fn run(app: &AppHandle, tool: &str, args: &serde_json::Value, limiter: &Semaphore) -> Outcome {
    // Tool names come from the client; only known ones reach the log verbatim.
    let label = if roadeep_mcp::tools::is_known(tool) { tool } else { "unknown-tool" };
    let Ok(_permit) = limiter.try_acquire() else {
        log::line(format!("mcp {label} {} 0ms", codes::BUSY));
        return Outcome::Err(WireError::new(codes::BUSY, "The Roadeep app is busy with other Roadeep calls. Try again shortly."));
    };
    let started = Instant::now();
    let roadeep = app.state::<Roadeep>();
    let outcome = match tokio::time::timeout(CALL_DEADLINE, tools::dispatch(&roadeep, tool, args)).await {
        Ok(Ok(value)) => Outcome::Ok(value),
        Ok(Err(err)) => Outcome::Err(tools::to_wire(err)),
        Err(_) => Outcome::Err(WireError::new(
            codes::TIMEOUT,
            format!("Roadeep did not finish within {} s.", CALL_DEADLINE.as_secs()),
        )),
    };
    let status = match &outcome {
        Outcome::Ok(_) => "ok".to_string(),
        Outcome::Err(e) => e.code.clone(),
    };
    log::line(format!("mcp {label} {status} {}ms", started.elapsed().as_millis()));
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The relay's side of one call: open, send, then roadeep-mcp's own reader.
    /// `pause` stands in for a relay that is slow to start reading.
    fn relay_client(name: String, pause: Duration) -> std::thread::JoinHandle<Outcome> {
        std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(&name).unwrap();
            pipe.write_all(wire::encode_request("roadeep_whoami", &serde_json::json!({})).as_bytes()).unwrap();
            std::thread::sleep(pause);
            roadeep_mcp::relay::read_answer(&mut pipe)
        })
    }

    /// The real pipe path, minus Roadeep: a protected-DACL pipe can be created,
    /// re-created for the next client, opened by us, and the peer check passes.
    #[test]
    fn a_secured_pipe_round_trips_a_request() {
        let name = format!(r"\\.\pipe\roadeep-mcp-test-{}", std::process::id());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let security = secure::PipeSecurity::current_user_only().expect("descriptor");
            let mut first = security.create(&name, true).expect("first instance");
            let client = relay_client(name.clone(), Duration::ZERO);

            first.connect().await.unwrap();
            // The protected DACL must still let us open the next instance.
            let _next = security.create(&name, false).expect("second instance");
            assert!(secure::client_is_same_user(&first));
            let line = read_request(&mut first).await.unwrap();
            let (tool, _) = wire::decode_request(&line).unwrap();
            assert_eq!(tool, "roadeep_whoami");
            let answer = Outcome::Ok(serde_json::json!({ "pong": true }));
            respond(first, &answer).await;
            assert_eq!(client.join().unwrap(), answer);
        });
    }

    /// Regression: the answer used to be followed by DisconnectNamedPipe, which
    /// throws away whatever the relay has not read yet — anything past the pipe
    /// buffer came out truncated, as INVALID_RESPONSE. A multi-megabyte answer
    /// to a relay that starts reading late must arrive whole, and the server
    /// side must finish promptly once the relay hangs up.
    #[test]
    fn a_large_answer_reaches_a_slow_relay_whole() {
        let name = format!(r"\\.\pipe\roadeep-mcp-test-big-{}", std::process::id());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let security = secure::PipeSecurity::current_user_only().expect("descriptor");
            let mut server = security.create(&name, true).expect("instance");
            let client = relay_client(name.clone(), Duration::from_millis(300));

            server.connect().await.unwrap();
            read_request(&mut server).await.unwrap();
            // ~3 MB of JSON, well past any pipe buffer, under wire::MAX_RESPONSE.
            let blob: Vec<String> = (0..60_000).map(|i| format!("member-{i:06}-{}", "x".repeat(32))).collect();
            let answer = Outcome::Ok(serde_json::json!({ "members": blob }));
            assert!(wire::encode_response(&answer).len() > 1 << 20);

            let started = std::time::Instant::now();
            respond(server, &answer).await;
            assert!(
                started.elapsed() < crate::pipe::CLIENT_CLOSE_WAIT,
                "the server must notice the relay's hang-up, not sit out the full wait"
            );
            match client.join().unwrap() {
                got @ Outcome::Ok(_) => assert!(got == answer, "the answer arrived altered"),
                Outcome::Err(err) => panic!("the relay did not get the whole answer: {} {}", err.code, err.message),
            }
        });
    }

    /// A client that came and went before we accepted (ERROR_NO_DATA) must not
    /// wedge the listener: the next client is still served. mio reports that
    /// case as a connection, which then reads nothing; any real `connect` error
    /// makes the loop replace the instance instead of retrying it forever.
    #[test]
    fn the_accept_loop_recovers_from_a_client_that_left_early() {
        let name = format!(r"\\.\pipe\roadeep-mcp-test-loop-{}", std::process::id());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let security = secure::PipeSecurity::current_user_only().expect("descriptor");
            let server = security.create(&name, true).expect("instance");
            // Connect and hang up before the server ever calls ConnectNamedPipe.
            drop(std::fs::OpenOptions::new().read(true).write(true).open(&name).unwrap());

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let loop_name = name.clone();
            let serving = tokio::spawn(async move {
                let security = secure::PipeSecurity::current_user_only().expect("descriptor");
                crate::pipe::serve("test", server, || security.create(&loop_name, false), |connected| {
                    let _ = tx.send(connected);
                })
                .await;
            });

            // The second client must get through. Its first open may race the
            // instance swap (ERROR_PIPE_BUSY / not found), so retry briefly.
            let client_name = name.clone();
            let client = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                loop {
                    match std::fs::OpenOptions::new().read(true).write(true).open(&client_name) {
                        Ok(mut pipe) => {
                            pipe.write_all(wire::encode_request("roadeep_whoami", &serde_json::json!({})).as_bytes()).unwrap();
                            return roadeep_mcp::relay::read_answer(&mut pipe);
                        }
                        Err(_) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                        Err(err) => panic!("never reached the pipe again: {err}"),
                    }
                }
            });

            let (pipe, line) = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let mut pipe = rx.recv().await.expect("the accept loop stopped");
                    // The early leaver, if handed out, reads as an empty request.
                    match read_request(&mut pipe).await {
                        Ok(line) if !line.is_empty() => return (pipe, line),
                        _ => continue,
                    }
                }
            })
            .await
            .expect("the accept loop never served the next client");
            let (tool, _) = wire::decode_request(&line).unwrap();
            assert_eq!(tool, "roadeep_whoami");
            let answer = Outcome::Ok(serde_json::json!({ "back": true }));
            respond(pipe, &answer).await;
            assert_eq!(client.join().unwrap(), answer);
            serving.abort();
        });
    }

    #[test]
    fn a_second_server_on_the_same_name_is_refused() {
        let name = format!(r"\\.\pipe\roadeep-mcp-test2-{}", std::process::id());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let security = secure::PipeSecurity::current_user_only().unwrap();
            let _ours = security.create(&name, true).unwrap();
            assert!(security.create(&name, true).is_err(), "first_pipe_instance must refuse a squatter");
        });
    }
}

mod secure {
    //! The pipe's DACL and the per-connection peer check.

    use std::os::windows::io::AsRawHandle;

    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

    /// A self-relative security descriptor: protected DACL, one ACE, full
    /// access for the current user's SID. Nobody else — not even another admin
    /// session — can open the pipe.
    pub struct PipeSecurity(PSECURITY_DESCRIPTOR);

    // The descriptor is immutable after creation and only ever read by
    // CreateNamedPipe, so sharing the pointer across tasks is sound.
    unsafe impl Send for PipeSecurity {}
    unsafe impl Sync for PipeSecurity {}

    impl PipeSecurity {
        pub fn current_user_only() -> Option<Self> {
            let sid = crate::win_user::current_user_sid()?;
            let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut descriptor, None).ok()?;
            }
            Some(Self(descriptor))
        }

        pub fn create(&self, name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
            let mut attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0 .0,
                bInheritHandle: false.into(),
            };
            let mut options = ServerOptions::new();
            options.first_pipe_instance(first).reject_remote_clients(true);
            // SAFETY: `attributes` and the descriptor it points to outlive the call.
            unsafe {
                options.create_with_security_attributes_raw(name, &mut attributes as *mut _ as *mut std::ffi::c_void)
            }
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }

    /// True when the connected client runs as the same user we do. Anything we
    /// cannot vouch for is refused.
    pub fn client_is_same_user(pipe: &NamedPipeServer) -> bool {
        let Some(mine) = crate::win_user::current_user_sid() else { return false };
        unsafe {
            let mut pid = 0u32;
            if GetNamedPipeClientProcessId(HANDLE(pipe.as_raw_handle()), &mut pid).is_err() || pid == 0 {
                return false;
            }
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let theirs = token_sid(process);
            let _ = CloseHandle(process);
            theirs.as_deref() == Some(mine.as_str())
        }
    }

    /// The user SID behind a process handle. `process` is borrowed, never closed.
    unsafe fn token_sid(process: HANDLE) -> Option<String> {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), needed, &mut needed).is_ok();
        let _ = CloseHandle(token);
        if !ok {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(text.0 as *mut _)));
        sid
    }
}
