// planner.json: reading, validating and writing the planner, and the changes
// the commands and the chat tools make to it.
//
// The file is written atomically (temp file + rename), compact. A file that
// does not parse, or holds entries that fail validation, is kept aside as
// planner.json.bad-<time> before anything is written over it (like
// agents.json): a user's tasks are never lost silently. A file that exists but
// cannot be read right now (another process holds it), or a damaged one that
// could not be kept aside, is an error, never "empty": Planner refuses to
// write until a read succeeds. A UTF-8 BOM (a file
// touched by Notepad) is accepted.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use super::jalali::days_of_key;
use super::schedule::{self, Now, MINUTE};
use super::{
    invalid, not_found, plog, FocusState, Habit, HabitDays, HabitIcon, Note, NudgeArg, Nudge, Phase, PlannerData, Reminder, Repeat, Task,
    FILE_VERSION,
};
use crate::errors;
use crate::mcpc::tools::{clean_block, one_line};

pub const FILE_NAME: &str = "planner.json";

pub const MAX_TASKS: usize = 2000;
pub const MAX_NOTES: usize = 2000;
pub const MAX_REMINDERS: usize = 500;
pub const MAX_HABITS: usize = 100;
pub const MAX_LOG_DAYS: usize = 3650;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_TASK_NOTE_CHARS: usize = 2000;
pub const MAX_NOTE_CHARS: usize = 4000;
pub const MAX_HABIT_TITLE_CHARS: usize = 80;
pub const MAX_SNOOZE_MINUTES: f64 = 240.0;
pub const NUDGE_MINUTES: (u32, u32) = (15, 240);
/// A one-off reminder may be set this far in the past (a click on "now").
const PAST_GRACE: i64 = MINUTE;
/// …and at most this far ahead.
const MAX_AHEAD: i64 = 10 * 366 * schedule::DAY;
/// Year 2200: past this a number is a mistake, not a date.
const MAX_MS: f64 = 7_258_118_400_000.0;

// ── Errors and text ───────────────────────────────────────────────────────────

/// `E_PLANNER_STORE|detail`, logged with its context. The detail is an OS
/// error, never planner content.
pub fn store_error(context: &str, err: &dyn std::fmt::Display) -> String {
    plog(format!("planner: {context}: {err}"));
    errors::coded(errors::PLANNER_STORE, &[&err.to_string()])
}

fn limit(n: usize) -> String {
    errors::coded(errors::PLANNER_LIMIT, &[&n.to_string()])
}

/// One line, trimmed, no control or bidi characters, 1..=max characters.
pub fn clean_title(text: &str, max: usize, field: &str) -> Result<String, String> {
    let line = one_line(text, usize::MAX);
    let n = line.chars().count();
    if n == 0 || n > max {
        return Err(invalid(field));
    }
    Ok(line)
}

/// Multi-line text: newlines and tabs kept, other control and bidi characters
/// dropped, trimmed, at most `max` characters (and not empty unless allowed).
pub fn clean_text(text: &str, max: usize, allow_empty: bool, field: &str) -> Result<String, String> {
    let block = clean_block(text);
    let block = block.trim();
    let n = block.chars().count();
    if (n == 0 && !allow_empty) || n > max {
        return Err(invalid(field));
    }
    Ok(block.to_string())
}

/// A day key argument: "" or absent = none.
fn due_arg(due: Option<&str>) -> Result<Option<String>, String> {
    match due.map(str::trim).filter(|d| !d.is_empty()) {
        None => Ok(None),
        Some(d) => days_of_key(d).map(|_| Some(d.to_string())).ok_or_else(|| invalid("due")),
    }
}

