// The character — bloub (src/character/bloub, MIT, Jérémy Perret) driven by Roadeep's
// states. bloub is a clock-free engine: `sample(t)` is a pure function of time. This
// facade owns the clock (advanced only by `update(dt)`, so a sleeping frame loop freezes
// the character instead of making it jump), maps app states, emotes and celebrations
// onto bloub states and expressions (motion.ts), and paints the frame (render.ts).
//
// The public surface is the one the island, the mini characters and the settings
// already used: setState, triggerEmote, celebrate, slap, gulp, animateMorph, blink,
// busy, update, draw, and the static preferences.

import { Sound } from "../core/sound";
import type { BotEmoteName, BotStateName } from "../core/layout";
import { BotEngine as BloubEngine, type Look } from "./bloub/engine";
import { GLYPH_STATES, composeFixed, type CharacterFrame } from "./fixed";
import { EXPRESSION_BY_ID, type ExpressionId } from "./bloub/expressions";
import { SHAPE_BY_ID, type ShapeId } from "./bloub/skins";
import { STATE_BY_ID, type StateId } from "./bloub/states";
import { minDurationOf } from "./bloub/cycles";
import { TOUR_TIME, tourLook } from "./bloub/gaze";
import { clamp, easings } from "./bloub/math";
import { DEFAULT_APPEARANCE, colorHex, normalizeAppearance, type CharacterAppearance } from "./appearance";
import {
  CELEBRATION_CHOREOGRAPHY, EMOTE_CHOREOGRAPHY, SETTLE_AT, STATE_CHOREOGRAPHY, choreographyAt,
  type CelebrationKind,
} from "./motion";
import { UNIT, paintFrame } from "./render";

export type { CelebrationKind };
export type RGB = readonly [number, number, number]; // components 0…1

/**
 * Pre-bloub eye names. Kept as the type of `AgentTask.miniEye`; each maps onto the
 * closest bloub expression.
 */
export type EyeShape =
  | "pill" | "wide" | "dot" | "line" | "flat" | "happy" | "closed"
  | "spiral" | "heart" | "star" | "tired" | "wink" | "cup";

const EYE_EXPRESSION: Record<EyeShape, ExpressionId> = {
  pill: "neutre", wide: "surpris", dot: "attentif", line: "colere", flat: "blase", happy: "heureux",
  closed: "somnolent", spiral: "confus", heart: "heureux", star: "fier", tired: "somnolent",
  wink: "heureux", cup: "surpris",
};

/**
 * How restless the resting eyes are (Settings → General). States, emotes and
 * celebrations play the same in every mode; only the resting life changes.
 *   normal — eyes follow the cursor, bloub's gaze drift and blink schedule.
 *   calm   — half the cursor range, slower blinks.
 *   still  — eyes stay put (no follow, no drift), slow blinks.
 */
export type EyeMotion = "normal" | "calm" | "still";

/** Share of the cursor-follow range the eyes use. */
export function lookGain(mode: EyeMotion): number {
  return mode === "normal" ? 1 : mode === "calm" ? 0.5 : 0;
}

/** Gaze drift and blink cadence, relative to bloub's measured ones. */
export function lifeFor(mode: EyeMotion): { wander: number; blinkRate: number } {
  return mode === "normal" ? { wander: 0.5, blinkRate: 1 } : mode === "calm" ? { wander: 0.5, blinkRate: 0.7 } : { wander: 0, blinkRate: 0.5 };
}

/**
 * The mode the character actually uses: an OS asking for less motion turns
 * the default "normal" into "calm"; an explicit choice is kept.
 */
export function effectiveEyeMotion(setting: EyeMotion, reducedMotion: boolean): EyeMotion {
  return setting === "normal" && reducedMotion ? "calm" : setting;
}

/** State → sound, as in BotStateCfg.sound. */
export const STATE_SOUND: Partial<Record<BotStateName, string>> = {
  working: "work", thinking: "think", searching: "search", approval: "approval",
  question: "question", error: "error", finished: "finish", ratelimit: "rate",
  sleeping: "sleep", dizzy: "dizzy",
};

export function hexToRGB(hex: string): RGB {
  const v = parseInt(hex.replace("#", ""), 16);
  return [((v >> 16) & 255) / 255, ((v >> 8) & 255) / 255, (v & 255) / 255];
}

