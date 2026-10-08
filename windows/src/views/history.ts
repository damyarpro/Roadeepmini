// The chat's history panel: previous Roadeep conversations, newest first,
// paged with "load more". It replaces the log inside the chat card while open;
// the chat owns opening, deleting and starting conversations (`HistoryHooks`).

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import { Bridge, isRoadeepError, type ChatThreadsPage } from "../core/bridge";
import { t } from "../core/i18n";
import { roadeepErrorText } from "./errors";
import { timeAgo } from "./integrations";

export interface HistoryHooks {
  list(offset: number, limit: number): Promise<ChatThreadsPage>;
  /** Opens the thread in the chat; rejects when it could not be read. */
  open(id: string): Promise<void>;
  remove(id: string): Promise<void>;
  newChat(): void;
  close(): void;
  /** The thread the chat is in, to mark it in the list. */
  currentId(): string | null;
}

export interface HistoryPanel {
  el: HTMLElement;
  /** Reloads the first page and moves focus into the panel. */
  show(): void;
}

const PAGE = 20;

/** One relative time, like the integration cards ("5m ago"). */
function relative(iso: string): string {
  const ago = timeAgo(iso);
  if (!ago) return "";
  return ago === t("time.justNow") ? ago : t("time.ago", { t: ago });
}

