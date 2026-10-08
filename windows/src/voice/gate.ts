// Nearest/loudest-speaker noise gate for the live voice microphone.
// mic → highpass (~90 Hz) → gain (driven by gateStep) → MediaStreamDestination.
// The gate tracks the room's noise floor and the recent loudest speech; it opens
// only for signal well above the floor and within `windowDb` of that speech, so
// a voice across the room or a TV behind the user stays out while the user talks.

export interface GateParams {
  /** Opens only this far above the noise floor. */
  openAboveFloorDb: number;
  /** …and only within this distance of the recent loudest speech. */
  windowDb: number;
  /** Floor follows quieter levels with this time constant, louder ones much slower. */
  floorFallMs: number;
  floorRiseMs: number;
  /** Rise while the level looks like speech: a steady new noise is learnt, a talker is not. */
  floorRiseSpeechMs: number;
  /** The remembered speech peak sinks this many dB per second while nobody that loud speaks. */
  peakDecayDbPerS: number;
  /** Only a level held this long may raise the remembered speech peak (a click or a slam doesn't). */
  peakHoldMs: number;
  attackMs: number;
  holdMs: number;
  releaseMs: number;
}

export const GATE_DEFAULTS: GateParams = {
  openAboveFloorDb: 10, windowDb: 12, floorFallMs: 300, floorRiseMs: 4000, floorRiseSpeechMs: 20000,
  peakDecayDbPerS: 1.5, peakHoldMs: 150, attackMs: 10, holdMs: 250, releaseMs: 80,
};

export interface GateState {
  floorDb: number;
  peakDb: number;
  open: boolean;
  /** Time (ms) the gate stays open until, without new qualifying signal. */
  holdUntil: number;
  /** Smoothed gain 0…1. */
  gain: number;
  /** How long (ms) the level has stayed above the peak, and its lowest level meanwhile. */
  riseMs: number;
  riseMinDb: number;
}

export const SILENCE_DB = -100;
export const toDb = (rms: number): number => rms > 0 ? Math.max(SILENCE_DB, 20 * Math.log10(rms)) : SILENCE_DB;

export function initialGate(): GateState {
  return { floorDb: NaN, peakDb: SILENCE_DB, open: false, holdUntil: 0, gain: 0, riseMs: 0, riseMinDb: SILENCE_DB };
}

const follow = (from: number, to: number, dtMs: number, tauMs: number) => from + (to - from) * (1 - Math.exp(-dtMs / Math.max(1, tauMs)));

/** One step of the gate for a block whose RMS is `rms`, `dtMs` after the previous one, at time `nowMs`. Pure. */
export function gateStep(state: GateState, rms: number, dtMs: number, nowMs: number, p: GateParams = GATE_DEFAULTS): GateState {
  const level = toDb(Number.isFinite(rms) ? Math.abs(rms) : 0);
  const dt = Math.max(0, Math.min(dtMs, 1000));
  const loud = level >= state.floorDb + p.openAboveFloorDb;
  let floorDb = Number.isNaN(state.floorDb) ? level
    : follow(state.floorDb, level, dt, level < state.floorDb ? p.floorFallMs : loud ? p.floorRiseSpeechMs : p.floorRiseMs);
  floorDb = Math.min(floorDb, -20);
  const speech = level >= floorDb + p.openAboveFloorDb;
  let peakDb = state.peakDb - (p.peakDecayDbPerS * dt) / 1000;
  peakDb = Math.max(peakDb, floorDb + p.openAboveFloorDb);
  const above = speech && level > peakDb;
  const riseMs = above ? (state.riseMs || 0) + dt : 0;
  const riseMinDb = above ? Math.min(state.riseMs > 0 ? state.riseMinDb : level, level) : SILENCE_DB;
  if (above && riseMs >= p.peakHoldMs) peakDb = Math.max(peakDb, riseMinDb);
  const qualifies = speech && level >= peakDb - p.windowDb;
  const holdUntil = qualifies ? nowMs + p.holdMs : state.holdUntil;
  const open = qualifies || nowMs < holdUntil;
  const target = open ? 1 : 0;
  const gain = follow(state.gain, target, dt, target > state.gain ? p.attackMs : p.releaseMs);
  return { floorDb, peakDb, open, holdUntil, gain: gain < 1e-4 ? 0 : gain > 0.9999 ? 1 : gain, riseMs, riseMinDb };
}

export function rmsOf(samples: Float32Array): number {
  let sum = 0;
  for (let i = 0; i < samples.length; i++) sum += samples[i] * samples[i];
  return Math.sqrt(sum / (samples.length || 1));
}

/** 0…1 for meters: -60 dBFS → 0, -10 dBFS → 1. */
export function levelOf(rms: number): number {
  return Math.max(0, Math.min(1, (toDb(rms) + 60) / 50));
}

