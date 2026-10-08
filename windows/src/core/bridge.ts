// Thin wrapper over the Tauri commands/events. Every call is a no-op when the
// page is opened in a plain browser, so the island can be iterated on with
// `npm run dev` alone.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Settings } from "./state";
import type { DockEdge, DockLayout } from "../island/dock";
import type { CodingHistorySnapshot, GitInspection, HandoffAgent, HandoffResult } from "../activity/types";

export const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!IS_TAURI) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    console.error(`[roadeep] ${cmd} failed`, err);
    return null;
  }
}

export interface BootInfo {
  settings: Settings;
  /** Logical screen rect of the monitor the island lives on. */
  screen: { x: number; y: number; width: number; height: number; scale: number };
  /** The island's layout in its window for the current dock and display. */
  layout: DockLayout;
  version: string;
  hookPath: string;
}

export const Bridge = {
  codingSnapshot: () => callOrThrow<unknown[]>("coding_snapshot"),
  codingStatus: () => callOrThrow<{ enabled: boolean; available: boolean; error?: string }>("coding_status"),
  codingClear: () => callOrThrow<void>("coding_clear"),
  codingHistorySnapshot: () => callOrThrow<CodingHistorySnapshot>("coding_history_snapshot"),
  codingHistorySave: (events: unknown[], revision: number) => callOrThrow<boolean>("coding_history_save", { events, revision }),
  codingHistoryClear: () => callOrThrow<CodingHistorySnapshot>("coding_history_clear"),
  codingGitInspect: (cwd: string) => callOrThrow<GitInspection>("coding_git_inspect", { cwd }),
  codingHandoffAgents: () => callOrThrow<HandoffAgent[]>("coding_handoff_agents"),
  codingHandoff: (cwd: string, agent: string, content: string) => callOrThrow<HandoffResult>("coding_handoff", { cwd, agent, content }),
  codingExport: (content: string) => callOrThrow<string>("coding_export", { content }),
  saveSettingsChecked: (settings: Settings) => callOrThrow<void>("save_settings", { settings }),
  boot: () => call<BootInfo>("boot"),

  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),

  /** Shrink the window down to the invisible wake strip (hidden) or back to full. */
  setCollapsed: (collapsed: boolean) => call<void>("set_collapsed", { collapsed }),

  /**
   * Pushes the island shape in window coordinates. Rust flips click-through from
   * its own cursor poll, so the flag is never a frame behind a click.
   */
  setIslandRect: (x: number, y: number, width: number, height: number) =>
    call<void>("set_island_rect", { x, y, width, height }),

  /** Give the window keyboard focus (chat field) and take it away again. */
  focusWindow: (focused: boolean) => call<void>("focus_window", { focused }),

  reposition: () => call<void>("reposition"),

  /**
   * Rust takes over and moves the window with the cursor until the button is
   * released (or Esc). `cx`, `cy`: the compact island's centre in the window;
   * `halfW`, `halfH`: its half size. False/null: no drag.
   */
  dragStart: (cx: number, cy: number, halfW: number, halfH: number, reducedMotion: boolean) =>
    call<boolean>("drag_start", { cx, cy, halfW, halfH, reducedMotion }),
  /** Answer to "dock-move": laid out for the new dock; the island moved by (dx, dy) in the window. */
  dockLayoutReady: (dx: number, dy: number) => call<void>("dock_layout_ready", { dx, dy }),
  /** Docks the island to an edge (menu, settings). `reset` also forgets a dragged-to display. */
  dockSet: (edge: DockEdge, pos: number, reset: boolean, reducedMotion: boolean) =>
    call<boolean>("dock_set", { edge, pos, reset, reducedMotion }),
  /** Slides the window to (or back from) the maximised chat's spot. */
  setMaximized: (on: boolean, reducedMotion: boolean) => call<void>("set_maximized", { on, reducedMotion }),

  openUrl: (url: string) => call<void>("open_url", { url }),

  /** "Open terminal" → opens the folder in VS Code when `code` is on PATH. */
  openInVSCode: (path: string | null) => call<boolean>("open_in_vscode", { path }),

  quit: () => call<void>("quit_app"),

  /** `section` jumps the settings window to that nav section (e.g. "account"). */
  openSettingsWindow: (section?: string) => call<void>("open_settings_window", { section: section ?? null }),

  /** Writes to %LOCALAPPDATA%\com.roadeep.desktop\roadeep.log, next to the Rust lines. */
  log: (message: string) => call<void>("log_line", { message }),

  // ── Claude Code hooks ─────────────────────────────────────────────────────
  hooksStatus: () => call<HookStatus>("hooks_status"),
  codingHooksStatus: () => callOrThrow<CodingHookStatus[]>("coding_hooks_status"),
  codingHooksPreview: (provider: string, projectPath: string | null = null, remove = false) =>
    callOrThrow<CodingHookPreview>("coding_hooks_preview", { provider, projectPath, remove }),
  codingHooksApply: (provider: string, fingerprint: string, projectPath: string | null = null, remove = false) =>
    callOrThrow<CodingHookStatus>("coding_hooks_apply", { provider, projectPath, fingerprint, remove }),
  /** Diff to show before anything is written. `install: false` previews removal. */
  hooksPreview: (install: boolean) => callOrThrow<HookPreview>("hooks_preview", { install }),
  /**
   * Writes ~/.claude/settings.json — only ever after an explicit click, and only
   * when the file still matches the preview the user looked at.
   */
  hooksApply: (install: boolean, fingerprint: string) =>
    callOrThrow<string>("hooks_apply", { install, fingerprint }),

  approvalDecision: (requestId: string, decision: "allow" | "deny") =>
    call<void>("approval_decision", { requestId, decision }),
  /** "The card is up" — until this lands the relay only waits a moment. */
  approvalAck: (requestId: string) => call<void>("approval_ack", { requestId }),
  /** "Nobody can act on this" — Claude Code asks in the terminal right away. */
  approvalDecline: (requestId: string) => call<void>("approval_decline", { requestId }),

  // ── Chat, files, secrets ──────────────────────────────────────────────────
  /**
   * One chat turn over Roadeep. Tokens and file bytes never leave Rust.
   * - `context` file → uploaded as an attachment (inbox copies only, max 20 MB).
   * - `context` window → prefixed to the first message of the thread.
   * - `agentId` → an id from `roadeepAgents()`, or `localAgentRef(id)` for a local
   *   agent (its instructions lead the first message of a new thread; its model,
   *   web search and base agent apply); omit for plain chat.
   * The model (settings.model, "" = server default) applies to new threads only;
   * with settings.language "fa" the first message asks for a Persian reply.
   * settings.chat{WebSearch,Reasoning,DeepResearch} ride along with every message,
   * minus whatever the model or the plan does not allow.
   *
   * `turn` is the caller's own counter: while the send is in flight the island
   * receives "chat-stream" events tagged with it (`onChatStream`) — the reply
   * text as it streams, status changes, and tool approvals to decide with
   * `chatApprovalDecide`. Over the WebSocket there are deltas; when the app has
   * to poll instead there are none, the rest is identical.
   * With settings.chatTools on (and the conversation's tools chip not off) and
   * an MCP server enabled, the model may call the servers' tools: Rust runs the
   * loop, sends `toolStep` events, and asks for "ask"-mode tools with an
   * `approval` whose id starts with LOCAL_APPROVAL_PREFIX.
   * Resolves with the FINAL text (and the thread / assistant message ids).
   * Rejects with a `RoadeepError`; `String(err)` still reads "Error: <message>".
   */
  chatSend: (query: string, context: ChatContext | null, agentId?: string | null, turn = 0, voiceTask = false) =>
    roadeepCall<ChatReply>("chat_send", { query, context, agentId: agentId ?? null, turn, voiceTask }),
  /** Starts a new thread on the next message (and abandons an in-flight reply). */
  chatReset: () => call<void>("chat_reset"),
  chatModelSet: (model: string) => roadeepCall<string>("chat_model_set", { model }),
  /** Cancels the reply being generated (also while it waits for an approval);
   *  the pending chatSend rejects with code CANCELLED. */
  chatCancel: () => roadeepCall<void>("chat_cancel"),
  /**
   * Answers the tool approval of the turn in flight (a "chat-stream" `approval`
   * event). The pending chatSend keeps going under the resumed job and resolves
   * with its reply; an `approvalDone` event follows. Rejects with
   * APPROVAL_ALREADY_DECIDED (second click, or decided elsewhere) or
   * APPROVAL_NOT_FOUND (not the approval the current turn waits on).
   */
  /**
   * Whether a chatSend is in flight in Rust, and its `turn`. A rebuilt UI
   * (language change…) uses it to re-attach to the running turn: its
   * "chat-stream" events keep that turn id. A second chatSend meanwhile rejects
   * with CHAT_BUSY.
   */
  chatTurnState: () => roadeepCall<ChatTurnState>("chat_turn_state"),
  chatApprovalDecide: (approvalId: string, decision: ChatApprovalDecision) =>
    roadeepCall<void>("chat_approval_decide", { approvalId, decision }),
  /**
   * The island's MCP tools chip. `defaultOn` is settings.chatTools (what a new
   * conversation starts with); `count: true` connects the enabled servers
   * (lazily, up to 15 s each) to count the tools the model would be offered.
   */
  chatToolsState: (defaultOn: boolean, count: boolean) =>
    roadeepCall<ChatToolsState>("chat_tools_state", { defaultOn, count }),
  /** Tools on or off for this conversation only (a new chat starts from the setting). */
  chatToolsSet: (on: boolean) => roadeepCall<void>("chat_tools_set", { on }),
  /**
   * One page of the account's chat conversations, newest first. `limit` is
   * capped at 30; page with `offset += limit` while `hasMore`.
   */
  chatThreads: (offset: number, limit: number) =>
    roadeepCall<ChatThreadsPage>("chat_threads", { offset, limit }),
  /**
   * Loads a conversation (active messages only, chronological, at most the
   * last 200) and makes it the CURRENT thread: the next chatSend continues it.
   */
  chatThreadOpen: (threadId: string) => roadeepCall<ChatThreadHistory>("chat_thread_open", { threadId }),
  /** Deletes a conversation; when it is the current one the chat resets. */
  chatThreadDelete: (threadId: string) => roadeepCall<void>("chat_thread_delete", { threadId }),
  /** Copies a dropped file into the inbox. */
  ingestFile: (path: string) => callOrThrow<DroppedFile>("ingest_file", { path }),
  /** Only ever tells you whether a key exists — never its value. */
  secretPresent: (key: string) => call<boolean>("secret_present", { key }),
  /** Reads back a stored *setting* (e.g. "n8n-url"); secrets always come back null. */
  secretPublicValue: (key: string) => call<string | null>("secret_public_value", { key }),
  secretSet: (key: string, value: string) => callOrThrow<void>("secret_set", { key, value }),
  secretClear: (key: string) => callOrThrow<void>("secret_clear", { key }),

  // ── Roadeep account ───────────────────────────────────────────────────────
  // Every call rejects with a `RoadeepError`. Sign-ins and every sign-out are
  // also broadcast to both windows as "roadeep-session" (`onRoadeepSession`),
  // so a window only needs to listen to stay in step.

  /** Email + password sign-in. Bad input rejects with code VALIDATION_ERROR. */
  roadeepLogin: (email: string, password: string) =>
    roadeepCall<RoadeepSession>("roadeep_login", { email, password }),
  /** Sends the SMS code. Accepts 09xxxxxxxxx, +98…, 0098…, Persian digits. */
  roadeepOtpSend: (phone: string) => roadeepCall<void>("roadeep_otp_send", { phone }),
  /** Verifies the SMS code (4–8 digits, Persian digits accepted) and signs in. */
  roadeepOtpVerify: (phone: string, otp: string) =>
    roadeepCall<RoadeepSession>("roadeep_otp_verify", { phone, otp }),
  /** Always signs out locally (and resets the chat thread); the server call is best effort. */
  roadeepLogout: () => roadeepCall<void>("roadeep_logout"),
  /**
   * Current session. When signed in, the profile is revalidated against the
   * server; offline it falls back to the cached profile. A rejected session
   * resolves `{signedIn: false}` and fires the event with reason "expired".
   */
  roadeepSession: () => roadeepCall<RoadeepSession>("roadeep_session"),
  /** Chat models for this account. Requires a session (NOT_SIGNED_IN otherwise). */
  roadeepModels: () => roadeepCall<RoadeepModel[]>("roadeep_models"),
  /** Roadeep agents the user can chat with. Requires a session. */
  roadeepAgents: () => roadeepCall<RoadeepAgent[]>("roadeep_agents"),
  /**
   * The same agents split into this account's exclusive ones and the public
   * ones (`/v1/agents/catalog/`, or the plain list split by visibility).
   */
  roadeepAgentCatalog: () => roadeepCall<RoadeepAgentCatalog>("roadeep_agent_catalog"),
  /**
   * Asks the chosen model to draft a local agent from a goal, in a one-off
   * thread (never the island's). With `previous` + `feedback` it revises that
   * draft. Rejects with AGENT_DRAFT_INVALID when the model twice fails to send
   * usable JSON, AGENT_DRAFT_BUSY while another draft runs, CANCELLED on stop.
   */
  agentDraft: (req: AgentDraftRequest) =>
    roadeepCall<AgentSuggestion>("agent_draft", {
      goal: req.goal,
      model: req.model,
      language: req.language,
      previous: req.previous ?? null,
      feedback: req.feedback ?? null,
    }),
  /** Stops the draft being written; the pending agentDraft rejects with CANCELLED. */
  agentDraftCancel: () => roadeepCall<void>("agent_draft_cancel"),
  /**
   * Plan locks from the profile, plus plan name and token balance (null when
   * the plan could not be read). Also teaches chat_send which features to drop.
   */
  roadeepProfileLocks: () => roadeepCall<RoadeepAccountStatus>("roadeep_profile_locks"),
  /**
   * Spendable units and plan name, at most 60 s old. Fresh values are also
   * pushed to every window after each finished chat turn and on sign-in
   * (`onRoadeepBalance`).
   */
  roadeepBalance: () => roadeepCall<RoadeepBalance>("roadeep_balance"),

  // ── Local agents (%APPDATA%\Roadeep\agents.json) ───────────────────────────
  // Pass `localAgentRef(agent.id)` to chatSend to chat with one.
  localAgentsList: () => roadeepCall<LocalAgent[]>("local_agents_list"),
  /** Creates (no id) or updates. Bad input rejects with VALIDATION_ERROR + fieldErrors. */
  localAgentSave: (agent: LocalAgentDraft) => roadeepCall<LocalAgent>("local_agent_save", { agent }),
  localAgentDelete: (id: string) => roadeepCall<void>("local_agent_delete", { id }),

  // ── Integrations ──────────────────────────────────────────────────────────
  refreshIntegration: (id: string) => call<void>("refresh_integration", { id }),
  /** Opens the configured n8n instance in the browser. */
  openN8n: () => call<void>("open_n8n"),

  /** Tray → Pause. Stops the integration pollers, not just the island. */
  setPaused: (paused: boolean) => call<void>("set_paused", { paused }),
};