const rgbHex = (c: RGB) => `#${c.map((v) => Math.round(clamp(v) * 255).toString(16).padStart(2, "0")).join("")}`;

/**
 * Shared by every character on screen (the island's and the mini ones), so one
 * preference governs them all. Set by the island from the settings.
 */
const prefs = { eyeMotion: "normal" as EyeMotion, celebrations: true, reducedMotion: false };
let appearance: CharacterAppearance = { ...DEFAULT_APPEARANCE };

/** The appearance every character currently uses (set from the settings). */
export const currentAppearance = (): CharacterAppearance => ({ ...appearance });

/** Cursor follow, in bloub head degrees: a little more than bloub's settings view, the
 * island character is small and far from the cursor. */
const YAW_RANGE = 24;
const PITCH_REST = 8;
const PITCH_RANGE = 16;
/** bloub's gaze-script catch-up (BloubBot.vue `SCRIPT_MORPH`): never 0, or 0/0. */
const SCRIPT_MORPH = 1 / 60;
const SLAP_WINDOW = 1.7;

type Squash = { kind: "slap" | "gulp"; at: number };
/** Squash keys: [time s, sx, sy]; ease in/out between them. */
const SQUASH_KEYS: Record<Squash["kind"], readonly (readonly [number, number, number])[]> = {
  slap: [[0, 1, 1], [0.07, 1.16, 0.78], [0.2, 0.95, 1.1], [0.37, 1, 1]],
  gulp: [[0, 1, 1], [0.08, 1.28, 0.78], [0.21, 0.92, 1.18], [0.43, 1, 1]],
};

function squashAt(s: Squash | null, now: number): [number, number] {
  if (!s) return [1, 1];
  const keys = SQUASH_KEYS[s.kind];
  const t = now - s.at;
  for (let i = 1; i < keys.length; i++) {
    const [t1, x1, y1] = keys[i];
    const [t0, x0, y0] = keys[i - 1];
    if (t <= t1) {
      const p = easings.easeInOutCubic(clamp((t - t0) / (t1 - t0)));
      return [x0 + (x1 - x0) * p, y0 + (y1 - y0) * p];
    }
  }
  return [1, 1];
}

interface Timed { until: number }
interface OneShot extends Timed { state: StateId; expression: ExpressionId | null }
interface Emote extends Timed { expression: ExpressionId }

export class BotEngine {
  isMini = false;
  /** Solid body colour for mini characters / integration pills (null = the chosen colour). */
  bodyColor: RGB | null = null;
  /** Extra canvas height above the body (the island's canvas is taller than wide). */
  particleOverhang = 0;
  /**
   * What the eye holes show: null erases them (a canvas holding only this character),
   * a colour paints them (a canvas shared with other drawings).
   */
  paper: string | null = null;

  /** Cursor direction, −1…1: right / up positive (tanh of the distance, from the island). */
  lookX = 0;
  lookY = 0;

  /** Eye size, eased toward `tgEs` (the island enlarges them on hover). */
  es = 1;
  tgEs = 1;

  /** Mailbox morph 0…1 (file dragged over the island) and its slot spring. */
  morph = 0;
  slotH = 0;
  slotHTarget = 0;
  slotHVel = 0;
  isChewing = false;

  state: BotStateName = "idle";

  /** Fired when three slaps land inside 1.7 s (→ dizzy + confused view). */
  onDizzy: (() => void) | null = null;

  private readonly bloub: BloubEngine;
  /**
   * Held on the resting face: its eyes stand in when the animated state shows none,
   * so the fixed body never reads as a blank blob (fixed.ts).
   */
  private readonly face: BloubEngine;
  /** The previous bloub state, to fade a glyph in only when coming from a body state. */
  private prevBloub: StateId = "idle";
  private clock = 0;
  private stateAt = 0;
  private oneShot: OneShot | null = null;
  private emote: Emote | null = null;
  private permanentExpression: ExpressionId | null = null;
  private permanentEmote: BotEmoteName | null = null;
  private squash: Squash | null = null;
  private slaps: number[] = [];
  private gulpAt = Number.NEGATIVE_INFINITY;
  private chewUntil = Number.NEGATIVE_INFINITY;
  private morphTween: { from: number; to: number; at: number; dur: number } | null = null;
  private arrivalAt = Number.NEGATIVE_INFINITY;
  private aiming = false;
  private lookKey = "";
  private lookChangedAt = Number.NEGATIVE_INFINITY;
  private faceChangedAt = Number.NEGATIVE_INFINITY;
  private miniLook = { x: 0, y: 0, next: 0 };
  private miniWinkAt = Number.POSITIVE_INFINITY;
  private shape: ShapeId;
  private expression: ExpressionId;

