//! The planner («برنامه‌ریز»): tasks, quick notes, timed reminders, habits and
//! a focus (Pomodoro) timer, all LOCAL to this PC in %APPDATA%\Roadeep\planner.json.
//! The island renders it (src/views/planner/), the chat assistant reaches it
//! through built-in tools (tools.rs), and one background task fires reminders,
//! focus phase ends and habit nudges (schedule.rs).
//!
//! # Contract with the front end (src/core/bridge-planner.ts mirrors this)
//!
//! All ids are UUID v4 strings. Times are `number` = ms since the epoch (UTC).
//! Day keys are `"YYYY-MM-DD"` GREGORIAN LOCAL dates (Windows time zone); the
//! UI shows them in Jalali. Every text is trimmed and stripped of control and
//! bidi characters; titles are one line.
//!
//! ```text
//! Task     { id, title (1..200), note (≤2000, multi-line), done, due: dayKey|null,
//!            createdAt, updatedAt, doneAt: number|null }
//! Note     { id, text (1..4000, multi-line), pinned, createdAt, updatedAt }
//! Reminder { id, title (1..200), at, repeat: "none"|"daily"|"weekdays"|"weekly",
//!            enabled, lastFiredAt: number|null, snoozedUntil: number|null, createdAt }
//!            `at` is the NEXT occurrence: a repeating reminder's `at` moves forward
//!            each time it fires. A one-off reminder with lastFiredAt >= at is done
//!            (fired, or skipped as too old); editing `at` to the future re-arms it.
//! Habit    { id, title (1..80), icon: "water"|"stretch"|"posture"|"eyes"|"walk"|"read"|"sleep"|"custom",
//!            days: "daily"|"weekdays", nudge: { everyMinutes (15..240), from: "HH:MM", to: "HH:MM" }|null,
//!            log: dayKey[] (sorted, unique, ≤ 3650), createdAt }
//! FocusState { phase: "idle"|"focus"|"break"|"longBreak"|"paused", endsAt: number|null,
//!            remainingMs: number|null (paused only), round (1-based in the cycle),
//!            roundsDoneToday, startedAt: number|null, dayKey,
//!            pausedFrom: "focus"|"break"|"longBreak"|null   (EXTRA: the phase a pause holds),
//!            next: "focus"|"break"|"longBreak"              (EXTRA: what focus_start starts when idle),
//!            durationMs: number|null                        (EXTRA: full length of the running/paused phase) }
//! PlannerData { version: 1, tasks, notes, reminders, habits, focus }
//! ```
//!
//! "Work week" (reminder repeat `weekdays`, habit days `weekdays`) is the
//! Iranian one: SATURDAY to WEDNESDAY. Thursday and Friday are off.
//!
//! ## Commands (`invoke(name, args)`; arg keys are camelCase)
//!
//! ```text
//! planner_get()                                                    -> PlannerData
//! planner_task_add({ title, due?: dayKey, note? })                 -> Task
//! planner_task_update({ id, title?, note?, due?: dayKey|null|"" (null/"" clear), done? }) -> Task
//! planner_task_delete({ id })                                      -> null
//! planner_note_add({ text })                                       -> Note
//! planner_note_update({ id, text?, pinned? })                      -> Note
//! planner_note_delete({ id })                                      -> null
//! planner_reminder_add({ title, at, repeat? })                     -> Reminder
//! planner_reminder_update({ id, title?, at?, repeat?, enabled? })  -> Reminder
//! planner_reminder_delete({ id })                                  -> null
//! planner_reminder_snooze({ id, minutes (1..240) })                -> Reminder
//! planner_habit_add({ title, icon, days?, nudge?: { everyMinutes, from, to } })  -> Habit
//! planner_habit_update({ id, title?, icon?, days?, nudge?: {…}|null }) -> Habit
//!     nudge: null or nudge.everyMinutes = 0 turns the nudge off (stored as null);
//!     no `nudge` key leaves it as it is.
//! planner_habit_delete({ id })                                     -> null
//! planner_habit_check({ id, dayKey, done: boolean })               -> Habit   (dayKey not in the future)
//! focus_start({ minutes?: 1..180, phase?: "focus"|"break"|"longBreak" }) -> FocusState
//!     No phase = `next`. Starting a focus right after a focus (break skipped)
//!     moves to the next round. No minutes = the settings' length for that phase.
//! focus_pause()  -> FocusState     (running → paused; otherwise unchanged)
//! focus_resume() -> FocusState     (paused → its phase; idle → starts `next`)
//! focus_skip()   -> FocusState     (ends the running/paused phase now; it counts as
//!                                   finished (a focus round is counted) but sends NO
//!                                   planner-fire: the island asked, it knows)
//! focus_stop()   -> FocusState     (back to idle, round 1, next focus; today's count kept)
//! focus_get()    -> FocusState
//! planner_pending_fires() -> planner-fire payload[]
//!     The fires sent before the island listened (the launch catch-ups, at most
//!     50, oldest first), then cleared. The island calls it once, right after
//!     subscribing to `planner-fire`; from then on nothing is kept.
//! ```
//!
//! A phase that ends on its own fires `planner-fire` and leaves the timer
//! `idle` (round kept, `next` set): the next phase only starts when the user
//! confirms from the island (focus_start / focus_resume).
//!
//! Errors are coded strings (errors.rs ↔ src/core/error-text.ts):
//! `E_PLANNER_STORE|detail`, `E_PLANNER_INVALID|field`, `E_PLANNER_NOT_FOUND`,
//! `E_PLANNER_LIMIT|limit`.
//!
//! ## Events (broadcast to every window)
//!
//! ```text
//! planner-changed { kind: "tasks"|"notes"|"reminders"|"habits"|"focus" }  after every change,
//!                 the scheduler's and the chat tools' included (one event per kind).
//! planner-fire    { kind: "reminder", id, title }
//!               | { kind: "focus", phase: "focusDone"|"breakDone"|"longBreakDone", round }
//!               | { kind: "habit", id, title, icon }
//! ```
//!
//! ## Scheduling rules (schedule.rs)
//!
//! - Missed while the app was closed or the PC slept: a reminder (or a focus
//!   phase end) less than 12 hours late fires once; an older one is skipped
//!   (a repeating reminder moves on to its next occurrence). Habit nudges are
//!   never caught up: a nudge more than 10 minutes late is dropped.
//! - Habit nudges: at `from + k × everyMinutes` (k ≥ 1, up to `to`), on the
//!   habit's days, only with settings.habitNudges, not when the habit is
//!   already checked today, not while the user has been idle > 10 minutes or
//!   the app is paused from the tray. Reminders and focus ends fire even when
//!   paused (the user set them).

