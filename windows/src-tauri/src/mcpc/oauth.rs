// OAuth sign-in for the user's remote MCP servers, per the MCP authorization
// spec (2025-06-18): protected resource metadata (RFC 9728) → authorization
// server metadata (RFC 8414 / OpenID discovery) → dynamic client registration
// (RFC 7591) → authorization code with PKCE S256 through the system browser and
// a one-shot loopback listener (RFC 8252) → tokens bound to the server with the
// `resource` parameter (RFC 8707).
//
// The tokens are JSON in the server's `oauth` keyring slot and never leave
// Rust. The HTTP transport asks `bearer` for a valid access token before each
// request (refreshing it a minute early, one refresh at a time per server) and
// tells `on_unauthorized` when the server answers 401.
//
// Every endpoint must be https. Plain http is accepted only when the MCP server
// itself is on this machine (a developer's local server), and then only for
// loopback hosts — a remote server can't point the app at local services.
// Nothing here logs a token, a code, a client secret or a URL with its query.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use super::store::{self, Auth, Transport};
use crate::errors;

/// The keyring slot (secrets::mcpc_*) holding the JSON below.
const SLOT: &str = "oauth";
/// How long the browser has to come back to the loopback listener.
const CALLBACK_WAIT: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Metadata, registration and token answers are small; anything bigger is wrong.
const MAX_BODY: usize = 1 << 20;
/// A callback request is one GET line and a few headers.
const MAX_REQUEST: usize = 16 * 1024;
/// Refresh this long before the access token runs out.
const REFRESH_EARLY_SECS: u64 = 60;
const MAX_TOKEN_CHARS: usize = 16 * 1024;
const MAX_HINT_CHARS: usize = 2000;
const USER_AGENT: &str = "Roadeep-Desktop";

// ── Stored state ──────────────────────────────────────────────────────────────

/// How the client proves itself at the token endpoint. Registration asks for
/// `none` (a public native client); a server that only takes secrets gets one
/// of the two secret methods instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuth {
    #[default]
    None,
    ClientSecretBasic,
    ClientSecretPost,
}

/// The `oauth` slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_auth: ClientAuth,
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix seconds; 0 when the server didn't say (used until refused).
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub token_endpoint: String,
    pub issuer: String,
    /// What the tokens were issued for; `bearer` only hands them to a server
    /// URL this covers.
    pub resource: String,
}

impl Tokens {
    fn fresh(&self, now: u64) -> bool {
        self.expires_at == 0 || now + REFRESH_EARLY_SECS < self.expires_at
    }

    fn creds(&self) -> Creds {
        Creds { id: self.client_id.clone(), secret: self.client_secret.clone(), auth: self.client_auth }
    }
}

/// What a 401 told us, kept for the next sign-in and the next `bearer`.
#[derive(Debug, Clone, Default)]
pub struct Hint {
    /// `resource_metadata` from `WWW-Authenticate`.
    pub resource_metadata: Option<String>,
    /// `scope` from `WWW-Authenticate`.
    pub scope: Option<String>,
    /// The current access token was refused: refresh before using it again.
    stale: bool,
}