  constructor() {
    this.shape = appearance.shape;
    this.expression = appearance.expression;
    this.bloub = new BloubEngine(UNIT, "idle", SHAPE_BY_ID.get(this.shape)?.radii ?? null,
      EXPRESSION_BY_ID.get(this.expression) ?? null);
    this.face = new BloubEngine(UNIT, "idle", SHAPE_BY_ID.get(this.shape)?.radii ?? null,
      EXPRESSION_BY_ID.get(this.expression) ?? null);
  }

  // ── Preferences (every character) ───────────────────────────────────────────

  /** Eye motion of every character (already resolved for reduced motion). */
  static setEyeMotion(mode: EyeMotion) {
    prefs.eyeMotion = mode === "calm" || mode === "still" ? mode : "normal";
  }

  /** Shape, colour and rest expression; status animations keep their meaning. */
  static setAppearance(setting: unknown) { appearance = normalizeAppearance(setting); }

  /** Whether celebrations show, and whether only their face may (reduced motion). */
  static setCelebrations(on: boolean, reducedMotion: boolean) {
    prefs.celebrations = on;
    prefs.reducedMotion = reducedMotion;
  }

  // ── Read-outs ───────────────────────────────────────────────────────────────

  /** The bloub state on screen. */
  get bloubState(): StateId { return this.bloub.state; }

  /** The rest expression currently asked of bloub. */
  get currentExpression(): ExpressionId { return this.expression; }

  /** The body shape currently asked of bloub (the chosen one, or round / box for a moment). */
  get currentShape(): ShapeId { return this.shape; }

  /** Seconds of character time (advances only with `update`). */
  get time(): number { return this.clock; }

  /** True while anything is still moving — lets the island stop its frame loop. */
  get busy(): boolean {
    const n = this.clock;
    if (this.isMini) return true;
    const choreo = STATE_CHOREOGRAPHY[this.state];
    const reduced = prefs.reducedMotion;
    const elapsed = n - this.stateAt;
    const pending = !reduced && (
      choreo.intro.reduce((s, b) => s + b.duration, 0) > elapsed ||
      choreo.loop.length > 1);
    const def = STATE_BY_ID.get(this.bloub.state);
    const local = n - this.bloub.stateSince;
    // Reduced motion: looping states hold a still frame (see `frame()`), so they settle too.
    const settle = reduced && !Number.isFinite(SETTLE_AT[this.bloub.state]) ? 0 : SETTLE_AT[this.bloub.state];
    const settling = local < (def?.morph ?? 0) + Math.max(0.2, settle);
    return pending || settling ||
      (this.oneShot !== null && n < this.oneShot.until) ||
      (this.emote !== null && n < this.emote.until) ||
      this.squash !== null || this.morphTween !== null || n < this.arrivalAt + TOUR_TIME ||
      n - this.lookChangedAt < BloubEngine.LOOK_MORPH * 2 ||
      n - this.faceChangedAt < BloubEngine.SHAPE_MORPH ||
      n < this.chewUntil || n - this.gulpAt < 0.5 ||
      Math.abs(this.slotH - this.slotHTarget) > 0.001 || Math.abs(this.slotHVel) > 0.001 ||
      Math.abs(this.tgEs - this.es) > 0.002;
  }

  // ── App events ──────────────────────────────────────────────────────────────

  setState(next: BotStateName, force = false) {
    if (!Object.prototype.hasOwnProperty.call(STATE_CHOREOGRAPHY, next)) return;
    if (this.state === next && !force) return;
    this.state = next;
    this.stateAt = this.clock;
    // A state change ends a pending one-shot unless it is a celebration of this state.
    if (this.oneShot && next !== "idle") this.oneShot = null;
    if (next === "finished") this.celebrate("finished");
    this.resolve();
  }

