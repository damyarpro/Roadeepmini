// Pieces every planner view shares: the card with its title row, icon
// buttons, the round check, toggle chips, the inline error line. Stroked icons
// on the 24 grid (draw with `icon(name)`), in the island's thin line style.

import { h, svg } from "../dom";
import { ICONS } from "../icons";
import { formatNumber, registerMessages, t } from "../../core/i18n";
import type { HabitIcon } from "../../core/bridge-planner";
import { plannerEn } from "../../core/locales/planner-en";
import { plannerFa } from "../../core/locales/planner-fa";
import "../planner.css";

registerMessages(plannerEn, plannerFa);

export const PL_ICONS = {
  menu: "M4.5 4.5h6v6h-6z M13.5 4.5h6v6h-6z M4.5 13.5h6v6h-6z M13.5 13.5h6v6h-6z",
  chat: "M5 5h14a1.5 1.5 0 0 1 1.5 1.5v9A1.5 1.5 0 0 1 19 17h-8l-4.5 3.5V17H5a1.5 1.5 0 0 1-1.5-1.5v-9A1.5 1.5 0 0 1 5 5z",
  home: "M4 11 12 4l8 7 M6 9.5V20h12V9.5 M10 20v-5h4v5",
  tasks: "M10 6.5h10 M10 12h10 M10 17.5h10 M4 6.5l1.3 1.3L7.8 5.2 M4 12l1.3 1.3 2.5-2.6 M4 17.5l1.3 1.3 2.5-2.6",
  notes: "M6 3.5h8.5l4 4V20.5H6z M14 3.5V8h4.5 M9 12h6.5 M9 15.5h4.5",
  reminders: "M6.5 16v-5a5.5 5.5 0 0 1 11 0v5l1.5 2h-14z M10 20.5a2 2 0 0 0 4 0",
  habits: "M12 20.5v-8 M12 12.5c0-4.2 2.9-6.7 7-6.7 0 4.2-2.9 6.7-7 6.7z M12 15c0-3.2-2.4-5.3-6-5.3 0 3.2 2.4 5.3 6 5.3z",
  today: "M12 8.2a3.8 3.8 0 1 0 0 7.6 3.8 3.8 0 0 0 0-7.6z M12 2.8v2 M12 19.2v2 M2.8 12h2 M19.2 12h2 M5.5 5.5l1.4 1.4 M17.1 17.1l1.4 1.4 M5.5 18.5l1.4-1.4 M17.1 6.9l1.4-1.4",
  week: "M4.5 6h15v14h-15z M4.5 10h15 M8.5 3.5v4 M15.5 3.5v4 M8 13.5h2 M11 13.5h2 M14 13.5h2 M8 16.5h2 M11 16.5h2",
  focus: "M12 6.5a7 7 0 1 0 0 14 7 7 0 0 0 0-14z M12 10v3.5l2.3 1.4 M9.5 3h5",
  settings: "M4 7h9 M17 7h3 M4 17h3 M11 17h9 M15 5v4 M9 15v4",
  feedback: "M12 19.5s-7-4.3-7-9.6A3.9 3.9 0 0 1 12 7.6a3.9 3.9 0 0 1 7 2.3c0 5.3-7 9.6-7 9.6z",
  plus: "M12 5v14 M5 12h14",
  check: ICONS.check,
  trash: "M5 7h14 M10 4h4 M7 7l1 13h8l1-13 M10.5 11v5 M13.5 11v5",
  pin: "M9 4h6 M10 4v5.5L7 13.5h10L14 9.5V4 M12 13.5v6.5",
  copy: "M8.5 8.5h10.5v11.5H8.5z M5 15.5V4h11",
  edit: "M4.5 19.5l1-4L15.8 5.2l3 3L8.5 18.5z M13.8 7.2l3 3",
  repeat: "M17 3.5l3 3-3 3 M4 11V9.5a3 3 0 0 1 3-3h13 M7 20.5l-3-3 3-3 M20 13v1.5a3 3 0 0 1-3 3H4",
  chevronStart: ICONS.chevronRight,
  chevronEnd: ICONS.chevronLeft,
  chevronDown: "M6 9.5l6 6 6-6",
  clock: "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16z M12 8v4l3 2",
} as const;

/** Filled glyphs for the focus controls (they read better solid at 12 px). */
export const PL_SOLID = {
  play: "M8 5.5v13l10.5-6.5z",
  pause: "M7 5h3.6v14H7z M13.4 5H17v14h-3.6z",
  stop: "M6.5 6.5h11v11h-11z",
  skip: "M5.5 5.5v13l8.5-6.5z M15.5 5.5H18v13h-2.5z",
  more: ICONS.ellipsis,
} as const;

