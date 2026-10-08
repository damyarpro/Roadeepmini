// Claude Code hook events → island state.
// Port of HookServer.processEvent / processPermissionRequest from the macOS app.
// Difference from macOS: no terminal filter. On Windows the hook fires from any
// terminal (Windows Terminal, VS Code, PowerShell…) and all of them are handled.

import { Bridge, onEvent } from "../core/bridge";
import { askedQuestions, type DiffHookFields } from "../core/bridge-hooks";
import { buildFileDiff, fileName, makeDiffStep, toOneLine } from "../core/diff";
import { Sound } from "../core/sound";
import { State } from "../core/state";
import type { Island } from "./island";
import { t } from "../core/i18n";
import { observeCodingHook } from "../activity/claude";
import { CODING_PROVIDERS, codingTaskId, isCodingProvider } from "../activity/providers";
import type { CodingProvider } from "../activity/providers";
import { beginApproval, endApproval, pendingQuestions } from "./approvals";
import { appendSessionDiff, clearSessionTrail, countStep, setFinalLine } from "./live-session";

/** The return to idle that Stop arms, per pill, so the next turn can cancel it. */
const stopTimers = new Map<string, number>();

function cancelStopTimer(id: string): boolean {
  const timer = stopTimers.get(id);
  if (timer == null) return false;
  window.clearTimeout(timer);
  stopTimers.delete(id);
  return true;
}

/** Clears the approval card if no decision was made before the hook gave up. */
let pendingTimeout: number | null = null;

/** Events after which a pending request of the same session is moot. */
const TURN_OVER = new Set(["Stop", "StopFailure", "UserPromptSubmit", "SessionEnd", "Interrupt"]);

/**
 * The card is answered elsewhere or withdrawn: it goes, its pill carries on,
 * and the island settles (a live-voice card behind it comes back).
 */
function dropPendingCard(island: Island) {
  if (!State.pendingApproval) return;
  if (pendingTimeout != null) window.clearTimeout(pendingTimeout);
  pendingTimeout = null;
  endApproval();
  island.cardCleared();
  State.notify();
}

export interface HookPayload extends DiffHookFields {
  provider?: string;
  original_event?: string;
  hook_event_name?: string;
  request_id?: string;
  session_id?: string;
  cwd?: string;
  message?: string;
  /** UserPromptSubmit carries `prompt`; `message` belongs to Notification/Stop. */
  prompt?: string;
  /** Stop: the turn's final answer (Markdown), when the relay forwards it. */
  last_assistant_message?: string;
  tool_name?: string;
  tool_use_id?: string;
  notification_type?: string;
  tool_input?: Record<string, unknown>;
}

const PROJECT_ALIASES: Record<string, string> = {
  "notch-buddy": "Notch Buddy",
  notchbuddy: "Notch Buddy",
  notch_buddy: "Notch Buddy",
};

function aliasProjectName(name: string): string {
  return PROJECT_ALIASES[name.toLowerCase()] ?? name;
}

function lastPathComponent(p: string): string {
  const cleaned = p.replace(/[\\/]+$/, "");
  const idx = Math.max(cleaned.lastIndexOf("\\"), cleaned.lastIndexOf("/"));
  return idx >= 0 ? cleaned.slice(idx + 1) : cleaned;
}

/**
 * frenchStep() — same verbs as the macOS app, translated. The label is fixed
 * when the step arrives, so switching language only affects new steps.
 */
const TOOL_LABEL_KEYS: Record<string, string> = {
  Bash: "tool.run",
  Read: "tool.read",
  Write: "tool.write",
  Edit: "tool.edit",
  Glob: "tool.find",
  Grep: "tool.search",
  WebSearch: "tool.webSearch",
  WebFetch: "tool.fetch",
  run_command: "tool.run",
  view_file: "tool.read",
  write_to_file: "tool.write",
  replace_file_content: "tool.edit",
  read_url_content: "tool.fetch",
  search_web: "tool.webSearch",
  TodoWrite: "tool.tasks",
  Task: "tool.agent",
  LS: "tool.list",
  MultiEdit: "tool.edit",
  NotebookEdit: "tool.notebook",
  PowerShell: "tool.run",
};

