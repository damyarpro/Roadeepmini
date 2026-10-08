// The «/» menu: every place the island can go, as a two-column grid with a
// filter field on top (labels match in Persian and English). Typing «/» first
// in the chat opens it, as do the header's grid tab and each planner view's
// grid button. Arrows move, Enter opens, Escape goes back where it came from.

import { h } from "./dom";
import type { ViewActions, ViewHost } from "./views";
import type { IslandViewName } from "../core/layout";
import { State } from "../core/state";
import { getLanguage, t } from "../core/i18n";
import { plannerEn } from "../core/locales/planner-en";
import { plannerFa } from "../core/locales/planner-fa";
import { icon, type PlIcon } from "./planner/ui";

/** roadeep.com's support page; opened in the browser through Rust's URL check. */
export const FEEDBACK_URL = "https://roadeep.com/support";

export type PaletteId =
  | "activity"
  | "chat" | "home" | "tasks" | "notes" | "reminders" | "habits" | "today" | "week" | "focus" | "settings" | "feedback";

export const PALETTE_ITEMS: { id: PaletteId; icon: PlIcon }[] = [
  { id: "activity", icon: "tasks" },
  { id: "chat", icon: "chat" },
  { id: "home", icon: "home" },
  { id: "tasks", icon: "tasks" },
  { id: "notes", icon: "notes" },
  { id: "reminders", icon: "reminders" },
  { id: "habits", icon: "habits" },
  { id: "today", icon: "today" },
  { id: "week", icon: "week" },
  { id: "focus", icon: "focus" },
];

/** Views the menu opens, i.e. the ones that count as "planner" for the header tab. */
export const PLANNER_VIEWS: ReadonlySet<IslandViewName> = new Set<IslandViewName>([
  "menu", "tasks", "notes", "reminders", "habits", "today", "week", "focus",
  "activity",
]);

/**
 * Folds what people type into one form: Arabic yeh/kaf to Persian, no
 * zero-width joiners or diacritics, lower case.
 */
