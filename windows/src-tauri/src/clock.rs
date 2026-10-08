//! The current date and time for the assistants: the system clock, corrected by the HTTP `Date`
//! header of a service the user already configured (OpenAI when a voice key is saved, else the
//! Roadeep API when signed in). No other host is ever contacted; offline it is the system clock.
use crate::planner::jalali::{civil_from_days, days_from_civil, to_jalali, weekday, WEEKDAYS_FA};
use crate::planner::schedule::{SystemZone, Zone, DAY, HOUR, MINUTE};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Manager;

const ONLINE_TTL: Duration = Duration::from_secs(10 * 60);
/// After a failed check, the system clock is used without retrying for this long.
const FAILURE_BACKOFF: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);
/// Below this, the difference is the Date header's one-second resolution plus latency.
const NOISE_MS: i64 = 2_000;
const SKEW_LOG_MS: i64 = 5 * 60 * 1000;
const OPENAI_URL: &str = "https://api.openai.com/v1";

const MONTHS_FA: [&str; 12] = [
    "فروردین", "اردیبهشت", "خرداد", "تیر", "مرداد", "شهریور", "مهر", "آبان", "آذر", "دی", "بهمن", "اسفند",
];
const GREGORIAN_MONTHS_FA: [&str; 12] = [
    "ژانویه", "فوریه", "مارس", "آوریل", "مه", "ژوئن", "ژوئیه", "اوت", "سپتامبر", "اکتبر", "نوامبر", "دسامبر",
];
/// Saturday first, like planner::jalali::weekday.
const WEEKDAYS_EN: [&str; 7] = ["Saturday", "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday"];

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Ymd {
    pub y: i64,
    pub m: u32,
    pub d: u32,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JalaliDate {
    pub y: i64,
    pub m: u32,
    pub d: u32,
    pub month_name_fa: &'static str,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Online,
    System,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Clock {
    pub epoch_ms: i64,
    /// Local time with its offset, e.g. "2026-10-07T14:05:09+03:30".
    pub iso_local: String,
    pub utc_offset_minutes: i64,
    pub timezone_name: Option<String>,
    pub jalali: JalaliDate,
    pub gregorian: Ymd,
    pub weekday_fa: &'static str,
    pub weekday_en: &'static str,
    pub source: Source,
    pub online_host: Option<String>,
    /// online − system, in seconds (online only).
    pub skew_seconds: Option<i64>,
}

fn offset_text(minutes: i64) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let m = minutes.abs();
    format!("{sign}{:02}:{:02}", m / 60, m % 60)
}
fn build(
    utc_ms: i64,
    offset_ms: i64,
    timezone_name: Option<String>,
    online: Option<(&str, i64)>,
) -> Clock {
    let local = utc_ms + offset_ms;
    let (days, tod) = (local.div_euclid(DAY), local.rem_euclid(DAY));
    let (y, m, d) = civil_from_days(days);
    let (jy, jm, jd) = to_jalali(y, m, d).unwrap_or((0, 1, 1));
    let minutes = offset_ms.div_euclid(MINUTE);
    let wd = weekday(days) as usize;
    Clock {
        epoch_ms: utc_ms,
        iso_local: format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{}",
            tod / HOUR,
            (tod / MINUTE) % 60,
            (tod / 1000) % 60,
            offset_text(minutes)
        ),
        utc_offset_minutes: minutes,
        timezone_name,
        jalali: JalaliDate { y: jy, m: jm, d: jd, month_name_fa: MONTHS_FA[(jm as usize).clamp(1, 12) - 1] },
        gregorian: Ymd { y, m, d },
        weekday_fa: WEEKDAYS_FA[wd],
        weekday_en: WEEKDAYS_EN[wd],
        source: if online.is_some() { Source::Online } else { Source::System },
        online_host: online.map(|(h, _)| h.to_string()),
        skew_seconds: online.map(|(_, skew)| skew / 1000),
    }
}

