// Settings → Shortcuts (adapted from upstream's): each global shortcut can be
// turned on or off and recorded anew, with what Windows made of it, and the
// keys the open island answers to are listed. Rust registers the shortcuts
// (src-tauri/src/shortcuts.rs) and reports how each one went.
//
// The chat shortcut is the first row; its keys are settings.shortcut, as they
// always were. The others are settings.shortcuts.

import "./shortcuts.css";
import { Bridge, onEvent } from "../core/bridge";
import {
  BridgeShortcuts, CHAT_SHORTCUT, SHORTCUTS, SHORTCUTS_STATUS_EVENT, effectiveBinding,
  type Binding, type ShortcutDef, type ShortcutsReport,
} from "../core/bridge-shortcuts";
import { localizeError } from "../core/error-text";
import { registerMessages, t } from "../core/i18n";
import { r3En } from "../core/locales/r3-en";
import { r3Fa } from "../core/locales/r3-fa";
import type { Settings } from "../core/state";
import { clear, h } from "../views/dom";
import { ISLAND_KEYS, clashWith, heldModifiers, keyLabels, normalizeKeys, recordPress } from "./shortcut-keys";
import { helpDisclosure, linkButton, sectionHead, settingRow, statusBadge, switchEl, type StatusKind } from "./ui";

registerMessages(r3En, r3Fa);

export interface ShortcutsHost {
  settings(): Settings;
  save(): Promise<void>;
}

const actionName = (id: string) => t(`shortcuts.action.${id}`);
const winKey = () => t("misc.keyWin");

/** Keycaps Ctrl + Alt + R, always left to right. `pending`: keys still to come. */
function keycaps(keys: string, pending = false): HTMLElement {
  const box = h("span", { class: "keycaps", dir: "ltr" });
  const names = keyLabels(keys, winKey());
  names.forEach((name, i) => {
    if (i > 0) box.append(h("span", { class: "keycap-plus", "aria-hidden": "true", text: "+" }));
    box.append(h("kbd", { text: name }));
  });
  if (pending) box.append(h("span", { class: "keycap-plus", "aria-hidden": "true", text: names.length ? "+ …" : "…" }));
  return box;
}

const spoken = (keys: string) => keyLabels(keys, winKey()).join(" + ");

/** The last report from Rust, and the section on screen (rebuilt on a language change). */
let report: ShortcutsReport | null = null;
let onScreen: { sync(): void } | null = null;
let listening = false;
/** Keys an action had before it was turned off, so turning it on brings them back. */
const lastKeys = new Map<string, string>();

/** Redraws from the settings and the report, unless a recording is under way. */
export function refreshShortcutsSection() {
  onScreen?.sync();
}

/** Listens for Rust's reports once per window, whatever redraws the section. */
export function initShortcutsSection() {
  if (listening) return;
  listening = true;
  void onEvent<ShortcutsReport>(SHORTCUTS_STATUS_EVENT, (fresh) => {
    report = fresh;
    onScreen?.sync();
  });
  void BridgeShortcuts.status().then((fresh) => {
    if (!fresh) return;
    report = fresh;
    onScreen?.sync();
  });
}

/** The badge for an action: what Rust reported for the keys it has now. */
function badgeFor(d: ShortcutDef, binding: Binding, settings: Settings): [StatusKind, string] | null {
  if (!binding.enabled || !binding.keys) return ["off", t("shortcuts.status.off")];
  if (report?.suspended) return ["off", t("shortcuts.status.paused")];
  const entry = report?.actions.find((a) => a.id === d.id);
  // A report about other keys is stale: the next one is on its way.
  if (!entry || normalizeKeys(entry.keys) !== normalizeKeys(binding.keys)) return null;
  switch (entry.status) {
    case "active":
      return ["ok", t("shortcuts.status.active")];
    case "off":
      return ["off", t("shortcuts.status.off")];
    case "inUse":
      return ["err", t("shortcuts.status.inUse")];
    case "duplicate": {
      const holder = clashWith(d.id, binding.keys, settings);
      return ["err", holder ? t("shortcuts.status.duplicate", { name: actionName(holder) }) : t("shortcuts.status.invalid")];
    }
    case "invalid":
      return ["err", entry.error ? localizeError(entry.error) : t("shortcuts.status.invalid")];
    case "typesCharacter":
      return ["warn", t("shortcuts.status.typesCharacter", { char: entry.typed ?? "?" })];
    case "unavailable":
      return ["warn", t("shortcuts.status.unavailable")];
  }
}

