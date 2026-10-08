import { diffPreview } from "./diff";
import { isCodingProvider } from "./providers";
import { isTestCommand, parseTestOutput } from "./test-output";
import type { ActivitySession, CodingEvent, CodingContext, CodingUsage, UsageWindow } from "./types";

const MAX_SESSIONS = 16;
const MAX_EVENTS = 160;
const MAX_TEXT = 12000;
const kinds = new Set(["session", "prompt", "tool", "finished", "error", "cancelled", "usage"]);
const object = (value: unknown): Record<string, unknown> | undefined => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
const bounded = (value: unknown, max: number): number | undefined => typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= max ? value : undefined;

function contextOf(raw: unknown): CodingContext | undefined {
  const value = object(raw); if (!value) return undefined;
  const context: CodingContext = {};
  for (const key of ["usedTokens", "limitTokens", "usedPercent"] as const) {
    const number = bounded(value[key], key === "usedPercent" ? 100 : 1_000_000_000);
    if (number !== undefined) context[key] = number;
  }
  const model = clean(value.model, 120); if (model) context.model = model;
  // Contradictory counters do not provide reliable context evidence.
  if (context.usedTokens !== undefined && context.limitTokens !== undefined && (context.limitTokens === 0 || context.usedTokens > context.limitTokens)) return undefined;
  return Object.keys(context).length ? context : undefined;
}
function usageOf(raw: unknown): CodingUsage | undefined {
  const value = object(raw); if (!value) return undefined;
  const usage: CodingUsage = {};
  for (const key of ["primary", "secondary"] as const) {
    const window = object(value[key]); if (!window) continue;
    const usedPercent = bounded(window.usedPercent, 100); if (usedPercent === undefined) continue;
    const result: UsageWindow = { usedPercent };
    const resetsAt = bounded(window.resetsAt, 8640000000000000); if (resetsAt !== undefined) result.resetsAt = resetsAt;
    const windowMinutes = bounded(window.windowMinutes, 525600); if (windowMinutes !== undefined) result.windowMinutes = windowMinutes;
    usage[key] = result;
  }
  return Object.keys(usage).length ? usage : undefined;
}

export function redactText(text: string): string {
  return text
    .replace(/\b(?:sk-[A-Za-z0-9_-]{8,}|gh[pousr]_[A-Za-z0-9]{8,}|github_pat_[A-Za-z0-9_]+)\b/g, "[redacted]")
    .replace(/\bBearer\s+[A-Za-z0-9._~+\/-]+/gi, "Bearer [redacted]")
    .replace(/(\b(?:api[_-]?key|access[_-]?token|refresh[_-]?token|password|secret|authorization)\b\s*[=:]\s*)(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,;]+)/gi, "$1[redacted]")
    .replace(/-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?(?:-----END [^-]*PRIVATE KEY-----|$)/g, "[redacted private key]")
    .replace(/(https?:\/\/)[^\s/@]+:[^\s/@]+@/gi, "$1[redacted]@");
}

function clean(value: unknown, limit: number): string | undefined {
  if (typeof value !== "string") return undefined;
  // Redact before truncation so private-key blocks and long values remain concealed.
  const text = redactText(value.slice(0, 100000)).replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, "");
  return text.length > limit ? `${text.slice(0, limit)}\n[truncated]` : text;
}

