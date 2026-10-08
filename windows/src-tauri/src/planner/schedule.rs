// Local time, repeat rules, the focus timer's phases and the one background
// task that fires reminders, focus phase ends and habit nudges.
//
// Everything that decides WHEN is a pure function over a clock value and a
// `Zone`, so the tests drive it with a fixed offset and a fake "now". The task
// (`run`) only reads the clock, asks `tick` what is due, emits, and sleeps
// until the next due moment — never longer than a minute, so an edit made
// while it sleeps (or a settings change) is picked up — and a change made
// through a command wakes it at once.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::jalali::{civil_from_days, day_key_of, days_from_civil, weekday};
use super::{plog, FocusState, Habit, HabitDays, HabitIcon, Kind, Phase, Planner, PlannerData, Repeat};

pub const MINUTE: i64 = 60_000;
pub const HOUR: i64 = 60 * MINUTE;
pub const DAY: i64 = 24 * HOUR;
/// A reminder or a focus end found later than this (app closed, PC asleep)
/// is skipped instead of fired: a 9 o'clock alarm the next evening is noise.
pub const STALE_AFTER: i64 = 12 * HOUR;
/// A habit nudge found later than this is dropped: it was about that moment.
pub const NUDGE_LATE: i64 = 10 * MINUTE;
/// No habit nudge after this long without keyboard or mouse input.
pub const IDLE_LIMIT_MS: u64 = 10 * 60 * 1000;
/// The scheduler re-reads everything at least this often.
const MAX_SLEEP_MS: i64 = 60_000;
/// Floor between passes, so a moment that can't be cleared never spins.
const MIN_SLEEP_MS: i64 = 200;

// ── Time zone ─────────────────────────────────────────────────────────────────

/// Local time = UTC + offset_ms(UTC).
pub trait Zone: Sync {
    fn offset_ms(&self, utc_ms: i64) -> i64;
}

/// A zone with one fixed offset (tests; Iran has had no DST since 2022).
#[cfg(test)]
pub struct FixedZone(pub i64);

#[cfg(test)]
impl Zone for FixedZone {
    fn offset_ms(&self, _utc_ms: i64) -> i64 {
        self.0
    }
}

/// The Windows time zone, DST rules included.
pub struct SystemZone;

static ZONE_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

impl Zone for SystemZone {
    fn offset_ms(&self, utc_ms: i64) -> i64 {
        use windows::Win32::Foundation::SYSTEMTIME;
        use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
        let whole = utc_ms - utc_ms.rem_euclid(1000);
        let (days, tod) = (whole.div_euclid(DAY), whole.rem_euclid(DAY));
        let (y, m, d) = civil_from_days(days);
        if !(1601..=30_000).contains(&y) {
            return 0;
        }
        let utc = SYSTEMTIME {
            wYear: y as u16,
            wMonth: m as u16,
            wDayOfWeek: 0,
            wDay: d as u16,
            wHour: (tod / HOUR) as u16,
            wMinute: ((tod / MINUTE) % 60) as u16,
            wSecond: ((tod / 1000) % 60) as u16,
            wMilliseconds: 0,
        };
        let mut local = SYSTEMTIME::default();
        // SAFETY: both pointers are to live SYSTEMTIMEs; None = the active zone.
        match unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) } {
            Ok(()) => {
                let local_ms = days_from_civil(i64::from(local.wYear), u32::from(local.wMonth), u32::from(local.wDay)) * DAY
                    + i64::from(local.wHour) * HOUR
                    + i64::from(local.wMinute) * MINUTE
                    + i64::from(local.wSecond) * 1000;
                local_ms - whole
            }
            Err(err) => {
                if !ZONE_FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
                    plog(format!("planner: local time conversion failed, using UTC: {err}"));
                }
                0
            }
        }
    }
}

/// (days since 1970-01-01, ms into that day) of `utc_ms` in local time.
pub fn local_parts(zone: &dyn Zone, utc_ms: i64) -> (i64, i64) {
    let local = utc_ms + zone.offset_ms(utc_ms);
    (local.div_euclid(DAY), local.rem_euclid(DAY))
}

/// The UTC moment of a local day and time. Two passes so a DST change between
/// the guess and the answer is accounted for.
pub fn local_to_utc(zone: &dyn Zone, days: i64, ms_of_day: i64) -> i64 {
    let local = days * DAY + ms_of_day;
    let guess = local - zone.offset_ms(local);
    local - zone.offset_ms(guess)
}

/// The local "YYYY-MM-DD" of a moment.
#[cfg(test)]
pub fn day_key(zone: &dyn Zone, utc_ms: i64) -> String {
    day_key_of(local_parts(zone, utc_ms).0)
}

/// "2026-10-03T14:30" in local time (what the chat tools print).
pub fn local_iso(zone: &dyn Zone, utc_ms: i64) -> String {
    let (days, tod) = local_parts(zone, utc_ms);
    format!("{}T{:02}:{:02}", day_key_of(days), tod / HOUR, (tod / MINUTE) % 60)
}

/// The Iranian work week: Saturday to Wednesday (Thursday and Friday off).
pub fn is_workday(days: i64) -> bool {
    weekday(days) <= 4
}

/// A moment and the local date it falls on.
pub struct Now<'z> {
    pub ms: i64,
    pub zone: &'z dyn Zone,
    pub today: String,
    pub today_days: i64,
}