export interface IntegrationUpdate {
  id: string;
  data: Record<string, unknown>;
  error: string | null;
  event: { success: boolean; label: string; detail: string | null } | null;
}

export type ChatContext =
  | { kind: "file"; name: string; path: string }
  | { kind: "window"; appName: string; title: string; url?: string };

export interface ChatReply {
  model?: string | null;
  /** The final reply. */
  text: string;
  /** The thread the turn went to. */
  threadId: string | null;
  /** The persisted assistant message, when the server said. */
  messageId: string | null;
}

export type ChatApprovalDecision = "approve" | "reject";

export interface ChatTurnState {
  busy: boolean;
  /** The running turn's counter; null when idle. */
  turn: number | null;
}

export type ChatStreamStatus = "thinking" | "searching" | "tool" | "generating" | "queued";

export interface ChatApproval {
  id: string;
  /** Tool name as the server calls it (e.g. "start_generation"). */
  tool: string;
  /** The proposed arguments in one short line (server text: render with textContent). */
  summary: string;
  /**
   * Local (MCP tool, "local-" id) approvals only: exactly what will run — the
   * server's and the tool's names and the arguments as pretty JSON, unmasked.
   * Text only, shown LTR in full.
   */
  call?: { server: string; tool: string; arguments: string };
}

