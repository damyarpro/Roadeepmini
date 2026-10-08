// «عادت‌ها»: each habit with its streak, the last seven days as dots and
// today's check; «عادت تازه» opens a small form (name, icon, which days, an
// optional nudge every N minutes between two times). With none yet, three
// suggestions add in one tap.

import { h } from "../dom";
import type { ViewActions, ViewHost } from "../views";
import {
  HABIT_ICONS, PLANNER_LIMITS, PlannerBridge, type HabitDraft, type HabitIcon, type PlannerHabit,
} from "../../core/bridge-planner";
import { Sound } from "../../core/sound";
import { getLanguage, textDirection, t } from "../../core/i18n";
import { openPalette } from "../palette";
import { Planner, attempt } from "./store";
import { addDays, dayKeyOf, habitDueOn, habitStreak, num, weekdayName } from "./time";
import {
  HABIT_COLORS, checkCircle, emptyState, habitGlyph, icon, iconButton, limitField, plannerShell, rerender,
  setChecked, stopEscape, timePicker,
} from "./ui";

/** Nudge choices in minutes; 0 = no nudge. */
const NUDGE_CHOICES = [0, 30, 45, 60, 90, 120] as const;

/** One-tap starters for an empty list (wellbeing: water, stretching, posture). */
export function habitPresets(): HabitDraft[] {
  return [
    { title: t("planner.habits.preset.water"), icon: "water", days: "daily", nudge: { everyMinutes: 60, from: "09:00", to: "18:00" } },
    { title: t("planner.habits.preset.stretch"), icon: "stretch", days: "weekdays", nudge: { everyMinutes: 90, from: "10:00", to: "17:00" } },
    { title: t("planner.habits.preset.posture"), icon: "posture", days: "weekdays", nudge: { everyMinutes: 45, from: "09:00", to: "17:00" } },
  ];
}

