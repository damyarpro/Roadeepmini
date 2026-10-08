// «این هفته»: Saturday to Friday as seven day buttons (weekday, day of month,
// how many tasks and reminders), today marked; the chosen day's tasks and
// reminders are listed underneath. ‹ › step a week back or ahead.

import { h, svg } from "../dom";
import { ICONS } from "../icons";
import type { ViewActions, ViewHost } from "../views";
import { PlannerBridge, type PlannerData } from "../../core/bridge-planner";
import { Sound } from "../../core/sound";
import { getLanguage, t } from "../../core/i18n";
import { weekStart } from "../../core/jalali";
import { openPalette } from "../palette";
import { Planner, attempt } from "./store";
import {
  addDays, dayKeyOf, dayOfMonth, fmtClock, fmtDate, fmtLongDate, num, occurrenceOn, weekdayName,
} from "./time";
import { checkCircle, iconButton, plannerShell, rerender, setChecked } from "./ui";

interface DayItems {
  tasks: PlannerData["tasks"];
  reminders: { title: string; at: number; id: string }[];
}

function itemsOn(data: PlannerData, key: string): DayItems {
  return {
    tasks: data.tasks.filter((x) => x.due === key).sort((a, b) => Number(a.done) - Number(b.done) || a.createdAt - b.createdAt),
    reminders: data.reminders
      .map((r) => ({ title: r.title, id: r.id, at: occurrenceOn(r, key) }))
      .filter((x): x is { title: string; id: string; at: number } => x.at != null)
      .sort((a, b) => a.at - b.at),
  };
}

export function buildWeek(actions: ViewActions): ViewHost {
  const shell = plannerShell({ icon: "week", title: t("planner.week.title"), onMenu: () => openPalette(actions), cls: "pl-week" });
  /** Weeks from this one (negative = earlier). */
  let offset = 0;
  let selected: string | null = null;
  const prev = iconButton(svg(ICONS.chevronLeft, 12, { stroke: 2 }), t("planner.week.prev"), () => moveWeek(-1), "pl-flip");
  const next = iconButton(svg(ICONS.chevronRight, 12, { stroke: 2 }), t("planner.week.next"), () => moveWeek(1), "pl-flip");
  const thisWeek = h("button", { type: "button", class: "pl-chip", text: t("planner.week.thisWeek"), onclick: () => moveWeek(-offset) });
  shell.tools.append(thisWeek, prev, next);

  const strip = h("div", { class: "wk-strip", role: "group", "aria-label": t("planner.week.days") });
  const dayTitle = h("p", { class: "wk-day-title" });
  const dayList = h("ul", { class: "pl-list wk-day" });
  shell.body.append(strip, dayTitle, h("div", { class: "pl-scroll" }, dayList));

  let renderedKey = "";

  function moveWeek(by: number) {
    offset += by;
    const today = dayKeyOf(new Date());
    selected = offset === 0 ? today : addDays(weekStart(today), offset * 7);
    actions.blip();
    sync();
  }

  strip.addEventListener("keydown", (e) => {
    const buttons = [...strip.querySelectorAll<HTMLButtonElement>("button")];
    const i = buttons.indexOf(e.target as HTMLButtonElement);
    if (i < 0) return;
    const rtl = getLanguage() === "fa";
    let n: number;
    switch (e.key) {
      case "ArrowRight": n = i + (rtl ? -1 : 1); break;
      case "ArrowLeft": n = i + (rtl ? 1 : -1); break;
      case "Home": n = 0; break;
      case "End": n = buttons.length - 1; break;
      default: return;
    }
    e.preventDefault();
    buttons[Math.max(0, Math.min(buttons.length - 1, n))].focus();
  });

  function sync() {
    Planner.ensure();
    const data = Planner.data;
    const today = dayKeyOf(new Date());
    const start = addDays(weekStart(today), offset * 7);
    if (!selected || selected < start || selected > addDays(start, 6)) selected = offset === 0 ? today : start;
    const key = `${Planner.revision}|${today}|${offset}|${selected}|${Planner.error ?? ""}`;
    if (key === renderedKey) return;
    renderedKey = key;

    shell.setTitle(offset === 0 ? t("planner.week.title")
      : t("planner.week.range", { from: fmtDate(start, today), to: fmtDate(addDays(start, 6), today) }));
    thisWeek.hidden = offset === 0;

    if (!data) {
      rerender(dayList, [h("li", { class: "pl-note", text: Planner.error ?? t("planner.loading") })]);
      return;
    }

    const days: HTMLElement[] = [];
    for (let i = 0; i < 7; i++) {
      const day = addDays(start, i);
      const items = itemsOn(data, day);
      const open = items.tasks.filter((x) => !x.done).length;
      const counts = h("span", { class: "wk-counts", "aria-hidden": "true" });
      if (open) counts.append(h("i", { class: "wk-task", text: num(open) }));
      if (items.reminders.length) counts.append(h("i", { class: "wk-rem", text: num(items.reminders.length) }));
      const parts = [fmtLongDate(day)];
      if (open) parts.push(t("planner.week.tasksN", { n: num(open) }));
      if (items.reminders.length) parts.push(t("planner.week.remindersN", { n: num(items.reminders.length) }));
      const b = h("button", {
        type: "button",
        class: `wk-day-btn${day === today ? " today" : ""}${day === selected ? " sel" : ""}${day < today ? " past" : ""}`,
        "aria-pressed": String(day === selected),
        "aria-label": parts.join("، "),
        onclick: () => {
          selected = day;
          sync();
        },
      },
        h("span", { class: "wk-name", text: weekdayName(day, true) }),
        h("span", { class: "wk-num", text: dayOfMonth(day) }),
        counts,
      );
      if (day === today) b.setAttribute("aria-current", "date");
      b.dataset.fk = `day:${day}`;
      days.push(b);
    }
    rerender(strip, days);

    const sel = selected as string;
    dayTitle.textContent = sel === today ? `${t("planner.today")} · ${fmtLongDate(sel)}` : fmtLongDate(sel);
    const items = itemsOn(data, sel);
    const rows: HTMLElement[] = [];
    for (const task of items.tasks) {
      const check = checkCircle(t(task.done ? "planner.tasks.markOpen" : "planner.tasks.markDone", { title: task.title }), async () => {
        setChecked(check, !task.done);
        if (!task.done) {
          Sound.play("pop");
          actions.celebrate?.("task");
        }
        const r = await attempt(() => PlannerBridge.taskUpdate(task.id, { done: !task.done }));
        if (!r.ok) shell.setError(r.error);
      });
      setChecked(check, task.done);
      check.dataset.fk = `check:${task.id}`;
      rows.push(h("li", { class: `pl-row compact${task.done ? " done" : ""}` }, check,
        h("div", { class: "pl-row-main" }, h("span", { class: "pl-row-title", dir: "auto", text: task.title }))));
    }
    for (const r of items.reminders) {
      rows.push(h("li", { class: "pl-row compact" },
        h("span", { class: "pl-time-at", text: fmtClock(r.at) }),
        h("div", { class: "pl-row-main" }, h("span", { class: "pl-row-title", dir: "auto", text: r.title }))));
    }
    rerender(dayList, rows.length ? rows : [h("li", { class: "pl-sec-empty", text: t("planner.week.free") })]);
  }

  return {
    el: shell.el,
    sync,
    focus() {
      strip.querySelector<HTMLElement>(".sel")?.focus();
    },
  };
}
