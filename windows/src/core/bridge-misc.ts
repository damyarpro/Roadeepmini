// Tauri commands for the global chat shortcut (Settings → General). Kept apart
// from bridge.ts, which belongs to the chat backend.

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI } from "./bridge";

export interface ShortcutStatus {
  /** The configured accelerator, e.g. "Ctrl+Alt+Space"; "" = off. */
  accelerator: string;
  registered: boolean;
  /** An error code (core/error-text.ts) when configured but not working. */
  error: string | null;
}

export interface ShortcutCheck {
  ok: boolean;
  /** Canonical spelling to store, when the combination parses. */
  accelerator: string;
  error: string | null;
}

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

export const BridgeMisc = {
  shortcutStatus: () => call<ShortcutStatus>("shortcut_status"),
  /** Validates a recorded combination and asks the OS whether it is free. */
  shortcutCheck: (accelerator: string) => call<ShortcutCheck>("shortcut_check", { accelerator }),
};
