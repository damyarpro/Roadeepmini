// Chat view — DOM port of PromptView / ChatBubble / TypingDotsView from
// IslandViewContent.swift, plus the Roadeep bits: the agent chip, the stop
// button, the sign-in prompt, the reply streaming in (markdown), tool
// approvals, the history panel and the token balance. With MCP servers
// connected, the turn may run their tools (Rust drives the loop): each step is
// a row above the reply, and a chip in the input bar turns tools off for one
// conversation.
//
// The island rebuilds its views on a language switch, so everything a turn in
// flight needs (the pending send, the streamed text, the approvals) lives at
// module scope; a rebuilt view simply re-attaches to it.

import { h, svg, clear } from "./dom";
import { ICONS } from "./icons";
import {
  Bridge, IS_TAURI, isRoadeepError, localAgentRef, onChatStream, onRoadeepBalance, LOCAL_AGENT_PREFIX, LOCAL_APPROVAL_PREFIX,
  type ChatApproval, type ChatContext, type ChatStreamEvent, type ChatStreamStatus, type ChatToolStep,
  type ChatToolsState, type RoadeepBalance,
} from "../core/bridge";
import { onMcpcStatus } from "../core/bridge-mcpc";
import { localizeError } from "../core/error-text";
import { Sound } from "../core/sound";
import { State, type ChatMessage } from "../core/state";
import type { ViewHost } from "./views";
import { getLanguage, isolate, isRtl, registerMessages, t, textDirection } from "../core/i18n";
import { toolsEn } from "../core/locales/tools-en";
import { toolsFa } from "../core/locales/tools-fa";
import { refreshAgents } from "../island/roadeep";
import { isInsufficientCredits, isSignedOutError, roadeepErrorText } from "./errors";
import { renderMarkdown } from "./markdown";
import { buildHistory } from "./history";
import { createBalanceChip, createCreditAlert, createIslAlert, ISLAND_CLOCK_ICON } from "./shared";
import { CHAT_MAX_H } from "../core/layout";
import { opensMenuFromChat } from "./palette";
import { createChatModelPicker } from "./chat-models";
import { createAssistantControls } from "./assistant-controls";
import { BridgeLocal, onLocalRuntimeProgress, type LocalRuntimeStatus, type LocalHistory } from "../core/bridge-local";
import { boundedHistory, cloudHandoff, localRoute, LocalRouteCancelled } from "../local/routing";
import {voiceDesktopAction,voiceTaskError} from "../local/voice-actions";
import "../local/messages";
import "../local/style.css";
import {voiceProposalCard,setVoiceProposal} from "../local/voice-proposals";
import {bindGlobalVoiceChat} from "../local/global-voice";

const STOP_ICON = "M7 7h10v10H7z";
/** Four squares: the «/» menu (same glyph as the header tab, views/planner/ui.ts). */
const PLANNER_MENU_ICON = "M4.5 4.5h6v6h-6z M13.5 4.5h6v6h-6z M4.5 13.5h6v6h-6z M13.5 13.5h6v6h-6z";
const TOP_UP_URL = "https://roadeep.com";
/** Within this many pixels of the end, the log counts as "at the bottom". */
const STICK_PX = 24;
/** Past this length a streaming reply re-renders at most every LONG_GAP_MS. */
const LONG_TEXT = 8000;
const LONG_GAP_MS = 100;
/** While a turn restored after a reload runs, how often Rust is asked whether it ended. */
const RESTORE_POLL_MS = 1500;
/** MCP server status changes come in bursts (connect, list, ready): count once after them. */
const TOOLS_REFRESH_MS = 800;
/** The one-time "each tool step costs credit" hint has been shown (per user, this PC). */
const CREDIT_HINT_KEY = "roadeep.chatToolsCreditHint";

registerMessages(toolsEn, toolsFa);

/** A tool approval decided during a turn, kept as one line above its reply. */
interface ApprovalRecord {
  tool: string;
  decision: "approve" | "reject" | "decided";
}

const RECORD_KEYS = {
  approve: "chat.approvalRecordAllowed",
  reject: "chat.approvalRecordRejected",
  decided: "chat.approvalRecordDecided",
} as const;

/** "decided": settled elsewhere (already decided, or gone) — which way is unknown. */
type ApprovalState = "pending" | "deciding" | "approve" | "reject" | "decided";

interface LiveApproval extends ChatApproval {
  state: ApprovalState;
  /** What the user clicked, while it is being sent. */
  choice: "approve" | "reject" | null;
  /**
   * The whole summary has been on screen (it fits, was scrolled to its end, or
   * was expanded). Until then Allow stays disabled: a long command must not be
   * approved with its tail unseen.
   */
  seen: boolean;
  expanded: boolean;
  error: string | null;
  /** The card in the current view; dropped when the view is rebuilt. */
  el: HTMLElement | null;
}

interface LiveTurn {
  turn: number;
  text: string;
  status: ChatStreamStatus | null;
  label: string | null;
  approvals: LiveApproval[];
  /** MCP tool steps, in order (Rust sends each again on every change). */
  steps: ChatToolStep[];
  /** The one-time credit hint shows under this turn's steps. */
  hint: boolean;
  /** Re-attached after a reload: no chatSend promise will settle it. */
  restored: boolean;
}

/** What stays above a finished reply: its tool steps and, once, the credit hint. */
interface ToolRecord {
  steps: ChatToolStep[];
  hint: boolean;
}

/** What the current chat view does when the turn changes. */
interface ChatViewHooks {
  /** New stream content: repaint the live row (throttled). */
  streamChanged(): void;
  /** The turn ended (reply, error or stop): repaint the log. */
  turnEnded(): void;
  /** Balance or other state changed: sync on the next frame. */
  notify(): void;
  /** A tool approval arrived: say it at once, outside the (busy) log. */
  announceApproval(a: ChatApproval): void;
}

// ── State that outlives a view rebuild ────────────────────────────────────────

let nextId = 1;
let turnSeq = 0;
let sending = false;
let live: LiveTurn | null = null;
/** The thread the log shows, when known (a reply or an opened thread). */
let currentThreadId: string | null = null;
let currentThreadModel: string | null = null;
/** Explicit fabricated browser preview, never an application thread. */
export function seedChatModelPreviewThread(model: string) {
  if (IS_TAURI) return;
  currentThreadId = "example-historical-thread";
  currentThreadModel = model;
  State.chatHistory = [{id:nextId++,role:"assistant",content:getLanguage() === "fa"
    ? "بررسی کد به پایان رسید. نتیجه‌های این گفت‌وگو در تاریخچه می‌مانند."
    : "The code review is complete. This conversation remains available in history."}];
}
/**
 * A message with its way out at the end of the log: out of tokens (top up), or a
 * local agent whose instructions Roadeep took for a "save to memory" request (edit it).
 */
type ChatAlert = { kind: "credits" } | { kind: "agentMemory"; agent: string };
let chatAlert: ChatAlert | null = null;
let balance: RoadeepBalance | null = null;
/** Approvals decided during a turn, by the id of the reply they led to. */
const approvalRecords = new Map<number, ApprovalRecord[]>();
/** MCP tool steps, by the id of the reply they led to (live turns and reopened threads). */
const toolRecords = new Map<number, ToolRecord>();
/** Tool-step rows the user opened, so a repaint keeps them open. */
const openSteps = new Set<string>();
/** The tools chip: null until Rust has said (and outside the app). */
let toolsState: ChatToolsState | null = null;
let toolsSeq = 0;
let toolsTimer: number | null = null;
/** Asked once per sign-in even before the chat is focused (the session may arrive later). */
let toolsAsked = false;
/** The island height the chat's content needs (null: not measured yet). */
let wanted: number | null = null;
let view: ChatViewHooks | null = null;
let listening = false;
let restoreChecked = false;
let restorePoll: number | null = null;
let assistantCleanup: (() => void) | null = null;
let localStatus: LocalRuntimeStatus | undefined;
let localRequestId: string | undefined;
let localRequestIdentity:string|undefined;
const routingIdentity=()=>`${State.settings.chatAgent??""}|${State.settings.model}`;
let voiceAskTurn: number | undefined;
const cancelledLocalTurns=new Set<number>();
let unsyncedLocal: LocalHistory[]=[];
const localReady=()=>!!localStatus?.ready&&localStatus.enabled;

/** Read by the island's geometry: what the chat asks for while it is the view. */
export function chatWantedHeight(): number | null {
  return State.view === "prompt" ? wanted : null;
}

const errText = (err: unknown) => (isRoadeepError(err) ? `${err.code} ${err.requestId ?? ""}`.trim() : String(err));

/** Bidi overrides, embeddings and isolates in server text could reorder what the user approves. */
const BIDI_CONTROLS = /[‪-‮⁦-⁩]/g;
const RTL_LETTER = /[֐-ࣿיִ-﷿ﹰ-﻿]/;

function listenOnce() {
  if (listening) return;
  listening = true;
  void onChatStream(onStream);
  void onRoadeepBalance((b) => {
    balance = b;
    view?.notify();
  });
  // A server added, enabled, connected or failing changes what the chip shows.
  void onMcpcStatus(() => scheduleToolsRefresh());
}