/// A time from the front end (a JS number).
pub fn ms_arg(value: f64, field: &str) -> Result<i64, String> {
    if value.is_finite() && (0.0..=MAX_MS).contains(&value) {
        Ok(value.round() as i64)
    } else {
        Err(invalid(field))
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn valid_id(id: &str) -> bool {
    crate::agents::valid_id(id)
}

/// The moment a reminder is set for, checked and normalized: a one-off may
/// not be in the past; a repeating one starts at its first occurrence from
/// now (a weekdays one on a workday).
fn reminder_at(at: i64, repeat: Repeat, now: &Now<'_>) -> Result<i64, String> {
    if at > now.ms + MAX_AHEAD || (repeat == Repeat::None && at < now.ms - PAST_GRACE) {
        return Err(invalid("at"));
    }
    let at = schedule::align(at, repeat, now.zone);
    Ok(schedule::roll_forward(at, repeat, now.zone, now.ms))
}

/// A nudge as the user set it; everyMinutes 0 = no nudge.
fn nudge_arg(arg: &NudgeArg) -> Result<Option<Nudge>, String> {
    if arg.every_minutes == 0 {
        return Ok(None);
    }
    let (from, to) = (schedule::parse_hhmm(arg.from.trim()), schedule::parse_hhmm(arg.to.trim()));
    let ok = (NUDGE_MINUTES.0..=NUDGE_MINUTES.1).contains(&arg.every_minutes) && matches!((from, to), (Some(f), Some(t)) if f < t);
    if !ok {
        return Err(invalid("nudge"));
    }
    Ok(Some(Nudge { every_minutes: arg.every_minutes, from: arg.from.trim().to_string(), to: arg.to.trim().to_string() }))
}

// ── Changes ───────────────────────────────────────────────────────────────────

pub struct TaskPatch {
    pub title: Option<String>,
    pub note: Option<String>,
    /// "" clears the due day.
    pub due: Option<String>,
    pub done: Option<bool>,
}

pub struct ReminderPatch {
    pub title: Option<String>,
    pub at: Option<i64>,
    pub repeat: Option<Repeat>,
    pub enabled: Option<bool>,
}

pub struct HabitPatch {
    pub title: Option<String>,
    pub icon: Option<HabitIcon>,
    pub days: Option<HabitDays>,
    pub nudge: Option<NudgeArg>,
}

impl PlannerData {
    pub fn add_task(&mut self, title: &str, due: Option<&str>, note: Option<&str>, now: i64) -> Result<Task, String> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(limit(MAX_TASKS));
        }
        let task = Task {
            id: new_id(),
            title: clean_title(title, MAX_TITLE_CHARS, "title")?,
            note: clean_text(note.unwrap_or(""), MAX_TASK_NOTE_CHARS, true, "note")?,
            done: false,
            due: due_arg(due)?,
            created_at: now,
            updated_at: now,
            done_at: None,
        };
        self.tasks.push(task.clone());
        Ok(task)
    }

    pub fn update_task(&mut self, id: &str, patch: TaskPatch, now: i64) -> Result<Task, String> {
        let task = self.tasks.iter_mut().find(|t| t.id == id).ok_or_else(not_found)?;
        let before = task.clone();
        if let Some(title) = &patch.title {
            task.title = clean_title(title, MAX_TITLE_CHARS, "title")?;
        }
        if let Some(note) = &patch.note {
            task.note = clean_text(note, MAX_TASK_NOTE_CHARS, true, "note")?;
        }
        if let Some(due) = &patch.due {
            task.due = due_arg(Some(due))?;
        }
        if let Some(done) = patch.done {
            if done && !task.done {
                task.done_at = Some(now);
            } else if !done {
                task.done_at = None;
            }
            task.done = done;
        }
        if *task != before {
            task.updated_at = now.max(task.created_at);
        }
        Ok(task.clone())
    }

    pub fn delete_task(&mut self, id: &str) -> Result<(), String> {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        if self.tasks.len() == before { Err(not_found()) } else { Ok(()) }
    }

    pub fn add_note(&mut self, text: &str, now: i64) -> Result<Note, String> {
        if self.notes.len() >= MAX_NOTES {
            return Err(limit(MAX_NOTES));
        }
        let note = Note { id: new_id(), text: clean_text(text, MAX_NOTE_CHARS, false, "text")?, pinned: false, created_at: now, updated_at: now };
        self.notes.push(note.clone());
        Ok(note)
    }

    pub fn update_note(&mut self, id: &str, text: Option<&str>, pinned: Option<bool>, now: i64) -> Result<Note, String> {
        let note = self.notes.iter_mut().find(|n| n.id == id).ok_or_else(not_found)?;
        let before = note.clone();
        if let Some(text) = text {
            note.text = clean_text(text, MAX_NOTE_CHARS, false, "text")?;
        }
        if let Some(pinned) = pinned {
            note.pinned = pinned;
        }
        if *note != before {
            note.updated_at = now.max(note.created_at);
        }
        Ok(note.clone())
    }

    pub fn delete_note(&mut self, id: &str) -> Result<(), String> {
        let before = self.notes.len();
        self.notes.retain(|n| n.id != id);
        if self.notes.len() == before { Err(not_found()) } else { Ok(()) }
    }

    pub fn add_reminder(&mut self, title: &str, at: i64, repeat: Repeat, now: &Now<'_>) -> Result<Reminder, String> {
        if self.reminders.len() >= MAX_REMINDERS {
            return Err(limit(MAX_REMINDERS));
        }
        let reminder = Reminder {
            id: new_id(),
            title: clean_title(title, MAX_TITLE_CHARS, "title")?,
            at: reminder_at(at, repeat, now)?,
            repeat,
            enabled: true,
            last_fired_at: None,
            snoozed_until: None,
            created_at: now.ms,
        };
        self.reminders.push(reminder.clone());
        Ok(reminder)
    }

    pub fn update_reminder(&mut self, id: &str, patch: ReminderPatch, now: &Now<'_>) -> Result<Reminder, String> {
        let r = self.reminders.iter_mut().find(|r| r.id == id).ok_or_else(not_found)?;
        if let Some(title) = &patch.title {
            r.title = clean_title(title, MAX_TITLE_CHARS, "title")?;
        }
        if patch.at.is_some() || patch.repeat.is_some() {
            let repeat = patch.repeat.unwrap_or(r.repeat);
            // A new time for a one-off that already fired re-arms it; an
            // unchanged one keeps its state.
            let at = match patch.at {
                Some(at) => reminder_at(at, repeat, now)?,
                None => schedule::roll_forward(schedule::align(r.at, repeat, now.zone), repeat, now.zone, now.ms),
            };
            r.at = at;
            r.repeat = repeat;
            r.snoozed_until = None;
        }
        if let Some(enabled) = patch.enabled {
            r.enabled = enabled;
            if !enabled {
                r.snoozed_until = None;
            }
        }
        // Switched back on after its time: the next occurrence, not a burst.
        if r.enabled && r.repeat != Repeat::None && r.at <= now.ms {
            r.at = schedule::roll_forward(r.at, r.repeat, now.zone, now.ms);
        }
        Ok(r.clone())
    }

    pub fn delete_reminder(&mut self, id: &str) -> Result<(), String> {
        let before = self.reminders.len();
        self.reminders.retain(|r| r.id != id);
        if self.reminders.len() == before { Err(not_found()) } else { Ok(()) }
    }

    pub fn snooze_reminder(&mut self, id: &str, minutes: f64, now: i64) -> Result<Reminder, String> {
        let r = self.reminders.iter_mut().find(|r| r.id == id).ok_or_else(not_found)?;
        if !(minutes.is_finite() && (1.0..=MAX_SNOOZE_MINUTES).contains(&minutes.round())) {
            return Err(invalid("minutes"));
        }
        if !r.enabled {
            return Err(invalid("enabled"));
        }
        r.snoozed_until = Some(now + minutes.round() as i64 * MINUTE);
        Ok(r.clone())
    }

    pub fn add_habit(&mut self, title: &str, icon: HabitIcon, days: HabitDays, nudge: Option<&NudgeArg>, now: i64) -> Result<Habit, String> {
        if self.habits.len() >= MAX_HABITS {
            return Err(limit(MAX_HABITS));
        }
        let habit = Habit {
            id: new_id(),
            title: clean_title(title, MAX_HABIT_TITLE_CHARS, "title")?,
            icon,
            days,
            nudge: nudge.map(nudge_arg).transpose()?.flatten(),
            log: Vec::new(),
            created_at: now,
        };
        self.habits.push(habit.clone());
        Ok(habit)
    }

    pub fn update_habit(&mut self, id: &str, patch: HabitPatch) -> Result<Habit, String> {
        let h = self.habits.iter_mut().find(|h| h.id == id).ok_or_else(not_found)?;
        if let Some(title) = &patch.title {
            h.title = clean_title(title, MAX_HABIT_TITLE_CHARS, "title")?;
        }
        if let Some(nudge) = &patch.nudge {
            h.nudge = nudge_arg(nudge)?;
        }
        if let Some(icon) = patch.icon {
            h.icon = icon;
        }
        if let Some(days) = patch.days {
            h.days = days;
        }
        Ok(h.clone())
    }

    pub fn delete_habit(&mut self, id: &str) -> Result<(), String> {
        let before = self.habits.len();
        self.habits.retain(|h| h.id != id);
        if self.habits.len() == before { Err(not_found()) } else { Ok(()) }
    }

    /// Marks (or unmarks) `day_key` done; never a day after `today`.
    pub fn check_habit(&mut self, id: &str, day_key: &str, done: bool, today: &str) -> Result<Habit, String> {
        let h = self.habits.iter_mut().find(|h| h.id == id).ok_or_else(not_found)?;
        if days_of_key(day_key).is_none() || day_key > today {
            return Err(invalid("dayKey"));
        }
        match (h.log.binary_search_by(|k| k.as_str().cmp(day_key)), done) {
            (Err(at), true) => h.log.insert(at, day_key.to_string()),
            (Ok(at), false) => {
                h.log.remove(at);
            }
            _ => {}
        }
        let excess = h.log.len().saturating_sub(MAX_LOG_DAYS);
        h.log.drain(..excess);
        Ok(h.clone())
    }
}