export function normalizeQuery(s: string): string {
  return s
    .toLowerCase()
    .replace(/[يى]/g, "ی")
    .replace(/ك/g, "ک")
    .replace(/[‌‍ـً-ٟ]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

const label = (id: PaletteId, lang: "fa" | "en") =>
  id === "activity" ? (lang === "fa" ? "فعالیت کدنویسی" : "Coding activity") : (lang === "fa" ? plannerFa : plannerEn)[`planner.menu.${id}`] ?? id;

/** Items whose label (in either language) or id contains the query, in menu order. */
export function filterPalette(query: string): PaletteId[] {
  const q = normalizeQuery(query);
  if (!q) return PALETTE_ITEMS.map((i) => i.id);
  return PALETTE_ITEMS.filter((i) =>
    [label(i.id, "fa"), label(i.id, "en"), i.id].some((text) => normalizeQuery(text).includes(q)),
  ).map((i) => i.id);
}

/**
 * The chat's «/» trigger: only a «/» typed into an EMPTY field opens the menu
 * (what follows is typed into the menu's own filter). A «/» typed in front of
 * a draft, or a pasted path like /etc/hosts, stays text.
 */
export function opensMenuFromChat(before: string, after: string): boolean {
  return before === "" && after === "/";
}

// What the next opening of the menu starts with, set by openPalette().
let pendingFilter = "";
let returnTo: IslandViewName | null = null;
/** The current menu view's field focus (views are rebuilt on a language switch). */
let activate: (() => void) | null = null;

/**
 * Opens the menu view, optionally with a filter already typed (the chat's
 * «/…»). The field takes the caret at once, so the next keys typed after «/»
 * land in it rather than in the chat.
 */
export function openPalette(actions: ViewActions, filter = "", from: IslandViewName | null = State.view) {
  pendingFilter = filter;
  returnTo = from && from !== "menu" ? from : returnTo;
  actions.setView("menu");
  activate?.();
}

/** Where a chosen item leads. */
export function runPaletteItem(id: PaletteId, actions: ViewActions) {
  actions.blip();
  switch (id) {
    case "chat":
      actions.setView("prompt");
      break;
    case "home":
      actions.setView(State.defaultView());
      break;
    case "settings":
      actions.openSettingsWindow();
      actions.setView(State.defaultView());
      break;
    case "feedback":
      actions.openUrl(FEEDBACK_URL);
      break;
    default:
      actions.setView(id);
  }
}

export function buildPalette(actions: ViewActions): ViewHost {
  const listId = "pal-list";
  const search = h("input", {
    type: "text",
    class: "pal-search",
    role: "combobox",
    "aria-expanded": "true",
    "aria-controls": listId,
    "aria-autocomplete": "list",
    "aria-label": t("planner.menu.search"),
    placeholder: t("planner.menu.search"),
    spellcheck: "false",
    autocomplete: "off",
  }) as HTMLInputElement;
  const grid = h("div", { class: "pal-grid", id: listId, role: "listbox", "aria-label": t("planner.menu.title") });
  const none = h("p", { class: "pal-none", text: t("planner.menu.none"), hidden: true });
  const tiles = new Map<PaletteId, HTMLElement>();
  for (const item of PALETTE_ITEMS) {
    const tile = h("div", {
      class: "pal-tile",
      id: `pal-${item.id}`,
      role: "option",
      "aria-selected": "false",
      onclick: () => choose(item.id),
    },
      h("span", { class: "pal-ico", "aria-hidden": "true" }, icon(item.icon, 15)),
      h("span", { class: "pal-label", text: t(`planner.menu.${item.id}`) }),
    );
    tile.addEventListener("mousemove", () => select(item.id));
    tiles.set(item.id, tile);
    grid.append(tile);
  }

  const body = h("div", { class: "pal" },
    h("div", { class: "pal-top" }, h("span", { class: "pal-slash", "aria-hidden": "true", text: "/" }), search),
    grid, none,
  );
  const el = h("div", { class: "view pl-view" }, h("div", { class: "card pl-card" }, body));

  let visible: PaletteId[] = PALETTE_ITEMS.map((i) => i.id);
  let selected: PaletteId | null = visible[0];

  function select(id: PaletteId | null) {
    selected = id;
    for (const [tid, tile] of tiles) {
      const on = tid === id;
      tile.classList.toggle("sel", on);
      tile.setAttribute("aria-selected", String(on));
    }
    if (id) search.setAttribute("aria-activedescendant", `pal-${id}`);
    else search.removeAttribute("aria-activedescendant");
    if (id) tiles.get(id)?.scrollIntoView({ block: "nearest" });
  }

  function applyFilter() {
    visible = filterPalette(search.value);
    for (const [id, tile] of tiles) tile.hidden = !visible.includes(id);
    none.hidden = visible.length > 0;
    select(visible.includes(selected as PaletteId) && search.value === "" ? selected : visible[0] ?? null);
  }

  function choose(id: PaletteId) {
    search.value = "";
    returnTo = null;
    runPaletteItem(id, actions);
  }

  function back() {
    const to = returnTo ?? State.defaultView();
    returnTo = null;
    actions.setView(to === "menu" ? State.defaultView() : to);
  }

  search.addEventListener("input", () => applyFilter());
  search.addEventListener("keydown", (e) => {
    const k = e as KeyboardEvent;
    if (k.isComposing) return;
    const i = selected ? visible.indexOf(selected) : -1;
    // The grid reads right-to-left in Persian: the next tile is on the left.
    const rtl = getLanguage() === "fa";
    const step = (n: number) => {
      if (!visible.length) return;
      const next = Math.max(0, Math.min(visible.length - 1, (i < 0 ? 0 : i) + n));
      select(visible[next]);
    };
    switch (k.key) {
      case "ArrowDown": step(2); break;
      case "ArrowUp": step(-2); break;
      case "ArrowLeft": step(rtl ? 1 : -1); break;
      case "ArrowRight": step(rtl ? -1 : 1); break;
      case "Home": if (visible.length) select(visible[0]); break;
      case "End": if (visible.length) select(visible[visible.length - 1]); break;
      case "Enter":
        if (selected && visible.includes(selected)) choose(selected);
        break;
      case "Escape":
        if (search.value) {
          search.value = "";
          applyFilter();
        } else {
          back();
        }
        break;
      default:
        // Typing keeps going to the field; the island's own keys stay out of it.
        e.stopPropagation();
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  });

  function focus() {
    if (pendingFilter !== "") {
      search.value = pendingFilter;
      pendingFilter = "";
    }
    applyFilter();
    search.focus();
    // The caret goes after what was typed in the chat.
    search.setSelectionRange(search.value.length, search.value.length);
  }
  activate = focus;

  return {
    el,
    sync() {
      if (pendingFilter !== "") {
        search.value = pendingFilter;
        pendingFilter = "";
        applyFilter();
      }
    },
    focus,
  };
}
