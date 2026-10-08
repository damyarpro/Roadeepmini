//! What Roadeep learns about the user, kept only on this computer (`memory.json` in the app
//! config dir). It reaches a provider only inside a live voice session's instructions.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::Manager;

const FILE: &str = "memory.json";
const MAX_FILE: u64 = 1024 * 1024;
pub const MAX_FACTS: usize = 100;
pub const MAX_FACT_CHARS: usize = 300;
pub const MAX_RECENT: usize = 50;
pub const MAX_SUMMARY_CHARS: usize = 120;
const MAX_TOOL_KEYS: usize = 200;
pub const MAX_BRIEF_CHARS: usize = 1500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Preference,
    Habit,
    Style,
    Fact,
}
impl Category {
    fn fa(self) -> &'static str {
        match self {
            Self::Preference => "ترجیح",
            Self::Habit => "عادت",
            Self::Style => "سبک",
            Self::Fact => "دانسته",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub id: String,
    pub text: String,
    pub category: Category,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recent {
    pub at: i64,
    pub tool: String,
    pub summary: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub tool_counts: BTreeMap<String, u64>,
    pub hour_histogram: [u32; 24],
    pub weekday_histogram: [u32; 7],
    pub recent: Vec<Recent>,
}
impl Default for Stats {
    fn default() -> Self {
        Self { tool_counts: BTreeMap::new(), hour_histogram: [0; 24], weekday_histogram: [0; 7], recent: Vec::new() }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Memory {
    pub enabled: bool,
    pub facts: Vec<Fact>,
    pub stats: Stats,
}
impl Default for Memory {
    fn default() -> Self {
        Self { enabled: true, facts: Vec::new(), stats: Stats::default() }
    }
}

// ── Text rules ───────────────────────────────────────────────────────────────

fn normalize(text: &str) -> String {
    let mapped: String = text
        .chars()
        .map(|c| match c {
            'ي' | 'ى' => 'ی',
            'ك' => 'ک',
            '\u{200C}' | '\u{200D}' => ' ',
            c if c.is_alphanumeric() => c.to_lowercase().next().unwrap_or(c),
            _ => ' ',
        })
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn digit_value(c: char) -> Option<u32> {
    c.to_digit(10)
        .or_else(|| ('۰'..='۹').contains(&c).then(|| c as u32 - '۰' as u32))
        .or_else(|| ('٠'..='٩').contains(&c).then(|| c as u32 - '٠' as u32))
}
/// Passwords, keys, tokens, one-time codes, PINs, card/national-ID/phone numbers and IBANs
/// never enter the memory.
pub fn looks_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    // Phrases are matched on normalized text (ZWNJ, Arabic letters and punctuation folded).
    let norm = normalize(text);
    const PHRASES: [&str; 25] = [
        "password", "passwd", "passcode", "api key", "apikey", "secret", "token", "bearer", "one time",
        "رمز", "پسورد", "گذرواژه", "کلمه عبور", "کلمهٔ عبور", "شماره کارت", "شبا", "کد ملی", "کدملی",
        "کد تایید", "کد تأیید", "کد تائید", "کد یکبار", "کد یک بار", "کد پویا", "سی وی وی",
    ];
    // «شماره‌ی کارت» / «کد‌ِ ملی»: the ezafe written as a separate «ی» still names the same thing.
    let folded = format!(" {norm} ").replace(" ی ", " ");
    if PHRASES.iter().any(|w| norm.contains(&normalize(w)) || folded.contains(&normalize(w))) || lower.contains("api_key") {
        return true;
    }
    // Short Latin markers only as whole words ("pin" but not "spinning").
    if norm.split(' ').any(|w| matches!(w, "otp" | "cvv" | "cvv2" | "cvc" | "pin" | "2fa")) {
        return true;
    }
    if ["sk-", "ghp_", "gho_", "xoxb-", "xoxp-", "akia", "aiza"].iter().any(|p| lower.contains(p)) {
        return true;
    }
    // Card numbers, IBANs, national IDs (10) and phone numbers (11): 10+ digits, allowing
    // spaces and dashes between groups. Dates (Gregorian or Jalali, any digits) and clock
    // times are taken out first: «۱۴۰۵/۰۷/۱۵ ۱۴:۰۵» is a reminder, not an ID.
    static DATE_TIME: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\b\d{4}[-/]\d{1,2}[-/]\d{1,2}|\d{1,2}:\d{2}(?::\d{2})?").expect("valid date/time pattern")
    });
    let text = DATE_TIME.replace_all(text, "|");
    let mut run = 0;
    for c in text.chars() {
        if digit_value(c).is_some() {
            run += 1;
            if run >= 10 {
                return true;
            }
        } else if !matches!(c, ' ' | '-') {
            run = 0;
        }
    }
    // Long opaque tokens.
    text.split_whitespace().any(|w| {
        w.chars().count() >= 24
            && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_.=+/".contains(c))
            && w.chars().any(|c| c.is_ascii_digit())
    })
}
/// Near-identical: same normalized text, or ≥80 % shared words.
fn similar(a: &str, b: &str) -> bool {
    let (na, nb) = (normalize(a), normalize(b));
    if na == nb {
        return true;
    }
    let wa: HashSet<&str> = na.split(' ').collect();
    let wb: HashSet<&str> = nb.split(' ').collect();
    let union = wa.union(&wb).count();
    union > 0 && wa.intersection(&wb).count() * 10 >= union * 8
}
fn clamp(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        return one;
    }
    let mut out: String = one.chars().take(max - 1).collect();
    out.push('…');
    out
}

// ── Mutations (pure) ─────────────────────────────────────────────────────────

pub fn remember(m: &mut Memory, text: &str, category: Category, now: i64) -> Result<Fact, String> {
    if !m.enabled {
        return Err("memory-disabled".into());
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || text.chars().count() > MAX_FACT_CHARS || text.chars().any(char::is_control) {
        return Err("memory-invalid".into());
    }
    if looks_secret(&text) {
        return Err("memory-secret".into());
    }
    let created_at = match m.facts.iter().position(|f| similar(&f.text, &text)) {
        Some(i) => m.facts.remove(i).created_at,
        None => now,
    };
    let fact = Fact { id: uuid::Uuid::new_v4().to_string(), text, category, created_at, updated_at: now };
    m.facts.push(fact.clone());
    if m.facts.len() > MAX_FACTS {
        m.facts.sort_by_key(|f| std::cmp::Reverse(f.updated_at));
        m.facts.truncate(MAX_FACTS);
    }
    Ok(fact)
}
/// Removes every fact whose text contains the query's words; returns how many.
/// At most this many facts are forgotten by one query; more means the user must be specific.
pub const MAX_FORGET: usize = 3;
/// Removes the facts whose text contains the query (or all its words); returns how many.
/// A vague query (< 3 characters) or one matching more than MAX_FORGET facts removes nothing:
/// "memory-query-ambiguous|<count>" lets the assistant ask which one.
pub fn forget(m: &mut Memory, query: &str) -> Result<usize, String> {
    let q = normalize(query);
    if q.chars().filter(|c| *c != ' ').count() < 3 {
        return Err("memory-query-too-vague".into());
    }
    let words: Vec<&str> = q.split(' ').collect();
    let matches = |f: &Fact| {
        let t = normalize(&f.text);
        t.contains(&q) || words.iter().all(|w| t.split(' ').any(|tw| tw.starts_with(w)))
    };
    let count = m.facts.iter().filter(|f| matches(f)).count();
    if count > MAX_FORGET {
        return Err(format!("memory-query-ambiguous|{count}"));
    }
    m.facts.retain(|f| !matches(f));
    Ok(count)
}
pub fn record(m: &mut Memory, tool: &str, summary: &str, at: i64, hour: u32, weekday: u32) -> Result<(), String> {
    if !m.enabled {
        return Err("memory-disabled".into());
    }
    let tool: String = tool.chars().filter(|c| c.is_ascii_alphanumeric() || "_-".contains(*c)).take(64).collect();
    if tool.is_empty() {
        return Err("memory-invalid".into());
    }
    let summary = if looks_secret(summary) { String::new() } else { clamp(summary, MAX_SUMMARY_CHARS) };
    let s = &mut m.stats;
    if s.tool_counts.contains_key(&tool) || s.tool_counts.len() < MAX_TOOL_KEYS {
        *s.tool_counts.entry(tool.clone()).or_default() += 1;
    }
    s.hour_histogram[(hour as usize).min(23)] += 1;
    s.weekday_histogram[(weekday as usize).min(6)] += 1;
    s.recent.push(Recent { at, tool, summary });
    if s.recent.len() > MAX_RECENT {
        let cut = s.recent.len() - MAX_RECENT;
        s.recent.drain(..cut);
    }
    Ok(())
}

const WEEKDAYS_FA: [&str; 7] = ["شنبه", "یکشنبه", "دوشنبه", "سه‌شنبه", "چهارشنبه", "پنجشنبه", "جمعه"];
/// Persian brief for the assistants: recent facts, then usage patterns; ≤ MAX_BRIEF_CHARS.
pub fn profile_brief(m: &Memory) -> String {
    if !m.enabled {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut facts: Vec<&Fact> = m.facts.iter().collect();
    facts.sort_by_key(|f| std::cmp::Reverse(f.updated_at));
    for f in facts {
        lines.push(format!("- [{}] {}", f.category.fa(), f.text));
    }
    let s = &m.stats;
    let mut tools: Vec<(&String, &u64)> = s.tool_counts.iter().collect();
    tools.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut patterns = Vec::new();
    if !tools.is_empty() {
        let top: Vec<String> = tools.iter().take(4).map(|(t, n)| format!("{t} ({n})")).collect();
        patterns.push(format!("- ابزارهای پرکاربرد: {}", top.join("، ")));
    }
    let top_of = |h: &[u32]| {
        let mut idx: Vec<usize> = (0..h.len()).filter(|&i| h[i] > 0).collect();
        idx.sort_by(|&a, &b| h[b].cmp(&h[a]).then(a.cmp(&b)));
        idx.truncate(3);
        idx
    };
    let hours = top_of(&s.hour_histogram);
    if !hours.is_empty() {
        let h: Vec<String> = hours.iter().map(|h| format!("{h}:00")).collect();
        patterns.push(format!("- ساعت‌های معمول استفاده: {}", h.join("، ")));
    }
    let days = top_of(&s.weekday_histogram);
    if !days.is_empty() {
        let d: Vec<&str> = days.iter().map(|&d| WEEKDAYS_FA[d]).collect();
        patterns.push(format!("- روزهای پرکار: {}", d.join("، ")));
    }
    let recent: Vec<String> = s
        .recent
        .iter()
        .rev()
        .filter(|r| !r.summary.is_empty())
        .take(3)
        .map(|r| r.summary.clone())
        .collect();
    if !recent.is_empty() {
        patterns.push(format!("- درخواست‌های اخیر: {}", recent.join(" | ")));
    }
    // Patterns are short and always useful; facts fill the remaining budget, newest first.
    let tail = patterns.join("\n");
    let mut out = String::new();
    let budget = MAX_BRIEF_CHARS.saturating_sub(tail.chars().count() + 1);
    for line in lines {
        if out.chars().count() + line.chars().count() + 1 > budget {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&tail);
    let out = out.trim_end().to_string();
    if out.chars().count() > MAX_BRIEF_CHARS { out.chars().take(MAX_BRIEF_CHARS).collect() } else { out }
}

// ── Storage ──────────────────────────────────────────────────────────────────

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn load_from(path: &Path) -> Result<Memory, String> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Memory::default()),
        Err(_) => return Err("memory-io".into()),
        Ok(meta) if meta.len() > MAX_FILE => return Err("memory-invalid".into()),
        Ok(_) => {}
    }
    let bytes = std::fs::read(path).map_err(|_| "memory-io".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "memory-invalid".to_string())
}
/// Temp file + rename: a crash never leaves a half-written memory.
pub fn save_to(path: &Path, m: &Memory) -> Result<(), String> {
    let dir = path.parent().ok_or("memory-io")?;
    std::fs::create_dir_all(dir).map_err(|_| "memory-io".to_string())?;
    let bytes = serde_json::to_vec(m).map_err(|_| "memory-io".to_string())?;
    let staging = path.with_extension("json.new");
    std::fs::write(&staging, bytes).map_err(|_| "memory-io".to_string())?;
    std::fs::rename(&staging, path).map_err(|_| {
        let _ = std::fs::remove_file(&staging);
        "memory-io".to_string()
    })
}
fn path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map(|p| p.join(FILE)).map_err(|_| "memory-io".into())
}
/// Load, change, save under one lock. With `recover`, a corrupt or oversized file is set aside
/// once as `memory.json.bad` and the change applies to a fresh memory (clear / enable must
/// always work, or the user could never get out of a broken file).
fn update_at<T>(path: &Path, recover: bool, f: impl FnOnce(&mut Memory) -> Result<T, String>) -> Result<T, String> {
    let _guard = LOCK.lock().map_err(|_| "memory-io".to_string())?;
    let mut m = match load_from(path) {
        Err(code) if recover && code == "memory-invalid" => {
            let bad = path.with_extension("json.bad");
            if !bad.exists() && std::fs::rename(path, &bad).is_err() {
                crate::log::line("memory: could not set the unreadable file aside");
            }
            crate::log::line("memory: unreadable memory replaced with a fresh one");
            Memory::default()
        }
        other => other?,
    };
    let out = f(&mut m)?;
    save_to(path, &m)?;
    Ok(out)
}
fn load_at(path: &Path) -> Result<Memory, String> {
    let _guard = LOCK.lock().map_err(|_| "memory-io".to_string())?;
    load_from(path)
}
pub fn load(app: &tauri::AppHandle) -> Result<Memory, String> {
    load_at(&path(app)?)
}
/// File work off the async runtime's worker threads.
async fn blocking<T: Send + 'static>(op: &'static str, f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let result = tauri::async_runtime::spawn_blocking(f).await.unwrap_or_else(|_| Err("memory-io".into()));
    logged(op, result)
}
/// The brief for a session (off the async threads); an unreadable memory is logged and left out.
pub async fn brief_for_session(app: &tauri::AppHandle) -> String {
    let app = app.clone();
    blocking("brief", move || load(&app).map(|m| profile_brief(&m))).await.unwrap_or_default()
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
fn require(window: &tauri::WebviewWindow, labels: &[&str]) -> Result<(), String> {
    if labels.contains(&window.label()) { Ok(()) } else { Err("Memory operation unavailable in this window".into()) }
}
fn logged<T>(op: &str, result: Result<T, String>) -> Result<T, String> {
    if let Err(code) = &result {
        crate::log::line(format!("memory:{op} outcome=error code={code}"));
    }
    result
}

#[tauri::command]
pub async fn memory_get(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<Memory, String> {
    require(&window, &["island", "settings"])?;
    let path = path(&app)?;
    blocking("get", move || load_at(&path)).await
}
#[tauri::command]
pub async fn memory_remember(app: tauri::AppHandle, window: tauri::WebviewWindow, text: String, category: Category) -> Result<Fact, String> {
    require(&window, &["island", "settings"])?;
    let path = path(&app)?;
    blocking("remember", move || update_at(&path, false, |m| remember(m, &text, category, now_ms()))).await
}
#[tauri::command]
pub async fn memory_forget(app: tauri::AppHandle, window: tauri::WebviewWindow, query: String) -> Result<usize, String> {
    require(&window, &["island", "settings"])?;
    let path = path(&app)?;
    blocking("forget", move || update_at(&path, false, |m| forget(m, &query))).await
}
#[tauri::command]
pub async fn memory_delete(app: tauri::AppHandle, window: tauri::WebviewWindow, id: String) -> Result<(), String> {
    require(&window, &["settings"])?;
    let path = path(&app)?;
    blocking("delete", move || update_at(&path, false, |m| {
        m.facts.retain(|f| f.id != id);
        Ok(())
    }))
    .await
}
fn clear(m: &mut Memory) -> Result<(), String> {
    let enabled = m.enabled;
    *m = Memory { enabled, ..Memory::default() };
    Ok(())
}
#[tauri::command]
pub async fn memory_clear(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    require(&window, &["settings"])?;
    let path = path(&app)?;
    blocking("clear", move || update_at(&path, true, clear)).await
}
#[tauri::command]
pub async fn memory_set_enabled(app: tauri::AppHandle, window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    require(&window, &["settings"])?;
    let path = path(&app)?;
    // Disabling stops learning and the brief; the data stays until cleared.
    blocking("set_enabled", move || update_at(&path, true, |m| {
        m.enabled = enabled;
        Ok(())
    }))
    .await
}
#[tauri::command]
pub async fn memory_record(app: tauri::AppHandle, window: tauri::WebviewWindow, tool: String, summary: String) -> Result<(), String> {
    require(&window, &["island"])?;
    use crate::planner::schedule::{local_parts, SystemZone, HOUR};
    let path = path(&app)?;
    let at = now_ms();
    let (days, tod) = local_parts(&SystemZone, at);
    let weekday = crate::planner::jalali::weekday(days);
    blocking("record", move || {
        update_at(&path, false, |m| if m.enabled { record(m, &tool, &summary, at, (tod / HOUR) as u32, weekday) } else { Ok(()) })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_are_bounded_deduped_and_newest_wins() {
        let mut m = Memory::default();
        let a = remember(&mut m, "قهوه را بدون شکر دوست دارم", Category::Preference, 1).unwrap();
        let b = remember(&mut m, "قهوه را  بدون شکر دوست دارم!", Category::Preference, 5).unwrap();
        assert_eq!(m.facts.len(), 1);
        assert_eq!((b.created_at, b.updated_at), (a.created_at, 5));
        assert!(remember(&mut m, &"x".repeat(MAX_FACT_CHARS + 1), Category::Fact, 6).is_err());
        assert!(remember(&mut m, "   ", Category::Fact, 6).is_err());
        for i in 0..MAX_FACTS + 10 {
            remember(&mut m, &format!("واقعیت شماره {i} درباره کاربر"), Category::Fact, 10 + i as i64).unwrap();
        }
        assert_eq!(m.facts.len(), MAX_FACTS);
        assert!(m.facts.iter().all(|f| f.updated_at >= 20), "oldest are dropped");
        assert_eq!(forget(&mut m, "شماره 109"), Ok(1));
        assert_eq!(forget(&mut m, "شماره 109"), Ok(0));
    }
    #[test]
    fn forget_refuses_vague_and_ambiguous_queries() {
        let mut m = Memory::default();
        for (i, text) in ["صبح‌ها چای می‌نوشم", "عصرها چای سبز می‌نوشم", "شب‌ها چای نمی‌نوشم", "چای را داغ دوست دارم", "قهوه نمی‌خورم"].iter().enumerate() {
            remember(&mut m, text, Category::Habit, i as i64).unwrap();
        }
        assert_eq!(forget(&mut m, "").unwrap_err(), "memory-query-too-vague");
        assert_eq!(forget(&mut m, " چ ").unwrap_err(), "memory-query-too-vague");
        assert_eq!(forget(&mut m, "چای").unwrap_err(), "memory-query-ambiguous|4");
        assert_eq!(m.facts.len(), 5, "an ambiguous query deletes nothing");
        assert_eq!(forget(&mut m, "چای سبز"), Ok(1));
        assert_eq!(forget(&mut m, "قهوه"), Ok(1));
        assert_eq!(m.facts.len(), 3);
    }
    #[test]
    fn one_time_codes_ids_and_phone_numbers_are_secrets() {
        for secret in [
            "کد تأیید بانک 48213 است",
            "کد تایید را یادت بماند",
            "رمز یک‌بار مصرفم",
            "کد ملی من 0012345678",
            "کدملی‌ام را بنویس",
            "شماره‌ی کارت جدیدم",
            "my OTP is 553311",
            "CVV2 = 412",
            "PIN 4455",
            "شماره‌ام 0912 345 6789",
            "+98 912 345 67 89",
        ] {
            assert!(looks_secret(secret), "{secret}");
        }
        for fine in ["پینگ‌پنگ بازی می‌کنم", "spinning class on 2026-10-07", "ساعت 7:30 بیدار می‌شوم", "سه فرزند دارم"] {
            assert!(!looks_secret(fine), "{fine}");
        }
        // Dates and times are not IDs: reminders and tasks keep their summaries.
        for dated in [
            "یادآوری دندانپزشک 2026-10-07 14:05",
            "یادآوری 2026-10-07T14:05:30",
            "جلسه ۱۴۰۵/۰۷/۱۵ ۱۴:۰۵",
            "کار تا 1405/7/15 10:30 و 11:45",
            "۱۴۰۵-۰۷-۱۵ ساعت ۹:۰۰",
        ] {
            assert!(!looks_secret(dated), "{dated}");
        }
        // …but an ID or phone number next to a date is still caught.
        assert!(looks_secret("2026-10-07 تماس با 09123456789"));
        assert!(looks_secret("۱۴۰۵/۰۷/۱۵ کد ۰۰۱۲۳۴۵۶۷۸"));
        let mut m = Memory::default();
        record(&mut m, "add_reminder", "دندانپزشک ۱۴۰۵/۰۷/۱۵ ۱۴:۰۵", 1, 9, 0).unwrap();
        assert_eq!(m.stats.recent[0].summary, "دندانپزشک ۱۴۰۵/۰۷/۱۵ ۱۴:۰۵");
        assert!(remember(&mut m, "هر روز 2026-10-07 07:30 ورزش", Category::Habit, 1).is_ok());
        m.stats.recent.clear();
        record(&mut m, "add_note", "کد تأیید 1234 بانک", 1, 9, 0).unwrap();
        record(&mut m, "add_task", "تماس با 09123456789", 2, 9, 0).unwrap();
        assert!(m.stats.recent.iter().all(|r| r.summary.is_empty()));
    }
    #[test]
    fn clear_and_enable_recover_from_a_corrupt_or_oversized_file() {
        let dir = std::env::temp_dir().join(format!("roadeep-memory-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let bad = path.with_extension("json.bad");
        std::fs::write(&path, b"{broken").unwrap();
        assert_eq!(update_at(&path, false, |m| remember(m, "چای دوست دارم", Category::Preference, 1)).unwrap_err(), "memory-invalid");
        update_at(&path, true, clear).unwrap();
        assert_eq!(std::fs::read(&bad).unwrap(), b"{broken", "the bad file is kept once");
        assert_eq!(load_from(&path).unwrap(), Memory::default());
        // Oversized: replaced, and the first .bad copy is not overwritten.
        std::fs::write(&path, vec![b' '; (MAX_FILE + 1) as usize]).unwrap();
        update_at(&path, true, |m| {
            m.enabled = false;
            Ok(())
        })
        .unwrap();
        assert!(!load_from(&path).unwrap().enabled);
        assert_eq!(std::fs::read(&bad).unwrap(), b"{broken");
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn secrets_are_never_stored() {
        let mut m = Memory::default();
        for secret in [
            "رمز ایمیلم 1234 است",
            "my password is hunter2",
            "کلید من sk-proj-abc",
            "کارتم 6037 9912 3456 7890",
            "IR820540102680020817909002",
            "شماره کارتم را یادت باشد",
            "token: eyJhbGciOiJIUzI1NiJ9abc123def456",
        ] {
            assert_eq!(remember(&mut m, secret, Category::Fact, 1).unwrap_err(), "memory-secret", "{secret}");
        }
        assert!(m.facts.is_empty());
        assert!(!looks_secret("ساعت ۷ صبح بیدار می‌شوم"));
        record(&mut m, "add_note", "یادداشت رمز وای‌فای", 1, 9, 0).unwrap();
        assert_eq!(m.stats.recent[0].summary, "");
    }
    #[test]
    fn disabled_memory_learns_nothing_and_briefs_nothing() {
        let mut m = Memory::default();
        remember(&mut m, "صبح‌ها ورزش می‌کنم", Category::Habit, 1).unwrap();
        m.enabled = false;
        assert_eq!(remember(&mut m, "چای دوست دارم", Category::Preference, 2).unwrap_err(), "memory-disabled");
        assert_eq!(record(&mut m, "list_tasks", "x", 1, 9, 0).unwrap_err(), "memory-disabled");
        assert_eq!(profile_brief(&m), "");
        assert_eq!(m.facts.len(), 1, "disabling keeps data until cleared");
    }
    #[test]
    fn stats_and_brief_are_bounded() {
        let mut m = Memory::default();
        for i in 0..80 {
            record(&mut m, if i % 3 == 0 { "add_task" } else { "list_notes" }, &format!("درخواست {i}"), i, 9 + (i % 2) as u32, 2).unwrap();
        }
        assert_eq!(m.stats.recent.len(), MAX_RECENT);
        assert_eq!(m.stats.tool_counts["list_notes"], 53);
        assert_eq!(m.stats.hour_histogram[9] + m.stats.hour_histogram[10], 80);
        for i in 0..MAX_FACTS {
            remember(&mut m, &format!("{i} {}", "ب".repeat(250)), Category::Style, 100 + i as i64).unwrap();
        }
        let brief = profile_brief(&m);
        assert!(brief.chars().count() <= MAX_BRIEF_CHARS);
        assert!(brief.contains("list_notes (53)") && brief.contains("9:00") && brief.contains("دوشنبه"));
        assert!(brief.contains("درخواست 79"));
        assert!(brief.starts_with(&format!("- [سبک] {}", MAX_FACTS - 1)), "newest fact first");
        assert_eq!(profile_brief(&Memory::default()), "");
    }
    #[test]
    fn storage_is_atomic_and_tolerates_missing_or_corrupt_files() {
        let dir = std::env::temp_dir().join(format!("roadeep-memory-{}", uuid::Uuid::new_v4()));
        let path = dir.join(FILE);
        assert_eq!(load_from(&path).unwrap(), Memory::default());
        let mut m = Memory::default();
        remember(&mut m, "نام گربه‌ام پیشی است", Category::Fact, 3).unwrap();
        save_to(&path, &m).unwrap();
        assert!(!path.with_extension("json.new").exists());
        assert_eq!(load_from(&path).unwrap(), m);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(load_from(&path).unwrap_err(), "memory-invalid");
        std::fs::write(&path, br#"{"facts":[]}"#).unwrap();
        assert!(load_from(&path).unwrap().enabled, "missing fields take defaults");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
