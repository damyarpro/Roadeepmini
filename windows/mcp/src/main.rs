//! roadeep-mcp — the process an MCP client (Claude Code, …) spawns over stdio.
//!
//! Reads JSON-RPC lines on stdin, answers on stdout. `tools/call` is relayed to
//! the running Roadeep app on a worker thread, so pings and other calls keep
//! flowing while a chat reply is being written. stdout carries protocol
//! messages only; the short diagnostic lines go to stderr, which MCP clients
//! keep in their logs. Neither ever contains arguments or replies.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use roadeep_mcp::protocol::{self, Incoming};
use roadeep_mcp::wire::{codes, Outcome, WireError};

/// Calls relayed at once. The app enforces its own limit too; this one keeps a
/// runaway client from piling up threads here.
const MAX_IN_FLIGHT: usize = 8;

fn write_message(out: &Mutex<std::io::Stdout>, message: &serde_json::Value) {
    let mut line = message.to_string();
    line.push('\n');
    // One lock per message: concurrent answers never interleave mid-line.
    let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
    if out.write_all(line.as_bytes()).and_then(|_| out.flush()).is_err() {
        // The client is gone; nothing left to talk to.
        std::process::exit(0);
    }
}

fn main() {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut line = Vec::new();
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();

    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break, // stdin closed: the client is shutting us down
            Ok(_) => {}
            Err(err) => {
                eprintln!("roadeep-mcp: stdin read failed: {err}");
                break;
            }
        }

        match protocol::handle_line(&line) {
            Incoming::Ignore => {}
            Incoming::Reply(message) => write_message(&out, &message),
            Incoming::ToolCall { id, name, arguments } => {
                if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    let busy = Outcome::Err(WireError::new(codes::BUSY, "Too many Roadeep calls at once. Try again shortly."));
                    write_message(&out, &protocol::tool_result(id, &busy));
                    continue;
                }
                let out = Arc::clone(&out);
                let in_flight = Arc::clone(&in_flight);
                workers.retain(|w| !w.is_finished());
                workers.push(std::thread::spawn(move || {
                    let started = Instant::now();
                    let outcome = roadeep_mcp::relay::call(&name, &arguments);
                    let status = match &outcome {
                        Outcome::Ok(_) => "ok".to_string(),
                        Outcome::Err(e) => e.code.clone(),
                    };
                    eprintln!("roadeep-mcp: {name} {status} {}ms", started.elapsed().as_millis());
                    write_message(&out, &protocol::tool_result(id, &outcome));
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                }));
            }
        }
    }
    // A client that writes its requests and then closes stdin still expects the
    // answers; each call is bounded by the relay's own timeout.
    for worker in workers {
        let _ = worker.join();
    }
    std::process::exit(0);
}
