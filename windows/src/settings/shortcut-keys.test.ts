import { describe, expect, it } from "vitest";
import { SHORTCUTS } from "../core/bridge-shortcuts";
import {
  KEY_CODES, clashWith, formatKeys, heldModifiers, keyLabels, normalizeKeys, parseKeys, recordPress,
  type KeyPress,
} from "./shortcut-keys";

const press = (key: string, code: string, mods: Partial<KeyPress> = {}): KeyPress =>
  ({ key, code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });
const ctrlAlt = { ctrlKey: true, altKey: true };

describe("accelerators", () => {
  it("parse in the spellings Rust accepts and come out in the one it stores", () => {
    expect(parseKeys("Ctrl+Alt+KeyR")).toEqual({ ctrl: true, alt: true, shift: false, meta: false, key: "KeyR" });
    expect(normalizeKeys("alt+control+r")).toBe("Ctrl+Alt+KeyR");
    expect(normalizeKeys("Super+Shift+Alt+Ctrl+k")).toBe("Ctrl+Alt+Shift+Super+KeyK");
    expect(normalizeKeys("ctrl+alt+right")).toBe("Ctrl+Alt+ArrowRight");
    expect(normalizeKeys("Ctrl+Alt+7")).toBe("Ctrl+Alt+Digit7");
    expect(normalizeKeys("Ctrl+Alt+]")).toBe("Ctrl+Alt+BracketRight");
    expect(normalizeKeys("Win+Space")).toBe("Super+Space");
  });

  it("need Ctrl, Alt or the Windows key and one known key", () => {
    for (const bad of ["", "A", "Shift+A", "Ctrl+Alt", "Ctrl+Alt+", "Ctrl+Alt+Nope", "A+Ctrl", "Ctrl+A+B", "Ctrl+Escape", "Ctrl+Numpad1"]) {
      expect(parseKeys(bad), bad).toBeNull();
    }
  });

  it("round-trip every key a shortcut may use, and every default is already in that form", () => {
    for (const code of KEY_CODES) expect(normalizeKeys(`Ctrl+Alt+${code}`)).toBe(`Ctrl+Alt+${code}`);
    for (const d of SHORTCUTS) expect(normalizeKeys(d.defaultKeys), d.id).toBe(d.defaultKeys);
  });

  it("print as the keys read, left to right", () => {
    expect(keyLabels("Ctrl+Alt+KeyR")).toEqual(["Ctrl", "Alt", "R"]);
    expect(keyLabels("Ctrl+Alt+ArrowRight")).toEqual(["Ctrl", "Alt", "→"]);
    expect(keyLabels("Super+Comma", "Win")).toEqual(["Win", ","]);
    expect(keyLabels("Ctrl+Digit1")).toEqual(["Ctrl", "1"]);
    expect(keyLabels("Escape")).toEqual(["Esc"]);
    expect(keyLabels("Ctrl+Alt+Space")).toEqual(["Ctrl", "Alt", "Space"]);
    expect(formatKeys({ ctrl: true, alt: false, shift: true, meta: false, key: "F5" })).toBe("Ctrl+Shift+F5");
  });
});

describe("the recorder", () => {
  it("waits through modifiers, cancels on Esc and turns off on Backspace", () => {
    expect(recordPress(press("Control", "ControlLeft", { ctrlKey: true }))).toEqual({ kind: "pending" });
    expect(recordPress(press("Alt", "AltLeft", ctrlAlt))).toEqual({ kind: "pending" });
    expect(recordPress(press("Escape", "Escape"))).toEqual({ kind: "cancel" });
    expect(recordPress(press("Backspace", "Backspace"))).toEqual({ kind: "clear" });
    expect(recordPress(press("Delete", "Delete"))).toEqual({ kind: "clear" });
    expect(heldModifiers(press("Alt", "AltLeft", ctrlAlt))).toBe("Ctrl+Alt");
  });

  it("asks for a modifier and refuses keys a shortcut can't use", () => {
    expect(recordPress(press("k", "KeyK"))).toEqual({ kind: "needsModifier" });
    expect(recordPress(press("K", "KeyK", { shiftKey: true }))).toEqual({ kind: "needsModifier" });
    expect(recordPress(press("1", "Numpad1", ctrlAlt))).toEqual({ kind: "unsupported" });
    expect(recordPress(press("Escape", "Escape", ctrlAlt))).toEqual({ kind: "unsupported" });
  });

  it("takes a Latin letter by what it types, as Windows registers it", () => {
    // AZERTY: the key labelled A sits where QWERTY has Q.
    expect(recordPress(press("a", "KeyQ", ctrlAlt))).toEqual({ kind: "keys", keys: "Ctrl+Alt+KeyA" });
    expect(recordPress(press("R", "KeyR", { ...ctrlAlt, shiftKey: true }))).toEqual({ kind: "keys", keys: "Ctrl+Alt+Shift+KeyR" });
  });

  it("takes a key by its place on a layout without Latin letters, or when AltGr typed something", () => {
    // Persian: the R key types ق.
    expect(recordPress(press("ق", "KeyR", ctrlAlt))).toEqual({ kind: "keys", keys: "Ctrl+Alt+KeyR" });
    // French: AltGr+E types €; Rust's check is what refuses it.
    expect(recordPress(press("€", "KeyE", ctrlAlt))).toEqual({ kind: "keys", keys: "Ctrl+Alt+KeyE" });
    expect(recordPress(press("ArrowLeft", "ArrowLeft", ctrlAlt))).toEqual({ kind: "keys", keys: "Ctrl+Alt+ArrowLeft" });
    expect(recordPress(press(",", "Comma", { metaKey: true }))).toEqual({ kind: "keys", keys: "Super+Comma" });
    expect(recordPress(press("F5", "F5", { altKey: true }))).toEqual({ kind: "keys", keys: "Alt+F5" });
    expect(recordPress(press("&", "Digit1", ctrlAlt))).toEqual({ kind: "keys", keys: "Ctrl+Alt+Digit1" });
  });
});

describe("clashes", () => {
  const settings = { shortcut: "Ctrl+Alt+KeyR", shortcuts: { goToAlert: { keys: "Ctrl+Alt+KeyA", enabled: true } } };

  it("name the enabled action already on those keys, whatever the spelling", () => {
    expect(clashWith("muteToggle", "ctrl+alt+r", settings)).toBe("openChat");
    expect(clashWith("muteToggle", "Ctrl+Alt+KeyA", settings)).toBe("goToAlert");
    expect(clashWith("goToAlert", "Ctrl+Alt+KeyA", settings)).toBeNull();
    expect(clashWith("muteToggle", "Ctrl+Shift+KeyA", settings)).toBeNull();
  });

  it("ignore actions that are off, even on their default keys", () => {
    expect(clashWith("goToAlert", "Ctrl+Alt+KeyT", settings)).toBeNull();
    expect(clashWith("openChat", "Ctrl+Alt+KeyR", { shortcut: "", shortcuts: {} })).toBeNull();
  });
});
