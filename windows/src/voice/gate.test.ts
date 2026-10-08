import { describe, expect, it, vi } from "vitest";
import { GATE_DEFAULTS, GateKernel, buildGate, gateStep, initialGate, levelOf, rmsOf, toDb, type GateState } from "./gate";

const amp = (db: number) => 10 ** (db / 20);
/** Feeds `ms` of a constant level in 10 ms blocks. */
function feed(state: GateState, db: number, ms: number, t0: number): { state: GateState; t: number; openMs: number } {
  let t = t0; let openMs = 0;
  for (let i = 0; i < ms / 10; i++) { t += 10; state = gateStep(state, amp(db), 10, t); if (state.open) openMs += 10; }
  return { state, t, openMs };
}

describe("gate math", () => {
  it("converts levels", () => {
    expect(toDb(1)).toBe(0); expect(toDb(0)).toBe(-100); expect(toDb(0.1)).toBeCloseTo(-20);
    expect(rmsOf(new Float32Array([0.5, -0.5]))).toBeCloseTo(0.5);
    expect(levelOf(0)).toBe(0); expect(levelOf(1)).toBe(1); expect(levelOf(amp(-35))).toBeCloseTo(0.5);
  });

  it("learns the noise floor and stays closed on steady noise", () => {
    const r = feed(initialGate(), -55, 3000, 0);
    expect(r.state.floorDb).toBeCloseTo(-55, 0); expect(r.state.open).toBe(false); expect(r.openMs).toBe(0);
  });

  it("opens fast for near speech and releases after the hold", () => {
    let r = feed(initialGate(), -60, 2000, 0);
    r = feed(r.state, -20, 30, r.t);
    expect(r.state.open).toBe(true); expect(r.state.gain).toBeGreaterThan(0.9); // ~10 ms attack
    r = feed(r.state, -60, 200, r.t); expect(r.state.open).toBe(true);           // hold
    r = feed(r.state, -60, 100, r.t); expect(r.state.open).toBe(false);          // ~250 ms hold over
    r = feed(r.state, -60, 300, r.t); expect(r.state.gain).toBeLessThan(0.05);
  });

  it("keeps a distant voice out while the near speaker is recent, then lets it in once the peak decays", () => {
    let r = feed(initialGate(), -65, 2000, 0);
    r = feed(r.state, -18, 1000, r.t);                 // near talker
    r = feed(r.state, -65, 300, r.t);
    const far = feed(r.state, -40, 500, r.t);           // 22 dB below the near talker, well above floor
    expect(far.openMs).toBe(0);
    // Long silence from the near talker: the remembered peak sinks and the far voice is let in.
    r = feed(far.state, -65, 15_000, far.t);
    expect(feed(r.state, -40, 200, r.t).state.open).toBe(true);
  });

  it("does not learn a talker as noise during continuous speech", () => {
    let r = feed(initialGate(), -60, 2000, 0);
    r = feed(r.state, -25, 8000, r.t);
    expect(r.state.open).toBe(true); expect(r.state.floorDb).toBeLessThan(-45);
  });

  it("does not let a short loud transient raise the speech peak", () => {
    let r = feed(initialGate(), -65, 2000, 0);
    r = feed(r.state, -30, 1000, r.t);                  // the user, at -30
    const peak = r.state.peakDb;
    r = feed(r.state, -5, 80, r.t);                     // a door slam, 80 ms
    expect(r.state.peakDb).toBeLessThanOrEqual(peak);
    r = feed(r.state, -65, 300, r.t);
    expect(feed(r.state, -30, 100, r.t).state.open).toBe(true); // the user still gets through
    r = feed(r.state, -12, 400, r.t);                   // a louder talker for 400 ms does move it
    expect(r.state.peakDb).toBeGreaterThan(-15);
  });

  it("is robust to bad input", () => {
    const s = gateStep(initialGate(), Number.NaN, 10, 10);
    expect(Number.isFinite(s.floorDb)).toBe(true); expect(s.open).toBe(false);
    expect(gateStep(s, 1, -50, 20).gain).toBeGreaterThanOrEqual(0);
    expect(GATE_DEFAULTS.windowDb).toBe(12);
  });
});

describe("GateKernel", () => {
  it("passes speech with a ramped gain and silences noise", () => {
    const k = new GateKernel(); const out = new Float32Array(128);
    const block = (v: number) => new Float32Array(128).fill(v);
    for (let i = 0; i < 400; i++) k.process(block(amp(-60)), out, 128 / 48);
    expect(k.open).toBe(false); expect(Math.max(...out.map(Math.abs))).toBeLessThan(1e-3);
    for (let i = 0; i < 10; i++) k.process(block(0.1), out, 128 / 48);
    expect(k.open).toBe(true); expect(out[127]).toBeGreaterThan(0.09); expect(k.raw).toBeCloseTo(0.1);
    k.process(undefined, out, 128 / 48); expect(out.every(v => v === 0)).toBe(true);
  });
});

describe("buildGate", () => {
  it("wires mic → highpass → gain → destination, drives the gain and cleans up", () => {
    const node = () => ({ connect: vi.fn(), disconnect: vi.fn() });
    const track = { stop: vi.fn() };
    const destination = { ...node(), stream: { getAudioTracks: () => [track], getTracks: () => [track] } };
    const highpass = { ...node(), type: "", frequency: { value: 0 }, Q: { value: 0 } };
    let level = 0;
    const analyser = { ...node(), fftSize: 0, smoothingTimeConstant: 1, getFloatTimeDomainData: (b: Float32Array) => b.fill(level) };
    const gain = { ...node(), gain: { value: 1, setTargetAtTime: vi.fn() } };
    const source = node();
    const ctx = {
      currentTime: 1, createMediaStreamSource: () => source, createBiquadFilter: () => highpass, createAnalyser: () => analyser,
      createGain: () => gain, createMediaStreamDestination: () => destination,
    } as unknown as AudioContext;
    let tick = () => {}; let now = 0;
    const timers = { setInterval: vi.fn((fn: () => void) => { tick = fn; return 7; }), clearInterval: vi.fn(), now: () => now };
    const g = buildGate(ctx, {} as MediaStream, timers);
    expect(highpass.type).toBe("highpass"); expect(highpass.frequency.value).toBe(90); expect(gain.gain.value).toBe(0);
    expect(source.connect).toHaveBeenCalledWith(highpass); expect(highpass.connect).toHaveBeenCalledWith(gain); expect(gain.connect).toHaveBeenCalledWith(destination);
    expect(g.track).toBe(track);
    level = amp(-60); for (let i = 0; i < 100; i++) { now += 10; tick(); }
    expect(gain.gain.setTargetAtTime).not.toHaveBeenCalled();
    level = amp(-20); now += 10; tick();
    expect(gain.gain.setTargetAtTime).toHaveBeenCalledWith(1, 1, expect.any(Number)); expect(g.levels().open).toBe(true);
    g.dispose(); g.dispose();
    expect(timers.clearInterval).toHaveBeenCalledOnce(); expect(track.stop).toHaveBeenCalledOnce(); expect(source.disconnect).toHaveBeenCalled();
  });
});
