// What the island does with the global shortcuts (Settings → Shortcuts) and
// with the keys pressed while it has the keyboard. Adapted from upstream's
// island/shortcuts.ts (the Mac's IslandWindowController.handleHotKey and
// handleIslandKey); Ctrl stands in for the Mac's ⌘.
//
// Rust (src-tauri/src/shortcuts.rs) registers the global shortcuts. The chat's
// still comes as it always did (tray → "chat", handled in main.ts); the others
// arrive as a `shortcut` event carrying the action id. The in-island keys are
// read here while the island has the keyboard: in the chat and the other views
// made for typing (island.ts KEYBOARD_VIEWS).

import { Bridge, onEvent } from "../core/bridge";
import { BridgeShortcuts, SHORTCUT_EVENT } from "../core/bridge-shortcuts";
import type { BotEmoteName, IslandViewName } from "../core/layout";
import { Sound } from "../core/sound";
import { State } from "../core/state";

/** The Claude Code pill, which holds an approval without a task of its own. */
const CLAUDE_TASK = "integration_claude";

/** What the shortcuts need from the island. */
export interface ShortcutHost {
  alert(view: IslandViewName): void;
  setView(view: IslandViewName): void;
  collapse(): void;
  /** The character reacts (Island.emote). */
  emote(name: BotEmoteName): void;
  /** Ctrl+P: keep the open island from folding away, or let it fold again (Island.setPinned). */
  setPinned(on: boolean): void;
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

/** The pill `delta` steps from `current`, wrapping around. */
export function cyclePill(ids: readonly string[], current: string | null, delta: number): string | null {
  if (ids.length === 0) return null;
  const at = Math.max(0, current == null ? 0 : ids.indexOf(current));
  return ids[(((at + delta) % ids.length) + ids.length) % ids.length];
}

/** Pill number `n`, counting from 1. */
export function pillByNumber(ids: readonly string[], n: number): string | null {
  return n >= 1 && n <= ids.length ? ids[n - 1] : null;
}

export type IslandKeyAction =
  | { kind: "cycle"; delta: 1 | -1 }
  | { kind: "pill"; number: number }
  | { kind: "settings" }
  | { kind: "pin" };

/** The parts of a KeyboardEvent the island reads. */
export interface IslandKeyPress {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/**
 * A key pressed while the island has the keyboard → what it does, or null to
 * leave it alone. In a text field Ctrl+← and Ctrl+→ keep moving by word.
 * Settings lists these (settings/shortcut-keys.ts ISLAND_KEYS).
 */
export function islandKeyAction(e: IslandKeyPress, ctx: { inTextField: boolean }): IslandKeyAction | null {
  if (!e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return null;
  if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
    if (ctx.inTextField) return null;
    return { kind: "cycle", delta: e.key === "ArrowRight" ? 1 : -1 };
  }
  const digit = /^Digit([1-9])$/.exec(e.code);
  if (digit) return { kind: "pill", number: Number(digit[1]) };
  const letter = /^[a-z]$/i.test(e.key) ? e.key.toLowerCase() : /^Key([A-Z])$/.exec(e.code)?.[1].toLowerCase();
  if (letter === "p") return { kind: "pin" };
  if (e.key === "," || e.code === "Comma") return { kind: "settings" };
  return null;
}

// ── Actions ───────────────────────────────────────────────────────────────────

function focusPill(host: ShortcutHost, id: string | null, open: boolean) {
  if (!id) return;
  State.setFocus(id);
  Sound.play("blip");
  if (open) host.alert("overview");
  else host.setView("overview");
}

/**
 * A global shortcut other than the chat's was pressed. `resume` lifts Pause,
 * as the tray's Open does.
 */
export function runGlobalShortcut(host: ShortcutHost, action: string, resume: () => void) {
  switch (action) {
    case "toggleIsland":
      if (State.mode === "expanded") {
        host.collapse();
      } else {
        resume();
        host.alert(State.defaultView());
      }
      break;

    case "goToAlert": {
      const pending = State.pendingApproval;
      const asking = State.tasks.find((t) => t.state === "question");
      if (pending) {
        // Its pill to the front and its card up. Nothing is answered from here.
        resume();
        State.setFocus(pending.taskId ?? CLAUDE_TASK);
        host.alert("approval");
      } else if (asking) {
        resume();
        State.setFocus(asking.id);
        host.alert("question");
      } else {
        // Nothing is waiting: the character says so.
        host.emote("annoyed");
        Sound.play("error");
      }
      break;
    }

    // The session's own window when it was found, else its folder in VS Code
    // (the island's "Open terminal").
    case "jumpToTerminal": {
      const task = State.focusTask;
      void BridgeShortcuts.openSession(task?.sessionId ?? null, task?.sessionCwd ?? null);
      if (State.mode === "expanded") host.collapse();
      break;
    }

    case "nextPill":
    case "prevPill":
      resume();
      focusPill(
        host,
        cyclePill(State.tasks.map((t) => t.id), State.focusTask?.id ?? null, action === "nextPill" ? 1 : -1),
        true,
      );
      break;

    case "muteToggle": {
      const on = !State.settings.soundEnabled;
      State.settings.soundEnabled = on;
      Sound.setEnabled(on);
      void Bridge.saveSettings(State.settings);
      if (on) Sound.play("tick");
      host.emote(on ? "happy" : "annoyed");
      State.notify();
      break;
    }

    default:
      void Bridge.log(`island: unknown shortcut ${action.slice(0, 40)}`);
      break;
  }
}

/** A key the island acts on while it has the keyboard. */
export function runIslandKey(host: ShortcutHost, action: IslandKeyAction) {
  const ids = State.tasks.map((t) => t.id);
  switch (action.kind) {
    case "cycle":
      focusPill(host, cyclePill(ids, State.focusTask?.id ?? null, action.delta), false);
      break;
    case "pill":
      focusPill(host, pillByNumber(ids, action.number), false);
      break;
    case "settings":
      void Bridge.openSettingsWindow();
      break;
    case "pin":
      // A permission card keeps the island pinned until it is answered.
      if (State.pendingApproval) return;
      host.setPinned(!State.isPinned);
      break;
  }
}

function inTextField(target: EventTarget | null): boolean {
  const el = target as { tagName?: string; isContentEditable?: boolean } | null;
  return el?.tagName === "INPUT" || el?.tagName === "TEXTAREA" || el?.isContentEditable === true;
}

/** Wires both kinds of shortcut to the island (main.ts, once). */
export function registerShortcutHandlers(host: ShortcutHost, resume: () => void) {
  void onEvent<string>(SHORTCUT_EVENT, (action) => runGlobalShortcut(host, action, resume));

  // Capture phase: the chat field stops its own key events from bubbling.
  window.addEventListener(
    "keydown",
    (e: KeyboardEvent) => {
      if (State.mode !== "expanded") return;
      const action = islandKeyAction(e, { inTextField: inTextField(e.target) });
      if (!action) return;
      e.preventDefault();
      e.stopPropagation();
      runIslandKey(host, action);
    },
    true,
  );
}
