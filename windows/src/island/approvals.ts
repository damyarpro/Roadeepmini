// The card a coding agent waits on: a permission request (Deny / Allow) or one
// of Claude Code's questions (AskUserQuestion, answered by picking options).
// Port of the macOS HookServer's beginApproval / endApproval.
//
// State.pendingApproval holds the request; the questions of a question card
// are kept here beside it, tied to its request id, so a stale list can never
// be shown for another request. What the relay forwards of them, and how an
// answer goes back (by position), is core/bridge-hooks.ts.

import { State, type ApprovalInfo } from "../core/state";
import type { IslandViewName } from "../core/layout";
import type { AskedQuestion } from "../core/bridge-hooks";

let asked: { requestId: string; questions: AskedQuestion[] } | null = null;
/** The pill that was in front when the card came up; it comes back after. */
let focusBefore: string | null = null;

/**
 * A permission card or a question comes up: its pill comes to the front, and
 * the pill that was there is remembered (HookServer.focusBeforeApproval).
 */
export function beginApproval(info: ApprovalInfo, questions: AskedQuestion[] | null) {
  // The same request sent again keeps the pill remembered the first time.
  if (State.pendingApproval?.requestId !== info.requestId) focusBefore = State.focusId;
  State.pendingApproval = info;
  asked = questions ? { requestId: info.requestId, questions } : null;
  State.isPinned = true;
  if (info.taskId && State.tasks.some((t) => t.id === info.taskId)) State.setFocus(info.taskId);
}

/**
 * The card has its answer, or is withdrawn: the session carries on, and the
 * pill you were on comes back — unless you moved to another one meanwhile.
 * The pin and the view are the island's to settle (Island.cardCleared).
 */
export function endApproval() {
  const req = State.pendingApproval;
  if (!req) return;
  State.pendingApproval = null;
  asked = null;
  const pill = req.taskId ?? "integration_claude";
  State.updateTask(pill, "working");
  State.setPillBadge(pill, null);
  const previous = focusBefore;
  focusBefore = null;
  if (previous && State.focusId === pill && State.tasks.some((t) => t.id === previous)) {
    State.focusId = previous;
  }
  State.notify();
}

/** The questions of the waiting request, when it is a question card. */
export function pendingQuestions(): AskedQuestion[] | null {
  const req = State.pendingApproval;
  return req && asked?.requestId === req.requestId ? asked.questions : null;
}

/** The view the waiting card is shown on, or null when nothing waits. */
export function cardView(): Extract<IslandViewName, "approval" | "question"> | null {
  if (!State.pendingApproval) return null;
  return pendingQuestions() ? "question" : "approval";
}
