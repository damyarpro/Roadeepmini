// Dates and times as the planner shows them, and the small rules the views
// share (next reminder time, habit streaks, task order). Persian shows the
// Jalali calendar with Persian digits; English shows the Gregorian one. Both
// weeks start on Saturday, like the Rust scheduler's "weekdays".

import type { PlannerHabit, PlannerReminder, PlannerTask } from "../../core/bridge-planner";
import {
  addDays, dateOfDayKey, dayKeyOf, daysBetween, isWorkday, jalaliOfDayKey, satWeekday,
} from "../../core/jalali";
import { formatNumber, getLanguage, t } from "../../core/i18n";

const MINUTE = 60_000;

// ── Pure rules ────────────────────────────────────────────────────────────────

/**
 * When a reminder goes off next, or null when it is switched off. A one-off
 * reminder keeps its own time even once past; a repeating one moves to the
 * next day its rule allows, at the same time of day.
 */
export function nextOccurrence(r: PlannerReminder, now: number): number | null {
  if (!r.enabled) return null;
  if (r.snoozedUntil != null && r.snoozedUntil > now) return r.snoozedUntil;
  if (r.repeat === "none") return r.at;
  const base = new Date(r.at);
  const from = Math.max(r.at, now);
  let key = dayKeyOf(new Date(from));
  // A week always holds a matching day.
  for (let i = 0; i < 8; i++) {
    const d = dateOfDayKey(key);
    d.setHours(base.getHours(), base.getMinutes(), 0, 0);
    const matches =
      r.repeat === "daily" ||
      (r.repeat === "weekdays" && isWorkday(key)) ||
      (r.repeat === "weekly" && d.getDay() === base.getDay());
    if (matches && d.getTime() >= from) return d.getTime();
    key = addDays(key, 1);
  }
  return null;
}

/** The time a reminder goes off on day `key`, or null when it doesn't (switched-off ones too). */
export function occurrenceOn(r: PlannerReminder, key: string): number | null {
  if (!r.enabled) return null;
  const base = new Date(r.at);
  if (r.repeat === "none") {
    const when = r.snoozedUntil != null && r.snoozedUntil > r.at ? r.snoozedUntil : r.at;
    return dayKeyOf(new Date(when)) === key ? when : null;
  }
  // A repeating reminder starts on the day of its `at`.
  if (key < dayKeyOf(base)) return null;
  const d = dateOfDayKey(key);
  const matches =
    r.repeat === "daily" ||
    (r.repeat === "weekdays" && isWorkday(key)) ||
    (r.repeat === "weekly" && d.getDay() === base.getDay());
  if (!matches) return null;
  d.setHours(base.getHours(), base.getMinutes(), 0, 0);
  return d.getTime();
}

/** Whether a habit is meant for this day ("weekdays" = Saturday to Wednesday). */
export function habitDueOn(h: Pick<PlannerHabit, "days">, key: string): boolean {
  return h.days === "daily" || isWorkday(key);
}

/**
 * Days in a row the habit was kept, up to today. Today not done yet doesn't
 * break the run (the day isn't over); days the habit isn't meant for are skipped.
 */
export function habitStreak(h: Pick<PlannerHabit, "days" | "log">, today: string): number {
  const done = new Set(h.log);
  let key = done.has(today) ? today : addDays(today, -1);
  let streak = 0;
  // Bounded by the log: a streak can't be longer than the days logged.
  for (let guard = 0; guard < h.log.length + 7; guard++) {
    if (!habitDueOn(h, key)) {
      key = addDays(key, -1);
      continue;
    }
    if (!done.has(key)) break;
    streak += 1;
    key = addDays(key, -1);
  }
  return streak;
}

/** Overdue first, then today, the coming days in order, and undated last; oldest first within. */
export function sortOpenTasks(tasks: PlannerTask[]): PlannerTask[] {
  return tasks
    .filter((x) => !x.done)
    .sort((a, b) => {
      if (a.due !== b.due) {
        if (a.due == null) return 1;
        if (b.due == null) return -1;
        return a.due < b.due ? -1 : 1;
      }
      return a.createdAt - b.createdAt;
    });
}

// ── Display ───────────────────────────────────────────────────────────────────

const FA_MONTHS = ["فروردین", "اردیبهشت", "خرداد", "تیر", "مرداد", "شهریور", "مهر", "آبان", "آذر", "دی", "بهمن", "اسفند"];
const FA_WEEKDAYS = ["شنبه", "یکشنبه", "دوشنبه", "سه‌شنبه", "چهارشنبه", "پنجشنبه", "جمعه"];
const FA_WEEKDAYS_SHORT = ["ش", "ی", "د", "س", "چ", "پ", "ج"];