function stepLabel(tool: string, input: Record<string, unknown>): string {
  const key = TOOL_LABEL_KEYS[tool];
  const label = key ? t(key) : tool;
  const str = (k: string) => (typeof input[k] === "string" ? (input[k] as string) : null);
  const cmd = str("command");
  if (cmd) return `${label} · ${cmd.slice(0, 40)}`;
  const path = str("path");
  if (path) return `${label} · ${lastPathComponent(path)}`;
  const file = str("file_path");
  if (file) return `${label} · ${lastPathComponent(file)}`;
  const query = str("query");
  if (query) return `${label} · ${query.slice(0, 40)}`;
  return label;
}

/**
 * What the Allow button actually authorises. Approving "Write" tells you nothing
 * — approving `Write · C:\…\.env` tells you everything, and the difference is
 * the whole point of approving from the island rather than blind.
 *
 * Ordered by how specific the field is, so an unfamiliar tool still shows
 * whatever identifying string it carries instead of falling back to its name.
 */
const APPROVAL_FIELDS = [
  "command", // Bash, PowerShell
  "file_path", // Write, Edit, MultiEdit, NotebookEdit
  "path", // Read, LS
  "url", // WebFetch
  "query", // WebSearch
  "pattern", // Glob, Grep
  "prompt", // Task
] as const;

function approvalTarget(tool: string, input: Record<string, unknown>): string {
  for (const field of APPROVAL_FIELDS) {
    const value = input[field];
    if (typeof value === "string" && value.trim()) {
      return `${tool} · ${value.trim()}`;
    }
  }
  return tool;
}

function upsert(taskId: string, projectName: string, cwd: string) {
  const t = State.tasks.find((x) => x.id === taskId);
  if (!t) return;
  t.directHook = true;
  t.name = projectName;
  if (cwd) t.sessionCwd = cwd;
}

function clearSession(provider: CodingProvider, taskId: string) {
  clearSessionTrail(taskId);
  const t = State.tasks.find((x) => x.id === taskId);
  if (!t) return;
  t.steps = [];
  t.stepIndex = 0;
  // Back to the pill's own name: Roadeep's for Claude Code, the agent's otherwise.
  t.name = provider === "claude" ? "Roadeep" : CODING_PROVIDERS[provider];
  t.pillBadge = null;
  t.sessionId = null;
}

/**
 * State.appendStep, counted: the step list keeps the last 20, the session's
 * own count goes on (the ticker scrolls by it).
 */
function appendStep(taskId: string, step: string) {
  const task = State.tasks.find((x) => x.id === taskId);
  if (!task) return;
  const before = task.steps.length;
  State.appendStep(taskId, step);
  countStep(taskId, before);
}

/**
 * PostToolUse of Edit / MultiEdit / Write → a diff stored for the pill and a
 * ticker step with its +N −M counts. Nothing is kept for a pill that does not
 * exist, so a stray event cannot grow memory. An edit the relay had to cut or
 * mask would give wrong counts: the PreToolUse step ("Edits · file") stands alone.
 */
function recordDiff(taskId: string, payload: HookPayload) {
  if (payload.roadeep_diff_truncated) return;
  if (!State.tasks.some((x) => x.id === taskId)) return;
  const diff = buildFileDiff(payload.tool_name ?? "", payload.tool_input ?? {});
  if (!diff) return;
  const id = appendSessionDiff(taskId, diff);
  appendStep(taskId, makeDiffStep(fileName(diff.path), diff.added, diff.removed, id));
}

export function registerHookHandlers(island: Island) {
  void onEvent<HookPayload>("hook", (payload) => handleHook(island, payload));
}