static HINTS: LazyLock<Mutex<HashMap<String, Hint>>> = LazyLock::new(Default::default);
/// One sign-in per server: a second start (or a sign-out) cancels the first.
static FLOWS: LazyLock<Mutex<HashMap<String, (u64, oneshot::Sender<()>)>>> = LazyLock::new(Default::default);
/// One refresh at a time per server.
static REFRESH: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = LazyLock::new(Default::default);
/// Per server, bumped whenever its sign-in is ended or replaced from outside a
/// refresh (sign-out, removal, a new address or auth, a new sign-in). A refresh
/// writes its tokens back only when nothing bumped it meanwhile. Every bump
/// runs its keyring change under this lock, and a refresh does its check and
/// its write under it too, so the two can't interleave and a refresh racing a
/// sign-out can't bring the tokens back. Entries are never removed: dropping
/// one would reset it to a value an old refresh may still hold.
static EPOCHS: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Default::default);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Tests reach this module's helpers; they must not write the user's log.
fn note(message: String) {
    if cfg!(not(test)) {
        crate::log::line(message);
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// For log lines: the host of a URL, never its path or query.
fn host_of(url: &str) -> String {
    Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "?".into())
}

/// Server-provided text for an error argument: one line, no control or bidi
/// characters, capped.
fn clean(text: &str, max: usize) -> String {
    crate::catalog::engine::clean(text, max)
}

// ── Public hooks (the HTTP transport calls these) ─────────────────────────────

/// A valid access token for the server, refreshing it first if it is about to
/// run out (or was just refused). None when not signed in, when the tokens were
/// issued for a different URL than the server's current one, or when a refresh
/// failed — the transport then sends no Authorization and reports needs_auth.
pub async fn bearer(app: &AppHandle, server_id: &str) -> Option<String> {
    let _ = app;
    let server = store::get(server_id).ok()?;
    let (Transport::Http { url }, Auth::Oauth) = (&server.transport, &server.auth) else {
        return None;
    };
    let server_url = Url::parse(url).ok()?;
    let gate = refresh_gate(server_id);
    let _turn = gate.lock().await;
    // Before the tokens are read: a sign-out after this point is always seen.
    let epoch = epoch_of(server_id);
    let tokens = load_tokens(server_id)?;
    if !resource_covers(&tokens.resource, &server_url) {
        note(format!("mcpc oauth: {server_id}: tokens were issued for another address; sign in again"));
        return None;
    }
    let stale = take_stale(server_id);
    if !stale && tokens.fresh(now_secs()) {
        return Some(tokens.access_token);
    }
    let Some(refresh_token) = tokens.refresh_token.clone() else {
        note(format!("mcpc oauth: {server_id}: access token {} and no refresh token", if stale { "refused" } else { "expired" }));
        return None;
    };
    let policy = Policy::for_server(&server_url);
    // The sign-in this refresh renews is still the one stored: same server
    // address, still OAuth, the slot still holding the refresh token we used.
    let still_current = || {
        let same_server = matches!(store::get(server_id), Ok(now) if now.transport == server.transport && now.auth == Auth::Oauth);
        same_server && load_tokens(server_id).and_then(|t| t.refresh_token).as_deref() == Some(refresh_token.as_str())
    };
    match refresh(&http_client().ok()?, &policy, &tokens, &refresh_token).await {
        Ok(renewed) => match commit_if_current(server_id, epoch, still_current, || save_tokens(server_id, &renewed)) {
            Some(Ok(())) => {
                note(format!("mcpc oauth: {server_id}: access token refreshed"));
                Some(renewed.access_token)
            }
            Some(Err(err)) => {
                note(format!("mcpc oauth: {server_id}: refreshed, but could not store the tokens: {err}"));
                Some(renewed.access_token)
            }
            None => {
                note(format!("mcpc oauth: {server_id}: signed out or changed during a refresh; the new tokens were dropped"));
                None
            }
        },
        Err(Refused::Grant(reason)) => {
            // The refresh token is dead: keeping it would only fail again. A
            // sign-in made meanwhile is someone else's and stays.
            note(format!("mcpc oauth: {server_id}: refresh refused ({reason}); signed out"));
            if let Some(Err(err)) = commit_if_current(server_id, epoch, still_current, || crate::secrets::mcpc_set(server_id, SLOT, "")) {
                note(format!("mcpc oauth: {server_id}: could not clear the dead tokens: {err}"));
            }
            None
        }
        Err(Refused::Other(err)) => {
            note(format!("mcpc oauth: {server_id}: refresh failed: {err}"));
            None
        }
    }
}

/// The server answered 401: remember its `WWW-Authenticate` hints for the
/// next sign-in, and make the next `bearer` refresh instead of resending the
/// refused token.
pub fn on_unauthorized(server_id: &str, www_authenticate: Option<&str>) {
    let params = www_authenticate.map(bearer_params).unwrap_or_default();
    let mut hints = lock(&HINTS);
    let hint = hints.entry(server_id.to_string()).or_default();
    if let Some(rm) = params.get("resource_metadata").filter(|v| v.len() <= MAX_HINT_CHARS && Url::parse(v).is_ok()) {
        hint.resource_metadata = Some(rm.clone());
    }
    if let Some(scope) = params.get("scope").filter(|v| scope_ok(v)) {
        hint.scope = Some(scope.clone());
    }
    hint.stale = true;
    let has_metadata = hint.resource_metadata.is_some();
    drop(hints);
    note(format!("mcpc oauth: {server_id}: 401 from the server (resource metadata hint: {})", if has_metadata { "yes" } else { "no" }));
}

/// The server was removed, or its address or auth changed: drop everything
/// kept in memory for it and stop a sign-in in progress. (Its keyring slots
/// are cleared by the caller.)
pub fn forget(server_id: &str) {
    cancel_flow(server_id);
    lock(&HINTS).remove(server_id);
    lock(&REFRESH).remove(server_id);
}

fn epoch_of(server_id: &str) -> u64 {
    lock(&EPOCHS).get(server_id).copied().unwrap_or(0)
}

/// Ends or replaces a server's sign-in: bumps its epoch and runs `change`
/// (the keyring write) under the same lock, so a refresh in flight can't write
/// the old sign-in's tokens back afterwards. Sign-out, removal, an address or
/// auth change and a new sign-in all go through here.
pub fn invalidate<T>(server_id: &str, change: impl FnOnce() -> T) -> T {
    let mut epochs = lock(&EPOCHS);
    let epoch = epochs.entry(server_id.to_string()).or_insert(0);
    *epoch = epoch.wrapping_add(1);
    change()
}

/// A refresh's write: runs `write` only when nothing invalidated the server
/// since `epoch` and `still_current` holds, both under the epochs lock.
fn commit_if_current<T>(server_id: &str, epoch: u64, still_current: impl FnOnce() -> bool, write: impl FnOnce() -> T) -> Option<T> {
    let epochs = lock(&EPOCHS);
    if epochs.get(server_id).copied().unwrap_or(0) != epoch || !still_current() {
        return None;
    }
    let out = write();
    drop(epochs);
    Some(out)
}

fn take_stale(server_id: &str) -> bool {
    lock(&HINTS).get_mut(server_id).map(|h| std::mem::take(&mut h.stale)).unwrap_or(false)
}

fn refresh_gate(server_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    lock(&REFRESH).entry(server_id.to_string()).or_default().clone()
}

fn load_tokens(server_id: &str) -> Option<Tokens> {
    let raw = crate::secrets::mcpc_get(server_id, SLOT)?;
    match serde_json::from_str::<Tokens>(&raw) {
        Ok(tokens) if !tokens.access_token.is_empty() => Some(tokens),
        Ok(_) => None,
        Err(err) => {
            // Not the value: serde's message names the field, not the data.
            note(format!("mcpc oauth: {server_id}: stored sign-in is unreadable ({err})"));
            None
        }
    }
}

fn save_tokens(server_id: &str, tokens: &Tokens) -> Result<(), String> {
    let json = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
    crate::secrets::mcpc_set(server_id, SLOT, &json)
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Settings → «ورود»: discovers how the server signs in, opens the browser,
/// waits (up to 5 minutes) for the user to finish, stores the tokens. Resolves
/// when signed in; any failure is a coded error.
#[tauri::command]
pub async fn mcpc_oauth_start(app: AppHandle, id: String) -> Result<(), String> {
    let server = store::get(&id)?;
    let url = match (&server.transport, &server.auth) {
        (Transport::Http { url }, Auth::Oauth) => url.clone(),
        _ => return Err(errors::coded(errors::MCPC_INVALID, &["auth"])),
    };
    let (generation, cancel) = begin_flow(&id);
    let hint = lock(&HINTS).get(&id).cloned().unwrap_or_default();
    note(format!("mcpc oauth: {id}: sign-in started for {}", host_of(&url)));
    let result = sign_in(&url, &hint, |auth_url| crate::open_url(auth_url.to_string()), cancel, CALLBACK_WAIT).await;
    end_flow(&id, generation);
    let tokens = result.inspect_err(|err| note(format!("mcpc oauth: {id}: sign-in failed: {}", err.lines().next().unwrap_or(""))))?;

    // The server may have been edited or removed while the browser was open.
    // Checked and saved under the epochs lock (an edit can't slip in between),
    // and a refresh of the previous sign-in still in flight won't overwrite it.
    let saved = invalidate(&id, || match store::get(&id) {
        Ok(now) if now.transport == server.transport && now.auth == Auth::Oauth => Some(save_tokens(&id, &tokens)),
        _ => None,
    });
    match saved {
        None => {
            note(format!("mcpc oauth: {id}: the server changed during sign-in; tokens dropped"));
            return Err(errors::coded(errors::MCPC_OAUTH_CANCELLED, &[]));
        }
        Some(Err(e)) => {
            note(format!("mcpc oauth: {id}: could not store the tokens: {e}"));
            return Err(errors::coded(errors::MCPC_OAUTH_KEYRING, &[&e]));
        }
        Some(Ok(())) => {}
    }
    if let Some(hint) = lock(&HINTS).get_mut(&id) {
        hint.stale = false;
    }
    note(format!("mcpc oauth: {id}: signed in ({})", host_of(&tokens.issuer)));
    super::auth_changed(&app, &id);
    Ok(())
}

/// Settings → «خروج»: forgets the tokens (and stops a sign-in in progress).
#[tauri::command]
pub fn mcpc_oauth_signout(app: AppHandle, id: String) -> Result<(), String> {
    store::get(&id)?;
    cancel_flow(&id);
    lock(&HINTS).remove(&id);
    invalidate(&id, || crate::secrets::mcpc_set(&id, SLOT, "")).map_err(|e| {
        note(format!("mcpc oauth: {id}: could not delete the tokens: {e}"));
        errors::coded(errors::MCPC_OAUTH_KEYRING, &[&e])
    })?;
    note(format!("mcpc oauth: {id}: signed out"));
    super::auth_changed(&app, &id);
    Ok(())
}

fn begin_flow(id: &str) -> (u64, oneshot::Receiver<()>) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let generation = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    // Replacing the entry drops the previous sender, which cancels that flow.
    if let Some((_, previous)) = lock(&FLOWS).insert(id.to_string(), (generation, tx)) {
        let _ = previous.send(());
    }
    (generation, rx)
}

fn end_flow(id: &str, generation: u64) {
    let mut flows = lock(&FLOWS);
    if flows.get(id).is_some_and(|(g, _)| *g == generation) {
        flows.remove(id);
    }
}

fn cancel_flow(id: &str) {
    if let Some((_, tx)) = lock(&FLOWS).remove(id) {
        let _ = tx.send(());
    }
}

// ── The flow ──────────────────────────────────────────────────────────────────

/// Which URLs this flow may talk to.
#[derive(Debug, Clone, Copy)]
struct Policy {
    loopback_http: bool,
}

impl Policy {
    fn for_server(server: &Url) -> Policy {
        Policy { loopback_http: server.scheme() == "http" && is_loopback(server) }
    }

    fn allows(&self, url: &Url) -> bool {
        let scheme_ok = url.scheme() == "https" || (self.loopback_http && url.scheme() == "http" && is_loopback(url));
        scheme_ok && url.host_str().is_some() && url.username().is_empty() && url.password().is_none()
    }

    fn check(&self, raw: &str) -> Result<Url, String> {
        let url = Url::parse(raw).map_err(|_| errors::coded(errors::MCPC_OAUTH_INSECURE, &[&host_of(raw)]))?;
        if raw.len() > 2000 || !self.allows(&url) {
            return Err(errors::coded(errors::MCPC_OAUTH_INSECURE, &[&host_of(raw)]));
        }
        Ok(url)
    }
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// The client's identity at the token endpoint.
#[derive(Debug, Clone)]
struct Creds {
    id: String,
    secret: Option<String>,
    auth: ClientAuth,
}

#[derive(Debug, Clone)]
struct ServerMeta {
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    registration_endpoint: Option<Url>,
    token_auth_methods: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct Discovered {
    resource: String,
    scope: Option<String>,
    meta: ServerMeta,
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // A redirect could carry a request (and a code or token) somewhere the
        // metadata never named.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| errors::coded(errors::MCPC_NETWORK, &[&e.to_string()]))
}

/// Discovery, registration, the browser and the code exchange. `open` gets
/// the authorization URL (the app opens the default browser with it).
pub(crate) async fn sign_in(
    server_url: &str,
    hint: &Hint,
    open: impl FnOnce(&str),
    cancel: oneshot::Receiver<()>,
    wait: Duration,
) -> Result<Tokens, String> {
    let server = Url::parse(server_url).map_err(|_| errors::coded(errors::MCPC_INVALID, &["url"]))?;
    let policy = Policy::for_server(&server);
    if !policy.allows(&server) {
        return Err(errors::coded(errors::MCPC_OAUTH_INSECURE, &[&host_of(server_url)]));
    }
    let http = http_client()?;
    let found = discover(&http, &policy, &server, hint).await?;

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| errors::coded(errors::MCPC_OAUTH_LISTEN, &[&e.to_string()]))?;
    let port = listener.local_addr().map_err(|e| errors::coded(errors::MCPC_OAUTH_LISTEN, &[&e.to_string()]))?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let creds = register(&http, &found, &redirect_uri).await?;
    let verifier = random_token();
    let state = random_token();
    let mut auth_url = found.meta.authorization_endpoint.clone();
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &creds.id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("code_challenge", &pkce_challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("resource", &found.resource);
        if let Some(scope) = &found.scope {
            q.append_pair("scope", scope);
        }
    }
    open(auth_url.as_str());
    let code = wait_for_code(listener, state, cancel, wait).await?;

    let form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
        ("resource", found.resource.clone()),
    ];
    let answer = token_request(&http, &found.meta.token_endpoint, &creds, form).await.map_err(|e| match e {
        Refused::Grant(detail) | Refused::Other(detail) if !detail.starts_with("E_") => errors::coded(errors::MCPC_OAUTH_TOKEN, &[&detail]),
        Refused::Grant(coded) | Refused::Other(coded) => coded,
    })?;
    Ok(Tokens {
        client_id: creds.id,
        client_secret: creds.secret,
        client_auth: creds.auth,
        access_token: answer.access_token,
        refresh_token: answer.refresh_token,
        expires_at: answer.expires_in.map(|s| now_secs().saturating_add(s)).unwrap_or(0),
        scope: answer.scope.or(found.scope),
        token_endpoint: found.meta.token_endpoint.to_string(),
        issuer: found.meta.issuer,
        resource: found.resource,
    })
}

// ── Discovery ─────────────────────────────────────────────────────────────────

async fn discover(http: &reqwest::Client, policy: &Policy, server: &Url, hint: &Hint) -> Result<Discovered, String> {
    let mut hint = hint.clone();
    if hint.resource_metadata.is_none() {
        // The 401 itself is the primary way a server says where its metadata is.
        if let Some(header) = probe(http, server).await {
            let params = bearer_params(&header);
            hint.resource_metadata = params.get("resource_metadata").cloned();
            hint.scope = hint.scope.or_else(|| params.get("scope").filter(|v| scope_ok(v)).cloned());
        }
    }
    let mut candidates: Vec<Url> = Vec::new();
    if let Some(raw) = &hint.resource_metadata {
        candidates.push(policy.check(raw)?);
    }
    candidates.extend(prm_candidates(server));

    let mut prm: Option<Value> = None;
    for url in &candidates {
        if let Some(doc) = fetch_json(http, url).await? {
            prm = Some(doc);
            break;
        }
    }

    let (resource, issuer, prm_scopes) = match prm {
        Some(doc) => {
            let resource = match doc.get("resource").and_then(Value::as_str) {
                Some(r) if resource_covers(r, server) => r.to_string(),
                Some(r) => {
                    let detail = format!("resource on {} does not match the server", host_of(r));
                    return Err(errors::coded(errors::MCPC_OAUTH_DISCOVERY, &[&detail]));
                }
                None => canonical(server),
            };
            let issuer = doc
                .get("authorization_servers")
                .and_then(Value::as_array)
                .and_then(|list| list.iter().find_map(Value::as_str))
                .ok_or_else(|| errors::coded(errors::MCPC_OAUTH_DISCOVERY, &["no authorization_servers"]))?
                .to_string();
            let scopes = doc.get("scopes_supported").and_then(Value::as_array).map(|list| {
                list.iter().filter_map(Value::as_str).filter(|s| scope_ok(s)).collect::<Vec<_>>().join(" ")
            });
            (resource, issuer, scopes.filter(|s| !s.is_empty()))
        }
        // Servers from before protected resource metadata (2025-03-26) are
        // their own authorization server.
        None => (canonical(server), origin(server), None),
    };
    let issuer_url = policy.check(&issuer)?;
    let meta = server_metadata(http, policy, &issuer_url).await?;
    let scope = hint.scope.or(prm_scopes).filter(|s| scope_ok(s));
    Ok(Discovered { resource, scope, meta })
}

/// An unauthenticated `initialize`, only to read the `WWW-Authenticate` of the
/// 401 it gets. Any other outcome just means no hint.
async fn probe(http: &reqwest::Client, server: &Url) -> Option<String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "Roadeep Desktop", "version": env!("CARGO_PKG_VERSION") } }
    });
    let resp = http
        .post(server.clone())
        .header("Accept", "application/json, text/event-stream")
        .json(&body)
        .send()
        .await
        .ok()?;
    if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
        return None;
    }
    resp.headers().get_all("www-authenticate").iter().filter_map(|v| v.to_str().ok()).find(|v| v.to_ascii_lowercase().contains("bearer")).map(str::to_string)
}