/**
 * "chat-stream" payload, island window only. `turn` echoes chatSend's `turn`;
 * drop events of any other turn.
 */
export type ChatStreamEvent =
  /** `text` is the FULL reply so far: replace, don't append. */
  | { turn: number; kind: "delta"; text: string }
  /** `label`: short server-provided text (usually Persian), or null. */
  | { turn: number; kind: "status"; status: ChatStreamStatus; label: string | null }
  /** The turn waits for `chatApprovalDecide(approval.id, ...)` (or chatCancel). */
  | { turn: number; kind: "approval"; approval: ChatApproval }
  | { turn: number; kind: "approvalDone"; approvalId: string; decision: ChatApprovalDecision }
  /** An MCP tool step, sent again on every state change (same `step.id`). */
  | { turn: number; kind: "toolStep"; step: ChatToolStep };

/**
 * Approvals the app asks itself for an MCP tool in "ask" mode carry this
 * prefix; they are decided with `chatApprovalDecide` like the server's.
 */
export const LOCAL_APPROVAL_PREFIX = "local-";

export type ChatToolStepState = "waiting" | "running" | "done" | "error" | "declined" | "stopped";

/** One MCP tool step of a turn (live) or of a reopened thread. Text only. */
export interface ChatToolStep {
  id: string;
  /** The server's name (live) or its short name (history). */
  server: string;
  tool: string;
  state: ChatToolStepState;
  /** Indented JSON, secret-looking values masked, cut short. */
  arguments: string;
  /** The result text, cut short; null until there is one (and for a declined step). */
  result: string | null;
  /** Coded error from the MCP client (`localizeError`), when the call could not run. */
  error: string | null;
}

