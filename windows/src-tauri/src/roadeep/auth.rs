// Roadeep account: the token session (Credential Manager backed), input
// validation, and the login / OTP / logout / profile calls. Mirrors the mobile
// client (features/auth/{api,store,phone}.ts and api/session.ts).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::http::{codes, Call, RoadeepError};
use super::Roadeep;
use crate::{log, secrets};

const ACCESS_KEY: &str = "roadeep-access-token";
const REFRESH_KEY: &str = "roadeep-refresh-token";
const USER_KEY: &str = "roadeep-user";

const LOGOUT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoadeepUser {
    pub id: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub avatar: Option<String>,
}

impl RoadeepUser {
    /// Tolerant by design: unknown keys are ignored and every field but `id` may
    /// be missing or null.
    pub fn from_api(v: &Value) -> Option<Self> {
        let id = match v.get("id")? {
            Value::String(s) if !s.is_empty() => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => return None,
        };
        let text = |k: &str| {
            v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
        };
        let name = text("name").or_else(|| {
            let full = [text("first_name"), text("last_name")].into_iter().flatten().collect::<Vec<_>>().join(" ");
            (!full.is_empty()).then_some(full)
        });
        Some(Self { id, name, email: text("email"), phone: text("phone"), avatar: text("avatar") })
    }
}

/// What `roadeep_session` returns and what the "roadeep-session" event carries.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub signed_in: bool,
    pub user: Option<RoadeepUser>,
    /// Why the session ended ("logout" | "expired"); null while signed in.
    pub reason: Option<String>,
}

#[derive(Default)]
struct Tokens {
    access: Option<String>,
    refresh: Option<String>,
    user: Option<RoadeepUser>,
}

/// Tokens in memory, mirrored to the Credential Manager. Deliberately not
/// `Debug`: nothing can print a token by accident.
pub struct Session {
    inner: Mutex<Tokens>,
    /// Bumped on every sign-in and sign-out, so a refresh that finishes after
    /// the user logged out is discarded instead of resurrecting the session.
    generation: AtomicU64,
    /// False only in unit tests, which must never touch the real Credential Manager.
    persistent: bool,
}

impl Session {
    pub fn load() -> Self {
        let user = secrets::session_get(USER_KEY).and_then(|raw| serde_json::from_str(&raw).ok());
        let tokens = Tokens {
            access: secrets::session_get(ACCESS_KEY),
            refresh: secrets::session_get(REFRESH_KEY),
            user,
        };
        Self { inner: Mutex::new(tokens), generation: AtomicU64::new(0), persistent: true }
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self { inner: Mutex::new(Tokens::default()), generation: AtomicU64::new(0), persistent: false }
    }

    pub fn access(&self) -> Option<String> {
        self.inner.lock().unwrap().access.clone()
    }

    pub fn refresh_token(&self) -> Option<String> {
        self.inner.lock().unwrap().refresh.clone()
    }

    /// A session exists as long as there is a refresh token.
    pub fn has_session(&self) -> bool {
        self.inner.lock().unwrap().refresh.is_some()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn info(&self) -> SessionInfo {
        let t = self.inner.lock().unwrap();
        SessionInfo { signed_in: t.refresh.is_some(), user: t.user.clone(), reason: None }
    }

    fn persist(&self, key: &str, value: Option<&str>) {
        if !self.persistent {
            return;
        }
        let result = match value {
            Some(v) => secrets::session_set(key, v),
            None => secrets::session_clear(key),
        };
        if let Err(err) = result {
            // The session still works for this run; it just won't survive a restart.
            log::line(format!("roadeep: could not persist {key}: {err}"));
        }
    }

    pub fn establish(&self, access: String, refresh: String, user: RoadeepUser) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.persist(ACCESS_KEY, Some(&access));
        self.persist(REFRESH_KEY, Some(&refresh));
        self.persist(USER_KEY, serde_json::to_string(&user).ok().as_deref());
        *self.inner.lock().unwrap() = Tokens { access: Some(access), refresh: Some(refresh), user: Some(user) };
    }

