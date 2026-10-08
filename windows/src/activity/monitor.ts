import { Bridge, IS_TAURI, onEvent } from "../core/bridge";
import { State } from "../core/state";
import { Activity } from "./store";
import { clearClaudePending } from "./claude";
import { HistoryPersistence } from "./history";

const history = new HistoryPersistence({
  snapshot: () => Bridge.codingHistorySnapshot(), save: (events, revision) => Bridge.codingHistorySave(events, revision),
  events: () => Activity.retainedEvents(), restore: events => Activity.restore(events), changed: () => State.notify(),
  log: () => { void Bridge.log("coding history operation failed"); },
});

export const Monitor = { available: true, error: false, busy: false, operation:null as "saving"|"clearing"|"loading"|null, history };
let generation = 0;
let unlisten: (() => void) | undefined;
let unlistenStatus: (() => void) | undefined;
let unlistenHistory: (() => void) | undefined;
let clearing = false;
Activity.subscribe(() => { State.notify(); if (!clearing) history.schedule(); });

/** Listen before replay; the store deduplicates events crossing the snapshot. */
export async function startMonitor() {
  try {
  unlisten = await onEvent<unknown>("coding-activity", event => {
    if (State.settings.observeCodex && !State.paused && !clearing) Activity.ingest(event);
  });
  unlistenStatus = await onEvent<{ enabled: boolean; available: boolean; error?: string }>("coding-status", status => {
    if (!State.settings.observeCodex || !status || typeof status.available !== "boolean") return;
    Monitor.available = status.available;
    Monitor.error = !!status.error;
    State.notify();
  });
  unlistenHistory = await onEvent<unknown>("coding-history-error", error => {
    if (!State.settings.retainCodingHistory || typeof error !== "string") return;
    history.error = true;
    void Bridge.log("coding history maintenance failed");
    State.notify();
  });
  window.addEventListener("beforeunload", () => { unlisten?.(); unlistenStatus?.(); unlistenHistory?.(); history.cancel(); }, { once: true });
  if (IS_TAURI && State.settings.retainCodingHistory) { await history.load(); history.schedule(); }
  await refreshMonitor();
  } catch {
    unlisten?.(); unlistenStatus?.(); unlistenHistory?.();
    Monitor.error = true;
    void Bridge.log("coding activity event subscription failed");
    State.notify();
  }
}

export async function refreshMonitor() {
  const turn = ++generation;
  if (!Monitor.busy) {
    if (!State.settings.retainCodingHistory) {
      history.cancel(); Activity.clearArchived();
    } else if (IS_TAURI && !history.active) {
      await history.load(); history.schedule();
      if (turn !== generation) return;
    }
  }
  if (!State.settings.observeCodex) {
    if (!State.settings.retainCodingHistory) Activity.clearHarness("codex",true);
    else Activity.archiveHarness("codex",true);
    Monitor.error = false;
    State.notify();
    return;
  }
  if (!IS_TAURI) return;
  try {
    const status = await Bridge.codingStatus();
    if (turn !== generation || !State.settings.observeCodex) return;
    Monitor.available = status.available;
    Monitor.error = !!status.error;
    const events = await Bridge.codingSnapshot();
    if (turn !== generation || !State.settings.observeCodex || State.paused) return;
    for (const event of events) Activity.ingest(event);
  } catch {
    if (turn === generation) Monitor.error = true;
    void Bridge.log("coding activity snapshot unavailable");
  }
  State.notify();
}

export async function toggleMonitor() {
  if (Monitor.busy) return;
  Monitor.busy = true;
  Monitor.operation = "saving";
  Monitor.error = false;
  State.notify();
  const enabled = !State.settings.observeCodex;
  ++generation;
  try {
    if (IS_TAURI) await Bridge.saveSettingsChecked({ ...State.settings, observeCodex: enabled });
    State.settings.observeCodex = enabled;
    await refreshMonitor();
  } catch { Monitor.error = true; void Bridge.log("coding activity preference save failed"); }
  finally { Monitor.busy = false; Monitor.operation = null; State.notify(); }
}

export async function clearActivity() {
  if (Monitor.busy) return;
  ++generation;
  history.cancel();
  Monitor.busy = true;
  Monitor.operation = "clearing";
  State.notify();
  clearing = true;
  try {
    if (IS_TAURI) await Bridge.codingClear();
    Activity.clear();
    clearClaudePending();
    if (IS_TAURI && State.settings.retainCodingHistory) await history.load();
    Monitor.error = false;
  } catch {
    Monitor.error = true; void Bridge.log("coding activity clear failed");
    if (IS_TAURI && State.settings.retainCodingHistory) await history.load();
  }
  finally { clearing = false; Monitor.busy = false; Monitor.operation = null; State.notify(); }
}

export async function toggleHistory() {
  if (Monitor.busy || history.busy) return;
  Monitor.busy = true; Monitor.operation = "saving";
  const enabled = !State.settings.retainCodingHistory;
  history.cancel(); State.notify();
  try {
    if (IS_TAURI) await Bridge.saveSettingsChecked({ ...State.settings, retainCodingHistory: enabled });
    State.settings.retainCodingHistory = enabled;
    history.error = false;
    if (enabled) { if (IS_TAURI) await history.load(); history.schedule(); }
    else Activity.clearArchived();
  } catch {
    void Bridge.log("coding history preference save failed");
    if (State.settings.retainCodingHistory && IS_TAURI) await history.load();
    history.error = true;
  } finally { Monitor.busy = false; Monitor.operation = null; State.notify(); }
}

export async function retryHistory() {
  if (Monitor.busy || history.busy || !State.settings.retainCodingHistory || !IS_TAURI) return;
  await history.load(); history.schedule();
}
