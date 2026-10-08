// The user's MCP servers, in %APPDATA%\Roadeep\mcp-servers.json. No secrets in
// it: tokens, environment values and OAuth state live in the Credential
// Manager (secrets::mcpc_*), the file only names them.
//
// Same discipline as agents.json: every call reads the file fresh, every
// change rewrites it atomically (temp file + rename), and a file that does not
// parse, or holds entries that fail today's validation, is moved aside as
// mcp-servers.json.bad-<time> before anything is written over it. A file that
// exists but can't be read right now is an error, never "no servers".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::ToolMode;
use crate::errors;
use crate::log;

pub const FILE_NAME: &str = "mcp-servers.json";
const FILE_VERSION: u32 = 1;
pub const MAX_SERVERS: usize = 50;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_ARGS: usize = 40;
const MAX_ARG_CHARS: usize = 2000;
const MAX_COMMAND_CHARS: usize = 400;
pub const MAX_ENV: usize = 20;
const MAX_URL_CHARS: usize = 2000;
/// Per-tool overrides kept per server.
const MAX_TOOL_OVERRIDES: usize = 500;
pub const MAX_TOOL_NAME_CHARS: usize = 128;

/// Serializes read-modify-write cycles; the file is the only state.
static FILE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Transport {
    Http {
        url: String,
    },
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Variable names only; the values are keyring slots `env:<NAME>`.
        #[serde(default)]
        env: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Auth {
    None,
    /// `Authorization: Bearer <token slot>`.
    Bearer,
    /// `<name>: <token slot>`.
    Header { name: String },
    /// mcpc/oauth.rs signs in and keeps the tokens in the `oauth` slot.
    Oauth,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    /// "custom" or "directory:<entry id>".
    pub source: String,
    pub transport: Transport,
    pub auth: Auth,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// stdio only: `command_hash` of the command the user confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_command: Option<String>,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolMode>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

fn yes() -> bool {
    true
}

/// What `mcpc_add` receives.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSpec {
    pub name: String,
    pub source: String,
    pub transport: Transport,
    pub auth: Auth,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// What `mcpc_update` receives: only the fields that change.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub transport: Option<Transport>,
    #[serde(default)]
    pub auth: Option<Auth>,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    servers: Vec<Value>,
}

// ── Validation ────────────────────────────────────────────────────────────────

fn invalid(field: &str) -> String {
    errors::coded(errors::MCPC_INVALID, &[field])
}

/// `^[a-z0-9][a-z0-9-]{1,40}$`.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && (2..=41).contains(&id.len())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_CHARS || !name.chars().all(crate::roadeep::chat::shown_char) {
        return Err(invalid("name"));
    }
    Ok(name.to_string())
}

fn valid_source(source: &str) -> bool {
    source == "custom"
        || source.strip_prefix("directory:").is_some_and(|id| {
            !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// http only to this machine; https anywhere. No user:password@ (that would be
/// a secret in the config file) and no fragment.
pub fn check_url(raw: &str) -> Result<reqwest::Url, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_URL_CHARS || raw.chars().any(char::is_control) {
        return Err(invalid("url"));
    }
    let url = reqwest::Url::parse(raw).map_err(|_| invalid("url"))?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    if !scheme_ok || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(invalid("url"));
    }
    Ok(url)
}

/// Text that goes on a command line: no control characters at all (newline,
/// NUL…). Shell metacharacters are checked where a shell is involved (stdio.rs).
fn command_text_ok(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

/// An HTTP header name we let the user set: a token, and none of the headers
/// the transport itself owns.
fn valid_header_name(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "host", "content-length", "content-type", "accept", "connection", "transfer-encoding", "mcp-session-id",
        "mcp-protocol-version", "user-agent", "cookie", "te", "upgrade", "last-event-id",
    ];
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
        && !RESERVED.contains(&name.to_ascii_lowercase().as_str())
}

pub fn clean_transport(transport: &Transport) -> Result<Transport, String> {
    match transport {
        Transport::Http { url } => Ok(Transport::Http { url: check_url(url)?.to_string() }),
        Transport::Stdio { command, args, env } => {
            let command = command.trim().to_string();
            if command.is_empty() || !command_text_ok(&command, MAX_COMMAND_CHARS) {
                return Err(invalid("command"));
            }
            if args.len() > MAX_ARGS || !args.iter().all(|a| command_text_ok(a, MAX_ARG_CHARS)) {
                return Err(invalid("args"));
            }
            let mut names: Vec<String> = Vec::new();
            for name in env {
                let name = name.trim();
                if !crate::secrets::mcpc_env_name_valid(name) || names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                    return Err(invalid("env"));
                }
                names.push(name.to_string());
            }
            if names.len() > MAX_ENV {
                return Err(invalid("env"));
            }
            Ok(Transport::Stdio { command, args: args.clone(), env: names })
        }
    }
}

