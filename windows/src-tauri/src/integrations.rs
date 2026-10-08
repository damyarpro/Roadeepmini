// Integration pollers — the Rust side of StripePoller / GithubPoller /
// VercelPoller / N8nPoller / ResendPoller / NotionPoller / CalcomPoller, plus the
// Windows-only GitLab, Sentry, Linear, Netlify and Cloudflare pollers.
//
// Same endpoints, same first-run delays and intervals as the Swift pollers. Each
// one emits an `integration` event; the island owns the badge, the sound and the
// 60 s auto-clear, exactly as the Swift handlers do.
//
// Every poll also ends in an outcome (ok, or a coded error) kept per service, so
// the settings window can show "updated 2 min ago" and run a connection test.
//
// Nothing is polled until its key exists in the Credential Manager, and no
// request goes anywhere the user has not configured. Every call is read-only.
//
// Catalog services (catalog/) share the outcome table, the island event and
// `poll_once`; their requests are made by catalog::engine.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::errors;
use crate::island::WINDOW_LABEL;
use crate::log;
use crate::secrets;

const TIMEOUT: Duration = Duration::from_secs(10);

/// The settings window's label (lib.rs builds it).
const SETTINGS_WINDOW: &str = "settings";

/// `Err` holds an `errors` code, the same text the island shows.
pub type PollResult = Result<(), String>;

/// Every native service: its pill id and the Credential Manager keys it reads.
pub(crate) const SERVICES: &[(&str, &[&str])] = &[
    ("integration_stripe", &["stripe-api-key"]),
    ("integration_github", &["github-token"]),
    ("integration_vercel", &["vercel-token"]),
    ("integration_n8n", &["n8n-url", "n8n-api-key"]),
    ("integration_resend", &["resend-api-key"]),
    ("integration_notion", &["notion-api-key"]),
    ("integration_calcom", &["calcom-api-key"]),
    ("integration_gitlab", &["gitlab-token", "gitlab-url"]),
    ("integration_sentry", &["sentry-token", "sentry-org", "sentry-url"]),
    ("integration_linear", &["linear-api-key"]),
    ("integration_netlify", &["netlify-token"]),
    ("integration_cloudflare", &["cloudflare-token", "cloudflare-account-id"]),
];

/// What the island receives. `event` is only set when something actually changed,
/// which is what drives the pill badge and the sound.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationUpdate {
    pub id: &'static str,
    pub data: Value,
    pub error: Option<String>,
    pub event: Option<IntegrationEvent>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationEvent {
    pub success: bool,
    pub label: String,
    pub detail: Option<String>,
}

/// The native services' ids without the "integration_" prefix: reserved, so no
/// catalog manifest can take one.
pub fn native_ids() -> Vec<&'static str> {
    SERVICES.iter().filter_map(|(pill, _)| pill.strip_prefix("integration_")).collect()
}

pub(crate) fn emit(app: &AppHandle, update: IntegrationUpdate) {
    let _ = app.emit_to(WINDOW_LABEL, "integration", update);
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_default()
}

/// Set from the tray's Pause item. While it is on, nothing reaches the network:
/// pausing Roadeep has to mean pausing Roadeep, not just hiding the island.
pub static PAUSED: AtomicBool = AtomicBool::new(false);

pub fn set_paused(on: bool) {
    PAUSED.store(on, Ordering::Relaxed);
}

/// Spawns every poller with the macOS delays and intervals. The newer services
/// poll every minute or two: deploys and new issues are worth knowing quickly,
/// and each poll is a handful of small read-only requests.
pub fn start(app: AppHandle) {
    spawn(app.clone(), "integration_n8n", 3, 15);
    spawn(app.clone(), "integration_vercel", 5, 30);
    spawn(app.clone(), "integration_stripe", 6, 30);
    spawn(app.clone(), "integration_resend", 6, 60);
    spawn(app.clone(), "integration_github", 7, 300);
    spawn(app.clone(), "integration_calcom", 8, 300);
    spawn(app.clone(), "integration_notion", 9, 300);
    spawn(app.clone(), "integration_netlify", 10, 60);
    spawn(app.clone(), "integration_cloudflare", 11, 60);
    spawn(app.clone(), "integration_sentry", 12, 60);
    spawn(app.clone(), "integration_gitlab", 13, 120);
    spawn(app.clone(), "integration_linear", 14, 120);
    crate::catalog::engine::start(app);
}

/// True when the user has this integration switched on in settings.
fn enabled(app: &AppHandle, id: &str) -> bool {
    app.try_state::<crate::Shared>()
        .map(|shared| {
            let settings = shared.settings.lock().unwrap();
            settings.active_integrations.iter().any(|x| x == id)
        })
        .unwrap_or(false)
}

fn spawn(app: AppHandle, id: &'static str, delay_secs: u64, every_secs: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(every_secs));
        loop {
            ticker.tick().await;
            // The ticker keeps its cadence; we just decline to do the work. An
            // integration the user switched off, or a paused app, must make no
            // network calls at all — CLAUDE.md allows talking only to services
            // the user configured, and a disabled one is not configured.
            if PAUSED.load(Ordering::Relaxed) || !enabled(&app, id) {
                continue;
            }
            // The outcome is recorded (and logged on change) inside poll_once.
            let _ = poll_once(app.clone(), id).await;
        }
    });
}

/// One poll of one service, from the timers, the island's Refresh buttons and
/// the settings window's connection test. The outcome is recorded either way.
pub async fn poll_once(app: AppHandle, id: &str) -> PollResult {
    let outcome = match id {
        "integration_stripe" => poll_stripe(app.clone()).await,
        "integration_github" => poll_github(app.clone()).await,
        "integration_vercel" => poll_vercel(app.clone()).await,
        "integration_n8n" => poll_n8n(app.clone()).await,
        "integration_resend" => poll_resend(app.clone()).await,
        "integration_notion" => poll_notion(app.clone()).await,
        "integration_calcom" => poll_calcom(app.clone()).await,
        "integration_gitlab" => poll_gitlab(app.clone()).await,
        "integration_sentry" => poll_sentry(app.clone()).await,
        "integration_linear" => poll_linear(app.clone()).await,
        "integration_netlify" => poll_netlify(app.clone()).await,
        "integration_cloudflare" => poll_cloudflare(app.clone()).await,
        _ => match crate::catalog::get().by_pill(id) {
            Some(service) => crate::catalog::engine::poll(&app, service).await,
            None => return Err(errors::coded(errors::INT_UNKNOWN, &[id])),
        },
    };
    record(&app, id, &outcome);
    outcome
}

/// "Test connection" in the settings window: one poll now. Refused while the
/// app is paused, since a pause means no network calls at all.
pub async fn test(app: AppHandle, id: &str) -> Result<PollStatus, String> {
    if PAUSED.load(Ordering::Relaxed) {
        return Err(errors::coded(errors::INT_PAUSED, &[]));
    }
    let outcome = poll_once(app, id).await;
    log::line(format!(
        "{id}: connection test {}",
        match &outcome {
            Ok(()) => "ok",
            Err(e) => code_of(e),
        }
    ));
    outcome?;
    Ok(STATUS.lock().unwrap().get(id).cloned().unwrap_or_else(|| PollStatus::new(id)))
}