/**
 * Asks Rust what the tools chip shows. With `count` (and tools on) the enabled
 * servers are connected (lazily, reused afterwards) to count their tools.
 * Status events never count: counting connects, a failing server reports a
 * new status on every try, and that would loop.
 */
function refreshTools(count = true) {
  if (State.roadeep.signedIn !== true) return;
  const seq = ++toolsSeq;
  Bridge.chatToolsState(State.settings.chatTools, count).then(
    (s) => {
      if (seq !== toolsSeq) return;
      // Not counted this time: the last count stays while tools stay on.
      const kept = !count && s.on && toolsState?.on ? toolsState.count : null;
      toolsState = { ...s, count: s.count ?? kept };
      view?.notify();
    },
    (err) => void Bridge.log(`chat: tools state unavailable ${errText(err)}`),
  );
}

function scheduleToolsRefresh() {
  if (toolsTimer != null) window.clearTimeout(toolsTimer);
  toolsTimer = window.setTimeout(() => {
    toolsTimer = null;
    refreshTools(false);
  }, TOOLS_REFRESH_MS);
}

/** Whether the one-time credit hint was already shown. Storage may be unavailable. */
function creditHintSeen(): boolean {
  try {
    return localStorage.getItem(CREDIT_HINT_KEY) === "1";
  } catch {
    return false;
  }
}

function markCreditHintSeen() {
  try {
    localStorage.setItem(CREDIT_HINT_KEY, "1");
  } catch (err) {
    void Bridge.log(`chat: could not keep the tools hint flag ${String(err)}`);
  }
}

/** Like roadeepErrorText, plus the chat's own MCP tool codes (strings in tools-*.ts). */
function chatErrorText(err: unknown): string {
  if (isRoadeepError(err) && err.code.startsWith("TOOL_")) return t(`rerr.${err.code}`);
  return roadeepErrorText(err);
}

function onStream(e: ChatStreamEvent) {
  if (!live || e.turn !== live.turn) return;
  switch (e.kind) {
    case "delta":
      if (typeof e.text === "string") live.text = e.text;
      break;
    case "status":
      live.status = e.status;
      live.label = e.label ?? null;
      break;
    case "approval": {
      if (typeof e.approval?.id !== "string") return;
      if (!live.approvals.some((a) => a.id === e.approval.id)) {
        live.approvals.push({
          ...e.approval, state: "pending", choice: null, seen: false, expanded: false, error: null, el: null,
        });
        Sound.play("approval");
        view?.announceApproval(e.approval);
        afterApprovals();
      }
      break;
    }
    case "approvalDone": {
      const a = live.approvals.find((x) => x.id === e.approvalId);
      if (a) {
        a.state = e.decision;
        a.error = null;
        afterApprovals();
      }
      break;
    }
    case "toolStep": {
      const step = e.step;
      if (typeof step?.id !== "string") return;
      const i = live.steps.findIndex((s) => s.id === step.id);
      if (i >= 0) live.steps[i] = step;
      else {
        live.steps.push(step);
        if (!live.hint && live.steps.length === 1 && !creditHintSeen()) {
          live.hint = true;
          markCreditHintSeen();
        }
      }
      break;
    }
    default:
      return;
  }
  view?.streamChanged();
}

/** The character asks while an approval waits, and thinks again once all are decided. */
function afterApprovals() {
  if (!live) return;
  const waiting = live.approvals.some((x) => x.state === "pending");
  State.stateOverride = waiting ? "approval" : "thinking";
  State.notify();
}

/** Ends the turn in flight and lets the current view repaint. */
function endTurn() {
  live = null;
  sending = false;
  if (restorePoll != null) {
    window.clearTimeout(restorePoll);
    restorePoll = null;
  }
  view?.turnEnded();
}

/**
 * Adds the turn's reply to the log (with no text: just its tool steps, when it
 * ended without an answer), with the approvals and tool steps that led to it.
 */
function keepTurn(text: string, origin:ChatMessage["origin"]="cloud", routeReason?:string): number {
  const id = nextId++;
  State.chatHistory.push({ id, role: "assistant", content: text, origin, routeReason });
  if (!live) return id;
  // A local (MCP tool) approval is already told by its step row.
  const decided = live.approvals.flatMap((a): ApprovalRecord[] =>
    !a.id.startsWith(LOCAL_APPROVAL_PREFIX) && (a.state === "approve" || a.state === "reject" || a.state === "decided")
      ? [{ tool: a.tool, decision: a.state }] : []);
  if (decided.length) approvalRecords.set(id, decided);
  if (live.steps.length) toolRecords.set(id, { steps: live.steps.slice(), hint: live.hint });
  return id;
}

/**
 * After a reload (or a CHAT_BUSY) the page has no pending chatSend, but Rust may
 * still be writing a reply: re-attach to its turn so the stream shows, the
 * input stays locked and Stop works. Nothing settles such a turn, so Rust is
 * asked now and then whether it ended; the streamed text then stays as the reply.
 */
async function restoreTurn() {
  if (sending) return;
  let state;
  try {
    state = await Bridge.chatTurnState();
  } catch (err) {
    void Bridge.log(`chat: turn state unavailable ${errText(err)}`);
    return;
  }
  if (!state.busy || sending) return;
  void Bridge.log("chat: re-attached to the turn in flight");
  const turn = state.turn ?? -1;
  turnSeq = Math.max(turnSeq, turn);
  sending = true;
  live = { turn, text: "", status: null, label: null, approvals: [], steps: [], hint: false, restored: true };
  State.stateOverride = "thinking";
  State.notify();
  const poll = async () => {
    restorePoll = null;
    if (!live?.restored) return;
    try {
      const now = await Bridge.chatTurnState();
      if (now.busy) {
        restorePoll = window.setTimeout(() => void poll(), RESTORE_POLL_MS);
        return;
      }
    } catch (err) {
      void Bridge.log(`chat: turn state unavailable ${errText(err)}`);
    }
    if (!live?.restored) return;
    if (live.text.trim() || live.steps.length) keepTurn(live.text);
    State.stateOverride = null;
    endTurn();
  };
  restorePoll = window.setTimeout(() => void poll(), RESTORE_POLL_MS);
}

// ── Pieces ────────────────────────────────────────────────────────────────────

function bubble(message: ChatMessage, records: ApprovalRecord[] = [], tools?: ToolRecord): HTMLElement {
  if (message.role === "user") {
    return h(
      "div",
      { class: "chat-row user" },
      h("div", { class: "bubble", dir: "auto", text: message.content }),
    );
  }
  // A turn that ended without an answer keeps only its steps.
  const origin=message.origin??"cloud";
  const provenance=origin?h("span",{class:"local-provenance",text:t(origin==="app"?"local.app":origin==="cloud"?"local.cloud":origin==="cpu"?"local.cpu":"local.offline"),title:message.routeReason}):null;
  const reply = message.content.trim() ? h("div", { class: "reply md" }, renderMarkdown(message.content)) : null;
  const steps = tools?.steps.length ? toolSteps(tools.steps) : null;
  const hint = tools?.hint ? creditHint() : null;
  if (!records.length && !steps) return h("div", { class: "chat-row stacked" }, provenance, reply);
  const notes = records.map((r) =>
    h("div", { class: `appr-record ${r.decision}` },
      h("span", { class: "appr-record-ico" }, svg(r.decision === "reject" ? ICONS.xmark : ICONS.check, 8, { stroke: r.decision === "reject" ? 0 : 3 })),
      h("span", { text: t(RECORD_KEYS[r.decision]) }),
      h("span", { class: "appr-record-tool", dir: "auto", text: r.tool.replace(BIDI_CONTROLS, "") }),
    ));
  return h("div", { class: "chat-row stacked" }, ...notes, steps, hint, provenance, reply);
}

// ── MCP tool steps ────────────────────────────────────────────────────────────

const STEP_ICONS: Record<ChatToolStep["state"], { path: string; stroke?: number } | null> = {
  waiting: { path: ICONS.bang },
  running: null, // a spinner
  done: { path: ICONS.check, stroke: 3 },
  error: { path: ICONS.xmark },
  declined: { path: ICONS.xmark },
  stopped: { path: STOP_ICON },
};

/** "GitHub · create_issue", bidi controls dropped (server text). */
function stepName(step: ChatToolStep): string {
  return [step.server, step.tool].map((s) => s.replace(BIDI_CONTROLS, "").trim()).filter(Boolean).join(" · ");
}

/** The step's details: what went in, what came back (text only, never HTML). */
function stepBody(step: ChatToolStep): HTMLElement {
  const body = h("div", { class: "tool-step-body" });
  const args = step.arguments.trim();
  body.append(h("div", { class: "tool-step-label", text: t("tools.arguments") }));
  body.append(args && args !== "{}"
    ? h("pre", { class: "tool-step-pre", dir: "ltr", tabindex: "0", text: args })
    : h("div", { class: "tool-step-none", text: t("tools.noArguments") }));
  if (step.result) {
    const text = step.result.replace(BIDI_CONTROLS, "");
    body.append(
      h("div", { class: "tool-step-label", text: t("tools.result") }),
      h("pre", { class: "tool-step-pre", dir: RTL_LETTER.test(text) ? "auto" : "ltr", tabindex: "0", text }),
    );
  }
  if (step.error) {
    body.append(
      h("div", { class: "tool-step-label", text: t("tools.error") }),
      h("div", { class: "tool-step-err", dir: "auto", text: localizeError(step.error) }),
    );
  }
  return body;
}

