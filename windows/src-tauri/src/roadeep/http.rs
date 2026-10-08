// Roadeep transport: one shared reqwest client, the `{success, data}` envelope,
// error normalization and request logging. Mirrors the mobile client
// (src/api/http.ts + errors.ts). Session handling (Bearer, 401 → refresh →
// retry) sits one level up in `Roadeep::request`.
//
// Everything that interprets a response is a pure function so it can be tested
// without a network.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::{header, Method};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::log;

pub const DEFAULT_BASE: &str = "https://roadeep.com/api";

/// Compile-time override (`ROADEEP_API_URL=... cargo build`), e.g. for staging.
pub fn base_url() -> &'static str {
    option_env!("ROADEEP_API_URL")
        .map(|s| s.trim().trim_end_matches('/'))
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_BASE)
}

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// GETs are retried on network errors and 502/503/504, never anything else.
const GET_RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(400), Duration::from_millis(1200)];
/// The contract's default when a 429 carries no usable Retry-After.
const DEFAULT_RETRY_AFTER: u64 = 60;

pub mod codes {
    pub const NETWORK: &str = "NETWORK_ERROR";
    pub const TIMEOUT: &str = "TIMEOUT";
    pub const SESSION_EXPIRED: &str = "SESSION_EXPIRED";
    pub const NOT_SIGNED_IN: &str = "NOT_SIGNED_IN";
    pub const INVALID_RESPONSE: &str = "INVALID_RESPONSE";
    pub const VALIDATION: &str = "VALIDATION_ERROR";
    pub const THROTTLED: &str = "THROTTLED";
    /// Every "not enough credit" rejection, whatever code the tool behind it uses.
    pub const INSUFFICIENT_CREDITS: &str = "INSUFFICIENT_CREDITS";
}

/// What every Roadeep command returns as its error. Serialized as-is to the
/// front end (see `RoadeepError` in bridge.ts).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoadeepError {
    /// Server `error.code` when there is one (`THROTTLED`, `MODEL_UNAVAILABLE`…),
    /// a client code from `codes`, or `HTTP_<status>`.
    pub code: String,
    /// Human-readable, English. The front end translates the codes it knows.
    pub message: String,
    pub status: Option<u16>,
    /// Seconds to wait before trying again (429 only).
    pub retry_after: Option<u64>,
    pub request_id: Option<String>,
    /// `{field: [messages]}` from validation errors. Boxed to keep `Result`s small.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_errors: Option<Box<Map<String, Value>>>,
    /// The local agent a turn ran with, for errors about that agent
    /// (`AGENT_MEMORY_TRIGGER`). Its display name only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
}

impl RoadeepError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            status: None,
            retry_after: None,
            request_id: None,
            field_errors: None,
            agent_name: None,
        }
    }

    pub fn validation(field: &str, message: &str) -> Self {
        let mut fields = Map::new();
        fields.insert(field.to_string(), Value::from(vec![message.to_string()]));
        Self { field_errors: Some(Box::new(fields)), ..Self::new(codes::VALIDATION, message) }
    }
}

impl std::fmt::Display for RoadeepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

pub fn fallback_message(status: Option<u16>) -> &'static str {
    match status {
        None => "Could not reach Roadeep. Check your internet connection and try again.",
        Some(400) => "The request was not valid.",
        Some(401) => "Please sign in to Roadeep again.",
        Some(403) => "You don't have access to this.",
        Some(404) => "Not found.",
        Some(409) => "This conflicts with the current state.",
        Some(422) => "The data entered is not valid.",
        Some(429) => "Too many requests. Try again in a moment.",
        Some(502..=504) => "Roadeep is temporarily unavailable. Try again shortly.",
        Some(s) if s >= 500 => "Roadeep had a server error. Try again shortly.",
        Some(_) => "Something unexpected happened. Try again.",
    }
}