  /**
   * A short "well done": a swirl of rings and a happy face. Off with the
   * Celebrations setting; with reduced motion only the face changes. Mini
   * characters never celebrate — one character cheering is enough.
   */
  celebrate(kind: CelebrationKind) {
    if (!prefs.celebrations || this.isMini) return;
    const c = CELEBRATION_CHOREOGRAPHY[kind];
    // No state of its own ("finished"): the app state's choreography already smiles.
    if (!c?.state) return;
    this.emote = { expression: c.expression, until: this.clock + c.seconds };
    if (!prefs.reducedMotion) {
      this.oneShot = { state: c.state, expression: c.expression, until: this.clock + Math.max(c.seconds, minDurationOf(c.state)) };
    }
    this.resolve();
  }

  /** A temporary emote: its expression on the rest face, sometimes a bloub state. */
  triggerEmote(emote: BotEmoteName, duration = 1.8) {
    const c = EMOTE_CHOREOGRAPHY[emote];
    if (!c) return;
    const seconds = Number.isFinite(duration) && duration > 0 ? duration : 1.8;
    this.emote = { expression: c.expression, until: this.clock + seconds };
    if (c.state && !prefs.reducedMotion) {
      this.oneShot = { state: c.state, expression: c.expression, until: this.clock + Math.max(seconds, minDurationOf(c.state)) };
    }
    if (emote === "annoyed") window.setTimeout(() => Sound.play("annoyed"), 60);
    this.resolve();
  }

  /** Mini characters keep an emote for good (an agent's mood). */
  setPermanentEmote(emote: BotEmoteName | null) {
    this.permanentEmote = emote;
    this.permanentExpression = emote ? EMOTE_CHOREOGRAPHY[emote]?.expression ?? null : null;
    this.miniWinkAt = emote === "wink" ? this.clock + 0.8 + Math.random() * 1.7 : Number.POSITIVE_INFINITY;
  }

  /** Legacy eye name on a mini character → its bloub expression, for good. */
  setPermanentEye(eye: EyeShape | null) {
    this.permanentExpression = eye ? EYE_EXPRESSION[eye] ?? null : null;
  }

  blink() {
    this.bloub.blink(this.clock);
    this.face.blink(this.clock);
  }

  /** A slap: squash and an angry face; three inside 1.7 s → `onDizzy`. */
  slap() {
    if (this.state === "dizzy") return;
    const n = this.clock;
    this.slaps = this.slaps.filter((s) => n - s < SLAP_WINDOW);
    this.slaps.push(n);
    Sound.play("slap");
    this.squash = { kind: "slap", at: n };
    if (this.slaps.length >= 3) {
      this.slaps = [];
      this.onDizzy?.();
    } else {
      this.emote = { expression: "colere", until: n + 0.8 };
      window.setTimeout(() => Sound.play("annoyed"), 60);
    }
    this.resolve();
  }

  /** Mailbox swallow — opens the slot, chews, then closes. */
  gulp() {
    this.slotHTarget = 0.42;
    this.gulpAt = this.clock;
    this.squash = { kind: "gulp", at: this.clock };
    this.blink();
  }

  animateMorph(target: number, durationMs?: number) {
    const to = clamp(target);
    const dur = (durationMs ?? (to > 0.5 ? 550 : 650)) / 1000;
    this.morphTween = { from: this.morph, to, at: this.clock, dur: Math.max(0.01, dur) };
  }

  resetMorph() {
    this.morphTween = null;
    this.morph = 0;
  }

  /**
   * The arrival (bloub's intro): the eyes travel a full turn round the ball, which is
   * round for the duration and morphs back to the chosen shape as it lands. Only on
   * the resting face, never under reduced motion, never on mini characters.
   */
  arrive() {
    if (prefs.reducedMotion || this.isMini || this.state !== "idle") return;
    this.arrivalAt = this.clock;
    this.setLook(tourLook(0), this.clock - SCRIPT_MORPH, SCRIPT_MORPH);
    this.resolve();
  }

