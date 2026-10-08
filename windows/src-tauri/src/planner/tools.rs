// The planner's built-in chat tools: offered to the model next to the user's
// MCP tools (server «رودیپ», server_id "builtin", qualified `roadeep__<tool>`),
// always in `auto` mode — they only touch local, reversible planner data —
// and run here instead of over MCP (mcpc::call_tool branches on the id).
//
// Descriptions are English, one line, and avoid every phrase the Roadeep
// server reads as a "save to memory" request (src/core/memory-words.ts);
// a test below checks. Results are compact JSON text. A call the model got
// wrong (bad field, no match, ambiguous title) is a tool error the model can
// read and fix; only a storage failure is a coded error.

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use super::jalali::{day_key_of, days_of_jalali, days_of_key, jalali_of_key, weekday_fa};
use super::schedule::{self, local_iso, local_to_utc, FocusPrefs, Now, SystemZone, DAY, HOUR, MINUTE};
use super::store::{TaskPatch, MAX_LOG_DAYS};
use super::{announce, now_ms, plog, FocusState, Habit, HabitDays, Phase, Planner, PlannerData, Reminder, Repeat, Task};
use crate::errors;
use crate::mcpc::{ToolMode, ToolOutcome, ToolSpec};

pub const SERVER_ID: &str = "builtin";
pub const SERVER_NAME: &str = "رودیپ";
/// The qualified-name prefix; mcpc::tools::offered keeps it from MCP servers.
pub const SLUG: &str = "roadeep";
/// Items per list result: enough to answer, small enough for the thread.
const MAX_LISTED: usize = 50;
const MAX_CANDIDATES: usize = 8;
/// `inMinutes` of add_reminder: up to a week ahead.
const MAX_IN_MINUTES: f64 = 7.0 * 24.0 * 60.0;

/// (name, description, read-only).
const TOOLS: [(&str, &str, bool); 10] = [
    ("add_task", "Add a task to the user's local to-do list, optionally due on a day.", false),
    ("complete_task", "Mark one of the user's open tasks done, by id or by words from its title.", false),
    ("list_tasks", "List the user's tasks: today (due today or overdue), open, done or all.", true),
    ("add_note", "Add a short text note to the user's local notes.", false),
    ("add_reminder", "Set a timed reminder: at = HH:MM (with optional day) or local YYYY-MM-DDTHH:MM, or inMinutes from now.", false),
    ("list_reminders", "List the user's upcoming reminders, soonest first.", true),
    ("start_focus", "Start a focus (Pomodoro) timer; minutes defaults to the user's setting.", false),
    ("stop_focus", "Stop the focus timer.", false),
    ("log_habit", "Mark one of the user's habits done for today, by words from its title.", false),
    ("list_habits", "List the user's habits with today's status and streaks.", true),
];

fn schema(tool: &str) -> Value {
    let text = |desc: &str| json!({ "type": "string", "description": desc });
    match tool {
        "add_task" => json!({ "type": "object", "properties": {
            "title": { "type": "string" },
            "due": text("YYYY-MM-DD, Jalali YYYY/MM/DD, today or tomorrow"),
            "note": { "type": "string" } }, "required": ["title"] }),
        "complete_task" => json!({ "type": "object", "properties": {
            "id": { "type": "string" },
            "title": text("words from the task's title") } }),
        "list_tasks" => json!({ "type": "object", "properties": {
            "scope": { "enum": ["today", "open", "done", "all"] } } }),
        "add_note" => json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] }),
        "add_reminder" => json!({ "type": "object", "properties": {
            "title": { "type": "string" },
            "at": text("HH:MM or YYYY-MM-DDTHH:MM, local time"),
            "day": text("with at = HH:MM: today, tomorrow, YYYY-MM-DD or Jalali YYYY/MM/DD"),
            "inMinutes": { "type": "integer" },
            "repeat": { "enum": ["none", "daily", "weekdays", "weekly"], "description": "weekdays = Saturday to Wednesday" } },
            "required": ["title"] }),
        "start_focus" => json!({ "type": "object", "properties": { "minutes": { "type": "integer" } } }),
        "log_habit" => json!({ "type": "object", "properties": { "title": text("words from the habit's title") }, "required": ["title"] }),
        _ => json!({ "type": "object", "properties": {} }),
    }
}

