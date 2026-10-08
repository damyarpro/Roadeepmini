import type { ActivitySession } from "./types";
import { CODEX_TASK_ID, selectedCodexSession } from "./island-adapter";

/** Observation navigation cannot launch a provider or answer another harness's approval. */
export function routeCodexTarget(taskId: string, sessions: ActivitySession[], actions: {
  select(session: ActivitySession): void;
  openActivity(): void;
}, now = Date.now()): boolean {
  if (taskId !== CODEX_TASK_ID) return false;
  const session = selectedCodexSession(sessions, now);
  if (session) {
    actions.select(session);
    actions.openActivity();
  }
  return true;
}
