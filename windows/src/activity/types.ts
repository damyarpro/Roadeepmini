import type { CodingProvider } from "./providers";
export interface CodingEvent {
  id: string;
  sessionId: string;
  harness: CodingProvider;
  source?: "hook" | "log" | "both";
  at: number;
  kind: "session" | "prompt" | "tool" | "finished" | "error" | "cancelled" | "usage";
  context?: CodingContext;
  usage?: CodingUsage;
  testSummary?: Pick<TestResult, "verdict" | "passed" | "failed" | "skipped">;
  cwd?: string;
  title?: string;
  tool?: string;
  callId?: string;
  phase?: "started" | "completed" | "failed";
  command?: string;
  output?: string;
  exitCode?: number;
  files?: string[];
  patch?: string;
}

export interface CodingContext { usedTokens?: number; limitTokens?: number; usedPercent?: number; model?: string }
export interface UsageWindow { usedPercent: number; resetsAt?: number; windowMinutes?: number }
export interface CodingUsage { primary?: UsageWindow; secondary?: UsageWindow }
export interface CodingHistorySnapshot { revision: number; events: unknown[] }
export interface GitInspection { root: string; head?: string; at: number; files: { path: string; status: string }[]; patch: string; truncated: boolean }
export interface HandoffAgent { id: "codex" | "claude" | "gemini"; name: string; executable: string }
export interface HandoffResult { path: string; agent: string }

export interface DiffPreview {
  lines: { kind: "context" | "added" | "removed" | "header"; text: string }[];
  added: number;
  removed: number;
  truncated: boolean;
}

export interface TestResult {
  verdict: "passed" | "failed" | "unknown" | "skipped";
  passed: number;
  failed: number;
  skipped: number;
  reason: string;
}

export interface ActivitySession {
  id: string;
  harness: CodingEvent["harness"];
  cwd?: string;
  title: string;
  status: "active" | "finished" | "error" | "cancelled" | "archived";
  context?: CodingContext;
  usage?: CodingUsage;
  updatedAt: number;
  events: CodingEvent[];
  changedFiles: string[];
  lastPatch?: { eventId: string; at: number; files: string[]; preview: DiffPreview };
  test?: TestResult & { eventId: string; at: number; command: string; freshness: "current" | "stale" | "unknown" };
  lastMutationAt?: number;
  conflicts: { file: string; sessionId: string }[];
  counts: { tools: number; errors: number };
}