/// The built-in tools as the chat offers them.
pub fn specs() -> Vec<ToolSpec> {
    TOOLS
        .iter()
        .map(|(name, description, read_only)| ToolSpec {
            server_id: SERVER_ID.into(),
            server_name: SERVER_NAME.into(),
            tool: (*name).into(),
            qualified: format!("{SLUG}__{name}"),
            description: (*description).into(),
            input_schema: schema(name),
            mode: ToolMode::Auto,
            read_only: *read_only,
            destructive: false,
        })
        .collect()
}

/// `auto` for a built-in tool; the coded "unknown tool" for anything else.
pub fn mode(tool: &str) -> Result<ToolMode, String> {
    if TOOLS.iter().any(|(name, ..)| *name == tool) {
        Ok(ToolMode::Auto)
    } else {
        Err(errors::coded(errors::MCPC_UNKNOWN_TOOL, &[&crate::mcpc::tools::one_line(tool, 80)]))
    }
}

/// Runs a built-in tool for the chat: the change, its events, a log line
/// (tool name and outcome only).
pub fn call(app: &AppHandle, tool: &str, arguments: &Value) -> Result<ToolOutcome, String> {
    mode(tool)?;
    let planner = app.state::<Planner>();
    let prefs = super::current_prefs(app);
    let zone = SystemZone;
    let now = Now::new(now_ms(), &zone);
    let (outcome, kinds) = planner.mutate(|data| Ok(run(data, tool, arguments, &now, &prefs)))?;
    announce(app, &planner, &kinds);
    plog(format!("planner: chat tool {tool} {}", if outcome.is_error { "refused" } else { "ok" }));
    Ok(outcome)
}

// ── Results ───────────────────────────────────────────────────────────────────

fn ok(value: Value) -> ToolOutcome {
    ToolOutcome { is_error: false, text: value.to_string(), omitted: Vec::new() }
}

fn fail(value: Value) -> ToolOutcome {
    ToolOutcome { is_error: true, text: value.to_string(), omitted: Vec::new() }
}

/// A coded planner error, in words the model can act on.
fn refused(coded: &str) -> ToolOutcome {
    let mut parts = coded.lines().next().unwrap_or("").split('|');
    let code = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("");
    fail(match code {
        c if c == errors::PLANNER_INVALID => json!({ "error": "invalid argument", "field": arg }),
        c if c == errors::PLANNER_LIMIT => json!({ "error": "the list is full", "limit": arg }),
        c if c == errors::PLANNER_NOT_FOUND => json!({ "error": "not found" }),
        _ => json!({ "error": code }),
    })
}

fn jalali(day_key: &str) -> Value {
    jalali_of_key(day_key).map_or(Value::Null, Value::String)
}

fn task_view(t: &Task, today: &str) -> Value {
    let mut v = json!({ "id": t.id, "title": t.title, "done": t.done, "due": t.due });
    if let Some(due) = &t.due {
        v["dueJalali"] = jalali(due);
        if !t.done && due.as_str() < today {
            v["overdue"] = Value::Bool(true);
        }
    }
    v
}

fn moment_view(ms: i64, now: &Now<'_>) -> (String, String) {
    let iso = local_iso(now.zone, ms);
    let (date, time) = iso.split_once('T').unwrap_or((&iso, ""));
    let shown = jalali_of_key(date).map_or_else(|| iso.clone(), |j| format!("{j} {time}"));
    (iso.clone(), shown)
}

fn reminder_view(r: &Reminder, now: &Now<'_>) -> Value {
    let next = schedule::reminder_due(r).unwrap_or(r.at);
    let (at, at_jalali) = moment_view(next, now);
    json!({ "id": r.id, "title": r.title, "at": at, "atJalali": at_jalali, "repeat": r.repeat, "enabled": r.enabled })
}

fn focus_view(f: &FocusState, now: &Now<'_>) -> Value {
    let left = match f.phase {
        Phase::Paused => f.remaining_ms,
        _ => f.ends_at.map(|end| (end - now.ms).max(0)),
    };
    json!({
        "phase": f.phase, "round": f.round, "roundsDoneToday": f.rounds_done_today,
        "endsAt": f.ends_at.map(|e| local_iso(now.zone, e)),
        "minutesLeft": left.map(|ms| (ms + MINUTE - 1) / MINUTE),
    })
}

