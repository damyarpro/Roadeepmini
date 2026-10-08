// «کارها»: a field to add (Enter), «امروز» / «فردا» for its date, the open
// tasks in date order (overdue first), a check that completes with a small
// celebration, a ⋯ menu per task (move the date, delete) and the done ones
// folded away underneath.

import { h } from "../dom";
import type { ViewActions, ViewHost } from "../views";
import { PLANNER_LIMITS, PlannerBridge, type PlannerTask } from "../../core/bridge-planner";
import { Sound } from "../../core/sound";
import { textDirection, t } from "../../core/i18n";
import { openPalette } from "../palette";
import { Planner, attempt } from "./store";
import { addDays, dayKeyOf, fmtDay, num, sortOpenTasks } from "./time";
import {
  checkCircle, emptyState, icon, iconButton, limitField, plannerShell, rerender, setChecked, setPressed, solid,
  stopEscape, toggleChip,
} from "./ui";

/** Done tasks listed under the fold; older ones stay in the file, not on screen. */
const DONE_SHOWN = 30;

export function buildTasks(actions: ViewActions): ViewHost {
  const shell = plannerShell({ icon: "tasks", title: t("planner.tasks.title"), onMenu: () => openPalette(actions) });

  const input = h("input", {
    type: "text",
    class: "pl-input",
    placeholder: t("planner.tasks.placeholder"),
    "aria-label": t("planner.tasks.new"),
    spellcheck: "false",
    autocomplete: "off",
  }) as HTMLInputElement;
  limitField(input, PLANNER_LIMITS.taskTitle);
  input.addEventListener("input", () => {
    input.dir = textDirection(input.value) ?? "";
  });

  /** Offset in days of the new task's date; null = no date. */
  let newDue: 0 | 1 | null = null;
  const todayChip = toggleChip(t("planner.today"), () => setNewDue(newDue === 0 ? null : 0));
  const tomorrowChip = toggleChip(t("planner.tomorrow"), () => setNewDue(newDue === 1 ? null : 1));
  todayChip.setAttribute("aria-label", t("planner.tasks.dueTodayA11y"));
  tomorrowChip.setAttribute("aria-label", t("planner.tasks.dueTomorrowA11y"));
  function setNewDue(v: 0 | 1 | null) {
    newDue = v;
    setPressed(todayChip, v === 0);
    setPressed(tomorrowChip, v === 1);
    input.focus();
  }
  const addBtn = iconButton(icon("plus", 14), t("planner.tasks.add"), () => void add(), "pl-add");

  const addRow = h("div", { class: "pl-add-row" }, input, todayChip, tomorrowChip, addBtn);
  const list = h("ul", { class: "pl-list", "aria-label": t("planner.tasks.open") });
  const doneToggle = h("button", { type: "button", class: "pl-fold", "aria-expanded": "false" });
  const doneList = h("ul", { class: "pl-list pl-done", "aria-label": t("planner.tasks.done"), hidden: true });
  const scroll = h("div", { class: "pl-scroll" }, list, doneToggle, doneList);
  shell.body.append(addRow, scroll);

  let doneOpen = false;
  /** The task whose ⋯ menu is open. */
  let menuFor: string | null = null;
  /** Checked a moment ago: shown done while Rust catches up. */
  const settling = new Set<string>();
  let renderedKey = "";

  doneToggle.addEventListener("click", () => {
    doneOpen = !doneOpen;
    renderedKey = "";
    sync();
  });

  stopEscape(shell.el, () => {
    if (menuFor) {
      const id = menuFor;
      menuFor = null;
      renderedKey = "";
      sync();
      list.querySelector<HTMLElement>(`[data-fk="more:${id}"]`)?.focus();
      return true;
    }
    return false;
  });

  input.addEventListener("keydown", (e) => {
    const k = e as KeyboardEvent;
    if (k.key === "Enter" && !k.isComposing) {
      e.preventDefault();
      void add();
    }
    if (k.key !== "Escape") e.stopPropagation();
  });

  async function add() {
    const title = input.value.trim();
    if (!title) {
      input.focus();
      return;
    }
    const due = newDue == null ? null : addDays(dayKeyOf(new Date()), newDue);
    addBtn.disabled = true;
    const r = await attempt(() => PlannerBridge.taskAdd(title, due));
    addBtn.disabled = false;
    if (!r.ok) {
      shell.setError(r.error);
      return;
    }
    shell.setError(null);
    input.value = "";
    input.dir = "";
    Sound.play("pop");
    input.focus();
  }

  async function toggleDone(task: PlannerTask, done: boolean) {
    if (done) {
      settling.add(task.id);
      renderedKey = "";
      sync();
      Sound.play("pop");
      actions.celebrate?.("task");
    }
    const r = await attempt(() => PlannerBridge.taskUpdate(task.id, { done }));
    settling.delete(task.id);
    if (!r.ok) shell.setError(r.error);
    renderedKey = "";
    sync();
  }

  async function setDue(task: PlannerTask, due: string | null) {
    menuFor = null;
    const r = await attempt(() => PlannerBridge.taskUpdate(task.id, { due }));
    if (!r.ok) shell.setError(r.error);
    renderedKey = "";
    sync();
  }

  async function remove(task: PlannerTask) {
    menuFor = null;
    const r = await attempt(() => PlannerBridge.taskDelete(task.id));
    if (!r.ok) shell.setError(r.error);
    renderedKey = "";
    sync();
    input.focus();
  }

  function row(task: PlannerTask, today: string): HTMLElement {
    const done = task.done || settling.has(task.id);
    const check = checkCircle(t(done ? "planner.tasks.markOpen" : "planner.tasks.markDone", { title: task.title }),
      () => void toggleDone(task, !done));
    check.dataset.fk = `check:${task.id}`;
    setChecked(check, done);
    const title = h("span", { class: "pl-row-title", dir: "auto", text: task.title });
    const main = h("div", { class: "pl-row-main" }, title);
    if (task.due && !task.done) {
      const overdue = task.due < today;
      const tag = h("span", {
        class: `pl-due${overdue ? " late" : task.due === today ? " today" : ""}`,
        text: overdue ? t("planner.tasks.overdue", { day: fmtDay(task.due, today) }) : fmtDay(task.due, today),
      });
      main.append(tag);
    }
    const more = iconButton(solid("more", 13), t("planner.tasks.more", { title: task.title }), () => {
      menuFor = menuFor === task.id ? null : task.id;
      renderedKey = "";
      sync();
      if (menuFor) list.querySelector<HTMLElement>(`[data-fk="due0:${task.id}"]`)?.focus();
    }, "pl-more");
    more.dataset.fk = `more:${task.id}`;
    more.setAttribute("aria-expanded", String(menuFor === task.id));
    const li = h("li", { class: `pl-row${done ? " done" : ""}${settling.has(task.id) ? " leaving" : ""}` }, check, main, more);
    if (menuFor === task.id) {
      const opt = (fk: string, label: string, run: () => void, cls = "") => {
        const b = h("button", { type: "button", class: `pl-chip ${cls}`.trim(), text: label, onclick: () => run() });
        b.dataset.fk = `${fk}:${task.id}`;
        return b;
      };
      li.append(h("div", { class: "pl-row-menu", role: "group", "aria-label": t("planner.tasks.more", { title: task.title }) },
        opt("due0", t("planner.today"), () => void setDue(task, today)),
        opt("due1", t("planner.tomorrow"), () => void setDue(task, addDays(today, 1))),
        opt("dueNone", t("planner.tasks.noDate"), () => void setDue(task, null)),
        h("div", { class: "grow" }),
        opt("del", t("planner.delete"), () => void remove(task), "danger"),
      ));
    }
    return li;
  }

  function sync() {
    Planner.ensure();
    const data = Planner.data;
    const today = dayKeyOf(new Date());
    const key = `${Planner.revision}|${today}|${doneOpen}|${menuFor}|${[...settling].join(",")}|${Planner.error ?? ""}`;
    if (key === renderedKey) return;
    renderedKey = key;

    if (!data) {
      rerender(list, [h("li", { class: "pl-note", text: Planner.error ?? t("planner.loading") })]);
      doneToggle.hidden = true;
      doneList.hidden = true;
      return;
    }
    const open = sortOpenTasks(data.tasks.map((x) => (settling.has(x.id) ? { ...x, done: false } : x)));
    const done = data.tasks
      .filter((x) => x.done && !settling.has(x.id))
      .sort((a, b) => (b.doneAt ?? 0) - (a.doneAt ?? 0));

    rerender(list, open.length
      ? open.map((x) => row(x, today))
      : [h("li", { class: "pl-empty-li" }, emptyState(t(done.length ? "planner.tasks.allDone" : "planner.tasks.empty")))]);

    doneToggle.hidden = done.length === 0;
    doneToggle.setAttribute("aria-expanded", String(doneOpen));
    doneToggle.replaceChildren(
      h("span", { text: t("planner.tasks.doneCount", { n: num(done.length) }) }),
      icon("chevronDown", 12),
    );
    doneToggle.classList.toggle("open", doneOpen);
    doneList.hidden = !doneOpen || done.length === 0;
    if (doneOpen) rerender(doneList, done.slice(0, DONE_SHOWN).map((x) => row(x, today)));
  }

  return {
    el: shell.el,
    sync,
    focus() {
      input.focus();
    },
  };
}