// ── Stored entries ────────────────────────────────────────────────────────────

/// A stored entry must still pass today's rules (cleaned on the way in), or
/// the file is suspect.
fn stored_task(v: &Value) -> Option<Task> {
    let mut t: Task = serde_json::from_value(v.clone()).ok()?;
    if !valid_id(&t.id) {
        return None;
    }
    t.title = clean_title(&t.title, MAX_TITLE_CHARS, "").ok()?;
    t.note = clean_text(&t.note, MAX_TASK_NOTE_CHARS, true, "").ok()?;
    if t.due.as_deref().is_some_and(|d| days_of_key(d).is_none()) {
        return None;
    }
    if !t.done {
        t.done_at = None;
    }
    Some(t)
}

fn stored_note(v: &Value) -> Option<Note> {
    let mut n: Note = serde_json::from_value(v.clone()).ok()?;
    if !valid_id(&n.id) {
        return None;
    }
    n.text = clean_text(&n.text, MAX_NOTE_CHARS, false, "").ok()?;
    Some(n)
}

fn stored_reminder(v: &Value) -> Option<Reminder> {
    let mut r: Reminder = serde_json::from_value(v.clone()).ok()?;
    if !valid_id(&r.id) || !(0..=MAX_MS as i64).contains(&r.at) {
        return None;
    }
    r.title = clean_title(&r.title, MAX_TITLE_CHARS, "").ok()?;
    Some(r)
}

