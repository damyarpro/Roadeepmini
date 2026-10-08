// Tauri commands and events of the planner (tasks, notes, reminders, habits,
// the focus timer). Rust owns the data (src-tauri/src/planner); every command
// rejects with a coded string (core/error-text.ts) that the views show with
// localizeError.
//
// Outside Tauri (the Vite preview) an in-memory stand-in answers instead, with
// a few sample items, so every planner view can be looked at in a browser.

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI, onEvent } from "./bridge";
import { addDays, dayKeyOf } from "./jalali";
import { State } from "./state";

export interface PlannerTask {
  id: string;
  title: string;
  note: string;
  done: boolean;
  /** Gregorian local day key, "YYYY-MM-DD". */
  due: string | null;
  createdAt: number;
  updatedAt: number;
  doneAt: number | null;
}

export interface PlannerNote {
  id: string;
  text: string;
  pinned: boolean;
  createdAt: number;
  updatedAt: number;
}

export type ReminderRepeat = "none" | "daily" | "weekdays" | "weekly";

export interface PlannerReminder {
  id: string;
  title: string;
  /** Ms since epoch; for a repeating reminder, the time of day (and weekday) it keeps. */
  at: number;
  repeat: ReminderRepeat;
  enabled: boolean;
  lastFiredAt: number | null;
  snoozedUntil: number | null;
  createdAt: number;
}

export type HabitIcon = "water" | "stretch" | "posture" | "eyes" | "walk" | "read" | "sleep" | "custom";
export const HABIT_ICONS: HabitIcon[] = ["water", "stretch", "posture", "eyes", "walk", "read", "sleep", "custom"];

export interface HabitNudge {
  /** 15…240 minutes. */
  everyMinutes: number;
  /** "HH:MM", local. */
  from: string;
  to: string;
}

export interface PlannerHabit {
  id: string;
  title: string;
  icon: HabitIcon;
  days: "daily" | "weekdays";
  nudge: HabitNudge | null;
  /** Day keys the habit was done on, sorted, unique. */
  log: string[];
  createdAt: number;
}

export type FocusPhase = "idle" | "focus" | "break" | "longBreak" | "paused";
export type FocusRunPhase = "focus" | "break" | "longBreak";

export interface FocusState {
  phase: FocusPhase;
  endsAt: number | null;
  /** While paused: what was left of the phase. */
  remainingMs: number | null;
  /** 1-based round within the cycle before a long break. */
  round: number;
  roundsDoneToday: number;
  startedAt: number | null;
  dayKey: string;
  /** While paused: the phase the pause holds. */
  pausedFrom?: FocusRunPhase | null;
  /** While idle: what focus_resume (or focus_start without a phase) starts. */
  next?: FocusRunPhase;
  /** Full length of the running or paused phase. */
  durationMs?: number | null;
}

export interface PlannerData {
  version: 1;
  tasks: PlannerTask[];
  notes: PlannerNote[];
  reminders: PlannerReminder[];
  habits: PlannerHabit[];
  focus: FocusState;
}

export type PlannerKind = "tasks" | "notes" | "reminders" | "habits" | "focus";

export interface PlannerChanged {
  kind: PlannerKind;
}

export type PlannerFire =
  | { kind: "reminder"; id: string; title: string }
  | { kind: "focus"; phase: "focusDone" | "breakDone" | "longBreakDone"; round: number }
  | { kind: "habit"; id: string; title: string; icon: HabitIcon };

export const PLANNER_CHANGED_EVENT = "planner-changed";
export const PLANNER_FIRE_EVENT = "planner-fire";

/** Limits Rust enforces too (planner/store.rs); the fields stop typing at these. */
export const PLANNER_LIMITS = {
  taskTitle: 200,
  taskNote: 2000,
  noteText: 4000,
  reminderTitle: 200,
  habitTitle: 80,
} as const;

export interface TaskPatch {
  title?: string;
  note?: string;
  due?: string | null;
  done?: boolean;
}

export interface HabitDraft {
  title: string;
  icon: HabitIcon;
  days?: "daily" | "weekdays";
  nudge?: HabitNudge | null;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) return mock(cmd, args ?? {}) as T;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    // Only the code: the message may echo a title or a note.
    const code = String(err).split(/[|\n]/, 1)[0].slice(0, 64);
    console.error(`[roadeep] ${cmd} failed`, code);
    void Bridge.log(`planner: ${cmd} failed ${code}`);
    throw err;
  }
}

