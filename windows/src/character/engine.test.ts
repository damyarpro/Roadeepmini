import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../core/sound", () => ({ Sound: { play: vi.fn() } }));

import { BotEngine, effectiveEyeMotion, lifeFor, lookGain, type EyeMotion } from "./engine";
import { DEFAULT_APPEARANCE } from "./appearance";
import { CATALOGUE, STATE_CHOREOGRAPHY } from "./motion";
import { GLYPH_STATES, fixedBodyPath } from "./fixed";
import { SHAPE_BY_ID } from "./bloub/skins";
import { TOUR_TIME } from "./bloub/gaze";
import type { BotStateName } from "../core/layout";

/** Advances the character by `seconds` in 1/60 s frames, like the island's loop. */
function run(e: BotEngine, seconds: number) {
  for (let t = 0; t < seconds; t += 1 / 60) e.update(1 / 60);
}

/** A 2D context that only checks every number it is given is finite. */
function finiteContext() {
  const finite = (...args: unknown[]) => { for (const a of args) if (typeof a === "number") expect(Number.isFinite(a)).toBe(true); };
  const gradient = { addColorStop: () => {} };
  return new Proxy({}, {
    get: (_t, p) => String(p).startsWith("create") ? () => gradient : finite,
    set: () => true,
  }) as CanvasRenderingContext2D;
}

beforeEach(() => {
  vi.stubGlobal("Path2D", class { constructor(public d?: string) {} });
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  BotEngine.setEyeMotion("normal");
  BotEngine.setCelebrations(true, false);
  BotEngine.setAppearance(DEFAULT_APPEARANCE);
});

describe("app states drive bloub", () => {
  const cases: [BotStateName, string, string][] = [
    // [app state, first bloub state, bloub state once the intro is over]
    ["idle", "idle", "idle"],
    ["working", "play", "thinking"],
    ["thinking", "orbit", "egg"],
    ["searching", "comet", "wide"],
    ["approval", "alert", "exclaim"],
    ["question", "notify", "notify"],
    ["error", "burst", "idle"],
    ["finished", "wink", "idle"],
    ["ratelimit", "hexagon", "hexagon"],
    ["sleeping", "sleep", "sleep"],
    ["dizzy", "burst", "idle"],
  ];
  for (const [state, first, later] of cases) {
    it(`${state} → ${first} → ${later}`, () => {
      const e = new BotEngine();
      e.setState(state);
      e.update(0.016);
      expect(e.bloubState).toBe(first);
      const c = STATE_CHOREOGRAPHY[state];
      const intro = c.intro.reduce((s, b) => s + b.duration, 0);
      const firstLoop = Number.isFinite(c.loop[0].duration) ? c.loop[0].duration : 0;
      run(e, intro + firstLoop + 0.1);
      expect(e.bloubState).toBe(later);
    });
  }

  it("shows each state's own expression, and the chosen one at rest", () => {
    BotEngine.setAppearance({ shape: "nuage", color: "rose", expression: "curieux" });
    const e = new BotEngine();
    e.update(0.016);
    expect(e.currentExpression).toBe("curieux");
    e.setState("error");
    run(e, 3);
    expect(e.bloubState).toBe("idle");
    expect(e.currentExpression).toBe("triste");
    e.setState("idle");
    e.update(0.016);
    expect(e.currentExpression).toBe("curieux");
  });

  it("plays only the still state under reduced motion", () => {
    BotEngine.setCelebrations(true, true);
    const e = new BotEngine();
    e.setState("approval");
    run(e, 6);
    expect(e.bloubState).toBe("exclaim");
    expect(e.busy).toBe(false);
  });

  it("lets the frame loop sleep once a resting state settles, and keeps it for loops", () => {
    const e = new BotEngine();
    run(e, 1);
    expect(e.busy).toBe(false);
    e.setState("sleeping");
    run(e, 5);
    expect(e.busy).toBe(true);
    e.setState("question");
    run(e, 2);
    expect(e.busy).toBe(false);
    e.setState("thinking");
    run(e, 8);
    expect(e.busy).toBe(true);
  });

  it("freezes between updates: time only moves with the island's frames", () => {
    const e = new BotEngine();
    run(e, 0.5);
    const before = e.frame();
    expect(e.frame()).toEqual(before);
    e.update(Number.NaN);
    e.update(-1);
    expect(e.frame()).toEqual(before);
    e.update(10); // a long pause is capped like bloub's clock
    expect(e.time).toBeLessThan(0.6);
  });
});

