// Runs a catalog service: reads its fields from the Credential Manager, checks
// them, sends the manifest's one request, and maps the answer onto the island's
// generic list card. One scheduler task polls every active catalog service.
//
// The request goes only where the manifest's `hosts` allow, after expansion
// with the user's values; redirects are not followed (a 3xx could otherwise
// carry the key elsewhere); the answer is capped at 2 MB.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::manifest::{text_at, Auth, Kind, Method, NotifyOn, Service};
use super::template::{url_allowed, Template, Values};
use crate::errors;
use crate::integrations::{self, IntegrationEvent, IntegrationUpdate, PollResult};
use crate::log;
use crate::secrets;

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: usize = 2 * 1024 * 1024;
/// Items read from one answer (the page an API returns), before `list.max`.
const MAX_SCANNED: usize = 200;
const TICK: Duration = Duration::from_secs(15);
const TITLE_MAX: usize = 120;
const STATUS_TEXT_MAX: usize = 40;
/// Ids remembered per service for "new item" events.
const SEEN_CAP: usize = 1000;

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Roadeep-Desktop")
        .build()
        .map_err(|e| e.to_string())
}

/// One client for every catalog request. If it cannot be built there is no
/// fallback to a default client: that one would follow redirects.
static CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(build_client);

// ── Field values ──────────────────────────────────────────────────────────────

/// The user's values, each checked against its pattern. `read` is the
/// Credential Manager (a map in tests); `language` names the field in errors.
pub fn read_values(service: &Service, language: &str, read: impl Fn(&str) -> Option<String>) -> Result<Values, String> {
    let mut values = Values::new();
    for field in &service.fields {
        let Some(raw) = read(&field.key) else {
            if field.optional {
                continue;
            }
            return Err(errors::coded(errors::INT_NO_KEY, &[]));
        };
        let value = raw.trim();
        let bad = || errors::coded(errors::INT_BAD_FIELD, &[field.label.get(language)]);
        if value.chars().any(char::is_control) || !field.pattern.is_match(value) {
            return Err(bad());
        }
        let value = if field.kind == Kind::Url { integrations::base_url(value)? } else { value.to_string() };
        values.insert(field.name.clone(), value);
    }
    Ok(values)
}

// ── Request ───────────────────────────────────────────────────────────────────

/// Everything needed to send the request. Holds the key: never logged.
pub struct Prepared {
    pub method: Method,
    pub url: reqwest::Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
}

pub fn prepare(service: &Service, values: &Values) -> Result<Prepared, String> {
    let blocked = || errors::coded(errors::INT_HOST_BLOCKED, &[]);
    let request = &service.request;
    let text = request.url.expand(values, None, true).ok_or_else(blocked)?;
    let mut url = reqwest::Url::parse(&text).map_err(|_| errors::coded(errors::INT_BAD_URL, &[]))?;
    // Before the key is attached: a request that would leave the declared hosts never starts.
    if !url_allowed(&url, &service.hosts, values) || url.fragment().is_some() {
        return Err(blocked());
    }
    {
        let mut pairs = url.query_pairs_mut();
        for (name, template) in &request.query {
            let value = template.expand(values, None, false).unwrap_or_default();
            // An optional field left empty drops its parameter.
            if !value.is_empty() {
                pairs.append_pair(name, &value);
            }
        }
    }
    let mut headers: Vec<(String, String)> = request.headers.clone();
    for part in &service.auth {
        let value_of = |name: &str| values.get(name).cloned();
        match part {
            Auth::Bearer { field } => {
                if let Some(v) = value_of(field) {
                    headers.push(("Authorization".into(), format!("Bearer {v}")));
                }
            }
            Auth::Header { name, field, prefix } => {
                if let Some(v) = value_of(field) {
                    headers.push((name.clone(), format!("{prefix}{v}")));
                }
            }
            Auth::Basic { user, pass } => {
                if let Some(u) = value_of(user) {
                    let p = pass.as_deref().and_then(value_of).unwrap_or_default();
                    let token = crate::util::base64_for(format!("{u}:{p}").as_bytes());
                    headers.push(("Authorization".into(), format!("Basic {token}")));
                }
            }
            Auth::Query { name, field } => {
                if let Some(v) = value_of(field) {
                    url.query_pairs_mut().append_pair(name, &v);
                }
            }
        }
    }
    // `query_pairs_mut` leaves a bare "?" behind when nothing was added.
    if url.query() == Some("") {
        url.set_query(None);
    }
    let body = request.body.as_ref().map(|b| expand_body(b, values));
    Ok(Prepared { method: request.method, url, headers, body })
}

