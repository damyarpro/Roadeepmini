// «یادآورها»: a title, a time, a day («امروز», «فردا» or stepped one day at a
// time, shown in the UI's calendar) and how it repeats; below, the reminders
// in the order they go off, each with its switch and a delete button.

import { h } from "../dom";
import type { ViewActions, ViewHost } from "../views";
import { PLANNER_LIMITS, PlannerBridge, type PlannerReminder, type ReminderRepeat } from "../../core/bridge-planner";
import { Sound } from "../../core/sound";
import { textDirection, t } from "../../core/i18n";
import { dateOfDayKey } from "../../core/jalali";
import { openPalette } from "../palette";
import { Planner, attempt } from "./store";
import { addDays, dayKeyOf, fmtDate, fmtWhen, nextOccurrence } from "./time";
import {
  emptyState, icon, iconButton, limitField, plannerShell, rerender, setPressed, timePicker, toggleChip,
} from "./ui";
import { ICONS } from "../icons";
import { svg } from "../dom";

const REPEATS: ReminderRepeat[] = ["none", "daily", "weekdays", "weekly"];
/** How far ahead the day stepper goes. */
const MAX_DAYS_AHEAD = 365;

/** The next full hour as "HH:MM" (what the time field starts with). */
function nextHour(now: Date): string {
  const d = new Date(now);
  d.setHours(d.getHours() + 1, 0, 0, 0);
  return `${String(d.getHours()).padStart(2, "0")}:00`;
}

