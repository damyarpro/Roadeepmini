// Tauri commands and events for the auto-updater (src-tauri/src/updater.rs).
// Outside Tauri (plain browser) every call returns null, which the Settings row
// shows as "updates are not enabled in this build".

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI, onEvent } from "./bridge";

export interface UpdateAvailable {
  version: string;
  /** Release notes, plain text — render with textContent only. */
  notes: string | null;
  /** Publication date, Unix milliseconds. */
  date: number | null;
}

export interface UpdateStatus {
  /** False when the build was made without the updater variables. */
  enabled: boolean;
  currentVersion: string;
  checking: boolean;
  available: UpdateAvailable | null;
  downloaded: boolean;
  /** 0..1 while downloading, 1 once downloaded. */
  progress: number | null;
  /** Unix milliseconds of the last finished check. */
  lastCheckedAt: number | null;
  /** An E_UPDATE_* code (core/error-text.ts). */
  error: string | null;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
  progress: number | null;
}

/** Emitted to every window when a check finds a newer version. */
export const UPDATE_AVAILABLE_EVENT = "update-available";
export const UPDATE_PROGRESS_EVENT = "update-progress";

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

export const BridgeUpdate = {
  status: () => call<UpdateStatus>("update_status"),
  check: () => call<UpdateStatus>("update_check"),
  /**
   * Only from an explicit click. Never resolves on success (the app quits and the
   * installer relaunches it); rejects with an E_UPDATE_* code otherwise.
   */
  async install(): Promise<void> {
    if (!IS_TAURI) return;
    try {
      await invoke<void>("update_install");
    } catch (err) {
      void Bridge.log(`update_install failed: ${String(err)}`);
      throw err;
    }
  },
  onAvailable: (handler: (u: UpdateAvailable) => void) => onEvent<UpdateAvailable>(UPDATE_AVAILABLE_EVENT, handler),
  onProgress: (handler: (p: UpdateProgress) => void) => onEvent<UpdateProgress>(UPDATE_PROGRESS_EVENT, handler),
};