/// Consecutive done days up to today (today not done yet doesn't break it);
/// a weekdays habit skips Thursday and Friday.
pub fn streak(h: &Habit, today_days: i64) -> u32 {
    let applies = |d: i64| h.days == HabitDays::Daily || schedule::is_workday(d);
    let done = |d: i64| schedule::done_on(h, &day_key_of(d));
    let mut day = if done(today_days) { today_days } else { today_days - 1 };
    let mut n = 0;
    for _ in 0..MAX_LOG_DAYS + 7 {
        if !applies(day) {
            day -= 1;
        } else if done(day) {
            n += 1;
            day -= 1;
        } else {
            break;
        }
    }
    n
}

fn habit_view(h: &Habit, now: &Now<'_>) -> Value {
    let mut v = json!({
        "id": h.id, "title": h.title, "icon": h.icon, "days": h.days,
        "doneToday": schedule::done_on(h, &now.today), "streak": streak(h, now.today_days),
    });
    if let Some(n) = &h.nudge {
        v["nudge"] = json!({ "everyMinutes": n.every_minutes, "from": n.from, "to": n.to });
    }
    v
}

// ── Arguments ─────────────────────────────────────────────────────────────────

/// A trimmed, non-empty string argument; Err when it is there but not a string.
fn text_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, ToolOutcome> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim()).filter(|s| !s.is_empty())),
        Some(_) => Err(fail(json!({ "error": "invalid argument", "field": key, "expected": "string" }))),
    }
}

/// A number argument (a numeric string is accepted too).
fn number_arg(args: &Map<String, Value>, key: &str) -> Result<Option<f64>, ToolOutcome> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_f64()),
        Some(Value::String(s)) if s.trim().parse::<f64>().is_ok() => Ok(s.trim().parse().ok()),
        Some(_) => Err(fail(json!({ "error": "invalid argument", "field": key, "expected": "number" }))),
    }
}

fn day_arg(day: &str, now: &Now<'_>) -> Option<i64> {
    match day.to_ascii_lowercase().as_str() {
        "today" => Some(now.today_days),
        "tomorrow" => Some(now.today_days + 1),
        // A Persian conversation may well name the day in Jalali.
        key => days_of_key(key).or_else(|| days_of_jalali(key)),
    }
}

/// "HH:MM" or "HH:MM:SS" (fractions ignored) → ms into the day.
fn time_of_day(text: &str) -> Option<i64> {
    let (h, rest) = text.split_once(':')?;
    let (m, s) = rest.split_once(':').unwrap_or((rest, "0"));
    let s = s.split('.').next().unwrap_or("0");
    let digits = |x: &str, max: i64| -> Option<i64> {
        (!x.is_empty() && x.len() <= 2 && x.bytes().all(|c| c.is_ascii_digit())).then(|| x.parse::<i64>().ok()).flatten().filter(|v| *v < max)
    };
    Some(digits(h, 24)? * HOUR + digits(m, 60)? * MINUTE + digits(s, 60)? * 1000)
}

/// `at` as the model writes it: "HH:MM" on `day` (default: today, or
/// tomorrow when that time has passed), or a full ISO date-time — local,
/// unless it carries "Z" or an offset.
fn parse_at(at: &str, day: Option<&str>, now: &Now<'_>) -> Option<i64> {
    if !at.is_ascii() {
        return None;
    }
    if at.len() >= 16 && matches!(at.as_bytes()[10], b'T' | b't' | b' ') {
        let days = days_of_key(&at[..10])?;
        let rest = &at[11..];
        let (time, offset) = if let Some(t) = rest.strip_suffix(['Z', 'z']) {
            (t, Some(0))
        } else if let Some(i) = rest.rfind(['+', '-']) {
            let sign = if rest.as_bytes()[i] == b'-' { -1 } else { 1 };
            (&rest[..i], Some(sign * time_of_day(&rest[i + 1..])?))
        } else {
            (rest, None)
        };
        let tod = time_of_day(time)?;
        return Some(match offset {
            Some(off) => days * DAY + tod - off,
            None => local_to_utc(now.zone, days, tod),
        });
    }
    let tod = time_of_day(at)?;
    match day {
        Some(day) => Some(local_to_utc(now.zone, day_arg(day, now)?, tod)),
        None => {
            let today = local_to_utc(now.zone, now.today_days, tod);
            Some(if today > now.ms { today } else { local_to_utc(now.zone, now.today_days + 1, tod) })
        }
    }
}