  /** Plays one bloub state for `seconds` (settings preview strip), then back to the app state. */
  playState(id: StateId, seconds?: number) {
    if (!STATE_BY_ID.has(id)) return;
    const def = STATE_BY_ID.get(id)!;
    const hold = seconds ?? Math.max(def.duration, minDurationOf(id));
    this.oneShot = { state: id, expression: null, until: this.clock + hold };
    this.emote = null;
    this.resolve();
  }

  // ── Frame ───────────────────────────────────────────────────────────────────

  update(dt: number) {
    const step = Number.isFinite(dt) ? clamp(dt, 0, 0.064) : 0;
    this.clock += step;
    const n = this.clock;

    if (this.morphTween) {
      const tw = this.morphTween;
      const p = clamp((n - tw.at) / tw.dur);
      this.morph = tw.from + (tw.to - tw.from) * easings.easeInOutCubic(p);
      if (p >= 1) this.morphTween = null;
    }
    if (n - this.gulpAt >= 0.46 && n - this.gulpAt < 0.46 + step + 1e-9) {
      this.slotHTarget = 0;
      this.chewUntil = n + 0.8;
    }
    this.isChewing = n < this.chewUntil;
    // Slot spring — ω₀ = 2π/0.25, ζ = 0.6.
    const omega = (2 * Math.PI) / 0.25;
    const acc = omega * omega * (this.slotHTarget - this.slotH) - 2 * 0.6 * omega * this.slotHVel;
    this.slotHVel += acc * step;
    this.slotH = Math.max(0, this.slotH + this.slotHVel * step);
    if (Math.abs(this.slotH - this.slotHTarget) < 0.0005 && Math.abs(this.slotHVel) < 0.0005) {
      this.slotH = this.slotHTarget;
      this.slotHVel = 0;
    }
    this.es += (this.tgEs - this.es) * (1 - Math.pow(0.0008, step));
    if (Math.abs(this.tgEs - this.es) < 0.002) this.es = this.tgEs;
    if (this.squash && n - this.squash.at > 0.5) this.squash = null;
    if (this.oneShot && n >= this.oneShot.until) this.oneShot = null;
    if (this.emote && n >= this.emote.until) this.emote = null;
    if (this.permanentEmote === "wink" && n >= this.miniWinkAt) {
      this.oneShot = { state: "wink", expression: null, until: n + 0.9 };
      this.miniWinkAt = n + 2.2 + Math.random() * 2;
    }
    this.resolve();
  }

  /** Draws the character into a `w`×`h` CSS-pixel canvas (DPR already applied). */
  draw(x: CanvasRenderingContext2D, W: number, H: number) {
    const frame = this.frame();
    const radius = W * 0.3;
    const [sx, sy] = squashAt(this.squash, this.clock);
    paintFrame(x, frame, W / 2, H / 2 + this.particleOverhang / 2, radius, {
      ink: this.bodyColor ? rgbHex(this.bodyColor) : colorHex(appearance.color),
      paper: this.paper,
      backdrop: this.paper ?? "#000000",
      eyeScale: this.es * (this.isMini ? 1.3 : 1),
      sx, sy,
      slot: { open: this.slotH / 0.42, morph: this.morph },
    });
  }

  /** The bloub frame at the current time (pure read). */
  frame(): CharacterFrame {
    const n = this.clock;
    const state = this.bloub.state;
    const radii = SHAPE_BY_ID.get(this.shape)?.radii ?? SHAPE_BY_ID.get("cercle")!.radii;
    const morph = STATE_BY_ID.get(state)?.morph ?? 0.3;
    const glyphIn = GLYPH_STATES.has(this.prevBloub) ? 1 : easings.easeOutCubic(clamp((n - this.bloub.stateSince) / morph));
    // Reduced motion: a looping state (the dots, the sleep bounce, the "!" buzz) is
    // pinned to the frame just after its fade, so the frame loop can stop.
    const pinned = prefs.reducedMotion && (GLYPH_STATES.has(state) || !Number.isFinite(SETTLE_AT[state]))
      ? Math.min(n, this.bloub.stateSince + morph + 0.05) : n;
    return composeFixed(this.bloub.sample(pinned), this.face.sample(n), state, radii, glyphIn);
  }

  // ── Resolution: app state → bloub state, shape, expression, gaze ────────────