impl<'z> Now<'z> {
    pub fn new(ms: i64, zone: &'z dyn Zone) -> Self {
        let today_days = local_parts(zone, ms).0;
        Self { ms, zone, today: day_key_of(today_days), today_days }
    }
}

// ── Reminders ─────────────────────────────────────────────────────────────────

fn occurs_on(repeat: Repeat, days: i64) -> bool {
    repeat != Repeat::Weekdays || is_workday(days)
}

/// A `weekdays` reminder set on a Thursday or Friday moves to the next
/// workday at the same time; other repeats are left alone.
pub fn align(at: i64, repeat: Repeat, zone: &dyn Zone) -> i64 {
    if repeat != Repeat::Weekdays {
        return at;
    }
    let (mut days, tod) = local_parts(zone, at);
    if is_workday(days) {
        return at;
    }
    while !is_workday(days) {
        days += 1;
    }
    local_to_utc(zone, days, tod)
}

/// The first occurrence of a repeating reminder strictly after `now`, at the
/// same local time of day as `at`. A one-off reminder has none: `at` itself.
pub fn roll_forward(at: i64, repeat: Repeat, zone: &dyn Zone, now: i64) -> i64 {
    if repeat == Repeat::None || at > now {
        return at;
    }
    let (at_days, tod) = local_parts(zone, at);
    let now_days = local_parts(zone, now).0;
    // Start just before today so a long absence doesn't walk every missed day.
    let (start, step) = match repeat {
        Repeat::Weekly => (at_days + ((now_days - at_days) / 7 - 1).max(0) * 7, 7),
        _ => (at_days.max(now_days - 2), 1),
    };
    let mut days = start;
    for _ in 0..30 {
        days += step;
        if occurs_on(repeat, days) {
            let t = local_to_utc(zone, days, tod);
            if t > now {
                return t;
            }
        }
    }
    // Unreachable with a sane zone; never leave a reminder due forever.
    now + DAY
}

/// When a reminder is next due: its `at` while that occurrence hasn't fired,
/// or its snooze, whichever comes first. None when it's off or done.
pub fn reminder_due(r: &super::Reminder) -> Option<i64> {
    if !r.enabled {
        return None;
    }
    let at = r.last_fired_at.is_none_or(|last| last < r.at).then_some(r.at);
    match (at, r.snoozed_until) {
        (Some(a), Some(s)) => Some(a.min(s)),
        (a, s) => a.or(s),
    }
}

// ── Focus timer ───────────────────────────────────────────────────────────────

/// What the timer and the nudges read from the settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusPrefs {
    pub focus_minutes: u32,
    pub break_minutes: u32,
    pub long_break_minutes: u32,
    pub rounds_before_long_break: u32,
    pub habit_nudges: bool,
}

impl Default for FocusPrefs {
    fn default() -> Self {
        Self { focus_minutes: 25, break_minutes: 5, long_break_minutes: 15, rounds_before_long_break: 4, habit_nudges: true }
    }
}

impl FocusPrefs {
    fn minutes(&self, phase: Phase) -> u32 {
        match phase {
            Phase::Break => self.break_minutes,
            Phase::LongBreak => self.long_break_minutes,
            _ => self.focus_minutes,
        }
        .max(1)
    }

    fn rounds(&self) -> u32 {
        self.rounds_before_long_break.max(1)
    }
}

const MAX_FOCUS_MINUTES: f64 = 180.0;

fn running(phase: Phase) -> bool {
    matches!(phase, Phase::Focus | Phase::Break | Phase::LongBreak)
}

/// Today's count starts again on a new local day.
pub fn roll_day(f: &mut FocusState, now: &Now<'_>) {
    if f.day_key != now.today {
        f.day_key = now.today.clone();
        f.rounds_done_today = 0;
    }
}

fn next_round(round: u32, prefs: &FocusPrefs) -> u32 {
    if round >= prefs.rounds() {
        1
    } else {
        round + 1
    }
}

fn clear_timing(f: &mut FocusState) {
    f.phase = Phase::Idle;
    f.ends_at = None;
    f.remaining_ms = None;
    f.started_at = None;
    f.paused_from = None;
    f.duration_ms = None;
}

/// Starts `phase` (default: what is running or paused, else `next`) for
/// `minutes` (default: the settings' length for that phase).
pub fn focus_start(f: &mut FocusState, phase: Option<Phase>, minutes: Option<f64>, now: i64, prefs: &FocusPrefs) -> Result<(), String> {
    let current = match f.phase {
        Phase::Paused => f.paused_from.unwrap_or(f.next),
        Phase::Idle => f.next,
        p => p,
    };
    let phase = phase.unwrap_or(current);
    if !running(phase) {
        return Err(super::invalid("phase"));
    }
    let minutes = match minutes {
        Some(m) if m.is_finite() && (1.0..=MAX_FOCUS_MINUTES).contains(&m.round()) => m.round() as i64,
        Some(_) => return Err(super::invalid("minutes")),
        None => i64::from(prefs.minutes(phase)),
    };
    // A focus right after a focus (the break skipped) is the next round.
    if phase == Phase::Focus && f.phase == Phase::Idle && f.next != Phase::Focus {
        f.round = next_round(f.round, prefs);
    }
    f.round = f.round.clamp(1, prefs.rounds());
    let duration = minutes * MINUTE;
    f.phase = phase;
    f.started_at = Some(now);
    f.ends_at = Some(now + duration);
    f.duration_ms = Some(duration);
    f.remaining_ms = None;
    f.paused_from = None;
    Ok(())
}