/// Auth that fits the transport: a local process gets its secrets through
/// environment variables, so anything but `none` is HTTP-only.
pub fn clean_auth(auth: &Auth, transport: &Transport) -> Result<Auth, String> {
    let auth = match auth {
        Auth::Header { name } => {
            let name = name.trim();
            if !valid_header_name(name) {
                return Err(invalid("header"));
            }
            Auth::Header { name: name.to_string() }
        }
        other => other.clone(),
    };
    if matches!(transport, Transport::Stdio { .. }) && auth != Auth::None {
        return Err(invalid("auth"));
    }
    Ok(auth)
}

pub fn valid_tool_name(tool: &str) -> bool {
    !tool.is_empty() && tool.chars().count() <= MAX_TOOL_NAME_CHARS && !tool.chars().any(char::is_control)
}

/// A stored server must still pass today's rules, or the file is suspect.
fn check_stored(value: &Value) -> Option<ServerConfig> {
    let server: ServerConfig = serde_json::from_value(value.clone()).ok()?;
    if !valid_id(&server.id) || !valid_source(&server.source) {
        return None;
    }
    let name = clean_name(&server.name).ok()?;
    let transport = clean_transport(&server.transport).ok()?;
    let auth = clean_auth(&server.auth, &transport).ok()?;
    let tools_ok = server.tools.len() <= MAX_TOOL_OVERRIDES && server.tools.keys().all(|t| valid_tool_name(t));
    let approval_ok = server.approved_command.as_deref().is_none_or(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()));
    (tools_ok && approval_ok).then_some(ServerConfig { name, transport, auth, ..server })
}

// ── Command approval ──────────────────────────────────────────────────────────

/// SHA-256 (hex) of exactly what would run: the command, its arguments and
/// the NAMES of the variables it gets (a new NODE_OPTIONS or PATH changes what
/// runs as surely as a new argument; the values live in the keyring).
/// JSON-encoded so `["a b"]` and `["a", "b"]` can't collide.
pub fn command_hash(command: &str, args: &[String], env: &[String]) -> String {
    let encoded = serde_json::to_vec(&(command, args, env)).unwrap_or_default();
    Sha256::digest(&encoded).iter().map(|b| format!("{b:02x}")).collect()
}

/// stdio: the hash of the server's command as it is now; None for HTTP.
pub fn current_command_hash(server: &ServerConfig) -> Option<String> {
    match &server.transport {
        Transport::Stdio { command, args, env } => Some(command_hash(command, args, env)),
        Transport::Http { .. } => None,
    }
}

/// stdio: the command the user confirmed is the one in the file now.
pub fn command_approved(server: &ServerConfig) -> bool {
    match current_command_hash(server) {
        Some(hash) => server.approved_command.as_deref() == Some(hash.as_str()),
        None => true,
    }
}

// ── Ids and time ──────────────────────────────────────────────────────────────

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `<slug of the name>-<4 hex>`; `mcp-<4 hex>` when the name has no ASCII letters
/// (a Persian name). Unique among `taken`.
pub fn new_id(name: &str, taken: &[&str]) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 24 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "mcp" } else { slug };
    loop {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let id = format!("{slug}-{}", &suffix[..4]);
        if !taken.contains(&id.as_str()) {
            return id;
        }
    }
}

// ── File ──────────────────────────────────────────────────────────────────────

pub fn store_path() -> PathBuf {
    crate::settings::config_dir().join(FILE_NAME)
}

fn store_error(context: &str, err: impl std::fmt::Display) -> String {
    log::line(format!("mcpc: {context}: {err}"));
    errors::coded(errors::MCPC_STORE, &[&err.to_string()])
}

