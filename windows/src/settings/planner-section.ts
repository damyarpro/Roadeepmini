// Settings → Planner (focus timer, sounds, habit nudges, celebrations) and the
// eye-motion row of Settings → General. Values save through the window's usual
// path; the island applies them live on "settings-changed".

import "./planner-section.css";
import { isRtl, registerMessages, t } from "../core/i18n";
import { h } from "../views/dom";
import { DEFAULT_SETTINGS, EYE_MOTIONS, PLANNER_BOUNDS, type EyeMotion, type Settings } from "../core/state";
import { plannerSettingsEn } from "../core/locales/planner-settings-en";
import { plannerSettingsFa } from "../core/locales/planner-settings-fa";
import { sectionHead, settingRow, switchEl, helpDisclosure } from "./ui";

registerMessages(plannerSettingsEn, plannerSettingsFa);

export interface PlannerHost {
  settings(): Settings;
  save(): Promise<void>;
}

type CountKey = keyof typeof PLANNER_BOUNDS;

/** A whole number within the key's bounds; anything unreadable keeps `fallback`. */
export function clampCount(key: CountKey, raw: number, fallback: number = DEFAULT_SETTINGS[key]): number {
  if (!Number.isFinite(raw)) return fallback;
  const { min, max } = PLANNER_BOUNDS[key];
  return Math.min(max, Math.max(min, Math.round(raw)));
}

const reducedMotion = () => !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

/** Redraws what depends on settings changed elsewhere (the global sound switch). */
let refreshSoundNotes: (() => void) | null = null;

export function refreshPlannerSection() {
  refreshSoundNotes?.();
}

/** A minutes / rounds field; out-of-range values are pulled in and the reader is told. */
function countRow(host: PlannerHost, key: CountKey, label: string, unit: string): HTMLElement {
  const { min, max } = PLANNER_BOUNDS[key];
  const id = `pl-${key}`;
  const note = h("span", { class: "set-hint pl-note", id: `${id}-note`, role: "status", "aria-live": "polite" });
  const input = h("input", {
    id, type: "number", min: String(min), max: String(max), step: "1", inputmode: "numeric",
    class: "num", value: String(host.settings()[key]), "aria-describedby": `${id}-note`,
  }) as HTMLInputElement;
  input.addEventListener("change", () => {
    const current = host.settings()[key];
    const raw = input.valueAsNumber;
    const value = clampCount(key, raw, current);
    input.value = String(value);
    note.textContent = Number.isFinite(raw) && value !== raw ? t("plannerSettings.clamped", { min, max }) : "";
    if (value === current) return;
    host.settings()[key] = value;
    void host.save();
  });
  return settingRow({ label, forId: id, extra: note }, input, h("span", { class: "unit pl-unit", text: unit }));
}

type FlagKey = "focusSound" | "reminderSound" | "habitNudges" | "celebrations";

function switchRow(host: PlannerHost, key: FlagKey, label: string, hint: HTMLElement | string | null = null): HTMLElement {
  const toggle = switchEl(host.settings()[key], false, label, (on) => {
    host.settings()[key] = on;
    void host.save();
  });
  return settingRow({ label, hint: typeof hint === "string" ? hint : null, extra: typeof hint === "string" ? null : hint }, toggle);
}

