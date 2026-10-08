// «امروز»: the date in full (Jalali in Persian), then what today holds — the
// focus timer, the tasks due or overdue, today's reminders and the habits
// with how many are done. Each section's heading opens its own view.

import { h } from "../dom";
import type { ViewActions, ViewHost } from "../views";
import type { IslandViewName } from "../../core/layout";
import { PlannerBridge, type PlannerTask } from "../../core/bridge-planner";
import { Sound } from "../../core/sound";
import { t } from "../../core/i18n";
import { openPalette } from "../palette";
import { Planner, attempt } from "./store";
import { focusRemaining, focusRunning, startFocus } from "./focus";
import { dayKeyOf, fmtClock, fmtCountdown, fmtDay, fmtLongDate, habitDueOn, num, occurrenceOn, sortOpenTasks } from "./time";
import {
  HABIT_COLORS, checkCircle, habitGlyph, icon, plannerShell, rerender, setChecked, type PlIcon,
} from "./ui";

/** Tasks listed here before «و N کار دیگر». */
const TASKS_SHOWN = 2;

export function buildToday(actions: ViewActions): ViewHost {
  const shell = plannerShell({ icon: "today", title: t("planner.today.title"), onMenu: () => openPalette(actions), cls: "pl-today" });
  const date = h("p", { class: "pl-date" });
  const sections = h("div", { class: "pl-scroll pl-sections" });
  shell.body.append(date, sections);

  let renderedKey = "";
  const focusLine = h("span", { class: "pl-sec-line" });
  let focusText = "";

  async function run(p: () => Promise<unknown>) {
    const r = await attempt(p);
    if (!r.ok) shell.setError(r.error);
  }

  function section(view: IslandViewName, ico: PlIcon, title: string, aside: string, ...content: (Node | null)[]): HTMLElement {
    const headBtn = h("button", { type: "button", class: "pl-sec-head", onclick: () => actions.setView(view) },
      h("span", { class: "pl-sec-ico", "aria-hidden": "true" }, icon(ico, 13)),
      h("span", { class: "pl-sec-title", text: title }),
      aside ? h("span", { class: "pl-sec-aside", text: aside }) : null,
    );
    headBtn.dataset.fk = `sec:${view}`;
    headBtn.setAttribute("aria-label", aside ? `${title} — ${aside}` : title);
    return h("section", { class: "pl-sec" }, headBtn, ...content.filter((c): c is Node => c != null));
  }

  function taskRow(task: PlannerTask, today: string): HTMLElement {
    const check = checkCircle(t("planner.tasks.markDone", { title: task.title }), () => {
      setChecked(check, true);
      Sound.play("pop");
      actions.celebrate?.("task");
      void run(() => PlannerBridge.taskUpdate(task.id, { done: true }));
    });
    check.dataset.fk = `check:${task.id}`;
    const late = task.due != null && task.due < today;
    return h("li", { class: "pl-row compact" }, check,
      h("div", { class: "pl-row-main" },
        h("span", { class: "pl-row-title", dir: "auto", text: task.title }),
        late ? h("span", { class: "pl-due late", text: t("planner.tasks.overdue", { day: fmtDay(task.due as string, today) }) }) : null,
      ));
  }

  function paintFocus() {
    const f = Planner.data?.focus;
    const left = focusRemaining(f, Date.now());
    const text = !f ? ""
      : f.phase === "paused" && left != null ? t("planner.today.focusPaused", { time: fmtCountdown(left) })
      : focusRunning(f) && left != null ? t(`planner.today.focusRunning.${f.phase}`, { time: fmtCountdown(left) })
      : f.roundsDoneToday > 0 ? t("planner.today.focusDone", { n: num(f.roundsDoneToday) })
      : t("planner.today.focusNone");
    if (text !== focusText) {
      focusText = text;
      focusLine.textContent = text;
    }
  }

  function sync() {
    Planner.ensure();
    const data = Planner.data;
    const now = Date.now();
    const today = dayKeyOf(new Date(now));
    paintFocus();
    const key = `${Planner.revision}|${today}|${Math.floor(now / 60_000)}|${Planner.error ?? ""}`;
    if (key === renderedKey) return;
    renderedKey = key;
    date.textContent = fmtLongDate(today);
    if (!data) {
      rerender(sections, [h("p", { class: "pl-note", text: Planner.error ?? t("planner.loading") })]);
      return;
    }

    // Focus
    const f = data.focus;
    // One line: the timer's title (opens it), its state and, when idle, a start.
    const focusHead = h("button", { type: "button", class: "pl-sec-head inline", onclick: () => actions.setView("focus") },
      h("span", { class: "pl-sec-ico", "aria-hidden": "true" }, icon("focus", 13)),
      h("span", { class: "pl-sec-title", text: t("planner.focus.title") }));
    focusHead.dataset.fk = "sec:focus";
    const focusSec = h("section", { class: "pl-sec" },
      h("div", { class: "pl-sec-row tight" }, focusHead, focusLine, h("div", { class: "grow" }),
        f.phase === "idle"
          ? h("button", { type: "button", class: "pl-chip", onclick: async () => {
              const err = await startFocus();
              shell.setError(err);
              if (!err) actions.setView("focus");
            } }, icon("focus", 12), h("span", { text: t("planner.today.startFocus") }))
          : null,
      ));

    // Tasks due today or earlier
    const due = sortOpenTasks(data.tasks).filter((x) => x.due != null && x.due <= today);
    const taskList = h("ul", { class: "pl-list" }, ...due.slice(0, TASKS_SHOWN).map((x) => taskRow(x, today)));
    const more = due.length - TASKS_SHOWN;
    const tasksSec = section("tasks", "tasks", t("planner.today.tasks"), due.length ? num(due.length) : "",
      due.length ? taskList : h("p", { class: "pl-sec-empty", text: t("planner.today.noTasks") }),
      more > 0 ? h("button", { type: "button", class: "pl-link", text: t("planner.today.moreTasks", { n: num(more) }), onclick: () => actions.setView("tasks") }) : null,
    );

    // Today's reminders, in time order
    const rems = data.reminders
      .map((r) => ({ r, at: occurrenceOn(r, today) }))
      .filter((x): x is { r: typeof x.r; at: number } => x.at != null)
      .sort((a, b) => a.at - b.at);
    const remList = h("ul", { class: "pl-times" }, ...rems.map(({ r, at }) =>
      h("li", { class: at < now ? "past" : "" },
        h("span", { class: "pl-time-at", text: fmtClock(at) }),
        h("span", { class: "pl-row-title", dir: "auto", text: r.title }))));
    const remSec = section("reminders", "reminders", t("planner.today.reminders"), rems.length ? num(rems.length) : "",
      rems.length ? remList : h("p", { class: "pl-sec-empty", text: t("planner.today.noReminders") }));

    // Habits meant for today
    const habits = data.habits.filter((x) => habitDueOn(x, today));
    const doneCount = habits.filter((x) => x.log.includes(today)).length;
    const chips = h("div", { class: "pl-habit-chips" }, ...habits.map((hb) => {
      const done = hb.log.includes(today);
      const b = h("button", {
        type: "button", class: `pl-habit-chip${done ? " on" : ""}`, role: "checkbox", "aria-checked": String(done),
        "aria-label": t(done ? "planner.habits.undo" : "planner.habits.check", { title: hb.title }),
        onclick: () => {
          if (!done) {
            Sound.play("pop");
            actions.celebrate?.("task");
          }
          void run(() => PlannerBridge.habitCheck(hb.id, today, !done));
        },
      }, habitGlyph(hb.icon, 12), h("span", { dir: "auto", text: hb.title }));
      b.style.setProperty("--pl-accent", HABIT_COLORS[hb.icon] ?? HABIT_COLORS.custom);
      b.dataset.fk = `habit:${hb.id}`;
      return b;
    }));
    const habitSec = section("habits", "habits", t("planner.today.habits"),
      habits.length ? t("planner.today.habitsProgress", { done: num(doneCount), total: num(habits.length) }) : "",
      habits.length ? chips : h("p", { class: "pl-sec-empty", text: t("planner.today.noHabits") }));

    rerender(sections, [focusSec, tasksSec, remSec, habitSec]);
  }

  return {
    el: shell.el,
    sync,
    tick() {
      paintFocus();
    },
    focus() {
      sections.querySelector<HTMLElement>("button")?.focus();
    },
  };
}