/** Per-block gate for the audio thread: applies gateStep's gain to the samples, ramped across the block. */
export class GateKernel {
  state: GateState = initialGate();
  raw = 0;
  private t = 0;
  constructor(private readonly params: GateParams = GATE_DEFAULTS) {}
  get open(): boolean { return this.state.open; }
  process(input: Float32Array | undefined, output: Float32Array, blockMs: number): void {
    const n = output.length;
    this.raw = input ? rmsOf(input) : 0;
    const from = this.state.gain;
    this.t += blockMs;
    this.state = gateStep(this.state, this.raw, blockMs, this.t, this.params);
    const to = this.state.gain;
    for (let i = 0; i < n; i++) output[i] = (input?.[i] ?? 0) * (from + ((to - from) * (i + 1)) / n);
  }
}

export interface GateTimers {
  setInterval(fn: () => void, ms: number): unknown;
  clearInterval(handle: unknown): void;
  now(): number;
}

export interface GateGraph {
  /** The gated track to send to the peer. */
  readonly track: MediaStreamTrack;
  readonly stream: MediaStream;
  /** Raw (pre-gate) and gated level of the last block, 0…1. */
  levels(): { raw: number; gated: number; open: boolean };
  dispose(): void;
}

const TICK_MS = 10;
export const GATE_PROCESSOR = "roadeep-gate";

/**
 * The gate on the audio thread (gate-worklet.ts): mic → highpass → worklet →
 * MediaStreamDestination. Rejects when the worklet can't be loaded; callers fall
 * back to buildGate.
 */
export async function buildGateWorklet(ctx: AudioContext, mic: MediaStream, moduleUrl: string, params: GateParams = GATE_DEFAULTS): Promise<GateGraph> {
  await ctx.audioWorklet.addModule(moduleUrl);
  const source = ctx.createMediaStreamSource(mic);
  const highpass = ctx.createBiquadFilter();
  highpass.type = "highpass"; highpass.frequency.value = 90; highpass.Q.value = 0.707;
  const node = new AudioWorkletNode(ctx, GATE_PROCESSOR, {
    numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1], channelCount: 1, channelCountMode: "explicit", processorOptions: params,
  });
  const destination = ctx.createMediaStreamDestination();
  source.connect(highpass); highpass.connect(node); node.connect(destination);
  let raw = 0; let open = false;
  node.port.onmessage = (event: MessageEvent) => {
    const data = event.data as { raw?: unknown; open?: unknown } | null;
    if (data && typeof data.raw === "number" && Number.isFinite(data.raw)) { raw = data.raw; open = data.open === true; }
  };
  let disposed = false;
  return {
    track: destination.stream.getAudioTracks()[0], stream: destination.stream,
    levels: () => ({ raw: levelOf(raw), gated: open ? levelOf(raw) : 0, open }),
    dispose() {
      if (disposed) return; disposed = true;
      node.port.onmessage = null;
      try { node.port.postMessage("stop"); } catch { /* port already closed */ }
      for (const n of [source, highpass, node]) { try { n.disconnect(); } catch { /* already disconnected */ } }
      destination.stream.getTracks().forEach(t => t.stop());
    },
  };
}

/** Fallback without AudioWorklet: drives a GainNode from the main thread every ~10 ms. */
export function buildGate(ctx: AudioContext, mic: MediaStream, timers: GateTimers, params: GateParams = GATE_DEFAULTS): GateGraph {
  const source = ctx.createMediaStreamSource(mic);
  const highpass = ctx.createBiquadFilter();
  highpass.type = "highpass"; highpass.frequency.value = 90; highpass.Q.value = 0.707;
  const analyser = ctx.createAnalyser();
  analyser.fftSize = 512; analyser.smoothingTimeConstant = 0;
  const gain = ctx.createGain();
  gain.gain.value = 0;
  const destination = ctx.createMediaStreamDestination();
  source.connect(highpass); highpass.connect(analyser); highpass.connect(gain); gain.connect(destination);
  const samples = new Float32Array(analyser.fftSize);
  let state = initialGate(); let last = timers.now(); let raw = 0;
  const handle = timers.setInterval(() => {
    const now = timers.now();
    analyser.getFloatTimeDomainData(samples);
    raw = rmsOf(samples);
    const previous = state.open;
    state = gateStep(state, raw, now - last, now, params);
    last = now;
    // The audio thread does the smoothing; this tick only moves the target.
    if (state.open !== previous) gain.gain.setTargetAtTime(state.open ? 1 : 0, ctx.currentTime, (state.open ? params.attackMs : params.releaseMs) / 3000);
  }, TICK_MS);
  const track = destination.stream.getAudioTracks()[0];
  let disposed = false;
  return {
    track, stream: destination.stream,
    levels: () => ({ raw: levelOf(raw), gated: state.open ? levelOf(raw) : 0, open: state.open }),
    dispose() {
      if (disposed) return; disposed = true;
      timers.clearInterval(handle);
      for (const node of [source, highpass, analyser, gain]) { try { node.disconnect(); } catch { /* already disconnected */ } }
      destination.stream.getTracks().forEach(t => t.stop());
    },
  };
}