fn stored_habit(v: &Value) -> Option<Habit> {
    let mut h: Habit = serde_json::from_value(v.clone()).ok()?;
    if !valid_id(&h.id) {
        return None;
    }
    h.title = clean_title(&h.title, MAX_HABIT_TITLE_CHARS, "").ok()?;
    if let Some(n) = &h.nudge {
        h.nudge = nudge_arg(&NudgeArg { every_minutes: n.every_minutes, from: n.from.clone(), to: n.to.clone() }).ok()?;
    }
    if h.log.iter().any(|k| days_of_key(k).is_none()) {
        return None;
    }
    h.log.sort();
    h.log.dedup();
    let excess = h.log.len().saturating_sub(MAX_LOG_DAYS);
    h.log.drain(..excess);
    Some(h)
}

/// A timer whose fields contradict each other is reset rather than trusted.
fn stored_focus(v: &Value) -> Option<FocusState> {
    let f: FocusState = serde_json::from_value(v.clone()).ok()?;
    let running = matches!(f.phase, Phase::Focus | Phase::Break | Phase::LongBreak);
    let consistent = match f.phase {
        Phase::Idle => f.ends_at.is_none(),
        Phase::Paused => f.remaining_ms.is_some_and(|r| r >= 0) && matches!(f.paused_from, Some(Phase::Focus | Phase::Break | Phase::LongBreak)),
        _ => running && f.ends_at.is_some(),
    };
    let next_ok = matches!(f.next, Phase::Focus | Phase::Break | Phase::LongBreak);
    (consistent && next_ok && (1..=100).contains(&f.round) && f.rounds_done_today <= 1000).then_some(f)
}

#[derive(Deserialize)]
struct RawFile {
    version: u32,
    #[serde(default)]
    tasks: Vec<Value>,
    #[serde(default)]
    notes: Vec<Value>,
    #[serde(default)]
    reminders: Vec<Value>,
    #[serde(default)]
    habits: Vec<Value>,
    #[serde(default)]
    focus: Option<Value>,
}