    /// Stores a refreshed access token (and a rotated refresh token) unless the
    /// session changed since the refresh started.
    pub fn apply_refresh(&self, generation: u64, access: String, rotated: Option<String>) -> bool {
        let mut t = self.inner.lock().unwrap();
        if self.generation() != generation || t.refresh.is_none() {
            return false;
        }
        self.persist(ACCESS_KEY, Some(&access));
        t.access = Some(access);
        if let Some(r) = rotated {
            self.persist(REFRESH_KEY, Some(&r));
            t.refresh = Some(r);
        }
        true
    }

    pub fn set_user(&self, user: RoadeepUser) {
        let mut t = self.inner.lock().unwrap();
        if t.refresh.is_none() {
            return; // signed out meanwhile
        }
        self.persist(USER_KEY, serde_json::to_string(&user).ok().as_deref());
        t.user = Some(user);
    }

    /// Forgets everything. Returns true when there was a session to end.
    pub fn clear(&self) -> bool {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let had = {
            let mut t = self.inner.lock().unwrap();
            let had = t.refresh.is_some() || t.access.is_some();
            *t = Tokens::default();
            had
        };
        for key in [ACCESS_KEY, REFRESH_KEY, USER_KEY] {
            self.persist(key, None);
        }
        had
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Persian (U+06F0–9) and Arabic-Indic (U+0660–9) digits → ASCII.
pub fn to_ascii_digits(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\u{06F0}'..='\u{06F9}' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
            '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
            _ => c,
        })
        .collect()
}

/// Canonical Iranian mobile form `09xxxxxxxxx`, same as the web and mobile
/// clients' `normalizePhone`. A country-code prefix is only rewritten when a
/// complete 10-digit mobile number follows it.
pub fn normalize_phone(value: &str) -> String {
    let cleaned: String = to_ascii_digits(value)
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | '-' | '(' | ')' | '\u{200C}' | '\u{200E}' | '\u{200F}'))
        .collect();
    let is_mobile_tail = |rest: &str| rest.len() == 10 && rest.starts_with('9') && rest.bytes().all(|b| b.is_ascii_digit());
    let rewritten = ["+98", "0098", "98"]
        .iter()
        .find_map(|prefix| cleaned.strip_prefix(prefix).filter(|rest| is_mobile_tail(rest)))
        .map(|rest| format!("0{rest}"))
        .unwrap_or(cleaned);
    rewritten.chars().take(11).collect()
}

pub fn is_valid_iran_mobile(value: &str) -> bool {
    value.len() == 11 && value.starts_with("09") && value.bytes().all(|b| b.is_ascii_digit())
}

pub fn normalize_otp(value: &str) -> String {
    to_ascii_digits(value).chars().filter(char::is_ascii_digit).collect()
}

pub fn is_valid_otp(value: &str) -> bool {
    (4..=8).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
}

/// Trimmed, not lower-cased (whether the backend folds case is not guaranteed).
/// A shape check only; the server is the authority.
pub fn normalize_email(value: &str) -> Option<String> {
    let email = value.trim();
    if email.len() > 254 || email.chars().any(char::is_whitespace) {
        return None;
    }
    let (local, domain) = email.split_once('@')?;
    let domain_ok = !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..");
    (!local.is_empty() && domain_ok).then(|| email.to_string())
}

/// "09123456789" → "0912***6789", for logs.
pub fn mask_phone(phone: &str) -> String {
    let chars: Vec<char> = phone.chars().collect();
    if chars.len() < 8 {
        return "***".into();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}***{tail}")
}

// ── Calls ─────────────────────────────────────────────────────────────────────

struct AuthResponse {
    access: String,
    refresh: String,
    user: RoadeepUser,
}

fn parse_auth_response(data: &Value) -> Result<AuthResponse, RoadeepError> {
    let text = |k: &str| data.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    match (text("access"), text("refresh"), data.get("user").and_then(RoadeepUser::from_api)) {
        (Some(access), Some(refresh), Some(user)) => Ok(AuthResponse { access, refresh, user }),
        _ => Err(RoadeepError::new(codes::INVALID_RESPONSE, "Roadeep sent an unexpected sign-in response.")),
    }
}