export function shortcutsSection(host: ShortcutsHost): HTMLElement {
  initShortcutsSection();
  const rows: { sync(): void; stop(): void }[] = [];
  const live = h("span", { class: "sr-only", role: "status", "aria-live": "polite" });

  /** Writes one action's binding: the chat's into settings.shortcut, the others into settings.shortcuts. */
  async function store(d: ShortcutDef, keys: string, enabled: boolean) {
    const s = host.settings();
    if (d.id === CHAT_SHORTCUT) s.shortcut = enabled ? keys : "";
    else s.shortcuts = { ...s.shortcuts, [d.id]: { keys, enabled } };
    await host.save();
  }

  function row(d: ShortcutDef) {
    const id = `sc-${d.id}`;
    const name = actionName(d.id);
    const badgeSlot = h("span", { class: "sec-badge" });
    const hint = h("span", { class: "set-hint", id: `${id}-hint`, hidden: true });
    const message = h("div", { class: "field-err", role: "alert", dir: "auto" });
    const recorder = h("button", {
      type: "button", class: "shortcut-rec", id, "aria-describedby": `${id}-hint`,
    }) as HTMLButtonElement;
    let recording = false;
    let el: HTMLElement | null = null;
    const binding = () => effectiveBinding(d, host.settings());
    const toggle = switchEl(binding().enabled, false, name, (on) => void setEnabled(on));

    function sync() {
      if (recording) return;
      const b = binding();
      el?.classList.toggle("off", !b.enabled);
      toggle.classList.toggle("on", b.enabled);
      toggle.setAttribute("aria-checked", b.enabled ? "true" : "false");
      recorder.classList.remove("recording");
      recorder.setAttribute("aria-pressed", "false");
      hint.hidden = true;
      hint.textContent = "";
      clear(recorder);
      // An action that is off still shows the keys it would take.
      recorder.append(b.keys ? keycaps(b.keys) : h("span", { text: t("shortcuts.none") }));
      recorder.setAttribute("aria-label", t("shortcuts.recordLabel", { name, combo: b.keys ? spoken(b.keys) : t("shortcuts.none") }));
      clear(badgeSlot);
      const badge = badgeFor(d, b, host.settings());
      if (badge) badgeSlot.append(statusBadge(badge[0], badge[1]));
    }

    function start() {
      if (recording) return;
      for (const other of rows) other.stop();
      message.textContent = "";
      recording = true;
      recorder.classList.add("recording");
      recorder.setAttribute("aria-pressed", "true");
      hint.hidden = false;
      hint.textContent = t("shortcuts.recordingHint");
      clear(recorder);
      recorder.append(h("span", { text: t("shortcuts.recording") }));
      recorder.setAttribute("aria-label", t("shortcuts.recording"));
      // Ours let go, so a combination Roadeep holds reaches the recorder.
      void BridgeShortcuts.suspend(true);
    }

    function stop() {
      if (!recording) return;
      recording = false;
      void BridgeShortcuts.suspend(false);
      sync();
    }

    function refuse(text: string, why: string) {
      message.textContent = text;
      stop();
      void Bridge.log(`settings: shortcut ${d.id} refused (${why})`);
    }

    async function take(keys: string) {
      const other = clashWith(d.id, keys, host.settings());
      if (other) return refuse(t("shortcuts.clash", { combo: spoken(keys), name: actionName(other) }), "clash");
      const check = await BridgeShortcuts.check(keys);
      if (!recording) return;
      if (check && !check.ok) {
        if (check.typed) return refuse(t("shortcuts.typesNote", { combo: spoken(keys), char: check.typed }), "types");
        return refuse(check.error ? localizeError(check.error) : t("err.shortcut.invalid"), "check");
      }
      // Outside the app there is nothing to check against: keep what was typed.
      const stored = check?.accelerator || keys;
      message.textContent = "";
      recording = false;
      await store(d, stored, true);
      void BridgeShortcuts.suspend(false);
      sync();
      live.textContent = t("shortcuts.saved", { combo: spoken(stored) });
    }

    async function setEnabled(on: boolean) {
      const b = binding();
      message.textContent = "";
      if (!on) {
        if (b.keys) lastKeys.set(d.id, b.keys);
        await store(d, b.keys, false);
        sync();
        live.textContent = t("shortcuts.turnedOff", { name });
        return;
      }
      const keys = b.keys || lastKeys.get(d.id) || d.defaultKeys;
      const other = clashWith(d.id, keys, host.settings());
      if (other) {
        message.textContent = t("shortcuts.clash", { combo: spoken(keys), name: actionName(other) });
        sync();
        return;
      }
      await store(d, keys, true);
      sync();
      live.textContent = t("shortcuts.saved", { combo: spoken(keys) });
    }

    recorder.addEventListener("click", start);
    recorder.addEventListener("blur", stop);
    recorder.addEventListener("keydown", (e) => {
      if (!recording) return;
      // Tab still moves on: the recording ends with the focus leaving.
      if (e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey) return;
      e.preventDefault();
      e.stopPropagation();
      const result = recordPress(e);
      switch (result.kind) {
        case "pending":
          clear(recorder);
          recorder.append(keycaps(heldModifiers(e), true));
          return;
        case "cancel":
          stop();
          return;
        case "clear":
          recording = false;
          void BridgeShortcuts.suspend(false);
          void setEnabled(false);
          return;
        case "needsModifier":
          message.textContent = t("shortcuts.needsModifier");
          return;
        case "unsupported":
          message.textContent = t("shortcuts.unsupportedKey");
          return;
        case "keys":
          void take(result.keys);
          return;
      }
    });

    el = settingRow(
      { label: name, forId: id, class: "sc-row", extra: h("div", {}, hint, message) },
      badgeSlot,
      toggle,
      recorder,
    );
    sync();
    rows.push({ sync, stop });
    return el;
  }

  const reset = linkButton(t("shortcuts.reset"), () => {
    for (const r of rows) r.stop();
    const s = host.settings();
    s.shortcut = SHORTCUTS[0].defaultKeys;
    s.shortcuts = {};
    lastKeys.clear();
    void host.save().then(() => {
      for (const r of rows) r.sync();
      live.textContent = t("shortcuts.resetDone");
    });
  });

  const islandRows = ISLAND_KEYS.map(({ combos, join, text }) => {
    const keys = h("span", { class: "sc-island-keys" });
    combos.forEach((combo, i) => {
      if (i > 0) keys.append(h("span", { class: "sc-join", text: t(join === "to" ? "shortcuts.to" : "shortcuts.or") }));
      keys.append(keycaps(combo));
    });
    return settingRow({ label: t(text) }, keys);
  });

  const globalRows = SHORTCUTS.map(row);
  onScreen = {
    sync() {
      for (const r of rows) r.sync();
    },
  };

  return h("section", { class: "sec sc", "aria-labelledby": "sec-shortcuts-title" },
    sectionHead({ id: "sec-shortcuts-title", icon: "keyboard", title: t("shortcuts.title"), desc: t("shortcuts.desc") }),
    h("div", { class: "card list", role: "group", "aria-labelledby": "sc-global-head" },
      h("div", { class: "card-head" },
        h("h3", { id: "sc-global-head", text: t("shortcuts.globalHead") }),
        helpDisclosure(t("shortcuts.globalHint"), `${t("settings.help")}: ${t("shortcuts.globalHead")}`),
      ),
      ...globalRows,
    ),
    h("div", { class: "sc-actions" }, reset, live),
    h("div", { class: "card list", role: "group", "aria-labelledby": "sc-island-head" },
      h("div", { class: "card-head" },
        h("h3", { id: "sc-island-head", text: t("shortcuts.islandHead") }),
        helpDisclosure(t("shortcuts.islandHint"), `${t("settings.help")}: ${t("shortcuts.islandHead")}`),
      ),
      ...islandRows,
    ),
  );
}