/// Moves a suspect file aside so the next write cannot destroy it.
fn quarantine(path: &Path, why: &str) {
    let backup = path.with_file_name(format!("{FILE_NAME}.bad-{}", now_ms()));
    match std::fs::rename(path, &backup) {
        Ok(()) => log::line(format!("mcpc: {why}; kept the old file as {}", backup.display())),
        Err(err) => log::line(format!("mcpc: {why}; could not move it aside: {err}")),
    }
}

/// Missing file = no servers. A damaged file is set aside (and logged) and
/// whatever was valid in it is kept.
pub fn load_from(path: &Path) -> std::io::Result<Vec<ServerConfig>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            log::line(format!("mcpc: could not read {}: {err}", path.display()));
            return Err(err);
        }
    };
    let file: StoreFile = match serde_json::from_slice(&bytes) {
        Ok(file) => file,
        Err(err) => {
            quarantine(path, &format!("{FILE_NAME} is not valid ({err})"));
            return Ok(Vec::new());
        }
    };
    let total = file.servers.len();
    let mut servers: Vec<ServerConfig> = Vec::new();
    for value in &file.servers {
        if let Some(server) = check_stored(value) {
            if servers.len() < MAX_SERVERS && !servers.iter().any(|s| s.id == server.id) {
                servers.push(server);
            }
        }
    }
    if servers.len() != total || file.version != FILE_VERSION {
        quarantine(path, &format!("{FILE_NAME} had {} unusable entries (format v{})", total - servers.len(), file.version));
        if let Err(err) = save_to(path, &servers) {
            log::line(format!("mcpc: could not rewrite the cleaned file: {err}"));
        }
    }
    Ok(servers)
}

/// Atomic: the old file stays intact until the new one is completely on disk.
pub fn save_to(path: &Path, servers: &[ServerConfig]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let values = servers.iter().map(serde_json::to_value).collect::<Result<Vec<_>, _>>()?;
    let json = serde_json::to_vec_pretty(&StoreFile { version: FILE_VERSION, servers: values })?;
    let tmp = path.with_file_name(format!("{FILE_NAME}.tmp"));
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Read-only view (no lock needed: writes are atomic renames).
pub fn load() -> Vec<ServerConfig> {
    load_from(&store_path()).unwrap_or_default()
}

pub fn load_strict() -> Result<Vec<ServerConfig>, String> {
    load_from(&store_path()).map_err(|e| store_error("could not read the server list", e))
}

pub fn get(id: &str) -> Result<ServerConfig, String> {
    load_strict()?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| errors::coded(errors::MCPC_UNKNOWN_SERVER, &[id]))
}

// ── Operations (path-explicit so tests never touch %APPDATA%) ─────────────────

/// Runs `change` on the stored list under the file lock and writes the result.
fn modify_in<T>(path: &Path, change: impl FnOnce(&mut Vec<ServerConfig>) -> Result<T, String>) -> Result<T, String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut servers = load_from(path).map_err(|e| store_error("refusing to write over a file that could not be read", e))?;
    let out = change(&mut servers)?;
    save_to(path, &servers).map_err(|e| store_error("could not save the server list", e))?;
    Ok(out)
}

fn find<'a>(servers: &'a mut [ServerConfig], id: &str) -> Result<&'a mut ServerConfig, String> {
    servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| errors::coded(errors::MCPC_UNKNOWN_SERVER, &[id]))
}

pub fn add_in(path: &Path, spec: &ServerSpec) -> Result<ServerConfig, String> {
    let name = clean_name(&spec.name)?;
    if !valid_source(&spec.source) {
        return Err(invalid("source"));
    }
    let transport = clean_transport(&spec.transport)?;
    let auth = clean_auth(&spec.auth, &transport)?;
    modify_in(path, |servers| {
        if servers.len() >= MAX_SERVERS {
            return Err(errors::coded(errors::MCPC_LIMIT, &[&MAX_SERVERS.to_string()]));
        }
        let taken: Vec<&str> = servers.iter().map(|s| s.id.as_str()).collect();
        let now = now_ms();
        let server = ServerConfig {
            id: new_id(&name, &taken),
            name,
            source: spec.source.clone(),
            transport,
            auth,
            enabled: spec.enabled.unwrap_or(true),
            approved_command: None,
            tools: BTreeMap::new(),
            created_at: now,
            updated_at: now,
        };
        servers.push(server.clone());
        Ok(server)
    })
}