export const PlannerBridge = {
  get: () => call<PlannerData>("planner_get"),

  taskAdd: (title: string, due: string | null = null, note = "") =>
    call<PlannerTask>("planner_task_add", { title, due, note }),
  taskUpdate: (id: string, patch: TaskPatch) => call<PlannerTask>("planner_task_update", { id, ...patch }),
  taskDelete: (id: string) => call<void>("planner_task_delete", { id }),

  noteAdd: (text: string) => call<PlannerNote>("planner_note_add", { text }),
  noteUpdate: (id: string, patch: { text?: string; pinned?: boolean }) =>
    call<PlannerNote>("planner_note_update", { id, ...patch }),
  noteDelete: (id: string) => call<void>("planner_note_delete", { id }),

  reminderAdd: (title: string, at: number, repeat: ReminderRepeat = "none") =>
    call<PlannerReminder>("planner_reminder_add", { title, at, repeat }),
  reminderUpdate: (
    id: string,
    patch: { title?: string; at?: number; repeat?: ReminderRepeat; enabled?: boolean },
  ) => call<PlannerReminder>("planner_reminder_update", { id, ...patch }),
  reminderDelete: (id: string) => call<void>("planner_reminder_delete", { id }),
  reminderSnooze: (id: string, minutes: number) => call<PlannerReminder>("planner_reminder_snooze", { id, minutes }),

  habitAdd: (draft: HabitDraft) => call<PlannerHabit>("planner_habit_add", { ...draft }),
  habitUpdate: (id: string, patch: Partial<HabitDraft>) => call<PlannerHabit>("planner_habit_update", { id, ...patch }),
  habitDelete: (id: string) => call<void>("planner_habit_delete", { id }),
  habitCheck: (id: string, dayKey: string, done: boolean) =>
    call<PlannerHabit>("planner_habit_check", { id, dayKey, done }),

  /**
   * Starts a phase: `phase` defaults to the state's `next` (after a focus
   * round, its break), `minutes` to the setting for that phase.
   */
  focusStart: (phase?: FocusRunPhase, minutes?: number) =>
    call<FocusState>("focus_start", { minutes: minutes ?? null, phase: phase ?? null }),
  focusPause: () => call<FocusState>("focus_pause"),
  /** Resumes a paused phase; from idle, starts `next`. */
  focusResume: () => call<FocusState>("focus_resume"),
  /** Ends the current phase now (counts as finished, no "planner-fire"). */
  focusSkip: () => call<FocusState>("focus_skip"),
  focusStop: () => call<FocusState>("focus_stop"),
  focusGet: () => call<FocusState>("focus_get"),

  /**
   * The alerts fired before the island listened (the launch catch-ups), oldest
   * first, then cleared in Rust. Call once, after `onPlannerFire` has resolved.
   */
  pendingFires: () => call<PlannerFire[]>("planner_pending_fires"),
};

// ── Events ────────────────────────────────────────────────────────────────────

const mockListeners = {
  changed: new Set<(e: PlannerChanged) => void>(),
  fire: new Set<(e: PlannerFire) => void>(),
};

export function onPlannerChanged(handler: (e: PlannerChanged) => void) {
  if (!IS_TAURI) {
    mockListeners.changed.add(handler);
    return Promise.resolve(() => void mockListeners.changed.delete(handler));
  }
  return onEvent<PlannerChanged>(PLANNER_CHANGED_EVENT, handler);
}

export function onPlannerFire(handler: (e: PlannerFire) => void) {
  if (!IS_TAURI) {
    mockListeners.fire.add(handler);
    return Promise.resolve(() => void mockListeners.fire.delete(handler));
  }
  return onEvent<PlannerFire>(PLANNER_FIRE_EVENT, handler);
}

// ── Browser stand-in ──────────────────────────────────────────────────────────
// Never used inside the app. Mirrors the Rust rules the views rely on (limits,
// focus phases) closely enough to look at; it is not a second implementation.

let mockData: PlannerData | null = null;
let mockFocusTimer: number | null = null;
const MINUTE = 60_000;

const uid = () =>
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;