export function buildReminders(actions: ViewActions): ViewHost {
  const shell = plannerShell({ icon: "reminders", title: t("planner.reminders.title"), onMenu: () => openPalette(actions) });

  const title = h("input", {
    type: "text",
    class: "pl-input",
    placeholder: t("planner.reminders.placeholder"),
    "aria-label": t("planner.reminders.what"),
    spellcheck: "false",
    autocomplete: "off",
  }) as HTMLInputElement;
  limitField(title, PLANNER_LIMITS.reminderTitle);
  title.addEventListener("input", () => {
    title.dir = textDirection(title.value) ?? "";
  });
  const time = timePicker(t("planner.reminders.time"), nextHour(new Date()));
  const addBtn = iconButton(icon("plus", 14), t("planner.reminders.add"), () => void add(), "pl-add");

  /** The chosen day, as an offset from today. */
  let dayOffset = 0;
  const todayChip = toggleChip(t("planner.today"), () => setDay(0));
  const tomorrowChip = toggleChip(t("planner.tomorrow"), () => setDay(1));
  const dayLabel = h("span", { class: "pl-step-label", "aria-live": "polite" });
  const prev = iconButton(svg(ICONS.chevronLeft, 12, { stroke: 2 }), t("planner.reminders.prevDay"), () => setDay(dayOffset - 1), "pl-flip");
  const next = iconButton(svg(ICONS.chevronRight, 12, { stroke: 2 }), t("planner.reminders.nextDay"), () => setDay(dayOffset + 1), "pl-flip");
  const stepper = h("div", { class: "pl-stepper", role: "group", "aria-label": t("planner.reminders.day") }, prev, dayLabel, next);
  const repeat = h("select", { class: "pl-select", "aria-label": t("planner.reminders.repeat") },
    ...REPEATS.map((r) => h("option", { value: r, text: t(`planner.repeat.${r}`) })),
  ) as HTMLSelectElement;
  repeat.addEventListener("keydown", (e) => e.stopPropagation());

  function setDay(offset: number) {
    dayOffset = Math.max(0, Math.min(MAX_DAYS_AHEAD, offset));
    const today = dayKeyOf(new Date());
    const key = addDays(today, dayOffset);
    // The exact date: «امروز» / «فردا» are the chips beside it.
    dayLabel.textContent = fmtDate(key, today);
    dayLabel.title = dayLabel.textContent;
    setPressed(todayChip, dayOffset === 0);
    setPressed(tomorrowChip, dayOffset === 1);
    prev.disabled = dayOffset === 0;
  }
  setDay(0);

  const form = h("div", { class: "pl-form" },
    h("div", { class: "pl-add-row" }, title, time.el, addBtn),
    h("div", { class: "pl-add-row sub" }, todayChip, tomorrowChip, stepper, h("div", { class: "grow" }), repeat),
  );
  const list = h("ul", { class: "pl-list", "aria-label": t("planner.reminders.list") });
  shell.body.append(form, h("div", { class: "pl-scroll" }, list));

  title.addEventListener("keydown", (e) => {
    const k = e as KeyboardEvent;
    if (k.key === "Enter" && !k.isComposing) {
      e.preventDefault();
      void add();
    }
    if (k.key !== "Escape") e.stopPropagation();
  });

  let renderedKey = "";
  let minuteTimer: number | null = null;

  async function add() {
    const text = title.value.trim();
    if (!text) {
      title.focus();
      return;
    }
    const m = /^(\d{2}):(\d{2})$/.exec(time.value) as RegExpExecArray;
    const d = dateOfDayKey(addDays(dayKeyOf(new Date()), dayOffset));
    d.setHours(Number(m[1]), Number(m[2]), 0, 0);
    const rep = repeat.value as ReminderRepeat;
    // A one-off in the past would never go off; a repeating one simply starts tomorrow.
    if (rep === "none" && d.getTime() <= Date.now()) {
      shell.setError(t("planner.reminders.past"));
      time.focus();
      return;
    }
    addBtn.disabled = true;
    const r = await attempt(() => PlannerBridge.reminderAdd(text, d.getTime(), rep));
    addBtn.disabled = false;
    if (!r.ok) {
      shell.setError(r.error);
      return;
    }
    shell.setError(null);
    title.value = "";
    title.dir = "";
    repeat.value = "none";
    time.value = nextHour(new Date());
    setDay(0);
    Sound.play("pop");
    title.focus();
  }

  async function run(p: () => Promise<unknown>) {
    const r = await attempt(p);
    if (!r.ok) shell.setError(r.error);
    renderedKey = "";
    sync();
  }

  function row(rem: PlannerReminder, now: number): HTMLElement {
    const nextAt = nextOccurrence(rem, now);
    const when = nextAt == null ? t("planner.reminders.off") : fmtWhen(nextAt, now);
    const sub = h("span", { class: `pl-due${nextAt != null && nextAt < now ? " late" : ""}` }, when);
    if (rem.repeat !== "none") {
      sub.append(h("span", { class: "pl-rep" }, icon("repeat", 11), t(`planner.repeat.${rem.repeat}`)));
    }
    const sw = h("button", {
      type: "button",
      class: `switch pl-switch${rem.enabled ? " on" : ""}`,
      role: "switch",
      "aria-checked": String(rem.enabled),
      "aria-label": t("planner.reminders.enable", { title: rem.title }),
      onclick: () => void run(() => PlannerBridge.reminderUpdate(rem.id, { enabled: !rem.enabled })),
    });
    sw.dataset.fk = `sw:${rem.id}`;
    const del = iconButton(icon("trash", 13), t("planner.reminders.delete", { title: rem.title }),
      () => void run(() => PlannerBridge.reminderDelete(rem.id)), "danger");
    del.dataset.fk = `del:${rem.id}`;
    return h("li", { class: `pl-row${rem.enabled ? "" : " off"}` },
      h("div", { class: "pl-row-main" }, h("span", { class: "pl-row-title", dir: "auto", text: rem.title }), sub),
      sw, del,
    );
  }

  function sync() {
    Planner.ensure();
    const data = Planner.data;
    const now = Date.now();
    // Relative times («۲۰ دقیقه دیگر») move on with the minute.
    const key = `${Planner.revision}|${Math.floor(now / 60_000)}|${Planner.error ?? ""}`;
    if (key === renderedKey) return;
    renderedKey = key;
    if (!data) {
      rerender(list, [h("li", { class: "pl-note", text: Planner.error ?? t("planner.loading") })]);
      return;
    }
    const rank = (r: PlannerReminder) => {
      const n = nextOccurrence(r, now);
      // Coming up first (soonest on top), then the past one-offs, then the ones switched off.
      return n == null ? [2, r.createdAt] : n < now ? [1, -n] : [0, n];
    };
    const sorted = [...data.reminders].sort((a, b) => {
      const ra = rank(a);
      const rb = rank(b);
      return ra[0] - rb[0] || ra[1] - rb[1];
    });
    rerender(list, sorted.length
      ? sorted.map((r) => row(r, now))
      : [h("li", { class: "pl-empty-li" }, emptyState(t("planner.reminders.empty")))]);
  }

  return {
    el: shell.el,
    sync,
    // The view re-renders on the minute while on screen (the island's frame loop runs then).
    tick(nowMs: number) {
      const minute = Math.floor(Date.now() / 60_000);
      if (minuteTimer !== minute) {
        minuteTimer = minute;
        if (nowMs > 0) sync();
      }
    },
    focus() {
      title.focus();
    },
  };
}
