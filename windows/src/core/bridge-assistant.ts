import { invoke } from "@tauri-apps/api/core";
import { IS_TAURI, onEvent } from "./bridge";

export interface VoiceOption { id: string; label: string }
/** `model` is the GPT-Live delegation (backend) model; `voices` labels are Persian names. */
export interface VoiceStatus { configured: boolean; model: string; voice: string; voices: VoiceOption[]; models: string[] }
/** An app tool (MCP server, Windows, agent computer) offered to the live session; `readOnly` = runs without approval. */
export interface AppTool { name: string; title: string; server: string; readOnly: boolean }
/** `clock_now`: system clock corrected by a configured service's HTTP Date (OpenAI with a voice key, else Roadeep). */
export interface ClockNow {
  epochMs: number; isoLocal: string; utcOffsetMinutes: number; timezoneName: string | null;
  jalali: { y: number; m: number; d: number; monthNameFa: string }; gregorian: { y: number; m: number; d: number };
  weekdayFa: string; weekdayEn: string; source: "online" | "system"; onlineHost: string | null; skewSeconds: number | null;
}
/** What Roadeep learned about the user (local `memory.json`); times are epoch ms. */
export type MemoryCategory = "preference" | "habit" | "style" | "fact";
export interface MemoryFact { id: string; text: string; category: MemoryCategory; createdAt: number; updatedAt: number }
export interface MemoryStats {
  toolCounts: Record<string, number>; hourHistogram: number[]; weekdayHistogram: number[];
  recent: { at: number; tool: string; summary: string }[];
}
export interface MemoryView { enabled: boolean; facts: MemoryFact[]; stats: MemoryStats }
export interface VoiceSession { id: string; sdp: string; appTools: AppTool[] }
export interface ComputerStatus {
  available: boolean; state: "unavailable" | "stopped" | "running" | "paused";
  agentId: string; error?: string | null; takeover: boolean;
}
export type ComputerOperation =
  | { action: "navigate"; url: string } | { action: "screenshot" }
  | { action: "click"; x: number; y: number } | { action: "type"; text: string }
  | { action: "key"; key: string } | { action: "scroll"; deltaY: number }
  | { action: "list"; path?: string } | { action: "read"; path: string }
  | { action: "write"; path: string; text: string } | { action: "terminal"; command: string };
export interface ComputerResult { ok: boolean; data: unknown; image?: string | null }
export interface ComputerSetup { buildCommand: string; runtimePath: string }
export interface ComputerSnapshot { agentId: string; image: string; data: unknown }
export const onComputerSnapshot = (handler:(snapshot:ComputerSnapshot)=>void) => onEvent("computer-snapshot",handler);

// These calls deliberately reject: an unavailable provider must never look like success.
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) throw new Error("ASSISTANT_NATIVE_REQUIRED");
  try { return await invoke<T>(command, args); }
  catch (cause) { throw cause instanceof Error ? cause : new Error(typeof cause === "string" ? cause : "ASSISTANT_OPERATION_FAILED"); }
}
export const BridgeAssistant = {
  voiceStatus: () => call<VoiceStatus>("voice_status"),
  voiceConfigure: (model: string, voice: string, apiKey?: string) => call<VoiceStatus>("voice_configure", { model, voice, apiKey }),
  voiceClearKey: () => call<void>("voice_clear_key"),
  voiceStart: (sdp: string) => call<VoiceSession>("voice_start", { sdp }),
  voiceEnd: (id?: string) => call<void>("voice_end", { id }),
  clockNow: () => call<ClockNow>("clock_now"),
  /** island + settings. Errors: "memory-disabled", "memory-secret", "memory-invalid", "memory-io". */
  memoryGet: () => call<MemoryView>("memory_get"),
  memoryRemember: (text: string, category: MemoryCategory) => call<MemoryFact>("memory_remember", { text, category }),
  memoryForget: (query: string) => call<number>("memory_forget", { query }),
  /** island only; a no-op while memory is disabled. */
  memoryRecord: (tool: string, summary: string) => call<void>("memory_record", { tool, summary }),
  /** settings only. */
  memoryDelete: (id: string) => call<void>("memory_delete", { id }),
  memoryClear: () => call<void>("memory_clear"),
  memorySetEnabled: (enabled: boolean) => call<void>("memory_set_enabled", { enabled }),
  /** Island only; rejects unless `approved` for a tool whose `readOnly` is false. */
  voiceAppTool: (name: string, args: Record<string, unknown>, approved: boolean) => call<string>("voice_app_tool", { name, arguments: args, approved }),
  computerStatus: (agentId: string) => call<ComputerStatus>("computer_status", { agentId }),
  computerSetup: () => call<ComputerSetup>("computer_setup"),
  computerLifecycle: (agentId: string, action: "start" | "pause" | "resume" | "stop" | "reset") => call<ComputerStatus>(`computer_${action}`, { agentId }),
  computerTakeover: (agentId: string, on: boolean) => call<ComputerStatus>("computer_takeover", { agentId, on }),
  computerOperate: (agentId: string, operation: ComputerOperation) => call<ComputerResult>("computer_operate", { agentId, operation }),
};