export function plannerSection(host: PlannerHost): HTMLElement {
  // The two sound switches only matter while Roadeep's sound is on; say so when it isn't.
  const soundNotes = [0, 1].map(() => h("span", { class: "set-hint pl-note" }));
  refreshSoundNotes = () => {
    for (const note of soundNotes) note.textContent = host.settings().soundEnabled ? "" : t("plannerSettings.soundOff");
  };
  refreshSoundNotes();

  return h("section", { class: "sec", "aria-labelledby": "sec-planner-title" },
    sectionHead({ id: "sec-planner-title", icon: "planner", title: t("plannerSettings.title"), desc: t("plannerSettings.desc") }),
    h("div", { class: "card list", role: "group", "aria-labelledby": "pl-focus-head" },
      h("div", { class: "card-head" },
        h("h3", { id: "pl-focus-head", text: t("plannerSettings.focusHead") }),
        helpDisclosure(t("plannerSettings.focusHint"), `${t("settings.help")}: ${t("plannerSettings.focusHead")}`),
      ),
      countRow(host, "focusMinutes", t("plannerSettings.focusMinutes"), t("plannerSettings.unitMinutes")),
      countRow(host, "breakMinutes", t("plannerSettings.breakMinutes"), t("plannerSettings.unitMinutes")),
      countRow(host, "longBreakMinutes", t("plannerSettings.longBreakMinutes"), t("plannerSettings.unitMinutes")),
      countRow(host, "roundsBeforeLongBreak", t("plannerSettings.rounds"), t("plannerSettings.unitRounds")),
      switchRow(host, "focusSound", t("plannerSettings.focusSound"), soundNotes[0]),
    ),
    h("div", { class: "card list", role: "group", "aria-labelledby": "pl-day-head" },
      h("div", { class: "card-head" }, h("h3", { id: "pl-day-head", text: t("plannerSettings.dayHead") })),
      switchRow(host, "reminderSound", t("plannerSettings.reminderSound"), soundNotes[1]),
      switchRow(host, "habitNudges", t("plannerSettings.habitNudges"), t("plannerSettings.habitNudgesHint")),
      switchRow(host, "celebrations", t("plannerSettings.celebrations"), t("plannerSettings.celebrationsHint")),
    ),
  );
}

/**
 * Settings → General: how restless the character's eyes are. A radio group
 * with roving focus; the arrows follow the reading direction.
 */
export function eyeMotionRow(host: PlannerHost): HTMLElement {
  const explanation = helpDisclosure("", `${t("settings.help")}: ${t("plannerSettings.eye")}`);
  const hint = explanation.querySelector<HTMLElement>(".settings-help-content")!;
  hint.id = "gen-eye-hint";
  explanation.querySelector("button")!.setAttribute("aria-controls", hint.id);
  const reducedNote = h("span", {class:"set-hint", role:"status"});
  const group = h("div", {
    class: "segmented", role: "radiogroup", "aria-labelledby": "gen-eye-label", "aria-describedby": "gen-eye-hint",
  });
  const buttons = new Map<EyeMotion, HTMLButtonElement>();

  const current = (): EyeMotion => {
    const m = host.settings().eyeMotion;
    return EYE_MOTIONS.includes(m) ? m : "normal";
  };
  const sync = () => {
    const mode = current();
    for (const [m, btn] of buttons) {
      const on = m === mode;
      btn.classList.toggle("on", on);
      btn.setAttribute("aria-checked", on ? "true" : "false");
      btn.tabIndex = on ? 0 : -1;
    }
    const reduced = mode === "normal" && reducedMotion();
    hint.textContent = t(`plannerSettings.eye.${mode}Hint`);
    reducedNote.textContent = reduced ? t("plannerSettings.eye.reducedNote") : "";
  };
  const choose = (mode: EyeMotion) => {
    if (mode === current()) return;
    host.settings().eyeMotion = mode;
    sync();
    void host.save();
  };

  EYE_MOTIONS.forEach((mode, i) => {
    const btn = h("button", {
      type: "button", role: "radio", class: "seg", id: `gen-eye-${mode}`, text: t(`plannerSettings.eye.${mode}`),
    }) as HTMLButtonElement;
    btn.addEventListener("click", () => choose(mode));
    btn.addEventListener("keydown", (e) => {
      const forward = isRtl() ? "ArrowLeft" : "ArrowRight";
      const back = isRtl() ? "ArrowRight" : "ArrowLeft";
      const n = EYE_MOTIONS.length;
      const next =
        e.key === forward || e.key === "ArrowDown" ? (i + 1) % n
          : e.key === back || e.key === "ArrowUp" ? (i - 1 + n) % n
          : e.key === "Home" ? 0
          : e.key === "End" ? n - 1
          : -1;
      if (next < 0) return;
      e.preventDefault();
      const target = EYE_MOTIONS[next];
      buttons.get(target)?.focus();
      choose(target);
    });
    buttons.set(mode, btn);
    group.append(btn);
  });
  sync();

  return h("div", { class: "set-row stack" },
    h("div", { class: "set-text" },
      h("div", {class:"set-label-line"},
        h("span", { class: "set-label", id: "gen-eye-label", text: t("plannerSettings.eye") }),
        explanation,
      ),
      reducedNote,
    ),
    group,
  );
}