const fa = () => getLanguage() === "fa";

/** A number as the UI shows it (Persian digits in fa), zero-padded to `width`. */
export function num(n: number, width = 1): string {
  return formatNumber(n, { minimumIntegerDigits: width });
}

/** "09:05" / «۰۹:۰۵» — 24-hour, local time. */
export function fmtClock(ms: number): string {
  const d = new Date(ms);
  return `${num(d.getHours(), 2)}:${num(d.getMinutes(), 2)}`;
}

/** "mm:ss" for a countdown (hours fold into the minutes). */
export function fmtCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  return `${num(Math.floor(total / 60), 2)}:${num(total % 60, 2)}`;
}

export function weekdayName(key: string, short = false): string {
  const i = satWeekday(key);
  if (fa()) return (short ? FA_WEEKDAYS_SHORT : FA_WEEKDAYS)[i];
  return dateOfDayKey(key).toLocaleDateString("en-US", { weekday: short ? "short" : "long" });
}

/** Day of month in the UI's calendar. */
export function dayOfMonth(key: string): string {
  return fa() ? num(jalaliOfDayKey(key).jd) : num(dateOfDayKey(key).getDate());
}

/** «شنبه ۱۵ فروردین ۱۴۰۵» / "Saturday, April 4, 2026". */
export function fmtLongDate(key: string): string {
  if (fa()) {
    const j = jalaliOfDayKey(key);
    return `${weekdayName(key)} ${num(j.jd)} ${FA_MONTHS[j.jm - 1]} ${num(j.jy)}`;
  }
  return dateOfDayKey(key).toLocaleDateString("en-US", { weekday: "long", month: "long", day: "numeric", year: "numeric" });
}

/** «۱۵ فروردین» (this year) or «۱۴۰۵/۰۱/۱۵»; "Apr 4" or "Apr 4, 2027". */
export function fmtDate(key: string, today: string): string {
  if (fa()) {
    const j = jalaliOfDayKey(key);
    if (j.jy === jalaliOfDayKey(today).jy) return `${num(j.jd)} ${FA_MONTHS[j.jm - 1]}`;
    return `${num(j.jy)}/${num(j.jm, 2)}/${num(j.jd, 2)}`;
  }
  const d = dateOfDayKey(key);
  const sameYear = d.getFullYear() === dateOfDayKey(today).getFullYear();
  return d.toLocaleDateString("en-US", sameYear ? { month: "short", day: "numeric" } : { month: "short", day: "numeric", year: "numeric" });
}

/** «امروز», «فردا», «دیروز», a weekday this week, or the date. */
export function fmtDay(key: string, today: string): string {
  const diff = daysBetween(today, key);
  if (diff === 0) return t("planner.today");
  if (diff === 1) return t("planner.tomorrow");
  if (diff === -1) return t("planner.yesterday");
  if (diff > 1 && diff < 7) return weekdayName(key);
  return fmtDate(key, today);
}

/** «۲۰ دقیقه دیگر», «امروز ۱۴:۳۰», «فردا ۰۹:۰۰», «دوشنبه ۰۹:۰۰»… */
export function fmtWhen(at: number, now: number): string {
  const diff = at - now;
  if (diff < 0) return t("planner.when.past", { time: fmtWhenAbsolute(at, now) });
  if (diff < MINUTE) return t("planner.when.now");
  if (diff < 60 * MINUTE) return t("planner.when.inMinutes", { n: Math.ceil(diff / MINUTE) });
  return fmtWhenAbsolute(at, now);
}

function fmtWhenAbsolute(at: number, now: number): string {
  const today = dayKeyOf(new Date(now));
  const key = dayKeyOf(new Date(at));
  return t("planner.when.dayTime", { day: fmtDay(key, today), time: fmtClock(at) });
}

/** «۳ ساعت پیش», «دیروز», a date — for when a note was written. */
export function fmtAgo(ms: number, now: number): string {
  const diff = now - ms;
  if (diff < MINUTE) return t("planner.ago.now");
  if (diff < 60 * MINUTE) return t("planner.ago.minutes", { n: Math.floor(diff / MINUTE) });
  const today = dayKeyOf(new Date(now));
  const key = dayKeyOf(new Date(ms));
  if (key === today) return t("planner.ago.hours", { n: Math.floor(diff / (60 * MINUTE)) });
  return fmtDay(key, today);
}

export { addDays, dayKeyOf };
