// What the island can say to a coding agent beyond Allow / Deny, and the hook
// payload fields that go with it: answering Claude Code's AskUserQuestion, and
// the edit text of a finished Edit / MultiEdit / Write for the live diff.
// Rust side: src-tauri/src/pipe.rs (approval_answer) and hook/src/protocol.rs
// (what the relay forwards). Kept apart from bridge.ts.
//
// The permission card itself keeps using bridge.ts: approvalAck once the card is
// up, approvalDecision for Allow / Deny, approvalDecline when nobody can act on
// it. Folding a waiting card away is not a decline: the request keeps waiting
// (up to 108 s after the ack) until it is answered, or withdrawn by the relay
// timing out, and nothing is ever decided for the user.

import { invoke } from "@tauri-apps/api/core";
import { Bridge, IS_TAURI } from "./bridge";

/** The tool through which Claude Code asks the user questions. */
export const QUESTION_TOOL = "AskUserQuestion";

/** The most questions, and options per question, the relay forwards (protocol.rs). */
export const MAX_QUESTIONS = 8;
export const MAX_OPTIONS = 16;

export interface AskedOption {
  label: string;
  /** "" when Claude Code gave none. */
  description: string;
}

/**
 * One question of Claude Code's AskUserQuestion, as `tool_input.questions` of a
 * `PermissionRequest` hook payload (provider "claude", tool_name
 * "AskUserQuestion"). The relay forwards the list only when the island can offer
 * every question: 1–8 questions with distinct texts, each with 2–16 options with
 * distinct labels. Otherwise `questions` is absent and the request gets the
 * ordinary Allow / Deny card.
 *
 * The texts are for display only: they may arrive shortened ("…", "[truncated]")
 * or with a secret masked ("[redacted]"). Answers go back by position and the
 * relay puts Claude Code's own texts back in, so what Claude Code receives never
 * depends on what the island was shown.
 */
export interface AskedQuestion {
  question: string;
  /** Claude Code's short chip label for the question; "" when none. */
  header: string;
  multiSelect: boolean;
  options: AskedOption[];
}

/**
 * The answer to one question, by position: the index of the picked option, or
 * for a multi-select question a non-empty list of distinct option indexes.
 * The answer list follows the questions' order, one entry per question.
 */
export type QuestionAnswer = number | number[];

/**
 * Hook payload fields for the live diff. PostToolUse of Claude Code's Edit,
 * MultiEdit and Write carries the edit text in `tool_input` (`old_string` /
 * `new_string`, `edits[].old_string` / `edits[].new_string`, `content`, next to
 * `file_path`), up to 256 KB per string and 512 KB together. When any of it had
 * to be cut, or a secret in it was masked, `roadeep_diff_truncated` is true and
 * the diff must not be shown with counts it cannot trust.
 */
export interface DiffHookFields {
  roadeep_diff_truncated?: boolean;
}

/** `tool_input` of a finished Edit / MultiEdit / Write, as forwarded. */
export interface EditHookInput {
  file_path?: string;
  old_string?: string;
  new_string?: string;
  content?: string;
  edits?: { old_string: string; new_string: string }[];
}

const isInt = (v: unknown, below: number): v is number =>
  typeof v === "number" && Number.isInteger(v) && v >= 0 && v < below;

/**
 * The questions of a permission request, when it is Claude Code asking and the
 * relay forwarded something the island can offer; null for anything else
 * (another tool, nothing forwarded, or a shape this build does not know).
 */
export function askedQuestions(
  toolName: string | undefined,
  input: Record<string, unknown> | undefined,
): AskedQuestion[] | null {
  if (toolName !== QUESTION_TOOL || !input || !Array.isArray(input.questions)) return null;
  const raw = input.questions as unknown[];
  if (raw.length === 0 || raw.length > MAX_QUESTIONS) return null;
  const out: AskedQuestion[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") return null;
    const q = item as Record<string, unknown>;
    if (typeof q.question !== "string" || !q.question || !Array.isArray(q.options)) return null;
    const options: AskedOption[] = [];
    for (const o of q.options as unknown[]) {
      const option = o && typeof o === "object" ? (o as Record<string, unknown>) : null;
      if (!option || typeof option.label !== "string" || !option.label) return null;
      options.push({
        label: option.label,
        description: typeof option.description === "string" ? option.description : "",
      });
    }
    if (options.length < 2 || options.length > MAX_OPTIONS) return null;
    out.push({
      question: q.question,
      header: typeof q.header === "string" ? q.header : "",
      multiSelect: q.multiSelect === true,
      options,
    });
  }
  return out;
}

/**
 * True when `answers` answers every question once, in order: an option index for
 * a single-select question, a non-empty list of distinct option indexes for a
 * multi-select one. The relay applies the same rule to the questions as Claude
 * Code asked them, and answers nothing when it does not hold.
 */
export function answersFit(questions: AskedQuestion[], answers: QuestionAnswer[]): boolean {
  if (questions.length === 0 || answers.length !== questions.length) return false;
  return questions.every((q, i) => {
    const a = answers[i];
    const n = q.options.length;
    if (!q.multiSelect) return isInt(a, n);
    return (
      Array.isArray(a) &&
      a.length > 0 &&
      a.every((x) => isInt(x, n)) &&
      new Set(a).size === a.length
    );
  });
}

export const HooksBridge = {
  /**
   * Answers the question Claude Code asked on the card of `requestId`. Resolves
   * true once the answer is on its way to the relay; false when it was refused
   * (wrong shape, or a request that is no longer waiting) — the app has then
   * handed the request back to the terminal — or outside Tauri. The card goes
   * away either way.
   */
  approvalAnswer: async (requestId: string, answers: QuestionAnswer[]): Promise<boolean> => {
    if (!IS_TAURI) return false;
    try {
      await invoke<void>("approval_answer", { requestId, answers });
      return true;
    } catch (err) {
      console.error("[roadeep] approval_answer failed", err);
      void Bridge.log(`approval_answer failed: ${String(err)}`);
      return false;
    }
  },
};
