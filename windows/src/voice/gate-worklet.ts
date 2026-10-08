// AudioWorklet processor of the live voice noise gate (see gate.ts). Loaded with
// `?worker&url` so it is bundled and served from the app's own origin (CSP
// script-src 'self'); gate.ts falls back to a main-thread gate when it can't load.

import { GATE_DEFAULTS, GATE_PROCESSOR, GateKernel, type GateParams } from "./gate";

declare const sampleRate: number;
declare class AudioWorkletProcessor { readonly port: MessagePort; constructor(options?: unknown); }
declare function registerProcessor(name: string, processor: unknown): void;

const REPORT_MS = 50;

class GateProcessor extends AudioWorkletProcessor {
  private readonly kernel: GateKernel;
  private sinceReport = 0;
  private stopped = false;
  constructor(options?: { processorOptions?: Partial<GateParams> }) {
    super(options);
    this.kernel = new GateKernel({ ...GATE_DEFAULTS, ...options?.processorOptions });
    this.port.onmessage = event => { if (event.data === "stop") this.stopped = true; };
  }
  process(inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    if (this.stopped) return false;
    const output = outputs[0]?.[0];
    if (!output) return true;
    const blockMs = (output.length / sampleRate) * 1000;
    this.kernel.process(inputs[0]?.[0], output, blockMs);
    this.sinceReport += blockMs;
    if (this.sinceReport >= REPORT_MS) {
      this.sinceReport = 0;
      this.port.postMessage({ raw: this.kernel.raw, open: this.kernel.open });
    }
    return true;
  }
}

registerProcessor(GATE_PROCESSOR, GateProcessor);
