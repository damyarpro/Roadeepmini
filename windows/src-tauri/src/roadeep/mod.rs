// Roadeep backend: account, chat, models and agents. Replaces the Anthropic
// client of the original project.
//
// Tokens never leave Rust: the front end only ever sees `SessionInfo`, and
// every authenticated call is made here. Layout:
//   http.rs  — transport, envelope, error normalization (pure, tested)
//   auth.rs  — token session, validation, login / OTP / logout / profile
//   chat.rs  — threads, uploads, async jobs, live turns, approvals, models, agents
//   ws.rs    — the realtime WebSocket (frames, per-turn tracker, connection)
//   threads.rs — conversation history (list, open, delete)
//   generation.rs — Generation Hub quotes / submits (used by the MCP tools)

pub mod auth;
pub mod chat;
pub mod generation;
pub mod http;
pub mod model_choice;
pub mod threads;
pub mod ws;

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::log;
use auth::{Session, SessionInfo};
use http::{codes, Call, RoadeepError, Transport};

pub const SESSION_EVENT: &str = "roadeep-session";
/// Broadcast with a fresh `Balance` after each finished chat turn and on sign-in.
pub const BALANCE_EVENT: &str = "roadeep-balance";
const REFRESH_PATH: &str = "/v1/auth/token/refresh/";
/// `roadeep_balance` answers from memory for this long.
pub const BALANCE_MAX_AGE: Duration = Duration::from_secs(60);

/// Spendable units and plan name (`GET /v1/users/plan/me/`); null when unknown.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Balance {
    pub units: Option<u64>,
    pub plan: Option<String>,
}

pub(crate) enum Refresh {
    Ok(String),
    /// 401/403 on refresh, or the session changed meanwhile: log out.
    Invalid,
    /// Network error, 5xx, malformed body: keep the session.
    Retriable,
}

pub struct Roadeep {
    pub(crate) transport: Transport,
    pub(crate) session: Session,
    /// Single-flight refresh: concurrent 401s share one refresh request.
    refresh_lock: tokio::sync::Mutex<()>,
    app: OnceLock<AppHandle>,
    /// Last balance read, with the session generation it belongs to: a value
    /// from before a sign-out or sign-in is never served.
    balance: Mutex<Option<(Instant, u64, Balance)>>,
}

impl Roadeep {
    pub fn new() -> Self {
        let transport = Transport::new();
        log::line(format!("roadeep: API base {}", transport.base()));
        Self {
            transport,
            session: Session::load(),
            refresh_lock: tokio::sync::Mutex::new(()),
            app: OnceLock::new(),
            balance: Mutex::new(None),
        }
    }

    /// Wires the handle used to broadcast "roadeep-session" to every window.
    pub fn attach(&self, app: AppHandle) {
        let _ = self.app.set(app);
    }

    pub(crate) fn emit_session(&self, info: &SessionInfo) {
        let Some(app) = self.app.get() else { return };
        if let Err(err) = app.emit(SESSION_EVENT, info.clone()) {
            log::line(format!("roadeep: could not emit {SESSION_EVENT}: {err}"));
        }
    }

    /// Emits to one window (`Some(label)`) or to all of them. A no-op in unit
    /// tests, where there is no app.
    pub(crate) fn emit_to<S: Serialize + Clone>(&self, label: Option<&str>, event: &str, payload: S) {
        let Some(app) = self.app.get() else { return };
        let result = match label {
            Some(label) => app.emit_to(label, event, payload),
            None => app.emit(event, payload),
        };
        if let Err(err) = result {
            log::line(format!("roadeep: could not emit {event}: {err}"));
        }
    }

    /// The balance, from memory when it is younger than `max_age` and belongs
    /// to the current session.
    pub async fn balance(&self, max_age: Duration) -> Result<Balance, RoadeepError> {
        let generation = self.session.generation();
        if let Some((at, g, cached)) = self.balance.lock().unwrap().as_ref() {
            if *g == generation && at.elapsed() <= max_age {
                return Ok(cached.clone());
            }
        }
        let data = self.request(Call::get("/v1/users/plan/me/")).await?;
        let (plan, units) = chat::parse_plan(&data);
        let balance = Balance { units, plan };
        self.store_balance(generation, &balance);
        Ok(balance)
    }

    /// Keeps a plan read made elsewhere (the account card) for `balance()`.
    pub(crate) fn store_balance(&self, generation: u64, balance: &Balance) {
        if self.session.generation() == generation {
            *self.balance.lock().unwrap() = Some((Instant::now(), generation, balance.clone()));
        }
    }

    /// Reads the balance afresh in the background and broadcasts it. Used after
    /// a finished chat turn (credit was spent) and after a sign-in.
    pub(crate) fn push_balance(&self) {
        let Some(app) = self.app.get().cloned() else { return };
        tauri::async_runtime::spawn(async move {
            let rd = app.state::<Roadeep>();
            match rd.balance(Duration::ZERO).await {
                Ok(balance) => rd.emit_to(None, BALANCE_EVENT, balance),
                Err(err) => log::line(format!("roadeep: balance unavailable ({})", err.code)),
            }
        });
    }

    /// A refresh was rejected: the session is over. Ends it unless the user
    /// signed out (or in) meanwhile, and returns the error to surface.
    pub(crate) fn expired(&self, generation: u64, request_id: Option<String>) -> RoadeepError {
        // An intentional logout meanwhile already cleared the session and must
        // not surface as "expired".
        if self.session.generation() == generation && self.session.has_session() {
            self.end_session("expired", false);
        }
        let mut err = RoadeepError::new(codes::SESSION_EXPIRED, "Your Roadeep session expired. Sign in again.");
        err.status = Some(401);
        err.request_id = request_id;
        err
    }