pub fn focus_pause(f: &mut FocusState, now: i64) {
    if !running(f.phase) {
        return;
    }
    f.remaining_ms = Some(f.ends_at.map_or(0, |end| (end - now).max(0)));
    f.paused_from = Some(f.phase);
    f.phase = Phase::Paused;
    f.ends_at = None;
}

pub fn focus_resume(f: &mut FocusState, now: i64, prefs: &FocusPrefs) -> Result<(), String> {
    match f.phase {
        Phase::Paused => {
            let phase = f.paused_from.filter(|p| running(*p)).unwrap_or(Phase::Focus);
            f.phase = phase;
            f.ends_at = Some(now + f.remaining_ms.unwrap_or(0).max(0));
            f.remaining_ms = None;
            f.paused_from = None;
            Ok(())
        }
        Phase::Idle => focus_start(f, None, None, now, prefs),
        _ => Ok(()),
    }
}

/// The running (or paused) phase is over: the timer goes idle with the next
/// phase ready, a focus round counted. Returns what `planner-fire` reports
/// (the phase's end and the round it belonged to).
fn end_phase(f: &mut FocusState, prefs: &FocusPrefs) -> Option<(&'static str, u32)> {
    let phase = match f.phase {
        Phase::Paused => f.paused_from.unwrap_or(Phase::Focus),
        p if running(p) => p,
        _ => return None,
    };
    let round = f.round;
    clear_timing(f);
    let name = match phase {
        Phase::Focus => {
            f.rounds_done_today = f.rounds_done_today.saturating_add(1);
            f.next = if round >= prefs.rounds() { Phase::LongBreak } else { Phase::Break };
            "focusDone"
        }
        Phase::Break => {
            f.next = Phase::Focus;
            f.round = next_round(round, prefs);
            "breakDone"
        }
        _ => {
            f.next = Phase::Focus;
            f.round = 1;
            "longBreakDone"
        }
    };
    Some((name, round))
}

/// "Done with this phase": it ends now and counts as finished, without a
/// `planner-fire` (the island asked for it and already knows).
pub fn focus_skip(f: &mut FocusState, prefs: &FocusPrefs) {
    end_phase(f, prefs);
}

/// Back to the start of a cycle; today's count stays.
pub fn focus_stop(f: &mut FocusState) {
    clear_timing(f);
    f.round = 1;
    f.next = Phase::Focus;
}

// ── Habit nudges ──────────────────────────────────────────────────────────────