fn fa_digits(text: &str) -> String {
    text.chars()
        .map(|c| match c.to_digit(10) {
            Some(n) => char::from_u32(0x06F0 + n).unwrap_or(c),
            None => c,
        })
        .collect()
}
/// One Persian line for the assistants' instructions.
pub fn persian_line(c: &Clock) -> String {
    let time = &c.iso_local[11..16];
    let source = match c.source {
        Source::Online => "تأییدشده آنلاین",
        Source::System => "طبق ساعت سیستم",
    };
    let zone = match &c.timezone_name {
        Some(name) => format!("{} ({name})", offset_text(c.utc_offset_minutes)),
        None => offset_text(c.utc_offset_minutes),
    };
    format!(
        "امروز {} {} {} {} ({} {} {})، ساعت {}، منطقهٔ زمانی {} — {}. (ISO: {}) \
همهٔ تاریخ‌های نسبی مثل «امروز»، «فردا» و «هفتهٔ بعد» را از همین تاریخ حساب کن. \
برای ساعت و تاریخ دقیق در ادامهٔ گفتگو ابزار get_datetime را صدا بزن.",
        c.weekday_fa,
        fa_digits(&c.jalali.d.to_string()),
        c.jalali.month_name_fa,
        fa_digits(&c.jalali.y.to_string()),
        fa_digits(&c.gregorian.d.to_string()),
        GREGORIAN_MONTHS_FA[(c.gregorian.m as usize).clamp(1, 12) - 1],
        fa_digits(&c.gregorian.y.to_string()),
        fa_digits(time),
        zone,
        source,
        c.iso_local,
    )
}
/// One English line for the Roadeep chat message.
pub fn chat_line(c: &Clock) -> String {
    format!(
        "{CHAT_LINE_PREFIX}now {} {}{} ({}), Jalali {:04}/{:02}/{:02}, source {}]\n\n",
        &c.iso_local[..10],
        &c.iso_local[11..16],
        offset_text(c.utc_offset_minutes),
        c.weekday_en,
        c.jalali.y,
        c.jalali.m,
        c.jalali.d,
        if c.source == Source::Online { "online" } else { "system" },
    )
}
pub const CHAT_LINE_PREFIX: &str = "[Roadeep clock: ";

/// IMF-fixdate (RFC 9110 §5.6.7), e.g. "Wed, 07 Oct 2026 10:35:00 GMT", to epoch ms.
pub fn parse_http_date(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.split_ascii_whitespace().collect();
    let [_, day, month, year, time, "GMT"] = parts.as_slice() else { return None };
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|m| m == month)? as u32
        + 1;
    let num = |s: &str, len: usize| (s.len() == len && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u32>().ok()).flatten();
    let (d, y) = (num(day, 2)?, num(year, 4)?);
    let t: Vec<&str> = time.split(':').collect();
    let [h, mi, s] = t.as_slice() else { return None };
    let (h, mi, s) = (num(h, 2)?, num(mi, 2)?, num(s, 2)?);
    if !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 || y < 2000 {
        return None;
    }
    let days = days_from_civil(i64::from(y), month, d);
    if civil_from_days(days) != (i64::from(y), month, d) {
        return None;
    }
    Some(days * DAY + i64::from(h) * HOUR + i64::from(mi) * MINUTE + i64::from(s.min(59)) * 1000)
}