/// Returns (before, after) so the caller can tidy secrets and connections.
pub fn update_in(path: &Path, id: &str, patch: &ServerPatch) -> Result<(ServerConfig, ServerConfig), String> {
    let name = patch.name.as_deref().map(clean_name).transpose()?;
    let new_transport = patch.transport.as_ref().map(clean_transport).transpose()?;
    modify_in(path, |servers| {
        let server = find(servers, id)?;
        let before = server.clone();
        let transport = new_transport.unwrap_or_else(|| before.transport.clone());
        let auth = clean_auth(patch.auth.as_ref().unwrap_or(&before.auth), &transport)?;
        if let Some(name) = name {
            server.name = name;
        }
        if let Some(enabled) = patch.enabled {
            server.enabled = enabled;
        }
        // A different command, args or variable names must be confirmed again.
        let same_command = match (&before.transport, &transport) {
            (Transport::Stdio { command: a, args: x, env: m }, Transport::Stdio { command: b, args: y, env: n }) => a == b && x == y && m == n,
            _ => false,
        };
        if !same_command {
            server.approved_command = None;
        }
        server.transport = transport;
        server.auth = auth;
        server.updated_at = now_ms();
        Ok((before, server.clone()))
    })
}

pub fn remove_in(path: &Path, id: &str) -> Result<ServerConfig, String> {
    modify_in(path, |servers| {
        let index = servers.iter().position(|s| s.id == id).ok_or_else(|| errors::coded(errors::MCPC_UNKNOWN_SERVER, &[id]))?;
        Ok(servers.remove(index))
    })
}

/// `shown`: the hash of the command the settings window displayed. Approved
/// only when it is still the command in the file, so an edit that lands
/// between showing and clicking is never approved unseen.
pub fn approve_in(path: &Path, id: &str, shown: &str) -> Result<ServerConfig, String> {
    modify_in(path, |servers| {
        let server = find(servers, id)?;
        let Some(current) = current_command_hash(server) else {
            return Err(invalid("transport"));
        };
        if current != shown {
            return Err(errors::coded(errors::MCPC_COMMAND_CHANGED, &[]));
        }
        server.approved_command = Some(current);
        server.updated_at = now_ms();
        Ok(server.clone())
    })
}

pub fn set_tool_mode_in(path: &Path, id: &str, tool: &str, mode: ToolMode) -> Result<(), String> {
    if !valid_tool_name(tool) {
        return Err(invalid("tool"));
    }
    modify_in(path, |servers| {
        let server = find(servers, id)?;
        if !server.tools.contains_key(tool) && server.tools.len() >= MAX_TOOL_OVERRIDES {
            return Err(errors::coded(errors::MCPC_LIMIT, &[&MAX_TOOL_OVERRIDES.to_string()]));
        }
        server.tools.insert(tool.to_string(), mode);
        server.updated_at = now_ms();
        Ok(())
    })
}

pub fn add(spec: &ServerSpec) -> Result<ServerConfig, String> {
    add_in(&store_path(), spec)
}

pub fn update(id: &str, patch: &ServerPatch) -> Result<(ServerConfig, ServerConfig), String> {
    update_in(&store_path(), id, patch)
}

pub fn remove(id: &str) -> Result<ServerConfig, String> {
    remove_in(&store_path(), id)
}

pub fn approve(id: &str, shown: &str) -> Result<ServerConfig, String> {
    approve_in(&store_path(), id, shown)
}

