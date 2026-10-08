import { State, type AgentTask } from "../core/state";
import type { BotStateName } from "../core/layout";
import type { Island } from "../island/island";
import type { ActivitySession } from "./types";
import { Activity } from "./store";

export const CODEX_TASK_ID = "integration_codex";

/** Aggregate parallel sessions without a completed worker masking its running parent. */
export function selectedCodexSession(sessions: ActivitySession[], now = Date.now()): ActivitySession | null {
  // Silence during a long tool is not a terminal signal; the store already bounds sessions.
  const recent = sessions.filter(s => s.harness === "codex" && s.status !== "archived" && (s.status === "active" || s.updatedAt >= now - 300_000));
  return recent.find(s => s.status === "active") ?? recent[0] ?? null;
}

export const activitySessionKey = (session: Pick<ActivitySession,"harness" | "id">) => `${session.harness}:${session.id}`;

/** Tool failure is evidence for the model to handle, not proof that the turn ended. */
export function activitySessionState(session?: ActivitySession | null): BotStateName {
  if (!session || session.status === "archived" || session.status === "cancelled") return "idle";
  if (session.status === "finished") return "finished";
  if (session.status === "error") return "error";
  const latest = [...session.events].reverse().find(event => event.kind !== "usage" && event.kind !== "session");
  return latest?.kind === "tool" && latest.phase === "started" ? "working" : "thinking";
}

export function codexTask(sessions: ActivitySession[], now = Date.now()): AgentTask | null {
  const session = selectedCodexSession(sessions,now);
  if (!session) return null;
  const state = activitySessionState(session);
  const project = session.cwd?.replace(/[\\/]+$/, "").split(/[\\/]/).at(-1);
  const evidence = session.events.filter(e => e.kind === "prompt" || (e.kind === "tool" && e.phase === "started")).slice(-20);
  const steps = evidence.map(e => e.kind === "prompt" ? "Codex" : e.tool ?? "Codex");
  return { id: CODEX_TASK_ID, name: project ? `Codex \u00b7 ${project}` : "Codex", color: "#F5F6F8", state, stepIndex: Math.max(0, steps.length - 1), steps, stepRevision:evidence.at(-1)?.id, stepSessionId:activitySessionKey(session), source: "agent", isIntegration: true, sessionCwd: session.cwd, pillBadge: state === "finished" || state === "error" ? state : null };
}

/** Observation changes display only; it never creates an approval or executes a tool. */
export function registerCodingIsland(island: Pick<Island, "reveal">): () => void {
  let signature = "";
  const sync = () => {
    if (State.pendingApproval?.taskId === CODEX_TASK_ID) return;
    const next = State.settings.observeCodex && !State.paused ? codexTask(Activity.sessions()) : null;
    const key = JSON.stringify(next);
    const evidenceChanged = key !== signature;
    signature = key;
    const existing = State.tasks.findIndex(t => t.id === CODEX_TASK_ID);
    if (!next && !State.paused && State.tasks[existing]?.directHook) return;
    if (!next) {
      if (existing >= 0) State.tasks.splice(existing, 1);
      if (State.focusId === CODEX_TASK_ID) State.focusId = "integration_claude";
      if (existing >= 0) State.notify();
      return;
    }
    let changed = evidenceChanged || existing < 0;
    if (next && State.tasks[existing]?.directHook) next.directHook = true;
    if (changed) {
      if (existing >= 0) State.tasks[existing] = next; else State.tasks.push(next);
    }
    // Respect chat, approvals, selected agents and active Claude sessions.
    const active = next.state === "working" || next.state === "thinking";
    if (active && State.focusId === "integration_claude" && ["idle", "finished", "error"].includes(State.focusTask?.state ?? "") && !State.pendingApproval && !State.stateOverride && State.view !== "prompt") {
      State.focusId = CODEX_TASK_ID;
      changed = true;
    }
    if (evidenceChanged && active && State.mode === "hidden") island.reveal();
    if (changed) State.notify();
  };
  const activity = Activity.subscribe(sync); const settings = State.subscribe(sync);
  const timer = window.setInterval(sync, 30_000);
  sync();
  const dispose = () => { activity(); settings(); window.clearInterval(timer); };
  window.addEventListener("beforeunload", dispose, { once: true });
  return dispose;
}