/** What the island's tools chip shows. */
export interface ChatToolsState {
  /** At least one MCP server is enabled; without one there is no chip. */
  available: boolean;
  /** Tools are on for this conversation. */
  on: boolean;
  /** Tools the model would be offered; null when not counted. */
  count: number | null;
}

export interface ChatThreadSummary {
  id: string;
  /** "" when the conversation has no title: show a placeholder. */
  title: string;
  /** Last message as one plain line; "" when none. */
  preview: string;
  /** ISO 8601; "" when unknown. */
  updatedAt: string;
  model: string | null;
}

export interface ChatThreadsPage {
  items: ChatThreadSummary[];
  hasMore: boolean;
}

export interface ChatHistoryMessage {
  id: string;
  role: "user" | "assistant";
  /** Markdown as the model wrote it (user text without the app's hidden extras). */
  text: string;
  /** ISO 8601; "" when unknown. */
  createdAt: string;
  /**
   * Assistant messages: the MCP tool steps that led to this answer (the app's
   * own tool messages are folded away). A message with no text holds the
   * steps of a turn that ended without an answer.
   */
  toolSteps?: ChatToolStep[];
}

export interface ChatThreadHistory {
  id: string;
  model?: string | null;
  /** "" when untitled. */
  title: string;
  messages: ChatHistoryMessage[];
}