pub fn set_tool_mode(id: &str, tool: &str, mode: ToolMode) -> Result<(), String> {
    set_tool_mode_in(&store_path(), id, tool, mode)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roadeep-mcpc-{tag}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn http(url: &str) -> ServerSpec {
        ServerSpec { name: "GitHub".into(), source: "directory:github".into(), transport: Transport::Http { url: url.into() }, auth: Auth::Bearer, enabled: None }
    }

    fn stdio(command: &str, args: &[&str]) -> ServerSpec {
        ServerSpec {
            name: "Files".into(),
            source: "custom".into(),
            transport: Transport::Stdio { command: command.into(), args: args.iter().map(|s| s.to_string()).collect(), env: vec!["API_KEY".into()] },
            auth: Auth::None,
            enabled: Some(false),
        }
    }

    #[test]
    fn the_file_format_matches_the_contract() {
        let parsed: ServerConfig = serde_json::from_value(json!({
            "id": "gh-3f9a", "name": "GitHub", "source": "directory:github",
            "transport": { "type": "http", "url": "https://api.githubcopilot.com/mcp/" },
            "auth": { "type": "header", "name": "X-Api-Key" },
            "tools": { "create_issue": "ask", "search_code": "auto", "delete_repo": "off" },
            "createdAt": 1, "updatedAt": 2
        }))
        .unwrap();
        assert!(parsed.enabled, "enabled defaults to true");
        assert_eq!(parsed.auth, Auth::Header { name: "X-Api-Key".into() });
        assert_eq!(parsed.tools["delete_repo"], ToolMode::Off);
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back["transport"]["type"], "http");
        assert!(back.get("approvedCommand").is_none());
        let s: Transport = serde_json::from_value(json!({ "type": "stdio", "command": "npx", "args": ["-y", "x"], "env": ["API_KEY"] })).unwrap();
        assert_eq!(serde_json::to_value(&s).unwrap()["env"], json!(["API_KEY"]));
        assert_eq!(serde_json::to_value(Auth::None).unwrap(), json!({ "type": "none" }));
    }

    #[test]
    fn round_trip_add_update_approve_remove() {
        let dir = temp_dir("store");
        let path = dir.join(FILE_NAME);
        let gh = add_in(&path, &http("https://api.githubcopilot.com/mcp/")).unwrap();
        assert!(gh.id.starts_with("github-") && valid_id(&gh.id), "{}", gh.id);
        assert!(gh.enabled);
        let fs = add_in(&path, &stdio("npx", &["-y", "@modelcontextprotocol/server-filesystem", r"C:\Users\me\Documents"])).unwrap();
        assert!(!fs.enabled);
        assert!(!command_approved(&fs));

        // Only the command that was shown can be approved.
        let shown = current_command_hash(&fs).unwrap();
        assert_eq!(approve_in(&path, &fs.id, &"0".repeat(64)).unwrap_err(), "E_MCPC_COMMAND_CHANGED");
        assert!(!command_approved(&load_from(&path).unwrap()[1]), "a refused approval writes nothing");
        let approved = approve_in(&path, &fs.id, &shown).unwrap();
        assert!(command_approved(&approved));
        assert!(approve_in(&path, &gh.id, &shown).is_err(), "only stdio servers have a command");

        // Renaming or toggling keeps the approval; a new argument drops it.
        let (_, renamed) = update_in(&path, &fs.id, &ServerPatch { name: Some(" Docs ".into()), enabled: Some(true), ..Default::default() }).unwrap();
        assert_eq!(renamed.name, "Docs");
        assert!(renamed.enabled && command_approved(&renamed));
        let patch = ServerPatch { transport: Some(Transport::Stdio { command: "npx".into(), args: vec!["-y".into(), "evil".into()], env: vec![] }), ..Default::default() };
        let (before, after) = update_in(&path, &fs.id, &patch).unwrap();
        assert!(before.approved_command.is_some());
        assert_eq!(after.approved_command, None);
        // So does a new variable name: it changes what runs, too.
        let again = approve_in(&path, &fs.id, &current_command_hash(&after).unwrap()).unwrap();
        let patch = ServerPatch { transport: Some(Transport::Stdio { command: "npx".into(), args: vec!["-y".into(), "evil".into()], env: vec!["NODE_OPTIONS".into()] }), ..Default::default() };
        let (_, after) = update_in(&path, &fs.id, &patch).unwrap();
        assert!(command_approved(&again) && !command_approved(&after));
        assert_eq!(after.approved_command, None);

        set_tool_mode_in(&path, &gh.id, "delete_repo", ToolMode::Off).unwrap();
        assert!(set_tool_mode_in(&path, &gh.id, "bad\nname", ToolMode::Off).is_err());
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].tools["delete_repo"], ToolMode::Off);

        remove_in(&path, &gh.id).unwrap();
        assert_eq!(remove_in(&path, &gh.id).unwrap_err(), format!("E_MCPC_UNKNOWN_SERVER|{}", gh.id));
        assert_eq!(load_from(&path).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn specs_are_validated() {
        let dir = temp_dir("validate");
        let path = dir.join(FILE_NAME);
        for url in ["http://example.com/mcp", "ftp://x", "https://user:pw@x.com/", "https://x.com/#f", "not a url", "http://10.0.0.1/"] {
            assert_eq!(add_in(&path, &http(url)).unwrap_err(), "E_MCPC_INVALID|url", "{url}");
        }
        for url in ["http://localhost:3000/mcp", "http://127.0.0.1:8931/mcp", "https://mcp.notion.com/mcp"] {
            assert!(add_in(&path, &http(url)).is_ok(), "{url}");
        }
        assert_eq!(add_in(&path, &stdio("", &[])).unwrap_err(), "E_MCPC_INVALID|command");
        assert_eq!(add_in(&path, &stdio("npx\nevil", &[])).unwrap_err(), "E_MCPC_INVALID|command");
        assert_eq!(add_in(&path, &stdio("npx", &["a\0b"])).unwrap_err(), "E_MCPC_INVALID|args");
        let many: Vec<&str> = (0..=MAX_ARGS).map(|_| "x").collect();
        assert_eq!(add_in(&path, &stdio("npx", &many)).unwrap_err(), "E_MCPC_INVALID|args");
        let mut bad_env = stdio("npx", &[]);
        bad_env.transport = Transport::Stdio { command: "npx".into(), args: vec![], env: vec!["A".into(), "a".into()] };
        assert_eq!(add_in(&path, &bad_env).unwrap_err(), "E_MCPC_INVALID|env");
        let mut stdio_token = stdio("npx", &[]);
        stdio_token.auth = Auth::Bearer;
        assert_eq!(add_in(&path, &stdio_token).unwrap_err(), "E_MCPC_INVALID|auth");
        let mut header = http("https://x.com/mcp");
        header.auth = Auth::Header { name: "Mcp-Session-Id".into() };
        assert_eq!(add_in(&path, &header).unwrap_err(), "E_MCPC_INVALID|header");
        header.auth = Auth::Header { name: "X Api".into() };
        assert_eq!(add_in(&path, &header).unwrap_err(), "E_MCPC_INVALID|header");
        let mut name = http("https://x.com/mcp");
        name.name = "a\u{202E}b".into();
        assert_eq!(add_in(&path, &name).unwrap_err(), "E_MCPC_INVALID|name");
        name.name = "x".repeat(61);
        assert_eq!(add_in(&path, &name).unwrap_err(), "E_MCPC_INVALID|name");
        let mut source = http("https://x.com/mcp");
        source.source = "directory:../x".into();
        assert_eq!(add_in(&path, &source).unwrap_err(), "E_MCPC_INVALID|source");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_file_is_kept_aside_and_valid_entries_survive() {
        let dir = temp_dir("damaged");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(load_from(&path).unwrap().is_empty());
        let kept: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert!(kept.iter().any(|n| n.starts_with("mcp-servers.json.bad-")), "{kept:?}");

        let good = json!({ "id": "ok-1234", "name": "Ok", "source": "custom", "transport": { "type": "http", "url": "https://a.com/mcp" }, "auth": { "type": "none" } });
        let bad = json!({ "id": "BAD", "name": "x", "source": "custom", "transport": { "type": "http", "url": "https://a.com" }, "auth": { "type": "none" } });
        std::fs::write(&path, serde_json::to_vec(&json!({ "version": 1, "servers": [good, bad] })).unwrap()).unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "ok-1234");
        // The cleaned list was written back.
        assert_eq!(load_from(&path).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ids_and_hashes() {
        let id = new_id("My GitHub (work)", &[]);
        assert!(id.starts_with("my-github-work-") && valid_id(&id), "{id}");
        let id = new_id("گیت‌هاب", &[]);
        assert!(id.starts_with("mcp-") && valid_id(&id), "{id}");
        assert!(valid_id(&new_id(&"a".repeat(60), &[])));
        assert_ne!(command_hash("a", &["b c".into()], &[]), command_hash("a", &["b".into(), "c".into()], &[]));
        assert_ne!(command_hash("a", &[], &[]), command_hash("a", &[], &["PATH".into()]), "variable names count");
        assert_eq!(command_hash("npx", &[], &[]).len(), 64);
    }
}
