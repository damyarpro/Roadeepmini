import { beforeEach, describe, expect, it, vi } from "vitest";
import { onEvent } from "../core/bridge";
import { BridgeShortcuts, type ShortcutsReport, type ShortcutStatusEntry } from "../core/bridge-shortcuts";
import { setLanguage } from "../core/i18n";
import { DEFAULT_SETTINGS, type Settings } from "../core/state";
import { shortcutsSection } from "./shortcuts-section";

vi.mock("../core/bridge", () => ({
  IS_TAURI: true,
  Bridge: { log: vi.fn(async () => null), saveSettings: vi.fn(async () => null) },
  onEvent: vi.fn(async () => () => {}),
}));
vi.mock("../core/bridge-shortcuts", async (original) => ({
  ...(await original<typeof import("../core/bridge-shortcuts")>()),
  BridgeShortcuts: { status: vi.fn(async () => null), suspend: vi.fn(async () => null), check: vi.fn(), openSession: vi.fn() },
}));

const flush = async () => {
  for (let i = 0; i < 12; i++) await Promise.resolve();
};

/** Rust's report, as the "shortcuts-status" event delivers it. */
let deliver: ((report: ShortcutsReport) => void) | null = null;

let settings: Settings;
const save = vi.fn(async () => {});

function mount() {
  const section = shortcutsSection({ settings: () => settings, save });
  document.body.append(section);
  if (!deliver) {
    const call = vi.mocked(onEvent).mock.calls.find(([name]) => name === "shortcuts-status");
    deliver = call![1] as (report: ShortcutsReport) => void;
  }
  return section;
}

const recorder = (section: HTMLElement, id: string) => section.querySelector<HTMLButtonElement>(`#sc-${id}`)!;
const row = (section: HTMLElement, id: string) => recorder(section, id).closest<HTMLElement>(".set-row")!;
const keys = (el: HTMLElement) => [...el.querySelectorAll("kbd")].map((k) => k.textContent).join("+");
const key = (target: HTMLElement, init: KeyboardEventInit) =>
  target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));

function report(entries: Partial<ShortcutStatusEntry>[]): ShortcutsReport {
  return { suspended: false, actions: entries.map((e) => ({ id: "openChat", keys: "", status: "off", ...e }) as ShortcutStatusEntry) };
}

beforeEach(() => {
  document.body.replaceChildren();
  setLanguage("en");
  settings = { ...DEFAULT_SETTINGS, shortcuts: {} };
  save.mockClear();
  vi.mocked(BridgeShortcuts.suspend).mockClear();
  vi.mocked(BridgeShortcuts.check).mockReset();
});