export function handleHook(island: Island, payload: HookPayload) {
  if (payload.provider !== undefined && !isCodingProvider(payload.provider)) {
    if (payload.request_id) void Bridge.approvalDecline(payload.request_id);
    return;
  }
  const provider: CodingProvider = payload.provider as CodingProvider | undefined ?? "claude";
  const taskId = codingTaskId(provider);
  if (State.paused) {
    // Silence here used to cost Claude Code nearly two minutes: the relay waited
    // for a decision from an island that had already decided not to look. Say so,
    // and the terminal takes the question immediately.
    if (payload.request_id) void Bridge.approvalDecline(payload.request_id);
    return;
  }

  const name = payload.hook_event_name ?? "";
  if (!State.tasks.some(task => task.id === taskId)) {
    State.tasks.push({id:taskId,name:CODING_PROVIDERS[provider],color:"#F5F6F8",state:"idle",steps:[],stepIndex:0,source:"agent",isIntegration:true,directHook:true});
  }
  State.tasks.find(task=>task.id===taskId)!.directHook=true;
  if (payload.session_id) State.tasks.find(task=>task.id===taskId)!.sessionId = payload.session_id;
  observeCodingHook(payload);

  // The turn that asked for a permission is over — answered in the terminal,
  // interrupted, or a new prompt — so the card would be lying. It goes, and the
  // relay is released without a decision. Same rule as the Mac.
  const pending = State.pendingApproval;
  if (pending && TURN_OVER.has(name) && pending.taskId === taskId &&
      pending.sessionId === (payload.session_id ?? "")) {
    void Bridge.approvalDecline(pending.requestId);
    dropPendingCard(island);
  }

  if (["UserPromptSubmit", "PreToolUse"].includes(name) && State.focusId === "integration_claude" &&
      ["idle", "finished", "error"].includes(State.focusTask?.state ?? "") &&
      !State.pendingApproval && !State.stateOverride && State.view !== "prompt") State.focusId = taskId;
  const cwd = payload.cwd ?? "";
  const raw = lastPathComponent(cwd);
  const projectName = `${CODING_PROVIDERS[provider]} · ${aliasProjectName(raw || t("hook.session"))}`;
  // Read after a card is dropped: its pill may have handed the front back.
  const focused = State.focusId === taskId;

  /**
   * Called where a handler is about to replace `finished` with a newer state:
   * the timer Stop armed would otherwise put the pill back to idle over it. The
   * badge that timer was going to clear goes now.
   */
  const supersedeStop = () => {
    if (cancelStopTimer(taskId)) State.setPillBadge(taskId, null);
  };

  /** Alerts force the island open; work events only reveal the compact island. */
  const surface = (view: Parameters<Island["alert"]>[0], isAlert: boolean) => {
    // A dropped file keeps the island until the user picks what to do with it:
    // a turn ending meanwhile only marks the pill (approvals come in through
    // island.alert directly, not here).
    if (island.inDropFlow) {
      if (isAlert && (view === "finished" || view === "error")) State.setPillBadge(taskId, view);
      return;
    }
    if (State.mode === "expanded") {
      if (isAlert) island.setView(view);
    } else if (isAlert) {
      island.alert(view);
    } else if (State.mode === "hidden") {
      island.reveal();
    }
  };

  switch (name) {
    case "SessionStart":
      upsert(taskId, projectName, cwd);
      setFinalLine(taskId, null);
      surface("overview", false);
      Sound.play("work");
      break;

    case "UserPromptSubmit": {
      upsert(taskId, projectName, cwd);
      supersedeStop();
      setFinalLine(taskId, null);
      State.updateTask(taskId, "thinking");
      // The field is `prompt`; reading `message` meant this step was always blank.
      const asked = payload.prompt ?? payload.message;
      if (asked) appendStep(taskId, asked.slice(0, 60));
      surface("overview", false);
      break;
    }

    case "PreToolUse": {
      upsert(taskId, projectName, cwd);
      supersedeStop();
      setFinalLine(taskId, null);
      State.updateTask(taskId, "working");
      const tool = payload.tool_name ?? t("hook.tool");
      appendStep(taskId, stepLabel(tool, payload.tool_input ?? {}));
      surface("overview", false);
      break;
    }

    case "PostToolUse":
      upsert(taskId, projectName, cwd);
      supersedeStop();
      // The question was answered in the terminal: the card would be lying.
      if (payload.tool_name === "AskUserQuestion" && pendingQuestions() &&
          State.pendingApproval?.taskId === taskId &&
          State.pendingApproval.sessionId === (payload.session_id ?? "")) {
        dropPendingCard(island);
      }
      State.updateTask(taskId, "thinking");
      recordDiff(taskId, payload);
      break;

    case "PostToolUseFailure":
      supersedeStop();
      State.updateTask(taskId, "working");
      appendStep(taskId, t("hook.failed"));
      break;

    case "Notification": {
      const message = payload.message ?? "";
      const lower = message.toLowerCase();
      if(payload.notification_type === "permission_prompt"){
        supersedeStop();
        State.updateTask(taskId,"question");
        appendStep(taskId, t("hook.upstreamPermission"));
        Sound.play("approval");surface("overview",false);
      } else if (lower.includes("rate limit") || lower.includes("limite d")) {
        supersedeStop();
        State.updateTask(taskId, "ratelimit");
        Sound.play("rate");
      } else if (message.endsWith("?")) {
        supersedeStop();
        State.updateTask(taskId, "question");
        appendStep(taskId, message);
      }
      break;
    }

    case "Stop": {
      State.updateTask(taskId, "finished");
      // Claude Code puts the turn's answer in the Stop payload itself; its first
      // paragraph becomes the last step and the finished card's line, and the
      // ticker holds still on it until the next turn starts.
      const finalText = toOneLine(payload.last_assistant_message ?? payload.message ?? "");
      if (finalText) appendStep(taskId, finalText);
      setFinalLine(taskId, finalText || null);
      Sound.play("finish");
      // A card waiting for an answer is never covered by another alert.
      if (focused && !State.pendingApproval) surface("finished", true);
      else State.setPillBadge(taskId, "finished");
      cancelStopTimer(taskId);
      stopTimers.set(taskId, window.setTimeout(() => {
        stopTimers.delete(taskId);
        State.updateTask(taskId, "idle");
        State.setPillBadge(taskId, null);
      }, 5200));
      break;
    }

    case "StopFailure":
      supersedeStop();
      State.updateTask(taskId, "error");
      Sound.play("error");
      if (focused && !State.pendingApproval) surface("error", true);
      else State.setPillBadge(taskId, "error");
      break;

    case "Interrupt":
    case "SessionEnd":
      // Nothing left for the timer to do, and it must not outlive the session.
      cancelStopTimer(taskId);
      State.updateTask(taskId, "idle");
      clearSession(provider, taskId);
      break;

    case "SubagentStart":
      appendStep(taskId, t("hook.subagent"));
      break;

    case "SubagentStop":
      appendStep(taskId, t("hook.subagentDone"));
      break;

    case "PermissionRequest": {
      if (provider !== "claude" && provider !== "codex") {
        if (payload.request_id) void Bridge.approvalDecline(payload.request_id);
        break;
      }
      const requestId = payload.request_id ?? "";
      // One card, one request. A second one must never quietly replace the first
      // — that would leave a human staring at request B while request A waits for
      // a decision nobody can give. Hand it straight back to the terminal.
      if (State.pendingApproval && State.pendingApproval.requestId !== requestId) {
        if (requestId) void Bridge.approvalDecline(requestId);
        break;
      }
      upsert(taskId, projectName, cwd);
      supersedeStop();
      if (pendingTimeout != null) window.clearTimeout(pendingTimeout);
      const tool = payload.tool_name ?? t("hook.tool");
      const input = payload.tool_input ?? {};
      if (!requestId) break;
      // Claude Code asking a question is not a permission to grant: the card
      // shows its options and sends back what was picked. Only Claude Code asks
      // this way; the relay forwards the questions only when all of them fit.
      const questions = provider === "claude" ? askedQuestions(tool, input) : null;
      const view = questions ? "question" : "approval";
      // The card always comes up, even over another pill or an island that is
      // already open: its pill comes to the front, and the one you were on
      // comes back once you answer (Mac #117, #120).
      beginApproval({
        taskId, provider,
        requestId,
        sessionId: payload.session_id ?? "",
        tool,
        command: approvalTarget(tool, input),
      }, questions);
      // The relay's short ack window closes in 800 ms; everything below this
      // line is synchronous, so the card really is up by the time it lands.
      void Bridge.approvalAck(requestId);
      State.updateTask(taskId, view);
      Sound.play(view);
      island.alert(view);
      // Roadeep answers within 108 s or not at all; after that the terminal has
      // taken over and the card would be lying.
      pendingTimeout = window.setTimeout(() => {
        pendingTimeout = null;
        if (State.pendingApproval?.requestId === requestId) dropPendingCard(island);
      }, 110_000);
      break;
    }

    default:
      break;
  }
  State.notify();
}