describe("emotes, celebrations and touch", () => {
  it("an emote turns the face and wears off", () => {
    const e = new BotEngine();
    e.setState("working");
    run(e, 3);
    expect(e.bloubState).toBe("thinking");
    e.triggerEmote("happy", 1);
    e.update(0.016);
    expect(e.bloubState).toBe("idle");
    expect(e.currentExpression).toBe("hilare");
    run(e, 1.1);
    expect(e.bloubState).toBe("thinking");
  });

  it("wink and surprise also play their bloub state", () => {
    const e = new BotEngine();
    e.triggerEmote("wink");
    e.update(0.016);
    expect(e.bloubState).toBe("wink");
    e.triggerEmote("surprised");
    e.update(0.016);
    expect(e.bloubState).toBe("wide");
  });

  it("celebrates with a swirl, not on mini characters and not when switched off", () => {
    const e = new BotEngine();
    e.celebrate("task");
    e.update(0.016);
    expect(e.bloubState).toBe("swirl");
    expect(e.busy).toBe(true);

    BotEngine.setCelebrations(false, false);
    const off = new BotEngine();
    off.celebrate("focus");
    off.update(0.016);
    expect(off.bloubState).toBe("idle");

    BotEngine.setCelebrations(true, false);
    const mini = new BotEngine();
    mini.isMini = true;
    mini.celebrate("task");
    mini.update(0.016);
    expect(mini.bloubState).toBe("idle");
  });

  it("keeps only the smile under reduced motion", () => {
    BotEngine.setCelebrations(true, true);
    const e = new BotEngine();
    e.celebrate("focus");
    e.update(0.016);
    expect(e.bloubState).toBe("idle");
    expect(e.currentExpression).toBe("hilare");
  });

  it("three quick slaps make it dizzy", () => {
    const e = new BotEngine();
    const dizzy = vi.fn();
    e.onDizzy = dizzy;
    e.slap(); run(e, 0.3);
    expect(e.currentExpression).toBe("colere");
    e.slap(); run(e, 0.3);
    e.slap();
    expect(dizzy).toHaveBeenCalledOnce();
  });

  it("opens a mailbox slot while a file hovers, keeping the chosen shape", () => {
    BotEngine.setAppearance({ shape: "goutte", color: "bleu", expression: "neutre" });
    const e = new BotEngine();
    e.setState("working");
    e.animateMorph(1);
    run(e, 0.8);
    expect(e.morph).toBe(1);
    expect(e.bloubState).toBe("idle");
    expect(e.currentShape).toBe("goutte");
    e.gulp();
    run(e, 0.6);
    expect(e.isChewing).toBe(true);
    e.animateMorph(0);
    run(e, 1.5);
    expect(e.morph).toBe(0);
    expect(e.currentShape).toBe("goutte");
    expect(e.bloubState).toBe("thinking");
  });
});

describe("arrival and gaze", () => {
  it("arrives with a turn of the eyes, in the chosen shape throughout", () => {
    BotEngine.setAppearance({ shape: "triangle", color: "creme", expression: "neutre" });
    const e = new BotEngine();
    e.update(0.016);
    e.arrive();
    e.update(0.016);
    expect(e.currentShape).toBe("triangle");
    expect(e.busy).toBe(true);
    run(e, TOUR_TIME + 0.1);
    expect(e.currentShape).toBe("triangle");
  });

  it("does not arrive under reduced motion, on minis, or away from rest", () => {
    BotEngine.setCelebrations(true, true);
    const reduced = new BotEngine();
    run(reduced, 1);
    reduced.arrive();
    reduced.update(0.016);
    expect(reduced.busy).toBe(false);
    BotEngine.setCelebrations(true, false);
    const working = new BotEngine();
    working.setState("approval");
    working.arrive();
    working.update(0.016);
    expect(working.bloubState).toBe("alert");
  });

  const eyeX = (e: BotEngine) => e.frame().eyes.reduce((s, eye) => s + eye.m[4], 0);
  for (const [mode, follows] of [["normal", true], ["calm", true], ["still", false]] as [EyeMotion, boolean][]) {
    it(`${mode}: the eyes ${follows ? "follow" : "ignore"} the cursor`, () => {
      BotEngine.setEyeMotion(mode);
      const left = new BotEngine();
      const right = new BotEngine();
      left.lookX = -1;
      right.lookX = 1;
      run(left, 1);
      run(right, 1);
      const gap = eyeX(right) - eyeX(left);
      if (follows) expect(gap).toBeGreaterThan(5);
      else expect(Math.abs(gap)).toBeLessThan(0.01);
    });
  }

  it("maps the eye-motion setting onto follow range, drift and blinks", () => {
    expect([lookGain("normal"), lookGain("calm"), lookGain("still")]).toEqual([1, 0.5, 0]);
    expect(lifeFor("still")).toEqual({ wander: 0, blinkRate: 0.5 });
    expect(lifeFor("calm").blinkRate).toBeLessThan(lifeFor("normal").blinkRate);
    expect(effectiveEyeMotion("normal", true)).toBe("calm");
    expect(effectiveEyeMotion("normal", false)).toBe("normal");
    expect(effectiveEyeMotion("still", true)).toBe("still");
  });

  it("ignores a non-finite cursor", () => {
    const e = new BotEngine();
    e.lookX = Number.NaN;
    e.lookY = Number.POSITIVE_INFINITY;
    run(e, 0.5);
    for (const eye of e.frame().eyes) for (const v of eye.m) expect(Number.isFinite(v)).toBe(true);
  });
});

