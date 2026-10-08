import { describe, expect, it } from "vitest";
import {
  CATALOGUE, CELEBRATION_CHOREOGRAPHY, EMOTE_CHOREOGRAPHY, SETTLE_AT, STATE_CHOREOGRAPHY, choreographyAt,
} from "./motion";
import { minDurationOf } from "./bloub/cycles";
import { EXPRESSION_BY_ID } from "./bloub/expressions";
import { STATES, STATE_BY_ID, type StateId } from "./bloub/states";

const choreographies = Object.entries(STATE_CHOREOGRAPHY);

describe("state choreography", () => {
  it("covers every app state with real bloub states and expressions", () => {
    expect(Object.keys(STATE_CHOREOGRAPHY).sort()).toEqual(
      ["approval", "dizzy", "error", "finished", "idle", "question", "ratelimit", "searching", "sleeping", "thinking", "working"]);
    for (const [, c] of choreographies) {
      expect(c.loop.length).toBeGreaterThan(0);
      for (const b of [...c.intro, ...c.loop]) expect(STATE_BY_ID.has(b.state)).toBe(true);
      expect(STATE_BY_ID.has(c.calm)).toBe(true);
      if (c.expression) expect(EXPRESSION_BY_ID.has(c.expression)).toBe(true);
    }
    for (const e of Object.values(EMOTE_CHOREOGRAPHY)) {
      expect(EXPRESSION_BY_ID.has(e.expression)).toBe(true);
      if (e.state) expect(STATE_BY_ID.has(e.state)).toBe(true);
    }
  });

  it("uses all 14 catalogue states and the swirl transition", () => {
    const used = new Set<StateId>();
    for (const [, c] of choreographies) for (const b of [...c.intro, ...c.loop]) used.add(b.state);
    for (const e of Object.values(EMOTE_CHOREOGRAPHY)) if (e.state) used.add(e.state);
    for (const c of Object.values(CELEBRATION_CHOREOGRAPHY)) if (c.state) used.add(c.state);
    expect(CATALOGUE).toHaveLength(14);
    for (const id of [...CATALOGUE, "swirl" as const]) expect(used.has(id), id).toBe(true);
  });

  it("never cuts a narrative state before it resolves, and alternates loops", () => {
    for (const [name, c] of choreographies) {
      for (const b of [...c.intro, ...c.loop]) expect(b.duration, `${name}/${b.state}`).toBeGreaterThanOrEqual(minDurationOf(b.state));
      // bloub only fades between DIFFERENT states: a repeated state would not restart.
      if (c.loop.length > 1) {
        c.loop.forEach((b, i) => expect(b.state, name).not.toBe(c.loop[(i + 1) % c.loop.length].state));
        expect(c.loop.every((b) => Number.isFinite(b.duration)), name).toBe(true);
      }
      const last = c.intro[c.intro.length - 1];
      if (last) expect(last.state).not.toBe(c.loop[0].state);
    }
  });

  it("walks intro then loop, wraps, and falls back to the calm state", () => {
    const thinking = STATE_CHOREOGRAPHY.thinking;
    const [orbit, egg] = thinking.loop;
    expect(choreographyAt(thinking, 0).state).toBe("orbit");
    expect(choreographyAt(thinking, orbit.duration + 0.1)).toMatchObject({ state: "egg", index: 1 });
    expect(choreographyAt(thinking, orbit.duration + egg.duration + 0.1)).toMatchObject({ state: "orbit", index: 2 });
    const working = STATE_CHOREOGRAPHY.working;
    expect(choreographyAt(working, 0.1).state).toBe("play");
    expect(choreographyAt(working, 999).state).toBe("thinking");
    expect(choreographyAt(working, 999, true).state).toBe("thinking");
    expect(choreographyAt(STATE_CHOREOGRAPHY.approval, 1, true).state).toBe("exclaim");
    for (const bad of [Number.NaN, -5, Number.POSITIVE_INFINITY]) expect(STATE_BY_ID.has(choreographyAt(thinking, bad).state)).toBe(true);
  });
});

describe("settle times", () => {
  /** A pose without its loop-only rotation: on a round body rotation is invisible. */
  const still = (id: StateId, t: number) => {
    const pose = STATE_BY_ID.get(id)!.pose(t);
    const round = pose.sil.radii.every((r) => Math.abs(r - pose.sil.radii[0]) < 1e-9);
    // Invisible decor is dropped by the engine too (`opacity > 0.01`).
    const arcs = pose.arcs.filter((a) => a.opacity > 0.01);
    const dots = pose.dots.filter((d) => d.opacity > 0.01 && d.r > 0.0005);
    return JSON.stringify({ ...pose, arcs, dots, sil: { ...pose.sil, rot: round ? 0 : pose.sil.rot } });
  };

  it("is given for every bloub state", () => {
    for (const s of STATES) expect(SETTLE_AT[s.id], s.id).toBeGreaterThanOrEqual(0);
  });

  it("marks the moment each one-shot state stops moving", () => {
    for (const s of STATES) {
      const at = SETTLE_AT[s.id];
      if (!Number.isFinite(at)) continue;
      expect(still(s.id, at + 0.05), s.id).toBe(still(s.id, at + 7.3));
    }
  });

  it("keeps the looping states looping", () => {
    for (const id of ["thinking", "sleep", "alert"] as const) expect(still(id, 10)).not.toBe(still(id, 10.3));
  });
});