/**
 * One tool step: icon, "server · tool", its state; a click opens what went in
 * and what came back. Opened rows stay open across repaints (`openSteps`).
 */
export function toolStepRow(step: ChatToolStep): HTMLElement {
  const name = stepName(step);
  const icon = STEP_ICONS[step.state];
  const ico = h("span", { class: "tool-step-ico", "aria-hidden": "true" },
    icon ? svg(icon.path, 8, icon.stroke ? { stroke: icon.stroke } : undefined) : h("i", { class: "tool-step-spin" }));
  const bodyId = `tool-step-${step.id.replace(/[^\w-]/g, "")}`;
  const open = openSteps.has(step.id);
  const head = h("button", {
    type: "button", class: "tool-step-head", "aria-expanded": String(open), "aria-controls": bodyId,
    "aria-label": `${name} — ${t(`tools.state.${step.state}`)}`, title: t("tools.details", { name }),
  },
    ico,
    h("span", { class: "tool-step-name", dir: "auto", text: name }),
    h("span", { class: "tool-step-state", text: t(`tools.state.${step.state}`) }),
    h("span", { class: "tool-step-chev", "aria-hidden": "true" }, svg(ICONS.chevronRight, 8, { stroke: 2.2 })),
  );
  const body = stepBody(step);
  body.id = bodyId;
  body.hidden = !open;
  const row = h("div", { class: `tool-step ${step.state}${open ? " open" : ""}` }, head, body);
  head.addEventListener("click", () => {
    const now = !openSteps.has(step.id);
    if (now) openSteps.add(step.id);
    else openSteps.delete(step.id);
    row.classList.toggle("open", now);
    head.setAttribute("aria-expanded", String(now));
    body.hidden = !now;
  });
  return row;
}

function toolSteps(steps: ChatToolStep[]): HTMLElement {
  return h("div", { class: "tool-steps" }, ...steps.map(toolStepRow));
}

/** Said once, the first time a turn uses a tool: every step is a paid message. */
function creditHint(): HTMLElement {
  return h("div", { class: "tools-hint", role: "note", dir: "auto", text: t("tools.creditHint") });
}

function alertCard(alert: ChatAlert): HTMLElement {
  if (alert.kind === "credits") return createCreditAlert({ onTopUp: () => void Bridge.openUrl(TOP_UP_URL) });
  return createIslAlert({
    text: alert.agent
      ? t("rerr.AGENT_MEMORY_TRIGGER.agent", { name: isolate(alert.agent) })
      : t("rerr.AGENT_MEMORY_TRIGGER"),
    action: t("chat.editAgent"),
    // The agents list lives in the integrations section.
    onAction: () => void Bridge.openSettingsWindow("agents"),
  });
}

function typingDots(): HTMLElement {
  return h("div", { class: "typing" }, h("i"), h("i"), h("i"));
}

/** The coloured chip showing what the question is about (a dropped file). */
function contextChip(label: string): HTMLElement {
  const chip = h("div", { class: "chip" }, h("i", { class: "chip-dot" }), h("span", { text: label }));
  requestAnimationFrame(() => chip.classList.add("settled"));
  return chip;
}

/**
 * The approval's summary, readable in full: wrapped, up to seven lines, then a
 * scroll with a visible cue (fade + "N more lines" + "Show all").
 * Bidi controls are dropped; text without a right-to-left letter (a command, a
 * URL, JSON) is shown left-to-right, anything else by its first strong letter.
 */
function approvalSummary(raw: string): HTMLElement | null {
  const text = raw.replace(BIDI_CONTROLS, "").trim();
  if (!text) return null;
  const dir = RTL_LETTER.test(text) ? textDirection(text) ?? "ltr" : "ltr";
  return h("div", { class: "appr-sum-wrap" },
    h("div", {
      class: "appr-sum", dir, tabindex: "0", role: "group", "aria-label": t("chat.approvalSummaryLabel"), text,
    }),
    h("div", { class: "appr-sum-foot" },
      h("span", { class: "appr-more" }),
      h("button", { type: "button", class: "appr-expand", text: t("chat.approvalShowAll") }),
    ),
  );
}

/**
 * A local MCP tool call exactly as it will run: the server and the tool by
 * name, and the arguments as the JSON that will be sent, in full and unmasked
 * (the model wrote every value). LTR, text only; the box scrolls like the
 * summary's and Allow waits until its end has been seen (checkSummary).
 */
export function localCallDetails(call: NonNullable<ChatApproval["call"]>): HTMLElement {
  const row = (label: string, value: string, dir: "ltr" | "auto") =>
    h("div", { class: "appr-call-row" },
      h("span", { class: "appr-call-k", text: label }),
      h("span", { class: "appr-call-v", dir, text: value.replace(BIDI_CONTROLS, "") }),
    );
  return h("div", { class: "appr-call" },
    row(t("tools.approvalServer"), call.server, "auto"),
    row(t("tools.approvalTool"), call.tool, "ltr"),
    h("div", { class: "appr-sum-wrap" },
      h("div", {
        class: "appr-sum appr-args", dir: "ltr", tabindex: "0", role: "group",
        "aria-label": t("tools.approvalArguments"), text: call.arguments,
      }),
      h("div", { class: "appr-sum-foot" },
        h("span", { class: "appr-more" }),
        h("button", { type: "button", class: "appr-expand", text: t("chat.approvalShowAll") }),
      ),
    ),
  );
}

/**
 * Whether the summary still hides lines; marks it seen once its end has been
 * on screen, and repaints the card (Allow, the cue, the hint).
 */
function checkSummary(a: LiveApproval) {
  const card = a.el;
  const sum = card?.querySelector<HTMLElement>(".appr-sum");
  if (!card || !sum) {
    a.seen = true;
    if (card) paintApproval(a);
    return;
  }
  sum.classList.toggle("expanded", a.expanded);
  const rest = sum.scrollHeight - sum.clientHeight - sum.scrollTop;
  const hidden = !a.expanded && rest > 2;
  if (!hidden) a.seen = true;
  const wrap = sum.parentElement as HTMLElement;
  wrap.classList.toggle("overflow", hidden);
  const lineH = parseFloat(getComputedStyle(sum).lineHeight) || 18;
  (wrap.querySelector(".appr-more") as HTMLElement).textContent =
    hidden ? t("chat.approvalMoreLines", { n: Math.max(1, Math.ceil(rest / lineH)) }) : "";
  paintApproval(a);
}

let hintSeq = 0;

function approvalCard(a: LiveApproval): HTMLElement {
  const hintId = `appr-hint-${++hintSeq}`;
  const deny = h("button", { type: "button", class: "btn secondary", text: t("approval.deny") });
  // Gated with aria-disabled (not `disabled`) so it stays in the tab order and
  // says why; the click guard in decide() is what keeps it from firing.
  const allow = h("button", {
    type: "button", class: "btn primary", text: t("approval.allow"), "aria-describedby": hintId,
  });
  const outcome = h("span", { class: "appr-outcome", role: "status" });
  const error = h("div", { class: "appr-error", dir: "auto", role: "alert" });
  const card = h("div", { class: "appr-card" },
    h("div", { class: "who-row" },
      h("span", { class: "appr-ico" }, svg(ICONS.bang, 9)),
      h("span", { class: "n", dir: "auto", text: a.tool.replace(BIDI_CONTROLS, "") }),
      h("span", { text: t("approval.needsPermission") }),
    ),
    a.call ? localCallDetails(a.call) : approvalSummary(a.summary),
    error,
    // Hint and buttons stay pinned to the bottom of the log while a long card
    // scrolls past (position: sticky), so Allow/Deny are never below the fold.
    h("div", { class: "appr-foot" },
      h("div", { class: "appr-hint", id: hintId, dir: "auto", text: t("chat.approvalSeeAll") }),
      h("div", { class: "actions" }, deny, allow, outcome),
    ),
  );
  const sum = card.querySelector<HTMLElement>(".appr-sum");
  if (sum) {
    sum.addEventListener("scroll", () => checkSummary(a), { passive: true });
    (card.querySelector(".appr-expand") as HTMLElement).addEventListener("click", () => {
      a.expanded = true;
      checkSummary(a);
      view?.streamChanged();
      // The expand button goes away; reading continues in the box itself.
      sum.focus();
    });
    // The box grows with the island; re-check whenever its size changes.
    new ResizeObserver(() => checkSummary(a)).observe(sum);
  }
  // Never auto-approved: only these two clicks decide.
  const decide = (decision: "approve" | "reject") => {
    if (a.state !== "pending") return;
    if (decision === "approve" && !a.seen) return;
    a.state = "deciding";
    a.choice = decision;
    a.error = null;
    paintApproval(a);
    Sound.play(decision === "approve" ? "approve" : "blip");
    Bridge.chatApprovalDecide(a.id, decision).then(
      () => {
        a.state = decision;
        paintApproval(a);
        afterApprovals();
        view?.streamChanged();
      },
      (err) => {
        void Bridge.log(`chat: approval decision failed ${errText(err)}`);
        if (isRoadeepError(err) && err.code === "APPROVAL_ALREADY_DECIDED") {
          // Decided elsewhere; approvalDone tells which way, if it comes.
          a.state = "decided";
        } else if (isRoadeepError(err) && err.code === "APPROVAL_NOT_FOUND") {
          a.state = "decided";
          a.error = roadeepErrorText(err);
        } else {
          a.state = "pending";
          a.error = roadeepErrorText(err);
        }
        paintApproval(a);
        afterApprovals();
        view?.streamChanged();
      },
    );
  };
  deny.addEventListener("click", () => decide("reject"));
  allow.addEventListener("click", () => decide("approve"));
  a.el = card;
  return card;
}