/// String values of a POST body with their placeholders filled; the JSON
/// serializer escapes them.
fn expand_body(v: &Value, values: &Values) -> Value {
    match v {
        Value::String(s) => match Template::parse(s, super::template::Context::Value) {
            Ok(t) => Value::String(t.expand(values, None, false).unwrap_or_default()),
            Err(_) => v.clone(),
        },
        Value::Array(a) => Value::Array(a.iter().map(|x| expand_body(x, values)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), expand_body(x, values))).collect()),
        other => other.clone(),
    }
}

pub enum Failure {
    /// An HTTP status other than 2xx (3xx included: redirects are not followed).
    Status(u16),
    /// No answer at all (offline, DNS, timeout): quiet, like the native pollers.
    Net(String),
    /// An answer we can't use (too big, not JSON).
    Coded(String),
}

pub async fn send(client: &reqwest::Client, prepared: Prepared) -> Result<Value, Failure> {
    let mut builder = match prepared.method {
        Method::Get => client.get(prepared.url),
        Method::Post => client.post(prepared.url),
    };
    for (name, value) in &prepared.headers {
        builder = builder.header(name, value);
    }
    if let Some(body) = &prepared.body {
        builder = builder.json(body);
    }
    let mut response = builder.send().await.map_err(|e| Failure::Net(integrations::no_connection(e)))?;
    let status = response.status();
    if !status.is_success() {
        return Err(Failure::Status(status.as_u16()));
    }
    let too_large = || Failure::Coded(errors::coded(errors::INT_TOO_LARGE, &[]));
    if response.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        return Err(too_large());
    }
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| Failure::Net(integrations::no_connection(e)))? {
        if bytes.len() + chunk.len() > MAX_BODY {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| Failure::Coded(errors::coded(errors::INT_BAD_RESPONSE, &[])))
}

/// The code for a non-2xx answer.
pub fn status_code_error(status: u16) -> String {
    match status {
        401 => errors::coded(errors::INT_INVALID_KEY, &[]),
        403 => errors::coded(errors::INT_KEY_ACCESS, &[]),
        404 => errors::coded(errors::INT_NOT_FOUND, &[]),
        429 => errors::coded(errors::INT_RATE_LIMITED, &[]),
        500..=599 => errors::coded(errors::INT_SERVER, &[&status.to_string()]),
        _ => errors::coded(errors::INT_HTTP, &[&status.to_string()]),
    }
}

// ── Mapping ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug)]
pub struct Mapped {
    pub count: u64,
    pub more: bool,
    pub open_url: Option<String>,
    /// Every item read, sorted; the payload keeps `list.max` of them.
    pub items: Vec<Item>,
}

impl Mapped {
    pub fn payload(&self, max: usize) -> Value {
        json!({
            "kind": "list",
            "count": self.count,
            "more": self.more,
            "openUrl": self.open_url,
            "items": self.items.iter().take(max).collect::<Vec<_>>(),
        })
    }
}

/// One line of service text for the island: whitespace collapsed, no control
/// or bidi characters, at most `max` characters (an ellipsis marks a cut).
pub fn clean(text: &str, max: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let shown: Vec<char> = joined.chars().filter(|c| crate::roadeep::chat::shown_char(*c)).collect();
    if shown.len() <= max {
        return shown.into_iter().collect();
    }
    let mut out: String = shown[..max - 1].iter().collect();
    out.push('…');
    out
}

/// A link for the island: http(s), on a `webHosts` entry, or nothing.
fn link(text: Option<String>, service: &Service, values: &Values) -> Option<String> {
    let url = reqwest::Url::parse(text?.trim()).ok()?;
    (url.as_str().len() <= 2000 && url_allowed(&url, &service.web_hosts, values)).then(|| url.to_string())
}