export const HABIT_GLYPHS: Record<HabitIcon, string> = {
  water: "M12 3.5s-6 6.6-6 11a6 6 0 0 0 12 0c0-4.4-6-11-6-11z",
  stretch: "M12 3a1.7 1.7 0 1 0 0 3.4A1.7 1.7 0 0 0 12 3z M5.5 6.5 12 10l6.5-3.5 M12 10v5 M12 15l-3.5 5.5 M12 15l3.5 5.5",
  posture: "M10.5 3a1.7 1.7 0 1 0 0 3.4 1.7 1.7 0 0 0 0-3.4z M10.5 8.5v6.5h5.5l1.5 5.5 M10.5 11.5H15 M6.5 20.5h4",
  eyes: "M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z",
  walk: "M13.5 3a1.7 1.7 0 1 0 0 3.4 1.7 1.7 0 0 0 0-3.4z M9.5 21l2.2-6 3 3v3 M11.7 15l1.3-6.2-3.3 2.2-1 3 M13 8.8l2 3h3",
  read: "M3.5 5.8c3-1 6-1 8.5 1 2.5-2 5.5-2 8.5-1v13.4c-3-1-6-1-8.5 1-2.5-2-5.5-2-8.5-1z M12 6.8v13.4",
  sleep: "M18.5 14.6A7.4 7.4 0 0 1 9.4 5.5a7.4 7.4 0 1 0 9.1 9.1z",
  custom: "M12 3.5l1.9 5.1 5.1 1.9-5.1 1.9L12 17.5l-1.9-5.1L5 10.5l5.1-1.9z",
};

export const HABIT_COLORS: Record<HabitIcon, string> = {
  water: "#38BDF8",
  stretch: "#34D399",
  posture: "#A78BFA",
  eyes: "#F472B6",
  walk: "#F59E0B",
  read: "#C9956A",
  sleep: "#818CF8",
  custom: "#EAB308",
};

export type PlIcon = keyof typeof PL_ICONS;

export function icon(name: PlIcon, size = 14): SVGSVGElement {
  return svg(PL_ICONS[name], size, { stroke: 1.8 });
}

export function solid(name: keyof typeof PL_SOLID, size = 12): SVGSVGElement {
  return svg(PL_SOLID[name], size);
}

export function habitGlyph(kind: HabitIcon, size = 14): SVGSVGElement {
  return svg(HABIT_GLYPHS[kind], size, { stroke: 1.8 });
}

/** A square icon button with its name for screen readers and as a tooltip. */
export function iconButton(glyph: SVGSVGElement, label: string, onClick: (e: MouseEvent) => void, cls = ""): HTMLButtonElement {
  return h("button", {
    type: "button",
    class: `pl-ibtn ${cls}`.trim(),
    title: label,
    "aria-label": label,
    onclick: onClick as EventListener,
  }, glyph);
}

/** The round check of a task or habit: a real checkbox for assistive tech. */
export function checkCircle(label: string, onToggle: () => void, color?: string): HTMLButtonElement {
  const el = h("button", {
    type: "button",
    class: "pl-check",
    role: "checkbox",
    "aria-checked": "false",
    "aria-label": label,
    onclick: () => onToggle(),
  }, svg(PL_ICONS.check, 12, { stroke: 2.6 }));
  if (color) el.style.setProperty("--pl-accent", color);
  return el;
}

export function setChecked(el: HTMLElement, on: boolean) {
  el.classList.toggle("on", on);
  el.setAttribute("aria-checked", String(on));
}

/** A small pressed / not-pressed chip («امروز», «فردا», a filter). */
export function toggleChip(label: string, onClick: () => void): HTMLButtonElement {
  return h("button", { type: "button", class: "pl-chip", "aria-pressed": "false", text: label, onclick: () => onClick() });
}

export function setPressed(el: HTMLElement, on: boolean) {
  el.classList.toggle("on", on);
  el.setAttribute("aria-pressed", String(on));
}

export interface PlannerShell {
  /** The view root (`.view`). */
  el: HTMLElement;
  /** Right of the title (inline end): view-specific controls before the menu button. */
  tools: HTMLElement;
  body: HTMLElement;
  /** Shows a failed command under the title; null hides it. */
  setError(message: string | null): void;
  /** Replaces the title text (the week view's range). */
  setTitle(text: string): void;
}

/**
 * The card every planner view sits in: icon + title, its own tools and the
 * grid button back to the menu, then the body. The left 88 px stay clear for
 * the character, who never flips sides.
 */