pub mod jalali;
pub mod schedule;
pub mod store;
pub mod tools;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::errors;
use crate::settings::Settings;
use schedule::{FocusPrefs, SystemZone};

pub const CHANGED_EVENT: &str = "planner-changed";
pub const FIRE_EVENT: &str = "planner-fire";
pub const FILE_VERSION: u32 = 1;

// ── Data ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub due: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub done_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    None,
    Daily,
    Weekdays,
    Weekly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub at: i64,
    #[serde(default)]
    pub repeat: Repeat,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub last_fired_at: Option<i64>,
    #[serde(default)]
    pub snoozed_until: Option<i64>,
    pub created_at: i64,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HabitIcon {
    Water,
    Stretch,
    Posture,
    Eyes,
    Walk,
    Read,
    Sleep,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum HabitDays {
    #[default]
    Daily,
    Weekdays,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Nudge {
    pub every_minutes: u32,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    pub id: String,
    pub title: String,
    pub icon: HabitIcon,
    #[serde(default)]
    pub days: HabitDays,
    #[serde(default)]
    pub nudge: Option<Nudge>,
    #[serde(default)]
    pub log: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Focus,
    Break,
    LongBreak,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FocusState {
    pub phase: Phase,
    pub ends_at: Option<i64>,
    pub remaining_ms: Option<i64>,
    pub round: u32,
    pub rounds_done_today: u32,
    pub started_at: Option<i64>,
    pub day_key: String,
    #[serde(default)]
    pub paused_from: Option<Phase>,
    #[serde(default = "focus_phase")]
    pub next: Phase,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

fn focus_phase() -> Phase {
    Phase::Focus
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            ends_at: None,
            remaining_ms: None,
            round: 1,
            rounds_done_today: 0,
            started_at: None,
            day_key: String::new(),
            paused_from: None,
            next: Phase::Focus,
            duration_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlannerData {
    pub version: u32,
    pub tasks: Vec<Task>,
    pub notes: Vec<Note>,
    pub reminders: Vec<Reminder>,
    pub habits: Vec<Habit>,
    pub focus: FocusState,
}

impl Default for PlannerData {
    fn default() -> Self {
        Self {
            version: FILE_VERSION,
            tasks: Vec::new(),
            notes: Vec::new(),
            reminders: Vec::new(),
            habits: Vec::new(),
            focus: FocusState::default(),
        }
    }
}

/// Which part of the data changed: one `planner-changed` event each.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Tasks,
    Notes,
    Reminders,
    Habits,
    Focus,
}

/// The parts that differ between two versions of the data.
pub fn changed_kinds(before: &PlannerData, after: &PlannerData) -> Vec<Kind> {
    let mut out = Vec::new();
    if before.tasks != after.tasks {
        out.push(Kind::Tasks);
    }
    if before.notes != after.notes {
        out.push(Kind::Notes);
    }
    if before.reminders != after.reminders {
        out.push(Kind::Reminders);
    }
    if before.habits != after.habits {
        out.push(Kind::Habits);
    }
    if before.focus != after.focus {
        out.push(Kind::Focus);
    }
    out
}

// ── Shared helpers ────────────────────────────────────────────────────────────

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The app log, except under `cargo test`, which must never write to the
/// user's %LOCALAPPDATA%\com.roadeep.desktop. Only counts, ids and codes go in: never the
/// text of a task, note or reminder.
pub(crate) fn plog(message: impl AsRef<str>) {
    #[cfg(not(test))]
    crate::log::line(message);
    #[cfg(test)]
    let _ = message;
}

/// The planner settings out of the app settings.
pub fn prefs_of(settings: &Settings) -> FocusPrefs {
    FocusPrefs {
        focus_minutes: settings.focus_minutes,
        break_minutes: settings.break_minutes,
        long_break_minutes: settings.long_break_minutes,
        rounds_before_long_break: settings.rounds_before_long_break,
        habit_nudges: settings.habit_nudges,
    }
}

fn current_prefs(app: &AppHandle) -> FocusPrefs {
    use tauri::Manager;
    prefs_of(&app.state::<crate::Shared>().settings.lock().unwrap_or_else(|e| e.into_inner()))
}

// ── State ─────────────────────────────────────────────────────────────────────

struct Inner {
    data: PlannerData,
    /// False when the file exists but could not be read: nothing may be
    /// written over it until a read succeeds.
    readable: bool,
    sched: schedule::Runtime,
}

/// The planner as the app holds it: the file's content in memory, written
/// through (atomically) on every change.
pub struct Planner {
    path: PathBuf,
    inner: Mutex<Inner>,
    /// Wakes the scheduler after a change, so a new reminder is not up to a
    /// minute late.
    wake: tokio::sync::Notify,
}

impl Planner {
    pub fn open(path: PathBuf) -> Self {
        let (data, readable) = match store::load_from(&path) {
            Ok(data) => (data, true),
            Err(_) => (PlannerData::default(), false),
        };
        Self { path, inner: Mutex::new(Inner { data, readable, sched: schedule::Runtime::default() }), wake: tokio::sync::Notify::new() }
    }

    pub fn default_path() -> PathBuf {
        crate::settings::config_dir().join(store::FILE_NAME)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reads the file again when the last attempt failed.
    fn ensure_readable(&self, inner: &mut Inner) -> Result<(), String> {
        if inner.readable {
            return Ok(());
        }
        match store::load_from(&self.path) {
            Ok(data) => {
                inner.data = data;
                inner.readable = true;
                Ok(())
            }
            Err(err) => Err(store::store_error("refusing to write over a file that could not be read", &err)),
        }
    }

    /// Applies `f` to a copy of the data and, when anything changed, writes it
    /// and keeps it. A failed `f` or a failed write leaves everything as it was.
    pub fn mutate<T>(&self, f: impl FnOnce(&mut PlannerData) -> Result<T, String>) -> Result<(T, Vec<Kind>), String> {
        let mut inner = self.lock();
        self.ensure_readable(&mut inner)?;
        let mut next = inner.data.clone();
        let out = f(&mut next)?;
        let kinds = changed_kinds(&inner.data, &next);
        if !kinds.is_empty() {
            store::save_to(&self.path, &next).map_err(|e| store::store_error("save failed", &e))?;
            inner.data = next;
        }
        Ok((out, kinds))
    }

    /// One scheduler pass (schedule::tick) under the lock. A write that fails
    /// keeps the new state in memory anyway, so nothing fires twice; the next
    /// change writes it again.
    fn tick(&self, now: i64, ctx: &schedule::TickCtx<'_>) -> schedule::TickOut {
        let mut inner = self.lock();
        if self.ensure_readable(&mut inner).is_err() {
            return schedule::TickOut::default();
        }
        let before = inner.data.clone();
        let Inner { data, sched, .. } = &mut *inner;
        let mut out = schedule::tick(data, sched, now, ctx);
        out.changed = changed_kinds(&before, &inner.data);
        if !out.changed.is_empty() {
            if let Err(err) = store::save_to(&self.path, &inner.data) {
                plog(format!("planner: could not write after a scheduler pass: {err}"));
            }
        }
        out
    }

    /// The fires sent before the island listened (schedule::Runtime::hold).
    pub fn take_pending_fires(&self) -> Vec<schedule::Fire> {
        self.lock().sched.take_held()
    }
}

/// One `planner-changed` per kind, to every window.
fn emit_changed(app: &AppHandle, kinds: &[Kind]) {
    #[derive(Serialize, Clone)]
    struct Changed {
        kind: Kind,
    }
    for kind in kinds {
        if let Err(err) = app.emit(CHANGED_EVENT, Changed { kind: *kind }) {
            plog(format!("planner: could not emit {CHANGED_EVENT}: {err}"));
        }
    }
}

/// After a change from a command or a chat tool: the events, and the
/// scheduler woken so a new reminder or timer is seen at once.
pub(crate) fn announce(app: &AppHandle, planner: &Planner, kinds: &[Kind]) {
    emit_changed(app, kinds);
    if !kinds.is_empty() {
        planner.wake.notify_one();
    }
}

/// Runs a change from a command: today's date in the local zone, the change,
/// the events.
fn change<T>(
    app: &AppHandle,
    planner: &Planner,
    what: &str,
    f: impl FnOnce(&mut PlannerData, &schedule::Now<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let zone = SystemZone;
    let now = schedule::Now::new(now_ms(), &zone);
    match planner.mutate(|data| f(data, &now)) {
        Ok((out, kinds)) => {
            announce(app, planner, &kinds);
            Ok(out)
        }
        Err(err) => {
            plog(format!("planner: {what} refused ({})", err.lines().next().unwrap_or("")));
            Err(err)
        }
    }
}

/// Starts the scheduler (schedule::run). Called once from setup.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(schedule::run(app));
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeArg {
    pub every_minutes: u32,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
}

#[tauri::command]
pub fn planner_get(app: AppHandle, planner: State<Planner>) -> Result<PlannerData, String> {
    // Brings today's focus count up to date before showing it.
    change(&app, &planner, "get", |data, now| {
        schedule::roll_day(&mut data.focus, now);
        Ok(data.clone())
    })
}

#[tauri::command]
pub fn planner_task_add(app: AppHandle, planner: State<Planner>, title: String, due: Option<String>, note: Option<String>) -> Result<Task, String> {
    let task = change(&app, &planner, "task add", |d, now| d.add_task(&title, due.as_deref(), note.as_deref(), now.ms))?;
    plog(format!("planner: task added {}", task.id));
    Ok(task)
}

/// Present-and-null (`Some(None)`) told apart from absent (`None`).
fn nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

/// The arguments of a command read from the raw invoke body. Tauri's own
/// argument parsing reads `key: null` and a missing key the same way, and a
/// patch needs them apart: null clears, absent leaves alone.
fn body_args<T: serde::de::DeserializeOwned>(request: &tauri::ipc::Request<'_>) -> Result<T, String> {
    match request.body() {
        tauri::ipc::InvokeBody::Json(value) => serde_json::from_value(value.clone()).map_err(|_| invalid("arguments")),
        tauri::ipc::InvokeBody::Raw(_) => Err(invalid("arguments")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskUpdateArgs {
    id: String,
    title: Option<String>,
    note: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    due: Option<Option<String>>,
    done: Option<bool>,
}

/// `due: null` or `due: ""` clears the due day.
#[tauri::command]
pub fn planner_task_update(app: AppHandle, planner: State<Planner>, request: tauri::ipc::Request<'_>) -> Result<Task, String> {
    let args: TaskUpdateArgs = body_args(&request)?;
    let patch = store::TaskPatch { title: args.title, note: args.note, due: args.due.map(Option::unwrap_or_default), done: args.done };
    let task = change(&app, &planner, "task update", |d, now| d.update_task(&args.id, patch, now.ms))?;
    plog(format!("planner: task updated {}", task.id));
    Ok(task)
}

#[tauri::command]
pub fn planner_task_delete(app: AppHandle, planner: State<Planner>, id: String) -> Result<(), String> {
    change(&app, &planner, "task delete", |d, _| d.delete_task(&id))?;
    plog(format!("planner: task deleted {id}"));
    Ok(())
}

#[tauri::command]
pub fn planner_note_add(app: AppHandle, planner: State<Planner>, text: String) -> Result<Note, String> {
    let note = change(&app, &planner, "note add", |d, now| d.add_note(&text, now.ms))?;
    plog(format!("planner: note added {}", note.id));
    Ok(note)
}

#[tauri::command]
pub fn planner_note_update(app: AppHandle, planner: State<Planner>, id: String, text: Option<String>, pinned: Option<bool>) -> Result<Note, String> {
    let note = change(&app, &planner, "note update", |d, now| d.update_note(&id, text.as_deref(), pinned, now.ms))?;
    plog(format!("planner: note updated {}", note.id));
    Ok(note)
}

#[tauri::command]
pub fn planner_note_delete(app: AppHandle, planner: State<Planner>, id: String) -> Result<(), String> {
    change(&app, &planner, "note delete", |d, _| d.delete_note(&id))?;
    plog(format!("planner: note deleted {id}"));
    Ok(())
}

#[tauri::command]
pub fn planner_reminder_add(app: AppHandle, planner: State<Planner>, title: String, at: f64, repeat: Option<Repeat>) -> Result<Reminder, String> {
    let at = store::ms_arg(at, "at")?;
    let reminder = change(&app, &planner, "reminder add", |d, now| d.add_reminder(&title, at, repeat.unwrap_or_default(), now))?;
    plog(format!("planner: reminder added {}", reminder.id));
    Ok(reminder)
}

#[tauri::command]
pub fn planner_reminder_update(
    app: AppHandle,
    planner: State<Planner>,
    id: String,
    title: Option<String>,
    at: Option<f64>,
    repeat: Option<Repeat>,
    enabled: Option<bool>,
) -> Result<Reminder, String> {
    let at = at.map(|a| store::ms_arg(a, "at")).transpose()?;
    let patch = store::ReminderPatch { title, at, repeat, enabled };
    let reminder = change(&app, &planner, "reminder update", |d, now| d.update_reminder(&id, patch, now))?;
    plog(format!("planner: reminder updated {}", reminder.id));
    Ok(reminder)
}

#[tauri::command]
pub fn planner_reminder_delete(app: AppHandle, planner: State<Planner>, id: String) -> Result<(), String> {
    change(&app, &planner, "reminder delete", |d, _| d.delete_reminder(&id))?;
    plog(format!("planner: reminder deleted {id}"));
    Ok(())
}

#[tauri::command]
pub fn planner_reminder_snooze(app: AppHandle, planner: State<Planner>, id: String, minutes: f64) -> Result<Reminder, String> {
    let reminder = change(&app, &planner, "reminder snooze", |d, now| d.snooze_reminder(&id, minutes, now.ms))?;
    plog(format!("planner: reminder snoozed {} for {} min", reminder.id, minutes.round()));
    Ok(reminder)
}

#[tauri::command]
pub fn planner_habit_add(
    app: AppHandle,
    planner: State<Planner>,
    title: String,
    icon: HabitIcon,
    days: Option<HabitDays>,
    nudge: Option<NudgeArg>,
) -> Result<Habit, String> {
    let habit = change(&app, &planner, "habit add", |d, now| d.add_habit(&title, icon, days.unwrap_or_default(), nudge.as_ref(), now.ms))?;
    plog(format!("planner: habit added {}", habit.id));
    Ok(habit)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HabitUpdateArgs {
    id: String,
    title: Option<String>,
    icon: Option<HabitIcon>,
    days: Option<HabitDays>,
    #[serde(default, deserialize_with = "nullable")]
    nudge: Option<Option<NudgeArg>>,
}

/// `nudge: null` or `nudge: { everyMinutes: 0 }` turns the nudge off.
#[tauri::command]
pub fn planner_habit_update(app: AppHandle, planner: State<Planner>, request: tauri::ipc::Request<'_>) -> Result<Habit, String> {
    let args: HabitUpdateArgs = body_args(&request)?;
    let off = || NudgeArg { every_minutes: 0, from: String::new(), to: String::new() };
    let patch = store::HabitPatch { title: args.title, icon: args.icon, days: args.days, nudge: args.nudge.map(|n| n.unwrap_or_else(off)) };
    let habit = change(&app, &planner, "habit update", |d, _| d.update_habit(&args.id, patch))?;
    plog(format!("planner: habit updated {}", habit.id));
    Ok(habit)
}

#[tauri::command]
pub fn planner_habit_delete(app: AppHandle, planner: State<Planner>, id: String) -> Result<(), String> {
    change(&app, &planner, "habit delete", |d, _| d.delete_habit(&id))?;
    plog(format!("planner: habit deleted {id}"));
    Ok(())
}

#[tauri::command]
pub fn planner_habit_check(app: AppHandle, planner: State<Planner>, id: String, day_key: String, done: bool) -> Result<Habit, String> {
    let habit = change(&app, &planner, "habit check", |d, now| d.check_habit(&id, &day_key, done, &now.today))?;
    plog(format!("planner: habit {} {} for a day", habit.id, if done { "checked" } else { "unchecked" }));
    Ok(habit)
}

/// The focus commands share one shape: a change to the timer, today's prefs.
fn focus_change(app: &AppHandle, planner: &Planner, what: &str, f: impl FnOnce(&mut FocusState, &schedule::Now<'_>, &FocusPrefs) -> Result<(), String>) -> Result<FocusState, String> {
    let prefs = current_prefs(app);
    let state = change(app, planner, what, |d, now| {
        schedule::roll_day(&mut d.focus, now);
        f(&mut d.focus, now, &prefs)?;
        Ok(d.focus.clone())
    })?;
    plog(format!("planner: focus {what} → {:?} round {}", state.phase, state.round));
    Ok(state)
}

#[tauri::command]
pub fn focus_start(app: AppHandle, planner: State<Planner>, minutes: Option<f64>, phase: Option<Phase>) -> Result<FocusState, String> {
    focus_change(&app, &planner, "start", |f, now, prefs| schedule::focus_start(f, phase, minutes, now.ms, prefs))
}

#[tauri::command]
pub fn focus_pause(app: AppHandle, planner: State<Planner>) -> Result<FocusState, String> {
    focus_change(&app, &planner, "pause", |f, now, _| {
        schedule::focus_pause(f, now.ms);
        Ok(())
    })
}

#[tauri::command]
pub fn focus_resume(app: AppHandle, planner: State<Planner>) -> Result<FocusState, String> {
    focus_change(&app, &planner, "resume", |f, now, prefs| schedule::focus_resume(f, now.ms, prefs))
}

#[tauri::command]
pub fn focus_skip(app: AppHandle, planner: State<Planner>) -> Result<FocusState, String> {
    focus_change(&app, &planner, "skip", |f, _, prefs| {
        schedule::focus_skip(f, prefs);
        Ok(())
    })
}

#[tauri::command]
pub fn focus_stop(app: AppHandle, planner: State<Planner>) -> Result<FocusState, String> {
    focus_change(&app, &planner, "stop", |f, _, _| {
        schedule::focus_stop(f);
        Ok(())
    })
}

#[tauri::command]
pub fn focus_get(app: AppHandle, planner: State<Planner>) -> Result<FocusState, String> {
    change(&app, &planner, "focus get", |d, now| {
        schedule::roll_day(&mut d.focus, now);
        Ok(d.focus.clone())
    })
}

/// The island calls this once, right after subscribing to `planner-fire`.
#[tauri::command]
pub fn planner_pending_fires(planner: State<Planner>) -> Vec<schedule::Fire> {
    let fires = planner.take_pending_fires();
    if !fires.is_empty() {
        plog(format!("planner: {} alert(s) handed over after launch", fires.len()));
    }
    fires
}

/// `E_PLANNER_INVALID|field`.
pub(crate) fn invalid(field: &str) -> String {
    errors::coded(errors::PLANNER_INVALID, &[field])
}

pub(crate) fn not_found() -> String {
    errors::coded(errors::PLANNER_NOT_FOUND, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patches_tell_null_from_absent() {
        let parse = |v: serde_json::Value| serde_json::from_value::<TaskUpdateArgs>(v).unwrap();
        assert_eq!(parse(json!({ "id": "a" })).due, None, "absent: unchanged");
        assert_eq!(parse(json!({ "id": "a", "due": null })).due, Some(None), "null: cleared");
        assert_eq!(parse(json!({ "id": "a", "due": "2026-10-03" })).due, Some(Some("2026-10-03".into())));
        let habit = |v: serde_json::Value| serde_json::from_value::<HabitUpdateArgs>(v).unwrap().nudge.map(|n| n.map(|n| n.every_minutes));
        assert_eq!(habit(json!({ "id": "a" })), None);
        assert_eq!(habit(json!({ "id": "a", "nudge": null })), Some(None));
        assert_eq!(habit(json!({ "id": "a", "nudge": { "everyMinutes": 30, "from": "09:00", "to": "17:00" } })), Some(Some(30)));
    }

    #[test]
    fn the_data_serializes_to_the_documented_shape() {
        let v = serde_json::to_value(PlannerData::default()).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["focus"], json!({ "phase": "idle", "endsAt": null, "remainingMs": null, "round": 1, "roundsDoneToday": 0,
            "startedAt": null, "dayKey": "", "pausedFrom": null, "next": "focus", "durationMs": null }));
        assert_eq!(serde_json::to_value(Phase::LongBreak).unwrap(), "longBreak");
        assert_eq!(serde_json::to_value(Kind::Reminders).unwrap(), "reminders");
        let fire = schedule::Fire::Focus { phase: "focusDone", round: 2 };
        assert_eq!(serde_json::to_value(fire).unwrap(), json!({ "kind": "focus", "phase": "focusDone", "round": 2 }));
        let habit = schedule::Fire::Habit { id: "h".into(), title: "Water".into(), icon: HabitIcon::Water };
        assert_eq!(serde_json::to_value(habit).unwrap(), json!({ "kind": "habit", "id": "h", "title": "Water", "icon": "water" }));
    }
}