export function buildHistory(hooks: HistoryHooks): HistoryPanel {
  const list = h("div", { class: "hist-list", role: "list", "aria-label": t("history.title") });
  const status = h("div", { class: "hist-status", role: "status" });
  const more = h("button", { type: "button", class: "btn secondary hist-more", text: t("history.loadMore") });
  const back = h("button", {
    type: "button", class: "hist-back", title: t("history.back"), "aria-label": t("history.back"),
    onclick: () => hooks.close(),
  }, svg(ICONS.chevronLeft, 12, { stroke: 2.2 }));
  const newChat = h("button", {
    type: "button", class: "hist-new", onclick: () => hooks.newChat(),
  }, svg(ICONS.plus, 11), h("span", { text: t("history.newChat") }));

  const el = h("div", { class: "hist", hidden: true },
    h("div", { class: "hist-head" },
      back,
      h("h2", { class: "hist-title", text: t("history.title") }),
      newChat,
    ),
    h("div", { class: "hist-scroll" }, list, status, more),
  );

  let items: ChatThreadsPage["items"] = [];
  /** Closes the delete confirm that is open, if any (only one at a time). */
  let closeConfirm: ((focus?: boolean) => void) | null = null;
  let confirmSeq = 0;
  let hasMore = false;
  let loading = false;
  /** Bumped on every reload so a slow page can't land on a newer list. */
  let generation = 0;

  function setStatus(kind: "loading" | "error" | "empty" | null, err?: unknown) {
    clear(status);
    status.className = `hist-status ${kind ?? ""}`;
    if (kind === "loading") {
      status.append(h("div", { class: "typing" }, h("i"), h("i"), h("i")), h("span", { text: t("history.loading") }));
    } else if (kind === "empty") {
      status.append(h("span", { text: t("history.empty") }));
    } else if (kind === "error") {
      status.append(
        h("span", { class: "hist-err", dir: "auto", text: roadeepErrorText(err) }),
        h("button", { type: "button", class: "btn secondary", text: t("history.retry"), onclick: () => void load(items.length === 0) }),
      );
    }
  }

  function syncMore() {
    more.hidden = !hasMore || items.length === 0;
    more.disabled = loading;
    more.textContent = t(loading && items.length ? "history.loadingMore" : "history.loadMore");
  }

  async function load(reset: boolean) {
    if (loading && !reset) return;
    const gen = reset ? ++generation : generation;
    loading = true;
    if (reset) {
      items = [];
      hasMore = false;
      closeConfirm = null;
      clear(list);
    }
    setStatus(items.length ? null : "loading");
    syncMore();
    try {
      const page = await hooks.list(items.length, PAGE);
      if (gen !== generation) return;
      // Skip anything already listed (a new thread can shift the pages).
      const known = new Set(items.map((i) => i.id));
      const fresh = page.items.filter((i) => !known.has(i.id));
      items.push(...fresh);
      hasMore = page.hasMore;
      for (const item of fresh) list.append(row(item));
      setStatus(items.length ? null : "empty");
    } catch (err) {
      if (gen !== generation) return;
      void Bridge.log(`chat: history unavailable ${isRoadeepError(err) ? `${err.code} ${err.requestId ?? ""}` : ""}`);
      setStatus("error", err);
    } finally {
      if (gen === generation) {
        loading = false;
        syncMore();
      }
    }
  }
  more.addEventListener("click", () => void load(false));

  function row(item: ChatThreadsPage["items"][number]): HTMLElement {
    const title = item.title.trim() || t("history.untitled");
    const meta = h("div", { class: "hist-meta" }, h("span", { text: relative(item.updatedAt) }));
    if (item.model) meta.append(h("span", { class: "hist-model", dir: "ltr", text: item.model }));
    const main = h("button", {
      type: "button", class: "hist-open", "data-id": item.id,
    },
      h("span", { class: "hist-name", dir: "auto", text: title }),
      item.preview.trim() ? h("span", { class: "hist-preview", dir: "auto", text: item.preview.trim() }) : null,
      meta,
    );
    const del = h("button", {
      type: "button", class: "hist-del", title: t("history.delete"), "aria-label": t("history.deleteNamed", { title }),
    }, svg(ICONS.xmark, 10));
    const note = h("div", { class: "hist-note", dir: "auto", role: "alert" });
    note.hidden = true;
    const li = h("div", { class: "hist-item", role: "listitem" }, main, del, note);
    if (item.id === hooks.currentId()) {
      li.classList.add("current");
      main.setAttribute("aria-current", "true");
    }

    const fail = (err: unknown) => {
      note.textContent = roadeepErrorText(err);
      note.hidden = false;
    };

    main.addEventListener("click", () => {
      if (li.classList.contains("busy")) return;
      li.classList.add("busy");
      note.hidden = true;
      hooks.open(item.id).catch((err) => {
        void Bridge.log(`chat: open thread failed ${isRoadeepError(err) ? `${err.code} ${err.requestId ?? ""}` : ""}`);
        fail(err);
      }).finally(() => li.classList.remove("busy"));
    });

    // Delete asks inline first; the confirm row replaces the item at its own
    // height (the list doesn't shift) and keeps the title on screen, so a wrong
    // row is caught before anything is deleted. «لغو» lands where the × was,
    // the destructive button ignores clicks for a moment (a double-click on ×
    // can never delete), and only one confirm is open at a time.
    del.addEventListener("click", () => {
      closeConfirm?.(false);
      note.hidden = true;
      const n = ++confirmSeq;
      const titleId = `hist-del-title-${n}`;
      const askId = `hist-del-ask-${n}`;
      const yes = h("button", { type: "button", class: "btn primary danger", text: t("history.deleteConfirm") });
      const no = h("button", { type: "button", class: "btn secondary", text: t("common.cancel") });
      const confirm = h("div", { class: "hist-confirm", role: "group", "aria-labelledby": `${titleId} ${askId}` },
        h("span", { class: "hist-confirm-title", id: titleId, dir: "auto", text: title }),
        h("div", { class: "hist-confirm-row" },
          h("span", { id: askId, dir: "auto", text: t("history.deleteAsk") }), h("div", { class: "grow" }), yes, no,
        ),
      );
      confirm.style.minHeight = `${li.offsetHeight}px`;
      const armedAt = performance.now() + 350;
      const restore = (focus = true) => {
        if (closeConfirm === restore) closeConfirm = null;
        confirm.remove();
        main.hidden = false;
        del.hidden = false;
        if (focus) main.focus();
      };
      closeConfirm = restore;
      no.addEventListener("click", () => restore());
      confirm.addEventListener("keydown", (e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          restore();
        }
      });
      yes.addEventListener("click", () => {
        if (performance.now() < armedAt) return;
        yes.disabled = true;
        no.disabled = true;
        hooks.remove(item.id).then(
          () => {
            if (closeConfirm === restore) closeConfirm = null;
            items = items.filter((i) => i.id !== item.id);
            const next = (li.nextElementSibling ?? li.previousElementSibling)?.querySelector<HTMLElement>(".hist-open");
            li.remove();
            if (!items.length && !hasMore) setStatus("empty");
            (next ?? newChat).focus();
          },
          (err) => {
            void Bridge.log(`chat: delete thread failed ${isRoadeepError(err) ? `${err.code} ${err.requestId ?? ""}` : ""}`);
            restore();
            fail(err);
          },
        );
      });
      main.hidden = true;
      del.hidden = true;
      li.insertBefore(confirm, note);
      no.focus();
    });
    return li;
  }

  // Arrow keys move between conversations; Escape goes back to the chat.
  el.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hooks.close();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Home" && e.key !== "End") return;
    const buttons = [...list.querySelectorAll<HTMLElement>(".hist-open:not([hidden])")];
    if (!buttons.length) return;
    const i = buttons.indexOf(document.activeElement as HTMLElement);
    let n: number;
    switch (e.key) {
      case "ArrowDown": n = i < 0 ? 0 : Math.min(buttons.length - 1, i + 1); break;
      case "ArrowUp": n = i < 0 ? 0 : Math.max(0, i - 1); break;
      case "Home": n = 0; break;
      default: n = buttons.length - 1;
    }
    e.preventDefault();
    buttons[n].focus();
    buttons[n].scrollIntoView({ block: "nearest" });
  });

  return {
    el,
    show() {
      back.focus();
      void load(true).then(() => {
        // Only if the user hasn't moved on in the meantime.
        if (document.activeElement !== back) return;
        const first = list.querySelector<HTMLElement>(".hist-item.current .hist-open") ??
          list.querySelector<HTMLElement>(".hist-open");
        first?.focus();
      });
    },
  };
}