export function normalizeEvent(raw: unknown): CodingEvent | undefined {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return undefined;
  const value = raw as Record<string, unknown>;
  if (typeof value.id !== "string" || !value.id || value.id.length > 200 || typeof value.sessionId !== "string" || !value.sessionId || value.sessionId.length > 200) return undefined;
  if (/[\u0000-\u001f\u007f]/.test(value.id + value.sessionId) || (typeof value.callId === "string" && (value.callId.length > 200 || /[\u0000-\u001f\u007f]/.test(value.callId)))) return undefined;
  if (!isCodingProvider(value.harness)) return undefined;
  if (typeof value.kind !== "string" || !kinds.has(value.kind) || typeof value.at !== "number" || !Number.isFinite(value.at) || value.at < 0 || value.at > 8640000000000000) return undefined;
  const event: CodingEvent = { id: value.id, sessionId: value.sessionId, harness: value.harness, at: value.at, kind: value.kind as CodingEvent["kind"] };
  if (value.source === "hook" || value.source === "log" || value.source === "both") event.source = value.source;
  for (const field of ["cwd", "title", "tool", "callId", "command", "output", "patch"] as const) {
    const text = clean(value[field], field === "output" || field === "patch" ? MAX_TEXT : field === "command" ? 2000 : 500);
    if (text !== undefined) event[field] = text;
  }
  if (typeof value.phase === "string" && ["started", "completed", "failed"].includes(value.phase)) event.phase = value.phase as CodingEvent["phase"];
  if (typeof value.exitCode === "number" && Number.isSafeInteger(value.exitCode)) event.exitCode = value.exitCode;
  if (Array.isArray(value.files)) event.files = value.files.slice(0, 60).map(file => clean(file, 500)).filter((file): file is string => !!file);
  event.context = contextOf(value.context);
  event.usage = usageOf(value.usage);
  const summary = object(value.testSummary);
  if (summary && event.kind === "tool" && (event.phase === "completed" || event.phase === "failed") && ["passed", "failed", "skipped", "unknown"].includes(String(summary.verdict))) {
    const passed = bounded(summary.passed, 1e9), failed = bounded(summary.failed, 1e9), skipped = bounded(summary.skipped, 1e9);
    if (passed !== undefined && failed !== undefined && skipped !== undefined && [passed, failed, skipped].every(Number.isSafeInteger)) {
      const verdict = failed > 0 || event.phase === "failed" || (event.exitCode !== undefined && event.exitCode !== 0) ? "failed" : summary.verdict === "passed" && (passed === 0 || event.exitCode !== 0) ? "unknown" : summary.verdict as "passed" | "failed" | "skipped" | "unknown";
      event.testSummary = { verdict, passed, failed, skipped };
    }
  }
  return event;
}

function pathKey(file: string, cwd?: string): string {
  const absolute = /^(?:[a-z]:[\\/]|[\\/]{2}|\/)/i.test(file);
  const path = (absolute ? file : cwd ? `${cwd}/${file}` : file).replace(/\\/g, "/");
  const parts: string[] = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === ".." && parts.length && parts.at(-1) !== "..") parts.pop();
    else parts.push(part);
  }
  // Windows observations are case insensitive; unknown relative roots never overlap.
  return cwd || absolute ? parts.join("/").toLowerCase() : "";
}

function isMutation(event: CodingEvent): boolean {
  return !!event.patch || /(?:apply_patch|edit|write|replace|delete|create_file)/i.test(event.tool ?? "");
}

function cloneEvent(event: CodingEvent): CodingEvent {
  return {
    ...event, files: event.files ? [...event.files] : undefined,
    context: event.context ? { ...event.context } : undefined,
    usage: event.usage ? {
      primary: event.usage.primary ? { ...event.usage.primary } : undefined,
      secondary: event.usage.secondary ? { ...event.usage.secondary } : undefined,
    } : undefined,
    testSummary: event.testSummary ? { ...event.testSummary } : undefined,
  };
}