#[derive(Clone, Debug, PartialEq)]
struct Target {
    url: String,
    host: String,
    /// Roadeep goes direct unless the user opted into the proxy (roadeep::http::proxy_policy).
    roadeep: bool,
}
/// Only services the user configured: OpenAI with a saved voice key, else the Roadeep API when signed in.
fn target(voice_key: bool, roadeep_base: Option<&str>) -> Option<Target> {
    let (url, roadeep) = if voice_key {
        (OPENAI_URL.to_string(), false)
    } else {
        (format!("{}/", roadeep_base?.trim_end_matches('/')), true)
    };
    let host = reqwest::Url::parse(&url).ok()?.host_str()?.to_string();
    Some(Target { url, host, roadeep })
}
fn system_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
/// online − system in ms, from one HEAD request's Date header (no body is read, no credentials sent).
async fn fetch_offset(target: &Target, https_only: bool) -> Result<i64, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(https_only);
    if target.roadeep {
        builder = crate::roadeep::http::proxy_policy(builder);
    }
    let client = builder.build().map_err(|_| "clock client unavailable".to_string())?;
    let before = system_ms();
    let response = client
        .head(&target.url)
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "timeout" } else { "unreachable" }.to_string())?;
    let after = system_ms();
    let server = response
        .headers()
        .get(reqwest::header::DATE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_http_date)
        .ok_or("no usable Date header")?;
    // The header is truncated to the second: its middle is the better estimate.
    let offset = server + 500 - (before + after) / 2;
    Ok(if offset.abs() <= NOISE_MS { 0 } else { offset })
}

#[derive(Clone, Debug)]
struct Online {
    offset_ms: i64,
    host: String,
    at: Instant,
}
#[derive(Default)]
struct Cache {
    online: Mutex<Option<Online>>,
    failed_at: Mutex<Option<Instant>>,
    refreshing: AtomicBool,
}
enum Lookup {
    Fresh(Online),
    BackingOff,
    Stale,
}
impl Cache {
    fn lookup(&self, now: Instant) -> Lookup {
        if let Some(o) = self.online.lock().unwrap().clone().filter(|o| now.duration_since(o.at) < ONLINE_TTL) {
            return Lookup::Fresh(o);
        }
        if self.failed_at.lock().unwrap().is_some_and(|t| now.duration_since(t) < FAILURE_BACKOFF) {
            return Lookup::BackingOff;
        }
        Lookup::Stale
    }
    fn store(&self, result: Result<Online, ()>, now: Instant) {
        match result {
            Ok(o) => {
                *self.online.lock().unwrap() = Some(o);
                *self.failed_at.lock().unwrap() = None;
            }
            Err(()) => *self.failed_at.lock().unwrap() = Some(now),
        }
    }
}
static CACHE: std::sync::LazyLock<Cache> = std::sync::LazyLock::new(Cache::default);

