// Tauri commands for the local Roadeep MCP server and the coding apps it can
// be registered with. Kept apart from bridge.ts so the MCP stream owns its own surface.

import { invoke } from "@tauri-apps/api/core";
import { IS_TAURI } from "./bridge";

export interface McpClient {
  /** Stable id (src-tauri/src/mcp/clients.rs), e.g. "claude-code", "cursor". */
  id: string;
  name: string;
  /** Its config file or config folder exists on this PC. */
  detected: boolean;
  /** A "roadeep" entry pointing at roadeep-mcp.exe is in its config. */
  installed: boolean;
  /** A "roadeep" entry exists but was not added by this app; installing replaces it. */
  conflict: boolean;
  /** Its entry still runs the relay from the previous version's folder; installing updates it. */
  legacyRelay?: boolean;
  configPath: string;
}

export interface McpStatus {
  exeReady: boolean;
  exePath: string;
  signedIn: boolean;
  clients: McpClient[];
}

/** Result of removing the previous version's relay folders (Settings only). */
export interface LegacyCleanupReport {
  /** A registration still runs an old relay, so nothing was removed. */
  referenced: boolean;
  removed: string[];
  /** Still there: in use right now, or kept because it is referenced. */
  kept: string[];
}

export interface McpPreview {
  diff: string;
  backup: string;
  configPath: string;
  /** Hand back to apply so only the reviewed diff is ever written. */
  fingerprint: string;
}

async function callOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) throw new Error("not running inside the Roadeep app");
  return invoke<T>(cmd, args);
}

export const BridgeMcp = {
  /** Never throws: an unreadable config simply reads as "not installed". */
  status: async (): Promise<McpStatus | null> => {
    if (!IS_TAURI) return null;
    try {
      return await invoke<McpStatus>("mcp_status");
    } catch (err) {
      console.error("[roadeep] mcp_status failed", err);
      return null;
    }
  },
  /** Throws with a readable message when that app's config can't be used. */
  preview: (client: string, install: boolean) => callOrThrow<McpPreview>("mcp_preview", { client, install }),
  /** Only from an explicit click. Resolves to the backup path ("" when there was no file). */
  apply: (client: string, install: boolean, fingerprint: string) =>
    callOrThrow<string>("mcp_apply", { client, install, fingerprint }),
  /** Only from an explicit click; refuses (removes nothing) while a registration still uses the old relay. */
  legacyCleanup: () => callOrThrow<LegacyCleanupReport>("legacy_relay_cleanup"),
};