/** Spendable units and plan name; null when the server did not say. */
export interface RoadeepBalance {
  units: number | null;
  plan: string | null;
}

// ── Roadeep types ─────────────────────────────────────────────────────────────

export interface RoadeepUser {
  id: string;
  name: string | null;
  email: string | null;
  phone: string | null;
  avatar: string | null;
}

/** Returned by login / verify / session, and the "roadeep-session" event payload. */
export interface RoadeepSession {
  signedIn: boolean;
  user: RoadeepUser | null;
  /** Why the session ended; null while signed in. */
  reason: "logout" | "expired" | null;
}

export interface RoadeepModel {
  id: string;
  displayName: string;
  description: string | null;
  provider: string | null;
  /** The server's global default model (what settings.model "" resolves to). */
  isDefault: boolean;
  vision: boolean;
  fileInput: boolean;
  /** Feature gates: an unknown capability counts as available, except deep research. */
  reasoning: boolean;
  webSearch: boolean;
  deepResearch: boolean;
  tools: boolean;
}

export interface RoadeepPlanLocks {
  chatbot: boolean;
  agents: boolean;
  webSearch: boolean;
  reasoning: boolean;
  fileUpload: boolean;
}

export interface RoadeepAccountStatus {
  locks: RoadeepPlanLocks;
  planName: string | null;
  /** Spendable tokens. */
  walletUnits: number | null;
}