trait HasId {
    fn id(&self) -> &str;
}
impl HasId for Task {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for Note {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for Reminder {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for Habit {
    fn id(&self) -> &str {
        &self.id
    }
}

/// The valid, unique entries of one list within its limit, and how many were left out.
fn keep<T: HasId>(values: &[Value], max: usize, parse: impl Fn(&Value) -> Option<T>) -> (Vec<T>, usize) {
    let mut out: Vec<T> = Vec::new();
    for v in values {
        if let Some(item) = parse(v) {
            if out.len() < max && !out.iter().any(|o| o.id() == item.id()) {
                out.push(item);
            }
        }
    }
    let dropped = values.len() - out.len();
    (out, dropped)
}

/// The data in a parsed file, and how many entries were unusable.
fn from_raw(raw: &RawFile) -> (PlannerData, usize) {
    let (tasks, a) = keep(&raw.tasks, MAX_TASKS, stored_task);
    let (notes, b) = keep(&raw.notes, MAX_NOTES, stored_note);
    let (reminders, c) = keep(&raw.reminders, MAX_REMINDERS, stored_reminder);
    let (habits, d) = keep(&raw.habits, MAX_HABITS, stored_habit);
    let (focus, e) = match &raw.focus {
        None => (FocusState::default(), 0),
        Some(v) => stored_focus(v).map_or((FocusState::default(), 1), |f| (f, 0)),
    };
    (PlannerData { version: FILE_VERSION, tasks, notes, reminders, habits, focus }, a + b + c + d + e)
}

// ── File ──────────────────────────────────────────────────────────────────────

/// How a suspect file is kept.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Aside {
    /// Moved away: nothing replaces it right now.
    Move,
    /// Copied: the cleaned data is about to replace it atomically, so
    /// planner.json never goes missing in between.
    Copy,
}

/// Keeps a suspect file as planner.json.bad-<time> so no later write can
/// destroy it. An error means it could NOT be kept: the caller must then
/// not let anything be written over it.
fn quarantine(path: &Path, why: &str, how: Aside) -> std::io::Result<()> {
    let backup = path.with_file_name(format!("{FILE_NAME}.bad-{}", super::now_ms()));
    let kept = match how {
        Aside::Move => std::fs::rename(path, &backup),
        Aside::Copy => std::fs::copy(path, &backup).map(|_| ()),
    };
    match kept {
        Ok(()) => {
            plog(format!("planner: {why}; kept the old file as {}", backup.display()));
            Ok(())
        }
        Err(err) => {
            plog(format!("planner: {why}; could not keep it aside ({err}), nothing will be written over it"));
            Err(err)
        }
    }
}

/// Reads the planner. Missing file = empty planner. A damaged file is set
/// aside (and logged) and whatever was valid in it is kept and written back.
/// A file that is there but cannot be read — or a damaged one that could not
/// be set aside — is an error, so nothing is written over it.
pub fn load_from(path: &Path) -> std::io::Result<PlannerData> {
    load_with(path, quarantine)
}

/// `load_from` with the quarantine step passed in (the tests make it fail).
fn load_with(path: &Path, quarantine: impl Fn(&Path, &str, Aside) -> std::io::Result<()>) -> std::io::Result<PlannerData> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(PlannerData::default()),
        Err(err) => {
            plog(format!("planner: could not read {}: {err}", path.display()));
            return Err(err);
        }
    };
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let raw: RawFile = match serde_json::from_slice(body) {
        Ok(raw) => raw,
        Err(err) => {
            quarantine(path, &format!("{FILE_NAME} is not valid ({err})"), Aside::Move)?;
            return Ok(PlannerData::default());
        }
    };
    let (data, dropped) = from_raw(&raw);
    if dropped > 0 || raw.version != FILE_VERSION {
        // Copied, not moved: if the rewrite below fails, the original is still
        // planner.json and the next launch reads it again.
        quarantine(path, &format!("{FILE_NAME} had {dropped} unusable entries (format v{})", raw.version), Aside::Copy)?;
        if let Err(err) = save_to(path, &data) {
            plog(format!("planner: could not rewrite the cleaned file: {err}"));
        }
    }
    Ok(data)
}