function seed(): PlannerData {
  const now = Date.now();
  const today = dayKeyOf(new Date());
  const fa = typeof document === "undefined" || document.documentElement.lang !== "en";
  const s = (faText: string, enText: string) => (fa ? faText : enText);
  const at = (days: number, h: number, m: number) => {
    const d = new Date();
    d.setDate(d.getDate() + days);
    d.setHours(h, m, 0, 0);
    return d.getTime();
  };
  const task = (title: string, due: string | null, done = false): PlannerTask => ({
    id: uid(), title, note: "", done, due, createdAt: now, updatedAt: now, doneAt: done ? now : null,
  });
  // A short run of done days ending yesterday or today, for the streak and the dots.
  const logOf = (days: number[]) => days.map((d) => addDays(today, -d)).sort();
  return {
    version: 1,
    tasks: [
      task(s("فرستادن گزارش هفتگی", "Send the weekly report"), addDays(today, -1)),
      task(s("تماس با پشتیبانی بانک", "Call the bank's support"), today),
      task(s("خرید نان و شیر", "Buy bread and milk"), today),
      task(s("آماده کردن اسلایدهای جلسه", "Prepare the meeting slides"), addDays(today, 1)),
      task(s("کتاب «ملت عشق» را تمام کنم", "Finish the novel"), null),
      task(s("پرداخت قبض برق", "Pay the electricity bill"), today, true),
    ],
    notes: [
      { id: uid(), text: s("رمز وای‌فای مهمان روی برگهٔ زرد پشت مانیتور است.", "The guest Wi-Fi password is on the yellow note behind the monitor."), pinned: true, createdAt: now - 86_400_000, updatedAt: now - 86_400_000 },
      { id: uid(), text: s("ایده: برای جلسهٔ شنبه یک خلاصهٔ یک‌صفحه‌ای از پیشرفت پروژه بنویسم.\nسه بخش: انجام‌شده، در جریان، گیرها.", "Idea: write a one-page progress summary for Saturday's meeting.\nThree parts: done, in progress, blockers."), pinned: false, createdAt: now - 3 * 3_600_000, updatedAt: now - 3 * 3_600_000 },
    ],
    reminders: [
      { id: uid(), title: s("قرص ویتامین", "Vitamin pill"), at: now + 20 * MINUTE, repeat: "none", enabled: true, lastFiredAt: null, snoozedUntil: null, createdAt: now },
      { id: uid(), title: s("جلسهٔ تیم", "Team meeting"), at: at(1, 9, 0), repeat: "weekdays", enabled: true, lastFiredAt: null, snoozedUntil: null, createdAt: now },
      { id: uid(), title: s("زنگ به مامان", "Call mum"), at: at(3, 19, 30), repeat: "weekly", enabled: false, lastFiredAt: null, snoozedUntil: null, createdAt: now },
    ],
    habits: [
      { id: uid(), title: s("آب بنوش", "Drink water"), icon: "water", days: "daily", nudge: { everyMinutes: 60, from: "09:00", to: "18:00" }, log: logOf([0, 1, 2, 3, 4]), createdAt: now },
      { id: uid(), title: s("کشش", "Stretch"), icon: "stretch", days: "weekdays", nudge: { everyMinutes: 90, from: "10:00", to: "17:00" }, log: logOf([1, 2, 5]), createdAt: now },
      { id: uid(), title: s("نشستن درست", "Sit up straight"), icon: "posture", days: "daily", nudge: null, log: logOf([2, 3]), createdAt: now },
    ],
    focus: {
      phase: "idle", endsAt: null, remainingMs: null, round: 2, roundsDoneToday: 1, startedAt: null, dayKey: today,
      pausedFrom: null, next: "focus", durationMs: null,
    },
  };
}

function emitChanged(kind: PlannerKind) {
  for (const fn of mockListeners.changed) fn({ kind });
}

/** Preview only: lets the console fire an alert (`__plannerFire({kind:"reminder",…})`). */
function emitFire(e: PlannerFire) {
  for (const fn of mockListeners.fire) fn(e);
}

function mockFocusSettings(): { focus: number; brk: number; long: number; rounds: number } {
  const s = State.settings;
  return { focus: s.focusMinutes, brk: s.breakMinutes, long: s.longBreakMinutes, rounds: s.roundsBeforeLongBreak };
}

function armMockFocus(d: PlannerData) {
  if (mockFocusTimer != null) window.clearTimeout(mockFocusTimer);
  mockFocusTimer = null;
  const f = d.focus;
  if (f.endsAt == null || f.phase === "paused" || f.phase === "idle") return;
  mockFocusTimer = window.setTimeout(() => endMockPhase(d), Math.max(0, f.endsAt - Date.now()));
}

