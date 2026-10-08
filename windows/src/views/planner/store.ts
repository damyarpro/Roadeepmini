// The island's copy of the planner data. Rust owns it; this reads it once the
// first planner view (or the compact focus chip) needs it, and again after
// every "planner-changed". Views read `Planner.data` in their sync().

import {
  PlannerBridge, onPlannerChanged, type FocusState, type PlannerData,
} from "../../core/bridge-planner";
import { Bridge } from "../../core/bridge";
import { localizeError } from "../../core/error-text";
import { State } from "../../core/state";

let data: PlannerData | null = null;
let loadError: string | null = null;
let inflight: Promise<void> | null = null;
/** A change arrived while a read was in flight: read once more after it. */
let again = false;
let listening = false;
/** Bumped on every successful read, so views can skip work when nothing changed. */
let revision = 0;

function listen() {
  if (listening) return;
  listening = true;
  void onPlannerChanged(() => void refresh());
}

/** Reads everything again (several calls in a row coalesce into one or two reads). */
export function refresh(): Promise<void> {
  listen();
  if (inflight) {
    again = true;
    return inflight;
  }
  inflight = (async () => {
    try {
      do {
        again = false;
        data = await PlannerBridge.get();
        loadError = null;
        revision += 1;
      } while (again);
    } catch (err) {
      loadError = localizeError(err);
      void Bridge.log("planner: read failed");
    } finally {
      inflight = null;
      State.notify();
    }
  })();
  return inflight;
}

export const Planner = {
  get data(): PlannerData | null {
    return data;
  },
  get error(): string | null {
    return loadError;
  },
  get revision(): number {
    return revision;
  },
  /** Reads the data the first time anything asks for it. */
  ensure() {
    if (!data && !inflight) void refresh();
  },
  refresh,
  /** A focus answer from Rust (start, pause…) is newer than the last full read. */
  setFocus(focus: FocusState) {
    if (!data) return;
    data = { ...data, focus };
    revision += 1;
    State.notify();
  },
};

/**
 * Runs a planner command; on failure returns the localized error instead of
 * throwing, so a view can show it inline. A successful command is followed by
 * "planner-changed" from Rust, which refreshes the data.
 */
export async function attempt<T>(run: () => Promise<T>): Promise<{ ok: true; value: T } | { ok: false; error: string }> {
  try {
    const value = await run();
    // The stand-in and Rust both announce the change; reading now as well
    // keeps the view from waiting on the event round-trip.
    void refresh();
    return { ok: true, value };
  } catch (err) {
    return { ok: false, error: localizeError(err) };
  }
}