/** A custom agent stored on this PC. Timestamps are Unix milliseconds. */
export interface LocalAgent {
  id: string;
  name: string;
  instructions: string;
  /** "" = the model chosen in settings. */
  model: string;
  webSearch: boolean;
  /** Official Roadeep agent it builds on, sent as agent_id. */
  baseAgentId: string | null;
  /** One line under the name; "" when none. */
  description: string;
  /** Pill colour "#RRGGBB"; "" = derived from the id (`agentColor`). */
  color: string;
  /** Quick chips in an empty chat with this agent (≤ 5). */
  starterPrompts: string[];
  createdAt: number;
  updatedAt: number;
}

export type LocalAgentDraft = Pick<
  LocalAgent,
  "name" | "instructions" | "model" | "webSearch" | "baseAgentId" | "description" | "color" | "starterPrompts"
> & {
  id?: string | null;
};

export const LOCAL_AGENT_PREFIX = "local:";
export const LOCAL_AGENT_LIMITS = {
  maxAgents: 50, name: 60, instructions: 8000, description: 200, starterPrompts: 5, starterPrompt: 200,
} as const;

/** The chatSend / settings.chatAgent value for a local agent. */
export const localAgentRef = (id: string) => `${LOCAL_AGENT_PREFIX}${id}`;

/** Broadcast after a local agent is saved or deleted (any window). */
export const LOCAL_AGENTS_EVENT = "local-agents-changed";

export interface RoadeepAgent {
  id: string;
  title: string;
  shortDescription: string | null;
  iconUrl: string | null;
  starterPrompts: string[];
  /** "public" | "private" when the server says. */
  visibility: string | null;
}

export interface RoadeepAgentCatalog {
  /** Only this account (private). */
  exclusive: RoadeepAgent[];
  public: RoadeepAgent[];
}

/** What the model drafts; already within the local agent limits. */
export interface AgentSuggestion {
  name: string;
  description: string;
  instructions: string;
  starterPrompts: string[];
  webSearch: boolean;
  /** "#RRGGBB" or null. */
  suggestedColor: string | null;
}

export interface AgentDraftRequest {
  goal: string;
  /** Roadeep model id; "" = server default. */
  model: string;
  /** What the agent is written in. */
  language: "fa" | "en";
  previous?: AgentSuggestion | null;
  feedback?: string | null;
}

/**
 * Codes worth translating in the UI. Server codes pass through untouched, so
 * anything not listed falls back to `message` (English, or the server's own
 * text); unknown HTTP failures read `HTTP_<status>`.
 */