/// "HH:MM" (00:00–23:59) → minutes into the day.
pub fn parse_hhmm(text: &str) -> Option<u32> {
    let (h, m) = text.split_once(':')?;
    if h.len() != 2 || m.len() != 2 || !h.bytes().chain(m.bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn habit_on(days: HabitDays, day: i64) -> bool {
    days == HabitDays::Daily || is_workday(day)
}

/// The nudge moments of `habit` on local day `day`: from + k × every (k ≥ 1)
/// up to `to`. Empty when the habit has no nudge or doesn't run that day.
fn nudge_slots(habit: &Habit, day: i64, zone: &dyn Zone) -> Vec<i64> {
    let Some(nudge) = habit.nudge.as_ref().filter(|n| n.every_minutes > 0) else { return Vec::new() };
    let (Some(from), Some(to)) = (parse_hhmm(&nudge.from), parse_hhmm(&nudge.to)) else { return Vec::new() };
    if !habit_on(habit.days, day) {
        return Vec::new();
    }
    let every = nudge.every_minutes;
    (1..)
        .map(|k| from + k * every)
        .take_while(|m| *m <= to)
        .map(|m| local_to_utc(zone, day, i64::from(m) * MINUTE))
        .collect()
}

/// The first nudge of `habit` strictly after `after` (looks a week ahead).
pub fn next_nudge(habit: &Habit, zone: &dyn Zone, after: i64) -> Option<i64> {
    let first = local_parts(zone, after).0;
    (first..first + 8).flat_map(|day| nudge_slots(habit, day, zone)).find(|t| *t > after)
}

/// The latest nudge of `habit` in (from, to].
fn latest_nudge(habit: &Habit, zone: &dyn Zone, from: i64, to: i64) -> Option<i64> {
    let (first, last) = (local_parts(zone, from).0, local_parts(zone, to).0);
    (first..=last).flat_map(|day| nudge_slots(habit, day, zone)).filter(|t| *t > from && *t <= to).max()
}

pub fn done_on(habit: &Habit, key: &str) -> bool {
    habit.log.binary_search_by(|k| k.as_str().cmp(key)).is_ok()
}

// ── One pass ──────────────────────────────────────────────────────────────────

/// What `planner-fire` carries.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fire {
    Reminder { id: String, title: String },
    Focus { phase: &'static str, round: u32 },
    Habit { id: String, title: String, icon: HabitIcon },
}

/// Fires kept for the island at most (a long absence shouldn't stack a wall of cards).
pub const HELD_MAX: usize = 50;

/// Scheduler memory that doesn't belong in the file.
#[derive(Debug, Default)]
pub struct Runtime {
    /// Habit nudges up to this moment are dealt with. None until the first
    /// pass, which starts it at "now": nudges are never caught up.
    nudge_cursor: Option<i64>,
    /// Fires sent before the island listened. The first pass runs during setup
    /// and fires the catch-ups (a reminder missed while the app was closed)
    /// before the island's webview has subscribed; Tauri drops an event nobody
    /// listens to, and lastFiredAt is already set, so without this they would
    /// be lost for good. The island drains it once, right after subscribing.
    held: VecDeque<Fire>,
    /// The island has drained `held`: it hears every fire live from then on.
    drained: bool,
}

impl Runtime {
    /// Keeps `fires` for the island until it has drained the backlog once.
    pub fn hold(&mut self, fires: &[Fire]) {
        if self.drained {
            return;
        }
        self.held.extend(fires.iter().cloned());
        while self.held.len() > HELD_MAX {
            self.held.pop_front();
        }
    }

    /// The fires kept so far, oldest first; nothing is kept after this.
    pub fn take_held(&mut self) -> Vec<Fire> {
        self.drained = true;
        self.held.drain(..).collect()
    }
}

pub struct TickCtx<'z> {
    pub zone: &'z dyn Zone,
    pub prefs: FocusPrefs,
    /// Since the last keyboard or mouse input; None when Windows didn't say.
    pub idle_ms: Option<u64>,
    /// Tray → Pause.
    pub paused: bool,
}

#[derive(Debug, Default)]
pub struct TickOut {
    pub fires: Vec<Fire>,
    /// Filled by Planner::tick from a before/after comparison.
    pub changed: Vec<Kind>,
    /// The next moment something may be due.
    pub next: Option<i64>,
}

/// Everything due at `now`: focus phase ends, reminders, habit nudges. Changes
/// `data` (the timer, lastFiredAt, the next occurrence) and says what fired
/// and when to look again.
pub fn tick(data: &mut PlannerData, rt: &mut Runtime, now_ms: i64, ctx: &TickCtx<'_>) -> TickOut {
    let now = Now::new(now_ms, ctx.zone);
    let mut out = TickOut::default();

    roll_day(&mut data.focus, &now);
    if running(data.focus.phase) && data.focus.ends_at.is_some_and(|end| end <= now_ms) {
        let late = now_ms - data.focus.ends_at.unwrap_or(now_ms);
        if let Some((phase, round)) = end_phase(&mut data.focus, &ctx.prefs) {
            if late < STALE_AFTER {
                plog(format!("planner: focus {phase} (round {round})"));
                out.fires.push(Fire::Focus { phase, round });
            } else {
                plog(format!("planner: focus {phase} found {} h late, not announced", late / HOUR));
            }
        }
    }

    for r in data.reminders.iter_mut().filter(|r| r.enabled) {
        let at_hit = r.last_fired_at.is_none_or(|last| last < r.at) && r.at <= now_ms;
        let snooze_hit = r.snoozed_until.is_some_and(|s| s <= now_ms);
        if !at_hit && !snooze_hit {
            continue;
        }
        // A repeating reminder missed for days still has a recent occurrence
        // (this morning's) that may be fresh enough to fire.
        let recent = at_hit.then(|| roll_forward(r.at, r.repeat, ctx.zone, now_ms - STALE_AFTER)).map(|t| if t <= now_ms { t } else { r.at });
        let latest = [recent, r.snoozed_until.filter(|_| snooze_hit)].into_iter().flatten().max().unwrap_or(now_ms);
        let stale = now_ms - latest >= STALE_AFTER;
        if snooze_hit {
            r.snoozed_until = None;
        }
        if at_hit {
            r.last_fired_at = Some(if stale { r.at } else { now_ms });
            r.at = roll_forward(r.at, r.repeat, ctx.zone, now_ms);
        } else if !stale {
            r.last_fired_at = Some(now_ms);
        }
        if stale {
            plog(format!("planner: reminder {} found {} h late, skipped", r.id, (now_ms - latest) / HOUR));
        } else {
            plog(format!("planner: reminder {} fired", r.id));
            out.fires.push(Fire::Reminder { id: r.id.clone(), title: r.title.clone() });
        }
    }

    let cursor = rt.nudge_cursor.unwrap_or(now_ms).max(now_ms - NUDGE_LATE);
    rt.nudge_cursor = Some(now_ms);
    if ctx.prefs.habit_nudges && cursor < now_ms {
        for habit in &data.habits {
            if latest_nudge(habit, ctx.zone, cursor, now_ms).is_none() {
                continue;
            }
            let why_not = if done_on(habit, &now.today) {
                Some("done today")
            } else if ctx.paused {
                Some("paused")
            } else if ctx.idle_ms.is_some_and(|idle| idle > IDLE_LIMIT_MS) {
                Some("user away")
            } else {
                None
            };
            match why_not {
                Some(why) => plog(format!("planner: habit nudge {} skipped ({why})", habit.id)),
                None => {
                    plog(format!("planner: habit nudge {}", habit.id));
                    out.fires.push(Fire::Habit { id: habit.id.clone(), title: habit.title.clone(), icon: habit.icon });
                }
            }
        }
    }

    rt.hold(&out.fires);
    out.next = next_due(data, ctx, now_ms);
    out
}

/// The earliest moment after `now` anything could be due.
pub fn next_due(data: &PlannerData, ctx: &TickCtx<'_>, now: i64) -> Option<i64> {
    let focus = data.focus.ends_at.filter(|_| running(data.focus.phase));
    let reminders = data.reminders.iter().filter_map(reminder_due);
    let nudges = data.habits.iter().filter(|_| ctx.prefs.habit_nudges).filter_map(|h| next_nudge(h, ctx.zone, now));
    focus.into_iter().chain(reminders).chain(nudges).min()
}

// ── The task ──────────────────────────────────────────────────────────────────

/// Milliseconds since the last keyboard or mouse input (GetLastInputInfo).
fn idle_ms() -> Option<u64> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `info` is a valid LASTINPUTINFO with cbSize set.
    let ok = unsafe { GetLastInputInfo(&mut info) }.as_bool();
    // Both are 32-bit tick counts; wrapping_sub survives the 49-day wrap.
    ok.then(|| u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime)))
}

