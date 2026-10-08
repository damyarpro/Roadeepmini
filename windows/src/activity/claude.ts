import { Activity, normalizeEvent } from "./store";
import { isCodingProvider } from "./providers";

interface HookObservation {
  provider?: string; hook_event_name?: string; session_id?: string; cwd?: string; tool_name?: string;
  tool_use_id?: string; tool_input?: Record<string, unknown>;
}
const pending = new Map<string, { id: string; files?: string[]; command?: string }[]>();
export function clearClaudePending() { pending.clear(); }

/** Observe hook metadata only. No permission decisions or invented tool output. */
export function observeClaudeHook(payload: HookObservation) { return observeCodingHook({...payload,provider:"claude"}); }
export function observeCodingHook(payload: HookObservation) {
  const provider = payload.provider ?? "claude";
  if (!isCodingProvider(provider)) return;
  const name = payload.hook_event_name ?? "";
  const kind = name === "SessionStart" ? "session" : name === "UserPromptSubmit" ? "prompt" : name === "Stop" || name === "SessionEnd" ? "finished" : name === "StopFailure" ? "error" : name === "Interrupt" ? "cancelled" : ["PreToolUse", "PostToolUse", "PostToolUseFailure"].includes(name) ? "tool" : null;
  if (!kind) return;
  const sessionId = payload.session_id || `${provider}:${payload.cwd || "local"}`;
  if (sessionId.length > 200 || (payload.tool_name?.length ?? 0) > 200) return;
  const key = `${provider}:${sessionId}:${payload.tool_name || "unknown"}`;
  let call: { id: string; files?: string[]; command?: string } | undefined;
  if (name === "PreToolUse") {
    const input = payload.tool_input ?? {};
    const path = typeof input.file_path === "string" ? input.file_path : typeof input.path === "string" ? input.path : undefined;
    const cleaned = normalizeEvent({id:crypto.randomUUID(),sessionId,harness:provider,kind:"tool",at:Date.now(),callId:payload.tool_use_id || crypto.randomUUID(),files:path ? [path] : undefined,command:input.command});
    if (!cleaned?.callId) return;
    call = { id: cleaned.callId, files:cleaned.files, command:cleaned.command };
    const queue = pending.get(key) ?? [];
    queue.push(call);
    if (queue.length > 8) queue.shift();
    pending.set(key, queue);
    if (pending.size > 64) pending.delete(pending.keys().next().value!);
  } else if (name === "PostToolUse" || name === "PostToolUseFailure") {
    const queue = pending.get(key);
    if (queue) {
      const index = payload.tool_use_id ? queue.findIndex(c => c.id === payload.tool_use_id) : 0;
      if (index >= 0) call = queue.splice(index, 1)[0];
      if (!queue.length) pending.delete(key);
    }
  } else if (name === "SessionStart" || name === "SessionEnd" || name === "UserPromptSubmit") {
    for (const saved of pending.keys()) if (saved.startsWith(`${provider}:${sessionId}:`)) pending.delete(saved);
  }
  Activity.ingest({ id:crypto.randomUUID(), sessionId, harness:provider, at:Date.now(), source:"hook", kind, cwd:payload.cwd,
    tool:payload.tool_name, callId:call?.id || payload.tool_use_id, files:call?.files, command:call?.command,
    phase:name === "PreToolUse" ? "started" : name === "PostToolUseFailure" ? "failed" : name === "PostToolUse" ? "completed" : undefined,
  });
}