export type RoadeepErrorCode =
  // client side
  | "NETWORK_ERROR" // no connection / DNS / TLS
  | "TIMEOUT"
  | "NOT_SIGNED_IN" // no session at all: show the sign-in screen
  | "SESSION_EXPIRED" // refresh rejected: already signed out, sign in again
  | "INVALID_RESPONSE"
  | "VALIDATION_ERROR" // also server-side; see fieldErrors (email, password, phone, otp, message, agent_id)
  | "CANCELLED"
  | "AWAITING_APPROVAL" // agent builder only (island chat shows an approval instead)
  | "INSUFFICIENT_CREDITS" // every "not enough credit" rejection, whatever the server code
  | "JOB_TIMEOUT" // no reply within 180 s
  | "EMPTY_RESPONSE"
  | "FILE_TOO_LARGE" // > 20 MB
  | "FILE_UNREADABLE"
  | "UPLOAD_FAILED"
  | "NOT_IN_APP" // page opened outside Tauri
  | "UNKNOWN"
  // server side
  | "THROTTLED" // see retryAfter (seconds)
  | "FILE_UPLOAD_LIMIT_REACHED"
  | "MODEL_UNAVAILABLE"
  | "LLM_ERROR"
  | "CHAT_UNAVAILABLE"
  | "FEATURE_NOT_ALLOWED"
  | "THREAD_NOT_FOUND"
  | "JOB_NOT_FOUND"
  | "APPROVAL_ALREADY_DECIDED"
  | "APPROVAL_NOT_FOUND"
  | "CHAT_BUSY" // a reply is already being written (one turn at a time)
  | "AGENT_MEMORY_TRIGGER" // a local agent's instructions read as "save to memory"; see agentName
  // MCP tools in the chat
  | "TOOL_MEMORY_TRIGGER" // a tool result read as "save to memory": the model never saw it
  | "TOOL_STEP_LIMIT" // the model kept calling tools past the limit and wrote no answer
  | "TOOL_APPROVAL_TIMEOUT" // nobody answered a tool approval in time
  | "INTERNAL_ERROR"
  // local agents
  | "LOCAL_AGENT_LIMIT"
  | "LOCAL_AGENT_NOT_FOUND"
  | "LOCAL_AGENT_STORE_ERROR"
  // agent builder
  | "AGENT_DRAFT_INVALID"
  | "AGENT_DRAFT_BUSY";

/** The serialized Rust `RoadeepError` a Roadeep command rejects with. */
export interface RoadeepErrorPayload {
  code: RoadeepErrorCode | (string & {});
  message: string;
  status: number | null;
  /** Seconds to wait before retrying (429 only). */
  retryAfter: number | null;
  requestId: string | null;
  fieldErrors?: Record<string, string[]>;
  /** The local agent the turn ran with (AGENT_MEMORY_TRIGGER). */
  agentName?: string;
}

/**
 * What every Roadeep call rejects with. `name` stays "Error" on purpose so the
 * island's existing `String(err)` minus the "Error: " prefix keeps working.
 */
export class RoadeepError extends Error {
  readonly code: RoadeepErrorPayload["code"];
  readonly status: number | null;
  readonly retryAfter: number | null;
  readonly requestId: string | null;
  readonly fieldErrors: Record<string, string[]> | undefined;
  readonly agentName: string | undefined;

  constructor(payload: RoadeepErrorPayload) {
    super(payload.message);
    this.code = payload.code;
    this.status = payload.status;
    this.retryAfter = payload.retryAfter;
    this.requestId = payload.requestId;
    this.fieldErrors = payload.fieldErrors;
    this.agentName = payload.agentName;
  }
}

export const isRoadeepError = (value: unknown): value is RoadeepError =>
  value instanceof RoadeepError;

function toRoadeepError(raw: unknown): RoadeepError {
  if (raw instanceof RoadeepError) return raw;
  if (typeof raw === "object" && raw !== null) {
    const r = raw as Partial<RoadeepErrorPayload>;
    if (typeof r.code === "string" && typeof r.message === "string") {
      return new RoadeepError({
        code: r.code,
        message: r.message,
        status: typeof r.status === "number" ? r.status : null,
        retryAfter: typeof r.retryAfter === "number" ? r.retryAfter : null,
        requestId: typeof r.requestId === "string" ? r.requestId : null,
        fieldErrors: r.fieldErrors,
        agentName: typeof r.agentName === "string" ? r.agentName : undefined,
      });
    }
  }
  // Tauri itself rejects with a plain string (e.g. bad arguments).
  const message =
    typeof raw === "string" ? raw : raw instanceof Error ? raw.message : "Unexpected error";
  return new RoadeepError({ code: "UNKNOWN", message, status: null, retryAfter: null, requestId: null });
}

/** Like `callOrThrow`, but always rejects with a `RoadeepError`. */
async function roadeepCall<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) {
    throw new RoadeepError({
      code: "NOT_IN_APP",
      message: "not running inside the Roadeep app",
      status: null,
      retryAfter: null,
      requestId: null,
    });
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    const error = toRoadeepError(err);
    console.error(`[roadeep] ${cmd} failed`, error.code, error.requestId ?? "");
    throw error;
  }
}