fn origin(url: &Url) -> String {
    let mut o = url.clone();
    o.set_path("/");
    o.set_query(None);
    o.set_fragment(None);
    o.to_string()
}

/// The server URL as a resource indicator: no fragment, otherwise as stored.
fn canonical(url: &Url) -> String {
    let mut c = url.clone();
    c.set_fragment(None);
    c.to_string()
}

fn with_path(base: &Url, path: &str) -> Url {
    let mut u = base.clone();
    u.set_path(path);
    u.set_query(None);
    u.set_fragment(None);
    u
}

/// RFC 9728 §3.1: the well-known name goes between the host and the path;
/// then the root form for servers that only publish that.
fn prm_candidates(server: &Url) -> Vec<Url> {
    let path = server.path().trim_end_matches('/');
    let mut out = Vec::new();
    if !path.is_empty() {
        out.push(with_path(server, &format!("/.well-known/oauth-protected-resource{path}")));
    }
    out.push(with_path(server, "/.well-known/oauth-protected-resource"));
    out
}

/// RFC 8414 §3.1 path insertion, then OpenID Connect discovery in both forms.
fn as_candidates(issuer: &Url) -> Vec<Url> {
    let path = issuer.path().trim_end_matches('/');
    if path.is_empty() {
        vec![with_path(issuer, "/.well-known/oauth-authorization-server"), with_path(issuer, "/.well-known/openid-configuration")]
    } else {
        vec![
            with_path(issuer, &format!("/.well-known/oauth-authorization-server{path}")),
            with_path(issuer, &format!("/.well-known/openid-configuration{path}")),
            with_path(issuer, &format!("{path}/.well-known/openid-configuration")),
        ]
    }
}

/// Tokens issued for `resource` may go to `server`: same origin, and the
/// resource path is the server path or a parent of it (segment-wise).
fn resource_covers(resource: &str, server: &Url) -> bool {
    let Ok(r) = Url::parse(resource) else { return false };
    if r.scheme() != server.scheme() || r.host_str() != server.host_str() || r.port_or_known_default() != server.port_or_known_default() || r.fragment().is_some() {
        return false;
    }
    let rp = r.path().trim_end_matches('/');
    let sp = server.path().trim_end_matches('/');
    rp.is_empty() || sp == rp || sp.starts_with(&format!("{rp}/"))
}