pub fn map_response(service: &Service, values: &Values, json: &Value) -> Result<Mapped, String> {
    let list = &service.list;
    let Some(array) = json.pointer(&list.path).and_then(Value::as_array) else {
        // GraphQL (and a few REST APIs) report failures in the body, with a 200.
        let message = json.pointer("/errors/0/message").or_else(|| json.pointer("/error/message")).and_then(Value::as_str);
        return Err(match message {
            Some(m) => errors::coded(errors::INT_API, &[&clean(m, 160)]),
            None => errors::coded(errors::INT_BAD_RESPONSE, &[]),
        });
    };

    let mut items: Vec<Item> = Vec::new();
    for raw in array.iter().take(MAX_SCANNED) {
        let Some(title) = text_at(raw, &list.title).map(|t| clean(&t, TITLE_MAX)).filter(|t| !t.is_empty()) else {
            continue;
        };
        let id = list.id.as_deref().and_then(|p| text_at(raw, p)).map(|id| clean(&id, 200)).unwrap_or_else(|| title.clone());
        let raw_status = list.status.as_deref().and_then(|p| text_at(raw, p));
        let status_text = raw_status.as_deref().map(|s| clean(s, STATUS_TEXT_MAX)).filter(|s| !s.is_empty());
        items.push(Item {
            id,
            subtitle: list.subtitle.as_deref().and_then(|p| text_at(raw, p)).map(|s| clean(&s, TITLE_MAX)).filter(|s| !s.is_empty()),
            status: service.map_status(raw_status.as_deref()),
            status_text,
            time: list.time.as_deref().and_then(|p| raw.pointer(p)).and_then(parse_time),
            url: list.url.as_ref().and_then(|t| link(t.expand(values, Some(raw), true), service, values)),
            title,
        });
    }
    if list.sort_time {
        // Stable: items without a time keep their order, after the dated ones.
        items.sort_by(|a, b| b.time.unwrap_or(i64::MIN).cmp(&a.time.unwrap_or(i64::MIN)));
    }

    let counted = service.count.as_deref().and_then(|p| json.pointer(p)).and_then(|v| match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    });
    let count = counted.unwrap_or(array.len() as u64);
    // Without a total, a full page means there are probably more.
    let more = counted.is_none() && page_is_full(service, array.len());
    let open_url = service.open_url.as_ref().and_then(|t| link(t.expand(values, None, true), service, values));
    Ok(Mapped { count, more, open_url, items })
}

/// Page-size parameters, as the common APIs name them.
const PAGE_SIZE_NAMES: &[&str] =
    &["limit", "per_page", "perpage", "page_size", "pagesize", "maxresults", "max_results", "first", "size", "count", "top", "$top"];

fn page_is_full(service: &Service, len: usize) -> bool {
    let literal = |name: &str, t: &Template| -> Option<usize> {
        PAGE_SIZE_NAMES.contains(&name.to_ascii_lowercase().as_str()).then_some(())?;
        match t.parts.as_slice() {
            [super::template::Part::Lit(s)] => s.trim().parse().ok(),
            _ => None,
        }
    };
    let from_query = service.request.query.iter().filter_map(|(n, t)| literal(n, t));
    let from_body = service.request.body.as_ref().and_then(Value::as_object).into_iter().flat_map(|o| {
        o.iter()
            .filter(|(k, _)| PAGE_SIZE_NAMES.contains(&k.to_ascii_lowercase().as_str()))
            .filter_map(|(_, v)| v.as_u64().map(|n| n as usize))
    });
    from_query.chain(from_body).any(|size| size > 0 && len >= size)
}

// ── Time ──────────────────────────────────────────────────────────────────────

/// RFC 3339 (lenient: space for T, any fraction, no offset = UTC), a plain
/// YYYY-MM-DD, or unix time (seconds below 1e12, else milliseconds), as ms.
pub fn parse_time(v: &Value) -> Option<i64> {
    let unix = |n: f64| -> Option<i64> {
        (n.is_finite() && n > 0.0).then_some(if n < 1e12 { (n * 1000.0) as i64 } else { n as i64 })
    };
    match v {
        Value::Number(n) => unix(n.as_f64()?),
        Value::String(s) => {
            let s = s.trim();
            if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return unix(s.parse().ok()?);
            }
            parse_datetime(s).filter(|ms| *ms > 0)
        }
        _ => None,
    }
}

fn digits(s: &str, at: usize, len: usize) -> Option<i64> {
    let part = s.get(at..at + len)?;
    part.bytes().all(|b| b.is_ascii_digit()).then(|| part.parse().ok())?
}