/// The scheduler: one task for the life of the app. It sleeps until the next
/// due moment (at most a minute) or until a change wakes it.
pub async fn run(app: AppHandle) {
    plog("planner: scheduler started");
    loop {
        let planner = app.state::<Planner>();
        let now = super::now_ms();
        let ctx = TickCtx {
            zone: &SystemZone,
            prefs: super::current_prefs(&app),
            idle_ms: idle_ms(),
            paused: crate::integrations::PAUSED.load(Ordering::Relaxed),
        };
        let out = planner.tick(now, &ctx);
        for fire in &out.fires {
            if let Err(err) = app.emit(super::FIRE_EVENT, fire) {
                plog(format!("planner: could not emit {}: {err}", super::FIRE_EVENT));
            }
        }
        super::emit_changed(&app, &out.changed);
        let wait = out.next.map_or(MAX_SLEEP_MS, |next| next - super::now_ms()).clamp(MIN_SLEEP_MS, MAX_SLEEP_MS);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(wait as u64)) => {}
            _ = planner.wake.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Nudge, Reminder};
    use super::*;

    /// Tehran, +03:30.
    const IR: FixedZone = FixedZone(3 * HOUR + 30 * MINUTE);

    /// UTC ms of a Tehran local date-time.
    fn at(y: i64, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
        local_to_utc(&IR, days_from_civil(y, mo, d), h * HOUR + mi * MINUTE)
    }

    fn reminder(at_ms: i64, repeat: Repeat) -> Reminder {
        Reminder { id: "r1".into(), title: "Call".into(), at: at_ms, repeat, enabled: true, last_fired_at: None, snoozed_until: None, created_at: 0 }
    }

    fn ctx(prefs: FocusPrefs) -> TickCtx<'static> {
        TickCtx { zone: &IR, prefs, idle_ms: Some(0), paused: false }
    }

    fn data_with(reminders: Vec<Reminder>, habits: Vec<Habit>) -> PlannerData {
        PlannerData { reminders, habits, ..PlannerData::default() }
    }

    #[test]
    fn local_time_round_trips_through_the_zone() {
        let t = at(2026, 10, 3, 9, 15);
        assert_eq!(local_iso(&IR, t), "2026-10-03T09:15");
        assert_eq!(day_key(&IR, t), "2026-10-03");
        // 23:00 UTC on the 2nd is already the 3rd in Tehran.
        let late = days_from_civil(2026, 10, 2) * DAY + 23 * HOUR;
        assert_eq!(day_key(&IR, late), "2026-10-03");
        assert_eq!(Now::new(late, &IR).today, "2026-10-03");
    }

    #[test]
    fn repeats_roll_to_the_next_occurrence() {
        let sat = at(2026, 10, 3, 9, 0); // Saturday
        let now = sat + MINUTE;
        assert_eq!(roll_forward(sat, Repeat::Daily, &IR, now), at(2026, 10, 4, 9, 0));
        assert_eq!(roll_forward(sat, Repeat::Weekly, &IR, now), at(2026, 10, 10, 9, 0));
        assert_eq!(roll_forward(sat, Repeat::None, &IR, now), sat, "one-off reminders don't move");
        // Wednesday → skips Thursday and Friday.
        let wed = at(2026, 10, 7, 9, 0);
        assert_eq!(roll_forward(wed, Repeat::Weekdays, &IR, wed), at(2026, 10, 10, 9, 0));
        // A long absence lands on the first occurrence after now, same time of day.
        let much_later = at(2027, 1, 20, 12, 0);
        assert_eq!(roll_forward(sat, Repeat::Daily, &IR, much_later), at(2027, 1, 21, 9, 0));
        assert_eq!(roll_forward(sat, Repeat::Weekly, &IR, much_later), at(2027, 1, 23, 9, 0));
        // `at` still ahead: untouched.
        assert_eq!(roll_forward(sat, Repeat::Daily, &IR, sat - 1), sat);
        // A weekdays reminder set for a Friday moves to Saturday.
        assert_eq!(align(at(2026, 10, 9, 8, 0), Repeat::Weekdays, &IR), at(2026, 10, 10, 8, 0));
        assert_eq!(align(at(2026, 10, 9, 8, 0), Repeat::Daily, &IR), at(2026, 10, 9, 8, 0));
        assert!(is_workday(days_from_civil(2026, 10, 3)) && !is_workday(days_from_civil(2026, 10, 8)));
    }

    #[test]
    fn a_due_reminder_fires_once_and_repeats_move_on() {
        let t = at(2026, 10, 3, 9, 0);
        let mut data = data_with(vec![reminder(t, Repeat::None), Reminder { id: "r2".into(), ..reminder(t, Repeat::Daily) }], vec![]);
        let mut rt = Runtime::default();
        let before = tick(&mut data, &mut rt, t - MINUTE, &ctx(FocusPrefs::default()));
        assert!(before.fires.is_empty());
        assert_eq!(before.next, Some(t), "sleeps until the reminder");

        let out = tick(&mut data, &mut rt, t + 500, &ctx(FocusPrefs::default()));
        assert_eq!(out.fires.len(), 2);
        assert_eq!(out.fires[0], Fire::Reminder { id: "r1".into(), title: "Call".into() });
        assert_eq!(data.reminders[0].last_fired_at, Some(t + 500));
        assert_eq!(data.reminders[0].at, t, "one-off keeps its time and is done");
        assert_eq!(reminder_due(&data.reminders[0]), None);
        assert_eq!(data.reminders[1].at, at(2026, 10, 4, 9, 0));
        assert_eq!(out.next, Some(at(2026, 10, 4, 9, 0)));

        let again = tick(&mut data, &mut rt, t + 2 * MINUTE, &ctx(FocusPrefs::default()));
        assert!(again.fires.is_empty(), "nothing fires twice");
    }

    #[test]
    fn fires_before_the_island_listens_are_held_until_drained_once() {
        let t = at(2026, 10, 3, 9, 0);
        let mut data = data_with(vec![reminder(t, Repeat::None), Reminder { id: "r2".into(), ..reminder(t + HOUR, Repeat::None) }], vec![]);
        let mut rt = Runtime::default();
        // Launch catch-up: the first pass fires r1 before any listener exists.
        let out = tick(&mut data, &mut rt, t + 5 * MINUTE, &ctx(FocusPrefs::default()));
        assert_eq!(out.fires.len(), 1);
        assert_eq!(rt.take_held(), vec![Fire::Reminder { id: "r1".into(), title: "Call".into() }]);
        assert!(rt.take_held().is_empty(), "taking clears them");
        // Once drained, the island listens live: later fires aren't kept.
        let later = tick(&mut data, &mut rt, t + HOUR, &ctx(FocusPrefs::default()));
        assert_eq!(later.fires.len(), 1);
        assert!(rt.take_held().is_empty());

        // Bounded: only the most recent HELD_MAX survive.
        let mut rt = Runtime::default();
        let many: Vec<Fire> = (0..HELD_MAX + 7).map(|i| Fire::Reminder { id: format!("r{i}"), title: "x".into() }).collect();
        rt.hold(&many);
        let held = rt.take_held();
        assert_eq!(held.len(), HELD_MAX);
        assert_eq!(held[0], Fire::Reminder { id: "r7".into(), title: "x".into() }, "oldest dropped first");
    }

    #[test]
    fn snooze_fires_again_later() {
        let t = at(2026, 10, 3, 9, 0);
        let mut data = data_with(vec![reminder(t, Repeat::None)], vec![]);
        let mut rt = Runtime::default();
        tick(&mut data, &mut rt, t, &ctx(FocusPrefs::default()));
        data.reminders[0].snoozed_until = Some(t + 10 * MINUTE);
        assert_eq!(reminder_due(&data.reminders[0]), Some(t + 10 * MINUTE));
        assert!(tick(&mut data, &mut rt, t + 9 * MINUTE, &ctx(FocusPrefs::default())).fires.is_empty());
        let out = tick(&mut data, &mut rt, t + 10 * MINUTE, &ctx(FocusPrefs::default()));
        assert_eq!(out.fires.len(), 1);
        assert_eq!(data.reminders[0].snoozed_until, None);
        assert_eq!(reminder_due(&data.reminders[0]), None);
        // A disabled reminder never fires, snooze or not.
        data.reminders[0].enabled = false;
        data.reminders[0].snoozed_until = Some(t + 20 * MINUTE);
        assert!(tick(&mut data, &mut rt, t + 30 * MINUTE, &ctx(FocusPrefs::default())).fires.is_empty());
    }

    #[test]
    fn missed_reminders_fire_within_twelve_hours_else_are_skipped() {
        let t = at(2026, 10, 3, 9, 0);
        let mut data = data_with(
            vec![
                Reminder { id: "recent".into(), ..reminder(t, Repeat::None) },
                Reminder { id: "old".into(), ..reminder(t - 13 * HOUR, Repeat::None) },
                Reminder { id: "old-daily".into(), ..reminder(t - 3 * DAY, Repeat::Daily) },
            ],
            vec![],
        );
        // The app starts 11 hours after the recent one.
        let now = t + 11 * HOUR;
        let out = tick(&mut data, &mut Runtime::default(), now, &ctx(FocusPrefs::default()));
        let fired: Vec<&str> = out.fires.iter().map(|f| match f { Fire::Reminder { id, .. } => id.as_str(), _ => "" }).collect();
        // The daily one's 09:00 today is 11 hours old: it fires once, not three times.
        assert_eq!(fired, vec!["recent", "old-daily"]);
        let old = &data.reminders[1];
        assert_eq!(old.last_fired_at, Some(old.at), "skipped: marked handled at its own time");
        assert_eq!(reminder_due(old), None);
        let daily = &data.reminders[2];
        assert_eq!(daily.at, at(2026, 10, 4, 9, 0), "next occurrence after now");
        assert_eq!(reminder_due(daily), Some(daily.at));

        // Closed for two days: the last 09:00 is 23 hours old, nothing fires.
        let mut data = data_with(vec![reminder(t - 3 * DAY, Repeat::Daily)], vec![]);
        let out = tick(&mut data, &mut Runtime::default(), t + 23 * HOUR, &ctx(FocusPrefs::default()));
        assert!(out.fires.is_empty());
        assert_eq!(data.reminders[0].at, at(2026, 10, 4, 9, 0), "today's 09:00 is still ahead");
    }

    #[test]
    fn focus_cycle_waits_for_the_user_between_phases() {
        let prefs = FocusPrefs { rounds_before_long_break: 2, ..FocusPrefs::default() };
        let mut data = PlannerData::default();
        let mut rt = Runtime::default();
        let t0 = at(2026, 10, 3, 10, 0);
        let f = &mut data.focus;
        focus_start(f, None, None, t0, &prefs).unwrap();
        assert_eq!((f.phase, f.round, f.ends_at), (Phase::Focus, 1, Some(t0 + 25 * MINUTE)));

        let out = tick(&mut data, &mut rt, t0 + 25 * MINUTE, &ctx(prefs));
        assert_eq!(out.fires, vec![Fire::Focus { phase: "focusDone", round: 1 }]);
        let f = &mut data.focus;
        assert_eq!((f.phase, f.next, f.round, f.rounds_done_today), (Phase::Idle, Phase::Break, 1, 1));
        assert_eq!(f.ends_at, None, "the break waits for the user");

        focus_start(f, None, None, t0 + 26 * MINUTE, &prefs).unwrap();
        assert_eq!((f.phase, f.duration_ms), (Phase::Break, Some(5 * MINUTE)));
        let out = tick(&mut data, &mut rt, t0 + 31 * MINUTE, &ctx(prefs));
        assert_eq!(out.fires, vec![Fire::Focus { phase: "breakDone", round: 1 }]);
        assert_eq!((data.focus.next, data.focus.round), (Phase::Focus, 2));

        let f = &mut data.focus;
        focus_start(f, None, Some(1.0), t0 + 32 * MINUTE, &prefs).unwrap();
        tick(&mut data, &mut rt, t0 + 33 * MINUTE, &ctx(prefs));
        assert_eq!(data.focus.next, Phase::LongBreak, "every 2nd round with these prefs");
        assert_eq!(data.focus.rounds_done_today, 2);
        let f = &mut data.focus;
        focus_start(f, None, None, t0 + 34 * MINUTE, &prefs).unwrap();
        assert_eq!(f.duration_ms, Some(15 * MINUTE));
        let out = tick(&mut data, &mut rt, t0 + 49 * MINUTE, &ctx(prefs));
        assert_eq!(out.fires, vec![Fire::Focus { phase: "longBreakDone", round: 2 }]);
        assert_eq!((data.focus.round, data.focus.next), (1, Phase::Focus));
    }

    #[test]
    fn pause_resume_skip_and_stop() {
        let prefs = FocusPrefs::default();
        let mut f = FocusState::default();
        let t0 = 1_000_000;
        focus_start(&mut f, None, Some(10.0), t0, &prefs).unwrap();
        focus_pause(&mut f, t0 + 4 * MINUTE);
        assert_eq!((f.phase, f.paused_from, f.remaining_ms, f.ends_at), (Phase::Paused, Some(Phase::Focus), Some(6 * MINUTE), None));
        focus_pause(&mut f, t0 + 5 * MINUTE);
        assert_eq!(f.remaining_ms, Some(6 * MINUTE), "pausing twice changes nothing");
        focus_resume(&mut f, t0 + 20 * MINUTE, &prefs).unwrap();
        assert_eq!((f.phase, f.ends_at), (Phase::Focus, Some(t0 + 26 * MINUTE)));

        focus_skip(&mut f, &prefs);
        assert_eq!((f.phase, f.next, f.rounds_done_today), (Phase::Idle, Phase::Break, 1), "a skipped focus counts as done");
        focus_skip(&mut f, &prefs);
        assert_eq!(f.rounds_done_today, 1, "skipping while idle does nothing");
        // Skipping the break: straight into the next focus round.
        focus_start(&mut f, Some(Phase::Focus), None, t0, &prefs).unwrap();
        assert_eq!(f.round, 2);
        focus_stop(&mut f);
        assert_eq!((f.phase, f.round, f.next, f.ends_at), (Phase::Idle, 1, Phase::Focus, None));

        assert_eq!(focus_start(&mut f, Some(Phase::Paused), None, t0, &prefs).unwrap_err(), "E_PLANNER_INVALID|phase");
        assert_eq!(focus_start(&mut f, None, Some(0.0), t0, &prefs).unwrap_err(), "E_PLANNER_INVALID|minutes");
        assert_eq!(focus_start(&mut f, None, Some(f64::NAN), t0, &prefs).unwrap_err(), "E_PLANNER_INVALID|minutes");
        // Resume from idle starts what's next.
        focus_resume(&mut f, t0, &prefs).unwrap();
        assert_eq!(f.phase, Phase::Focus);
    }

    #[test]
    fn a_new_day_resets_the_count_and_old_phase_ends_stay_quiet() {
        let prefs = FocusPrefs::default();
        let mut data = PlannerData::default();
        let t0 = at(2026, 10, 3, 22, 0);
        roll_day(&mut data.focus, &Now::new(t0, &IR));
        data.focus.rounds_done_today = 3;
        focus_start(&mut data.focus, None, None, t0, &prefs).unwrap();
        // The app was closed overnight: the phase ended 13 h ago.
        let next_day = t0 + 25 * MINUTE + 13 * HOUR;
        let out = tick(&mut data, &mut Runtime::default(), next_day, &ctx(prefs));
        assert!(out.fires.is_empty(), "too old to announce");
        assert_eq!(data.focus.phase, Phase::Idle);
        assert_eq!(data.focus.day_key, "2026-10-04");
        assert_eq!(data.focus.rounds_done_today, 1, "yesterday's 3 are gone; the late round counts today");
    }

    fn water(nudge: Option<Nudge>, days: HabitDays) -> Habit {
        Habit { id: "h1".into(), title: "Water".into(), icon: HabitIcon::Water, days, nudge, log: vec![], created_at: 0 }
    }

    fn hourly() -> Option<Nudge> {
        Some(Nudge { every_minutes: 60, from: "09:00".into(), to: "12:00".into() })
    }

    #[test]
    fn habit_nudge_slots_follow_the_window_and_days() {
        let h = water(hourly(), HabitDays::Daily);
        let sat = days_from_civil(2026, 10, 3);
        assert_eq!(nudge_slots(&h, sat, &IR), vec![at(2026, 10, 3, 10, 0), at(2026, 10, 3, 11, 0), at(2026, 10, 3, 12, 0)]);
        assert_eq!(next_nudge(&h, &IR, at(2026, 10, 3, 10, 0)), Some(at(2026, 10, 3, 11, 0)));
        assert_eq!(next_nudge(&h, &IR, at(2026, 10, 3, 12, 30)), Some(at(2026, 10, 4, 10, 0)), "tomorrow's first");
        let work = water(hourly(), HabitDays::Weekdays);
        assert_eq!(next_nudge(&work, &IR, at(2026, 10, 7, 13, 0)), Some(at(2026, 10, 10, 10, 0)), "Wed → Sat");
        assert_eq!(next_nudge(&water(None, HabitDays::Daily), &IR, 0), None);
        let off = Some(Nudge { every_minutes: 0, ..hourly().unwrap() });
        assert_eq!(next_nudge(&water(off, HabitDays::Daily), &IR, 0), None);
        assert_eq!(parse_hhmm("09:30"), Some(570));
        for bad in ["9:30", "24:00", "12:60", "ab:cd", "", "12:3"] {
            assert_eq!(parse_hhmm(bad), None, "{bad}");
        }
    }

    #[test]
    fn habit_nudges_respect_settings_idle_pause_and_done() {
        let t = at(2026, 10, 3, 10, 0);
        let mut data = data_with(vec![], vec![water(hourly(), HabitDays::Daily)]);
        let mut rt = Runtime::default();
        // First pass starts the cursor: nothing is caught up.
        assert!(tick(&mut data, &mut rt, t + 30 * MINUTE, &ctx(FocusPrefs::default())).fires.is_empty());
        let out = tick(&mut data, &mut rt, at(2026, 10, 3, 11, 0) + 1000, &ctx(FocusPrefs::default()));
        assert_eq!(out.fires, vec![Fire::Habit { id: "h1".into(), title: "Water".into(), icon: HabitIcon::Water }]);

        let mut away = ctx(FocusPrefs::default());
        away.idle_ms = Some(IDLE_LIMIT_MS + 1);
        assert!(tick(&mut data, &mut rt, at(2026, 10, 3, 12, 0), &away).fires.is_empty(), "user away");

        let mut rt = Runtime { nudge_cursor: Some(t + 30 * MINUTE), ..Runtime::default() };
        let mut paused = ctx(FocusPrefs::default());
        paused.paused = true;
        assert!(tick(&mut data, &mut rt, at(2026, 10, 3, 11, 0), &paused).fires.is_empty(), "paused");

        let mut rt = Runtime { nudge_cursor: Some(t + 30 * MINUTE), ..Runtime::default() };
        let off = FocusPrefs { habit_nudges: false, ..FocusPrefs::default() };
        let out = tick(&mut data, &mut rt, at(2026, 10, 3, 11, 0), &ctx(off));
        assert!(out.fires.is_empty(), "setting off");
        assert_eq!(out.next, None, "and nothing to wake for");

        let mut rt = Runtime { nudge_cursor: Some(t + 30 * MINUTE), ..Runtime::default() };
        data.habits[0].log = vec!["2026-10-03".into()];
        assert!(tick(&mut data, &mut rt, at(2026, 10, 3, 11, 0), &ctx(FocusPrefs::default())).fires.is_empty(), "done today");

        // Asleep through a slot: a nudge more than 10 minutes late is dropped.
        data.habits[0].log.clear();
        let mut rt = Runtime { nudge_cursor: Some(t + 30 * MINUTE), ..Runtime::default() };
        assert!(tick(&mut data, &mut rt, at(2026, 10, 3, 11, 20), &ctx(FocusPrefs::default())).fires.is_empty());
    }
}