    /// Clears the session; tells the windows when there was one to end (or
    /// always, for an explicit logout).
    pub(crate) fn end_session(&self, reason: &str, always_emit: bool) {
        *self.balance.lock().unwrap() = None;
        let had = self.session.clear();
        if had {
            log::line(format!("roadeep: session ended ({reason})"));
        }
        if had || always_emit {
            self.emit_session(&SessionInfo { signed_in: false, user: None, reason: Some(reason.into()) });
        }
    }

    /// One API call with the session: Bearer, 401 → single refresh → one
    /// retry, envelope unwrap. Mirrors `request()` in the mobile http.ts.
    pub async fn request(&self, call: Call) -> Result<Value, RoadeepError> {
        self.request_paged(call).await.map(|(data, _)| data)
    }

    /// `request` plus the `X-Has-More` header of paged lists.
    pub async fn request_paged(&self, call: Call) -> Result<(Value, Option<bool>), RoadeepError> {
        let generation = self.session.generation();
        let mut token = None;
        if call.auth {
            if !self.session.has_session() && self.session.access().is_none() {
                return Err(RoadeepError::new(codes::NOT_SIGNED_IN, "Sign in to Roadeep first."));
            }
            token = self.session.access();
            // Access missing but refresh present: refresh first.
            if token.is_none() {
                if let Refresh::Ok(access) = self.refresh(None).await {
                    token = Some(access);
                }
            }
        }

        let mut raw = self.transport.send(&call, token.as_deref()).await?;

        if raw.status == 401 && call.auth && self.session.has_session() && !http::is_auth_endpoint(&call.path) {
            match self.refresh(token.as_deref()).await {
                Refresh::Ok(access) => raw = self.transport.send(&call, Some(&access)).await?,
                Refresh::Invalid => return Err(self.expired(generation, raw.request_id)),
                Refresh::Retriable => {} // keep the session, surface the 401 below
            }
        }

        let path = call.path.clone();
        let has_more = raw.has_more;
        raw.into_result().map(|data| (data, has_more)).inspect_err(|err| {
            log::line(format!(
                "roadeep {} {path} error {} status={} req={}",
                call.method,
                err.code,
                err.status.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
                err.request_id.as_deref().unwrap_or("-")
            ));
        })
    }

    /// `used` is the access token the failed request carried. If another caller
    /// already replaced it, that new token is reused instead of refreshing again.
    pub(crate) async fn refresh(&self, used: Option<&str>) -> Refresh {
        let _guard = self.refresh_lock.lock().await;
        if let Some(current) = self.session.access() {
            if used != Some(current.as_str()) {
                return Refresh::Ok(current);
            }
        }
        let Some(refresh) = self.session.refresh_token() else { return Refresh::Invalid };
        let generation = self.session.generation();

        // No Authorization header: the server rejects a stale Bearer with 401
        // even here, which would log out a user whose refresh token is fine.
        let call = Call::post(REFRESH_PATH, json!({ "refresh": refresh })).public();
        let raw = match self.transport.send(&call, None).await {
            Ok(raw) => raw,
            Err(err) => {
                log::line(format!("roadeep: refresh failed ({}), keeping session", err.code));
                return Refresh::Retriable;
            }
        };
        let outcome = refresh_outcome(raw.status, raw.body.as_ref());
        match outcome {
            RefreshParse::Invalid => {
                log::line(format!("roadeep: refresh rejected ({})", raw.status));
                Refresh::Invalid
            }
            RefreshParse::Retriable => {
                log::line(format!("roadeep: refresh not usable ({}), keeping session", raw.status));
                Refresh::Retriable
            }
            RefreshParse::Ok { access, rotated } => {
                if self.session.apply_refresh(generation, access.clone(), rotated) {
                    Refresh::Ok(access)
                } else {
                    log::line("roadeep: refresh discarded after a session change");
                    Refresh::Invalid
                }
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum RefreshParse {
    Ok { access: String, rotated: Option<String> },
    Invalid,
    Retriable,
}

/// 401/403 → invalid; other failures or a body without `access` → retriable.
/// `access` comes from `data.access`, falling back to top-level `access`.
fn refresh_outcome(status: u16, body: Option<&Value>) -> RefreshParse {
    if status == 401 || status == 403 {
        return RefreshParse::Invalid;
    }
    if !(200..300).contains(&status) {
        return RefreshParse::Retriable;
    }
    let pick = |k: &str| {
        body.and_then(|b| b.get("data"))
            .and_then(|d| d.get(k))
            .and_then(Value::as_str)
            .or_else(|| body.and_then(|b| b.get(k)).and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match pick("access") {
        Some(access) => RefreshParse::Ok { access, rotated: pick("refresh") },
        None => RefreshParse::Retriable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_response_shapes() {
        assert_eq!(
            refresh_outcome(200, Some(&json!({ "success": true, "data": { "access": "a", "refresh": "r" } }))),
            RefreshParse::Ok { access: "a".into(), rotated: Some("r".into()) }
        );
        assert_eq!(
            refresh_outcome(200, Some(&json!({ "access": "a" }))),
            RefreshParse::Ok { access: "a".into(), rotated: None }
        );
        assert_eq!(refresh_outcome(200, Some(&json!({ "data": {} }))), RefreshParse::Retriable);
        assert_eq!(refresh_outcome(200, None), RefreshParse::Retriable);
        assert_eq!(refresh_outcome(401, None), RefreshParse::Invalid);
        assert_eq!(refresh_outcome(403, None), RefreshParse::Invalid);
        assert_eq!(refresh_outcome(500, None), RefreshParse::Retriable);
        assert_eq!(refresh_outcome(429, None), RefreshParse::Retriable);
    }
}
