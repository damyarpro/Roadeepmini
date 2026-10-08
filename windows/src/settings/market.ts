// The service marketplace: a dialog over the settings window listing every
// service the app can show (the hand-coded ones and the catalog's), with a
// search and category chips. Adding one only puts it in "My services"
// (main.ts); its keys are entered there.

import { isolate, t } from "../core/i18n";
import { Bridge } from "../core/bridge";
import { CATEGORIES, type Category } from "../core/catalog";
import { h } from "../views/dom";
import { followTextDirection, icon, statusBadge } from "./ui";

export interface MarketItem {
  /** Pill id, "integration_<id>". */
  id: string;
  name: string;
  color: string;
  category: Category;
  /** One line in the current language. */
  desc: string;
}

export interface MarketOptions {
  items: MarketItem[];
  isAdded: (id: string) => boolean;
  /** Called after the dialog has closed; focus is then the caller's to place. */
  onAdd: (id: string) => void;
}

/** Search folds case and the Arabic/Persian letter variants a keyboard may type. */
function fold(text: string): string {
  return text.toLowerCase().replace(/ي/g, "ی").replace(/ك/g, "ک").replace(/‌/g, "");
}

let backdrop: HTMLElement | null = null;
let dialog: HTMLElement | null = null;
let restoreFocus: HTMLElement | null = null;

function close(focusBack: boolean) {
  if (!backdrop) return;
  document.removeEventListener("keydown", onKey, true);
  backdrop.remove();
  backdrop = dialog = null;
  document.body.classList.remove("wiz-open");
  if (focusBack) restoreFocus?.focus?.();
  restoreFocus = null;
}

function onKey(e: KeyboardEvent) {
  if (!dialog) return;
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    close(true);
    return;
  }
  // Keep Tab inside the dialog.
  if (e.key === "Tab") {
    const items = [...dialog.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled)")]
      .filter((el) => el.offsetParent !== null);
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
}

export function openMarket(opts: MarketOptions) {
  if (backdrop) return;
  restoreFocus = document.activeElement as HTMLElement | null;
  const items = [...opts.items].sort((a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" }));
  let query = "";
  let category: Category | "all" = "all";

  const live = h("span", { class: "sr-only", role: "status", "aria-live": "polite" });
  let liveTimer = 0;
  const announce = (text: string) => {
    window.clearTimeout(liveTimer);
    // Once typing pauses, not on every key.
    liveTimer = window.setTimeout(() => { live.textContent = text; }, 500);
  };

  const search = h("input", {
    type: "search", class: "mk-search", autocomplete: "off", spellcheck: "false",
    placeholder: t("market.search"), "aria-label": t("market.search"), "aria-controls": "mk-grid",
  }) as HTMLInputElement;
  followTextDirection(search);

  // Only the categories that have something in them.
  const present = CATEGORIES.filter((c) => items.some((i) => i.category === c));
  const chips = h("div", { class: "mk-cats", role: "group", "aria-label": t("market.categories") });
  const chipButtons: { id: Category | "all"; el: HTMLButtonElement }[] = [];
  for (const id of ["all" as const, ...present]) {
    const el = h("button", {
      type: "button", class: "mk-cat", "aria-controls": "mk-grid",
      text: id === "all" ? t("market.all") : t(`integrations.cat.${id}`),
    }) as HTMLButtonElement;
    el.addEventListener("click", () => {
      category = id;
      apply(true);
    });
    chipButtons.push({ id, el });
    chips.append(el);
  }

  const grid = h("ul", { class: "mk-grid", id: "mk-grid" });
  const none = h("p", { class: "hint mk-none", dir: "auto" });
  const cards = items.map((item) => {
    const card = marketCard(item, opts);
    grid.append(card);
    const words = fold(`${item.name} ${item.desc} ${t(`integrations.cat.${item.category}`)}`);
    return { item, card, words };
  });

  function apply(speak: boolean) {
    const q = fold(query.trim());
    let shown = 0;
    for (const { item, card, words } of cards) {
      card.hidden = (category !== "all" && item.category !== category) || (q !== "" && !words.includes(q));
      if (!card.hidden) shown++;
    }
    for (const chip of chipButtons) {
      chip.el.setAttribute("aria-pressed", chip.id === category ? "true" : "false");
    }
    none.hidden = shown > 0;
    // Every listed category has items, so only a search can empty the list.
    none.textContent = shown > 0 ? "" : t("market.noResults", { q: isolate(query.trim()) });
    if (speak) announce(t("market.results", { n: shown }));
  }

  search.addEventListener("input", () => {
    query = search.value;
    apply(true);
  });

  const closeBtn = h("button", {
    type: "button", class: "mk-close", "aria-label": t("market.close"), title: t("market.close"),
    onclick: () => close(true),
  }, icon("close", 18));

  dialog = h("div", { class: "mk", role: "dialog", "aria-modal": "true", "aria-labelledby": "mk-title", "aria-describedby": "mk-intro" },
    h("div", { class: "mk-head" },
      h("div", { class: "mk-titles" },
        h("h2", { id: "mk-title", text: t("market.title") }),
        h("p", { class: "hint", id: "mk-intro", text: t("market.intro") }),
      ),
      closeBtn,
    ),
    h("div", { class: "mk-tools" }, search, chips),
    h("div", { class: "mk-body" }, grid, none),
    live,
  );

  // Adding closes the dialog and hands over to "My services".
  dialog.addEventListener("mk-add", (e) => {
    const id = (e as CustomEvent<string>).detail;
    close(false);
    opts.onAdd(id);
  });

  backdrop = h("div", { class: "mk-backdrop" }, dialog);
  backdrop.addEventListener("mousedown", (e) => {
    if (e.target === backdrop) close(true);
  });
  document.body.append(backdrop);
  // Shares the wizard's scroll lock.
  document.body.classList.add("wiz-open");
  document.addEventListener("keydown", onKey, true);
  apply(false);
  search.focus();
  void Bridge.log(`settings: market opened (${items.length} services)`);
}

function marketCard(item: MarketItem, opts: MarketOptions): HTMLElement {
  const nameId = `mk-name-${item.id}`;
  const foot = h("div", { class: "mk-card-foot" });
  if (opts.isAdded(item.id)) {
    foot.append(statusBadge("ok", t("market.added")));
  } else {
    const add = h("button", {
      type: "button", class: "sm",
      "aria-label": t("market.addNamed", { name: item.name }),
    }, icon("plus", 14), h("span", { text: t("market.add") })) as HTMLButtonElement;
    add.addEventListener("click", () => {
      add.dispatchEvent(new CustomEvent("mk-add", { detail: item.id, bubbles: true }));
    });
    foot.append(add);
  }
  return h("li", { class: "mk-card", "aria-labelledby": nameId },
    h("div", { class: "mk-card-top" },
      h("i", { class: "agent-swatch", style: `--c:${item.color}`, "aria-hidden": "true" }),
      h("span", { class: "mk-name", id: nameId, dir: "auto", text: item.name }),
      h("span", { class: "mk-cat-label", text: t(`integrations.cat.${item.category}`) }),
    ),
    h("p", { class: "mk-desc", dir: "auto", text: item.desc }),
    foot,
  );
}