fn non_empty(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Accepts `{field: ["msg"]}` or `{field: "msg"}`; ignores anything else.
fn to_field_errors(value: Option<&Value>) -> Option<Map<String, Value>> {
    let obj = value?.as_object()?;
    let mut out = Map::new();
    for (field, raw) in obj {
        let messages: Vec<Value> = match raw {
            Value::Array(items) => items.iter().filter_map(|m| non_empty(Some(m))).map(Value::from).collect(),
            other => non_empty(Some(other)).map(Value::from).into_iter().collect(),
        };
        if !messages.is_empty() {
            out.insert(field.clone(), Value::Array(messages));
        }
    }
    (!out.is_empty()).then_some(out)
}

fn first_field_message(fields: Option<&Map<String, Value>>) -> Option<String> {
    let fields = fields?;
    // DRF puts form-level errors under these keys; they read better than a field error.
    let preferred = fields.get("non_field_errors").or_else(|| fields.get("detail"));
    preferred
        .or_else(|| fields.values().next())
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map(str::to_string)
}

const DRF_META_KEYS: &[&str] = &["success", "message", "data", "error", "detail", "code", "status"];

/// Normalizes every error body shape the backend is known to produce:
/// - `{error: {code, message, details}}` (current envelope)
/// - `{success: false, error: "text" | null, message}`
/// - `{detail: "text", code?}` (DRF default)
/// - `{field: ["msg"], non_field_errors: [...]}` (DRF serializer errors)
/// - anything else, including non-JSON bodies (passed as `None`) → by status.
pub fn normalize_error(
    status: u16,
    body: Option<&Value>,
    request_id: Option<String>,
    retry_after: Option<u64>,
) -> RoadeepError {
    let status_code = format!("HTTP_{status}");
    let fallback = || fallback_message(Some(status)).to_string();

    let (code, message, field_errors) = match body.and_then(Value::as_object) {
        Some(obj) if obj.get("error").map(Value::is_object).unwrap_or(false) => {
            let error = &obj["error"];
            let fields = to_field_errors(error.get("details"));
            let message = non_empty(error.get("message"))
                .or_else(|| first_field_message(fields.as_ref()))
                .unwrap_or_else(fallback);
            (non_empty(error.get("code")).unwrap_or(status_code), message, fields)
        }
        Some(obj) if non_empty(obj.get("detail")).is_some() => (
            non_empty(obj.get("code")).unwrap_or(status_code),
            non_empty(obj.get("detail")).unwrap_or_default(),
            None,
        ),
        Some(obj) if obj.get("success") == Some(&Value::Bool(false)) => (
            status_code,
            non_empty(obj.get("message")).or_else(|| non_empty(obj.get("error"))).unwrap_or_else(fallback),
            None,
        ),
        Some(obj) => {
            let candidates: Map<String, Value> = obj
                .iter()
                .filter(|(k, _)| !DRF_META_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let fields = to_field_errors(Some(&Value::Object(candidates)));
            if fields.is_some() {
                let message = first_field_message(fields.as_ref()).unwrap_or_else(fallback);
                (codes::VALIDATION.to_string(), message, fields)
            } else {
                (status_code, non_empty(obj.get("message")).unwrap_or_else(fallback), None)
            }
        }
        None => (status_code, fallback(), None),
    };

    let throttled = status == 429;
    let code = if is_insufficient_credit(Some(status), &code, &message) { codes::INSUFFICIENT_CREDITS.to_string() } else { code };
    RoadeepError {
        code: if throttled && code.starts_with("HTTP_") { codes::THROTTLED.into() } else { code },
        message,
        status: Some(status),
        // Never auto-retried; the UI shows how long to wait.
        retry_after: if throttled { Some(retry_after.unwrap_or(DEFAULT_RETRY_AFTER)) } else { None },
        request_id,
        field_errors: field_errors.map(Box::new),
        agent_name: None,
    }
}

/// "Not enough credit", the way the web client decides it (WEB
/// src/utils/token-balance.ts `isInsufficientCreditError`): 402, any code with
/// INSUFFICIENT_CREDIT in it (each tool has its own), or a message that pairs a
/// shortage word with a credit word, in English or Persian. A 401 never is.
pub fn is_insufficient_credit(status: Option<u16>, code: &str, message: &str) -> bool {
    if status == Some(401) {
        return false;
    }
    if status == Some(402) || code.to_ascii_uppercase().contains("INSUFFICIENT_CREDIT") {
        return true;
    }
    let text = message.trim().to_lowercase();
    if text.is_empty() {
        return false;
    }
    if text.contains("payment required") || text.contains("insufficient_funds") {
        return true;
    }
    let words: Vec<&str> = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
    let pair = |a: &str, b: &str| words.windows(2).any(|w| w[0] == a && w[1] == b);
    let noun_en = words
        .iter()
        .any(|w| matches!(*w, "credit" | "credits" | "balance" | "token" | "tokens" | "fund" | "funds"));
    // "not … enough" with up to three words in between.
    let not_enough = words
        .iter()
        .enumerate()
        .any(|(i, w)| *w == "not" && words.iter().skip(i + 1).take(4).any(|x| *x == "enough"));
    let shortage_en = ["insufficient", "exhausted", "depleted"].iter().any(|s| text.contains(s))
        || not_enough
        || pair("out", "of")
        || pair("ran", "out")
        || pair("too", "low");
    if noun_en && shortage_en {
        return true;
    }
    let noun_fa = ["اعتبار", "موجودی", "توکن", "رصيد", "رصید"].iter().any(|s| message.contains(s));
    let shortage_fa = ["کافی نیست", "ناکافی", "تمام شد", "تمام شده", "به پایان رسید", "کسری", "غير كاف", "لا يكفي", "نفد"]
        .iter()
        .any(|s| message.contains(s));
    noun_fa && shortage_fa
}

/// Unwraps `{success: true, data}`. `success: false` is an error even on a 2xx.
/// Bodies without a boolean `success` pass through untouched.
pub fn unwrap_envelope(
    status: u16,
    body: Option<Value>,
    request_id: Option<String>,
) -> Result<Value, RoadeepError> {
    match body {
        Some(Value::Object(mut obj)) if obj.get("success").map(Value::is_boolean).unwrap_or(false) => {
            if obj.get("success") == Some(&Value::Bool(false)) {
                return Err(normalize_error(status, Some(&Value::Object(obj)), request_id, None));
            }
            Ok(obj.remove("data").unwrap_or(Value::Null))
        }
        Some(other) => Ok(other),
        None => Ok(Value::Null),
    }
}

/// Non-JSON bodies (proxy HTML error pages) are treated as absent; the status
/// drives the error.
pub fn parse_body(text: &str) -> Option<Value> {
    if text.trim().is_empty() {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// `Retry-After` as delta-seconds or an HTTP-date (IMF-fixdate).
pub fn parse_retry_after(value: Option<&str>, now: SystemTime) -> Option<u64> {
    let value = value?.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs);
    }
    let at = parse_http_date(value)?;
    let now = now.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(at.saturating_sub(now))
}

/// "Sun, 06 Nov 1994 08:49:37 GMT" → Unix seconds.
fn parse_http_date(value: &str) -> Option<u64> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [_, day, month, year, time, "GMT"] = parts.as_slice() else { return None };
    let day: u64 = day.parse().ok()?;
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|m| m == month)? as u64
        + 1;
    let year: i64 = year.parse().ok()?;
    let hms: Vec<u64> = time.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [h, m, s] = hms.as_slice() else { return None };
    if !(1..=31).contains(&day) || *h > 23 || *m > 59 || *s > 60 {
        return None;
    }
    // Days from civil (Howard Hinnant), proleptic Gregorian.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + (*h * 3600 + *m * 60 + *s) as i64;
    u64::try_from(secs).ok()
}

