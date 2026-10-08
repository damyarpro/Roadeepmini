// What a coding session leaves on its pill besides its steps, as the macOS
// HookServer keeps it: the diffs of the files it edited (live diff), the
// turn's final message once the turn is over, and how many steps the session
// has had in all (State keeps only the last 20).
//
// Kept beside State rather than in it: none of it is a setting, the settings
// window never reads it, and every entry is bounded — 50 diffs per pill,
// forgotten an hour after the last one or at the end of the session.

import type { FileDiff } from "../core/diff";

/** Live diffs kept per pill (oldest dropped first) — same cap as macOS. */
export const MAX_DIFFS_PER_PILL = 50;
/** A pill's diffs are forgotten after an hour without a new one, as on macOS. */
export const DIFF_TTL_MS = 3_600_000;

const diffs = new Map<string, FileDiff[]>();
const diffTimers = new Map<string, number>();
/** Never reset, so an id can never point at a newer diff than the one clicked. */
let nextDiffId = 0;

/** Stores a diff for a pill and returns its id (the ticker step carries it). */
export function appendSessionDiff(pillId: string, diff: FileDiff): number {
  const id = nextDiffId++;
  const list = diffs.get(pillId) ?? [];
  list.push({ ...diff, id });
  while (list.length > MAX_DIFFS_PER_PILL) list.shift();
  diffs.set(pillId, list);
  // One timer per pill, re-armed on every diff: nothing polls.
  const previous = diffTimers.get(pillId);
  if (previous != null) window.clearTimeout(previous);
  diffTimers.set(pillId, window.setTimeout(() => clearSessionDiffs(pillId), DIFF_TTL_MS));
  return id;
}

export function findDiff(pillId: string, id: number): FileDiff | null {
  return diffs.get(pillId)?.find((d) => d.id === id) ?? null;
}

/** The pill's diffs, oldest first. */
export function sessionDiffs(pillId: string): readonly FileDiff[] {
  return diffs.get(pillId) ?? [];
}

export function clearSessionDiffs(pillId: string) {
  const timer = diffTimers.get(pillId);
  if (timer != null) window.clearTimeout(timer);
  diffTimers.delete(pillId);
  diffs.delete(pillId);
}

// ── The final line ────────────────────────────────────────────────────────────

/** Claude's final message after Stop, on one line; cleared when a new turn starts. */
const finalLines = new Map<string, string>();

export function setFinalLine(pillId: string, text: string | null) {
  if (text) finalLines.set(pillId, text);
  else finalLines.delete(pillId);
}

export function finalLine(pillId: string): string | null {
  return finalLines.get(pillId) ?? null;
}

// ── Step count ────────────────────────────────────────────────────────────────

/** Position of the newest step in the whole session, per pill. */
const stepSeqs = new Map<string, number>();

/**
 * One step was appended to the pill. `lengthBefore` is its step list's length
 * before the append, which is where the count starts on the first step.
 */
export function countStep(pillId: string, lengthBefore: number) {
  const newest = stepSeqs.get(pillId) ?? lengthBefore - 1;
  stepSeqs.set(pillId, newest + 1);
}

/** Where the pill's newest step sits in its session; undefined before any. */
export function stepSeq(pillId: string): number | undefined {
  return stepSeqs.get(pillId);
}

/** The pill's session is over or starts again: its trail goes with it. */
export function clearSessionTrail(pillId: string) {
  clearSessionDiffs(pillId);
  finalLines.delete(pillId);
  stepSeqs.delete(pillId);
}