/// For fuzzy title matching: Arabic letter forms → Persian, Persian and
/// Arabic digits → ASCII, no half-spaces or diacritics, lower case, single spaces.
fn normalize(text: &str) -> String {
    let mapped: String = text
        .chars()
        .filter_map(|c| match c {
            'ي' | 'ى' => Some('ی'),
            'ك' => Some('ک'),
            '۰'..='۹' => char::from_digit(c as u32 - '۰' as u32, 10),
            '٠'..='٩' => char::from_digit(c as u32 - '٠' as u32, 10),
            '\u{200C}' | '\u{064B}'..='\u{065F}' | '\u{0670}' => None,
            c => Some(c),
        })
        .flat_map(char::to_lowercase)
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Indexes of the titles a query names: exact matches, else titles that
/// contain it, else titles that contain all of its words.
fn matching<'a>(query: &str, titles: impl Iterator<Item = (usize, &'a str)> + Clone) -> Vec<usize> {
    let q = normalize(query);
    if q.is_empty() {
        return Vec::new();
    }
    let normed: Vec<(usize, String)> = titles.map(|(i, t)| (i, normalize(t))).collect();
    let exact: Vec<usize> = normed.iter().filter(|(_, t)| *t == q).map(|(i, _)| *i).collect();
    if !exact.is_empty() {
        return exact;
    }
    let within: Vec<usize> = normed.iter().filter(|(_, t)| t.contains(&q)).map(|(i, _)| *i).collect();
    if !within.is_empty() {
        return within;
    }
    let words: Vec<&str> = q.split(' ').collect();
    normed.iter().filter(|(_, t)| words.iter().all(|w| t.contains(w))).map(|(i, _)| *i).collect()
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// One built-in tool call against the data. Only a successful call changes it.
pub fn run(data: &mut PlannerData, tool: &str, arguments: &Value, now: &Now<'_>, prefs: &FocusPrefs) -> ToolOutcome {
    let empty = Map::new();
    let args = match arguments {
        Value::Object(map) => map,
        Value::Null => &empty,
        _ => return fail(json!({ "error": "arguments must be an object" })),
    };
    let result = match tool {
        "add_task" => add_task(data, args, now),
        "complete_task" => complete_task(data, args, now),
        "list_tasks" => list_tasks(data, args, now),
        "add_note" => add_note(data, args, now),
        "add_reminder" => add_reminder(data, args, now),
        "list_reminders" => Ok(list_reminders(data, now)),
        "start_focus" => start_focus(data, args, now, prefs),
        "stop_focus" => Ok(stop_focus(data, now)),
        "log_habit" => log_habit(data, args, now),
        "list_habits" => Ok(list_habits(data, now)),
        _ => Err(fail(json!({ "error": "unknown tool" }))),
    };
    result.unwrap_or_else(|outcome| outcome)
}

type Outcome = Result<ToolOutcome, ToolOutcome>;

fn add_task(data: &mut PlannerData, args: &Map<String, Value>, now: &Now<'_>) -> Outcome {
    let title = text_arg(args, "title")?.unwrap_or("");
    let due = match text_arg(args, "due")? {
        None => None,
        Some(d) => Some(day_arg(d, now).map(day_key_of).ok_or_else(|| refused(&super::invalid("due")))?),
    };
    let note = text_arg(args, "note")?;
    let task = data.add_task(title, due.as_deref(), note, now.ms).map_err(|e| refused(&e))?;
    Ok(ok(json!({ "ok": true, "task": task_view(&task, &now.today) })))
}

fn complete_task(data: &mut PlannerData, args: &Map<String, Value>, now: &Now<'_>) -> Outcome {
    let index = match (text_arg(args, "id")?, text_arg(args, "title")?) {
        (Some(id), _) => data.tasks.iter().position(|t| t.id == id).ok_or_else(|| refused(&super::not_found()))?,
        (None, Some(title)) => {
            let open = data.tasks.iter().enumerate().filter(|(_, t)| !t.done).map(|(i, t)| (i, t.title.as_str()));
            let found = matching(title, open);
            match found.as_slice() {
                [one] => *one,
                [] => return Err(fail(json!({ "error": "no open task matches that title" }))),
                many => {
                    let candidates: Vec<Value> = many.iter().take(MAX_CANDIDATES).map(|i| task_view(&data.tasks[*i], &now.today)).collect();
                    return Err(fail(json!({ "error": "several open tasks match; call again with one id", "candidates": candidates })));
                }
            }
        }
        (None, None) => return Err(refused(&super::invalid("title"))),
    };
    if data.tasks[index].done {
        return Ok(ok(json!({ "ok": true, "alreadyDone": true, "task": task_view(&data.tasks[index], &now.today) })));
    }
    let id = data.tasks[index].id.clone();
    let patch = TaskPatch { title: None, note: None, due: None, done: Some(true) };
    let task = data.update_task(&id, patch, now.ms).map_err(|e| refused(&e))?;
    Ok(ok(json!({ "ok": true, "task": task_view(&task, &now.today) })))
}

fn listed(items: Vec<Value>) -> (Vec<Value>, usize) {
    let more = items.len().saturating_sub(MAX_LISTED);
    (items.into_iter().take(MAX_LISTED).collect(), more)
}

fn list_tasks(data: &PlannerData, args: &Map<String, Value>, now: &Now<'_>) -> Outcome {
    let scope = text_arg(args, "scope")?.unwrap_or("open").to_ascii_lowercase();
    let today = now.today.as_str();
    let by_due = |a: &&Task, b: &&Task| (a.due.is_none(), &a.due, a.created_at).cmp(&(b.due.is_none(), &b.due, b.created_at));
    let mut open: Vec<&Task> = data.tasks.iter().filter(|t| !t.done).collect();
    open.sort_by(by_due);
    let mut done: Vec<&Task> = data.tasks.iter().filter(|t| t.done).collect();
    done.sort_by_key(|t| std::cmp::Reverse(t.done_at));
    let chosen: Vec<&Task> = match scope.as_str() {
        "today" => open.into_iter().filter(|t| t.due.as_deref().is_some_and(|d| d <= today)).collect(),
        "open" => open,
        "done" => done,
        "all" => open.into_iter().chain(done).collect(),
        _ => return Err(refused(&super::invalid("scope"))),
    };
    let count = chosen.len();
    let (tasks, more) = listed(chosen.iter().map(|t| task_view(t, today)).collect());
    Ok(ok(json!({ "scope": scope, "today": today, "todayJalali": jalali(today), "weekday": weekday_fa(now.today_days), "count": count, "tasks": tasks, "more": more })))
}

fn add_note(data: &mut PlannerData, args: &Map<String, Value>, now: &Now<'_>) -> Outcome {
    let text = text_arg(args, "text")?.unwrap_or("");
    let note = data.add_note(text, now.ms).map_err(|e| refused(&e))?;
    Ok(ok(json!({ "ok": true, "note": { "id": note.id, "chars": note.text.chars().count() } })))
}

fn add_reminder(data: &mut PlannerData, args: &Map<String, Value>, now: &Now<'_>) -> Outcome {
    let title = text_arg(args, "title")?.unwrap_or("");
    let repeat = match text_arg(args, "repeat")?.map(str::to_ascii_lowercase).as_deref() {
        None | Some("none") => Repeat::None,
        Some("daily") => Repeat::Daily,
        Some("weekdays") => Repeat::Weekdays,
        Some("weekly") => Repeat::Weekly,
        Some(_) => return Err(refused(&super::invalid("repeat"))),
    };
    let at = match (number_arg(args, "inMinutes")?, text_arg(args, "at")?) {
        (Some(m), _) if (1.0..=MAX_IN_MINUTES).contains(&m) => now.ms + (m.round() as i64) * MINUTE,
        (Some(_), _) => return Err(refused(&super::invalid("inMinutes"))),
        (None, Some(at)) => parse_at(at, text_arg(args, "day")?, now).ok_or_else(|| refused(&super::invalid("at")))?,
        (None, None) => return Err(refused(&super::invalid("at"))),
    };
    let reminder = data.add_reminder(title, at, repeat, now).map_err(|e| refused(&e))?;
    Ok(ok(json!({ "ok": true, "reminder": reminder_view(&reminder, now) })))
}

fn list_reminders(data: &PlannerData, now: &Now<'_>) -> ToolOutcome {
    let mut upcoming: Vec<(i64, &Reminder)> = data.reminders.iter().filter_map(|r| schedule::reminder_due(r).map(|due| (due, r))).collect();
    upcoming.sort_by_key(|(due, _)| *due);
    let count = upcoming.len();
    let (reminders, more) = listed(upcoming.iter().map(|(_, r)| reminder_view(r, now)).collect());
    let (now_iso, now_jalali) = moment_view(now.ms, now);
    ok(json!({ "now": now_iso, "nowJalali": now_jalali, "weekday": weekday_fa(now.today_days), "count": count, "reminders": reminders, "more": more }))
}

fn start_focus(data: &mut PlannerData, args: &Map<String, Value>, now: &Now<'_>, prefs: &FocusPrefs) -> Outcome {
    let minutes = number_arg(args, "minutes")?;
    let f = &mut data.focus;
    schedule::roll_day(f, now);
    if f.phase == Phase::Focus {
        return Ok(ok(json!({ "ok": true, "alreadyRunning": true, "focus": focus_view(f, now) })));
    }
    if f.phase == Phase::Paused && f.paused_from == Some(Phase::Focus) && minutes.is_none() {
        schedule::focus_resume(f, now.ms, prefs).map_err(|e| refused(&e))?;
    } else {
        schedule::focus_start(f, Some(Phase::Focus), minutes, now.ms, prefs).map_err(|e| refused(&e))?;
    }
    Ok(ok(json!({ "ok": true, "focus": focus_view(f, now) })))
}

fn stop_focus(data: &mut PlannerData, now: &Now<'_>) -> ToolOutcome {
    schedule::roll_day(&mut data.focus, now);
    schedule::focus_stop(&mut data.focus);
    ok(json!({ "ok": true, "focus": focus_view(&data.focus, now) }))
}

fn log_habit(data: &mut PlannerData, args: &Map<String, Value>, now: &Now<'_>) -> Outcome {
    let title = text_arg(args, "title")?.ok_or_else(|| refused(&super::invalid("title")))?;
    let found = matching(title, data.habits.iter().enumerate().map(|(i, h)| (i, h.title.as_str())));
    let index = match found.as_slice() {
        [one] => *one,
        [] => return Err(fail(json!({ "error": "no habit matches that title", "habits": data.habits.iter().map(|h| h.title.clone()).take(20).collect::<Vec<_>>() }))),
        many => {
            let candidates: Vec<Value> = many.iter().take(MAX_CANDIDATES).map(|i| json!({ "id": data.habits[*i].id, "title": data.habits[*i].title })).collect();
            return Err(fail(json!({ "error": "several habits match; use more of the title", "candidates": candidates })));
        }
    };
    let id = data.habits[index].id.clone();
    let habit = data.check_habit(&id, &now.today, true, &now.today).map_err(|e| refused(&e))?;
    Ok(ok(json!({ "ok": true, "habit": habit_view(&habit, now) })))
}

fn list_habits(data: &PlannerData, now: &Now<'_>) -> ToolOutcome {
    let habits: Vec<Value> = data.habits.iter().map(|h| habit_view(h, now)).collect();
    ok(json!({ "today": now.today, "todayJalali": jalali(&now.today), "count": habits.len(), "habits": habits }))
}

#[cfg(test)]
mod tests {
    use super::super::jalali::days_from_civil;
    use super::super::schedule::FixedZone;
    use super::super::HabitIcon;
    use super::*;

    const IR: FixedZone = FixedZone(3 * HOUR + 30 * MINUTE);

    /// Saturday 2026-10-03 (1405/07/11), 10:00 in Tehran.
    fn now() -> Now<'static> {
        Now::new(local_to_utc(&IR, days_from_civil(2026, 10, 3), 10 * HOUR), &IR)
    }

    fn call(data: &mut PlannerData, tool: &str, args: Value) -> (bool, Value) {
        let out = run(data, tool, &args, &now(), &FocusPrefs::default());
        (out.is_error, serde_json::from_str(&out.text).expect("results are JSON"))
    }

    #[test]
    fn specs_are_small_valid_and_free_of_the_servers_trigger_words() {
        let specs = specs();
        assert_eq!(specs.len(), 10);
        for s in &specs {
            assert!(s.qualified.starts_with("roadeep__") && s.qualified.len() <= 64);
            assert_eq!((s.server_id.as_str(), s.mode), (SERVER_ID, ToolMode::Auto));
            assert!(s.description.chars().count() <= 120 && !s.description.contains('\n'), "{}", s.description);
            let all = format!("{} {}", s.description, s.input_schema).to_lowercase();
            for trigger in ["remember", "memoriz", "memoris", "save this", "store this", "note this", "keep this for later", "add this to memory", "حفظ", "حافظه", "ذخیره", "یادت باش", "فراموش"] {
                assert!(!all.contains(trigger), "{trigger} in {}", s.qualified);
            }
        }
        assert_eq!(mode("add_task"), Ok(ToolMode::Auto));
        assert_eq!(mode("rm_rf").unwrap_err(), "E_MCPC_UNKNOWN_TOOL|rm_rf");
        // The preamble the model gets stays small with all ten.
        let entries: Vec<crate::roadeep::chat::tools::ToolEntry> = specs
            .iter()
            .map(|s| crate::roadeep::chat::tools::ToolEntry { qualified: s.qualified.clone(), description: s.description.clone(), schema: s.input_schema.clone() })
            .collect();
        let preamble = crate::roadeep::chat::tools::preamble(&entries);
        assert_eq!(preamble.listed, 10);
        assert!(preamble.text.chars().count() < 3_000, "{}", preamble.text.chars().count());
    }

    #[test]
    fn add_list_and_complete_tasks() {
        let mut d = PlannerData::default();
        let (err, v) = call(&mut d, "add_task", json!({ "title": "Buy milk", "due": "tomorrow" }));
        assert!(!err);
        assert_eq!(v["task"]["due"], "2026-10-04");
        assert_eq!(v["task"]["dueJalali"], "1405/07/12");
        call(&mut d, "add_task", json!({ "title": "Pay rent", "due": "2026-10-01" }));
        call(&mut d, "add_task", json!({ "title": "Pay phone bill", "due": "today" }));
        call(&mut d, "add_task", json!({ "title": "Read" }));
        assert!(call(&mut d, "add_task", json!({ "title": "" })).0);
        assert_eq!(call(&mut d, "add_task", json!({ "title": "x", "due": "someday" })).1["field"], "due");
        assert_eq!(call(&mut d, "add_task", json!({ "title": "Jalali", "due": "1405/07/15" })).1["task"]["due"], "2026-10-07");
        d.tasks.pop();
        assert_eq!(call(&mut d, "add_task", json!({ "title": 5 })).1["field"], "title");

        let (_, today) = call(&mut d, "list_tasks", json!({ "scope": "today" }));
        let titles: Vec<&str> = today["tasks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect();
        assert_eq!(titles, vec!["Pay rent", "Pay phone bill"], "overdue first, then due today");
        assert_eq!(today["tasks"][0]["overdue"], true);
        assert_eq!(today["todayJalali"], "1405/07/11");
        assert_eq!(today["weekday"], "شنبه");
        assert_eq!(call(&mut d, "list_tasks", json!({})).1["count"], 4);

        // "pay" matches two open tasks: the model gets candidates.
        let (err, v) = call(&mut d, "complete_task", json!({ "title": "pay" }));
        assert!(err);
        assert_eq!(v["candidates"].as_array().unwrap().len(), 2);
        let (err, v) = call(&mut d, "complete_task", json!({ "title": "  PAY rent " }));
        assert!(!err, "{v}");
        assert_eq!(v["task"]["done"], true);
        assert!(call(&mut d, "complete_task", json!({ "title": "pay rent" })).0, "done tasks aren't matched again");
        let (err, v) = call(&mut d, "complete_task", json!({ "title": "pay" }));
        assert!(!err, "only one open match left: {v}");
        assert!(call(&mut d, "complete_task", json!({ "title": "groceries" })).0);
        let id = d.tasks[0].id.clone();
        assert_eq!(call(&mut d, "complete_task", json!({ "id": id })).1["task"]["done"], true);
        assert_eq!(call(&mut d, "list_tasks", json!({ "scope": "done" })).1["count"], 3);
        assert!(call(&mut d, "list_tasks", json!({ "scope": "later" })).0);
    }

    #[test]
    fn persian_titles_match_across_letter_forms_and_digits() {
        let mut d = PlannerData::default();
        call(&mut d, "add_task", json!({ "title": "خرید کتاب‌های درسی ۲" }));
        // Arabic yeh/kaf, no half-space, ASCII digit.
        let (err, v) = call(&mut d, "complete_task", json!({ "title": "كتابهاي درسي 2" }));
        assert!(!err, "{v}");
    }

    #[test]
    fn reminders_from_the_model() {
        let mut d = PlannerData::default();
        let (err, v) = call(&mut d, "add_reminder", json!({ "title": "Stand up", "inMinutes": 20 }));
        assert!(!err, "{v}");
        assert_eq!(v["reminder"]["at"], "2026-10-03T10:20");
        assert_eq!(v["reminder"]["atJalali"], "1405/07/11 10:20");
        assert_eq!(call(&mut d, "add_reminder", json!({ "title": "Call mom", "at": "2026-10-04T18:30" })).1["reminder"]["at"], "2026-10-04T18:30");
        assert_eq!(call(&mut d, "add_reminder", json!({ "title": "Gym", "at": "09:00" })).1["reminder"]["at"], "2026-10-04T09:00", "09:00 has passed: tomorrow");
        assert_eq!(call(&mut d, "add_reminder", json!({ "title": "Lunch", "at": "13:00" })).1["reminder"]["at"], "2026-10-03T13:00");
        assert_eq!(call(&mut d, "add_reminder", json!({ "title": "Tea", "at": "16:00", "day": "tomorrow" })).1["reminder"]["at"], "2026-10-04T16:00");
        assert_eq!(call(&mut d, "add_reminder", json!({ "title": "UTC", "at": "2026-10-03T12:00Z" })).1["reminder"]["at"], "2026-10-03T15:30");
        let (_, v) = call(&mut d, "add_reminder", json!({ "title": "Standup", "at": "09:30", "day": "2026-10-08", "repeat": "weekdays" }));
        assert_eq!(v["reminder"]["at"], "2026-10-10T09:30", "Thursday → Saturday");
        for bad in [json!({ "title": "x" }), json!({ "title": "x", "at": "yesterday" }), json!({ "title": "x", "at": "2026-10-02T09:00" }), json!({ "title": "x", "inMinutes": 0 }), json!({ "title": "x", "inMinutes": 20, "repeat": "hourly" })] {
            assert!(call(&mut d, "add_reminder", bad.clone()).0, "{bad}");
        }
        let (_, list) = call(&mut d, "list_reminders", json!({}));
        assert_eq!(list["count"], 7);
        assert_eq!(list["reminders"][0]["title"], "Stand up", "soonest first");
        assert_eq!(list["nowJalali"], "1405/07/11 10:00");
    }

    #[test]
    fn notes_focus_and_habits() {
        let mut d = PlannerData::default();
        let (err, v) = call(&mut d, "add_note", json!({ "text": "Wi-Fi password is on the fridge" }));
        assert!(!err && v["note"]["chars"].as_u64() == Some(31), "{v}");
        assert!(call(&mut d, "add_note", json!({ "text": "   " })).0);

        let (_, v) = call(&mut d, "start_focus", json!({ "minutes": 50 }));
        assert_eq!((v["focus"]["phase"].as_str(), v["focus"]["minutesLeft"].as_i64()), (Some("focus"), Some(50)));
        assert_eq!(call(&mut d, "start_focus", json!({})).1["alreadyRunning"], true);
        assert_eq!(call(&mut d, "stop_focus", json!({})).1["focus"]["phase"], "idle");
        assert!(call(&mut d, "start_focus", json!({ "minutes": 999 })).0);

        assert!(call(&mut d, "log_habit", json!({ "title": "water" })).0, "no habits yet");
        d.add_habit("Drink water", HabitIcon::Water, HabitDays::Daily, None, 0).unwrap();
        d.add_habit("Stretch", HabitIcon::Stretch, HabitDays::Weekdays, None, 0).unwrap();
        d.habits[0].log = vec!["2026-10-01".into(), "2026-10-02".into()];
        let (err, v) = call(&mut d, "log_habit", json!({ "title": "water" }));
        assert!(!err, "{v}");
        assert_eq!((v["habit"]["doneToday"].as_bool(), v["habit"]["streak"].as_u64()), (Some(true), Some(3)));
        let (_, list) = call(&mut d, "list_habits", json!({}));
        assert_eq!(list["count"], 2);
        assert_eq!(list["habits"][1]["streak"], 0);
    }

    #[test]
    fn streaks_skip_days_off_and_an_unfinished_today() {
        let today = days_from_civil(2026, 10, 10); // Saturday
        let mut h = Habit { id: "h".into(), title: "x".into(), icon: HabitIcon::Read, days: HabitDays::Weekdays, nudge: None, log: vec![], created_at: 0 };
        // Wed 10-07 and Tue 10-06 done; Thu/Fri are off; today not done yet.
        h.log = vec!["2026-10-06".into(), "2026-10-07".into()];
        assert_eq!(streak(&h, today), 2);
        h.days = HabitDays::Daily;
        assert_eq!(streak(&h, today), 0, "a daily habit missed Thursday");
        h.log.push("2026-10-10".into());
        assert_eq!(streak(&Habit { days: HabitDays::Weekdays, ..h.clone() }, today), 3);
    }

    #[test]
    fn bad_arguments_never_change_anything() {
        let mut d = PlannerData::default();
        let before = d.clone();
        assert!(call(&mut d, "add_task", json!(["title"])).0);
        assert!(call(&mut d, "nope", json!({})).0);
        assert!(call(&mut d, "add_reminder", json!({ "title": "x", "at": "25:00" })).0);
        assert_eq!(d, before);
    }
}
