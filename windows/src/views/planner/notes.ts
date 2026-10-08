// «یادداشت‌ها»: a quick field (Enter saves, Shift+Enter starts a new line),
// the notes newest first with the pinned ones on top, and per note: pin, copy,
// edit, delete. The text is selectable, so part of it can be copied too.

import { h } from "../dom";
import type { ViewActions, ViewHost } from "../views";
import { Bridge } from "../../core/bridge";
import { PLANNER_LIMITS, PlannerBridge, type PlannerNote } from "../../core/bridge-planner";
import { Sound } from "../../core/sound";
import { isRtl, textDirection, t } from "../../core/i18n";
import { openPalette } from "../palette";
import { Planner, attempt } from "./store";
import { fmtAgo } from "./time";
import { emptyState, icon, iconButton, limitField, plannerShell, rerender, stopEscape } from "./ui";

export function buildNotes(actions: ViewActions): ViewHost {
  const shell = plannerShell({ icon: "notes", title: t("planner.notes.title"), onMenu: () => openPalette(actions) });

  const field = h("textarea", {
    class: "pl-input pl-textarea",
    rows: "1",
    placeholder: t("planner.notes.placeholder"),
    "aria-label": t("planner.notes.new"),
    spellcheck: "false",
  }) as HTMLTextAreaElement;
  limitField(field, PLANNER_LIMITS.noteText);
  /** Grows with the text up to four lines, then scrolls. */
  const fit = (el: HTMLTextAreaElement) => {
    el.style.height = "auto";
    const full = el.scrollHeight + 2;
    el.style.height = `${Math.min(full, 96)}px`;
    el.style.overflowY = full > 96 ? "auto" : "hidden";
  };
  field.addEventListener("input", () => {
    field.dir = textDirection(field.value) ?? (isRtl() ? "rtl" : "ltr");
    fit(field);
  });
  const saveBtn = iconButton(icon("plus", 14), t("planner.notes.save"), () => void add(), "pl-add");
  const addRow = h("div", { class: "pl-add-row top" }, field, saveBtn);
  const list = h("ul", { class: "pl-list pl-notes", "aria-label": t("planner.notes.title") });
  const status = h("div", { class: "sr-only", "aria-live": "polite" });
  shell.body.append(addRow, h("div", { class: "pl-scroll" }, list), status);

  let editing: string | null = null;
  let renderedKey = "";

  stopEscape(shell.el, () => {
    if (!editing) return false;
    const id = editing;
    editing = null;
    renderedKey = "";
    sync();
    list.querySelector<HTMLElement>(`[data-fk="edit:${id}"]`)?.focus();
    return true;
  });

  const enterSaves = (el: HTMLTextAreaElement, save: () => void) =>
    el.addEventListener("keydown", (e) => {
      const k = e as KeyboardEvent;
      if (k.key === "Enter" && !k.shiftKey && !k.isComposing) {
        e.preventDefault();
        save();
      }
      if (k.key !== "Escape") e.stopPropagation();
    });
  enterSaves(field, () => void add());

  async function add() {
    const text = field.value.trim();
    if (!text) {
      field.focus();
      return;
    }
    saveBtn.disabled = true;
    const r = await attempt(() => PlannerBridge.noteAdd(text));
    saveBtn.disabled = false;
    if (!r.ok) {
      shell.setError(r.error);
      return;
    }
    shell.setError(null);
    field.value = "";
    fit(field);
    Sound.play("pop");
    field.focus();
  }

  async function run(p: () => Promise<unknown>) {
    const r = await attempt(p);
    if (!r.ok) shell.setError(r.error);
    renderedKey = "";
    sync();
  }

  async function copy(note: PlannerNote) {
    try {
      await navigator.clipboard.writeText(note.text);
      status.textContent = t("planner.notes.copied");
      Sound.play("blip");
    } catch {
      void Bridge.log("planner: note copy failed");
      shell.setError(t("planner.notes.copyFailed"));
    }
  }

  function card(note: PlannerNote, now: number): HTMLElement {
    const tools = h("div", { class: "pl-note-tools" });
    const pin = iconButton(icon("pin", 13), t(note.pinned ? "planner.notes.unpin" : "planner.notes.pin"),
      () => void run(() => PlannerBridge.noteUpdate(note.id, { pinned: !note.pinned })), note.pinned ? "on" : "");
    pin.dataset.fk = `pin:${note.id}`;
    pin.setAttribute("aria-pressed", String(note.pinned));
    const copyBtn = iconButton(icon("copy", 13), t("planner.notes.copy"), () => void copy(note));
    copyBtn.dataset.fk = `copy:${note.id}`;
    const edit = iconButton(icon("edit", 13), t("planner.notes.edit"), () => {
      editing = note.id;
      renderedKey = "";
      sync();
      const area = list.querySelector<HTMLTextAreaElement>(`[data-fk="area:${note.id}"]`);
      area?.focus();
      area?.setSelectionRange(area.value.length, area.value.length);
    });
    edit.dataset.fk = `edit:${note.id}`;
    const del = iconButton(icon("trash", 13), t("planner.delete"), () => void run(() => PlannerBridge.noteDelete(note.id)), "danger");
    del.dataset.fk = `del:${note.id}`;
    tools.append(pin, copyBtn, edit, del);

    const meta = h("span", { class: "pl-meta" },
      note.pinned ? h("span", { class: "pl-pinned", text: t("planner.notes.pinned") }) : null,
      h("span", { text: fmtAgo(note.updatedAt, now) }),
    );

    if (editing === note.id) {
      const area = h("textarea", { class: "pl-input pl-textarea", rows: "3", dir: "auto", "aria-label": t("planner.notes.edit") }) as HTMLTextAreaElement;
      area.value = note.text;
      area.dataset.fk = `area:${note.id}`;
      limitField(area, PLANNER_LIMITS.noteText);
      const save = () => {
        const text = area.value.trim();
        editing = null;
        if (text && text !== note.text) void run(() => PlannerBridge.noteUpdate(note.id, { text }));
        else {
          renderedKey = "";
          sync();
        }
      };
      enterSaves(area, save);
      return h("li", { class: "pl-note-card editing" }, area,
        h("div", { class: "pl-note-foot" },
          h("span", { class: "pl-meta", text: t("planner.notes.editHint") }),
          h("div", { class: "grow" }),
          h("button", { type: "button", class: "pl-chip", text: t("planner.cancel"), onclick: () => {
            editing = null;
            renderedKey = "";
            sync();
          } }),
          h("button", { type: "button", class: "pl-chip primary", text: t("planner.save"), onclick: save }),
        ));
    }

    return h("li", { class: `pl-note-card${note.pinned ? " pinned" : ""}` },
      h("p", { class: "pl-note-text", dir: "auto", text: note.text }),
      h("div", { class: "pl-note-foot" }, meta, h("div", { class: "grow" }), tools),
    );
  }

  function sync() {
    Planner.ensure();
    const data = Planner.data;
    const key = `${Planner.revision}|${editing}|${Planner.error ?? ""}`;
    if (key === renderedKey) return;
    renderedKey = key;
    if (!data) {
      rerender(list, [h("li", { class: "pl-note", text: Planner.error ?? t("planner.loading") })]);
      return;
    }
    const notes = [...data.notes].sort((a, b) =>
      a.pinned !== b.pinned ? (a.pinned ? -1 : 1) : b.createdAt - a.createdAt);
    const now = Date.now();
    rerender(list, notes.length
      ? notes.map((n) => card(n, now))
      : [h("li", { class: "pl-empty-li" }, emptyState(t("planner.notes.empty")))]);
  }

  return {
    el: shell.el,
    sync,
    focus() {
      field.focus();
    },
  };
}