/// A space-separated scope list of printable ASCII (RFC 6749 §3.3).
fn scope_ok(scope: &str) -> bool {
    !scope.trim().is_empty() && scope.len() <= 1000 && scope.chars().all(|c| c == ' ' || (c.is_ascii_graphic() && c != '"' && c != '\\'))
}

async fn server_metadata(http: &reqwest::Client, policy: &Policy, issuer: &Url) -> Result<ServerMeta, String> {
    for url in as_candidates(issuer) {
        let Some(doc) = fetch_json(http, &url).await? else { continue };
        let endpoint = |key: &str| -> Result<Option<Url>, String> {
            match doc.get(key).and_then(Value::as_str) {
                Some(raw) => policy.check(raw).map(Some),
                None => Ok(None),
            }
        };
        let authorization_endpoint =
            endpoint("authorization_endpoint")?.ok_or_else(|| errors::coded(errors::MCPC_OAUTH_DISCOVERY, &["no authorization_endpoint"]))?;
        let token_endpoint = endpoint("token_endpoint")?.ok_or_else(|| errors::coded(errors::MCPC_OAUTH_DISCOVERY, &["no token_endpoint"]))?;
        let registration_endpoint = endpoint("registration_endpoint")?;
        let strings = |key: &str| doc.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>());
        if strings("code_challenge_methods_supported").is_some_and(|m| !m.iter().any(|x| x == "S256")) {
            return Err(errors::coded(errors::MCPC_OAUTH_DISCOVERY, &["no PKCE S256"]));
        }
        let issuer_text = doc.get("issuer").and_then(Value::as_str).unwrap_or(issuer.as_str()).to_string();
        if issuer_text.trim_end_matches('/') != issuer.as_str().trim_end_matches('/') {
            // RFC 8414 wants them equal; several real servers differ only in
            // form. The endpoints were checked on their own, so note and go on.
            note(format!("mcpc oauth: metadata issuer differs from {}", host_of(issuer.as_str())));
        }
        return Ok(ServerMeta {
            issuer: issuer_text,
            authorization_endpoint,
            token_endpoint,
            registration_endpoint,
            token_auth_methods: strings("token_endpoint_auth_methods_supported"),
        });
    }
    Err(errors::coded(errors::MCPC_OAUTH_DISCOVERY, &["no authorization server metadata"]))
}

/// GET a metadata document: Some(JSON object) on 200, None on any 4xx (try
/// the next candidate), an error when the network or the server fails.
async fn fetch_json(http: &reqwest::Client, url: &Url) -> Result<Option<Value>, String> {
    let resp = http
        .get(url.clone())
        .header("Accept", "application/json")
        .header("MCP-Protocol-Version", "2025-06-18")
        .send()
        .await
        .map_err(|e| errors::coded(errors::MCPC_NETWORK, &[&network_detail(&e)]))?;
    let status = resp.status();
    if status.is_client_error() || status.is_redirection() {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(errors::coded(errors::MCPC_HTTP, &[status.as_str()]));
    }
    let bytes = read_capped(resp).await?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(doc) if doc.is_object() => Ok(Some(doc)),
        _ => Ok(None),
    }
}

/// reqwest's message without the URL it embeds (which may carry a query).
fn network_detail(err: &reqwest::Error) -> String {
    let kind = if err.is_timeout() {
        "timeout"
    } else if err.is_connect() {
        "could not connect"
    } else {
        "request failed"
    };
    match err.url() {
        Some(u) => format!("{kind} ({})", u.host_str().unwrap_or("?")),
        None => kind.to_string(),
    }
}

async fn read_capped(mut resp: reqwest::Response) -> Result<Vec<u8>, String> {
    if resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        return Err(errors::coded(errors::MCPC_TOO_LARGE, &[]));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| errors::coded(errors::MCPC_NETWORK, &[&network_detail(&e)]))? {
        if out.len() + chunk.len() > MAX_BODY {
            return Err(errors::coded(errors::MCPC_TOO_LARGE, &[]));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

// ── Registration ──────────────────────────────────────────────────────────────

/// Which method to ask for: `none` when the server takes it (or doesn't say),
/// else a secret method it lists. JWT-only servers can't take a native app.
fn pick_client_auth(methods: Option<&[String]>) -> Option<ClientAuth> {
    let Some(methods) = methods else { return Some(ClientAuth::None) };
    let has = |m: &str| methods.iter().any(|x| x == m);
    if methods.is_empty() || has("none") {
        Some(ClientAuth::None)
    } else if has("client_secret_basic") {
        Some(ClientAuth::ClientSecretBasic)
    } else if has("client_secret_post") {
        Some(ClientAuth::ClientSecretPost)
    } else {
        None
    }
}

fn auth_name(auth: ClientAuth) -> &'static str {
    match auth {
        ClientAuth::None => "none",
        ClientAuth::ClientSecretBasic => "client_secret_basic",
        ClientAuth::ClientSecretPost => "client_secret_post",
    }
}

async fn register(http: &reqwest::Client, found: &Discovered, redirect_uri: &str) -> Result<Creds, String> {
    let Some(endpoint) = &found.meta.registration_endpoint else {
        return Err(errors::coded(errors::MCPC_OAUTH_NO_DCR, &[]));
    };
    let wanted = pick_client_auth(found.meta.token_auth_methods.as_deref()).ok_or_else(|| errors::coded(errors::MCPC_OAUTH_NO_DCR, &[]))?;
    let mut body = serde_json::json!({
        "client_name": "Roadeep Desktop",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": auth_name(wanted),
    });
    if let Some(scope) = &found.scope {
        body["scope"] = scope.clone().into();
    }
    let resp = http
        .post(endpoint.clone())
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| errors::coded(errors::MCPC_NETWORK, &[&network_detail(&e)]))?;
    let status = resp.status();
    let bytes = read_capped(resp).await?;
    let doc: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(errors::coded(errors::MCPC_OAUTH_REGISTER, &[&oauth_error_detail(status, &doc)]));
    }
    let id = doc.get("client_id").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 512);
    let Some(id) = id else {
        return Err(errors::coded(errors::MCPC_OAUTH_REGISTER, &["no client_id"]));
    };
    let secret = doc.get("client_secret").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 2048).map(str::to_string);
    let auth = match (&secret, doc.get("token_endpoint_auth_method").and_then(Value::as_str)) {
        (None, _) => ClientAuth::None,
        (Some(_), Some("client_secret_post")) => ClientAuth::ClientSecretPost,
        (Some(_), Some("client_secret_basic")) => ClientAuth::ClientSecretBasic,
        (Some(_), _) if wanted == ClientAuth::None => ClientAuth::ClientSecretPost,
        (Some(_), _) => wanted,
    };
    Ok(Creds { id: id.to_string(), secret, auth })
}

/// `<status> <error>: <description>` from an OAuth error body, cleaned.
fn oauth_error_detail(status: reqwest::StatusCode, doc: &Value) -> String {
    let error = doc.get("error").and_then(Value::as_str).unwrap_or("");
    let description = doc.get("error_description").and_then(Value::as_str).unwrap_or("");
    let text = match (error.is_empty(), description.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!(" {error}"),
        (true, false) => format!(" {description}"),
        (false, false) => format!(" {error}: {description}"),
    };
    clean(&format!("{}{text}", status.as_str()), 200)
}

// ── The browser round trip ────────────────────────────────────────────────────

/// 32 random bytes (two v4 UUIDs: 244 bits from the OS generator) as base64url
/// — 43 characters, a valid PKCE verifier and an unguessable state.
fn random_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    base64url(&bytes)
}