function paintApproval(a: LiveApproval) {
  const card = a.el;
  if (!card) return;
  const [deny, allow] = [...card.querySelectorAll<HTMLButtonElement>(".actions .btn")];
  const outcome = card.querySelector(".appr-outcome") as HTMLElement;
  const error = card.querySelector(".appr-error") as HTMLElement;
  const done = a.state === "approve" || a.state === "reject" || a.state === "decided";
  const deciding = a.state === "deciding";
  const pending = a.state === "pending";
  deny.disabled = !pending;
  allow.disabled = !pending;
  if (pending && !a.seen) allow.setAttribute("aria-disabled", "true");
  else allow.removeAttribute("aria-disabled");
  (card.querySelector(".appr-hint") as HTMLElement).hidden = !pending || a.seen;
  // While the click is sent, the chosen button shows as in flight and the other
  // keeps its slot (invisible), so nothing moves.
  deny.hidden = allow.hidden = done;
  deny.style.visibility = deciding && a.choice !== "reject" ? "hidden" : "";
  allow.style.visibility = deciding && a.choice !== "approve" ? "hidden" : "";
  deny.classList.toggle("chosen", deciding && a.choice === "reject");
  allow.classList.toggle("chosen", deciding && a.choice === "approve");
  card.classList.toggle("done", done);
  outcome.textContent =
    deciding ? t(a.choice === "reject" ? "chat.approvalRejecting" : "chat.approvalAllowing")
      : a.state === "approve" ? t("chat.approvalAllowed")
        : a.state === "reject" ? t("chat.approvalRejected")
          : a.state === "decided" ? t("chat.approvalAlreadyDecided") : "";
  error.textContent = a.error ?? "";
  error.hidden = !a.error;
}

/**
 * Where a streaming reply can be cut so the part before never changes again:
 * after its last blank line outside a code fence. Only the tail is re-rendered
 * while text arrives.
 */