fn parse_datetime(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (year, month, day) = (digits(s, 0, 4)?, digits(s, 5, 2)?, digits(s, 8, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut ms = days_from_civil(year, month, day) * 86_400_000;
    if b.len() == 10 {
        return Some(ms);
    }
    if !matches!(b[10], b'T' | b't' | b' ') || b.len() < 16 || b[13] != b':' {
        return None;
    }
    let (hour, minute) = (digits(s, 11, 2)?, digits(s, 14, 2)?);
    let mut i = 16;
    let mut second = 0;
    if b.get(i) == Some(&b':') {
        second = digits(s, i + 1, 2)?;
        i += 3;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut frac_ms = 0;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let frac = &s[start..i];
        if frac.is_empty() {
            return None;
        }
        frac_ms = format!("{:0<3}", &frac[..frac.len().min(3)]).parse::<i64>().ok()?;
    }
    let offset_min = match b.get(i) {
        None => 0,
        Some(b'Z' | b'z') if i + 1 == b.len() => 0,
        Some(sign @ (b'+' | b'-')) => {
            let rest = &s[i + 1..];
            let (h, m) = match rest.len() {
                5 if rest.as_bytes()[2] == b':' => (digits(rest, 0, 2)?, digits(rest, 3, 2)?),
                4 => (digits(rest, 0, 2)?, digits(rest, 2, 2)?),
                2 => (digits(rest, 0, 2)?, 0),
                _ => return None,
            };
            let total = h * 60 + m;
            if *sign == b'+' { total } else { -total }
        }
        _ => return None,
    };
    ms += ((hour * 60 + minute - offset_min) * 60 + second) * 1000 + frac_ms;
    Some(ms)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ── Notify ────────────────────────────────────────────────────────────────────

/// What a service's items looked like at the last successful poll: id → mapped
/// status. `None` until the first poll, which only fills the card.
pub type Seen = Option<HashMap<String, &'static str>>;

/// The event this poll raises, if any, and the state to keep for the next one.
pub fn notify(service: &Service, seen: &mut Seen, items: &[Item]) -> Option<IntegrationEvent> {
    let rule = service.notify.as_ref()?;
    let wanted = |status: &str| rule.statuses.is_empty() || rule.statuses.contains(&status);
    let event = seen.as_ref().and_then(|before| {
        let hit = items.iter().find(|item| {
            let changed = match rule.on {
                NotifyOn::New => !before.contains_key(&item.id),
                NotifyOn::Status => before.get(&item.id).is_some_and(|old| *old != item.status),
            };
            changed && wanted(item.status)
        })?;
        Some(IntegrationEvent {
            success: !matches!(hit.status, "warn" | "err"),
            label: hit.title.clone(),
            detail: hit.status_text.clone(),
        })
    });
    let map = seen.get_or_insert_with(HashMap::new);
    // Ids that scrolled off the page stay known, so their return isn't "new".
    if map.len() + items.len() > SEEN_CAP {
        map.clear();
    }
    for item in items {
        map.insert(item.id.clone(), item.status);
    }
    event
}

static SEEN: LazyLock<Mutex<HashMap<String, Seen>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

// ── Polling ───────────────────────────────────────────────────────────────────

fn language(app: &AppHandle) -> String {
    app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().language.clone()).unwrap_or_else(|| "fa".into())
}

/// One poll of one catalog service (via integrations::poll_once, which records
/// the outcome). Failures with an answer go to the card; a network blip only
/// to the caller, so the island doesn't flash an error for it.
pub async fn poll(app: &AppHandle, service: &'static Service) -> PollResult {
    let id: &'static str = &service.pill;
    let fail = |error: String| integrations::fail(app, id, error);
    let values = match read_values(service, &language(app), secrets::get) {
        Ok(values) => values,
        Err(e) if e == errors::coded(errors::INT_NO_KEY, &[]) => return Err(e),
        Err(e) => return Err(fail(e)),
    };
    let prepared = prepare(service, &values).map_err(fail)?;
    let client = CLIENT.as_ref().map_err(|e| errors::coded(errors::INT_NO_CONNECTION, &[e.as_str()]))?;
    let json = match send(client, prepared).await {
        Ok(json) => json,
        Err(Failure::Net(e)) => return Err(e),
        Err(Failure::Status(code)) => return Err(fail(status_code_error(code))),
        Err(Failure::Coded(e)) => return Err(fail(e)),
    };
    let mapped = map_response(service, &values, &json).map_err(fail)?;
    let event = {
        let mut seen = SEEN.lock().unwrap();
        notify(service, seen.entry(service.id.clone()).or_default(), &mapped.items)
    };
    integrations::emit(app, IntegrationUpdate { id, data: mapped.payload(service.list.max), error: None, event });
    Ok(())
}

/// The one scheduler for every catalog service: every 15 s it polls each
/// active one whose `pollEvery` has passed. Paused or switched off means no
/// request at all, as for the native pollers.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last: HashMap<&'static str, Instant> = HashMap::new();
        let mut ticker = tokio::time::interval(TICK);
        // The first tick fires at once; the first poll waits one tick, like
        // the native pollers' start-up delays.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if integrations::PAUSED.load(Ordering::Relaxed) {
                continue;
            }
            let active: Vec<String> = match app.try_state::<crate::Shared>() {
                Some(shared) => shared.settings.lock().unwrap().active_integrations.clone(),
                None => continue,
            };
            for pill in active {
                let Some(service) = super::get().by_pill(&pill) else { continue };
                let due = last.get(service.pill.as_str()).is_none_or(|at| at.elapsed() >= Duration::from_secs(service.poll_every));
                if !due {
                    continue;
                }
                last.insert(&service.pill, Instant::now());
                let app = app.clone();
                // Its own task: one slow service never delays the others. A
                // poll ends within TIMEOUT, far below the shortest pollEvery.
                tauri::async_runtime::spawn(async move {
                    let _ = integrations::poll_once(app, &service.pill).await;
                });
            }
        }
    });
    log::line("catalog: scheduler started");
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;

    fn store(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn catalog_values_are_checked_against_their_pattern() {
        let catalog = fixtures::catalog();
        let supa = catalog.by_pill("integration_supa").unwrap();
        assert_eq!(read_values(supa, "en", store(&[])).unwrap_err(), errors::coded(errors::INT_NO_KEY, &[]));
        let v = read_values(supa, "en", store(&[("x.supa.token", "  sbp_0123456789abc \n")])).unwrap();
        assert_eq!(v["token"], "sbp_0123456789abc");
        assert!(!v.contains_key("org"), "optional and absent");
        for bad in ["short", "sbp_0123456789\r\nX-Evil: 1", "sbp_01234 56789abc"] {
            assert_eq!(
                read_values(supa, "en", store(&[("x.supa.token", bad)])).unwrap_err(),
                errors::coded(errors::INT_BAD_FIELD, &["Token"]),
                "{bad:?}"
            );
        }
        assert_eq!(
            read_values(supa, "fa", store(&[("x.supa.token", "sbp_0123456789abc"), ("x.supa.org", "Bad Org")])).unwrap_err(),
            errors::coded(errors::INT_BAD_FIELD, &["سازمان"])
        );

        let woo = catalog.by_pill("integration_shop-woo").unwrap();
        let key = format!("ck_{}", "a".repeat(40));
        let secret = format!("cs_{}", "b".repeat(40));
        let with_site = |site: &str| {
            read_values(woo, "en", store(&[("x.shop-woo.siteUrl", site), ("x.shop-woo.key", &key), ("x.shop-woo.secret", &secret)]))
        };
        assert_eq!(with_site("https://shop.example.com/").unwrap()["siteUrl"], "https://shop.example.com");
        for bad in ["https://user:pw@shop.example.com", "https://shop.example.com/?a=1", "https://shop.example.com/#x"] {
            assert_eq!(with_site(bad).unwrap_err(), errors::coded(errors::INT_BAD_URL, &[]), "{bad}");
        }
    }

    #[test]
    fn catalog_requests_carry_auth_and_encoded_query() {
        let catalog = fixtures::catalog();
        let supa = catalog.by_pill("integration_supa").unwrap();
        let values = read_values(supa, "en", store(&[("x.supa.token", "sbp_0123456789abc"), ("x.supa.org", "my-org")])).unwrap();
        let p = prepare(supa, &values).unwrap();
        assert_eq!(p.method, Method::Get);
        assert_eq!(p.url.as_str(), "https://api.supa.example.com/v1/projects?org=my-org&per_page=3");
        assert!(p.headers.contains(&("Accept".into(), "application/json".into())));
        assert!(p.headers.contains(&("Authorization".into(), "Bearer sbp_0123456789abc".into())));

        let values = read_values(supa, "en", store(&[("x.supa.token", "sbp_0123456789abc")])).unwrap();
        assert_eq!(prepare(supa, &values).unwrap().url.as_str(), "https://api.supa.example.com/v1/projects?per_page=3");

        let jiro = catalog.by_pill("integration_jiro").unwrap();
        let values = read_values(jiro, "en", store(&[("x.jiro.site", "acme"), ("x.jiro.email", "a@b.co"), ("x.jiro.token", "tok12345678")])).unwrap();
        let p = prepare(jiro, &values).unwrap();
        assert_eq!(p.url.as_str(), "https://acme.atlassian.net/rest/api/3/search");
        let basic = format!("Basic {}", crate::util::base64_for(b"a@b.co:tok12345678"));
        assert!(p.headers.contains(&("Authorization".into(), basic)));
        assert_eq!(p.body.unwrap()["jql"], "assignee = currentUser() AND project = \"acme\"");

        let woo = catalog.by_pill("integration_shop-woo").unwrap();
        let mut values = Values::new();
        values.insert("siteUrl".into(), "https://shop.example.com".into());
        values.insert("key".into(), "ck_1 &x".into());
        values.insert("secret".into(), "cs".into());
        let p = prepare(woo, &values).unwrap();
        assert_eq!(p.url.as_str(), "https://shop.example.com/wp-json/wc/v3/orders?x=ck_1+%26x");
    }

    #[test]
    fn catalog_requests_never_leave_the_declared_hosts() {
        let catalog = fixtures::catalog();
        let jiro = catalog.by_pill("integration_jiro").unwrap();
        // Values that pass no pattern still can't steer the request: each check stands alone.
        for site in ["evil.com#", "evil.com/x", "a.b"] {
            let mut values = Values::new();
            values.insert("site".into(), site.into());
            assert!(prepare(jiro, &values).is_err(), "{site}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn catalog_redirects_are_not_followed() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await;
            let reply = "HTTP/1.1 302 Found\r\nLocation: https://elsewhere.example.com/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        let client = build_client().unwrap();
        let prepared = Prepared {
            method: Method::Get,
            url: reqwest::Url::parse(&format!("http://127.0.0.1:{port}/list")).unwrap(),
            headers: vec![("Authorization".into(), "Bearer secret".into())],
            body: None,
        };
        match send(&client, prepared).await {
            Err(Failure::Status(302)) => {}
            Err(Failure::Status(s)) => panic!("status {s}"),
            Err(Failure::Net(e) | Failure::Coded(e)) => panic!("{e}"),
            Ok(v) => panic!("followed: {v}"),
        }
        server.await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn catalog_large_answers_are_refused() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await;
            // No Content-Length: the cap has to hold while reading.
            let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n[").await;
            let chunk = vec![b'1'; 64 * 1024];
            for _ in 0..40 {
                if socket.write_all(&chunk).await.is_err() {
                    break;
                }
            }
        });
        let prepared = Prepared {
            method: Method::Get,
            url: reqwest::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap(),
            headers: vec![],
            body: None,
        };
        match send(&build_client().unwrap(), prepared).await {
            Err(Failure::Coded(e)) => assert_eq!(e, errors::coded(errors::INT_TOO_LARGE, &[])),
            _ => panic!("expected E_INT_TOO_LARGE"),
        }
    }

    #[test]
    fn catalog_status_codes_have_their_own_errors() {
        assert_eq!(status_code_error(401), "E_INT_INVALID_KEY");
        assert_eq!(status_code_error(403), "E_INT_KEY_ACCESS");
        assert_eq!(status_code_error(404), "E_INT_NOT_FOUND");
        assert_eq!(status_code_error(429), "E_INT_RATE_LIMITED");
        assert_eq!(status_code_error(503), "E_INT_SERVER|503");
        assert_eq!(status_code_error(302), "E_INT_HTTP|302");
    }

    fn supa_answer() -> Value {
        json!({
            "total": 41,
            "data": [
                { "id": "p1", "name": "Old", "region": "eu", "status": "ACTIVE_HEALTHY", "created_at": "2024-01-01T00:00:00Z" },
                { "id": "p2", "name": "  New\u{202E}  project \n", "status": "coming_up", "created_at": 1717200000 },
                { "id": "p3", "name": "Undated", "status": "PAUSED" },
                { "id": "p4", "name": "" },
                { "id": 5, "name": "Mid", "status": "ACTIVE_HEALTHY", "created_at": "2024-03-01" }
            ]
        })
    }

    #[test]
    fn catalog_answers_map_to_the_list_payload() {
        let catalog = fixtures::catalog();
        let supa = catalog.by_pill("integration_supa").unwrap();
        let mapped = map_response(supa, &Values::new(), &supa_answer()).unwrap();
        let ids: Vec<&str> = mapped.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["p2", "5", "p1", "p3"], "newest first, undated last, untitled dropped");
        let new = &mapped.items[0];
        assert_eq!(new.title, "New project", "bidi and control characters stripped");
        assert_eq!(new.status, "info");
        assert_eq!(new.status_text.as_deref(), Some("coming_up"));
        assert_eq!(new.time, Some(1_717_200_000_000));
        assert_eq!(new.url.as_deref(), Some("https://supa.example.com/dashboard/project/p2"));
        assert_eq!(mapped.items[2].subtitle.as_deref(), Some("eu"));
        assert_eq!(mapped.items[3].status, "warn", "unmapped → \"*\"");
        assert_eq!(mapped.count, 41);
        assert!(!mapped.more);
        assert_eq!(mapped.open_url.as_deref(), Some("https://supa.example.com/dashboard/projects"));

        let payload = mapped.payload(supa.list.max);
        assert_eq!(payload["kind"], "list");
        assert_eq!(payload["items"].as_array().unwrap().len(), 2);
        assert_eq!(payload["items"][0]["statusText"], "coming_up");
        assert!(payload["items"][0].get("subtitle").is_none());
        assert_eq!(payload["openUrl"], "https://supa.example.com/dashboard/projects");
    }

    #[test]
    fn catalog_counts_default_to_the_page_and_flag_more() {
        let catalog = fixtures::catalog();
        let supa = catalog.by_pill("integration_supa").unwrap();
        // per_page=3 and three items, no total: probably more.
        let page = json!({ "data": [ { "id": 1, "name": "a" }, { "id": 2, "name": "b" }, { "id": 3, "name": "c" } ] });
        let mapped = map_response(supa, &Values::new(), &page).unwrap();
        assert_eq!((mapped.count, mapped.more), (3, true));
        let short = json!({ "data": [ { "id": 1, "name": "a" } ], "total": "7" });
        let mapped = map_response(supa, &Values::new(), &short).unwrap();
        assert_eq!((mapped.count, mapped.more), (7, false));
    }

    #[test]
    fn catalog_status_map_ignores_case_and_defaults_to_info() {
        let catalog = fixtures::catalog();
        let jiro = catalog.by_pill("integration_jiro").unwrap();
        assert_eq!(jiro.map_status(Some("DONE")), "ok");
        assert_eq!(jiro.map_status(Some("blocked")), "err");
        assert_eq!(jiro.map_status(Some("Something else")), "info");
        assert_eq!(jiro.map_status(None), "info");
    }

    #[test]
    fn catalog_bad_answers_and_body_errors() {
        let catalog = fixtures::catalog();
        let supa = catalog.by_pill("integration_supa").unwrap();
        assert_eq!(map_response(supa, &Values::new(), &json!({ "nope": 1 })).unwrap_err(), "E_INT_BAD_RESPONSE");
        let gql = json!({ "errors": [ { "message": "Field 'x' doesn't exist" } ] });
        assert_eq!(map_response(supa, &Values::new(), &gql).unwrap_err(), "E_INT_API|Field 'x' doesn't exist");
    }

    #[test]
    fn catalog_item_links_must_stay_on_web_hosts() {
        let catalog = fixtures::catalog();
        let jiro = catalog.by_pill("integration_jiro").unwrap();
        let mut values = Values::new();
        values.insert("site".into(), "acme".into());
        let answer = json!({ "issues": [ { "id": "1", "key": "AB-1", "fields": { "summary": "Fix", "status": { "name": "Done" } } } ] });
        let mapped = map_response(jiro, &values, &answer).unwrap();
        assert_eq!(mapped.items[0].url.as_deref(), Some("https://acme.atlassian.net/browse/AB-1"));
        assert_eq!(mapped.items[0].status, "ok");
        assert_eq!(mapped.open_url.as_deref(), Some("https://acme.atlassian.net/jira"));

        let woo = catalog.by_pill("integration_shop-woo").unwrap();
        let mut values = Values::new();
        values.insert("siteUrl".into(), "https://shop.example.com".into());
        let answer = json!([
            { "id": 1, "number": "1001", "links": { "self": "https://shop.example.com/order/1" } },
            { "id": 2, "number": "1002", "links": { "self": "https://phish.example.net/order/2" } },
            { "id": 3, "number": "1003", "links": { "self": "javascript:alert(1)" } }
        ]);
        let mapped = map_response(woo, &values, &answer).unwrap();
        let urls: Vec<Option<&str>> = mapped.items.iter().map(|i| i.url.as_deref()).collect();
        assert_eq!(urls, [Some("https://shop.example.com/order/1"), None, None]);
        assert_eq!(mapped.open_url.as_deref(), Some("https://shop.example.com/wp-admin/edit.php?post_type=shop_order"));
    }

    #[test]
    fn catalog_times_parse_in_every_documented_form() {
        let t = |v: Value| parse_time(&v);
        assert_eq!(t(json!("1970-01-01T00:00:00Z")), None, "zero is no time");
        assert_eq!(t(json!("2024-01-01T00:00:00Z")), Some(1_704_067_200_000));
        assert_eq!(t(json!("2024-01-01T01:30:00+01:30")), Some(1_704_067_200_000));
        assert_eq!(t(json!("2024-01-01 00:00:00.5")), Some(1_704_067_200_500));
        assert_eq!(t(json!("2024-01-01T00:00:00.123456-00:00")), Some(1_704_067_200_123));
        assert_eq!(t(json!("2024-01-01T00:00")), Some(1_704_067_200_000));
        assert_eq!(t(json!("2024-01-01")), Some(1_704_067_200_000));
        assert_eq!(t(json!(1_704_067_200)), Some(1_704_067_200_000));
        assert_eq!(t(json!(1_704_067_200_123u64)), Some(1_704_067_200_123));
        assert_eq!(t(json!("1704067200")), Some(1_704_067_200_000));
        assert_eq!(t(json!(1_704_067_200.5)), Some(1_704_067_200_500));
        for bad in [json!("yesterday"), json!("2024-13-01"), json!("2024-01-01T25:00:00Z"), json!("2024-01-01Tx"), json!(null), json!(-5), json!("2024-01-01T00:00:00Zjunk")] {
            assert_eq!(t(bad.clone()), None, "{bad}");
        }
    }

    fn item(id: &str, status: &'static str) -> Item {
        Item { id: id.into(), title: format!("T{id}"), subtitle: None, status, status_text: Some(status.into()), time: None, url: None }
    }

    #[test]
    fn catalog_notify_on_new_items() {
        let catalog = fixtures::catalog();
        let supa = catalog.by_pill("integration_supa").unwrap(); // on new, statuses err|warn
        let mut seen: Seen = None;
        assert!(notify(supa, &mut seen, &[item("a", "err")]).is_none(), "the first poll only fills the card");
        assert!(notify(supa, &mut seen, &[item("a", "err")]).is_none());
        assert!(notify(supa, &mut seen, &[item("b", "ok"), item("a", "err")]).is_none(), "ok is not a wanted status");
        let event = notify(supa, &mut seen, &[item("c", "warn"), item("b", "ok")]).unwrap();
        assert_eq!((event.label.as_str(), event.detail.as_deref(), event.success), ("Tc", Some("warn"), false));
        // "a" scrolled off and comes back: not new.
        assert!(notify(supa, &mut seen, &[item("a", "err")]).is_none());
    }

    #[test]
    fn catalog_notify_on_status_changes() {
        let catalog = fixtures::catalog();
        let jiro = catalog.by_pill("integration_jiro").unwrap(); // on status, statuses err
        let mut seen: Seen = None;
        assert!(notify(jiro, &mut seen, &[item("a", "info")]).is_none());
        assert!(notify(jiro, &mut seen, &[item("a", "info"), item("b", "err")]).is_none(), "a new item is not a change");
        assert!(notify(jiro, &mut seen, &[item("a", "ok"), item("b", "err")]).is_none(), "ok is not wanted");
        let event = notify(jiro, &mut seen, &[item("a", "err")]).unwrap();
        assert_eq!((event.label.as_str(), event.success), ("Ta", false));

        let woo = catalog.by_pill("integration_shop-woo").unwrap(); // no notify
        let mut seen: Seen = None;
        notify(woo, &mut seen, &[item("a", "ok")]);
        assert!(notify(woo, &mut seen, &[item("b", "ok")]).is_none());
    }

    #[test]
    fn catalog_text_is_one_clean_line() {
        assert_eq!(clean("a\u{200F}b\u{2066}c\td\u{0007}", 10), "abc d");
        assert_eq!(clean("می\u{200C}خواهم", 10), "می\u{200C}خواهم", "ZWNJ stays");
        assert_eq!(clean(&"x".repeat(50), 40).chars().count(), 40);
        assert!(clean(&"x".repeat(50), 40).ends_with('…'));
    }
}