// ── Outcomes ──────────────────────────────────────────────────────────────────

/// What the settings window shows per service. `last_ok` is when the service
/// last answered properly (ms since the epoch); `error` the current failure.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PollStatus {
    pub id: String,
    pub last_ok: Option<i64>,
    pub error: Option<String>,
}

impl PollStatus {
    fn new(id: &str) -> Self {
        PollStatus { id: id.to_string(), last_ok: None, error: None }
    }
}

static STATUS: LazyLock<Mutex<HashMap<String, PollStatus>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The code alone: arguments can hold service text, which stays out of the log.
fn code_of(coded: &str) -> &str {
    coded.split(['|', '\n']).next().unwrap_or("")
}

/// Updates the outcome table; returns the new status and whether the failure
/// changed (a new code, or ok again), which is what gets logged.
fn apply_outcome(map: &mut HashMap<String, PollStatus>, id: &str, outcome: &PollResult, at: i64) -> (PollStatus, bool) {
    let entry = map.entry(id.to_string()).or_insert_with(|| PollStatus::new(id));
    let before = entry.error.as_deref().map(code_of).map(str::to_string);
    match outcome {
        Ok(()) => {
            entry.last_ok = Some(at);
            entry.error = None;
        }
        Err(e) => entry.error = Some(e.clone()),
    }
    let changed = before.as_deref() != entry.error.as_deref().map(code_of);
    (entry.clone(), changed)
}

fn record(app: &AppHandle, id: &str, outcome: &PollResult) {
    let (status, changed) = apply_outcome(&mut STATUS.lock().unwrap(), id, outcome, now_ms());
    if changed {
        match &status.error {
            Some(e) => log::line(format!("{id}: poll failed ({})", code_of(e))),
            None => log::line(format!("{id}: poll ok again")),
        }
    }
    let _ = app.emit_to(SETTINGS_WINDOW, "integration-status", status);
}

/// Every recorded outcome, for the settings window when it opens.
pub fn statuses() -> Vec<PollStatus> {
    STATUS.lock().unwrap().values().cloned().collect()
}

/// A key was saved or removed: the old outcome described the old key.
pub fn key_changed(key: &str) {
    let mut ids: Vec<&str> = SERVICES.iter().filter(|(_, keys)| keys.contains(&key)).map(|(id, _)| *id).collect();
    if let Some((service, _)) = crate::catalog::get().field_for_key(key) {
        ids.push(&service.pill);
    }
    STATUS.lock().unwrap().retain(|id, _| !ids.contains(&id.as_str()));
}

// ── Shared request helpers ────────────────────────────────────────────────────

/// Sends the island an error for the card, and hands the same code back.
pub(crate) fn fail(app: &AppHandle, id: &'static str, error: String) -> String {
    emit(app, IntegrationUpdate { id, data: json!({}), error: Some(error.clone()), event: None });
    error
}

fn no_key() -> String {
    errors::coded(errors::INT_NO_KEY, &[])
}

/// Without the URL: a self-hosted base URL can carry credentials.
pub(crate) fn no_connection(e: reqwest::Error) -> String {
    errors::coded(errors::INT_NO_CONNECTION, &[&e.without_url().to_string()])
}

enum Fetch {
    Status(u16),
    Net(String),
}

/// GET/POST and read the JSON body (lenient, like the older pollers: an
/// unreadable body is `Null`). The headers come back for counts like X-Hits.
async fn fetch(request: reqwest::RequestBuilder) -> Result<(reqwest::header::HeaderMap, Value), Fetch> {
    let response = request.send().await.map_err(|e| Fetch::Net(no_connection(e)))?;
    if !response.status().is_success() {
        return Err(Fetch::Status(response.status().as_u16()));
    }
    let headers = response.headers().clone();
    Ok((headers, response.json::<Value>().await.unwrap_or(Value::Null)))
}

/// A failed fetch as a code: HTTP failures go to the card too; a network blip
/// only to the caller, so the island doesn't flash an error for it.
fn fetch_error(app: &AppHandle, id: &'static str, forbidden: &str, failure: Fetch) -> String {
    match failure {
        Fetch::Status(code) => fail(app, id, status_error(code, forbidden)),
        Fetch::Net(error) => error,
    }
}

/// A user-entered base URL, without its trailing slash. https only — except to
/// this machine — so a token never crosses the network in clear; no
/// credentials, query or fragment, since paths are appended to it.
pub(crate) fn base_url(raw: &str) -> Result<String, String> {
    let bad = || errors::coded(errors::INT_BAD_URL, &[]);
    let trimmed = raw.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(trimmed).map_err(|_| bad())?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && local);
    let clean = url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none();
    if !scheme_ok || !clean || url.host_str().is_none_or(str::is_empty) {
        return Err(bad());
    }
    Ok(trimmed.to_string())
}

/// An optional base URL setting, or the service's public default.
fn base_or(key: &str, default: &str) -> Result<String, String> {
    match secrets::get(key) {
        Some(raw) => base_url(&raw),
        None => Ok(default.to_string()),
    }
}