/// Auth endpoints whose 401 means "credentials rejected": refreshing would turn
/// "wrong password" into "session expired".
pub fn is_auth_endpoint(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path).trim_end_matches('/');
    ["login", "register", "logout", "otp/send", "otp/verify", "token/refresh"]
        .iter()
        .any(|e| path.ends_with(&format!("/v1/auth/{e}")))
}

/// The Bearer may only travel to the Roadeep API host.
pub fn is_trusted(url: &str, base: &str) -> bool {
    match (reqwest::Url::parse(url), reqwest::Url::parse(base)) {
        (Ok(u), Ok(b)) => {
            u.scheme() == b.scheme() && u.host_str() == b.host_str() && u.port_or_known_default() == b.port_or_known_default()
        }
        _ => false, // unparseable: fail closed
    }
}

/// `X-Has-More` on paged lists (threads, messages); None when the server does not say.
pub fn parse_has_more(value: Option<&str>) -> Option<bool> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Request paths only — never the query string.
fn path_for_log(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

// ── Transport ─────────────────────────────────────────────────────────────────

pub struct Upload {
    pub file_name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub thread_id: Option<String>,
}

pub enum Payload {
    Empty,
    Json(Value),
    Upload(Upload),
}

pub struct Call {
    pub method: Method,
    /// Relative to the base URL, e.g. "/v1/chat/models/".
    pub path: String,
    pub payload: Payload,
    /// Attach the session Bearer and handle 401 → refresh → retry.
    pub auth: bool,
    pub timeout: Duration,
}

impl Call {
    pub fn get(path: impl Into<String>) -> Self {
        Self { method: Method::GET, path: path.into(), payload: Payload::Empty, auth: true, timeout: DEFAULT_TIMEOUT }
    }
    pub fn post(path: impl Into<String>, body: Value) -> Self {
        Self { method: Method::POST, path: path.into(), payload: Payload::Json(body), auth: true, timeout: DEFAULT_TIMEOUT }
    }
    pub fn delete(path: impl Into<String>) -> Self {
        Self { method: Method::DELETE, path: path.into(), payload: Payload::Empty, auth: true, timeout: DEFAULT_TIMEOUT }
    }
    pub fn public(mut self) -> Self {
        self.auth = false;
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// A response before the envelope is interpreted.
pub struct Raw {
    pub status: u16,
    pub request_id: Option<String>,
    pub retry_after: Option<u64>,
    /// `X-Has-More`, for paged lists.
    pub has_more: Option<bool>,
    pub body: Option<Value>,
}

impl Raw {
    /// Envelope unwrap on success, normalized error otherwise.
    pub fn into_result(self) -> Result<Value, RoadeepError> {
        if !(200..300).contains(&self.status) {
            return Err(normalize_error(self.status, self.body.as_ref(), self.request_id, self.retry_after));
        }
        unwrap_envelope(self.status, self.body, self.request_id)
    }
}

#[derive(Clone)]
pub struct Transport {
    client: reqwest::Client,
    base: &'static str,
}

/// Roadeep is a domestic service: a VPN/system proxy (HTTPS_PROXY, often a local
/// v2ray/xray on 127.0.0.1) usually cannot reach it, while a direct connection
/// can. So Roadeep traffic goes direct unless ROADEEP_PROXY=1 opts back
/// in. The integrations keep honouring the proxy — they often need it.
pub(crate) fn proxy_policy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    if proxy_opt_in() {
        builder
    } else {
        builder.no_proxy()
    }
}

/// ROADEEP_PROXY=1: the user routes Roadeep through the system proxy.
/// The WebSocket cannot follow (it always goes direct), so chat then polls.
pub(crate) fn proxy_opt_in() -> bool {
    std::env::var("ROADEEP_PROXY").is_ok_and(|v| v == "1")
}

impl Transport {
    pub fn new() -> Self {
        let client = proxy_policy(reqwest::Client::builder())
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("Roadeep-Windows/", env!("CARGO_PKG_VERSION")))
            .https_only(base_url().starts_with("https://"))
            .build()
            .unwrap_or_else(|err| {
                log::line(format!("roadeep: client builder failed ({err}), using defaults"));
                reqwest::Client::new()
            });
        Self { client, base: base_url() }
    }

    pub fn base(&self) -> &'static str {
        self.base
    }

    /// Sends one call, retrying GETs on network errors and 502/503/504.
    /// `bearer` is attached only when the URL is on the Roadeep host.
    pub async fn send(&self, call: &Call, bearer: Option<&str>) -> Result<Raw, RoadeepError> {
        let mut attempt = 0;
        loop {
            let result = self.send_once(call, bearer).await;
            let retriable = call.method == Method::GET
                && match &result {
                    Ok(raw) => matches!(raw.status, 502..=504),
                    Err(err) => err.code == codes::NETWORK || err.code == codes::TIMEOUT,
                };
            if !retriable || attempt >= GET_RETRY_DELAYS.len() {
                return result;
            }
            tokio::time::sleep(GET_RETRY_DELAYS[attempt]).await;
            attempt += 1;
        }
    }

    async fn send_once(&self, call: &Call, bearer: Option<&str>) -> Result<Raw, RoadeepError> {
        let url = format!("{}{}", self.base, call.path);
        let path = path_for_log(&call.path);
        let mut req = self
            .client
            .request(call.method.clone(), &url)
            .timeout(call.timeout)
            .header(header::ACCEPT, "application/json");
        if let Some(token) = bearer {
            if is_trusted(&url, self.base) {
                req = req.bearer_auth(token);
            } else {
                log::line(format!("roadeep: refused to send the Bearer to a foreign URL ({path})"));
            }
        }
        req = match &call.payload {
            Payload::Empty => req,
            Payload::Json(body) => req.json(body),
            Payload::Upload(up) => {
                let part = reqwest::multipart::Part::bytes(up.bytes.clone())
                    .file_name(up.file_name.clone())
                    .mime_str(&up.mime)
                    .map_err(|e| RoadeepError::new(codes::VALIDATION, format!("Invalid file type: {e}")))?;
                let mut form = reqwest::multipart::Form::new().part("files", part);
                if let Some(thread) = &up.thread_id {
                    form = form.text("thread_id", thread.clone());
                }
                req.multipart(form)
            }
        };

        let started = Instant::now();
        let response = match req.send().await {
            Ok(r) => r,
            Err(err) => {
                let ms = started.elapsed().as_millis();
                let (code, message) = if err.is_timeout() {
                    (codes::TIMEOUT, "Roadeep took too long to answer. Try again.")
                } else {
                    (codes::NETWORK, fallback_message(None))
                };
                // reqwest's Display never includes headers, so no token can leak here.
                log::line(format!("roadeep {} {path} failed {code} {ms}ms: {err}", call.method));
                return Err(RoadeepError::new(code, message));
            }
        };

        let status = response.status().as_u16();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let retry_after = parse_retry_after(
            response.headers().get(header::RETRY_AFTER).and_then(|v| v.to_str().ok()),
            SystemTime::now(),
        );
        let has_more = parse_has_more(response.headers().get("x-has-more").and_then(|v| v.to_str().ok()));
        let text = match response.text().await {
            Ok(t) => t,
            Err(err) => {
                log::line(format!("roadeep {} {path} body read failed: {err}", call.method));
                return Err(RoadeepError::new(codes::NETWORK, fallback_message(None)));
            }
        };
        let ms = started.elapsed().as_millis();
        log::line(format!(
            "roadeep {} {path} {status} {ms}ms req={}",
            call.method,
            request_id.as_deref().unwrap_or("-")
        ));
        Ok(Raw { status, request_id, retry_after, has_more, body: parse_body(&text) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_success_unwraps_data() {
        let body = json!({ "success": true, "message": "ok", "data": { "items": [1] } });
        assert_eq!(unwrap_envelope(200, Some(body), None).unwrap(), json!({ "items": [1] }));
        // success without data → null
        assert_eq!(unwrap_envelope(200, Some(json!({ "success": true })), None).unwrap(), Value::Null);
        // no envelope → passthrough
        assert_eq!(unwrap_envelope(200, Some(json!([1, 2])), None).unwrap(), json!([1, 2]));
        assert_eq!(unwrap_envelope(204, None, None).unwrap(), Value::Null);
        // a non-boolean `success` is not an envelope
        let odd = json!({ "success": "yes", "x": 1 });
        assert_eq!(unwrap_envelope(200, Some(odd.clone()), None).unwrap(), odd);
    }

    #[test]
    fn envelope_failure_on_2xx_is_an_error() {
        let body = json!({ "success": false, "message": "نشد", "error": null });
        let err = unwrap_envelope(200, Some(body), Some("rid".into())).unwrap_err();
        assert_eq!(err.message, "نشد");
        assert_eq!(err.code, "HTTP_200");
        assert_eq!(err.request_id.as_deref(), Some("rid"));
    }

    #[test]
    fn normalizes_current_error_envelope() {
        let body = json!({ "success": false, "message": "x",
            "error": { "code": "MODEL_UNAVAILABLE", "message": "Model is off", "details": { "model": ["bad"] } } });
        let err = normalize_error(400, Some(&body), None, None);
        assert_eq!(err.code, "MODEL_UNAVAILABLE");
        assert_eq!(err.message, "Model is off");
        assert_eq!(err.status, Some(400));
        assert_eq!(err.field_errors.unwrap()["model"], json!(["bad"]));
    }

    #[test]
    fn error_envelope_without_message_uses_first_field_error() {
        let body = json!({ "error": { "code": "VALIDATION", "details": { "phone": "شماره نامعتبر" } } });
        let err = normalize_error(400, Some(&body), None, None);
        assert_eq!(err.code, "VALIDATION");
        assert_eq!(err.message, "شماره نامعتبر");
    }

    #[test]
    fn normalizes_drf_detail() {
        let err = normalize_error(401, Some(&json!({ "detail": "Token invalid", "code": "token_not_valid" })), None, None);
        assert_eq!(err.code, "token_not_valid");
        assert_eq!(err.message, "Token invalid");
        let err = normalize_error(404, Some(&json!({ "detail": "Not here" })), None, None);
        assert_eq!(err.code, "HTTP_404");
    }

    #[test]
    fn normalizes_success_false_with_string_error() {
        let err = normalize_error(400, Some(&json!({ "success": false, "error": "bad thing" })), None, None);
        assert_eq!(err.message, "bad thing");
        assert_eq!(err.code, "HTTP_400");
    }

    #[test]
    fn normalizes_drf_field_errors_preferring_non_field_errors() {
        let body = json!({ "email": ["Enter a valid email."], "non_field_errors": ["Wrong credentials"] });
        let err = normalize_error(400, Some(&body), None, None);
        assert_eq!(err.code, codes::VALIDATION);
        assert_eq!(err.message, "Wrong credentials");
        let fields = err.field_errors.unwrap();
        assert_eq!(fields["email"], json!(["Enter a valid email."]));

        let err = normalize_error(400, Some(&json!({ "password": ["Too short"] })), None, None);
        assert_eq!(err.message, "Too short");
    }

    #[test]
    fn normalizes_plain_message_and_non_json() {
        let err = normalize_error(500, Some(&json!({ "message": "boom" })), None, None);
        assert_eq!((err.code.as_str(), err.message.as_str()), ("HTTP_500", "boom"));

        // HTML proxy page → parse_body gives None → status fallback
        assert!(parse_body("<html>502 Bad Gateway</html>").is_none());
        assert!(parse_body("   ").is_none());
        let err = normalize_error(502, None, Some("r1".into()), None);
        assert_eq!(err.code, "HTTP_502");
        assert_eq!(err.message, fallback_message(Some(502)));
        assert_eq!(err.request_id.as_deref(), Some("r1"));
    }

    #[test]
    fn throttling_gets_a_code_and_a_retry_after() {
        let err = normalize_error(429, None, None, None);
        assert_eq!(err.code, codes::THROTTLED);
        assert_eq!(err.retry_after, Some(60));

        let body = json!({ "error": { "code": "FILE_UPLOAD_LIMIT_REACHED", "message": "limit" } });
        let err = normalize_error(429, Some(&body), None, Some(12));
        assert_eq!(err.code, "FILE_UPLOAD_LIMIT_REACHED");
        assert_eq!(err.retry_after, Some(12));

        assert_eq!(normalize_error(400, None, None, Some(5)).retry_after, None);
    }

    #[test]
    fn insufficient_credit_is_one_code() {
        let err = normalize_error(403, Some(&json!({ "error": { "code": "AD_DESIGN_INSUFFICIENT_CREDITS", "message": "x" } })), None, None);
        assert_eq!(err.code, codes::INSUFFICIENT_CREDITS);
        assert_eq!(normalize_error(402, None, None, None).code, codes::INSUFFICIENT_CREDITS);
        let err = normalize_error(403, Some(&json!({ "detail": "You do not have enough tokens for this." })), None, None);
        assert_eq!(err.code, codes::INSUFFICIENT_CREDITS);
        let err = normalize_error(400, Some(&json!({ "message": "اعتبار شما کافی نیست" })), None, None);
        assert_eq!(err.code, codes::INSUFFICIENT_CREDITS);
        // look-alikes stay what they are
        for (status, msg) in [
            (507, "insufficient storage space"),
            (401, "Token is invalid or expired"),
            (403, "Feature not allowed on your plan"),
            (400, "توکن نامعتبر است"),
        ] {
            let err = normalize_error(status, Some(&json!({ "message": msg })), None, None);
            assert_ne!(err.code, codes::INSUFFICIENT_CREDITS, "{msg}");
        }
        assert!(is_insufficient_credit(Some(200), "", "Payment required"));
        assert!(is_insufficient_credit(None, "", "Your balance is too low"));
        assert!(!is_insufficient_credit(Some(401), "INSUFFICIENT_CREDITS", ""));
    }

    #[test]
    fn has_more_header() {
        assert_eq!(parse_has_more(Some("true")), Some(true));
        assert_eq!(parse_has_more(Some(" False ")), Some(false));
        assert_eq!(parse_has_more(Some("maybe")), None);
        assert_eq!(parse_has_more(None), None);
    }

    #[test]
    fn retry_after_parsing() {
        let now = UNIX_EPOCH + Duration::from_secs(784_111_717); // Sun, 06 Nov 1994 08:48:37 GMT
        assert_eq!(parse_retry_after(Some("120"), now), Some(120));
        assert_eq!(parse_retry_after(Some(" 7 "), now), Some(7));
        assert_eq!(parse_retry_after(Some("Sun, 06 Nov 1994 08:49:37 GMT"), now), Some(60));
        // a date in the past → 0, not a negative wait
        assert_eq!(parse_retry_after(Some("Sun, 06 Nov 1994 08:00:00 GMT"), now), Some(0));
        assert_eq!(parse_retry_after(Some("soon"), now), None);
        assert_eq!(parse_retry_after(Some("-5"), now), None);
        assert_eq!(parse_retry_after(None, now), None);
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_http_date("Tue, 29 Feb 2000 12:00:00 GMT"), Some(951_825_600));
    }

    #[test]
    fn auth_endpoints_are_never_refreshed() {
        for p in ["/v1/auth/login/", "/v1/auth/otp/send/", "/v1/auth/otp/verify", "/v1/auth/token/refresh/", "/v1/auth/logout/"] {
            assert!(is_auth_endpoint(p), "{p}");
        }
        for p in ["/v1/auth/me/", "/v1/chat/models/", "/v1/chat/async/?x=/v1/auth/login/"] {
            assert!(!is_auth_endpoint(p), "{p}");
        }
    }

    #[test]
    fn bearer_only_goes_to_the_roadeep_host() {
        let base = "https://roadeep.com/api";
        assert!(is_trusted("https://roadeep.com/api/v1/chat/models/", base));
        assert!(!is_trusted("https://evil.com/api/v1/chat/models/", base));
        assert!(!is_trusted("https://roadeep.com.evil.com/api/", base));
        assert!(!is_trusted("http://roadeep.com/api/", base));
        assert!(!is_trusted("https://roadeep.com:8443/api/", base));
        assert!(!is_trusted("not a url", base));
    }

    #[test]
    fn base_url_has_no_trailing_slash() {
        assert!(!base_url().ends_with('/'));
        assert!(base_url().starts_with("http"));
    }

    /// Live TLS smoke check: `cargo test -p roadeep -- --ignored live_tls`.
    /// An unauthenticated GET must come back as an HTTP status, not a TLS error.
    #[test]
    #[ignore]
    fn live_tls_roadeep_models_unauthenticated() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let raw = rt
            .block_on(Transport::new().send(&Call::get("/v1/chat/models/").public(), None))
            .expect("TLS/network failure talking to Roadeep");
        assert!(matches!(raw.status, 200 | 401), "unexpected status {}", raw.status);
        eprintln!("live: GET /v1/chat/models/ → {} req={:?}", raw.status, raw.request_id);
    }
}