describe("drawing", () => {
  it("paints every state, shape and size with finite coordinates", () => {
    const ctx = finiteContext();
    for (const shape of ["cercle", "nuage", "goutte"] as const) {
      BotEngine.setAppearance({ shape, color: "encre", expression: "confus" });
      for (const state of Object.keys(STATE_CHOREOGRAPHY) as BotStateName[]) {
        const e = new BotEngine();
        e.setState(state);
        for (let i = 0; i < 6; i++) {
          run(e, 0.4);
          e.draw(ctx, 40, 80);
          e.draw(ctx, 200, 240);
        }
      }
    }
    const mini = new BotEngine();
    mini.isMini = true;
    mini.bodyColor = [0.2, 0.4, 0.9];
    mini.setPermanentEmote("wink");
    run(mini, 3);
    mini.draw(ctx, 22, 22);
  });
});

describe("the body is always the shape chosen in Settings", () => {
  const body = (shape: "cercle" | "nuage" | "goutte" | "triangle") => fixedBodyPath(SHAPE_BY_ID.get(shape)!.radii);

  for (const shape of ["nuage", "goutte", "triangle"] as const) {
    it(`${shape}: every app state, emote, celebration, slap, arrival and mailbox`, () => {
      BotEngine.setAppearance({ shape, color: "violet", expression: "neutre" });
      const check = (e: BotEngine, eyes = true) => {
        const f = e.frame();
        expect(f.bodyPath).toBe(body(shape));
        // Never a blank blob: some eye is always visible (except mid-arrival, when the
        // eyes travel round the back of the ball — that turn is the animation).
        if (eyes) expect(f.eyes.reduce((m, eye) => Math.max(m, eye.alpha), 0), e.bloubState).toBeGreaterThan(0.3);
      };
      for (const state of Object.keys(STATE_CHOREOGRAPHY) as BotStateName[]) {
        const e = new BotEngine();
        e.setState(state);
        for (let i = 0; i < 40; i++) { e.update(0.1); check(e); }
      }
      const e = new BotEngine();
      const acts: [(e: BotEngine) => void, boolean][] = [
        [(x) => x.triggerEmote("wink"), true], [(x) => x.triggerEmote("surprised"), true], [(x) => x.celebrate("task"), false],
        [(x) => x.slap(), true], [(x) => x.arrive(), false], [(x) => { x.animateMorph(1); x.gulp(); }, true], [(x) => x.animateMorph(0), true],
      ];
      for (const [act, eyes] of acts) { act(e); for (let i = 0; i < 20; i++) { e.update(0.1); check(e, eyes); } }
    });
  }

  it("holds for all 14 catalogue states and swirl, the glyph states drawing their glyph beside the head", () => {
    BotEngine.setAppearance({ shape: "capsule", color: "creme", expression: "neutre" });
    for (const id of [...CATALOGUE, "swirl" as const]) {
      const e = new BotEngine();
      e.playState(id, 30);
      for (let i = 0; i < 25; i++) {
        e.update(0.1);
        const f = e.frame();
        expect(e.bloubState).toBe(id);
        expect(f.bodyPath).toBe(fixedBodyPath(SHAPE_BY_ID.get("capsule")!.radii));
        expect(f.eyes.length, id).toBeGreaterThan(0);
        expect(f.glyph !== null, id).toBe(GLYPH_STATES.has(id));
      }
    }
  });
});

describe("reduced motion lets every state settle", () => {
  for (const state of ["working", "thinking", "searching", "approval", "sleeping"] as BotStateName[]) {
    it(`${state} holds a still frame and stops the frame loop`, () => {
      BotEngine.setCelebrations(true, true);
      const e = new BotEngine();
      e.setState(state);
      run(e, 2);
      expect(e.busy).toBe(false);
      const a = e.frame();
      run(e, 1.3);
      const b = e.frame();
      expect(b.bodyPath).toBe(a.bodyPath);
      expect(JSON.stringify(b.glyph)).toBe(JSON.stringify(a.glyph));
      expect(JSON.stringify(b.arcs)).toBe(JSON.stringify(a.arcs));
    });
  }
});

describe("ink colour on the black island", () => {
  it("outlines the body and the side glyph faintly", () => {
    BotEngine.setAppearance({ shape: "cercle", color: "encre", expression: "neutre" });
    const strokes: string[] = [];
    const ctx = new Proxy({} as Record<string, unknown>, {
      get: (t, p) => p === "stroke" ? () => strokes.push(String(t.strokeStyle)) : String(p).startsWith("create") ? () => ({ addColorStop() {} }) : t[p as string] ?? (() => {}),
      set: (t, p, v) => { t[p as string] = v; return true; },
    }) as unknown as CanvasRenderingContext2D;
    const e = new BotEngine();
    e.setState("approval");
    run(e, 1);
    e.draw(ctx, 60, 100);
    expect(e.frame().glyph).not.toBeNull();
    expect(strokes.filter((s) => s === "rgba(255,255,255,0.22)").length).toBeGreaterThanOrEqual(2);
  });
});