function derive(events: CodingEvent[]): ActivitySession {
  const first = events[0];
  const session: ActivitySession = { id: first.sessionId, harness: first.harness, title: first.title || first.sessionId, status: "active", updatedAt: first.at, events, changedFiles: [], conflicts: [], counts: { tools: 0, errors: 0 } };
  const starts = new Map<string, CodingEvent>();
  const files = new Set<string>();
  const uncertainMutations = new Set<string>();
  for (const event of events) {
    session.updatedAt = Math.max(session.updatedAt, event.at);
    if (event.cwd) session.cwd = event.cwd;
    if (event.context) session.context = { ...event.context };
    if (event.usage) session.usage = { ...session.usage, ...event.usage };
    if (event.title && (event.kind === "session" || event.kind === "prompt")) session.title = event.title;
    if (event.kind === "prompt") {
      files.clear(); starts.clear(); uncertainMutations.clear();
      session.test = undefined; session.lastPatch = undefined; session.lastMutationAt = undefined;
      session.counts = { tools: 0, errors: 0 };
    }
    if (event.kind === "finished") session.status = "finished";
    else if (event.kind === "cancelled") session.status = "cancelled";
    else if (event.kind === "error") { session.status = "error"; session.counts.errors++; }
    else if (event.kind === "prompt" || (event.kind !== "usage" && event.phase === "started")) session.status = "active";
    if (event.kind !== "tool") continue;
    if (event.testSummary) session.test = { ...event.testSummary, eventId: event.id, at: event.at, command: "", reason: "Historical normalized evidence", freshness: "unknown" };
    if (event.phase === "started") {
      session.counts.tools++;
      if (event.callId) starts.set(event.callId, event);
    }
    const start = event.callId ? starts.get(event.callId) : undefined;
    const merged = { ...start, ...event, tool: event.tool || start?.tool, command: event.command || start?.command, files: event.files ?? start?.files };
    if (isMutation(merged)) {
      const mutationKey = event.callId ?? event.id;
      if (event.phase === "started" || event.phase === undefined) uncertainMutations.add(mutationKey);
      else {
        uncertainMutations.delete(mutationKey);
        // Claude PostToolUse observes a successful tool completion but no process exit code.
        const confirmed = event.harness === "claude" || event.exitCode === 0 || (event.exitCode === undefined && /^Success\. Updated the following files:/m.test(event.output ?? ""));
        if (event.phase === "completed" && !confirmed) uncertainMutations.add(mutationKey);
        if (event.phase === "completed" && confirmed) {
          session.lastMutationAt = Math.max(session.lastMutationAt ?? 0, event.at);
          for (const file of merged.files ?? []) if (files.size < 120) files.add(file);
          if (merged.patch) session.lastPatch = { eventId: event.id, at: event.at, files: merged.files ?? [], preview: diffPreview(merged.patch) };
        }
      }
    }
    if (event.phase === "failed") session.counts.errors++;
    if (merged.command && isTestCommand(merged.command)) {
      const result = parseTestOutput(event.output ?? "", event.phase === "failed" ? event.exitCode ?? -1 : event.phase === "started" ? undefined : event.exitCode);
      session.test = { ...result, eventId: event.id, at: start?.at ?? event.at, command: merged.command, freshness: "unknown" };
    }
  }
  session.changedFiles = [...files];
  let promptIndex = events.length - 1;
  while (promptIndex >= 0 && events[promptIndex].kind !== "prompt") promptIndex--;
  if (promptIndex >= 0) session.events = events.slice(promptIndex);
  if (session.test) session.test.freshness = session.test.reason === "Historical normalized evidence" || uncertainMutations.size ? "unknown" : session.lastMutationAt !== undefined && session.lastMutationAt >= session.test.at ? "stale" : session.test.verdict === "passed" || session.test.verdict === "failed" || session.test.verdict === "skipped" ? "current" : "unknown";
  return session;
}

export class ActivityStore {
  private readonly history = new Map<string, CodingEvent[]>();
  private readonly archived = new Set<string>();
  private readonly listeners = new Set<() => void>();

  ingest(raw: unknown, restored = false): boolean {
    const event = normalizeEvent(raw);
    if (!event) return false;
    if (!restored) delete event.testSummary;
    const key = `${event.harness}:${event.sessionId}`;
    const events = this.history.get(key) ?? [];
    if (events.some(prior => prior.id === event.id)) return false;
    // Stable provider call IDs join direct hook metadata with richer rollout evidence.
    // Do not guess identity from command text or time: parallel calls may be identical.
    const duplicate = event.kind === "tool" && event.callId ? events.findIndex(prior =>
      prior.kind === "tool" && prior.callId === event.callId && prior.phase === event.phase &&
      (prior.source ?? "log") !== (event.source ?? "log")) : -1;
    if (duplicate >= 0) {
      const prior = events[duplicate];
      const richer = event.source === "hook" ? prior : event;
      events[duplicate] = { ...prior, ...event, ...richer, source: "both" };
      events.sort((a,b)=>a.at-b.at || (a.phase==="started"?-1:b.phase==="started"?1:0));
      this.notify();
      return true;
    }
    events.push(event);
    events.sort((a, b) => a.at - b.at || (a.phase === "started" ? -1 : b.phase === "started" ? 1 : 0));
    if (events.length > MAX_EVENTS) events.splice(0, events.length - MAX_EVENTS);
    this.history.set(key, events);
    if (!restored && event.kind !== "usage") this.archived.delete(key);
    if (this.history.size > MAX_SESSIONS) {
      const oldest = [...this.history.entries()].sort((a, b) => a[1].at(-1)!.at - b[1].at(-1)!.at)[0][0];
      this.history.delete(oldest);
      this.archived.delete(oldest);
    }
    this.notify();
    return true;
  }

