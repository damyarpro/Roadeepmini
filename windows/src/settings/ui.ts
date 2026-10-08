// Small controls the settings sections share.

import { isRtl, textDirection, t } from "../core/i18n";
import { h } from "../views/dom";
import { helpDisclosure } from "./help";
export { helpDisclosure } from "./help";

// Line icons (Lucide geometry, 24×24, stroked with currentColor). Every shape
// is written as a path so one builder covers them all.
const ICONS = {
  dashboard: ["M3 3h7v7H3z", "M14 3h7v7h-7z", "M3 14h7v7H3z", "M14 14h7v7h-7z"],
  arrowBack: ["m12 5-7 7 7 7", "M5 12h14"],
  help: ["M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z", "M12 7v6", "M12 17h.01"],
  account: ["M12 3a5 5 0 1 0 0 10a5 5 0 1 0 0-10z", "M20 21a8 8 0 0 0-16 0"],
  integrations: [
    "M10 22V7a1 1 0 0 0-1-1H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-5a1 1 0 0 0-1-1H2",
    "M15 2h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-6a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z",
  ],
  agents: [
    "M12 8V4H8", "M6 8h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z",
    "M2 14h2", "M20 14h2", "M15 13v2", "M9 13v2",
  ],
  claude: ["M4 17l6-6-6-6", "M12 19h8"],
  mcp: ["M12 22v-5", "M9 8V2", "M15 8V2", "M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8z"],
  servers: [
    "M4 2h16a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z",
    "M4 14h16a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2z",
    "M6 6h.01", "M6 18h.01",
  ],
  general: [
    "M21 4h-7", "M10 4H3", "M21 12h-9", "M8 12H3", "M21 20h-5", "M12 20H3",
    "M14 2v4", "M8 10v4", "M16 18v4",
  ],
  planner: [
    "M8 2v4", "M16 2v4", "M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
    "M3 10h18", "m9 16 2 2 4-4",
  ],
  keyboard: [
    "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
    "M6 8h.01", "M10 8h.01", "M14 8h.01", "M18 8h.01", "M8 12h.01", "M12 12h.01", "M16 12h.01", "M7 16h10",
  ],
  check: ["M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z", "m9 12 2 2 4-4"],
  minus: ["M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z", "M8 12h8"],
  alert: [
    "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3",
    "M12 9v4", "M12 17h.01",
  ],
  error: ["M12 2a10 10 0 1 0 0 20a10 10 0 1 0 0-20z", "m15 9-6 6", "m9 9 6 6"],
  close: ["M18 6 6 18", "m6 6 12 12"],
  logout: ["M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4", "m16 17 5-5-5-5", "M21 12H9"],
  plus: ["M5 12h14", "M12 5v14"],
  chevronUp: ["m18 15-6-6-6 6"],
  chevronDown: ["m6 9 6 6 6-6"],
  trash: ["M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"],
  eye: [
    "M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0",
    "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6z",
  ],
  eyeOff: [
    "M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49",
    "M14.084 14.158a3 3 0 0 1-4.242-4.242",
    "M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143",
    "m2 2 20 20",
  ],
  lock: [
    "M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z",
    "M7 11V7a5 5 0 0 1 10 0v4",
  ],
} as const;

export type IconName = keyof typeof ICONS;

export function icon(name: IconName, size = 16): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const el = document.createElementNS(ns, "svg");
  el.setAttribute("viewBox", "0 0 24 24");
  el.setAttribute("width", String(size));
  el.setAttribute("height", String(size));
  el.setAttribute("fill", "none");
  el.setAttribute("stroke", "currentColor");
  el.setAttribute("stroke-width", "1.75");
  el.setAttribute("stroke-linecap", "round");
  el.setAttribute("stroke-linejoin", "round");
  el.setAttribute("aria-hidden", "true");
  el.setAttribute("class", "icon");
  for (const d of ICONS[name]) {
    const p = document.createElementNS(ns, "path");
    p.setAttribute("d", d);
    el.append(p);
  }
  return el;
}

export type StatusKind = "ok" | "off" | "warn" | "err";

const STATUS_ICON: Record<StatusKind, IconName> = { ok: "check", off: "minus", warn: "alert", err: "error" };