/// Atomic: the old file stays intact until the new one is completely on disk.
pub fn save_to(path: &Path, data: &PlannerData) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec(data)?;
    let tmp = path.with_file_name(format!("{FILE_NAME}.tmp"));
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::super::schedule::{local_to_utc, FixedZone, HOUR};
    use super::super::{Kind, Planner};
    use super::*;
    use crate::planner::jalali::days_from_civil;
    use std::path::PathBuf;

    const IR: FixedZone = FixedZone(3 * HOUR + 30 * MINUTE);

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roadeep-planner-{tag}-{}-{}", std::process::id(), new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn backups(dir: &Path) -> usize {
        std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("planner.json.bad-")).count()
    }

    fn now_at(h: i64) -> i64 {
        local_to_utc(&IR, days_from_civil(2026, 10, 3), h * HOUR)
    }

    #[test]
    fn text_is_cleaned_and_bounded() {
        assert_eq!(clean_title("  Buy\u{202E} milk \n now ", 200, "title").unwrap(), "Buy milk now");
        assert_eq!(clean_title("می‌خوام", 200, "title").unwrap(), "می‌خوام", "ZWNJ is kept");
        assert_eq!(clean_title("   ", 200, "title").unwrap_err(), "E_PLANNER_INVALID|title");
        assert!(clean_title(&"ن".repeat(200), 200, "title").is_ok(), "characters, not bytes");
        assert!(clean_title(&"n".repeat(201), 200, "title").is_err());
        assert_eq!(clean_text(" a\r\nb\u{0007}\u{2066}c ", 100, false, "text").unwrap(), "a\nbc");
        assert_eq!(clean_text("", 100, true, "note").unwrap(), "");
        assert_eq!(clean_text("\n\n", 100, false, "text").unwrap_err(), "E_PLANNER_INVALID|text");
        assert!(ms_arg(f64::NAN, "at").is_err() && ms_arg(-1.0, "at").is_err() && ms_arg(1e15, "at").is_err());
        assert_eq!(ms_arg(1_759_000_000_000.4, "at").unwrap(), 1_759_000_000_000);
    }

    #[test]
    fn tasks_add_update_complete_and_delete() {
        let mut d = PlannerData::default();
        let t = d.add_task(" Pay rent ", Some("2026-10-05"), None, 10).unwrap();
        assert_eq!((t.title.as_str(), t.due.as_deref(), t.done), ("Pay rent", Some("2026-10-05"), false));
        assert_eq!(d.add_task("x", Some("2026-02-30"), None, 10).unwrap_err(), "E_PLANNER_INVALID|due");
        assert!(d.add_task("x", Some(""), None, 10).unwrap().due.is_none(), "empty = no due day");

        let patch = |done| TaskPatch { title: None, note: None, due: None, done: Some(done) };
        let done = d.update_task(&t.id, patch(true), 20).unwrap();
        assert_eq!((done.done, done.done_at, done.updated_at), (true, Some(20), 20));
        let again = d.update_task(&t.id, patch(true), 30).unwrap();
        assert_eq!((again.done_at, again.updated_at), (Some(20), 20), "completing twice keeps the first time");
        let undone = d.update_task(&t.id, patch(false), 40).unwrap();
        assert_eq!((undone.done, undone.done_at), (false, None));
        let cleared = d.update_task(&t.id, TaskPatch { title: None, note: Some("a\nb".into()), due: Some(String::new()), done: None }, 50).unwrap();
        assert_eq!((cleared.due, cleared.note.as_str()), (None, "a\nb"));
        assert_eq!(d.update_task("nope", patch(true), 1).unwrap_err(), "E_PLANNER_NOT_FOUND");
        d.delete_task(&t.id).unwrap();
        assert_eq!(d.delete_task(&t.id).unwrap_err(), "E_PLANNER_NOT_FOUND");
    }

    #[test]
    fn limits_hold() {
        let mut d = PlannerData::default();
        for i in 0..MAX_TASKS {
            d.tasks.push(Task { id: new_id(), title: format!("t{i}"), note: String::new(), done: false, due: None, created_at: 0, updated_at: 0, done_at: None });
        }
        assert_eq!(d.add_task("one more", None, None, 0).unwrap_err(), "E_PLANNER_LIMIT|2000");
        for _ in 0..MAX_HABITS {
            d.add_habit("h", HabitIcon::Water, HabitDays::Daily, None, 0).unwrap();
        }
        assert_eq!(d.add_habit("h", HabitIcon::Water, HabitDays::Daily, None, 0).unwrap_err(), "E_PLANNER_LIMIT|100");
        let h = d.habits[0].id.clone();
        for day in 0..(MAX_LOG_DAYS as i64 + 5) {
            d.check_habit(&h, &crate::planner::jalali::day_key_of(days_from_civil(2010, 1, 1) + day), true, "2030-01-01").unwrap();
        }
        let log = &d.habits[0].log;
        assert_eq!(log.len(), MAX_LOG_DAYS);
        assert_eq!(log[0], "2010-01-06", "the oldest days go first");
    }

    #[test]
    fn reminders_are_validated_and_normalized() {
        let mut d = PlannerData::default();
        let now = Now::new(now_at(10), &IR);
        assert_eq!(d.add_reminder("x", now.ms - HOUR, Repeat::None, &now).unwrap_err(), "E_PLANNER_INVALID|at");
        assert!(d.add_reminder("x", now.ms - 30_000, Repeat::None, &now).is_ok(), "a click on now is fine");
        // A daily reminder at 09:00 set at 10:00 starts tomorrow.
        let daily = d.add_reminder("Pills", now_at(9), Repeat::Daily, &now).unwrap();
        assert_eq!(daily.at, now_at(9) + schedule::DAY);
        // Weekdays set for Thursday 2026-10-08 moves to Saturday.
        let thu = local_to_utc(&IR, days_from_civil(2026, 10, 8), 9 * HOUR);
        let work = d.add_reminder("Standup", thu, Repeat::Weekdays, &now).unwrap();
        assert_eq!(work.at, local_to_utc(&IR, days_from_civil(2026, 10, 10), 9 * HOUR));

        let snoozed = d.snooze_reminder(&daily.id, 10.0, now.ms).unwrap();
        assert_eq!(snoozed.snoozed_until, Some(now.ms + 10 * MINUTE));
        assert_eq!(d.snooze_reminder(&daily.id, 0.0, now.ms).unwrap_err(), "E_PLANNER_INVALID|minutes");
        let off = d.update_reminder(&daily.id, ReminderPatch { title: None, at: None, repeat: None, enabled: Some(false) }, &now).unwrap();
        assert_eq!((off.enabled, off.snoozed_until), (false, None), "switching off drops the snooze");
        assert_eq!(d.snooze_reminder(&daily.id, 10.0, now.ms).unwrap_err(), "E_PLANNER_INVALID|enabled");

        // A fired one-off re-armed by a new time.
        let mut one = d.add_reminder("Tea", now_at(11), Repeat::None, &now).unwrap();
        d.reminders.iter_mut().find(|r| r.id == one.id).unwrap().last_fired_at = Some(now_at(11));
        let later = Now::new(now_at(12), &IR);
        one = d.update_reminder(&one.id, ReminderPatch { title: Some("Green tea".into()), at: Some(now_at(13)), repeat: None, enabled: None }, &later).unwrap();
        assert_eq!(schedule::reminder_due(&one), Some(now_at(13)));
        assert_eq!(one.title, "Green tea");
    }

    #[test]
    fn habits_nudges_and_checks() {
        let mut d = PlannerData::default();
        let nudge = |every, from: &str, to: &str| NudgeArg { every_minutes: every, from: from.into(), to: to.into() };
        let h = d.add_habit("Water", HabitIcon::Water, HabitDays::Daily, Some(&nudge(60, "09:00", "18:00")), 0).unwrap();
        assert_eq!(h.nudge.as_ref().unwrap().every_minutes, 60);
        assert!(d.add_habit("x", HabitIcon::Eyes, HabitDays::Daily, Some(&nudge(0, "", "")), 0).unwrap().nudge.is_none(), "0 = off");
        for bad in [nudge(10, "09:00", "18:00"), nudge(300, "09:00", "18:00"), nudge(30, "18:00", "09:00"), nudge(30, "9:00", "18:00")] {
            assert_eq!(d.add_habit("x", HabitIcon::Eyes, HabitDays::Daily, Some(&bad), 0).unwrap_err(), "E_PLANNER_INVALID|nudge");
        }
        let checked = d.check_habit(&h.id, "2026-10-03", true, "2026-10-03").unwrap();
        d.check_habit(&h.id, "2026-10-01", true, "2026-10-03").unwrap();
        let both = d.check_habit(&h.id, "2026-10-03", true, "2026-10-03").unwrap();
        assert_eq!(checked.log, vec!["2026-10-03"]);
        assert_eq!(both.log, vec!["2026-10-01", "2026-10-03"], "sorted and unique");
        assert_eq!(d.check_habit(&h.id, "2026-10-04", true, "2026-10-03").unwrap_err(), "E_PLANNER_INVALID|dayKey", "not the future");
        assert_eq!(d.check_habit(&h.id, "10/03", true, "2026-10-03").unwrap_err(), "E_PLANNER_INVALID|dayKey");
        assert_eq!(d.check_habit(&h.id, "2026-10-03", false, "2026-10-03").unwrap().log, vec!["2026-10-01"]);
        let off = d.update_habit(&h.id, HabitPatch { title: None, icon: Some(HabitIcon::Walk), days: None, nudge: Some(nudge(0, "", "")) }).unwrap();
        assert_eq!((off.icon, off.nudge), (HabitIcon::Walk, None));
    }

    #[test]
    fn file_round_trip_bom_and_old_fields() {
        let dir = temp_dir("roundtrip");
        let path = dir.join(FILE_NAME);
        assert_eq!(load_from(&path).unwrap(), PlannerData::default(), "missing file = empty planner");
        let mut d = PlannerData::default();
        d.add_task("A", None, None, 1).unwrap();
        d.add_note("line 1\nline 2", 1).unwrap();
        d.add_habit("Stretch", HabitIcon::Stretch, HabitDays::Weekdays, None, 1).unwrap();
        save_to(&path, &d).unwrap();
        assert_eq!(load_from(&path).unwrap(), d);
        assert!(!dir.join("planner.json.tmp").exists());

        // Notepad's BOM, and a file without the fields added later.
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(br#"{ "version": 1, "tasks": [ { "id": "3f1c2b9e-0000-4000-8000-123456789abc", "title": "Old", "createdAt": 1, "updatedAt": 1 } ] }"#);
        std::fs::write(&path, &bom).unwrap();
        let old = load_from(&path).unwrap();
        assert_eq!(old.tasks.len(), 1);
        assert_eq!(old.focus, FocusState::default());
        assert_eq!(backups(&dir), 0, "an old but valid file is not quarantined");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_files_and_bad_entries_are_kept_aside() {
        let dir = temp_dir("corrupt");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(load_from(&path).unwrap(), PlannerData::default());
        assert_eq!(backups(&dir), 1);
        assert!(!path.exists(), "moved aside, not overwritten");

        let good = PlannerData::default().add_task("Good", None, None, 1).unwrap();
        let raw = serde_json::json!({ "version": 1,
            "tasks": [ good, { "id": "bad id", "title": "x", "createdAt": 1, "updatedAt": 1 }, good ],
            "reminders": [ { "id": new_id(), "title": "", "at": 1, "createdAt": 1 } ],
            "habits": [ { "id": new_id(), "title": "h", "icon": "dragon", "createdAt": 1 } ],
            "focus": { "phase": "focus", "endsAt": null, "remainingMs": null, "round": 1, "roundsDoneToday": 0, "startedAt": null, "dayKey": "" } });
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let cleaned = load_from(&path).unwrap();
        assert_eq!(cleaned.tasks, vec![good], "invalid and duplicate entries dropped");
        assert!(cleaned.reminders.is_empty() && cleaned.habits.is_empty());
        assert_eq!(cleaned.focus, FocusState::default(), "a contradictory timer is reset");
        assert_eq!(backups(&dir), 2);
        let original = serde_json::to_vec(&raw).unwrap();
        let kept: Vec<Vec<u8>> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("planner.json.bad-"))
            .map(|e| std::fs::read(e.path()).unwrap()).collect();
        assert!(kept.contains(&original), "the dropped entries survive in the copy");
        assert_eq!(load_from(&path).unwrap(), cleaned, "the rewritten file is clean");
        assert_eq!(backups(&dir), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_file_that_cannot_be_kept_aside_is_never_written_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp_dir("noquarantine");
        let path = dir.join(FILE_NAME);

        // Not JSON, and held open by someone (readable, but it can't be moved).
        std::fs::write(&path, b"{ not json").unwrap();
        {
            const FILE_SHARE_READ: u32 = 1;
            let _held = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(&path).unwrap();
            assert!(load_from(&path).is_err(), "not an empty planner");
            let planner = Planner::open(path.clone());
            let err = planner.mutate(|d| d.add_task("New", None, None, 2)).unwrap_err();
            assert!(err.starts_with("E_PLANNER_STORE|"), "{err}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json", "file untouched");
        assert_eq!(backups(&dir), 0);

        // Some entries unusable, and the copy aside fails: no cleaned rewrite.
        let good = PlannerData::default().add_task("Good", None, None, 1).unwrap();
        let raw = serde_json::json!({ "version": 1, "tasks": [ good, { "id": "bad id", "title": "x", "createdAt": 1, "updatedAt": 1 } ] });
        let original = serde_json::to_vec(&raw).unwrap();
        std::fs::write(&path, &original).unwrap();
        let fail = |_: &Path, _: &str, how: Aside| -> std::io::Result<()> {
            assert_eq!(how, Aside::Copy);
            Err(std::io::Error::other("disk full"))
        };
        assert!(load_with(&path, fail).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original, "the dropped entries are still in the file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_file_is_never_written_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp_dir("locked");
        let path = dir.join(FILE_NAME);
        let mut d = PlannerData::default();
        d.add_task("Keep me", None, None, 1).unwrap();
        save_to(&path, &d).unwrap();
        let before = std::fs::read(&path).unwrap();
        let planner = {
            let _held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
            let planner = Planner::open(path.clone());
            let err = planner.mutate(|d| d.add_task("New", None, None, 2)).unwrap_err();
            assert!(err.starts_with("E_PLANNER_STORE|"), "{err}");
            planner
        };
        assert_eq!(std::fs::read(&path).unwrap(), before, "file untouched");
        // Readable again: the next change starts from the real file.
        let (task, kinds) = planner.mutate(|d| d.add_task("New", None, None, 2)).unwrap();
        assert_eq!(kinds, vec![Kind::Tasks]);
        assert_eq!(load_from(&path).unwrap().tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), vec!["Keep me", &task.title]);
        // A change that changes nothing writes nothing.
        let (_, kinds) = planner.mutate(|d| d.update_task(&task.id, TaskPatch { title: Some("New".into()), note: None, due: None, done: None }, 3)).unwrap();
        assert!(kinds.is_empty());
        // A refused change leaves the data as it was.
        assert!(planner.mutate(|d| d.add_task("", None, None, 4)).is_err());
        assert_eq!(load_from(&path).unwrap().tasks.len(), 2);
        assert_eq!(backups(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