fn current_target(app: &tauri::AppHandle) -> Option<Target> {
    let roadeep = app.state::<crate::roadeep::Roadeep>();
    let signed_in = roadeep.session.has_session();
    target(crate::voice::key_configured(), signed_in.then(|| roadeep.transport.base()))
}
async fn refresh(app: &tauri::AppHandle) -> Option<Online> {
    let target = current_target(app)?;
    let started = Instant::now();
    let result = fetch_offset(&target, true).await;
    match &result {
        Ok(offset) if offset.abs() > SKEW_LOG_MS => crate::log::line(format!(
            "clock: system clock differs from {} by {} s; using online time",
            target.host,
            offset / 1000
        )),
        Ok(_) => {}
        Err(reason) => crate::log::line(format!(
            "clock: online check via {} failed ({reason}, {} ms); using system clock",
            target.host,
            started.elapsed().as_millis()
        )),
    }
    let online = result.ok().map(|offset_ms| Online { offset_ms, host: target.host, at: Instant::now() });
    CACHE.store(online.clone().ok_or(()), Instant::now());
    online
}
fn timezone_name() -> Option<String> {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut info = TIME_ZONE_INFORMATION::default();
    // SAFETY: a live, writable TIME_ZONE_INFORMATION.
    if unsafe { GetTimeZoneInformation(&mut info) } == u32::MAX {
        return None;
    }
    let len = info.StandardName.iter().position(|&c| c == 0).unwrap_or(info.StandardName.len());
    let name = String::from_utf16_lossy(&info.StandardName[..len]).trim().to_string();
    (!name.is_empty()).then(|| name.chars().filter(|c| !c.is_control()).take(64).collect())
}
fn clock_from(online: Option<&Online>) -> Clock {
    let system = system_ms();
    let utc = system + online.map_or(0, |o| o.offset_ms);
    build(utc, SystemZone.offset_ms(utc), timezone_name(), online.map(|o| (o.host.as_str(), o.offset_ms)))
}
/// The verified clock: waits at most a few seconds for an online check when the cache is stale.
pub async fn now(app: &tauri::AppHandle) -> Clock {
    let online = match CACHE.lookup(Instant::now()) {
        Lookup::Fresh(o) => Some(o),
        Lookup::BackingOff => None,
        Lookup::Stale => refresh(app).await,
    };
    clock_from(online.as_ref())
}
/// Never waits on the network: the cached online offset, else the system clock with a
/// background refresh for the next caller.
pub fn now_cached(app: &tauri::AppHandle) -> Clock {
    match CACHE.lookup(Instant::now()) {
        Lookup::Fresh(o) => clock_from(Some(&o)),
        lookup => {
            if matches!(lookup, Lookup::Stale) && !CACHE.refreshing.swap(true, Ordering::SeqCst) {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    refresh(&app).await;
                    CACHE.refreshing.store(false, Ordering::SeqCst);
                });
            }
            clock_from(None)
        }
    }
}
#[tauri::command]
pub async fn clock_now(app: tauri::AppHandle) -> Clock {
    now(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn http_dates_parse_strictly() {
        let ms = parse_http_date("Wed, 07 Oct 2026 10:35:09 GMT").unwrap();
        assert_eq!(ms, days_from_civil(2026, 10, 7) * DAY + 10 * HOUR + 35 * MINUTE + 9000);
        for bad in [
            "",
            "Wed, 07 Oct 2026 10:35:09 UTC",
            "Wed, 7 Oct 2026 10:35:09 GMT",
            "Wed, 31 Feb 2026 10:35:09 GMT",
            "Wed, 07 Okt 2026 10:35:09 GMT",
            "Wed, 07 Oct 2026 24:00:00 GMT",
            "Wednesday, 07-Oct-26 10:35:09 GMT",
        ] {
            assert_eq!(parse_http_date(bad), None, "{bad}");
        }
    }
    #[test]
    fn jalali_weekday_and_iso_are_correct() {
        let utc = days_from_civil(2026, 10, 7) * DAY + 10 * HOUR + 35 * MINUTE;
        let c = build(utc, 210 * MINUTE, Some("Iran Standard Time".into()), Some(("api.openai.com", 0)));
        assert_eq!((c.jalali.y, c.jalali.m, c.jalali.d), (1405, 7, 15));
        assert_eq!(c.jalali.month_name_fa, "مهر");
        assert_eq!(c.gregorian, Ymd { y: 2026, m: 10, d: 7 });
        assert_eq!((c.weekday_en, c.weekday_fa), ("Wednesday", "چهارشنبه"));
        assert_eq!(c.iso_local, "2026-10-07T14:05:00+03:30");
        assert_eq!(c.utc_offset_minutes, 210);
        assert_eq!(c.source, Source::Online);
        let line = persian_line(&c);
        assert!(line.starts_with("امروز چهارشنبه ۱۵ مهر ۱۴۰۵ (۷ اکتبر ۲۰۲۶)، ساعت ۱۴:۰۵، منطقهٔ زمانی +03:30"), "{line}");
        assert!(line.contains("تأییدشده آنلاین") && line.contains("get_datetime"));
        let chat = chat_line(&c);
        assert!(chat.starts_with(CHAT_LINE_PREFIX) && chat.contains("2026-10-07 14:05+03:30 (Wednesday), Jalali 1405/07/15, source online"), "{chat}");
        // Local midnight crossing and a negative zone.
        let late = build(days_from_civil(2026, 3, 20) * DAY + 21 * HOUR, 210 * MINUTE, None, None);
        assert_eq!((late.jalali.y, late.jalali.m, late.jalali.d), (1405, 1, 1));
        assert_eq!(late.source, Source::System);
        assert!(persian_line(&late).contains("طبق ساعت سیستم"));
        assert!(build(0, -300 * MINUTE, None, None).iso_local.ends_with("-05:00"));
    }
    #[test]
    fn only_user_configured_hosts_are_ever_contacted() {
        assert_eq!(target(false, None), None);
        let openai = target(true, Some("https://roadeep.com/api")).unwrap();
        assert_eq!((openai.host.as_str(), openai.roadeep), ("api.openai.com", false));
        let roadeep = target(false, Some("https://roadeep.com/api/")).unwrap();
        assert_eq!((roadeep.host.as_str(), roadeep.url.as_str(), roadeep.roadeep), ("roadeep.com", "https://roadeep.com/api/", true));
        // The base is the app's own compile-time Roadeep API; nothing else is reachable here.
        assert_eq!(target(false, Some(crate::roadeep::http::base_url())).unwrap().url, format!("{}/", crate::roadeep::http::base_url()));
    }
    #[test]
    fn cache_keeps_online_offset_ten_minutes_and_backs_off_failures() {
        let cache = Cache::default();
        let t0 = Instant::now();
        assert!(matches!(cache.lookup(t0), Lookup::Stale));
        cache.store(Err(()), t0);
        assert!(matches!(cache.lookup(t0 + Duration::from_secs(30)), Lookup::BackingOff));
        assert!(matches!(cache.lookup(t0 + Duration::from_secs(61)), Lookup::Stale));
        cache.store(Ok(Online { offset_ms: 7_000, host: "api.openai.com".into(), at: t0 }), t0);
        assert!(matches!(cache.lookup(t0 + Duration::from_secs(599)), Lookup::Fresh(o) if o.offset_ms == 7_000));
        assert!(matches!(cache.lookup(t0 + Duration::from_secs(601)), Lookup::Stale));
    }
    fn serve_once(response: String) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0_u8; 1024];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&bytes).into_owned()
        });
        (url, thread)
    }
    fn local(url: String) -> Target {
        Target { url, host: "127.0.0.1".into(), roadeep: false }
    }
    #[tokio::test]
    async fn offset_comes_from_the_date_header_of_a_head_request() {
        let ahead = system_ms() + 3_600_000;
        let (d, t) = (ahead.div_euclid(DAY), ahead.rem_euclid(DAY));
        let (y, m, day) = civil_from_days(d);
        let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][m as usize - 1];
        let date = format!("Thu, {day:02} {month} {y} {:02}:{:02}:{:02} GMT", t / HOUR, (t / MINUTE) % 60, (t / 1000) % 60);
        let (url, thread) = serve_once(format!("HTTP/1.1 401 Unauthorized\r\nDate: {date}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
        let offset = fetch_offset(&local(url), false).await.unwrap();
        assert!((offset - 3_600_000).abs() < 3_000, "{offset}");
        let request = thread.join().unwrap();
        assert!(request.starts_with("HEAD / "));
        assert!(!request.to_lowercase().contains("authorization"));
    }
    #[tokio::test]
    async fn missing_date_or_unreachable_host_falls_back() {
        let (url, thread) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        assert!(fetch_offset(&local(url), false).await.is_err());
        thread.join().unwrap();
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", closed.local_addr().unwrap());
        drop(closed);
        assert!(fetch_offset(&local(url), false).await.is_err());
        assert_eq!(clock_from(None).source, Source::System);
        // https_only refuses a plain-HTTP target outright.
        assert!(fetch_offset(&local("http://127.0.0.1:9/".into()), true).await.is_err());
    }
}