fn base64url(bytes: &[u8]) -> String {
    crate::util::base64_for(bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fn pkce_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// Accepts connections on the loopback port until one brings `/callback`.
/// Each connection is read on its own task: browsers open idle speculative
/// connections, and one of those must not hold up the real request.
async fn wait_for_code(listener: TcpListener, state: String, cancel: oneshot::Receiver<()>, wait: Duration) -> Result<String, String> {
    let (tx, mut rx) = mpsc::channel::<Result<String, String>>(4);
    let accept = async move {
        // Dropped with this future: no connection task outlives the flow.
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    let tx = tx.clone();
                    let state = state.clone();
                    tasks.spawn(async move {
                        if let Some(outcome) = handle_callback(socket, &state, wait).await {
                            let _ = tx.send(outcome).await;
                        }
                    });
                }
                Err(err) => return errors::coded(errors::MCPC_OAUTH_LISTEN, &[&err.to_string()]),
            }
        }
    };
    tokio::select! {
        Some(outcome) = rx.recv() => outcome,
        err = accept => Err(err),
        _ = cancel => Err(errors::coded(errors::MCPC_OAUTH_CANCELLED, &[])),
        _ = tokio::time::sleep(wait) => Err(errors::coded(errors::MCPC_OAUTH_TIMEOUT, &[])),
    }
}

/// One connection: answers it, and returns the flow's outcome when it was the
/// callback carrying our state (None for anything else, e.g. /favicon.ico or
/// a callback with a wrong or missing state).
async fn handle_callback(mut socket: TcpStream, state: &str, wait: Duration) -> Option<Result<String, String>> {
    let request = tokio::time::timeout(wait, read_request(&mut socket)).await.ok()??;
    let mut words = request.split(' ');
    let (method, target) = (words.next()?, words.next()?);
    if method != "GET" || !target.starts_with('/') {
        respond(&mut socket, "405 Method Not Allowed", None).await;
        return None;
    }
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        respond(&mut socket, "404 Not Found", None).await;
        return None;
    }
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    // Without our state it isn't the answer to this sign-in (a stale tab, a
    // page poking at the port): it gets a 400 and the real one is still
    // awaited, so a stray request can't end the flow.
    if query.get("state").map(String::as_str) != Some(state) {
        note("mcpc oauth: a callback without this sign-in's state was ignored".into());
        respond(&mut socket, "400 Bad Request", Some(false)).await;
        return None;
    }
    let outcome = callback_outcome(&query, state);
    respond(&mut socket, "200 OK", Some(outcome.is_ok())).await;
    Some(outcome)
}

fn callback_outcome(query: &HashMap<String, String>, state: &str) -> Result<String, String> {
    // Checked first: a request without our state is not an answer to our
    // request, whatever else it says (RFC 6749 §10.12).
    if query.get("state").map(String::as_str) != Some(state) {
        return Err(errors::coded(errors::MCPC_OAUTH_STATE, &[]));
    }
    if let Some(error) = query.get("error") {
        let description = query.get("error_description").map(|d| format!(": {d}")).unwrap_or_default();
        return Err(errors::coded(errors::MCPC_OAUTH_DENIED, &[&clean(&format!("{error}{description}"), 200)]));
    }
    match query.get("code") {
        Some(code) if !code.is_empty() && code.len() <= 4096 => Ok(code.clone()),
        _ => Err(errors::coded(errors::MCPC_OAUTH_DENIED, &["no code"])),
    }
}

/// The request line, once the whole header block has arrived.
async fn read_request(socket: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > MAX_REQUEST {
            return None;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    Some(text.lines().next()?.to_string())
}

/// `page`: Some(true) signed in, Some(false) failed, None no page. The page
/// is fixed text — nothing from the request is echoed into it.
async fn respond(socket: &mut TcpStream, status: &str, page: Option<bool>) {
    let body = match page {
        Some(true) => page_html("ورود انجام شد. می‌توانید این زبانه را ببندید و به Roadeep برگردید.", "Signed in. You can close this tab and go back to Roadeep."),
        Some(false) => page_html("ورود کامل نشد. به Roadeep برگردید و دوباره امتحان کنید.", "Sign-in didn't complete. Go back to Roadeep and try again."),
        None => String::new(),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = socket.write_all(head.as_bytes()).await;
    let _ = socket.write_all(body.as_bytes()).await;
    let _ = socket.shutdown().await;
}

fn page_html(fa: &str, en: &str) -> String {
    format!(
        "<!doctype html><html lang=\"fa\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Roadeep</title></head>\
<body style=\"margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#18181b;color:#f4f4f5;font:16px/1.7 Vazirmatn,Tahoma,system-ui,sans-serif\">\
<main style=\"text-align:center;padding:24px\"><p dir=\"rtl\" lang=\"fa\" style=\"margin:0 0 8px\">{fa}</p><p dir=\"ltr\" lang=\"en\" style=\"margin:0;color:#a1a1aa\">{en}</p></main></body></html>"
    )
}

// ── Token endpoint ────────────────────────────────────────────────────────────

#[derive(Debug)]
struct TokenAnswer {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
}

#[derive(Debug)]
enum Refused {
    /// The grant itself is no good (invalid_grant / invalid_client…): the
    /// stored tokens are dead. Detail for the log or the error argument.
    Grant(String),
    /// Anything else (network, 5xx, a malformed answer): may work later.
    Other(String),
}

/// RFC 6749 §2.3.1: the id and secret are form-encoded before Basic.
fn form_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

async fn token_request(http: &reqwest::Client, endpoint: &Url, creds: &Creds, mut form: Vec<(&str, String)>) -> Result<TokenAnswer, Refused> {
    let mut req = http.post(endpoint.clone()).header("Accept", "application/json");
    match (creds.auth, &creds.secret) {
        (ClientAuth::ClientSecretBasic, Some(secret)) => {
            req = req.basic_auth(form_encode(&creds.id), Some(form_encode(secret)));
        }
        (ClientAuth::ClientSecretPost, Some(secret)) => {
            form.push(("client_id", creds.id.clone()));
            form.push(("client_secret", secret.clone()));
        }
        _ => form.push(("client_id", creds.id.clone())),
    }
    let resp = req.form(&form).send().await.map_err(|e| Refused::Other(errors::coded(errors::MCPC_NETWORK, &[&network_detail(&e)])))?;
    let status = resp.status();
    let bytes = read_capped(resp).await.map_err(Refused::Other)?;
    let doc: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !status.is_success() {
        let error = doc.get("error").and_then(Value::as_str).unwrap_or("");
        let detail = oauth_error_detail(status, &doc);
        let dead = matches!(error, "invalid_grant" | "invalid_client" | "unauthorized_client");
        return Err(if dead { Refused::Grant(detail) } else { Refused::Other(detail) });
    }
    parse_token_answer(&doc).map_err(Refused::Other)
}

fn parse_token_answer(doc: &Value) -> Result<TokenAnswer, String> {
    let access_token = doc
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty() && t.len() <= MAX_TOKEN_CHARS && t.chars().all(|c| c.is_ascii_graphic()))
        .ok_or_else(|| "no usable access_token".to_string())?
        .to_string();
    if let Some(kind) = doc.get("token_type").and_then(Value::as_str) {
        // A DPoP or MAC token can't be sent as a plain bearer header.
        if !kind.eq_ignore_ascii_case("bearer") {
            return Err(format!("token_type {}", clean(kind, 20)));
        }
    }
    let expires_in = match doc.get("expires_in") {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    };
    let refresh_token = doc
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty() && t.len() <= MAX_TOKEN_CHARS)
        .map(str::to_string);
    let scope = doc.get("scope").and_then(Value::as_str).filter(|s| scope_ok(s)).map(str::to_string);
    Ok(TokenAnswer { access_token, refresh_token, expires_in, scope })
}

async fn refresh(http: &reqwest::Client, policy: &Policy, tokens: &Tokens, refresh_token: &str) -> Result<Tokens, Refused> {
    let endpoint = policy.check(&tokens.token_endpoint).map_err(Refused::Other)?;
    let form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("resource", tokens.resource.clone()),
    ];
    let answer = token_request(http, &endpoint, &tokens.creds(), form).await?;
    Ok(Tokens {
        access_token: answer.access_token,
        // Servers that don't rotate refresh tokens leave it out: keep ours.
        refresh_token: answer.refresh_token.or_else(|| tokens.refresh_token.clone()),
        expires_at: answer.expires_in.map(|s| now_secs().saturating_add(s)).unwrap_or(0),
        scope: answer.scope.or_else(|| tokens.scope.clone()),
        ..tokens.clone()
    })
}

// ── WWW-Authenticate ──────────────────────────────────────────────────────────

