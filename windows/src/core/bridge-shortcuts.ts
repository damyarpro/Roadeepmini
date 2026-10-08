// Tauri commands for the global shortcuts (src-tauri/src/shortcuts.rs) and for
// "Open terminal" (src-tauri/src/shortcuts/session_window.rs), with what both
// sides share: the actions, their default keys and the state Rust reports.

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI } from "./bridge";
import type { Settings } from "./state";

/** The chat shortcut. Its keys are `settings.shortcut` ("" = off), not `settings.shortcuts`. */
export const CHAT_SHORTCUT = "openChat";

/** Stored in settings.json: never rename one. */
export type ShortcutId =
  | "openChat"
  | "toggleIsland"
  | "goToAlert"
  | "jumpToTerminal"
  | "nextPill"
  | "prevPill"
  | "muteToggle";

export interface ShortcutDef {
  id: ShortcutId;
  /** In Rust's spelling: "Ctrl+Alt+KeyR" (see settings/shortcut-keys.ts). */
  defaultKeys: string;
  enabledByDefault: boolean;
}

const def = (id: ShortcutId, defaultKeys: string, enabledByDefault: boolean): ShortcutDef =>
  ({ id, defaultKeys, enabledByDefault });

/**
 * Same order and defaults as ACTIONS in src-tauri/src/shortcuts.rs (a test
 * reads that file). Only the chat is on until the user turns another on: a
 * global shortcut takes its keys away from every other app.
 */
export const SHORTCUTS: readonly ShortcutDef[] = [
  def("openChat", "Ctrl+Alt+KeyR", true),
  def("toggleIsland", "Ctrl+Alt+KeyN", false),
  def("goToAlert", "Ctrl+Alt+KeyA", false),
  def("jumpToTerminal", "Ctrl+Alt+KeyT", false),
  def("nextPill", "Ctrl+Alt+ArrowRight", false),
  def("prevPill", "Ctrl+Alt+ArrowLeft", false),
  def("muteToggle", "Ctrl+Alt+KeyS", false),
];

export interface Binding {
  /** The keys, "" for none. */
  keys: string;
  enabled: boolean;
}

/** `settings.shortcuts`: the actions the user changed, by id. */
export type Bindings = Record<string, Binding>;

/** The binding in force: the chat's from `settings.shortcut`, the others as stored or by default. */
export function effectiveBinding(d: ShortcutDef, settings: Pick<Settings, "shortcut" | "shortcuts">): Binding {
  if (d.id === CHAT_SHORTCUT) {
    const keys = (settings.shortcut ?? "").trim();
    return { keys, enabled: keys !== "" };
  }
  const own = settings.shortcuts?.[d.id];
  return own ? { keys: own.keys, enabled: own.enabled } : { keys: d.defaultKeys, enabled: d.enabledByDefault };
}

export type ShortcutState =
  | "active" | "off" | "inUse" | "duplicate" | "invalid" | "typesCharacter" | "unavailable";

export interface ShortcutStatusEntry {
  id: string;
  /** The keys this is about, "" for none. */
  keys: string;
  status: ShortcutState;
  /** What a `typesCharacter` combination types. */
  typed?: string;
  /** An error code (core/error-text.ts), for `inUse` and `invalid`. */
  error?: string;
}

export interface ShortcutsReport {
  actions: ShortcutStatusEntry[];
  /** Settings is recording a combination: nothing is registered meanwhile. */
  suspended: boolean;
}

export interface ShortcutCheck {
  ok: boolean;
  /** The spelling to store, when the combination parses. */
  accelerator: string;
  error: string | null;
  /** What it types on an installed keyboard layout (Ctrl+Alt is AltGr there). */
  typed: string | null;
}

/** Rust's report after every change, to every window. */
export const SHORTCUTS_STATUS_EVENT = "shortcuts-status";
/** A global shortcut other than the chat's, to the island: its action id. */
export const SHORTCUT_EVENT = "shortcut";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!IS_TAURI) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    console.error(`[roadeep] ${cmd} failed`, err);
    void Bridge.log(`${cmd} failed: ${String(err)}`);
    return null;
  }
}

export const BridgeShortcuts = {
  /** How each global shortcut went the last time they were registered. */
  status: () => call<ShortcutsReport>("shortcuts_status"),
  /** Lets go of every global shortcut while Settings records one; false takes them back. */
  suspend: (suspended: boolean) => call<void>("shortcuts_suspend", { suspended }),
  /** Is this combination usable: a shortcut, free, and typing no character? */
  check: (accelerator: string) => call<ShortcutCheck>("shortcut_check", { accelerator }),
  /** "Open terminal": the session's own window when it was found, else its folder in VS Code. */
  openSession: (sessionId: string | null, path: string | null) =>
    call<boolean>("open_session", { sessionId, path }),
};
