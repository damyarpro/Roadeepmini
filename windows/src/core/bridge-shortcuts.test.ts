import { describe, expect, it } from "vitest";
import rust from "../../src-tauri/src/shortcuts.rs?raw";
import { CHAT_SHORTCUT, SHORTCUTS, effectiveBinding } from "./bridge-shortcuts";
import { DEFAULT_SETTINGS } from "./state";

// Rust registers the shortcuts (src-tauri/src/shortcuts.rs ACTIONS); the
// settings window shows the same table. These checks keep the two in step.

describe("global shortcut table", () => {
  it("is the same on both sides of the bridge", () => {
    const rows = [...rust.matchAll(/^\s*action\("(\w+)", "([^"]+)", (true|false)\),/gm)]
      .map(([, id, keys, on]) => ({ id, keys, on: on === "true" }));
    expect(rows).toEqual(SHORTCUTS.map((d) => ({ id: d.id, keys: d.defaultKeys, on: d.enabledByDefault })));
  });

  it("starts with the chat shortcut Roadeep always had, the only one on", () => {
    expect(SHORTCUTS[0].id).toBe(CHAT_SHORTCUT);
    expect(SHORTCUTS[0].defaultKeys).toBe(DEFAULT_SETTINGS.shortcut);
    expect(SHORTCUTS.filter((d) => d.enabledByDefault).map((d) => d.id)).toEqual([CHAT_SHORTCUT]);
    expect(DEFAULT_SETTINGS.shortcuts).toEqual({});
  });

  it("reads the chat from settings.shortcut and the others from settings.shortcuts", () => {
    const chat = SHORTCUTS[0];
    const alert = SHORTCUTS.find((d) => d.id === "goToAlert")!;
    expect(effectiveBinding(chat, { shortcut: "Ctrl+Alt+KeyK", shortcuts: {} })).toEqual({ keys: "Ctrl+Alt+KeyK", enabled: true });
    expect(effectiveBinding(chat, { shortcut: "", shortcuts: { openChat: { keys: "Ctrl+Alt+KeyQ", enabled: true } } }))
      .toEqual({ keys: "", enabled: false });
    expect(effectiveBinding(alert, { shortcut: "", shortcuts: {} })).toEqual({ keys: "Ctrl+Alt+KeyA", enabled: false });
    expect(effectiveBinding(alert, { shortcut: "", shortcuts: { goToAlert: { keys: "Ctrl+Shift+KeyG", enabled: true } } }))
      .toEqual({ keys: "Ctrl+Shift+KeyG", enabled: true });
  });
});