/** A status with words and a shape, so colour is never the only signal. */
export function statusBadge(kind: StatusKind, text: string): HTMLElement {
  return h("span", { class: `status-badge ${kind}` }, icon(STATUS_ICON[kind], 14), h("span", { text }));
}

/** Section header: icon tile, title, one-line description, and a slot for a status. */
export function sectionHead(opts: {
  id: string; icon: IconName; title: string; desc?: string; badge?: HTMLElement | null;
}): HTMLElement {
  return h("header", { class: "sec-head" },
    h("span", { class: "sec-icon" }, icon(opts.icon, 18)),
    h("div", { class: "sec-titles" },
      h("div", {class:"sec-title-line"},
        h("h2", { id: opts.id, tabindex: "-1", text: opts.title }),
        opts.desc ? helpDisclosure(opts.desc, `${t("settings.help")}: ${opts.title}`) : null,
      ),
    ),
    opts.badge ?? null,
  );
}

type Child = Node | string | null | undefined | false;

/** Label and helper text on the reading side, the control on the other. */
export function settingRow(
  opts: { label: string; hint?: string | null; forId?: string; extra?: Child; class?: string },
  ...controls: Child[]
): HTMLElement {
  return h("div", { class: opts.class ? `set-row ${opts.class}` : "set-row" },
    h("div", { class: "set-text" },
      h("div", {class:"set-label-line"}, opts.forId
        ? h("label", { class: "set-label", for: opts.forId, text: opts.label })
        : h("span", { class: "set-label", text: opts.label }),
        opts.hint ? helpDisclosure(opts.hint, `${t("settings.help")}: ${opts.label}`) : null,
      ),
      opts.extra ?? null,
    ),
    h("div", { class: "set-ctrl" }, ...controls),
  );
}

export function switchEl(on: boolean, disabled: boolean, label: string, onChange: (v: boolean) => void): HTMLButtonElement {
  const el = h("button", {
    type: "button",
    class: on ? "switch on" : "switch",
    role: "switch",
    "aria-checked": on ? "true" : "false",
    "aria-label": label,
  }) as HTMLButtonElement;
  el.disabled = disabled;
  el.addEventListener("click", () => {
    const next = !el.classList.contains("on");
    el.classList.toggle("on", next);
    el.setAttribute("aria-checked", next ? "true" : "false");
    onChange(next);
  });
  return el;
}

export function fieldError(text: string | undefined): HTMLElement {
  return h("div", { class: "field-err", role: text ? "alert" : undefined, dir: "auto", text: text ?? "" });
}

/** A labelled input with room for its own error underneath. */
export function field(id: string, label: string, input: HTMLElement, error?: string): HTMLElement {
  input.id = id;
  if (error) input.setAttribute("aria-invalid", "true");
  return h("div", { class: "field" }, h("label", { for: id, text: label }), input, fieldError(error));
}

/**
 * `dir="auto"` on an empty field falls back to LTR, which puts a Persian
 * placeholder on the wrong side; follow the typed text, else the UI language.
 */
export function followTextDirection(el: HTMLInputElement | HTMLTextAreaElement) {
  const sync = () => { el.dir = textDirection(el.value) ?? (isRtl() ? "rtl" : "ltr"); };
  sync();
  el.addEventListener("input", sync);
}

export function linkButton(text: string, onClick: () => void): HTMLElement {
  return h("button", { type: "button", class: "link", text, onclick: onClick });
}

/** Character count the way the Rust side counts (code points, not UTF-16). */
export const charCount = (s: string) => Array.from(s).length;

/** Stands in for a command inside a translated sentence (see withCommand). */
export const CMD_SLOT = "";

/**
 * A translated sentence with a shell command in it, as text around an LTR
 * <code>. Inline bidi isolates are not enough here: in Persian the command's
 * hyphens and spaces still reorder and it breaks across lines.
 */
export function withCommand(sentence: string, command: string): (Node | string)[] {
  const at = sentence.indexOf(CMD_SLOT);
  const code = h("code", { class: "cmd", dir: "ltr", text: command });
  if (at < 0) return [sentence, " ", code];
  return [sentence.slice(0, at), code, sentence.slice(at + CMD_SLOT.length)];
}