fn establish(rd: &Roadeep, auth: AuthResponse) -> SessionInfo {
    rd.session.establish(auth.access, auth.refresh, auth.user);
    let info = rd.session.info();
    log::line(format!("roadeep: signed in (user {})", info.user.as_ref().map(|u| u.id.as_str()).unwrap_or("?")));
    rd.emit_session(&info);
    rd.push_balance();
    info
}

pub async fn login(rd: &Roadeep, email: &str, password: &str) -> Result<SessionInfo, RoadeepError> {
    let email = normalize_email(email).ok_or_else(|| RoadeepError::validation("email", "Enter a valid email address."))?;
    if password.is_empty() {
        return Err(RoadeepError::validation("password", "Enter your password."));
    }
    if password.len() > 1024 {
        return Err(RoadeepError::validation("password", "That password is too long."));
    }
    let data = rd
        .request(Call::post("/v1/auth/login/", json!({ "email": email, "password": password })).public())
        .await?;
    Ok(establish(rd, parse_auth_response(&data)?))
}

pub async fn otp_send(rd: &Roadeep, phone: &str) -> Result<(), RoadeepError> {
    let phone = normalize_phone(phone);
    if !is_valid_iran_mobile(&phone) {
        return Err(RoadeepError::validation("phone", "Enter a valid mobile number (09xxxxxxxxx)."));
    }
    log::line(format!("roadeep: sending OTP to {}", mask_phone(&phone)));
    rd.request(Call::post("/v1/auth/otp/send/", json!({ "phone": phone })).public()).await?;
    Ok(())
}

pub async fn otp_verify(rd: &Roadeep, phone: &str, otp: &str) -> Result<SessionInfo, RoadeepError> {
    let phone = normalize_phone(phone);
    if !is_valid_iran_mobile(&phone) {
        return Err(RoadeepError::validation("phone", "Enter a valid mobile number (09xxxxxxxxx)."));
    }
    let otp = normalize_otp(otp);
    if !is_valid_otp(&otp) {
        return Err(RoadeepError::validation("otp", "Enter the code you received by SMS."));
    }
    let data = rd
        .request(Call::post("/v1/auth/otp/verify/", json!({ "phone": phone, "otp": otp })).public())
        .await?;
    Ok(establish(rd, parse_auth_response(&data)?))
}

/// Local sign-out always succeeds; blacklisting the refresh token server-side
/// is best effort and happens in the background.
pub fn logout(rd: &Roadeep) {
    let refresh = rd.session.refresh_token();
    let access = rd.session.access();
    rd.end_session("logout", true);
    let Some(refresh) = refresh else { return };
    let transport = rd.transport.clone();
    tauri::async_runtime::spawn(async move {
        let call = Call::post("/v1/auth/logout/", json!({ "refresh": refresh })).public().timeout(LOGOUT_TIMEOUT);
        match transport.send(&call, access.as_deref()).await.and_then(|raw| raw.into_result()) {
            Ok(_) => {}
            Err(err) => log::line(format!("roadeep: server logout failed ({}), local session already cleared", err.code)),
        }
    });
}