function endMockPhase(d: PlannerData, fire = true) {
  const f = d.focus;
  const { rounds } = mockFocusSettings();
  const phase = f.phase === "paused" ? f.pausedFrom ?? "focus" : f.phase;
  if (phase === "focus") {
    f.roundsDoneToday += 1;
    if (fire) emitFire({ kind: "focus", phase: "focusDone", round: f.round });
    f.next = f.round >= rounds ? "longBreak" : "break";
  } else if (phase === "break" || phase === "longBreak") {
    if (fire) emitFire({ kind: "focus", phase: phase === "break" ? "breakDone" : "longBreakDone", round: f.round });
    f.round = phase === "longBreak" ? 1 : f.round + 1;
    f.next = "focus";
  }
  // The next phase waits for the user (focus_start / focus_resume).
  Object.assign(f, { phase: "idle", endsAt: null, startedAt: null, remainingMs: null, pausedFrom: null, durationMs: null });
  emitChanged("focus");
}

function mock(cmd: string, a: Record<string, unknown>): unknown {
  if (typeof window !== "undefined" && !(window as { __plannerFire?: unknown }).__plannerFire) {
    (window as { __plannerFire?: unknown }).__plannerFire = emitFire;
  }
  const d = (mockData ??= seed());
  const now = Date.now();
  const find = <T extends { id: string }>(list: T[]): T => {
    const item = list.find((x) => x.id === a.id);
    if (!item) throw "E_PLANNER_NOT_FOUND";
    return item;
  };
  const text = (v: unknown, max: number): string => {
    const s = String(v ?? "").trim();
    if (!s) throw "E_PLANNER_EMPTY";
    return s.slice(0, max);
  };
  const copy = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
  const f = d.focus;
  const fs = mockFocusSettings();
  const startPhase = (phase: FocusRunPhase, minutes: number) => {
    // A focus right after a focus (its break skipped) is the next round.
    if (phase === "focus" && f.phase === "idle" && f.next !== "focus") f.round = f.round >= fs.rounds ? 1 : f.round + 1;
    f.phase = phase;
    f.durationMs = minutes * MINUTE;
    f.startedAt = now;
    f.endsAt = now + minutes * MINUTE;
    f.remainingMs = null;
    armMockFocus(d);
    emitChanged("focus");
    return copy(d.focus);
  };
  switch (cmd) {
    case "planner_get":
      return copy(d);
    case "planner_task_add": {
      const t: PlannerTask = {
        id: uid(), title: text(a.title, PLANNER_LIMITS.taskTitle), note: String(a.note ?? ""), done: false,
        due: (a.due as string | null) ?? null, createdAt: now, updatedAt: now, doneAt: null,
      };
      d.tasks.push(t);
      emitChanged("tasks");
      return copy(t);
    }
    case "planner_task_update": {
      const t = find(d.tasks);
      if (a.title !== undefined) t.title = text(a.title, PLANNER_LIMITS.taskTitle);
      if (a.note !== undefined) t.note = String(a.note);
      if (a.due !== undefined) t.due = (a.due as string | null) || null;
      if (a.done !== undefined) {
        t.done = !!a.done;
        t.doneAt = t.done ? now : null;
      }
      t.updatedAt = now;
      emitChanged("tasks");
      return copy(t);
    }
    case "planner_task_delete":
      find(d.tasks);
      d.tasks = d.tasks.filter((x) => x.id !== a.id);
      emitChanged("tasks");
      return null;
    case "planner_note_add": {
      const n: PlannerNote = { id: uid(), text: text(a.text, PLANNER_LIMITS.noteText), pinned: false, createdAt: now, updatedAt: now };
      d.notes.push(n);
      emitChanged("notes");
      return copy(n);
    }
    case "planner_note_update": {
      const n = find(d.notes);
      if (a.text !== undefined) n.text = text(a.text, PLANNER_LIMITS.noteText);
      if (a.pinned !== undefined) n.pinned = !!a.pinned;
      n.updatedAt = now;
      emitChanged("notes");
      return copy(n);
    }
    case "planner_note_delete":
      find(d.notes);
      d.notes = d.notes.filter((x) => x.id !== a.id);
      emitChanged("notes");
      return null;
    case "planner_reminder_add": {
      const r: PlannerReminder = {
        id: uid(), title: text(a.title, PLANNER_LIMITS.reminderTitle), at: Number(a.at),
        repeat: (a.repeat as ReminderRepeat) ?? "none", enabled: true, lastFiredAt: null, snoozedUntil: null, createdAt: now,
      };
      d.reminders.push(r);
      emitChanged("reminders");
      return copy(r);
    }
    case "planner_reminder_update": {
      const r = find(d.reminders);
      if (a.title !== undefined) r.title = text(a.title, PLANNER_LIMITS.reminderTitle);
      if (a.at !== undefined) r.at = Number(a.at);
      if (a.repeat !== undefined) r.repeat = a.repeat as ReminderRepeat;
      if (a.enabled !== undefined) r.enabled = !!a.enabled;
      emitChanged("reminders");
      return copy(r);
    }
    case "planner_reminder_delete":
      find(d.reminders);
      d.reminders = d.reminders.filter((x) => x.id !== a.id);
      emitChanged("reminders");
      return null;
    case "planner_reminder_snooze": {
      const r = find(d.reminders);
      r.snoozedUntil = now + Number(a.minutes) * MINUTE;
      emitChanged("reminders");
      return copy(r);
    }
    case "planner_habit_add": {
      const h: PlannerHabit = {
        id: uid(), title: text(a.title, PLANNER_LIMITS.habitTitle), icon: (a.icon as HabitIcon) ?? "custom",
        days: (a.days as "daily" | "weekdays") ?? "daily", nudge: (a.nudge as HabitNudge | null) ?? null, log: [], createdAt: now,
      };
      d.habits.push(h);
      emitChanged("habits");
      return copy(h);
    }
    case "planner_habit_update": {
      const h = find(d.habits);
      if (a.title !== undefined) h.title = text(a.title, PLANNER_LIMITS.habitTitle);
      if (a.icon !== undefined) h.icon = a.icon as HabitIcon;
      if (a.days !== undefined) h.days = a.days as "daily" | "weekdays";
      if (a.nudge !== undefined) h.nudge = (a.nudge as HabitNudge | null) ?? null;
      emitChanged("habits");
      return copy(h);
    }
    case "planner_habit_delete":
      find(d.habits);
      d.habits = d.habits.filter((x) => x.id !== a.id);
      emitChanged("habits");
      return null;
    case "planner_habit_check": {
      const h = find(d.habits);
      const key = String(a.dayKey);
      const set = new Set(h.log);
      if (a.done) set.add(key);
      else set.delete(key);
      h.log = [...set].sort();
      emitChanged("habits");
      return copy(h);
    }
    case "focus_start": {
      const phase = (a.phase as FocusRunPhase | null) ?? f.next ?? "focus";
      const minutes = Number(a.minutes) || (phase === "focus" ? fs.focus : phase === "break" ? fs.brk : fs.long);
      return startPhase(phase, minutes);
    }
    case "focus_resume":
      if (f.phase === "paused" && f.remainingMs != null) {
        f.phase = f.pausedFrom ?? "focus";
        f.endsAt = now + f.remainingMs;
        f.remainingMs = null;
        f.pausedFrom = null;
        armMockFocus(d);
        emitChanged("focus");
        return copy(d.focus);
      }
      if (f.phase === "idle") {
        const phase = f.next ?? "focus";
        return startPhase(phase, phase === "focus" ? fs.focus : phase === "break" ? fs.brk : fs.long);
      }
      return copy(d.focus);
    case "focus_pause":
      if (f.endsAt != null && f.phase !== "idle" && f.phase !== "paused") {
        f.pausedFrom = f.phase;
        f.remainingMs = Math.max(0, f.endsAt - now);
        f.phase = "paused";
        f.endsAt = null;
        armMockFocus(d);
        emitChanged("focus");
      }
      return copy(d.focus);
    case "focus_skip":
      if (f.phase !== "idle") endMockPhase(d, false);
      armMockFocus(d);
      return copy(d.focus);
    case "focus_stop":
      Object.assign(f, {
        phase: "idle", endsAt: null, remainingMs: null, startedAt: null, pausedFrom: null, durationMs: null,
        round: 1, next: "focus",
      });
      armMockFocus(d);
      emitChanged("focus");
      return copy(d.focus);
    case "focus_get":
      return copy(d.focus);
    case "planner_pending_fires":
      return [];
    default:
      throw `E_PLANNER_UNKNOWN_COMMAND|${cmd}`;
  }
}