export function plannerShell(opts: { icon: PlIcon; title: string; onMenu(): void; cls?: string }): PlannerShell {
  const title = h("h2", { class: "pl-title", text: opts.title });
  const tools = h("div", { class: "pl-tools" });
  const menu = iconButton(icon("menu", 13), t("planner.openMenu"), () => opts.onMenu(), "pl-menu-btn");
  const error = h("div", { class: "pl-error", role: "alert", hidden: true });
  const head = h("div", { class: "pl-head" },
    h("span", { class: "pl-head-ico", "aria-hidden": "true" }, icon(opts.icon, 15)),
    title,
    h("div", { class: "grow" }),
    tools,
    menu,
  );
  const body = h("div", { class: "pl-body" });
  const inner = h("div", { class: `pl ${opts.cls ?? ""}`.trim() }, head, error, body);
  const el = h("div", { class: "view pl-view" }, h("div", { class: "card pl-card" }, inner));
  let errorTimer: number | null = null;
  return {
    el,
    tools,
    body,
    setError(message) {
      if (errorTimer != null) window.clearTimeout(errorTimer);
      errorTimer = null;
      error.hidden = !message;
      error.textContent = message ?? "";
      // Said once, then out of the way; the action can simply be tried again.
      if (message) errorTimer = window.setTimeout(() => {
        error.hidden = true;
        errorTimer = null;
      }, 6000);
    },
    setTitle(text) {
      title.textContent = text;
    },
  };
}

/**
 * Replaces a list's rows and puts the keyboard back on the control it was on
 * (matched by `data-fk`), so a check or a menu doesn't lose focus when the
 * data comes back.
 */
export function rerender(container: HTMLElement, rows: Node[]) {
  const active = document.activeElement as HTMLElement | null;
  const key = active && container.contains(active) ? active.dataset.fk : undefined;
  const top = container.scrollTop;
  container.replaceChildren(...rows);
  container.scrollTop = top;
  if (!key) return;
  for (const el of container.querySelectorAll<HTMLElement>("[data-fk]")) {
    if (el.dataset.fk === key) {
      el.focus();
      return;
    }
  }
}

export interface TimePicker {
  el: HTMLElement;
  /** "HH:MM". */
  get value(): string;
  set value(v: string);
  focus(): void;
}

/**
 * Hour and minute as two native selects (5-minute steps), so the digits
 * follow the UI language — a native time field shows the OS's digits and
 * clock — while the keyboard and screen readers still get real controls.
 */
export function timePicker(label: string, initial: string): TimePicker {
  const opt = (n: number) => h("option", { value: String(n).padStart(2, "0"), text: formatNumber(n, { minimumIntegerDigits: 2 }) });
  const hours = h("select", { class: "pl-tp-part", "aria-label": t("planner.time.hour", { label }) },
    ...Array.from({ length: 24 }, (_, i) => opt(i))) as HTMLSelectElement;
  const minutes = h("select", { class: "pl-tp-part", "aria-label": t("planner.time.minute", { label }) },
    ...Array.from({ length: 12 }, (_, i) => opt(i * 5))) as HTMLSelectElement;
  for (const s of [hours, minutes]) s.addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key !== "Escape") e.stopPropagation();
  });
  const el = h("span", { class: "pl-tp", role: "group", "aria-label": label }, hours, h("span", { class: "pl-tp-sep", text: ":" }), minutes);
  const picker: TimePicker = {
    el,
    get value() {
      return `${hours.value}:${minutes.value}`;
    },
    set value(v: string) {
      const m = /^(\d{1,2}):(\d{2})/.exec(v);
      if (!m) return;
      hours.value = m[1].padStart(2, "0");
      // Rounded down to the 5-minute step the list offers.
      minutes.value = String(Math.floor(Number(m[2]) / 5) * 5).padStart(2, "0");
    },
    focus: () => hours.focus(),
  };
  picker.value = initial;
  return picker;
}

/** Keeps typing within `max` characters (Rust refuses longer ones anyway). */
export function limitField(el: HTMLInputElement | HTMLTextAreaElement, max: number) {
  el.maxLength = max;
}

/** An empty-state block: a line of text and optional actions under it. */
export function emptyState(text: string, ...actions: HTMLElement[]): HTMLElement {
  return h("div", { class: "pl-empty" }, h("p", { text }), actions.length ? h("div", { class: "pl-empty-actions" }, ...actions) : null);
}

/**
 * Keyboard inside a view: Escape leaves the open sub-state (a menu, an edit)
 * instead of closing the island, and the fields keep Enter for themselves.
 */
export function stopEscape(el: HTMLElement, onEscape: () => boolean) {
  el.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && onEscape()) {
      e.preventDefault();
      e.stopPropagation();
    }
  });
}