function stableSplit(text: string): number {
  let split = 0;
  let inFence = false;
  let lineStart = 0;
  while (lineStart <= text.length) {
    let end = text.indexOf("\n", lineStart);
    if (end < 0) end = text.length;
    const line = text.slice(lineStart, end);
    if (/^ {0,3}(```|~~~)/.test(line)) inFence = !inFence;
    else if (!inFence && line.trim() === "" && end < text.length) split = end + 1;
    lineStart = end + 1;
  }
  return split;
}

/** Fills the agent picker: none, Roadeep's (exclusive, then public), then the local ones. */
function fillAgentOptions(select: HTMLSelectElement) {
  clear(select);
  const r = State.roadeep;
  select.append(h("option", { value: "", text: t("chat.agentNone") }));
  const exclusive = r.agents.filter((a) => r.exclusiveIds.includes(a.id));
  const open = r.agents.filter((a) => !r.exclusiveIds.includes(a.id));
  for (const [label, list] of [[t("chat.groupExclusive"), exclusive], [t("chat.groupPublic"), open]] as const) {
    if (!list.length) continue;
    const group = h("optgroup", { label });
    for (const a of list) group.append(h("option", { value: a.id, text: a.title }));
    select.append(group);
  }
  if (r.localAgents.length) {
    const group = h("optgroup", { label: t("chat.groupLocal") });
    for (const a of r.localAgents) group.append(h("option", { value: localAgentRef(a.id), text: a.name }));
    select.append(group);
  }
  // Lists not read yet: keep the saved choice visible rather than showing "none".
  const chosen = State.settings.chatAgent;
  if (chosen && ![...select.options].some((o) => o.value === chosen)) {
    select.append(h("option", { value: chosen, text: "…" }));
  }
}

/** Starter prompts of the chosen agent, local or Roadeep. */
function starterPrompts(): string[] {
  const chosen = State.settings.chatAgent;
  if (!chosen) return [];
  const r = State.roadeep;
  if (chosen.startsWith(LOCAL_AGENT_PREFIX)) {
    const id = chosen.slice(LOCAL_AGENT_PREFIX.length);
    return r.localAgents.find((a) => a.id === id)?.starterPrompts ?? [];
  }
  return r.agents.find((a) => a.id === chosen)?.starterPrompts ?? [];
}

/** The chosen local agent's name ("" for none or a Roadeep agent). */
function chosenAgentName(): string {
  const chosen = State.settings.chatAgent;
  if (!chosen?.startsWith(LOCAL_AGENT_PREFIX)) return "";
  const id = chosen.slice(LOCAL_AGENT_PREFIX.length);
  return State.roadeep.localAgents.find((a) => a.id === id)?.name ?? "";
}

/** The pill colour of the chosen agent, for its starter chips. */
function chosenAgentColor(): string | null {
  return State.tasks.find((task) => task.agentRef && task.agentRef === State.settings.chatAgent)?.color ?? null;
}

// ── The view ──────────────────────────────────────────────────────────────────

/**
 * `openMenu` opens the «/» menu (views/palette.ts), optionally with a filter;
 * typing «/» into the empty field, or the menu button, leads there.
 */
export function buildPrompt(onHeightChange: () => void, openMenu?: (filter: string) => void): ViewHost {
  assistantCleanup?.();
  // A rebuild (language switch) starts measured afresh with the history closed;
  // a turn in flight is re-attached below.
  wanted = null;
  if (live) for (const a of live.approvals) a.el = null;
  listenOnce();

  const chipRow = h("div", { class: "chip-row" });
  const modelPicker = createChatModelPicker({
    snapshot: () => {
      const agent = State.roadeep.localAgents.find((agent) => localAgentRef(agent.id) === State.settings.chatAgent);
      return { signedIn: State.roadeep.signedIn === true, busy: sending,
        selected: State.settings.model, pinnedModel: agent?.model || null,
        currentModel: currentThreadModel, hasThread: !!currentThreadId };
    },
    load: () => Bridge.roadeepModels(),
    async change(model) {
      const saved = await Bridge.chatModelSet(model);
      State.settings.model = saved;
      State.chatHistory = [];
      currentThreadId = null;
      currentThreadModel = null;
      chatAlert = null;
      renderedCount = -1;
      refreshTools();
      State.notify();
      onHeightChange();
    },
    log: (message) => void Bridge.log(message),
    busyChanged: () => State.notify(),
  });
  const balanceChip = createBalanceChip();
  const historyBtn = h("button", {
    type: "button", class: "top-btn", title: t("history.open"), "aria-label": t("history.open"),
  }, svg(ISLAND_CLOCK_ICON, 13, { stroke: 1.8 }));
  const top = h("div", { class: "chat-top" }, modelPicker.el, chipRow, h("div", { class: "grow" }), balanceChip.el, historyBtn);

  const starters = h("div", { class: "starters", role: "group", "aria-label": t("chat.starters") });
  const log = h("div", { class: "chat-log", role: "log", "aria-live": "polite" });
  // One line, like a text field; in the maximised island it grows to a few
  // lines (Shift+Enter starts a new one).
  const input = h("textarea", {
    class: "chat-input",
    rows: "1",
    "aria-label": t("chat.inputLabel"),
    placeholder: t("chat.placeholder"),
    spellcheck: "false",
  }) as HTMLTextAreaElement;
  /** Height follows the text, up to the CSS max-height. */
  const fitInput = () => {
    input.style.height = "auto";
    input.style.height = `${input.scrollHeight}px`;
  };
  let fittedFor: boolean | null = null;
  // The island grows and shrinks around it (maximise, restore): a field fitted
  // at an in-between width would keep a wrong height.
  let fittedWidth = 0;
  new ResizeObserver(() => {
    if (input.clientWidth === fittedWidth) return;
    fittedWidth = input.clientWidth;
    fitInput();
  }).observe(input);
  const send = h("button", { class: "send-btn", title: t("chat.send"), "aria-label": t("chat.send") });
  const agentPick = h("select", {
    class: "agent-pick",
    title: t("chat.agentLabel"),
    "aria-label": t("chat.agentLabel"),
  }) as HTMLSelectElement;

  // `dir="auto"` on an empty field falls back to LTR, which puts a Persian
  // placeholder and caret on the wrong side; follow the typed text instead and
  // the UI language while there is none.
  const syncInputDir = () => {
    input.dir = textDirection(input.value) ?? (isRtl() ? "rtl" : "ltr");
  };
  syncInputDir();
  // The field before the edit under way: `input` only sees the result, and a
  // «/» must open the menu only when nothing was there (opensMenuFromChat).
  let valueBefore = "";
  input.addEventListener("beforeinput", () => {
    valueBefore = input.value;
  });
  input.addEventListener("input", () => {
    if (openMenu && opensMenuFromChat(valueBefore, input.value)) {
      input.value = "";
      syncInputDir();
      setSendIcon();
      fitInput();
      openMenu("");
      return;
    }
    syncInputDir();
    setSendIcon();
    fitInput();
  });
  // MCP tools for this conversation: on/off, with how many the model gets.
  const toolsLabel = h("span", { class: "tools-chip-label" });
  const toolsChip = h("button", { type: "button", class: "tools-chip", hidden: true },
    h("i", { class: "tools-chip-dot", "aria-hidden": "true" }), toolsLabel);
  const menuBtn = openMenu
    ? h("button", {
        type: "button", class: "chat-menu-btn", title: t("chat.menu"), "aria-label": t("chat.menu"),
        onclick: () => openMenu(""),
      }, svg(PLANNER_MENU_ICON, 13, { stroke: 1.8 }))
    : null;
  const bar = h("div", { class: "chat-bar" }, menuBtn, agentPick, toolsChip, input, send);

  let toolsKey = "";
  function syncToolsChip() {
    const s = toolsState;
    const shown = !!s?.available && State.roadeep.signedIn === true;
    const key = `${getLanguage()}|${shown}|${s?.on}|${s?.count}|${sending}`;
    if (key === toolsKey) return;
    toolsKey = key;
    toolsChip.hidden = !shown;
    toolsChip.disabled = sending;
    if (!s || !shown) return;
    const counted = s.on && s.count != null;
    toolsLabel.textContent = counted ? t("tools.chipCount", { n: s.count! }) : t("tools.chip");
    const label = !s.on ? t("tools.chipOff") : counted ? t("tools.chipOnCount", { n: s.count! }) : t("tools.chipOn");
    toolsChip.title = label;
    toolsChip.setAttribute("aria-label", label);
    toolsChip.setAttribute("aria-pressed", String(s.on));
    toolsChip.classList.toggle("on", s.on);
  }

  toolsChip.addEventListener("click", () => {
    const s = toolsState;
    if (!s || sending || modelPicker.busy) return;
    const on = !s.on;
    // Shown at once; Rust has the last word (and the count) right after.
    toolsState = { ...s, on, count: on ? s.count : null };
    Sound.play("blip");
    announcer.textContent = t(on ? "tools.turnedOn" : "tools.turnedOff");
    State.notify();
    Bridge.chatToolsSet(on).then(
      () => refreshTools(),
      (err) => {
        void Bridge.log(`chat: tools toggle failed ${errText(err)}`);
        refreshTools();
      },
    );
  });
  toolsChip.addEventListener("keydown", (e) => e.stopPropagation());

  const signInTitle = h("div", { class: "title", text: t("chat.signInTitle") });
  const signInSub = h("div", { class: "sub" });
  const signIn = h(
    "div",
    { class: "chat-signin" },
    h("div", { style: "display:flex;flex-direction:column;gap:5px;min-width:0" }, signInTitle, signInSub),
    h("div", { class: "grow" }),
    h("button", {
      class: "btn primary",
      text: t("chat.openSettings"),
      onclick: () => void Bridge.openSettingsWindow("account"),
    }),
  );

  let renderedCount = -1;
  let renderedSend: string | null = null;
  let agentKey = "";
  let startersKey = "";
  let liveFrame: number | null = null;
  let liveTimer: number | null = null;
  let lastLiveRender = 0;
  let historyOpen = false;

  const history = buildHistory({
    list: (offset, limit) => Bridge.chatThreads(offset, limit),
    async open(id) {
      if (sending || modelPicker.busy) return;
      const thread = await Bridge.chatThreadOpen(id);
      State.chatHistory = thread.messages
        .filter((m) => m.role === "user" || m.role === "assistant")
        .map((m) => {
          const message: ChatMessage = { id: nextId++, role: m.role, content: m.text };
          // Rust folded the thread's tool messages into these steps.
          if (m.toolSteps?.length) toolRecords.set(message.id, { steps: m.toolSteps, hint: false });
          return message;
        });
      currentThreadId = thread.id;
      currentThreadModel = thread.model ?? null;
      chatAlert = null;
      renderedCount = -1;
      // Opening a thread starts it with tools as the setting says.
      refreshTools();
      closeHistory();
    },
    async remove(id) {
      if (sending || modelPicker.busy) return;
      await Bridge.chatThreadDelete(id);
      // Rust resets the chat when it was the current thread; the log follows.
      if (id === currentThreadId) {
        currentThreadId = null;
        currentThreadModel = null;
        State.chatHistory = [];
        refreshTools();
        State.notify();
        onHeightChange();
      }
    },
    newChat() {
      if (sending || modelPicker.busy) return;
      State.chatHistory = [];
      currentThreadId = null;
      currentThreadModel = null;
      chatAlert = null;
      void Bridge.chatReset().then(() => refreshTools());
      closeHistory();
    },
    close: () => closeHistory(),
    currentId: () => currentThreadId,
  });

  // Approvals are said here, outside the log, which stays aria-busy while a
  // reply streams in.
  const announcer = h("div", { class: "sr-only", "aria-live": "assertive", "aria-atomic": "true" });
  const assistant = createAssistantControls({
    hideChips:true,
    snapshot: () => ({ visible: State.view === "prompt" && State.mode === "expanded" && !historyOpen,
      signedIn: State.roadeep.signedIn === true, busy: sending || modelPicker.busy,
      agentId: State.settings.chatAgent ?? "default",
      identity: `${State.settings.chatAgent ?? "default"}|${State.settings.model}` }),
    setup: () => void Bridge.openSettingsWindow("assistant"),
    changed: () => scheduleMeasure(),
  });
  const unsubscribeAssistant = State.subscribe(() => {
    assistant.sync();
    if(localRequestId&&localRequestIdentity!==routingIdentity()){
      if(live)cancelledLocalTurns.add(live.turn);
      void BridgeLocal.cancel(localRequestId).catch(()=>void Bridge.log("local chat identity cancellation failed"));
    }
  });
  bindGlobalVoiceChat(query=>submit(query,true,true),()=>{
    if(voiceAskTurn===live?.turn&&sending){
      if(live)cancelledLocalTurns.add(live.turn);
      if(localRequestId)void BridgeLocal.cancel(localRequestId).catch(()=>void Bridge.log("Global voice cancellation failed"));
      return Bridge.chatCancel();
    }
  });
  let unlistenLocal:(()=>void)|undefined;
  let disposedLocal=false;
  const receiveLocal=(next:LocalRuntimeStatus)=>{if(disposedLocal)return;localStatus=next;State.notify();};
  if(IS_TAURI){void BridgeLocal.status().then(receiveLocal).catch(()=>void Bridge.log("local runtime status unavailable"));void onLocalRuntimeProgress(receiveLocal).then(stop=>{if(disposedLocal)stop();else unlistenLocal=stop;});}
  assistantCleanup = () => { modelPicker.dispose();disposedLocal=true;proposalCard.dispose();unlistenLocal?.();unsubscribeAssistant(); assistant.dispose(); };
  const localNotice=h("div",{class:"chat-local-notice"},h("span",{text:t("local.signIn")}),h("button",{type:"button",class:"assistant-action",text:t("chat.openSettings"),onclick:()=>void Bridge.openSettingsWindow("account")}));
  const proposalCard=voiceProposalCard();
  const body = h("div", { class: "chat-body" }, top, modelPicker.hint, log, starters, localNotice,proposalCard.el, assistant.el, bar, history.el, signIn, announcer);
  const el = h("div", { class: "view" }, h("div", { class: "card wash chat-card" }, body));
  (el.querySelector(".card") as HTMLElement).style.setProperty("--wash", "rgba(99,102,241,0.5)");

  /**
   * The chat grows with its content (up to CHAT_MAX_H) and the history list
   * takes the full height. Everything but the log is fixed, so the height it
   * needs is the island's height minus the log's box plus the log's content.
   * While a reply streams in it only grows, so a re-render never makes it jump.
   */
  let measureFrame: number | null = null;
  /** Measures on the next frame (several changes in one frame measure once). */
  /** The agent chip as wide as its chosen label (a <select> sizes to its longest option). */
  let pickKey = "";
  function sizeAgentPick() {
    const label = agentPick.selectedOptions[0]?.text ?? "";
    const style = getComputedStyle(agentPick);
    const key = `${label}|${style.font}`;
    if (key === pickKey || !style.font) return;
    pickKey = key;
    const ctx = document.createElement("canvas").getContext("2d");
    if (!ctx) return;
    ctx.font = style.font;
    const pad = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    agentPick.style.width = `${Math.ceil(ctx.measureText(label).width + pad + 2)}px`;
  }
  void document.fonts?.ready.then(() => {
    pickKey = "";
    sizeAgentPick();
  });

  function scheduleMeasure() {
    if (measureFrame != null) return;
    measureFrame = requestAnimationFrame(() => {
      measureFrame = null;
      measure();
    });
  }

  /** An approval card to bring to the top of the log once the island has settled. */
  let pendingReveal: HTMLElement | null = null;

  /**
   * Scrolls the card's top to the top of the log. If the log can't scroll that
   * far (the card is near the end), a spacer under the live row makes room, so
   * the row before the card is either fully visible or fully out of view.
   */
  function revealCard(card: HTMLElement) {
    if (!card.isConnected) return;
    const target = log.scrollTop + card.getBoundingClientRect().top - log.getBoundingClientRect().top - 4;
    const max = log.scrollHeight - log.clientHeight;
    if (max > 0 && target > max) liveRow.style.paddingBottom = `${Math.ceil(target - max)}px`;
    log.scrollTop = target;
  }

  /** Pixels the content estimate fell short by, learnt once the island settled. */
  let overflowPad = 0;

  function measure() {
    let next: number;
    if (historyOpen) {
      next = CHAT_MAX_H;
    } else {
      const island = el.closest<HTMLElement>("#island");
      if (!island || body.classList.contains("signed-out")) return;
      // scrollHeight never reads less than the box, so the content is measured
      // from its first to its last row, plus the log's padding (4 px each side)
      // and 2 px so rounding never leaves a scrollbar on content that fits.
      const first = log.firstElementChild;
      const last = log.lastElementChild;
      const content = first && last
        ? last.getBoundingClientRect().bottom - first.getBoundingClientRect().top + 10
        : 0;
      next = Math.min(CHAT_MAX_H, Math.ceil(island.offsetHeight - log.clientHeight + content + overflowPad));
      if (!assistant.el.querySelector<HTMLElement>(".assistant-panel")?.hidden) {
        const children=[...body.children].filter((child):child is HTMLElement => child instanceof HTMLElement && getComputedStyle(child).display!=="none");
        const fixed=children.filter(child=>child!==log).reduce((sum,child)=>{
          const css=getComputedStyle(child);
          return sum+child.getBoundingClientRect().height+(parseFloat(css.marginTop)||0)+(parseFloat(css.marginBottom)||0);
        },0);
        const css=getComputedStyle(body);
        next=Math.min(CHAT_MAX_H,Math.ceil(island.offsetHeight-body.clientHeight+fixed+Math.max(24,content)+(parseFloat(css.paddingTop)||0)+(parseFloat(css.paddingBottom)||0)+(parseFloat(css.gap)||0)*Math.max(0,children.length-1)));
      } else if(log.clientHeight===0)return;
      if (live && wanted != null) next = Math.max(next, wanted);
    }
    // Settled (the island reached the height asked for) but the log still
    // scrolls by a few pixels: ask for them, so nothing scrolls below the max.
    const island = el.closest<HTMLElement>("#island");
    // The extra is kept (not re-derived) so the height can't swing back and forth.
    if (!historyOpen && wanted != null && island && Math.abs(island.offsetHeight - wanted) < 1) {
      const over = log.scrollHeight - log.clientHeight;
      if (over > 0 && wanted < CHAT_MAX_H) {
        overflowPad += over;
        next = Math.max(next, Math.min(CHAT_MAX_H, wanted + over));
      }
    }
    if (next === wanted) {
      // Settled at the asked height: the deferred card reveal can run now.
      if (pendingReveal && island && Math.abs(island.offsetHeight - (wanted ?? 0)) < 1) {
        const card = pendingReveal;
        pendingReveal = null;
        revealCard(card);
      }
      return;
    }
    // A small dead band only when shrinking, so a re-render never makes it jump.
    if (wanted != null && next < wanted && wanted - next < 4) return;
    wanted = next;
    onHeightChange();
  }

  function openHistory() {
    if (sending || historyOpen) return;
    historyOpen = true;
    body.classList.add("history-open");
    history.el.hidden = false;
    historyBtn.classList.add("on");
    historyBtn.setAttribute("aria-expanded", "true");
    measure();
    Sound.play("blip");
    history.show();
  }

  function closeHistory() {
    if (!historyOpen) return;
    historyOpen = false;
    body.classList.remove("history-open");
    history.el.hidden = true;
    historyBtn.classList.remove("on");
    historyBtn.setAttribute("aria-expanded", "false");
    // Measured again once the log is back on screen.
    wanted = null;
    renderedCount = -1;
    State.notify();
    onHeightChange();
    input.focus();
  }
  historyBtn.addEventListener("click", () => (historyOpen ? closeHistory() : openHistory()));

  /**
   * Send, Stop, or their quiet forms, so one control is bright at a time: Stop
   * steps back while an approval waits (Allow leads), Send while there is
   * nothing to send or the out-of-credit alert offers its top-up.
   */
  function setSendIcon() {
    const waiting = !!live?.approvals.some((a) => a.state === "pending");
    const idle = !input.value.trim() || chatAlert?.kind === "credits";
    const mode = sending ? (waiting ? "stop-quiet" : "stop") : idle ? "send-quiet" : "send";
    if (renderedSend === mode) return;
    renderedSend = mode;
    clear(send);
    const stop = mode.startsWith("stop");
    send.append(stop ? svg(STOP_ICON, 10) : svg(ICONS.arrowUp, 11));
    const label = t(stop ? "chat.stop" : "chat.send");
    send.title = label;
    send.setAttribute("aria-label", label);
    send.classList.toggle("stop", stop);
    send.classList.toggle("quiet", mode.endsWith("quiet"));
  }

  // ── Balance ──

  function refreshBalance() {
    if (State.roadeep.signedIn !== true) return;
    Bridge.roadeepBalance().then(
      (b) => {
        balance = b;
        State.notify();
      },
      (err) => void Bridge.log(`chat: balance unavailable ${errText(err)}`),
    );
  }

  balanceChip.el.addEventListener("click", () => {
    if (balance?.units != null && balance.units <= 0) void Bridge.openUrl(TOP_UP_URL);
    else void Bridge.openSettingsWindow("account");
  });

  // ── Log ──

  /** What the log shows, as one number: a change means a full repaint. */
  const logCount = () => State.chatHistory.length + (live ? 0.5 : 0) + (chatAlert ? 0.25 : 0);
  const atBottom = () => log.scrollHeight - log.scrollTop - log.clientHeight <= STICK_PX;

  const liveRow = h("div", { class: "chat-row live" });
  const liveHead = h("div", { class: "md-part" });
  const liveTail = h("div", { class: "md-part" });
  const liveReply = h("div", { class: "reply md" }, liveHead, liveTail);
  const liveApprovals = h("div", { class: "chat-approvals" });
  const liveSteps = h("div", { class: "tool-steps" });
  const liveHint = creditHint();
  const liveStatus = h("div", { class: "chat-status", role: "status" });
  // Tool steps and approvals come before the text: a tool runs before the
  // answer is written. The step rows come first (in order), the approval card
  // waiting for a click under them.
  liveRow.append(liveSteps, liveHint, liveApprovals, liveReply, liveStatus);
  /** The live step rows by step id, with what they show (repainted on change only). */
  const liveStepRows = new Map<string, { key: string; el: HTMLElement }>();
  /**
   * Any row changing size (a font arriving, a code block wrapping, a card
   * expanding) or the log's box settling after the island moved re-measures,
   * so the island always ends at the content's height up to CHAT_MAX_H.
   */
  const sizes = new ResizeObserver(() => scheduleMeasure());
  sizes.observe(log);
  sizes.observe(liveRow);
  void document.fonts?.ready.then(() => scheduleMeasure());
  /** Ids of the message rows in the log, in order. */
  let rowIds: number[] = [];
  let alertRow: HTMLElement | null = null;
  let renderedHead: string | null = null;
  let renderedTail: string | null = null;
  let statusKey = "";

  /**
   * Finished rows are never rebuilt (a screen reader would read the whole
   * conversation again): when the log still starts with the rows it shows, only
   * the new ones are appended; anything else (a new chat, an opened thread)
   * rebuilds it.
   */
  function renderLog(stick: boolean) {
    const keep = log.scrollTop;
    const msgs = State.chatHistory;
    const prefix = rowIds.length <= msgs.length && rowIds.every((id, i) => msgs[i].id === id);
    liveRow.remove();
    alertRow?.remove();
    alertRow = null;
    if (!prefix) {
      for (const row of log.children) if (row !== liveRow) sizes.unobserve(row);
      clear(log);
      rowIds = [];
      overflowPad = 0;
    }
    for (let i = rowIds.length; i < msgs.length; i++) {
      const row = bubble(msgs[i], approvalRecords.get(msgs[i].id), toolRecords.get(msgs[i].id));
      log.append(row);
      sizes.observe(row);
      rowIds.push(msgs[i].id);
    }
    if (live) {
      renderedHead = renderedTail = null;
      statusKey = "";
      clear(liveApprovals);
      clear(liveSteps);
      liveStepRows.clear();
      for (const a of live.approvals) a.el = null;
      renderLive();
      log.append(liveRow);
    }
    if (chatAlert) {
      alertRow = h("div", { class: "chat-row" }, alertCard(chatAlert));
      log.append(alertRow);
    }
    syncBusy();
    log.scrollTop = stick ? log.scrollHeight : keep;
    measure();
  }

  /** The turn's tool-step rows: new ones appended, changed ones replaced in place. */
  function renderSteps() {
    if (!live) return;
    for (const step of live.steps) {
      const key = `${getLanguage()}|${step.state}|${step.result ?? ""}|${step.error ?? ""}|${step.arguments}`;
      const shown = liveStepRows.get(step.id);
      if (shown?.key === key) continue;
      const el = toolStepRow(step);
      if (shown) shown.el.replaceWith(el);
      else liveSteps.append(el);
      liveStepRows.set(step.id, { key, el });
    }
    liveSteps.hidden = live.steps.length === 0;
    liveHint.hidden = !live.hint;
  }

  /** Repaints the live row; returns an approval card it just added, if any. */
  function renderLive(): HTMLElement | null {
    if (!live) return null;
    // Only the part after the last finished paragraph changes as text arrives.
    const split = stableSplit(live.text);
    const head = live.text.slice(0, split);
    const tail = live.text.slice(split);
    if (head !== renderedHead) {
      renderedHead = head;
      liveHead.replaceChildren(renderMarkdown(head));
    }
    if (tail !== renderedTail) {
      renderedTail = tail;
      liveTail.replaceChildren(renderMarkdown(tail));
    }
    liveReply.hidden = !live.text;
    renderSteps();
    let added: HTMLElement | null = null;
    for (const a of live.approvals) {
      if (!a.el) {
        added = approvalCard(a);
        liveApprovals.append(added);
        // Overflow is only known once laid out.
        requestAnimationFrame(() => checkSummary(a));
      }
      paintApproval(a);
    }
    const waiting = live.approvals.some((a) => a.state === "pending");
    // The reveal spacer only holds the card in place until the reply arrives.
    if (!waiting && live.text) liveRow.style.paddingBottom = "";
    syncBusy();
    // The status line says what the server is doing; while text streams in,
    // "generating" says nothing new.
    const showStatus = !waiting && (!live.text || (live.status != null && live.status !== "generating"));
    const key = `${getLanguage()}|${showStatus}|${live.status}|${live.label}`;
    if (key !== statusKey) {
      statusKey = key;
      clear(liveStatus);
      liveStatus.hidden = !showStatus;
      if (showStatus) {
        liveStatus.append(typingDots());
        if (live.status) {
          liveStatus.append(h("span", { text: t(`chat.status.${live.status}`) }));
        }
        if (live.label?.trim()) liveStatus.append(h("span", { class: "chat-status-label", dir: "auto", text: live.label.trim() }));
      }
    }
    setSendIcon();
    return added;
  }

  /** Repaints the live row on the next frame — and no more than every 100 ms once the reply is long. */
  function scheduleLive() {
    if (liveFrame != null || liveTimer != null) return;
    const frame = () => {
      liveFrame = requestAnimationFrame(() => {
        liveFrame = null;
        if (!live || !liveRow.isConnected) return;
        lastLiveRender = performance.now();
        const stick = atBottom();
        const card = renderLive();
        // A new approval shows from its top (what is asked comes first);
        // otherwise the log follows the text if the reader was at the end.
        if (card) {
          revealCard(card);
          pendingReveal = card;
        } else if (stick) log.scrollTop = log.scrollHeight;
        measure();
      });
    };
    const wait = live && live.text.length > LONG_TEXT ? LONG_GAP_MS - (performance.now() - lastLiveRender) : 0;
    if (wait > 0) {
      liveTimer = window.setTimeout(() => {
        liveTimer = null;
        frame();
      }, wait);
    } else frame();
  }

  /** The log is busy while a reply streams in — never while an approval waits for the user. */
  function syncBusy() {
    const busy = !!live && !live.approvals.some((a) => a.state === "pending");
    log.setAttribute("aria-busy", String(busy));
  }

  function cancelLive() {
    if (liveFrame != null) cancelAnimationFrame(liveFrame);
    if (liveTimer != null) window.clearTimeout(liveTimer);
    liveFrame = liveTimer = null;
  }

  view = {
    streamChanged: scheduleLive,
    turnEnded() {
      const stick = atBottom();
      cancelLive();
      pendingReveal = null;
      liveRow.style.paddingBottom = "";
      // Repaint now, keeping the reader's place if they scrolled up.
      renderedCount = logCount();
      renderLog(stick);
      State.notify();
      onHeightChange();
      if (State.view === "prompt") input.focus();
    },
    notify: () => State.notify(),
    announceApproval(a) {
      const summary = a.summary.replace(BIDI_CONTROLS, "").trim();
      announcer.textContent = [a.tool.replace(BIDI_CONTROLS, ""), t("approval.needsPermission"), summary]
        .filter(Boolean).join(" — ");
    },
  };

  if (!restoreChecked) {
    restoreChecked = true;
    void restoreTurn();
  }

  // ── Agent chip ──

  agentPick.addEventListener("change", () => {
    const next = agentPick.value || null;
    if (modelPicker.busy) return;
    if (next === State.settings.chatAgent) return;
    State.settings.chatAgent = next;
    currentThreadId = currentThreadModel = null;
    // The character wears the colour of the agent's pill, if it has one; otherwise an
    // agent pill must not stay in focus for a conversation it is not in.
    const pill = State.tasks.find((task) => task.agentRef && task.agentRef === next);
    if (pill) State.setFocus(pill.id);
    else if (State.focusTask?.agentRef) State.setFocus("integration_claude");
    // A different agent means a different conversation.
    if (State.chatHistory.length) {
      State.chatHistory = [];
      void Bridge.chatReset().then(() => refreshTools());
    }
    void Bridge.saveSettings(State.settings);
    Sound.play("blip");
    State.notify();
    onHeightChange();
    input.focus();
  });
  agentPick.addEventListener("keydown", (e) => e.stopPropagation());

  // ── Sending ──

  async function submit(text?: string, fromVoice = false, liveVoice = false): Promise<string> {
    const query = (text ?? input.value).trim();
    if (!query || sending || modelPicker.busy) {
      if (fromVoice) throw new Error("Chat is busy. Wait for the current turn to finish.");
      return "";
    }
    if (text == null) {
      input.value = "";
      fitInput();
    }
    syncInputDir();
    sending = true;
    chatAlert = null;
    Sound.play("send");

    // The file rides along until a reply has come back for it — not just with
    // the first message, which may have failed before the upload went out.
    const answered = State.chatHistory.some((m) => m.role === "assistant" && m.origin!=="app");
    const userMessage: ChatMessage = { id: nextId++, role: "user", content: query };
    State.chatHistory.push(userMessage);
    State.stateOverride = "thinking";
    const turn = ++turnSeq;
    live = { turn, text: "", status: null, label: null, approvals: [], steps: [], hint: false, restored: false };
    State.notify();
    onHeightChange();

    const file = fromVoice?null:State.droppedFile;
    const context: ChatContext | null =
      !answered && file ? { kind: "file", name: file.name, path: file.path } : null;

    // Everything after the await uses module state and the CURRENT view: the
    // one that started the turn may have been rebuilt meanwhile.
    let busyElsewhere = false;
    let finalText = "";
    let failure: unknown;
    try {
      const requestId=`chat-${crypto.randomUUID()}`;
      localRequestId=requestId;localRequestIdentity=routingIdentity();if(fromVoice)voiceAskTurn=turn;
      const cancelled=()=>cancelledLocalTurns.has(turn)||turn!==live?.turn||localRequestIdentity!==routingIdentity();
      if(IS_TAURI&&fromVoice&&!liveVoice){
        const analysis=await BridgeLocal.analyzeVoice(query,State.settings.adminVoiceMode==="always"?"always":"manual",getLanguage(),requestId);
        if(cancelled())throw new LocalRouteCancelled();
        if(analysis.route==="silent"){State.chatHistory=State.chatHistory.filter(message=>message!==userMessage);State.stateOverride=null;return "";}
        if(analysis.route==="proposal"||analysis.route==="done"||analysis.route==="reply"){
          if(analysis.proposal)setVoiceProposal(analysis.proposal);
          finalText=analysis.text;keepTurn(finalText,"app");State.stateOverride=null;return finalText;
        }
      }
      // Live voice acts only through its own approved tools: no unapproved desktop launch here.
      if(IS_TAURI&&fromVoice&&!liveVoice){
        live.label=t("local.desktopChecking");view?.streamChanged();
        const action=await voiceDesktopAction({fromVoice,query,language:getLanguage(),requestId,cancelled});
        if(action){
          live.steps.push({id:requestId,server:t("local.desktopServer"),tool:t("local.desktopOpen"),state:"done",arguments:JSON.stringify({target:action.action??""}),result:action.text,error:null});
          finalText=action.text;keepTurn(finalText,"app");
          unsyncedLocal=boundedHistory([...unsyncedLocal,{role:"user",text:query},{role:"assistant",text:finalText}]);
          State.stateOverride=null;Sound.play("finish");return finalText;
        }
      }
      let route:"local"|"cloud"="cloud";
      let localText="";
      let localReason="";
      if(IS_TAURI){
        const result=await localRoute({query,history:State.chatHistory.slice(0,-1).map(message=>({role:message.role,text:message.content})),forceCloud:!!file||(!fromVoice&&!!State.settings.chatAgent),requestId,cancelled:()=>cancelledLocalTurns.has(turn)||turn!==live?.turn||localRequestIdentity!==routingIdentity(),status:next=>{localStatus=next;}});
        route=result.route;localText=result.text;localReason=result.reason;
      }
      if(cancelledLocalTurns.has(turn)||turn!==live?.turn)throw new LocalRouteCancelled();
      localRequestId=undefined;
      if(route==="local"){
        finalText=localText;keepTurn(localText,localReason.includes("cpu")?"cpu":"local");
        unsyncedLocal=boundedHistory([...unsyncedLocal,{role:"user",text:query},{role:"assistant",text:localText}]);
        State.stateOverride=null;Sound.play("finish");return finalText;
      }
      if(State.roadeep.signedIn!==true){finalText=t("local.signIn");keepTurn(finalText,"app");State.stateOverride=null;return finalText;}
      live.label=t(localReason==="local-runtime-unavailable"?"local.fallback":"local.cloudReason");view?.streamChanged();
      if(cancelledLocalTurns.has(turn))throw new LocalRouteCancelled();
      const handoffHistory=fromVoice?State.chatHistory.slice(0,-1).map(message=>({role:message.role,text:message.content})):unsyncedLocal;
      const reply = await Bridge.chatSend(cloudHandoff(query,handoffHistory), context, fromVoice?null:State.settings.chatAgent, turn, fromVoice);
      if(!fromVoice)unsyncedLocal=[];
      finalText = reply.text;
      if(fromVoice&&!liveVoice){const pending=await BridgeLocal.proposalPending();if(cancelledLocalTurns.has(turn)||turn!==live?.turn)throw new LocalRouteCancelled();if(pending.proposal){setVoiceProposal(pending.proposal);finalText=t("adminVoice.proposalQuestion",{text:pending.proposal.text});}}
      // The final text replaces whatever streamed in.
      keepTurn(finalText,"cloud",t(localReason==="local-runtime-unavailable"?"local.fallback":"local.cloudReason"));
      if (!fromVoice && reply.threadId) {
        currentThreadModel = reply.model ?? null;
        currentThreadId = reply.threadId;
      }
      State.stateOverride = null;
      Sound.play("finish");
    } catch (err) {
      failure = err;
      State.stateOverride = null;
      // Tool steps that already ran (or were declined) stay in the log, whatever ended the turn.
      const busy = isRoadeepError(err) && err.code === "CHAT_BUSY";
      if (!busy && !fromVoice && live?.steps.length) keepTurn("");
      if (err instanceof LocalRouteCancelled || (isRoadeepError(err) && err.code === "CANCELLED")) {
        // The user pressed stop: nothing went wrong.
      } else if(fromVoice&&!busy){
        if(isSignedOutError(err)){State.roadeep.signedIn=false;State.roadeep.expired=isRoadeepError(err)&&err.code==="SESSION_EXPIRED";}
        finalText=isSignedOutError(err)?t("local.signIn"):isInsufficientCredits(err)?chatErrorText(err):voiceTaskError(isRoadeepError(err)?err.code:err);
        keepTurn(finalText,"app");failure=undefined;
        void Bridge.log(`voice task failed turn=${turn} request=${isRoadeepError(err)?err.requestId??"none":"native"} code=${isRoadeepError(err)?err.code:"local-action-or-chat"}`);Sound.play("error");
      } else if (isRoadeepError(err) && err.code === "CHAT_BUSY") {
        // Another turn is still being written (started before a reload): take
        // this message back and show that one instead.
        void Bridge.log("chat: send refused, a turn is already in flight");
        State.chatHistory = State.chatHistory.filter((m) => m !== userMessage);
        busyElsewhere = true;
      } else if (isSignedOutError(err)) {
        State.roadeep.signedIn = false;
        State.roadeep.expired = isRoadeepError(err) && err.code === "SESSION_EXPIRED";
        Sound.play("error");
      } else if (isInsufficientCredits(err)) {
        // Said in the chat, with the way out, rather than as a passing note.
        void Bridge.log(`chat: send failed ${errText(err)}`);
        // Rust pushes the fresh balance ("roadeep-balance") after this failure.
        chatAlert = { kind: "credits" };
        Sound.play("error");
      } else if (isRoadeepError(err) && err.code === "AGENT_MEMORY_TRIGGER") {
        // Rust already dropped the thread: the next message starts a new one.
        void Bridge.log("chat: send failed AGENT_MEMORY_TRIGGER");
        currentThreadId = null;
        currentThreadModel = null;
        chatAlert = { kind: "agentMemory", agent: err.agentName?.trim() || chosenAgentName() };
        Sound.play("error");
      } else {
        if (isRoadeepError(err) && err.code === "LOCAL_AGENT_NOT_FOUND") void refreshAgents();
        void Bridge.log(`chat: send failed ${isRoadeepError(err) ? `${err.code} ${err.requestId ?? ""}` : ""}`);
        State.noteMessage = chatErrorText(err);
        State.view = "note";
        Sound.play("error");
      }
    } finally {
      cancelledLocalTurns.delete(turn);localRequestId=undefined;localRequestIdentity=undefined;voiceAskTurn=undefined;
      endTurn();
    }
    if (busyElsewhere) void restoreTurn();
    if (fromVoice && failure) throw failure;
    return finalText;
  }

  send.addEventListener("click", () => {
    if (sending) {
      if(live)cancelledLocalTurns.add(live.turn);
      if(localRequestId)void BridgeLocal.cancel(localRequestId).catch(()=>void Bridge.log("local chat cancellation failed"));
      Bridge.chatCancel().catch((err) => {
        void Bridge.log(`chat: cancel failed ${isRoadeepError(err) ? err.code : String(err)}`);
      });
      return;
    }
    void submit();
  });
  input.addEventListener("keydown", (e) => {
    const k = e as KeyboardEvent;
    if (k.key === "Enter" && !k.isComposing && !(k.shiftKey && State.maximized)) {
      e.preventDefault();
      void submit();
    }
    e.stopPropagation(); // Escape closes the island, not the chat
  });

  return {
    el,
    sync() {
      assistant.el.style.setProperty("--assistant-panel-max",`${Math.max(150,Math.min(480,CHAT_MAX_H-180))}px`);
      assistant.sync();
      modelPicker.sync();
      const r = State.roadeep;
      const signedOut = r.signedIn === false && !localReady();
      body.classList.toggle("local-ready",localReady());
      localNotice.hidden=r.signedIn===true;
      if(!State.chatHistory.length)unsyncedLocal=[];
      body.classList.toggle("signed-out", signedOut);
      if (signedOut) {
        signInSub.textContent = t(r.expired ? "chat.expiredSub" : "chat.signInSub");
        // The sign-in card is compact, whatever the conversation measured.
        if (wanted != null) {
          wanted = null;
          onHeightChange();
        }
        balance = null;
        currentThreadId = null;
        currentThreadModel = null;
        chatAlert = null;
        toolsState = null;
        toolsAsked = false;
        if (historyOpen) closeHistory();
        balanceChip.set(null);
        return;
      }
      balanceChip.set(r.signedIn === true ? balance : null);
      if (r.signedIn === true && !toolsAsked) {
        toolsAsked = true;
        refreshTools();
      }
      // Emptied elsewhere (agent switch, a new drop, sign-out): a new thread.
      if (!sending && State.chatHistory.length === 0) {
        currentThreadId = currentThreadModel = null;
      }

      const file = State.droppedFile;
      const wantChip = file?.name ?? "";
      if (chipRow.dataset.label !== wantChip) {
        chipRow.dataset.label = wantChip;
        clear(chipRow);
        if (wantChip) chipRow.append(contextChip(wantChip));
      }

      const key = [
        getLanguage(),
        State.settings.chatAgent ?? "",
        ...r.agents.map((a) => `${a.id}=${a.title}`),
        ...r.localAgents.map((a) => `${a.id}=${a.name}`),
      ].join("|");
      if (key !== agentKey) {
        agentKey = key;
        fillAgentOptions(agentPick);
      }
      agentPick.value = State.settings.chatAgent ?? "";
      sizeAgentPick();
      agentPick.classList.toggle("on", !!State.settings.chatAgent);
      agentPick.disabled = sending || modelPicker.busy;
      syncToolsChip();
      historyBtn.disabled = sending || modelPicker.busy;

      // Starter chips: only in an empty conversation with an agent chosen.
      const prompts = State.chatHistory.length === 0 && !sending && !chatAlert ? starterPrompts().slice(0, 3) : [];
      const color = chosenAgentColor();
      const sKey = [getLanguage(), color ?? "", ...prompts].join("|");
      if (sKey !== startersKey) {
        startersKey = sKey;
        clear(starters);
        for (const p of prompts) {
          const chip = h("button", {
            type: "button", class: "starter", dir: "auto", title: p, text: p,
            onclick: () => void submit(p),
          });
          if (color) chip.style.setProperty("--agent", color);
          starters.append(chip);
        }
      }

      const count = logCount();
      if (count !== renderedCount) {
        renderedCount = count;
        renderLog(true);
      }

      input.placeholder = t(State.chatHistory.length === 0 ? "chat.placeholder" : "chat.placeholderContinue");
      input.disabled = sending || modelPicker.busy;
      if (fittedFor !== State.maximized) {
        // Maximised and back: the field wraps (or not) and takes its new height.
        fittedFor = State.maximized;
        requestAnimationFrame(fitInput);
      }
      bar.classList.toggle("busy", sending);
      setSendIcon();
    },
    focus() {
      scheduleMeasure();
      void refreshAgents();
      if (State.roadeep.signedIn === false && !localReady()) return;
      refreshBalance();
      refreshTools();
      if (historyOpen) return;
      input.focus();
      input.select();
    },
  };
}