/// The cached session, revalidated against /auth/me/ when signed in. Offline
/// or a server hiccup keeps the cached profile; a rejected session signs out
/// (inside `Roadeep::request`).
pub async fn session(rd: &Roadeep) -> SessionInfo {
    if !rd.session.has_session() {
        return rd.session.info();
    }
    match rd.request(Call::get("/v1/auth/me/")).await {
        Ok(data) => match RoadeepUser::from_api(&data) {
            Some(user) => rd.session.set_user(user),
            None => log::line("roadeep: /auth/me/ returned an unexpected profile shape"),
        },
        Err(err) => log::line(format!("roadeep: profile refresh failed ({}), using cached profile", err.code)),
    }
    rd.session.info()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_normalization_matches_the_mobile_client() {
        for input in [
            "09123456789",
            "۰۹۱۲۳۴۵۶۷۸۹",
            "٠٩١٢٣٤٥٦٧٨٩",
            "+989123456789",
            "+98 912 345 6789",
            "+۹۸ ۹۱۲ ۳۴۵ ۶۷۸۹",
            "00989123456789",
            "989123456789",
            "0912-345-6789",
            "(0912) 345 6789",
            "091234567891234",
        ] {
            let n = normalize_phone(input);
            assert_eq!(n, "09123456789", "{input}");
            assert!(is_valid_iran_mobile(&n));
        }
        // half-typed international input is left alone
        assert_eq!(normalize_phone("+98912"), "+98912");
        assert_eq!(normalize_phone("98912345678"), "98912345678");
        for bad in ["0812345678", "08123456789", "+1 202 555 0123", "0912345678", ""] {
            assert!(!is_valid_iran_mobile(&normalize_phone(bad)), "{bad}");
        }
    }

    #[test]
    fn otp_helpers() {
        assert_eq!(normalize_otp("۱۲۳ ۴۵۶"), "123456");
        assert!(is_valid_otp("123456"));
        assert!(!is_valid_otp("12"));
        assert!(!is_valid_otp("123456789"));
        assert_eq!(to_ascii_digits("کد ۱۲۳"), "کد 123");
    }

    #[test]
    fn email_shape() {
        assert_eq!(normalize_email("  a@b.co ").as_deref(), Some("a@b.co"));
        assert_eq!(normalize_email("A.B@Example.com").as_deref(), Some("A.B@Example.com"));
        for bad in ["", "a", "a@b", "@b.co", "a@@b.co", "a b@c.co", "a@.co", "a@b.", "a@b..co"] {
            assert!(normalize_email(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn phone_mask_hides_the_middle() {
        assert_eq!(mask_phone("09123456789"), "0912***6789");
        assert_eq!(mask_phone("123"), "***");
    }

    #[test]
    fn user_mapping_is_tolerant() {
        let u = RoadeepUser::from_api(&json!({ "id": 42, "first_name": "Sara", "last_name": "K", "email": null, "extra": 1 })).unwrap();
        assert_eq!(u.id, "42");
        assert_eq!(u.name.as_deref(), Some("Sara K"));
        assert_eq!(u.email, None);
        let u = RoadeepUser::from_api(&json!({ "id": "u1", "name": "Ali", "phone": "0912" })).unwrap();
        assert_eq!(u.name.as_deref(), Some("Ali"));
        assert!(RoadeepUser::from_api(&json!({ "name": "no id" })).is_none());
    }

    #[test]
    fn auth_response_requires_tokens_and_user() {
        assert!(parse_auth_response(&json!({ "access": "a", "refresh": "r", "user": { "id": 1 } })).is_ok());
        assert!(parse_auth_response(&json!({ "access": "a", "user": { "id": 1 } })).is_err());
        assert!(parse_auth_response(&json!({ "access": "", "refresh": "r", "user": { "id": 1 } })).is_err());
    }

    #[test]
    fn refresh_after_session_change_is_discarded() {
        let s = Session::empty();
        // No session: nothing to apply to.
        assert!(!s.apply_refresh(s.generation(), "a2".into(), None));
        {
            let mut t = s.inner.lock().unwrap();
            t.refresh = Some("r1".into());
            t.access = Some("a1".into());
        }
        let generation = s.generation();
        assert!(s.apply_refresh(generation, "a2".into(), Some("r2".into())));
        assert_eq!(s.access().as_deref(), Some("a2"));
        assert_eq!(s.refresh_token().as_deref(), Some("r2"));
        // A sign-out bumps the generation; a late refresh must not land.
        s.generation.fetch_add(1, Ordering::SeqCst);
        assert!(!s.apply_refresh(generation, "a3".into(), None));
        assert_eq!(s.access().as_deref(), Some("a2"));
        assert!(s.clear());
        assert!(!s.has_session());
        assert!(!s.clear(), "a second clear has nothing to end");
    }
}
