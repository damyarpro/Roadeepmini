// Keyboard shortcuts in Settings, the pure part (adapted from upstream's
// core/shortcuts.ts, the Mac's ShortcutLogic.swift): the spelling Rust stores
// and parses (shortcut.rs: "Ctrl+Alt+Shift+Super+<key>", the key named as a
// KeyboardEvent.code), the names printed on the keycaps, what a key press in
// the recorder means, and which action already has a combination.
//
// Whether a combination types a character with AltGr, or is held by another
// app, only Windows knows: the recorder asks Rust (shortcut_check).

import { SHORTCUTS, effectiveBinding, type ShortcutId } from "../core/bridge-shortcuts";
import type { Settings } from "../core/state";

export interface Combo {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  /** The Windows key. */
  meta: boolean;
  /** The key, by its code: "KeyA", "Digit1", "ArrowLeft", "Comma"… */
  key: string;
}

const PUNCTUATION: Record<string, string> = {
  Comma: ",", Period: ".", Slash: "/", Semicolon: ";", Quote: "'", Backquote: "`",
  BracketLeft: "[", BracketRight: "]", Backslash: "\\", Minus: "-", Equal: "=",
};

const NAMED = [
  "Space", "Enter", "Tab", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown",
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
];

/** Every key a shortcut may use, by its code. All of them parse in Rust. */
export const KEY_CODES: readonly string[] = [
  ...[..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"].map((c) => `Key${c}`),
  ...[..."0123456789"].map((d) => `Digit${d}`),
  ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
  ...NAMED,
  ...Object.keys(PUNCTUATION),
];

/** Other spellings Rust accepts, upper-cased. */
const ALIASES: Record<string, string> = {
  UP: "ArrowUp", DOWN: "ArrowDown", LEFT: "ArrowLeft", RIGHT: "ArrowRight",
  ...Object.fromEntries(Object.entries(PUNCTUATION).map(([code, ch]) => [ch, code])),
};

/** The modifier spellings Rust accepts, upper-cased, and a few more. */
const MODIFIERS: Record<string, "ctrl" | "alt" | "shift" | "meta" | undefined> = {
  CTRL: "ctrl", CONTROL: "ctrl", COMMANDORCONTROL: "ctrl", CMDORCTRL: "ctrl",
  ALT: "alt", OPTION: "alt", SHIFT: "shift",
  SUPER: "meta", META: "meta", WIN: "meta", CMD: "meta", COMMAND: "meta",
};

function keyCode(token: string): string | null {
  const up = token.toUpperCase();
  if (/^[A-Z]$/.test(up)) return `Key${up}`;
  if (/^[0-9]$/.test(up)) return `Digit${up}`;
  return ALIASES[up] ?? KEY_CODES.find((k) => k.toUpperCase() === up) ?? null;
}

/**
 * An accelerator → its parts, or null where Rust would refuse it: no Ctrl,
 * Alt or Windows key (it would take that key from every app), an unknown key,
 * or Escape (it ends a recording).
 */
export function parseKeys(text: string): Combo | null {
  const tokens = text.split("+").map((t) => t.trim());
  if (!text.trim() || tokens.some((t) => !t)) return null;
  const combo: Combo = { ctrl: false, alt: false, shift: false, meta: false, key: "" };
  for (const [i, token] of tokens.entries()) {
    const last = i === tokens.length - 1;
    const modifier = MODIFIERS[token.toUpperCase()];
    if (modifier) {
      if (last) return null;
      combo[modifier] = true;
      continue;
    }
    if (!last) return null;
    const key = keyCode(token);
    if (!key) return null;
    combo.key = key;
  }
  return combo.key && (combo.ctrl || combo.alt || combo.meta) ? combo : null;
}

/** Combo → "Ctrl+Alt+Shift+Super+KeyA", the spelling stored and handed to Rust. */
export function formatKeys(c: Combo): string {
  return [c.ctrl && "Ctrl", c.alt && "Alt", c.shift && "Shift", c.meta && "Super", c.key].filter(Boolean).join("+");
}

/** One spelling per combination, or null when it isn't one. */
export function normalizeKeys(text: string): string | null {
  const combo = parseKeys(text);
  return combo ? formatKeys(combo) : null;
}

const GLYPHS: Record<string, string> = {
  ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→", Escape: "Esc", ...PUNCTUATION,
};

/** The names printed on the keys, left to right: "Ctrl+Alt+ArrowRight" → Ctrl, Alt, →. */
export function keyLabels(text: string, win = "Win"): string[] {
  return text.split("+").map((t) => t.trim()).filter(Boolean).map((part) => {
    if (/^(Super|Meta|Win)$/i.test(part)) return win;
    if (/^Key[A-Z]$/.test(part)) return part.slice(3);
    if (/^Digit[0-9]$/.test(part)) return part.slice(5);
    return GLYPHS[part] ?? part;
  });
}

// ── The recorder ──────────────────────────────────────────────────────────────

/** The parts of a KeyboardEvent the recorder reads. */
export interface KeyPress {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type Recorded =
  /** Only modifiers so far: keep listening. */
  | { kind: "pending" }
  | { kind: "cancel" }
  /** Backspace or Delete alone: turn the shortcut off. */
  | { kind: "clear" }
  | { kind: "keys"; keys: string }
  /** Needs Ctrl, Alt or the Windows key. */
  | { kind: "needsModifier" }
  | { kind: "unsupported" };

const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta", "AltGraph", "OS", "Super", "Hyper"]);

/**
 * The key of a press, by code. A Latin letter goes by what it types, so
 * Ctrl+Alt+A is the key labelled A on AZERTY too: Windows registers a letter
 * by its virtual key, which follows the layout. On a layout without Latin
 * letters (Persian) the key goes by its place, as Windows does.
 */
function pressedKey(e: KeyPress): string | null {
  if (/^[a-z]$/i.test(e.key)) return `Key${e.key.toUpperCase()}`;
  if (/^(Key[A-Z]|Digit[0-9]|F([1-9]|1[0-9]|2[0-4]))$/.test(e.code)) return e.code;
  if (NAMED.includes(e.code) || e.code in PUNCTUATION) return e.code;
  if (e.code === "NumpadEnter") return "Enter";
  return null;
}

/** A key press in the recorder → what to do with it. */
export function recordPress(e: KeyPress): Recorded {
  if (MODIFIER_KEYS.has(e.key)) return { kind: "pending" };
  const bare = !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey;
  if (e.key === "Escape" && bare) return { kind: "cancel" };
  if ((e.key === "Backspace" || e.key === "Delete") && bare) return { kind: "clear" };
  const key = pressedKey(e);
  if (!key) return { kind: "unsupported" };
  if (!(e.ctrlKey || e.altKey || e.metaKey)) return { kind: "needsModifier" };
  return { kind: "keys", keys: formatKeys({ ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey, key }) };
}

/** The modifiers held during a press, as an accelerator's start ("Ctrl+Alt"). */
export function heldModifiers(e: KeyPress): string {
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean).join("+");
}

// ── Conflicts ─────────────────────────────────────────────────────────────────

/** The other enabled action already on `keys`, if any. */
export function clashWith(
  id: ShortcutId,
  keys: string,
  settings: Pick<Settings, "shortcut" | "shortcuts">,
): ShortcutId | null {
  const wanted = normalizeKeys(keys);
  if (!wanted) return null;
  for (const d of SHORTCUTS) {
    if (d.id === id) continue;
    const b = effectiveBinding(d, settings);
    if (b.enabled && normalizeKeys(b.keys) === wanted) return d.id;
  }
  return null;
}

// ── Inside the open island ────────────────────────────────────────────────────

/**
 * The island's own keys, shown read-only in Settings. The ones with Ctrl are
 * island/shortcuts.ts islandKeyAction (a test keeps the two in step); Enter is
 * the chat's own, Esc the island's.
 */
export const ISLAND_KEYS: readonly { combos: string[]; join?: "or" | "to"; text: string }[] = [
  { combos: ["Ctrl+ArrowRight", "Ctrl+ArrowLeft"], join: "or", text: "shortcuts.island.cycle" },
  { combos: ["Ctrl+Digit1", "Ctrl+Digit9"], join: "to", text: "shortcuts.island.byNumber" },
  { combos: ["Ctrl+KeyP"], text: "shortcuts.island.pin" },
  { combos: ["Ctrl+Comma"], text: "shortcuts.island.settings" },
  { combos: ["Enter"], text: "shortcuts.island.send" },
  { combos: ["Escape"], text: "shortcuts.island.close" },
];