describe("Settings → Shortcuts", () => {
  it("lists every action, the chat first with its keys, and the island's own keys", () => {
    const section = mount();
    const rows = [...section.querySelectorAll<HTMLButtonElement>("button.shortcut-rec")].map((b) => b.id);
    expect(rows).toEqual(["sc-openChat", "sc-toggleIsland", "sc-goToAlert", "sc-jumpToTerminal", "sc-nextPill", "sc-prevPill", "sc-muteToggle"]);
    expect(keys(recorder(section, "openChat"))).toBe("Ctrl+Alt+R");
    // Off by default, still showing the keys it would take.
    expect(keys(recorder(section, "nextPill"))).toBe("Ctrl+Alt+→");
    expect(row(section, "nextPill").classList.contains("off")).toBe(true);
    expect(row(section, "nextPill").textContent).toContain("Off");
    expect(section.textContent).toContain("In the open island");
    expect(section.textContent).toContain("Keep the island open");
  });

  it("speaks Persian with the keys still left to right", () => {
    setLanguage("fa");
    const section = mount();
    expect(section.querySelector("h2")?.textContent).toBe("میان‌برها");
    expect(recorder(section, "openChat").querySelector(".keycaps")?.getAttribute("dir")).toBe("ltr");
  });

  it("shows what Rust made of each shortcut, never a stale report", () => {
    settings.shortcuts = { goToAlert: { keys: "Ctrl+Alt+KeyA", enabled: true }, muteToggle: { keys: "Ctrl+Alt+KeyS", enabled: true } };
    const section = mount();
    deliver!(report([
      { id: "openChat", keys: "Ctrl+Alt+KeyR", status: "active" },
      { id: "goToAlert", keys: "Ctrl+Alt+KeyA", status: "typesCharacter", typed: "ą" },
      { id: "muteToggle", keys: "Ctrl+Alt+KeyQ", status: "inUse" },
    ]));
    expect(row(section, "openChat").querySelector(".status-badge.ok")?.textContent).toBe("Active");
    expect(row(section, "goToAlert").querySelector(".status-badge.warn")?.textContent).toBe("Types “ą”");
    // About other keys than the ones set now: no badge until the next report.
    expect(row(section, "muteToggle").querySelector(".status-badge")).toBeNull();
  });

  it("records a combination: lets ours go, checks it, saves it and takes them back", async () => {
    vi.mocked(BridgeShortcuts.check).mockResolvedValue({ ok: true, accelerator: "Ctrl+Alt+KeyK", error: null, typed: null });
    const section = mount();
    const button = recorder(section, "openChat");
    button.click();
    expect(BridgeShortcuts.suspend).toHaveBeenLastCalledWith(true);
    expect(button.getAttribute("aria-pressed")).toBe("true");
    key(button, { key: "Control", code: "ControlLeft", ctrlKey: true });
    expect(keys(button)).toBe("Ctrl");
    key(button, { key: "k", code: "KeyK", ctrlKey: true, altKey: true });
    await flush();
    expect(BridgeShortcuts.check).toHaveBeenCalledWith("Ctrl+Alt+KeyK");
    expect(settings.shortcut).toBe("Ctrl+Alt+KeyK");
    expect(save).toHaveBeenCalled();
    expect(BridgeShortcuts.suspend).toHaveBeenLastCalledWith(false);
    expect(keys(button)).toBe("Ctrl+Alt+K");
    expect(section.textContent).toContain("Shortcut saved: Ctrl + Alt + K");
  });

  it("turns another action on by recording it", async () => {
    vi.mocked(BridgeShortcuts.check).mockResolvedValue({ ok: true, accelerator: "Ctrl+Shift+KeyG", error: null, typed: null });
    const section = mount();
    recorder(section, "goToAlert").click();
    key(recorder(section, "goToAlert"), { key: "G", code: "KeyG", ctrlKey: true, shiftKey: true });
    await flush();
    expect(settings.shortcuts.goToAlert).toEqual({ keys: "Ctrl+Shift+KeyG", enabled: true });
    expect(settings.shortcut).toBe("Ctrl+Alt+KeyR");
  });

  it("refuses keys that type a character, are taken or belong to another action", async () => {
    const section = mount();
    const mute = recorder(section, "muteToggle");
    vi.mocked(BridgeShortcuts.check).mockResolvedValue({ ok: false, accelerator: "Ctrl+Alt+KeyE", error: null, typed: "€" });
    mute.click();
    key(mute, { key: "€", code: "KeyE", ctrlKey: true, altKey: true });
    await flush();
    expect(row(section, "muteToggle").querySelector(".field-err")?.textContent).toContain("types “€”");
    expect(settings.shortcuts).toEqual({});
    expect(BridgeShortcuts.suspend).toHaveBeenLastCalledWith(false);

    vi.mocked(BridgeShortcuts.check).mockClear();
    mute.click();
    key(mute, { key: "r", code: "KeyR", ctrlKey: true, altKey: true });
    await flush();
    expect(BridgeShortcuts.check).not.toHaveBeenCalled();
    expect(row(section, "muteToggle").querySelector(".field-err")?.textContent).toContain("“Open the chat”");
    expect(settings.shortcuts).toEqual({});
  });

  it("keeps listening when a modifier is missing, cancels on Esc, turns off on Backspace", async () => {
    const section = mount();
    const chat = recorder(section, "openChat");
    chat.click();
    key(chat, { key: "k", code: "KeyK" });
    expect(row(section, "openChat").querySelector(".field-err")?.textContent).toBe("Hold Ctrl, Alt or the Windows key with it.");
    expect(chat.getAttribute("aria-pressed")).toBe("true");
    key(chat, { key: "Escape", code: "Escape" });
    expect(chat.getAttribute("aria-pressed")).toBe("false");
    expect(settings.shortcut).toBe("Ctrl+Alt+KeyR");
    chat.click();
    key(chat, { key: "Backspace", code: "Backspace" });
    await flush();
    expect(settings.shortcut).toBe("");
    expect(save).toHaveBeenCalled();
    expect(keys(chat)).toBe("");
  });

  it("switches an action on with its keys and off again keeping them", async () => {
    const section = mount();
    const toggle = row(section, "jumpToTerminal").querySelector<HTMLButtonElement>("button.switch")!;
    toggle.click();
    await flush();
    expect(settings.shortcuts.jumpToTerminal).toEqual({ keys: "Ctrl+Alt+KeyT", enabled: true });
    toggle.click();
    await flush();
    expect(settings.shortcuts.jumpToTerminal).toEqual({ keys: "Ctrl+Alt+KeyT", enabled: false });

    // The chat's keys come back when it is turned on again.
    const chatToggle = row(section, "openChat").querySelector<HTMLButtonElement>("button.switch")!;
    settings.shortcut = "Ctrl+Alt+KeyK";
    chatToggle.click();
    await flush();
    expect(settings.shortcut).toBe("");
    chatToggle.click();
    await flush();
    expect(settings.shortcut).toBe("Ctrl+Alt+KeyK");
  });

  it("won't switch an action on over keys another one has", async () => {
    settings.shortcuts = { goToAlert: { keys: "Ctrl+Alt+KeyT", enabled: true } };
    const section = mount();
    const toggle = row(section, "jumpToTerminal").querySelector<HTMLButtonElement>("button.switch")!;
    toggle.click();
    await flush();
    expect(settings.shortcuts.jumpToTerminal).toBeUndefined();
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(row(section, "jumpToTerminal").querySelector(".field-err")?.textContent).toContain("Go to the waiting permission");
  });

  it("resets every shortcut, the chat's included", async () => {
    settings.shortcut = "";
    settings.shortcuts = { goToAlert: { keys: "Ctrl+Alt+KeyA", enabled: true } };
    const section = mount();
    [...section.querySelectorAll<HTMLButtonElement>("button.link")].find((b) => b.textContent === "Reset to defaults")!.click();
    await flush();
    expect(settings.shortcut).toBe("Ctrl+Alt+KeyR");
    expect(settings.shortcuts).toEqual({});
    expect(keys(recorder(section, "openChat"))).toBe("Ctrl+Alt+R");
  });
});