  private resolve() {
    const n = this.clock;
    const reduced = prefs.reducedMotion;
    const mailbox = this.morph > 0.05;
    const choreo = STATE_CHOREOGRAPHY[this.state];
    const oneShot = this.oneShot && n < this.oneShot.until ? this.oneShot : null;
    const emote = this.emote && n < this.emote.until ? this.emote : null;
    const arriving = n < this.arrivalAt + TOUR_TIME;

    let state: StateId;
    let stateExpression: ExpressionId | null = null;
    if (mailbox || arriving) state = "idle";
    else if (oneShot) state = oneShot.state;
    else if (emote) state = "idle";
    else {
      const at = choreographyAt(choreo, n - this.stateAt, reduced);
      state = at.state;
      stateExpression = choreo.expression ?? null;
    }
    if (state !== this.bloub.state) this.prevBloub = this.bloub.state;
    this.bloub.setState(state, n);

    const expression: ExpressionId =
      (mailbox ? (this.isChewing ? "heureux" : this.slotH > 0.08 ? "surpris" : null) : null) ??
      oneShot?.expression ?? emote?.expression ?? this.permanentExpression ?? stateExpression ?? appearance.expression;
    // The body is always the shape chosen in Settings (fixed.ts): no state, arrival or
    // mailbox changes it.
    const shape: ShapeId = appearance.shape;
    if (expression !== this.expression || shape !== this.shape) this.faceChangedAt = n;
    if (expression !== this.expression) {
      this.expression = expression;
      this.bloub.setExpression(EXPRESSION_BY_ID.get(expression) ?? null, n);
      this.face.setExpression(EXPRESSION_BY_ID.get(expression) ?? null, n);
    }
    if (shape !== this.shape) {
      this.shape = shape;
      this.bloub.setShape(SHAPE_BY_ID.get(shape)?.radii ?? null, n);
      this.face.setShape(SHAPE_BY_ID.get(shape)?.radii ?? null, n);
    }

    this.bloub.setLife(lifeFor(prefs.eyeMotion));
    this.face.setLife(lifeFor(prefs.eyeMotion));
    this.aim(state, arriving);
  }

  private setLook(look: Look | null, now: number, morph?: number) {
    this.bloub.setLook(look, now, morph);
    this.face.setLook(look, now, morph);
  }

  private aim(state: StateId, arriving: boolean) {
    const n = this.clock;
    if (arriving) {
      this.setLook(tourLook(n - this.arrivalAt), n, SCRIPT_MORPH);
      this.aiming = true;
      return;
    }
    let look: Look | null = null;
    if (this.isMini) {
      if (prefs.eyeMotion === "normal") {
        if (n >= this.miniLook.next) {
          this.miniLook = { x: Math.random() * 2 - 1, y: Math.random() * 2 - 1, next: n + 0.5 + Math.random() * 1.5 };
        }
        look = { yaw: this.miniLook.x * 22, pitch: PITCH_REST + this.miniLook.y * 12, mix: 0.85, spin: 0, wander: 0.3 };
      }
    } else {
      const gain = lookGain(prefs.eyeMotion);
      if (gain > 0) {
        const lx = clamp(Number.isFinite(this.lookX) ? this.lookX : 0, -1, 1);
        const ly = clamp(Number.isFinite(this.lookY) ? this.lookY : 0, -1, 1);
        look = { yaw: lx * YAW_RANGE, pitch: PITCH_REST + ly * PITCH_RANGE, mix: gain, spin: 0, wander: 1 - gain * 0.6 };
      }
    }
    // The resting face (which stands in for eye-less states) always follows. The
    // animated engine only on its resting-face states: elsewhere the gaze IS the animation.
    const steer = STATE_BY_ID.get(state)?.baseFace === true;
    const key = look ? `${look.yaw.toFixed(2)}|${look.pitch.toFixed(2)}|${look.mix}|${steer}` : "";
    if (key === this.lookKey) return;
    this.lookKey = key;
    this.lookChangedAt = n;
    const release = TOUR_TIME / 2;
    this.face.setLook(look, n, look ? undefined : release);
    if (look && steer) { this.bloub.setLook(look, n); this.aiming = true; }
    else if (this.aiming) { this.bloub.setLook(null, n, release); this.aiming = false; }
  }
}