export interface DroppedFile {
  name: string;
  path: string;
  size: number;
}

export interface HookStatus {
  installed: boolean;
  settingsPath: string;
  hookPath: string;
  hookReady: boolean;
  /** Hooks still run the relay from the previous version's folder; applying again updates them. */
  legacyRelay?: boolean;
}
export interface CodingHookStatus extends HookStatus {
  provider: string;
  label: string;
  supported: boolean;
  scope: "user" | "project" | "unavailable";
  detected: boolean;
  events: string[];
  permission: boolean;
  reason?: string;
}
export interface CodingHookPreview extends HookPreview { provider: string }

export interface HookPreview {
  diff: string;
  backup: string;
  settingsPath: string;
  /** Hand back to hooksApply so only the reviewed diff is ever written. */
  fingerprint: string;
}

/** Same as `call`, but surfaces the error so the UI can show what went wrong. */
async function callOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) throw new Error("not running inside the Roadeep app");
  return invoke<T>(cmd, args);
}

/** Broadcast to every window on sign-in and sign-out (logout or expiry). */
export const ROADEEP_SESSION_EVENT = "roadeep-session";
/** Island only, while a chatSend is in flight. */
export const CHAT_STREAM_EVENT = "chat-stream";
/** Every window, after each finished chat turn and on sign-in. */
export const ROADEEP_BALANCE_EVENT = "roadeep-balance";

export type BridgeEvent =
  | { name: "cursor"; payload: { x: number; y: number } }
  | { name: "tray"; payload: string }
  | { name: "hook"; payload: Record<string, unknown> }
  | { name: "screen-changed"; payload: null }
  | { name: "dock-layout"; payload: DockLayout }
  | { name: "dock-drag"; payload: DockDragFrame }
  | { name: "dock-move"; payload: { layout: DockLayout; wait: boolean } }
  | { name: "dock-settled"; payload: null }
  | { name: typeof ROADEEP_SESSION_EVENT; payload: RoadeepSession }
  | { name: typeof CHAT_STREAM_EVENT; payload: ChatStreamEvent }
  | { name: typeof ROADEEP_BALANCE_EVENT; payload: RoadeepBalance };

/** One frame of a drag (or of the spring into the dock after it). */
export interface DockDragFrame {
  /** Island velocity, logical px/s. */
  vx: number;
  vy: number;
  /** The edge a drop would dock to (the final one while settling). */
  edge: DockEdge;
  settling: boolean;
}

export interface DragDropPayload {
  type: "enter" | "over" | "drop" | "leave";
  paths?: string[];
}

/** Files dragged onto the island. Only reaches us when the window takes the mouse. */
export async function onDragDrop(handler: (e: DragDropPayload) => void) {
  if (!IS_TAURI) return () => {};
  return getCurrentWebview().onDragDropEvent((event) => {
    handler(event.payload as DragDropPayload);
  });
}

export async function onEvent<T>(name: string, handler: (payload: T) => void) {
  if (!IS_TAURI) return () => {};
  return listen<T>(name, (e) => handler(e.payload));
}

/** Sign-in / sign-out from any window. Resolves to the unlisten function. */
export function onRoadeepSession(handler: (session: RoadeepSession) => void) {
  return onEvent<RoadeepSession>(ROADEEP_SESSION_EVENT, handler);
}

/**
 * Live progress of the chatSend in flight (island window). Filter on `turn`.
 * Resolves to the unlisten function.
 */
export function onChatStream(handler: (event: ChatStreamEvent) => void) {
  return onEvent<ChatStreamEvent>(CHAT_STREAM_EVENT, handler);
}

/** A fresh balance (after a chat turn, on sign-in). Resolves to the unlisten function. */
export function onRoadeepBalance(handler: (balance: RoadeepBalance) => void) {
  return onEvent<RoadeepBalance>(ROADEEP_BALANCE_EVENT, handler);
}