/// The auth-params of the Bearer challenge (RFC 9110 §11.2): `key=token` or
/// `key="quoted \" string"`, comma-separated. Keys lowercased.
fn bearer_params(header: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let lower = header.to_ascii_lowercase();
    let Some(start) = lower.find("bearer") else { return out };
    let rest = &header[start + "bearer".len()..];
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    loop {
        while i < chars.len() && (chars[i] == ',' || chars[i].is_whitespace()) {
            i += 1;
        }
        let key_start = i;
        while i < chars.len() && (chars[i].is_ascii_alphanumeric() || "_-.".contains(chars[i])) {
            i += 1;
        }
        if i == key_start {
            break;
        }
        let key: String = chars[key_start..i].iter().collect::<String>().to_ascii_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            // A bare word: the next challenge's scheme. Stop there.
            break;
        }
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < chars.len() && chars[i] == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                value.push(chars[i]);
                i += 1;
            }
            i += 1;
        } else {
            while i < chars.len() && chars[i] != ',' && !chars[i].is_whitespace() {
                value.push(chars[i]);
                i += 1;
            }
        }
        out.entry(key).or_insert(value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn www_authenticate_params() {
        let gh = r#"Bearer error="invalid_request", error_description="No access token was provided in this request", resource_metadata="https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/""#;
        let p = bearer_params(gh);
        assert_eq!(p["resource_metadata"], "https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/");
        assert_eq!(p["error"], "invalid_request");
        let linear = r#"Bearer realm="OAuth", resource_metadata="https://mcp.linear.app/.well-known/oauth-protected-resource/mcp", scope="read write""#;
        assert_eq!(bearer_params(linear)["scope"], "read write");
        // No space after the commas, and a parameter outside the RFC.
        let figma = r#"Bearer resource_metadata="https://mcp.figma.com/.well-known/oauth-protected-resource",scope="mcp:connect",authorization_uri="https://api.figma.com/x""#;
        let p = bearer_params(figma);
        assert_eq!(p["scope"], "mcp:connect");
        assert_eq!(p["authorization_uri"], "https://api.figma.com/x");
        let escaped = r#"bearer Realm=plain, error_description="say \"hi\", ok", scope=a"#;
        let p = bearer_params(escaped);
        assert_eq!(p["realm"], "plain");
        assert_eq!(p["error_description"], r#"say "hi", ok"#);
        assert_eq!(p["scope"], "a");
        assert!(bearer_params(r#"Basic realm="x""#).is_empty());
        let two = r#"Bearer realm="a", Basic realm="b""#;
        assert_eq!(bearer_params(two)["realm"], "a");
    }

    #[test]
    fn unauthorized_hints_are_kept_and_consumed() {
        on_unauthorized("test-hint-1", Some(r#"Bearer resource_metadata="https://x.example/.well-known/oauth-protected-resource", scope="a b""#));
        let hint = lock(&HINTS).get("test-hint-1").cloned().unwrap();
        assert_eq!(hint.resource_metadata.as_deref(), Some("https://x.example/.well-known/oauth-protected-resource"));
        assert_eq!(hint.scope.as_deref(), Some("a b"));
        assert!(take_stale("test-hint-1"));
        assert!(!take_stale("test-hint-1"), "stale is used once");
        on_unauthorized("test-hint-1", Some(r#"Bearer resource_metadata="not a url", scope="bad\"""#));
        let hint = lock(&HINTS).get("test-hint-1").cloned().unwrap();
        assert_eq!(hint.resource_metadata.as_deref(), Some("https://x.example/.well-known/oauth-protected-resource"), "junk doesn't replace a good hint");
        forget("test-hint-1");
        assert!(lock(&HINTS).get("test-hint-1").is_none());
    }

    #[test]
    fn well_known_paths() {
        let s = |u: &str| Url::parse(u).unwrap();
        let names = |v: Vec<Url>| v.into_iter().map(|u| u.to_string()).collect::<Vec<_>>();
        assert_eq!(
            names(prm_candidates(&s("https://mcp.notion.com/mcp?x=1"))),
            ["https://mcp.notion.com/.well-known/oauth-protected-resource/mcp", "https://mcp.notion.com/.well-known/oauth-protected-resource"]
        );
        assert_eq!(names(prm_candidates(&s("https://api.githubcopilot.com/mcp/")))[0], "https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp");
        assert_eq!(names(prm_candidates(&s("https://mcp.stripe.com"))), ["https://mcp.stripe.com/.well-known/oauth-protected-resource"]);
        assert_eq!(
            names(as_candidates(&s("https://access.stripe.com/mcp"))),
            [
                "https://access.stripe.com/.well-known/oauth-authorization-server/mcp",
                "https://access.stripe.com/.well-known/openid-configuration/mcp",
                "https://access.stripe.com/mcp/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            names(as_candidates(&s("https://mcp.linear.app"))),
            ["https://mcp.linear.app/.well-known/oauth-authorization-server", "https://mcp.linear.app/.well-known/openid-configuration"]
        );
    }

    #[test]
    fn resource_must_cover_the_server() {
        let s = |u: &str| Url::parse(u).unwrap();
        assert!(resource_covers("https://mcp.notion.com/mcp", &s("https://mcp.notion.com/mcp")));
        assert!(resource_covers("https://mcp.stripe.com", &s("https://mcp.stripe.com/")));
        assert!(resource_covers("https://api.githubcopilot.com/mcp/", &s("https://api.githubcopilot.com/mcp/")));
        assert!(resource_covers("https://mcp.example.com/v2", &s("https://mcp.example.com/v2/mcp")));
        assert!(!resource_covers("https://mcp.example.com/v2", &s("https://mcp.example.com/v20/mcp")), "segment-wise");
        assert!(!resource_covers("https://mcp.example.com/other", &s("https://mcp.example.com/mcp")));
        assert!(!resource_covers("https://evil.example.com/mcp", &s("https://mcp.example.com/mcp")));
        assert!(!resource_covers("http://mcp.example.com/mcp", &s("https://mcp.example.com/mcp")));
        assert!(!resource_covers("https://mcp.example.com:8443/mcp", &s("https://mcp.example.com/mcp")));
        assert!(!resource_covers("not a url", &s("https://mcp.example.com/mcp")));
    }

    #[test]
    fn endpoints_must_be_https_unless_the_server_is_local() {
        let remote = Policy::for_server(&Url::parse("https://mcp.example.com/mcp").unwrap());
        assert!(remote.check("https://auth.example.com/token").is_ok());
        assert!(remote.check("http://auth.example.com/token").unwrap_err().starts_with(errors::MCPC_OAUTH_INSECURE));
        assert!(remote.check("http://127.0.0.1:8080/token").is_err(), "a remote server can't send us to a local port");
        assert!(remote.check("https://user:pw@auth.example.com/token").is_err());
        assert!(remote.check("javascript:alert(1)").is_err());
        let local = Policy::for_server(&Url::parse("http://127.0.0.1:3000/mcp").unwrap());
        assert!(local.check("http://localhost:3001/token").is_ok());
        assert!(local.check("http://auth.example.com/token").is_err());
    }

    #[test]
    fn pkce_is_base64url_sha256() {
        // Expected values from Python: urlsafe_b64encode(sha256(v)) without "=".
        assert_eq!(pkce_challenge("abc"), "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0");
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mA4uZQ_mo8Dq4dShJpUwPEZo8aSn1M"), "bM2NBGNxmt-mCjnSPNFHuuf-YSWwwP5ANMt6t4PiI9w");
        let a = random_token();
        assert_eq!(a.len(), 43);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{a}");
        assert_ne!(a, random_token());
    }

    #[test]
    fn client_auth_choice() {
        let v = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_client_auth(None), Some(ClientAuth::None));
        assert_eq!(pick_client_auth(Some(&v(&["client_secret_basic", "none"]))), Some(ClientAuth::None));
        assert_eq!(pick_client_auth(Some(&v(&["client_secret_post", "client_secret_basic"]))), Some(ClientAuth::ClientSecretBasic));
        assert_eq!(pick_client_auth(Some(&v(&["client_secret_post"]))), Some(ClientAuth::ClientSecretPost));
        assert_eq!(pick_client_auth(Some(&v(&["private_key_jwt"]))), None);
        assert_eq!(form_encode("a b+c:d"), "a%20b%2Bc%3Ad");
    }

    #[test]
    fn token_answers() {
        let ok = parse_token_answer(&serde_json::json!({ "access_token": "at", "token_type": "bearer", "expires_in": "3600", "refresh_token": "rt", "scope": "read" })).unwrap();
        assert_eq!((ok.access_token.as_str(), ok.expires_in, ok.refresh_token.as_deref(), ok.scope.as_deref()), ("at", Some(3600), Some("rt"), Some("read")));
        assert_eq!(parse_token_answer(&serde_json::json!({ "access_token": "at", "expires_in": 59.5 })).unwrap().expires_in, Some(59));
        assert!(parse_token_answer(&serde_json::json!({ "access_token": "at", "token_type": "DPoP" })).is_err());
        assert!(parse_token_answer(&serde_json::json!({ "access_token": "" })).is_err());
        assert!(parse_token_answer(&serde_json::json!({ "access_token": "a\r\nInjected: x" })).is_err());
        assert!(parse_token_answer(&serde_json::json!({ "error": "x" })).is_err());
    }

    #[test]
    fn freshness_refreshes_a_minute_early() {
        let mut t = Tokens {
            client_id: "c".into(),
            client_secret: None,
            client_auth: ClientAuth::None,
            access_token: "a".into(),
            refresh_token: None,
            expires_at: 0,
            scope: None,
            token_endpoint: "https://x/token".into(),
            issuer: "https://x".into(),
            resource: "https://x/mcp".into(),
        };
        assert!(t.fresh(1_000), "no expiry known: used until refused");
        t.expires_at = 1_000;
        assert!(t.fresh(900));
        assert!(!t.fresh(940));
        assert!(!t.fresh(2_000));
        // The stored form is the contract's snake_case JSON.
        let json = serde_json::to_value(&t).unwrap();
        for key in ["client_id", "access_token", "expires_at", "token_endpoint", "issuer", "resource", "client_auth"] {
            assert!(json.get(key).is_some(), "{key}");
        }
        assert!(json.get("client_secret").is_none() && json.get("refresh_token").is_none());
    }

    #[test]
    fn callback_checks_state_first() {
        let q = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        assert_eq!(callback_outcome(&q(&[("code", "c1"), ("state", "s")]), "s"), Ok("c1".into()));
        assert!(callback_outcome(&q(&[("code", "c1"), ("state", "other")]), "s").unwrap_err().starts_with(errors::MCPC_OAUTH_STATE));
        assert!(callback_outcome(&q(&[("code", "c1")]), "s").unwrap_err().starts_with(errors::MCPC_OAUTH_STATE));
        assert!(callback_outcome(&q(&[("error", "access_denied"), ("state", "other")]), "s").unwrap_err().starts_with(errors::MCPC_OAUTH_STATE));
        let denied = callback_outcome(&q(&[("error", "access_denied"), ("error_description", "nope\u{202E}"), ("state", "s")]), "s").unwrap_err();
        assert_eq!(denied, "E_MCPC_OAUTH_DENIED|access_denied: nope");
        assert!(callback_outcome(&q(&[("state", "s")]), "s").unwrap_err().starts_with(errors::MCPC_OAUTH_DENIED));
    }

    // ── End to end against a local fake server ───────────────────────────────

    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct Fake {
        challenge: Mutex<Option<String>>,
        registered: Mutex<Option<Value>>,
        token_forms: Mutex<Vec<HashMap<String, String>>>,
        /// Leave registration out of the metadata.
        no_dcr: AtomicBool,
        /// The PRM names a resource the server URL isn't under.
        wrong_resource: AtomicBool,
    }

    struct Req {
        method: String,
        path: String,
        body: String,
    }

    async fn read_full(socket: &mut TcpStream) -> Option<Req> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let length = head
            .lines()
            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
            .unwrap_or(0);
        while buf.len() < head_end + length {
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let mut first = head.lines().next()?.split(' ');
        Some(Req { method: first.next()?.into(), path: first.next()?.into(), body: String::from_utf8_lossy(&buf[head_end..]).to_string() })
    }

    async fn reply(socket: &mut TcpStream, status: &str, extra: &str, body: &str) {
        let text = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}", body.len());
        let _ = socket.write_all(text.as_bytes()).await;
        let _ = socket.shutdown().await;
    }

    async fn fake_server(fake: Arc<Fake>) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let b = base.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let fake = fake.clone();
                let base = b.clone();
                tokio::spawn(async move {
                    let Some(req) = read_full(&mut socket).await else { return };
                    match (req.method.as_str(), req.path.as_str()) {
                        ("POST", "/mcp") => {
                            let header = format!("WWW-Authenticate: Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\", scope=\"read\"\r\n");
                            reply(&mut socket, "401 Unauthorized", &header, "{}").await;
                        }
                        ("GET", "/.well-known/oauth-protected-resource/mcp") => {
                            let resource = if fake.wrong_resource.load(Ordering::SeqCst) { format!("{base}/elsewhere") } else { format!("{base}/mcp") };
                            let doc = serde_json::json!({ "resource": resource, "authorization_servers": [format!("{base}/auth")], "scopes_supported": ["read", "write"] });
                            reply(&mut socket, "200 OK", "", &doc.to_string()).await;
                        }
                        ("GET", "/.well-known/oauth-authorization-server/auth") => {
                            let mut doc = serde_json::json!({
                                "issuer": format!("{base}/auth"),
                                "authorization_endpoint": format!("{base}/auth/authorize"),
                                "token_endpoint": format!("{base}/auth/token"),
                                "code_challenge_methods_supported": ["S256"],
                                "token_endpoint_auth_methods_supported": ["none"],
                            });
                            if !fake.no_dcr.load(Ordering::SeqCst) {
                                doc["registration_endpoint"] = format!("{base}/auth/register").into();
                            }
                            reply(&mut socket, "200 OK", "", &doc.to_string()).await;
                        }
                        ("POST", "/auth/register") => {
                            *fake.registered.lock().unwrap() = serde_json::from_str(&req.body).ok();
                            reply(&mut socket, "201 Created", "", r#"{"client_id":"client-1"}"#).await;
                        }
                        ("POST", "/auth/token") => {
                            let form: HashMap<String, String> = Url::parse(&format!("http://x/?{}", req.body)).unwrap().query_pairs().into_owned().collect();
                            fake.token_forms.lock().unwrap().push(form.clone());
                            let challenge = fake.challenge.lock().unwrap().clone();
                            let ok = match form.get("grant_type").map(String::as_str) {
                                Some("authorization_code") => {
                                    form.get("code").map(String::as_str) == Some("code-1")
                                        && form.get("code_verifier").map(|v| pkce_challenge(v)) == challenge
                                        && form.get("client_id").map(String::as_str) == Some("client-1")
                                }
                                Some("refresh_token") => form.get("refresh_token").map(String::as_str) == Some("refresh-1"),
                                _ => false,
                            };
                            if ok {
                                let body = r#"{"access_token":"access-1","token_type":"Bearer","expires_in":3600,"refresh_token":"refresh-1"}"#;
                                reply(&mut socket, "200 OK", "", body).await;
                            } else {
                                reply(&mut socket, "400 Bad Request", "", r#"{"error":"invalid_grant"}"#).await;
                            }
                        }
                        _ => reply(&mut socket, "404 Not Found", "", "{}").await,
                    }
                });
            }
        });
        base
    }

    /// Plays the browser: reads the authorization URL, records the PKCE
    /// challenge, and comes back to the loopback listener like a redirect.
    fn browser(fake: Arc<Fake>, state_override: Option<&'static str>) -> impl FnOnce(&str) {
        move |auth_url: &str| {
            let url = Url::parse(auth_url).unwrap();
            let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
            *fake.challenge.lock().unwrap() = q.get("code_challenge").cloned();
            assert_eq!(q["code_challenge_method"], "S256");
            assert_eq!(q["response_type"], "code");
            let redirect = q["redirect_uri"].clone();
            let state = state_override.map(str::to_string).unwrap_or_else(|| q["state"].clone());
            tokio::spawn(async move {
                // A speculative connection that never sends anything must not
                // block the real one.
                let _idle = TcpStream::connect(Url::parse(&redirect).unwrap().socket_addrs(|| None).unwrap()[0]).await;
                let client = reqwest::Client::new();
                let _ = client.get(format!("{redirect}?code=code-1&state={state}")).send().await;
            });
        }
    }

    #[tokio::test]
    async fn signs_in_and_refreshes_against_a_local_server() {
        let fake = Arc::new(Fake::default());
        let base = fake_server(fake.clone()).await;
        let (_keep, cancel) = oneshot::channel();
        let server_url = format!("{base}/mcp");
        let tokens = sign_in(&server_url, &Hint::default(), browser(fake.clone(), None), cancel, Duration::from_secs(10)).await.unwrap();
        assert_eq!(tokens.access_token, "access-1");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(tokens.client_id, "client-1");
        assert_eq!(tokens.resource, server_url);
        assert_eq!(tokens.issuer, format!("{base}/auth"));
        assert_eq!(tokens.scope.as_deref(), Some("read"), "the 401's scope wins over scopes_supported");
        assert!(tokens.expires_at > now_secs() + 3000);

        let registered = fake.registered.lock().unwrap().clone().unwrap();
        assert_eq!(registered["client_name"], "Roadeep Desktop");
        assert_eq!(registered["token_endpoint_auth_method"], "none");
        assert_eq!(registered["grant_types"], serde_json::json!(["authorization_code", "refresh_token"]));
        assert!(registered["redirect_uris"][0].as_str().unwrap().starts_with("http://127.0.0.1:"));
        let exchange = fake.token_forms.lock().unwrap()[0].clone();
        assert_eq!(exchange["resource"], server_url, "RFC 8707 resource on the token request");

        let policy = Policy::for_server(&Url::parse(&server_url).unwrap());
        let renewed = refresh(&http_client().unwrap(), &policy, &tokens, "refresh-1").await.unwrap();
        assert_eq!(renewed.access_token, "access-1");
        assert_eq!(renewed.client_id, "client-1");
        let dead = refresh(&http_client().unwrap(), &policy, &tokens, "revoked").await.unwrap_err();
        assert!(matches!(dead, Refused::Grant(ref d) if d.contains("invalid_grant")), "{dead:?}");
    }

    #[tokio::test]
    async fn a_forged_state_is_refused_and_the_real_callback_still_counts() {
        let fake = Arc::new(Fake::default());
        let base = fake_server(fake.clone()).await;
        let (_keep, cancel) = oneshot::channel();
        let statuses = Arc::new(Mutex::new(Vec::<u16>::new()));
        let seen = statuses.clone();
        let fake2 = fake.clone();
        let browse = move |auth_url: &str| {
            let url = Url::parse(auth_url).unwrap();
            let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
            *fake2.challenge.lock().unwrap() = q.get("code_challenge").cloned();
            let (redirect, state) = (q["redirect_uri"].clone(), q["state"].clone());
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                for query in ["code=evil&state=forged".to_string(), "code=evil".to_string(), format!("code=code-1&state={state}")] {
                    let status = client.get(format!("{redirect}?{query}")).send().await.unwrap().status().as_u16();
                    seen.lock().unwrap().push(status);
                }
            });
        };
        let tokens = sign_in(&format!("{base}/mcp"), &Hint::default(), browse, cancel, Duration::from_secs(10)).await.unwrap();
        assert_eq!(tokens.access_token, "access-1");
        let forms = fake.token_forms.lock().unwrap().clone();
        assert_eq!(forms.len(), 1, "only the real code is exchanged");
        assert_eq!(forms[0]["code"], "code-1");
        // The third answer may still be on its way when sign_in returns.
        for _ in 0..50 {
            if statuses.lock().unwrap().len() == 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(*statuses.lock().unwrap(), vec![400, 400, 200]);
    }

    #[test]
    fn a_refresh_racing_a_sign_out_does_not_write_back() {
        let id = format!("epoch-{}", uuid::Uuid::new_v4().simple());
        let writes = Mutex::new(0);
        let write = || *writes.lock().unwrap() += 1;

        // Nothing happened meanwhile: the refresh stores its tokens.
        let epoch = epoch_of(&id);
        assert_eq!(commit_if_current(&id, epoch, || true, write), Some(()));

        // Signed out (or removed, or edited) while the refresh was out.
        let epoch = epoch_of(&id);
        let cleared = invalidate(&id, || "cleared");
        assert_eq!(cleared, "cleared");
        assert_eq!(commit_if_current(&id, epoch, || true, write), None);

        // Same epoch, but the slot no longer holds the refresh token used.
        let epoch = epoch_of(&id);
        assert_eq!(commit_if_current(&id, epoch, || false, write), None);
        assert_eq!(*writes.lock().unwrap(), 1);

        // forget() never resets the epoch an old refresh may hold.
        let epoch = epoch_of(&id);
        invalidate(&id, || ());
        forget(&id);
        assert_ne!(epoch_of(&id), epoch);
    }

    #[tokio::test]
    async fn only_forged_callbacks_end_in_a_timeout() {
        let fake = Arc::new(Fake::default());
        let base = fake_server(fake.clone()).await;
        let (_keep, cancel) = oneshot::channel();
        let err = sign_in(&format!("{base}/mcp"), &Hint::default(), browser(fake.clone(), Some("forged")), cancel, Duration::from_millis(800)).await.unwrap_err();
        assert_eq!(err, "E_MCPC_OAUTH_TIMEOUT");
        assert!(fake.token_forms.lock().unwrap().is_empty(), "no code exchange");
    }

    #[tokio::test]
    async fn no_registration_means_use_a_token() {
        let fake = Arc::new(Fake::default());
        fake.no_dcr.store(true, Ordering::SeqCst);
        let base = fake_server(fake.clone()).await;
        let (_keep, cancel) = oneshot::channel();
        let err = sign_in(&format!("{base}/mcp"), &Hint::default(), |_: &str| panic!("no browser"), cancel, Duration::from_secs(10)).await.unwrap_err();
        assert_eq!(err, "E_MCPC_OAUTH_NO_DCR");
    }

    #[tokio::test]
    async fn a_resource_that_does_not_match_is_refused() {
        let fake = Arc::new(Fake::default());
        fake.wrong_resource.store(true, Ordering::SeqCst);
        let base = fake_server(fake.clone()).await;
        let (_keep, cancel) = oneshot::channel();
        let err = sign_in(&format!("{base}/mcp"), &Hint::default(), |_: &str| panic!("no browser"), cancel, Duration::from_secs(10)).await.unwrap_err();
        assert!(err.starts_with(errors::MCPC_OAUTH_DISCOVERY), "{err}");
    }

    #[tokio::test]
    async fn cancel_and_timeout_end_the_wait() {
        let fake = Arc::new(Fake::default());
        let base = fake_server(fake.clone()).await;
        let (tx, cancel) = oneshot::channel();
        let err = sign_in(&format!("{base}/mcp"), &Hint::default(), move |_: &str| {
            let _ = tx.send(());
        }, cancel, Duration::from_secs(10)).await.unwrap_err();
        assert_eq!(err, "E_MCPC_OAUTH_CANCELLED");
        let (_keep, cancel) = oneshot::channel();
        let err = sign_in(&format!("{base}/mcp"), &Hint::default(), |_: &str| {}, cancel, Duration::from_millis(300)).await.unwrap_err();
        assert_eq!(err, "E_MCPC_OAUTH_TIMEOUT");
    }

    #[tokio::test]
    async fn a_remote_server_must_be_https() {
        let (_keep, cancel) = oneshot::channel();
        let err = sign_in("http://mcp.example.com/mcp", &Hint::default(), |_: &str| {}, cancel, Duration::from_secs(1)).await.unwrap_err();
        assert!(err.starts_with(errors::MCPC_OAUTH_INSECURE), "{err}");
    }
}
