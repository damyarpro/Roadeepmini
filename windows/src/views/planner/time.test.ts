import { describe, expect, it } from "vitest";
import type { PlannerReminder, PlannerTask } from "../../core/bridge-planner";
import { setLanguage } from "../../core/i18n";
import {
  fmtClock, fmtCountdown, fmtLongDate, habitStreak, nextOccurrence, occurrenceOn, sortOpenTasks,
} from "./time";

const at = (y: number, mo: number, d: number, h = 0, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime();

const reminder = (over: Partial<PlannerReminder>): PlannerReminder => ({
  id: "r", title: "x", at: 0, repeat: "none", enabled: true, lastFiredAt: null, snoozedUntil: null, createdAt: 0, ...over,
});

describe("nextOccurrence", () => {
  // 2026-03-25 is a Wednesday, 2026-03-26 a Thursday.
  const now = at(2026, 3, 25, 12, 0);

  it("keeps a one-off reminder's own time, even past", () => {
    expect(nextOccurrence(reminder({ at: at(2026, 3, 25, 9) }), now)).toBe(at(2026, 3, 25, 9));
  });

  it("is null when switched off and follows a snooze", () => {
    expect(nextOccurrence(reminder({ enabled: false, at: now + 1 }), now)).toBeNull();
    expect(nextOccurrence(reminder({ at: now - 5, snoozedUntil: now + 600_000 }), now)).toBe(now + 600_000);
  });

  it("moves a daily reminder to the next day once its time passed", () => {
    const r = reminder({ at: at(2026, 3, 20, 9), repeat: "daily" });
    expect(nextOccurrence(r, now)).toBe(at(2026, 3, 26, 9));
    expect(nextOccurrence(r, at(2026, 3, 25, 8))).toBe(at(2026, 3, 25, 9));
  });

  it("skips Thursday and Friday for weekdays", () => {
    const r = reminder({ at: at(2026, 3, 20, 9), repeat: "weekdays" });
    // Wednesday noon → Saturday 09:00.
    expect(nextOccurrence(r, now)).toBe(at(2026, 3, 28, 9));
  });

  it("keeps the weekday of a weekly reminder", () => {
    const r = reminder({ at: at(2026, 3, 23, 19, 30), repeat: "weekly" }); // a Monday
    expect(nextOccurrence(r, now)).toBe(at(2026, 3, 30, 19, 30));
  });

  it("waits for a repeating reminder that starts later", () => {
    const r = reminder({ at: at(2026, 4, 2, 7), repeat: "daily" });
    expect(nextOccurrence(r, now)).toBe(at(2026, 4, 2, 7));
  });
});

describe("occurrenceOn", () => {
  it("places one-off and repeating reminders on their days", () => {
    const once = reminder({ at: at(2026, 3, 25, 9) });
    expect(occurrenceOn(once, "2026-03-25")).toBe(at(2026, 3, 25, 9));
    expect(occurrenceOn(once, "2026-03-26")).toBeNull();
    const work = reminder({ at: at(2026, 3, 21, 8, 30), repeat: "weekdays" });
    expect(occurrenceOn(work, "2026-03-20")).toBeNull(); // before it starts
    expect(occurrenceOn(work, "2026-03-25")).toBe(at(2026, 3, 25, 8, 30));
    expect(occurrenceOn(work, "2026-03-26")).toBeNull(); // Thursday
    const weekly = reminder({ at: at(2026, 3, 23, 19), repeat: "weekly" });
    expect(occurrenceOn(weekly, "2026-03-30")).toBe(at(2026, 3, 30, 19));
    expect(occurrenceOn(weekly, "2026-03-31")).toBeNull();
    expect(occurrenceOn({ ...weekly, enabled: false }, "2026-03-30")).toBeNull();
  });
});

describe("habitStreak", () => {
  const today = "2026-03-25"; // Wednesday

  it("counts the run up to today, or yesterday when today is still open", () => {
    expect(habitStreak({ days: "daily", log: ["2026-03-23", "2026-03-24", "2026-03-25"] }, today)).toBe(3);
    expect(habitStreak({ days: "daily", log: ["2026-03-23", "2026-03-24"] }, today)).toBe(2);
    expect(habitStreak({ days: "daily", log: ["2026-03-22", "2026-03-24"] }, today)).toBe(1);
    expect(habitStreak({ days: "daily", log: [] }, today)).toBe(0);
  });

  it("skips the weekend of a weekdays habit", () => {
    // Saturday 2026-03-28: Thursday and Friday don't break the run.
    expect(habitStreak({ days: "weekdays", log: ["2026-03-24", "2026-03-25", "2026-03-28"] }, "2026-03-28")).toBe(3);
  });
});

describe("sortOpenTasks", () => {
  const task = (id: string, due: string | null, createdAt = 0, done = false): PlannerTask => ({
    id, title: id, note: "", done, due, createdAt, updatedAt: 0, doneAt: null,
  });

  it("puts overdue first and undated last, oldest first within a day", () => {
    const out = sortOpenTasks([
      task("none", null), task("later", "2026-04-01"), task("today2", "2026-03-25", 2),
      task("today1", "2026-03-25", 1), task("late", "2026-03-20"), task("done", "2026-03-01", 0, true),
    ]);
    expect(out.map((x) => x.id)).toEqual(["late", "today1", "today2", "later", "none"]);
  });
});

describe("display", () => {
  it("shows Jalali dates with Persian digits in fa", () => {
    setLanguage("fa");
    expect(fmtLongDate("2026-04-04")).toBe("شنبه ۱۵ فروردین ۱۴۰۵");
    expect(fmtClock(at(2026, 4, 4, 9, 5))).toBe("۰۹:۰۵");
    expect(fmtCountdown(25 * 60_000)).toBe("۲۵:۰۰");
  });

  it("shows Gregorian dates in en", () => {
    setLanguage("en");
    expect(fmtLongDate("2026-04-04")).toBe("Saturday, April 4, 2026");
    expect(fmtCountdown(61_500)).toBe("01:02");
    setLanguage("fa");
  });
});