/// Identifiers that end up in a URL path (an org slug, an account id, a site id).
fn path_safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') && s != "." && s != ".."
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Ids come as numbers from some APIs and strings from others.
fn id_of(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Newest first by an ISO-8601 field: every API here writes UTC timestamps in
/// one fixed format, so the strings sort like the instants.
fn sort_newest(list: &mut [Value], key: &str) {
    list.sort_by(|a, b| str_of(b, key).unwrap_or("").cmp(str_of(a, key).unwrap_or("")));
}

/// The newest finished item fires an event once, like Vercel's deployments.
fn finished_event(seen_key: &'static str, list: &[Value], success: &str, label_key: &str, detail_key: &str) -> Option<IntegrationEvent> {
    let latest = list.iter().find(|item| matches!(str_of(item, "state"), Some("ready" | "error" | "canceled")))?;
    let id = str_of(latest, "id")?;
    if !is_new(seen_key, id) {
        return None;
    }
    Some(IntegrationEvent {
        success: str_of(latest, "state") == Some(success),
        label: str_of(latest, label_key)?.to_string(),
        detail: str_of(latest, detail_key).filter(|s| !s.is_empty()).map(str::to_string),
    })
}

/// Remembers the newest id per integration so an event fires once, not on every poll.
struct Seen(Mutex<std::collections::HashMap<&'static str, String>>);

static SEEN: std::sync::LazyLock<Seen> =
    std::sync::LazyLock::new(|| Seen(Mutex::new(std::collections::HashMap::new())));

/// Returns true the first time a given id is seen (and false on the very first
/// load, which only fills the card).
fn is_new(key: &'static str, id: &str) -> bool {
    let mut map = SEEN.0.lock().unwrap();
    match map.insert(key, id.to_string()) {
        Some(previous) => previous != id,
        None => false, // first poll: populate silently, like the Swift pollers
    }
}

/// An `errors` code; `forbidden` is the service's own reason for a 403.
fn status_error(code: u16, forbidden: &str) -> String {
    match code {
        401 => errors::coded(errors::INT_INVALID_KEY, &[]),
        403 => errors::coded(forbidden, &[]),
        _ => errors::coded(errors::INT_HTTP, &[&code.to_string()]),
    }
}

// ── Stripe ────────────────────────────────────────────────────────────────────

async fn poll_stripe(app: AppHandle) -> PollResult {
    let Some(key) = secrets::get("stripe-api-key") else { return Err(no_key()) };
    let auth = format!("Basic {}", crate::util::base64_for(format!("{key}:").as_bytes()));
    let http = client();

    let balance = http
        .get("https://api.stripe.com/v1/balance")
        .header("Authorization", &auth)
        .send()
        .await;

    let (amount, currency) = match balance {
        Ok(r) if r.status().is_success() => {
            let json: Value = r.json().await.unwrap_or(json!({}));
            let mut buckets: Vec<Value> = Vec::new();
            for k in ["available", "pending"] {
                if let Some(arr) = json.get(k).and_then(Value::as_array) {
                    buckets.extend(arr.iter().cloned());
                }
            }
            let currency = buckets
                .first()
                .and_then(|b| b.get("currency"))
                .and_then(Value::as_str)
                .unwrap_or("eur")
                .to_string();
            let amount: i64 = buckets
                .iter()
                .filter_map(|b| b.get("amount").and_then(Value::as_i64))
                .sum();
            (amount, currency)
        }
        Ok(r) => {
            let code = r.status().as_u16();
            return Err(fail(&app, "integration_stripe", status_error(code, errors::INT_STRIPE_SECRET_KEY)));
        }
        Err(e) => return Err(fail(&app, "integration_stripe", no_connection(e))),
    };

    let charges = http
        .get("https://api.stripe.com/v1/charges?limit=3")
        .header("Authorization", &auth)
        .send()
        .await;
    let response = charges.map_err(no_connection)?;
    if !response.status().is_success() {
        return Err(status_error(response.status().as_u16(), errors::INT_STRIPE_SECRET_KEY));
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let payments: Vec<Value> = json
        .get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    let description = c
                        .get("description")
                        .and_then(Value::as_str)
                        .or_else(|| {
                            c.get("billing_details")
                                .and_then(|b| b.get("name"))
                                .and_then(Value::as_str)
                        })
                        .map(str::to_string);
                    Some(json!({
                        "id": c.get("id")?.as_str()?,
                        "amount": c.get("amount")?.as_i64()?,
                        "currency": c.get("currency")?.as_str()?,
                        "description": description,
                        "createdAt": c.get("created").and_then(Value::as_i64).unwrap_or(0) * 1000,
                        "status": c.get("status").and_then(Value::as_str).unwrap_or("succeeded"),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let newest = payments
        .first()
        .and_then(|p| p.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let event = if !newest.is_empty() && is_new("stripe", &newest) {
        let label = payments[0]
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                let cents = payments[0].get("amount").and_then(Value::as_i64).unwrap_or(0);
                format!("{:.2}", cents as f64 / 100.0)
            });
        Some(IntegrationEvent { success: true, label, detail: None })
    } else {
        None
    };

    emit(&app, IntegrationUpdate {
        id: "integration_stripe",
        data: json!({ "balance": amount, "currency": currency, "payments": payments }),
        error: None,
        event,
    });
    Ok(())
}

// ── GitHub ────────────────────────────────────────────────────────────────────

async fn poll_github(app: AppHandle) -> PollResult {
    let Some(token) = secrets::get("github-token") else { return Err(no_key()) };
    let http = client();

    let user = http
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Roadeep")
        .send()
        .await;
    let response = user.map_err(no_connection)?;
    if !response.status().is_success() {
        return Err(fail(&app, "integration_github", status_error(response.status().as_u16(), errors::INT_TOKEN_SCOPE)));
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let public = json.get("public_repos").and_then(Value::as_i64).unwrap_or(0);
    let private = json
        .get("owned_private_repos")
        .or_else(|| json.get("total_private_repos"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let repos = http
        .get("https://api.github.com/user/repos?per_page=100&affiliation=owner&sort=pushed")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Roadeep")
        .send()
        .await;
    let stars: i64 = match repos {
        Ok(r) if r.status().is_success() => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .map(|list| {
                list.iter()
                    .filter_map(|r| r.get("stargazers_count").and_then(Value::as_i64))
                    .sum()
            })
            .unwrap_or(0),
        _ => 0,
    };

    emit(&app, IntegrationUpdate {
        id: "integration_github",
        data: json!({ "totalRepos": public + private, "totalStars": stars }),
        error: None,
        event: None,
    });
    Ok(())
}

// ── Vercel ────────────────────────────────────────────────────────────────────

async fn poll_vercel(app: AppHandle) -> PollResult {
    let Some(token) = secrets::get("vercel-token") else { return Err(no_key()) };
    let response = client()
        .get("https://api.vercel.com/v6/deployments?limit=5")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .await;
    let response = response.map_err(no_connection)?;
    if !response.status().is_success() {
        return Err(fail(&app, "integration_vercel", status_error(response.status().as_u16(), errors::INT_TOKEN_ACCESS)));
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let terminal = ["READY", "ERROR", "CANCELED"];
    let deployments: Vec<Value> = json
        .get("deployments")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|d| {
                    let state = d.get("state")?.as_str()?;
                    if !terminal.contains(&state) {
                        return None;
                    }
                    let meta = d.get("meta");
                    let pick = |keys: [&str; 3]| {
                        meta.and_then(|m| keys.iter().find_map(|k| m.get(*k).and_then(Value::as_str)))
                            .map(str::to_string)
                    };
                    Some(json!({
                        "id": d.get("uid")?.as_str()?,
                        "projectName": d.get("name")?.as_str()?,
                        "url": d.get("url").and_then(Value::as_str).unwrap_or(""),
                        "state": state,
                        "createdAt": d.get("createdAt").and_then(Value::as_f64).unwrap_or(0.0),
                        "commitMessage": pick(["githubCommitMessage", "gitlabCommitMessage", "bitbucketCommitMessage"]),
                        "branch": pick(["githubCommitRef", "gitlabCommitRef", "bitbucketBranch"]),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let event = deployments.first().and_then(|latest| {
        let id = latest.get("id")?.as_str()?;
        if !is_new("vercel", id) {
            return None;
        }
        let success = latest.get("state")?.as_str()? == "READY";
        Some(IntegrationEvent {
            success,
            label: latest.get("projectName")?.as_str()?.to_string(),
            detail: None,
        })
    });

    emit(&app, IntegrationUpdate {
        id: "integration_vercel",
        data: json!({ "deployments": deployments }),
        error: None,
        event,
    });
    Ok(())
}

// ── Resend ────────────────────────────────────────────────────────────────────

async fn poll_resend(app: AppHandle) -> PollResult {
    let Some(key) = secrets::get("resend-api-key") else { return Err(no_key()) };
    let response = client()
        .get("https://api.resend.com/emails?limit=100")
        .header("Authorization", format!("Bearer {key}"))
        .header("Accept", "application/json")
        .send()
        .await;
    let response = response.map_err(no_connection)?;
    if !response.status().is_success() {
        return Err(fail(&app, "integration_resend", status_error(response.status().as_u16(), errors::INT_KEY_ACCESS)));
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let total = json
        .get("total")
        .or_else(|| json.get("count"))
        .and_then(Value::as_i64);
    let emails: Vec<Value> = json
        .get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .take(5)
                .filter_map(|e| {
                    let to = match e.get("to") {
                        Some(Value::Array(a)) => a.clone(),
                        Some(Value::String(s)) => vec![Value::String(s.clone())],
                        _ => vec![],
                    };
                    Some(json!({
                        "id": e.get("id")?.as_str()?,
                        "to": to,
                        "subject": e.get("subject").and_then(Value::as_str).unwrap_or(""),
                        "createdAt": e.get("created_at").and_then(Value::as_str).unwrap_or(""),
                        "lastEvent": e.get("last_event").and_then(Value::as_str).unwrap_or(""),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    emit(&app, IntegrationUpdate {
        id: "integration_resend",
        data: json!({ "emails": emails, "total": total }),
        error: None,
        event: None,
    });
    Ok(())
}

// ── Notion ────────────────────────────────────────────────────────────────────

async fn poll_notion(app: AppHandle) -> PollResult {
    let Some(token) = secrets::get("notion-api-key") else { return Err(no_key()) };
    let response = client()
        .post("https://api.notion.com/v1/search")
        .header("Authorization", format!("Bearer {token}"))
        .header("Notion-Version", "2022-06-28")
        .header("Content-Type", "application/json")
        .json(&json!({
            "sort": { "direction": "descending", "timestamp": "last_edited_time" },
            "page_size": 3
        }))
        .send()
        .await;
    let response = response.map_err(no_connection)?;
    if !response.status().is_success() {
        return Err(fail(&app, "integration_notion", status_error(response.status().as_u16(), errors::INT_NOTION_ACCESS)));
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let pages: Vec<Value> = json
        .get("results")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_notion_page).collect())
        .unwrap_or_default();

    emit(&app, IntegrationUpdate {
        id: "integration_notion",
        data: json!({ "pages": pages }),
        error: None,
        event: None,
    });
    Ok(())
}

fn parse_notion_page(obj: &Value) -> Option<Value> {
    let id = obj.get("id")?.as_str()?;
    let is_database = obj.get("object").and_then(Value::as_str) == Some("database");

    let mut title = "Untitled".to_string();
    if is_database {
        if let Some(text) = obj
            .get("title")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|t| t.get("plain_text"))
            .and_then(Value::as_str)
        {
            if !text.is_empty() {
                title = text.to_string();
            }
        }
    } else if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        for prop in props.values() {
            if prop.get("type").and_then(Value::as_str) != Some("title") {
                continue;
            }
            if let Some(text) = prop
                .get("title")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|t| t.get("plain_text"))
                .and_then(Value::as_str)
            {
                if !text.is_empty() {
                    title = text.to_string();
                    break;
                }
            }
        }
    }

    let emoji = obj
        .get("icon")
        .filter(|i| i.get("type").and_then(Value::as_str) == Some("emoji"))
        .and_then(|i| i.get("emoji"))
        .and_then(Value::as_str);

    Some(json!({
        "id": id,
        "title": title,
        "emoji": emoji,
        "lastEditedAt": obj.get("last_edited_time").and_then(Value::as_str)?,
        "url": obj.get("url").and_then(Value::as_str).unwrap_or("https://notion.so"),
    }))
}

// ── Cal.com ───────────────────────────────────────────────────────────────────

async fn poll_calcom(app: AppHandle) -> PollResult {
    let Some(key) = secrets::get("calcom-api-key") else { return Err(no_key()) };
    let response = client()
        .get("https://api.cal.com/v2/bookings?status=upcoming")
        .header("Authorization", format!("Bearer {key}"))
        .header("cal-api-version", "2024-08-13")
        .send()
        .await;
    let response = response.map_err(no_connection)?;
    if !response.status().is_success() {
        return Err(fail(&app, "integration_calcom", status_error(response.status().as_u16(), errors::INT_KEY_ACCESS)));
    }
    let json: Value = response.json().await.unwrap_or(json!({}));
    let bookings: Vec<Value> = json
        .get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|b| {
                    let start = b
                        .get("start")
                        .or_else(|| b.get("startTime"))
                        .and_then(Value::as_str)?;
                    let attendee = b.get("attendees").and_then(Value::as_array).and_then(|a| a.first());
                    let notes = b
                        .get("responses")
                        .and_then(|r| r.get("notes"))
                        .and_then(|n| n.get("value"))
                        .and_then(Value::as_str)
                        .or_else(|| b.get("description").and_then(Value::as_str))
                        .filter(|s| !s.is_empty());
                    Some(json!({
                        "id": b.get("id").map(|v| v.to_string()).unwrap_or_default(),
                        "title": b.get("title").and_then(Value::as_str).unwrap_or("Meeting"),
                        "start": start,
                        "status": b.get("status").and_then(Value::as_str).unwrap_or("accepted"),
                        "attendeeName": attendee.and_then(|a| a.get("name")).and_then(Value::as_str),
                        "attendeeEmail": attendee.and_then(|a| a.get("email")).and_then(Value::as_str),
                        "attendeeNotes": notes,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    emit(&app, IntegrationUpdate {
        id: "integration_calcom",
        data: json!({ "bookings": bookings }),
        error: None,
        event: None,
    });
    Ok(())
}

// ── n8n ───────────────────────────────────────────────────────────────────────

async fn poll_n8n(app: AppHandle) -> PollResult {
    let (Some(key), Some(raw_base)) = (secrets::get("n8n-api-key"), secrets::get("n8n-url")) else {
        return Err(no_key());
    };
    let base = raw_base.trim_end_matches('/').to_string();
    let http = client();

    // Same two shapes as the Swift poller: the public API first, then /rest.
    let list_urls = [
        format!("{base}/api/v1/executions?limit=1&includeData=false"),
        format!("{base}/rest/executions?limit=1&includeData=false"),
    ];

    let mut items: Option<Vec<Value>> = None;
    // What the last attempt ran into, for when neither shape answers.
    let mut failure = errors::coded(errors::INT_HTTP, &["0"]);
    for url in &list_urls {
        let response = match http.get(url).header("X-N8N-API-KEY", &key).header("Accept", "application/json").send().await {
            Ok(response) => response,
            Err(e) => {
                failure = no_connection(e);
                continue;
            }
        };
        if !response.status().is_success() {
            // Only the status: a self-hosted base URL can carry credentials.
            log::line(format!("n8n list HTTP {}", response.status()));
            failure = status_error(response.status().as_u16(), errors::INT_KEY_ACCESS);
            continue;
        }
        let Ok(json) = response.json::<Value>().await else { continue };
        items = match &json {
            Value::Object(o) => o.get("data").and_then(Value::as_array).cloned(),
            Value::Array(a) => Some(a.clone()),
            _ => None,
        };
        if items.is_some() {
            break;
        }
    }

    let Some(items) = items else { return Err(failure) };
    // An instance that answered is a working connection, even with nothing new.
    let Some(first) = items.into_iter().next() else { return Ok(()) };
    let Some(id) = id_of(&first, "id") else { return Ok(()) };

    let status = first.get("status").and_then(Value::as_str).unwrap_or("");
    if !["success", "error", "crashed", "canceled", "failed"].contains(&status) {
        return Ok(());
    }
    if !is_new("n8n", &id) {
        return Ok(());
    }
    let success = status == "success";

    let detail_urls = [
        format!("{base}/api/v1/executions/{id}?includeData=true"),
        format!("{base}/api/v1/executions/{id}"),
        format!("{base}/rest/executions/{id}?includeData=true"),
        format!("{base}/rest/executions/{id}"),
    ];
    let mut name = "Workflow".to_string();
    let mut detail = None;
    for url in &detail_urls {
        let Ok(response) = http.get(url).header("X-N8N-API-KEY", &key).header("Accept", "application/json").send().await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(json) = response.json::<Value>().await else { continue };
        name = json
            .get("workflowData")
            .and_then(|w| w.get("name"))
            .and_then(Value::as_str)
            .or_else(|| json.get("name").and_then(Value::as_str))
            .unwrap_or("Workflow")
            .to_string();
        detail = n8n_detail(&json, success);
        break;
    }

    log::line(format!("n8n execution {id} {status} · {name}"));
    emit(&app, IntegrationUpdate {
        id: "integration_n8n",
        data: json!({ "workflow": name, "status": status }),
        error: None,
        event: Some(IntegrationEvent { success, label: name, detail }),
    });
    Ok(())
}

fn n8n_detail(json: &Value, success: bool) -> Option<String> {
    let result = json.get("data")?.get("resultData")?;
    if !success {
        if let Some(error) = result.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or("");
            if let Some(node) = error.get("node").and_then(|n| n.get("name")).and_then(Value::as_str) {
                if !node.is_empty() {
                    return Some(format!("{node}\n{message}"));
                }
            }
            return Some(message.to_string());
        }
        let runs = result.get("runData")?.as_object()?;
        for (node, value) in runs {
            if let Some(message) = value
                .as_array()
                .and_then(|a| a.first())
                .and_then(|r| r.get("error"))
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
            {
                return Some(format!("{node}\n{message}"));
            }
        }
        return None;
    }

    let last_node = result.get("lastNodeExecuted")?.as_str()?;
    let items = result
        .get("runData")?
        .get(last_node)?
        .as_array()?
        .first()?
        .get("data")?
        .get("main")?
        .as_array()?
        .first()?
        .as_array()?;
    let count = items.len();
    let header = errors::coded(errors::N8N_ITEMS, &[&count.to_string(), last_node]);

    let fields = items
        .first()
        .and_then(|i| i.get("json"))
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .take(4)
                .map(|(k, v)| format!("{k}: {}", fmt_value(v)))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|s| !s.is_empty());

    Some(match fields {
        Some(f) => format!("{header}\n{f}"),
        None => header,
    })
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.chars().take(50).collect(),
        Value::Array(a) => format!("[{}]", a.len()),
        Value::Object(_) => "{…}".into(),
        other => other.to_string(),
    }
}

// ── GitLab ────────────────────────────────────────────────────────────────────

/// Open merge requests assigned to me or written by me, and the latest pipeline
/// of my three most recently active projects. gitlab.com unless the user gave
/// their own instance.
async fn poll_gitlab(app: AppHandle) -> PollResult {
    const ID: &str = "integration_gitlab";
    let Some(token) = secrets::get("gitlab-token") else { return Err(no_key()) };
    let base = base_or("gitlab-url", "https://gitlab.com").map_err(|e| fail(&app, ID, e))?;
    let api = format!("{base}/api/v4");
    let http = client();
    let get = |path: &str| {
        http.get(format!("{api}{path}"))
            .header("PRIVATE-TOKEN", &token)
            .header("Accept", "application/json")
    };

    let mut requests: Vec<Value> = Vec::new();
    for (scope, role) in [("assigned_to_me", "assigned"), ("created_by_me", "authored")] {
        let path = format!("/merge_requests?state=opened&scope={scope}&order_by=updated_at&per_page=20");
        let (_, json) = fetch(get(&path)).await.map_err(|f| fetch_error(&app, ID, errors::INT_TOKEN_SCOPE, f))?;
        for mr in json.as_array().into_iter().flatten() {
            let Some(id) = id_of(mr, "id") else { continue };
            // Both lists hold a merge request I opened and assigned to myself.
            if requests.iter().any(|r| str_of(r, "id") == Some(id.as_str())) {
                continue;
            }
            requests.push(json!({
                "id": id,
                "title": str_of(mr, "title").unwrap_or(""),
                "ref": mr.get("references").and_then(|r| str_of(r, "full")).unwrap_or(""),
                "url": str_of(mr, "web_url").unwrap_or(""),
                "updatedAt": str_of(mr, "updated_at").unwrap_or(""),
                "draft": mr.get("draft").and_then(Value::as_bool).unwrap_or(false),
                "role": role,
            }));
        }
    }
    sort_newest(&mut requests, "updatedAt");
    let open_count = requests.len();
    requests.truncate(5);

    let projects_path = "/projects?membership=true&archived=false&simple=true&order_by=last_activity_at&sort=desc&per_page=3";
    let (_, projects) = fetch(get(projects_path)).await.map_err(|f| fetch_error(&app, ID, errors::INT_TOKEN_SCOPE, f))?;
    let mut pipelines: Vec<Value> = Vec::new();
    for project in projects.as_array().into_iter().flatten() {
        let Some(project_id) = id_of(project, "id").filter(|id| id.chars().all(|c| c.is_ascii_digit())) else { continue };
        // A project without CI (or without access to it) just has no pipeline.
        let Ok((_, list)) = fetch(get(&format!("/projects/{project_id}/pipelines?per_page=1&order_by=id&sort=desc"))).await
        else {
            continue;
        };
        let Some(p) = list.as_array().and_then(|a| a.first()) else { continue };
        let Some(id) = id_of(p, "id") else { continue };
        let status = str_of(p, "status").unwrap_or("");
        pipelines.push(json!({
            "id": id,
            "project": str_of(project, "name").unwrap_or(""),
            "ref": str_of(p, "ref").unwrap_or(""),
            "status": status,
            "state": pipeline_state(status),
            "url": str_of(p, "web_url").unwrap_or(""),
            "updatedAt": str_of(p, "updated_at").or_else(|| str_of(p, "created_at")).unwrap_or(""),
        }));
    }
    sort_newest(&mut pipelines, "updatedAt");

    let event = finished_event("gitlab", &pipelines, "ready", "project", "ref");
    emit(&app, IntegrationUpdate {
        id: ID,
        data: json!({
            "mergeRequests": requests,
            "openCount": open_count,
            "pipelines": pipelines,
            "openUrl": format!("{base}/dashboard/merge_requests"),
        }),
        error: None,
        event,
    });
    Ok(())
}

/// GitLab's pipeline statuses in the island's deploy vocabulary.
fn pipeline_state(status: &str) -> &'static str {
    match status {
        "success" => "ready",
        "failed" => "error",
        "canceled" | "skipped" => "canceled",
        _ => "building",
    }
}

// ── Sentry ────────────────────────────────────────────────────────────────────

/// Unresolved issues of one organization (the configured slug, else the first
/// the token can see): how many, and the newest. A new issue is an error event.
async fn poll_sentry(app: AppHandle) -> PollResult {
    const ID: &str = "integration_sentry";
    let Some(token) = secrets::get("sentry-token") else { return Err(no_key()) };
    let base = base_or("sentry-url", "https://sentry.io").map_err(|e| fail(&app, ID, e))?;
    let http = client();
    let auth = format!("Bearer {token}");
    let get = |url: String| http.get(url).header("Authorization", &auth).header("Accept", "application/json");

    let org = match secrets::get("sentry-org") {
        Some(raw) => {
            let slug = raw.trim().to_ascii_lowercase();
            if !path_safe(&slug) {
                return Err(fail(&app, ID, errors::coded(errors::INT_BAD_ID, &[])));
            }
            slug
        }
        None => {
            let (_, orgs) = fetch(get(format!("{base}/api/0/organizations/")))
                .await
                .map_err(|f| fetch_error(&app, ID, errors::INT_TOKEN_SCOPE, f))?;
            match orgs.as_array().and_then(|a| a.first()).and_then(|o| str_of(o, "slug")).filter(|s| path_safe(s)) {
                Some(slug) => slug.to_string(),
                None => return Err(fail(&app, ID, errors::coded(errors::INT_SENTRY_NO_ORG, &[]))),
            }
        }
    };

    let url = format!("{base}/api/0/organizations/{org}/issues/?query=is%3Aunresolved&sort=new&statsPeriod=14d&limit=100");
    let (headers, json) = match fetch(get(url)).await {
        Ok(answer) => answer,
        // A slug that doesn't exist (or isn't this token's) is a settings mistake.
        Err(Fetch::Status(404)) => return Err(fail(&app, ID, errors::coded(errors::INT_BAD_ID, &[]))),
        Err(f) => return Err(fetch_error(&app, ID, errors::INT_TOKEN_SCOPE, f)),
    };
    let list = json.as_array().cloned().unwrap_or_default();
    // X-Hits is the full count when Sentry sends it; otherwise the page is all we know.
    let hits = headers
        .get("x-hits")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<i64>().ok());
    let unresolved = hits.unwrap_or(list.len() as i64);
    let more = hits.is_none() && list.len() >= 100;

    let issues: Vec<Value> = list
        .iter()
        .take(5)
        .filter_map(|i| {
            Some(json!({
                "id": id_of(i, "id")?,
                "shortId": str_of(i, "shortId").unwrap_or(""),
                "title": str_of(i, "title").unwrap_or(""),
                "project": i.get("project").and_then(|p| str_of(p, "slug")).unwrap_or(""),
                "level": str_of(i, "level").unwrap_or("error"),
                "url": str_of(i, "permalink").unwrap_or(""),
                "firstSeen": str_of(i, "firstSeen").unwrap_or(""),
                "lastSeen": str_of(i, "lastSeen").unwrap_or(""),
            }))
        })
        .collect();

    let event = issues.first().and_then(|newest| {
        let id = str_of(newest, "id")?;
        if !is_new("sentry", id) {
            return None;
        }
        Some(IntegrationEvent {
            success: false,
            label: str_of(newest, "title")?.to_string(),
            detail: str_of(newest, "shortId").filter(|s| !s.is_empty()).map(str::to_string),
        })
    });

    // sentry.io's regional API hosts (us., de.) share one web app per organization.
    let host = reqwest::Url::parse(&base).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let open_url = if host == "sentry.io" || host.ends_with(".sentry.io") {
        format!("https://{org}.sentry.io/issues/")
    } else {
        format!("{base}/organizations/{org}/issues/")
    };

    emit(&app, IntegrationUpdate {
        id: ID,
        data: json!({ "unresolved": unresolved, "more": more, "issues": issues, "openUrl": open_url }),
        error: None,
        event,
    });
    Ok(())
}

// ── Linear ────────────────────────────────────────────────────────────────────

const LINEAR_QUERY: &str = "query { viewer { assignedIssues(first: 50, orderBy: createdAt, \
    filter: { state: { type: { nin: [\"completed\", \"canceled\"] } } }) { \
    nodes { id identifier title url priority createdAt state { name type color } } \
    pageInfo { hasNextPage } } } }";

/// My open assigned issues (GraphQL), newest first. A newly assigned issue is
/// a success event.
async fn poll_linear(app: AppHandle) -> PollResult {
    const ID: &str = "integration_linear";
    let Some(key) = secrets::get("linear-api-key") else { return Err(no_key()) };
    let response = client()
        .post("https://api.linear.app/graphql")
        // Personal API keys go bare; OAuth tokens would take "Bearer".
        .header("Authorization", key.trim())
        .header("Content-Type", "application/json")
        .json(&json!({ "query": LINEAR_QUERY }))
        .send()
        .await
        .map_err(no_connection)?;
    let status = response.status().as_u16();
    let json: Value = response.json().await.unwrap_or(Value::Null);
    if let Some(error) = linear_error(status, &json) {
        return Err(fail(&app, ID, error));
    }

    let connection = json.pointer("/data/viewer/assignedIssues");
    let mut issues: Vec<Value> = connection
        .and_then(|c| c.get("nodes"))
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|i| {
                    let state = i.get("state");
                    Some(json!({
                        "id": str_of(i, "id")?,
                        "identifier": str_of(i, "identifier").unwrap_or(""),
                        "title": str_of(i, "title").unwrap_or(""),
                        "url": str_of(i, "url").unwrap_or(""),
                        "priority": i.get("priority").and_then(Value::as_i64).unwrap_or(0),
                        "createdAt": str_of(i, "createdAt").unwrap_or(""),
                        "state": state.and_then(|s| str_of(s, "name")).unwrap_or(""),
                        "stateType": state.and_then(|s| str_of(s, "type")).unwrap_or(""),
                        "stateColor": state.and_then(|s| str_of(s, "color")).unwrap_or(""),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    sort_newest(&mut issues, "createdAt");
    let total = issues.len();
    let more = connection
        .and_then(|c| c.pointer("/pageInfo/hasNextPage"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    issues.truncate(5);

    let event = issues.first().and_then(|newest| {
        let id = str_of(newest, "id")?;
        if !is_new("linear", id) {
            return None;
        }
        let identifier = str_of(newest, "identifier").unwrap_or("");
        Some(IntegrationEvent {
            success: true,
            label: format!("{identifier} {}", str_of(newest, "title").unwrap_or("")).trim().to_string(),
            detail: None,
        })
    });

    emit(&app, IntegrationUpdate {
        id: ID,
        data: json!({ "issues": issues, "total": total, "more": more }),
        error: None,
        event,
    });
    Ok(())
}

/// GraphQL reports failures in the body, sometimes with a 200: an
/// authentication error is a bad key, anything else is shown as Linear said it.
fn linear_error(status: u16, json: &Value) -> Option<String> {
    let first = json.get("errors").and_then(Value::as_array).and_then(|e| e.first());
    let auth = status == 401
        || first
            .and_then(|e| e.pointer("/extensions/code"))
            .and_then(Value::as_str)
            .is_some_and(|c| c.eq_ignore_ascii_case("AUTHENTICATION_ERROR"));
    if auth {
        return Some(errors::coded(errors::INT_INVALID_KEY, &[]));
    }
    if let Some(message) = first.and_then(|e| str_of(e, "message")) {
        let short: String = message.chars().take(160).collect();
        return Some(errors::coded(errors::INT_API, &[&short]));
    }
    if !(200..300).contains(&status) {
        return Some(status_error(status, errors::INT_KEY_ACCESS));
    }
    None
}

// ── Netlify ───────────────────────────────────────────────────────────────────

/// The latest deploys of my three most recently updated sites. Netlify has no
/// account-wide deploy list, so it is one request per site.
async fn poll_netlify(app: AppHandle) -> PollResult {
    const ID: &str = "integration_netlify";
    let Some(token) = secrets::get("netlify-token") else { return Err(no_key()) };
    let http = client();
    let auth = format!("Bearer {token}");
    let get = |url: String| {
        http.get(url)
            .header("Authorization", &auth)
            .header("Accept", "application/json")
            .header("User-Agent", "Roadeep")
    };

    // sort_by is what Netlify's own dashboard sends; the sort below makes the
    // result right even if the API ignores it.
    let sites_url = "https://api.netlify.com/api/v1/sites?filter=all&sort_by=updated_at&order_by=desc&per_page=20";
    let (_, sites) = fetch(get(sites_url.into()))
        .await
        .map_err(|f| fetch_error(&app, ID, errors::INT_TOKEN_ACCESS, f))?;
    let mut sites = sites.as_array().cloned().unwrap_or_default();
    sort_newest(&mut sites, "updated_at");

    let mut deploys: Vec<Value> = Vec::new();
    for site in sites.iter().take(3) {
        let Some(site_id) = str_of(site, "id").filter(|id| path_safe(id)) else { continue };
        let url = format!("https://api.netlify.com/api/v1/sites/{site_id}/deploys?per_page=2");
        let (_, list) = fetch(get(url)).await.map_err(|f| fetch_error(&app, ID, errors::INT_TOKEN_ACCESS, f))?;
        for d in list.as_array().into_iter().flatten() {
            let Some(id) = id_of(d, "id") else { continue };
            let raw = str_of(d, "state").unwrap_or("");
            deploys.push(json!({
                "id": id,
                "site": str_of(d, "name").or_else(|| str_of(site, "name")).unwrap_or(""),
                "state": netlify_state(raw),
                "status": raw,
                "branch": str_of(d, "branch").unwrap_or(""),
                "title": str_of(d, "title").unwrap_or(""),
                "url": str_of(d, "deploy_ssl_url").or_else(|| str_of(d, "ssl_url")).unwrap_or(""),
                "adminUrl": str_of(d, "admin_url").unwrap_or(""),
                "createdAt": str_of(d, "created_at").unwrap_or(""),
            }));
        }
    }
    sort_newest(&mut deploys, "createdAt");
    deploys.truncate(5);

    let event = finished_event("netlify", &deploys, "ready", "site", "branch");
    emit(&app, IntegrationUpdate { id: ID, data: json!({ "deploys": deploys }), error: None, event });
    Ok(())
}

fn netlify_state(state: &str) -> &'static str {
    match state {
        "ready" => "ready",
        "error" | "rejected" => "error",
        "skipped" | "canceled" | "cancelled" => "canceled",
        _ => "building",
    }
}

// ── Cloudflare ────────────────────────────────────────────────────────────────

/// The latest deployment of every Pages project in the account, newest first.
async fn poll_cloudflare(app: AppHandle) -> PollResult {
    const ID: &str = "integration_cloudflare";
    let (Some(token), Some(raw_account)) = (secrets::get("cloudflare-token"), secrets::get("cloudflare-account-id")) else {
        return Err(no_key());
    };
    let account = raw_account.trim().to_ascii_lowercase();
    if account.len() != 32 || !account.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(fail(&app, ID, errors::coded(errors::INT_BAD_ID, &[])));
    }
    let request = client()
        .get(format!("https://api.cloudflare.com/client/v4/accounts/{account}/pages/projects"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json");
    let json = match fetch(request).await {
        Ok((_, json)) => json,
        Err(Fetch::Status(404)) => return Err(fail(&app, ID, errors::coded(errors::INT_BAD_ID, &[]))),
        Err(f) => return Err(fetch_error(&app, ID, errors::INT_CLOUDFLARE_ACCESS, f)),
    };
    if json.get("success").and_then(Value::as_bool) == Some(false) {
        let message = json.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("");
        let short: String = message.chars().take(160).collect();
        return Err(fail(&app, ID, errors::coded(errors::INT_API, &[&short])));
    }

    let mut deployments: Vec<Value> = json
        .get("result")
        .and_then(Value::as_array)
        .map(|projects| projects.iter().filter_map(cloudflare_deployment).collect())
        .unwrap_or_default();
    sort_newest(&mut deployments, "createdAt");
    deployments.truncate(5);

    let event = finished_event("cloudflare", &deployments, "ready", "project", "branch");
    emit(&app, IntegrationUpdate {
        id: ID,
        data: json!({
            "deployments": deployments,
            "openUrl": format!("https://dash.cloudflare.com/{account}/workers-and-pages"),
        }),
        error: None,
        event,
    });
    Ok(())
}

fn cloudflare_deployment(project: &Value) -> Option<Value> {
    let d = project.get("latest_deployment").filter(|d| d.is_object())?;
    let stage = d.get("latest_stage");
    let stage_name = stage.and_then(|s| str_of(s, "name")).unwrap_or("");
    let stage_status = stage.and_then(|s| str_of(s, "status")).unwrap_or("");
    let state = match (stage_name, stage_status) {
        (_, "failure") => "error",
        (_, "canceled") => "canceled",
        ("deploy", "success") => "ready",
        _ => "building",
    };
    let meta = d.pointer("/deployment_trigger/metadata");
    Some(json!({
        "id": id_of(d, "id")?,
        "project": str_of(project, "name").unwrap_or(""),
        "environment": str_of(d, "environment").unwrap_or(""),
        "state": state,
        "url": str_of(d, "url").unwrap_or(""),
        "branch": meta.and_then(|m| str_of(m, "branch")).unwrap_or(""),
        "commitMessage": meta.and_then(|m| str_of(m, "commit_message")).unwrap_or(""),
        "createdAt": str_of(d, "created_on").unwrap_or(""),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_service_key_is_a_known_secret() {
        for (id, keys) in SERVICES {
            assert!(id.starts_with("integration_"), "{id}");
            for key in *keys {
                assert!(secrets::KNOWN_KEYS.contains(key), "{id} reads {key}, which secret_set refuses");
            }
        }
    }

    #[test]
    fn base_urls_must_be_https_or_this_machine() {
        assert_eq!(base_url(" https://gitlab.example.com/ ").unwrap(), "https://gitlab.example.com");
        assert_eq!(base_url("https://git.example.com/gitlab").unwrap(), "https://git.example.com/gitlab");
        assert_eq!(base_url("http://localhost:8080").unwrap(), "http://localhost:8080");
        assert_eq!(base_url("http://127.0.0.1").unwrap(), "http://127.0.0.1");
        for bad in [
            "http://gitlab.example.com",
            "gitlab.example.com",
            "ftp://example.com",
            "https://user:pass@example.com",
            "https://example.com/?a=1",
            "https://example.com/#x",
            "",
        ] {
            assert_eq!(base_url(bad), Err(errors::coded(errors::INT_BAD_URL, &[])), "{bad}");
        }
    }

    #[test]
    fn path_parts_cannot_escape_the_path() {
        assert!(path_safe("my-org_1"));
        assert!(path_safe("abc.def"));
        let long = "x".repeat(65);
        for bad in ["", "..", ".", "a/b", "a?b", "a b", "a%2Fb", long.as_str()] {
            assert!(!path_safe(bad), "{bad}");
        }
    }

    #[test]
    fn outcomes_record_the_last_success_and_log_only_changes() {
        let mut map = HashMap::new();
        let (s, changed) = apply_outcome(&mut map, "integration_x", &Ok(()), 1000);
        assert_eq!((s.last_ok, s.error.clone(), changed), (Some(1000), None, false));

        let err = errors::coded(errors::INT_HTTP, &["500"]);
        let (s, changed) = apply_outcome(&mut map, "integration_x", &Err(err.clone()), 2000);
        assert_eq!((s.last_ok, s.error.clone(), changed), (Some(1000), Some(err), true));

        // Same code again (another status argument): not a change worth a log line.
        let again = errors::coded(errors::INT_HTTP, &["502"]);
        let (_, changed) = apply_outcome(&mut map, "integration_x", &Err(again), 3000);
        assert!(!changed);

        let (s, changed) = apply_outcome(&mut map, "integration_x", &Ok(()), 4000);
        assert_eq!((s.last_ok, s.error, changed), (Some(4000), None, true));
    }

    #[test]
    fn codes_in_the_log_carry_no_arguments() {
        assert_eq!(code_of("E_INT_API|token sk_live_123"), "E_INT_API");
        assert_eq!(code_of("E_INT_INVALID_KEY"), "E_INT_INVALID_KEY");
        assert_eq!(code_of("I_N8N_ITEMS|3|Set\nname: Ada"), "I_N8N_ITEMS");
    }

    #[test]
    fn newest_finished_item_fires_once() {
        let list = vec![
            json!({ "id": "b", "state": "building", "project": "site" }),
            json!({ "id": "a1", "state": "error", "project": "site", "branch": "main" }),
        ];
        // First sighting only fills the card.
        assert!(finished_event("test-finished", &list, "ready", "project", "branch").is_none());
        let next = vec![json!({ "id": "a2", "state": "ready", "project": "site", "branch": "" })];
        let event = finished_event("test-finished", &next, "ready", "project", "branch").unwrap();
        assert!(event.success);
        assert_eq!(event.label, "site");
        assert_eq!(event.detail, None);
        assert!(finished_event("test-finished", &next, "ready", "project", "branch").is_none());
    }

    #[test]
    fn deploy_states_map_to_four_words() {
        assert_eq!(pipeline_state("success"), "ready");
        assert_eq!(pipeline_state("failed"), "error");
        assert_eq!(pipeline_state("running"), "building");
        assert_eq!(netlify_state("rejected"), "error");
        assert_eq!(netlify_state("enqueued"), "building");
        let project = json!({
            "name": "docs",
            "latest_deployment": {
                "id": "d1", "url": "https://x.pages.dev", "environment": "production",
                "created_on": "2026-01-01T00:00:00Z",
                "latest_stage": { "name": "deploy", "status": "success" },
                "deployment_trigger": { "metadata": { "branch": "main", "commit_message": "fix" } }
            }
        });
        let d = cloudflare_deployment(&project).unwrap();
        assert_eq!((str_of(&d, "state"), str_of(&d, "branch")), (Some("ready"), Some("main")));
        assert!(cloudflare_deployment(&json!({ "name": "empty", "latest_deployment": null })).is_none());
    }

    #[test]
    fn linear_errors_come_from_the_body() {
        let auth = json!({ "errors": [{ "message": "Authentication required", "extensions": { "code": "AUTHENTICATION_ERROR" } }] });
        assert_eq!(linear_error(400, &auth), Some(errors::coded(errors::INT_INVALID_KEY, &[])));
        let other = json!({ "errors": [{ "message": "Rate limited" }] });
        assert_eq!(linear_error(200, &other), Some(errors::coded(errors::INT_API, &["Rate limited"])));
        assert_eq!(linear_error(200, &json!({ "data": {} })), None);
        assert_eq!(linear_error(500, &Value::Null), Some(errors::coded(errors::INT_HTTP, &["500"])));
    }

    #[test]
    fn a_changed_key_forgets_the_old_outcome() {
        STATUS.lock().unwrap().insert("integration_linear".into(), PollStatus::new("integration_linear"));
        STATUS.lock().unwrap().insert("integration_netlify".into(), PollStatus::new("integration_netlify"));
        key_changed("linear-api-key");
        let map = STATUS.lock().unwrap();
        assert!(!map.contains_key("integration_linear"));
        assert!(map.contains_key("integration_netlify"));
    }
}
