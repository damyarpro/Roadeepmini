//! The client end of `\\.\pipe\roadeep-mcp-<sid>`: connect, send one request,
//! read one answer. Every failure becomes an error `Outcome` — the MCP client
//! always gets a tool result, never a hang or a crash.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::win;
use crate::wire::{self, codes, Outcome, WireError};

/// Getting a connection. The app answers instantly when it is up; a closed
/// app is a missing pipe, which we see at once.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// A chat reply or a generation submit can take minutes. The app gives up at
/// 230 s and always answers; this is the backstop if it does not.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(240);

const ERROR_FILE_NOT_FOUND: i32 = 2;
/// Every instance is serving another call right now; one will free up.
const ERROR_PIPE_BUSY: i32 = 231;

enum ConnectError {
    NotRunning,
    Untrusted,
    Other(String),
}

fn not_running() -> Outcome {
    Outcome::Err(WireError::new(
        codes::ROADEEP_NOT_RUNNING,
        format!("The Roadeep app is not running. {}", wire::SIGN_IN_HINT),
    ))
}

fn connect() -> Result<std::fs::File, ConnectError> {
    use std::os::windows::io::AsRawHandle;
    let path = wire::pipe_name(&win::user_key());
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => {
                let handle = windows::Win32::Foundation::HANDLE(file.as_raw_handle());
                return if win::pipe_server_is_same_user(handle) { Ok(file) } else { Err(ConnectError::Untrusted) };
            }
            Err(err) if err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => return Err(ConnectError::NotRunning),
            Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => return Err(ConnectError::Other(err.to_string())),
        }
    }
}

fn exchange(request: &str) -> Outcome {
    let mut pipe = match connect() {
        Ok(p) => p,
        Err(ConnectError::NotRunning) => return not_running(),
        Err(ConnectError::Untrusted) => {
            return Outcome::Err(WireError::new(
                codes::UNTRUSTED_PIPE,
                "The Roadeep app's pipe belongs to another account; refusing to use it.",
            ))
        }
        Err(ConnectError::Other(e)) => {
            return Outcome::Err(WireError::new(codes::PIPE_ERROR, format!("Could not reach the Roadeep app: {e}")))
        }
    };
    if let Err(e) = pipe.write_all(request.as_bytes()).and_then(|_| pipe.flush()) {
        return Outcome::Err(WireError::new(codes::PIPE_ERROR, format!("Could not send to the Roadeep app: {e}")));
    }
    // Dropping `pipe` on return is our hang-up, which the app waits for before
    // closing its end — so it never discards an answer we have not read.
    read_answer(&mut pipe)
}

/// Reads one answer line off a connected pipe. Public so the app's tests can
/// drive this exact code against its own server end.
pub fn read_answer(pipe: &mut impl Read) -> Outcome {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&b'\n') {
                    break;
                }
                if buf.len() > wire::MAX_RESPONSE {
                    return Outcome::Err(WireError::new(codes::INVALID_RESPONSE, "The Roadeep app's answer was too large."));
                }
            }
            // The app closes the pipe once it has answered; a broken pipe with
            // no data means it went away mid-call.
            Err(_) => break,
        }
    }
    if buf.is_empty() {
        return Outcome::Err(WireError::new(codes::PIPE_ERROR, "The Roadeep app closed the connection without answering."));
    }
    wire::decode_response(&buf)
}

/// Relays one tool call to the app. Runs the blocking I/O on a worker so the
/// deadline holds even against a pipe that accepts and then never answers.
pub fn call(tool: &str, arguments: &Value) -> Outcome {
    let request = wire::encode_request(tool, arguments);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(exchange(&request));
    });
    rx.recv_timeout(CALL_TIMEOUT).unwrap_or_else(|_| {
        Outcome::Err(WireError::new(
            codes::TIMEOUT,
            format!("The Roadeep app did not answer within {} s.", CALL_TIMEOUT.as_secs()),
        ))
    })
}