export function buildHabits(actions: ViewActions): ViewHost {
  const shell = plannerShell({ icon: "habits", title: t("planner.habits.title"), onMenu: () => openPalette(actions) });
  const list = h("ul", { class: "pl-list", "aria-label": t("planner.habits.title") });
  const scroll = h("div", { class: "pl-scroll" }, list);

  // ── The add form ──
  const name = h("input", {
    type: "text", class: "pl-input", placeholder: t("planner.habits.namePlaceholder"),
    "aria-label": t("planner.habits.name"), spellcheck: "false", autocomplete: "off",
  }) as HTMLInputElement;
  limitField(name, PLANNER_LIMITS.habitTitle);
  name.addEventListener("input", () => {
    name.dir = textDirection(name.value) ?? "";
  });
  let chosenIcon: HabitIcon = "water";
  const iconRadios = HABIT_ICONS.map((kind) => {
    const b = h("button", {
      type: "button", role: "radio", class: "pl-icon-pick", "aria-label": t(`planner.habits.icon.${kind}`),
      title: t(`planner.habits.icon.${kind}`), onclick: () => pickIcon(kind),
    }, habitGlyph(kind, 14));
    b.style.setProperty("--pl-accent", HABIT_COLORS[kind]);
    return b;
  });
  const iconGroup = h("div", { class: "pl-icon-row", role: "radiogroup", "aria-label": t("planner.habits.iconLabel") }, ...iconRadios);
  iconGroup.addEventListener("keydown", (e) => {
    const i = iconRadios.indexOf(e.target as HTMLButtonElement);
    if (i < 0) return;
    const rtl = getLanguage() === "fa";
    let n: number;
    switch (e.key) {
      case "ArrowRight": n = i + (rtl ? -1 : 1); break;
      case "ArrowLeft": n = i + (rtl ? 1 : -1); break;
      case "ArrowDown": n = i + 1; break;
      case "ArrowUp": n = i - 1; break;
      default: return;
    }
    e.preventDefault();
    n = (n + iconRadios.length) % iconRadios.length;
    pickIcon(HABIT_ICONS[n]);
    iconRadios[n].focus();
  });
  function pickIcon(kind: HabitIcon) {
    chosenIcon = kind;
    iconRadios.forEach((b, i) => {
      const on = HABIT_ICONS[i] === kind;
      b.classList.toggle("on", on);
      b.setAttribute("aria-checked", String(on));
      b.tabIndex = on ? 0 : -1;
    });
  }
  pickIcon("water");

  const days = h("select", { class: "pl-select", "aria-label": t("planner.habits.days") },
    h("option", { value: "daily", text: t("planner.habits.daily") }),
    h("option", { value: "weekdays", text: t("planner.habits.weekdays") }),
  ) as HTMLSelectElement;
  const nudge = h("select", { class: "pl-select", "aria-label": t("planner.habits.nudge") },
    ...NUDGE_CHOICES.map((m) => h("option", {
      value: String(m), text: m === 0 ? t("planner.habits.nudgeOff") : t("planner.habits.nudgeEvery", { n: m }),
    })),
  ) as HTMLSelectElement;
  const from = timePicker(t("planner.habits.fromA11y"), "09:00");
  const to = timePicker(t("planner.habits.toA11y"), "18:00");
  const window_ = h("span", { class: "pl-window" }, h("span", { class: "pl-meta", text: t("planner.habits.from") }), from.el,
    h("span", { class: "pl-meta", text: t("planner.habits.to") }), to.el);
  const syncWindow = () => {
    window_.hidden = nudge.value === "0";
  };
  nudge.addEventListener("change", syncWindow);
  for (const s of [days, nudge]) s.addEventListener("keydown", (e) => e.stopPropagation());

  // Same add pattern as the other views: a field and the round «+»; the
  // habit's icon, days and nudge fold out under «جزئیات» (defaults otherwise).
  const saveBtn = iconButton(icon("plus", 14), t("planner.habits.new"), () => void save(), "pl-add");
  const moreBtn = h("button", {
    type: "button", class: "pl-chip", "aria-expanded": "false", onclick: () => (formOpen ? closeForm() : openForm()),
  }, h("span", { text: t("planner.habits.details") }), icon("chevronDown", 11));
  const options = h("div", { class: "pl-form pl-habit-form", hidden: true, role: "group", "aria-label": t("planner.habits.details") },
    iconGroup,
    h("div", { class: "pl-add-row sub" }, days, nudge, window_),
  );
  const form = h("div", { class: "pl-form" }, h("div", { class: "pl-add-row" }, name, moreBtn, saveBtn), options);
  name.addEventListener("keydown", (e) => {
    const k = e as KeyboardEvent;
    if (k.key === "Enter" && !k.isComposing) {
      e.preventDefault();
      void save();
    }
    if (k.key !== "Escape") e.stopPropagation();
  });

  shell.body.append(form, scroll);

  let formOpen = false;
  let renderedKey = "";

  stopEscape(shell.el, () => {
    if (!formOpen) return false;
    closeForm();
    return true;
  });

  function openForm() {
    formOpen = true;
    options.hidden = false;
    moreBtn.setAttribute("aria-expanded", "true");
    moreBtn.classList.add("on");
    syncWindow();
    iconRadios.find((b) => b.tabIndex === 0)?.focus();
  }

  function closeForm() {
    formOpen = false;
    options.hidden = true;
    moreBtn.setAttribute("aria-expanded", "false");
    moreBtn.classList.remove("on");
    moreBtn.focus();
  }

  function resetForm() {
    name.value = "";
    name.dir = "";
    pickIcon("custom");
    days.value = "daily";
    nudge.value = "0";
    from.value = "09:00";
    to.value = "18:00";
    syncWindow();
  }
  resetForm();

  async function save() {
    const title = name.value.trim();
    if (!title) {
      name.focus();
      return;
    }
    const every = Number(nudge.value);
    if (every > 0 && from.value >= to.value) {
      shell.setError(t("planner.habits.badWindow"));
      from.focus();
      return;
    }
    const draft: HabitDraft = {
      title,
      icon: chosenIcon,
      days: days.value === "weekdays" ? "weekdays" : "daily",
      nudge: every > 0 ? { everyMinutes: every, from: from.value, to: to.value } : null,
    };
    saveBtn.disabled = true;
    const r = await attempt(() => PlannerBridge.habitAdd(draft));
    saveBtn.disabled = false;
    if (!r.ok) {
      shell.setError(r.error);
      return;
    }
    shell.setError(null);
    Sound.play("pop");
    resetForm();
    if (formOpen) closeForm();
    name.focus();
  }

  async function run(p: () => Promise<unknown>) {
    const r = await attempt(p);
    if (!r.ok) shell.setError(r.error);
    renderedKey = "";
    sync();
  }

  function row(habit: PlannerHabit, today: string): HTMLElement {
    const color = HABIT_COLORS[habit.icon] ?? HABIT_COLORS.custom;
    const doneToday = habit.log.includes(today);
    const check = checkCircle(t(doneToday ? "planner.habits.undo" : "planner.habits.check", { title: habit.title }), () => {
      if (!doneToday) {
        Sound.play("pop");
        actions.celebrate?.("task");
      }
      void run(() => PlannerBridge.habitCheck(habit.id, today, !doneToday));
    }, color);
    check.dataset.fk = `check:${habit.id}`;
    setChecked(check, doneToday);

    const streak = habitStreak(habit, today);
    const streakText = streak > 1
      ? t("planner.habits.streak", { n: num(streak) })
      : !habitDueOn(habit, today) ? t("planner.habits.restDay")
      : habit.days === "weekdays" ? t("planner.habits.weekdays") : t("planner.habits.daily");

    // The last seven days, oldest first along the reading direction.
    const dots = h("span", { class: "pl-dots", role: "img" });
    const doneDays: string[] = [];
    for (let i = 6; i >= 0; i--) {
      const key = addDays(today, -i);
      const done = habit.log.includes(key);
      if (done) doneDays.push(weekdayName(key));
      const dot = h("i", { class: `${done ? "on" : ""}${habitDueOn(habit, key) ? "" : " rest"}${i === 0 ? " today" : ""}` });
      dots.append(dot);
    }
    dots.style.setProperty("--pl-accent", color);
    dots.setAttribute("aria-label", t("planner.habits.week", { n: num(doneDays.length) }));

    const glyph = h("span", { class: "pl-habit-ico", "aria-hidden": "true" }, habitGlyph(habit.icon, 14));
    glyph.style.setProperty("--pl-accent", color);
    const del = iconButton(icon("trash", 13), t("planner.habits.delete", { title: habit.title }),
      () => void run(() => PlannerBridge.habitDelete(habit.id)), "danger pl-hover-only");
    del.dataset.fk = `del:${habit.id}`;
    return h("li", { class: `pl-row${doneToday ? " done-soft" : ""}` },
      check,
      glyph,
      h("div", { class: "pl-row-main" },
        h("span", { class: "pl-row-title", dir: "auto", text: habit.title }),
        h("span", { class: `pl-due${streak > 1 ? " streak" : ""}`, text: streakText }),
      ),
      dots, del,
    );
  }

  function presets(): HTMLElement {
    const buttons = habitPresets().map((p) => {
      const b = h("button", { type: "button", class: "pl-preset", onclick: () => void run(() => PlannerBridge.habitAdd(p)) },
        h("span", { class: "pl-habit-ico", "aria-hidden": "true" }, habitGlyph(p.icon, 14)),
        h("span", { text: p.title }));
      (b.firstElementChild as HTMLElement).style.setProperty("--pl-accent", HABIT_COLORS[p.icon]);
      b.setAttribute("aria-label", t("planner.habits.addPreset", { title: p.title }));
      return b;
    });
    return emptyState(t("planner.habits.empty"), ...buttons);
  }

  function sync() {
    Planner.ensure();
    const data = Planner.data;
    const today = dayKeyOf(new Date());
    const key = `${Planner.revision}|${today}|${Planner.error ?? ""}`;
    if (key === renderedKey) return;
    renderedKey = key;
    if (!data) {
      rerender(list, [h("li", { class: "pl-note", text: Planner.error ?? t("planner.loading") })]);
      return;
    }
    rerender(list, data.habits.length
      ? data.habits.map((x) => row(x, today))
      : [h("li", { class: "pl-empty-li" }, presets())]);
  }

  return {
    el: shell.el,
    sync,
    focus() {
      name.focus();
    },
  };
}
