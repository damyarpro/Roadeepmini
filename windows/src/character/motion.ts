// What the character does in each app state: a pure table from Roadeep's states,
// emotes and celebrations onto bloub's 14 animated states, its rest expressions and
// its montage blocks. The engine facade (engine.ts) only plays what is written here.

import type { BotEmoteName, BotStateName } from "../core/layout";
import { makeBlock, type Block } from "./bloub/cycles";
import type { ExpressionId } from "./bloub/expressions";
import { SEQUENCE, type StateId } from "./bloub/states";

export type CelebrationKind = "task" | "focus" | "finished";

export interface Choreography {
  /** Played once on entering the app state. */
  intro: Block[];
  /** Then looped for as long as the state lasts (a single block simply holds). */
  loop: Block[];
  /** Rest expression shown while this app state is on a face (`idle`) block. */
  expression?: ExpressionId;
  /** The one still block used instead of everything above under reduced motion. */
  calm: StateId;
}

const hold = (state: StateId): Block => ({ state, duration: Number.POSITIVE_INFINITY });
const once = (state: StateId, duration?: number): Block => duration == null ? makeBlock(state) : { state, duration };

/**
 * Every Roadeep state, and why it looks the way it does:
 *   idle       the ball at rest, with the expression chosen in Settings
 *   working    a "play" triangle kicks off, then the three typing dots
 *   thinking   rings orbit the ball, then it narrows into the pondering egg
 *   searching  a comet trail sweeps round, then wide eyes look around
 *   approval   the "!" travels, then stands upright — it needs you
 *   question   the notification pastille: something to answer
 *   error      the body bursts and regathers, then looks sad
 *   finished   a wink, then a happy face
 *   ratelimit  the hexagon — a stop sign, waiting
 *   sleeping   the small bouncing dot
 *   dizzy      a burst (three slaps), then a confused face
 */
export const STATE_CHOREOGRAPHY: Readonly<Record<BotStateName, Choreography>> = {
  idle: { intro: [], loop: [hold("idle")], calm: "idle" },
  working: { intro: [once("play")], loop: [hold("thinking")], calm: "thinking" },
  thinking: { intro: [], loop: [once("orbit"), once("egg", 2.2)], calm: "egg" },
  searching: { intro: [], loop: [once("comet"), once("wide", 1.6)], calm: "wide" },
  approval: { intro: [], loop: [once("alert"), once("exclaim", 1.8)], calm: "exclaim" },
  question: { intro: [], loop: [hold("notify")], calm: "notify" },
  error: { intro: [once("burst")], loop: [hold("idle")], expression: "triste", calm: "idle" },
  finished: { intro: [once("wink")], loop: [hold("idle")], expression: "heureux", calm: "idle" },
  ratelimit: { intro: [], loop: [hold("hexagon")], calm: "hexagon" },
  // The body stays put (fixed.ts): the bouncing dot rides beside a drowsy face.
  sleeping: { intro: [], loop: [hold("sleep")], expression: "somnolent", calm: "sleep" },
  dizzy: { intro: [once("burst")], loop: [hold("idle")], expression: "confus", calm: "idle" },
};

/**
 * An emote is short: its expression shows on the rest face (the character turns to
 * face you for its duration), and two of them also play a bloub state.
 */
export const EMOTE_CHOREOGRAPHY: Readonly<Record<BotEmoteName, { expression: ExpressionId; state?: StateId }>> = {
  love: { expression: "heureux" },
  surprised: { expression: "surpris", state: "wide" },
  proud: { expression: "fier" },
  wink: { expression: "heureux", state: "wink" },
  yawn: { expression: "somnolent" },
  happy: { expression: "hilare" },
  annoyed: { expression: "colere" },
};

/** Celebrations: `swirl` (rings + a turn of the eyes), never on mini characters. */
export const CELEBRATION_CHOREOGRAPHY: Readonly<Record<CelebrationKind, { state: StateId | null; expression: ExpressionId; seconds: number }>> = {
  task: { state: "swirl", expression: "heureux", seconds: 1.6 },
  focus: { state: "swirl", expression: "hilare", seconds: 2 },
  // "finished" already winks on entry; the celebration only keeps the smile.
  finished: { state: null, expression: "heureux", seconds: 1.6 },
};

/**
 * Local time after which a bloub state's pose stops changing (only the resting life —
 * gaze drift, blinks — goes on). Infinity = it loops. Read off each state's `pose()`
 * constants in bloub/states.ts; the island lets its frame loop sleep past it.
 */
export const SETTLE_AT: Readonly<Record<StateId, number>> = {
  idle: 0, wink: 0, wide: 0, exclaim: 0, egg: 0, hexagon: 0,
  notify: 0.45,
  // the "!" keeps a 2.5 Hz buzz for as long as it is shown
  alert: Number.POSITIVE_INFINITY,
  play: 2.2,
  burst: 2.4,
  comet: 2.45,
  swirl: 1.3,
  orbit: 3.6,
  thinking: Number.POSITIVE_INFINITY,
  sleep: Number.POSITIVE_INFINITY,
};

/** The 14 catalogue states, in bloub's reference order (swirl is a transition). */
export const CATALOGUE: readonly StateId[] = SEQUENCE;

/**
 * Which block of a choreography plays `elapsed` seconds after entering the state.
 * Intro blocks play once; loop blocks repeat. `index` counts every block played, so
 * the caller can tell a new block from the same one (a montage can repeat a state).
 */
export function choreographyAt(c: Choreography, elapsed: number, reduced = false): { state: StateId; index: number; since: number } {
  if (reduced) return { state: c.calm, index: 0, since: Math.max(0, elapsed) };
  let t = Math.max(0, Number.isFinite(elapsed) ? elapsed : 0);
  let index = 0;
  for (const b of c.intro) {
    if (t < b.duration) return { state: b.state, index, since: t };
    t -= b.duration;
    index++;
  }
  const total = c.loop.reduce((s, b) => s + b.duration, 0);
  if (!Number.isFinite(total)) {
    for (const b of c.loop) {
      if (t < b.duration) return { state: b.state, index, since: t };
      t -= b.duration;
      index++;
    }
    const last = c.loop[c.loop.length - 1];
    return { state: last.state, index: index - 1, since: t + last.duration };
  }
  const turns = Math.floor(t / total);
  t -= turns * total;
  index += turns * c.loop.length;
  for (const b of c.loop) {
    if (t < b.duration) return { state: b.state, index, since: t };
    t -= b.duration;
    index++;
  }
  return { state: c.loop[0].state, index, since: 0 };
}