  clear(): void { this.history.clear(); this.archived.clear(); this.notify(); }
  restore(raw: unknown[]): void {
    // Existing live evidence wins, including usage-only events racing a restore.
    const liveKeys = new Set(this.history.keys());
    for (const value of raw.slice(0, 512)) {
      const event = normalizeEvent(value); if (!event) continue;
      const key = `${event.harness}:${event.sessionId}`;
      if (liveKeys.has(key)) continue;
      this.archived.add(key); this.ingest(event, true);
    }
    this.notify();
  }
  clearArchived(): void {
    for (const key of this.archived) this.history.delete(key);
    this.archived.clear(); this.notify();
  }
  archiveHarness(harness: CodingEvent["harness"], keepHooks=false): void {
    for (const [key, events] of this.history) if (events[0]?.harness === harness && (!keepHooks || !events.some(e=>e.source==="hook"||e.source==="both"))) this.archived.add(key);
    this.notify();
  }
  retainedEvents(now = Date.now()): CodingEvent[] {
    const candidates = [...this.history.values()].flat().filter(event => event.at >= now - 7 * 86400000 && event.at <= now).sort((a, b) => a.at - b.at).slice(-512);
    // Persistent history retains structure, never user prompts or process output.
    let bytes = 2;
    const events: CodingEvent[] = [];
    const tests = new Map(this.sessions().filter(session => session.test).map(session => [session.test!.eventId, session.test!]));
    for (const event of candidates.reverse()) {
      const { command: _command, output: _output, patch: _patch, title: _title, ...safe } = cloneEvent(event);
      const test = tests.get(event.id);
      if (test && (event.phase === "completed" || event.phase === "failed")) safe.testSummary = { verdict: test.verdict, passed: test.passed, failed: test.failed, skipped: test.skipped };
      const size = new TextEncoder().encode(JSON.stringify(safe)).length + 1;
      if (bytes + size > 2 * 1024 * 1024) break;
      bytes += size; events.push(safe);
    }
    return events.reverse();
  }
  clearHarness(harness: CodingEvent["harness"], keepHooks=false): void {
    let changed = false;
    for (const [key, events] of this.history) {
      if (events[0]?.harness === harness) {
        if(keepHooks){
          const hooks=events.filter(e=>e.source==="hook"||e.source==="both").map(e=>{const {output:_output,patch:_patch,exitCode:_exit,context:_context,usage:_usage,...metadata}=e;return {...metadata,source:"hook" as const};});
          if(hooks.length){this.history.set(key,hooks);changed=true;continue;}
        }
        this.history.delete(key);
        this.archived.delete(key);
        changed = true;
      }
    }
    if (changed) this.notify();
  }
  subscribe(listener: () => void): () => void { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  private notify(): void { for (const listener of this.listeners) listener(); }

  sessions(): ActivitySession[] {
    const sessions = [...this.history.values()].map(events => derive(events.map(cloneEvent))).sort((a, b) => b.updatedAt - a.updatedAt);
    for (const session of sessions) if (this.archived.has(`${session.harness}:${session.id}`) || session.events.every(event => event.kind === "usage")) {
      session.status = "archived";
      if (session.test) session.test.freshness = "unknown";
    }
    for (const session of sessions) {
      if (session.status !== "active") continue;
      for (const other of sessions) {
        if (other === session || other.status !== "active") continue;
        const otherFiles = new Set(other.changedFiles.map(file => pathKey(file, other.cwd)).filter(Boolean));
        for (const file of session.changedFiles) {
          const key = pathKey(file, session.cwd);
          if (key && otherFiles.has(key)) session.conflicts.push({ file, sessionId: other.id });
        }
      }
    }
    return sessions;
  }
}

export const Activity = new ActivityStore();
