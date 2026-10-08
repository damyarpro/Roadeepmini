// The body stays the shape chosen in Settings, in every state.
//
// bloub's states morph the body itself (the "!" bar, the thinking dots, the egg, the
// triangle, the collapse of burst and comet…). The user wants the chosen shape kept,
// with everything else animating as before. This is done here, on the SAMPLED frame,
// so the bloub engine stays intact:
//
// - the body path is always the chosen shape's silhouette;
// - eyes come from the animated engine when its state shows them, otherwise from a
//   second engine held on the resting face (same expression, gaze and blinks), so the
//   character never reads as a blank blob; the two cross-fade during state fades;
// - states whose silhouette is a glyph rather than a body (thinking dots, the "!" of
//   alert and exclaim, the bouncing sleep dot) keep that glyph — animated exactly as
//   sampled — but miniature, beside the head;
// - decor (rings, ribbons, pastille, particles) is kept; burst's particles, which hid
//   behind the collapsing body, now spiral in front of the full one.

import { BotEngine as BloubEngine, type BotFrame, type RenderedEye } from "./bloub/engine";
import { EXPRESSION_BY_ID } from "./bloub/expressions";
import { SHAPE_BY_ID } from "./bloub/skins";
import type { DotRender } from "./bloub/decor";
import { closedPath, toPoints } from "./bloub/shape";
import type { StateId } from "./bloub/states";
import { UNIT } from "./render";

/** States whose sampled silhouette is a glyph, not a body. */
export const GLYPH_STATES: ReadonlySet<StateId> = new Set<StateId>(["thinking", "alert", "exclaim", "sleep"]);

/** Where the glyph sits, in ball radii from the body's centre, and its scale. */
export const GLYPH = { x: 1.02, y: -1.08, scale: 0.4 } as const;

export interface Glyph {
  bodyPath: string;
  dots: DotRender[];
  alpha: number;
  x: number;
  y: number;
  scale: number;
}

export type CharacterFrame = BotFrame & { glyph: Glyph | null };

const bodies = new WeakMap<number[], string>();
/** The chosen shape's outline at rest, in engine units (cached per profile). */
export function fixedBodyPath(radii: number[]): string {
  let d = bodies.get(radii);
  if (!d) {
    d = closedPath(toPoints({ radii, rot: 0, cx: 0, cy: 0, sx: 1, sy: 1 }, UNIT));
    bodies.set(radii, d);
  }
  return d;
}

/**
 * Composes the frame actually drawn.
 * @param animated  the animated engine's frame (bloub state, decor, its own eyes)
 * @param face      the resting-face engine's frame (eyes when the state has none)
 * @param state     the animated engine's current state
 * @param radii     the chosen shape's profile
 * @param glyphIn   0…1 fade of the glyph (0 just after entering a glyph state from a body state)
 */
export function composeFixed(animated: BotFrame, face: BotFrame | null, state: StateId, radii: number[], glyphIn = 1): CharacterFrame {
  const own = animated.eyes.reduce((m, e) => Math.max(m, e.alpha), 0);
  const eyes: RenderedEye[] = [...animated.eyes];
  if (face && own < 0.99) {
    for (const e of face.eyes) eyes.push({ ...e, alpha: e.alpha * (1 - own) });
  }
  const glyphState = GLYPH_STATES.has(state);
  const glyph: Glyph | null = glyphState && glyphIn > 0.01
    ? { bodyPath: animated.bodyPath, dots: animated.dots, alpha: glyphIn, x: GLYPH.x * UNIT, y: GLYPH.y * UNIT, scale: GLYPH.scale }
    : null;
  return {
    ...animated,
    bodyPath: fixedBodyPath(radii),
    bodyAlpha: 1,
    eyes,
    dots: glyphState ? [] : animated.dots,
    // Behind a full-size body the burst's particles would never be seen.
    dotsBehind: false,
    glyph,
  };
}

/**
 * One frame of a bloub state at local time `t` on the fixed body — for previews
 * (settings tiles, the state strip, the dev board) that have no frame loop.
 */
export function stillFrame(look: { shape: string; expression: string }, state: StateId, t: number): CharacterFrame {
  const radii = SHAPE_BY_ID.get(look.shape)?.radii ?? SHAPE_BY_ID.get("cercle")!.radii;
  const expr = EXPRESSION_BY_ID.get(look.expression) ?? null;
  const animated = new BloubEngine(UNIT, state, radii, expr);
  const face = new BloubEngine(UNIT, "idle", radii, expr);
  return composeFixed(animated.sample(t), face.sample(t), state, radii);
}
