// Entry point: boot the bridge, wire the island, start the greeting.

import "./style.css";
import {initializeGlobalVoice} from "./local/global-voice";
import {bindVoiceCharacter} from "./local/voice-host";
import {CHARACTER_OPTIONS,applyCharacterPatch} from "./character/appearance";
import { Bridge, IS_TAURI, onEvent, type DockDragFrame } from "./core/bridge";
import { Sound } from "./core/sound";
import { DEFAULT_DOCK, State, type Settings } from "./core/state";
import type { DockLayout } from "./island/dock";
import { Island } from "./island/island";
import { registerHookHandlers } from "./island/hooks";
import { registerIntegrationHandlers, refreshConfigured } from "./island/integrations";
import { registerShortcutHandlers } from "./island/shortcuts";
import { getLanguage, loadFonts, normalizeLanguage, setLanguage, t } from "./core/i18n";
import { installContextMenu, quitEntry } from "./core/context-menu";
import { seedDemoChat } from "./island/dev-preview";
import { startMonitor, refreshMonitor } from "./activity/monitor";
import { registerCodingIsland } from "./activity/island-adapter";
import { seedActivityPreview } from "./activity/preview";
import { seedCodingPowerPreview } from "./views/coding-power-preview";
import { normalizeAppearance } from "./character/appearance";

/** Longest the first paint waits on the bundled font before going without it. */
const FONT_WAIT_MS = 800;

async function main() {
  const root = document.getElementById("root");
  if (!root) return;

  void Sound.preload();

  // The language has to be known before the views are built: they carry their
  // wording from construction. Outside Tauri there is no boot, so it stays fa.
  const boot = await Bridge.boot();
  if (boot) {
    State.settings = { ...State.settings, ...boot.settings };
  }
  // Plain-browser preview only: ?lang=en shows the English UI.
  if (!IS_TAURI) {
    const lang = new URLSearchParams(location.search).get("lang");
    if (lang) State.settings.language = normalizeLanguage(lang);
    const params = new URLSearchParams(location.search);
    if (["shape", "color", "expression", "body", "eyes"].some(key => params.has(key))) State.settings.characterAppearance = normalizeAppearance({shape:params.get("shape"),color:params.get("color"),expression:params.get("expression"),body:params.get("body"),eyes:params.get("eyes")});
  }
  State.settings.language = normalizeLanguage(State.settings.language);
  setLanguage(State.settings.language);
  await Promise.race([
    loadFonts(),
    new Promise<void>((resolve) => window.setTimeout(resolve, FONT_WAIT_MS)),
  ]);

  if (!IS_TAURI) seedCodingPowerPreview(location.search);
  const island = new Island(root, boot?.layout ?? null);
  island.applySettings();
  bindVoiceCharacter({options:()=>CHARACTER_OPTIONS,apply:(settings,patch)=>({...settings,characterAppearance:applyCharacterPatch({characterAppearance:settings.characterAppearance},patch)})});
  initializeGlobalVoice();
  /** The island sits anywhere but its default top-centre spot. */
  const moved = () => {
    const d = State.settings.dock ?? DEFAULT_DOCK;
    return d.edge !== DEFAULT_DOCK.edge || Math.abs(d.pos - DEFAULT_DOCK.pos) > 0.001 || d.monitor !== "";
  };
  const reducedMotion = () => !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  installContextMenu({
    entries: () =>
      island.canShowMenu
        ? [
            // Forced: collapses even a pinned card (an approval still waits in Claude Code).
            { id: "minimize", label: t("ctx.minimize"), icon: "minimize", run: () => island.collapse() },
            // The way back when the island ended up somewhere awkward.
            ...(moved()
              ? [{
                  id: "dock-reset", label: t("ctx.dockReset"), icon: "dock" as const,
                  run: () => void Bridge.dockSet("top", 0.5, true, reducedMotion()),
                }]
              : []),
            { id: "settings", label: t("ctx.settings"), icon: "settings", run: () => void Bridge.openSettingsWindow() },
            "separator",
            quitEntry(),
          ]
        : null,
    onOpen: (rect) => island.setMenuRect(rect),
    onClose: () => island.setMenuRect(null),
  });
  State.loadIntegrationTasks();
  registerHookHandlers(island);
  registerCodingIsland(island);
  void startMonitor();
  if (!IS_TAURI) seedActivityPreview(location.search);

  await onEvent<{ x: number; y: number }>("cursor", ({ x, y }) => island.onCursor(x, y));

  /** Pause has to reach Rust too, or the pollers keep calling out. */
  const setPaused = (on: boolean) => {
    if (State.paused === on) return;
    State.paused = on;
    void Bridge.setPaused(on);
    if (!on) void refreshMonitor();
  };

  await onEvent<string>("tray", (what) => {
    switch (what) {
      case "settings":
        setPaused(false);
        island.alert("settings");
        break;
      case "open":
        setPaused(false);
        island.alert(State.defaultView());
        break;
      case "pause":
        setPaused(!State.paused);
        if (State.paused) island.fsm.forceHidden();
        else island.reveal();
        break;
      case "chat":
        // The global shortcut: a second press puts the chat away again.
        if (State.mode === "expanded" && State.view === "prompt") {
          island.collapse();
          void Bridge.focusWindow(false);
          break;
        }
        setPaused(false);
        island.focusChat();
        break;
    }
  });

  await onEvent<null>("screen-changed", () => void Bridge.reposition());

  // Docking: Rust moves the window, the island follows its layout and shape.
  await onEvent<DockLayout>("dock-layout", (layout) => island.setLayout(layout));
  await onEvent<DockDragFrame>("dock-drag", (frame) => island.onDockDrag(frame));
  await onEvent<{ layout: DockLayout; wait: boolean }>("dock-move", (move) => void island.onDockMove(move));
  await onEvent<null>("dock-settled", () => island.onDockSettled());

  // The settings window writes preferences; apply them here without a restart.
  await onEvent<Settings>("settings-changed", (s) => {
    State.settings = { ...State.settings, ...s };
    const language = normalizeLanguage(State.settings.language);
    State.settings.language = language;
    if (language !== getLanguage()) {
      setLanguage(language);
      island.relocalize();
    }
    island.applySettings();
    State.loadIntegrationTasks();
    void refreshMonitor();
    void refreshConfigured();
  });

  registerIntegrationHandlers(island);
  registerShortcutHandlers(island, () => setPaused(false));

  island.launch();

  // Plain-browser preview only (dev-preview.ts): ?chat=N seeds a sample chat.
  if (!IS_TAURI) {
    window.setTimeout(() => seedDemoChat(location.search, getLanguage(), State), 300);
  }

  // In a plain browser there is no wake strip behind the cursor: make the whole
  // page wake the island so the visuals can be checked with `npm run dev`.
  if (!IS_TAURI) {
    document.addEventListener("click", () => Sound.resume(), { once: true });
  }
}

void main();
