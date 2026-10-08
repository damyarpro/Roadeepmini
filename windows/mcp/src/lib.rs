//! roadeep-mcp — a local MCP server for Roadeep, backed by the Roadeep app.
//!
//! The executable holds no credentials at all. It speaks MCP (JSON-RPC 2.0,
//! newline-delimited) on stdio and forwards every `tools/call` over the per-user
//! named pipe `\\.\pipe\roadeep-mcp-<sid>` to the running Roadeep app, which makes
//! the Roadeep call with its signed-in session and answers on the same pipe.
//!
//! The library half is the contract shared with the app, so it lives in exactly
//! one place:
//!   wire     — pipe name and the one-line request/response format
//!   tools    — the tool list (names, descriptions, JSON Schemas)
//!   protocol — JSON-RPC framing and dispatch (pure, tested)
//!   relay    — the client end of the pipe (exe only)

pub mod protocol;
pub mod tools;
pub mod wire;

#[cfg(windows)]
pub mod relay;
#[cfg(windows)]
pub mod win;
