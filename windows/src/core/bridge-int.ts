// Tauri commands for the services section of the settings window: when each
// service last answered, and a one-off connection test. Kept apart from
// bridge.ts, which belongs to the chat backend.

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI } from "./bridge";

/** integrations.rs `PollStatus`: the last good answer (ms) and the current failure. */
export interface PollStatus {
  id: string;
  lastOk: number | null;
  /** An error code (core/error-text.ts). */
  error: string | null;
}

export type TestResult = { ok: true; status: PollStatus } | { ok: false; error: string };

/** Event the Rust pollers send the settings window after every poll. */
export const POLL_STATUS_EVENT = "integration-status";

export const BridgeInt = {
  async statuses(): Promise<PollStatus[]> {
    if (!IS_TAURI) return [];
    try {
      return await invoke<PollStatus[]>("integration_status");
    } catch (err) {
      void Bridge.log(`integration_status failed: ${String(err)}`);
      return [];
    }
  },

  /** Unlike the other bridges, the error comes back: it is what the user asked to see. */
  async test(id: string): Promise<TestResult | null> {
    if (!IS_TAURI) return null;
    try {
      return { ok: true, status: await invoke<PollStatus>("test_integration", { id }) };
    } catch (err) {
      return { ok: false, error: String(err) };
    }
  },
};
