// GPT-Live voice session: full-duplex WebRTC audio plus the "oai-events" data
// channel. The native gateway (voice_start) creates the provider session from
// our SDP offer; this class owns every browser resource of one session (mic,
// WebAudio gate, peer, channel, speaker, timers) and releases all of them on
// every exit path. App tools run here; mutating ones wait for the user's
// approval (card click or the user's own transcript), never the model's word.

import { BridgeAssistant } from "../core/bridge-assistant";
import { Bridge } from "../core/bridge";
import { ApprovalBox, cloneCard, containsWords, matchApproval, type ApprovalOutcome, type LiveApproval, type LivePreview } from "./approval";
import { buildGate, buildGateWorklet, levelOf, rmsOf, type GateGraph, type GateTimers } from "./gate";
import { parseAppToolArgs, parseToolArgs } from "./schema";
import { APP_TOOL_PREFIX, createToolRuntime, validAppTools, type AppTool, type Prepared, type ToolHost, type ToolResult } from "./tools";

export { matchApproval };
export type { AppTool, LiveApproval, LivePreview, ToolResult };

export type LivePhase = "idle" | "connecting" | "listening" | "thinking" | "speaking" | "closing" | "error";
export interface LiveTranscript { id: string; speaker: "user" | "assistant"; text: string }
export interface LiveSnapshot {
  phase: LivePhase; muted: boolean; error?: string; notice?: string;
  transcripts: LiveTranscript[]; approval?: LiveApproval; inputLevel: number; outputLevel: number;
  /** Set when the assistant just saved a fact about the user (show «به خاطر سپردم: …» briefly). */
  remembered?: { text: string; at: number };
  /** The last approval's outcome; `ok` once an approved tool ran (false on an error). Kept until the next card. */
  lastDecision?: LiveDecision;
}
export interface LiveDecision { id: string; outcome: ApprovalOutcome; ok?: boolean }

export interface LiveToolRuntime {
  isMutating(name: string): boolean;
  summarize(name: string, args: Record<string, unknown>): string;
  /**
   * `signal` aborts when the session ends (e.g. to cancel a chat request); late
   * results are discarded anyway. `approved` is true only after the user approved.
   */
  run(name: string, args: Record<string, unknown>, signal?: AbortSignal, approved?: boolean): Promise<string | ToolResult>;
  /** The session's app tools from voice_start; null when the session ends. */
  setAppTools?(tools: readonly AppTool[] | null): void;
  /** Whether an `app__*` name belongs to the current session. */
  knows?(name: string): boolean;
  /** Notes an executed tool in the usage memory (best effort, never throws). */
  record?(name: string, args: Record<string, unknown>): void;
  /** Optional: resolve targets and check values before an approval card is shown. */
  prepare?(name: string, args: Record<string, unknown>): Promise<Prepared>;
}
export interface LiveVoiceDeps {
  /** `appTools` (untrusted, validated here) are the session's extra app tools. */
  start(sdp: string): Promise<{ id: string; sdp: string; appTools?: unknown }>;
  end(id?: string): Promise<void>;
  tools: LiveToolRuntime;
}

/** The session's audio processing: gated mic track out, levels in and out. */
export interface AudioGraph {
  readonly track: MediaStreamTrack;
  readonly stream: MediaStream;
  readonly running: boolean;
  resume(): Promise<void>;
  /** 0…1, what the gate lets through. */
  inputLevel(): number;
  attachOutput(remote: MediaStream): void;
  outputLevel(): number;
  close(): Promise<void>;
}

export interface LiveEnv extends GateTimers {
  getUserMedia(constraints: MediaStreamConstraints): Promise<MediaStream>;
  createPeer(): RTCPeerConnection;
  createAudio(): HTMLAudioElement;
  createGraph(mic: MediaStream, timers: GateTimers): AudioGraph | Promise<AudioGraph>;
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  /** Wall-clock ms (approval expiry is shown to the user). */
  now(): number;
  uuid(): string;
  /** Calls `fn` once on the next user gesture (autoplay recovery); returns an unsubscribe. */
  onUserGesture(fn: () => void): () => void;
  log(message: string): void;
}

export const MIC_CONSTRAINTS: MediaStreamConstraints = {
  audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: false, channelCount: 1 },
  video: false,
};
export const LIMITS = {
  micMs: 30_000, audioMs: 5_000, offerMs: 10_000, iceMs: 10_000, createMs: 45_000, startedMs: 15_000,
  closeMs: 15_000, sessionMs: 10 * 60_000, lossMs: 10_000, meterMs: 100, eventBytes: 256 * 1024,
  output: 6000, commentaryBytes: 480, transcripts: 60, transcriptChars: 24_000, seen: 2048,
  /** User speech this soon after audible assistant output may be its echo: never an approval. */
  echoGuardMs: 600,
  /** Malformed events tolerated (dropped) before the session is considered broken. */
  malformed: 20,
  /** Longest wait for an active response to finish before response.create is sent anyway. */
  responseWaitMs: 8000,
} as const;

const SPEAKING_LEVEL = 0.1;

const FA = {
  micDenied: "اجازهٔ دسترسی به میکروفون داده نشد.",
  micMissing: "میکروفونی پیدا نشد. اتصال دستگاه را بررسی کنید.",
  micBusy: "میکروفون در دسترس نیست؛ برنامه‌های دیگری را که از آن استفاده می‌کنند ببندید.",
  micLost: "دسترسی به میکروفون قطع شد. دوباره شروع کنید.",
  timeout: "برقراری ارتباط بیش از حد طول کشید. دوباره تلاش کنید.",
  audio: "صدای برنامه فعال نشد. دوباره تلاش کنید.",
  unsupported: "این نسخهٔ WebView گفت‌وگوی زنده را پشتیبانی نمی‌کند.",
  invalidAnswer: "پاسخ سرویس صوتی نامعتبر بود. دوباره تلاش کنید.",
  invalidEvent: "پیام نامعتبر از سرویس دریافت شد. گفت‌وگو را دوباره شروع کنید.",
  provider: "سرویس گفت‌وگوی زنده خطا داد. دوباره شروع کنید.",
  channel: "ارتباط دادهٔ گفت‌وگو قطع شد. دوباره شروع کنید.",
  network: "ارتباط صوتی قطع شد. دوباره شروع کنید.",
  playback: "پخش صدای پاسخ ممکن نشد.",
  start: "شروع گفت‌وگوی زنده ممکن نشد",
  autoplay: "پخش صدا مسدود شد؛ یک بار روی جزیره کلیک کنید.",
  maxTime: "زمان این گفت‌وگو (۱۰ دقیقه) تمام شد؛ برای ادامه دوباره شروع کنید.",
  closedByService: "گفت‌وگو از طرف سرویس پایان یافت.",
  unconfirmed: "ارتباط محلی پایان یافت؛ تأیید پایان جلسه از سرویس دریافت نشد.",
  busyApproval: "یک درخواست دیگر منتظر تأیید کاربر است؛ این کار انجام نشد. پس از پاسخ کاربر، اگر هنوز لازم بود دوباره درخواست کن.",
  rejected: "کاربر این کار را تأیید نکرد؛ هیچ تغییری انجام نشد.",
  expired: "کاربر در مهلت ۶۰ ثانیه پاسخی نداد؛ این کار انجام نشد.",
  toolFailed: "اجرای این کار ناموفق بود؛ نگو که انجام شد.",
  tooLarge: "جزئیات این درخواست برای نمایش کامل به کاربر بیش از حد بزرگ است؛ انجام نشد. درخواست کوچک‌تری بده.",
  interrupt: "Stop speaking now and listen to the user. Do not resume the interrupted answer unless the user asks.",
};

/**
 * What the voice is asked to say for an approval. It contains no yes/no word
 * (matchApproval), so an echo of it can never approve or reject the card.
 */
export const APPROVAL_QUESTION = "برای کاری که روی صفحه نشان داده‌ام اجازه می‌خواهم. کوتاه بپرس «موافقید؟» و منتظر بمان.";
/**
 * What the voice is asked to say for an approval: a fixed sentence with no
 * argument value and no yes/no word, so its echo can never answer the card.
 */
export const approvalQuestion = (): string => APPROVAL_QUESTION;

/** Most bytes of pretty-printed arguments an approval card shows (never truncated). */
export const MAX_DETAILS_BYTES = 16 * 1024;
/** Output transcript kept for echo checks, and the margin around its audio spans. */
const ECHO_TEXT_MS = 10_000;
const ECHO_SPAN_MARGIN_MS = 300;

/** Provider errors that mean the session itself is unusable; others are logged and the call goes on. */
const FATAL_ERROR = /session|auth|api_key|quota|billing|permission|forbidden/i;

class Cancelled extends Error { constructor() { super("cancelled"); this.name = "AbortError"; } }
class Timeout extends Error { constructor() { super("timeout"); this.name = "TimeoutError"; } }

interface Session {
  stream?: MediaStream; graph?: AudioGraph; peer?: RTCPeerConnection; channel?: RTCDataChannel; audio?: HTMLAudioElement;
  remote?: MediaStream; id?: string;
  creationRequested: boolean; started: boolean; closing: boolean; finalized: boolean;
  cancel(): void; cancelled: Promise<never>;
  closed: Promise<boolean>; resolveClosed(value: boolean): void;
  begun: Promise<void>; resolveBegun(): void;
  seenEvents: Set<string>; seenCalls: Set<string>; pendingCalls: number;
  meter?: unknown; maxTimer?: unknown; lossTimer?: unknown; gestureOff?: () => void;
  suppressOutput: boolean; quietSince: number; lastOutput: number;
  /** Latest transcript end_ms: the session audio clock. */
  audioClock: number;
  malformed: number;
  tools: AbortController;
  /** Output transcript audio spans and recent text, for echo checks. */
  outputSpans: { start: number; end: number }[];
  outputText: { at: number; text: string }[];
  responseActive: boolean; createPending: boolean; createTimer?: unknown;
}

function newSession(): Session {
  let cancel!: () => void; let resolveClosed!: (v: boolean) => void; let resolveBegun!: () => void;
  const cancelled = new Promise<never>((_, reject) => { cancel = () => reject(new Cancelled()); });
  cancelled.catch(() => undefined);
  const closed = new Promise<boolean>(resolve => { resolveClosed = resolve; });
  const begun = new Promise<void>(resolve => { resolveBegun = resolve; });
  return {
    creationRequested: false, started: false, closing: false, finalized: false, cancel, cancelled, closed, resolveClosed, begun, resolveBegun,
    seenEvents: new Set(), seenCalls: new Set(), pendingCalls: 0, suppressOutput: false, quietSince: 0, lastOutput: 0,
    audioClock: -Infinity, malformed: 0, tools: new AbortController(), outputSpans: [], outputText: [], responseActive: false, createPending: false,
  };
}

function remember(set: Set<string>, key: string): boolean {
  if (set.has(key)) return false;
  set.add(key);
  if (set.size > LIMITS.seen) set.delete(set.values().next().value as string);
  return true;
}

/** UTF-8 chunks of at most `bytes` bytes, never splitting a character. */
export function commentaryChunks(text: string, bytes: number = LIMITS.commentaryBytes): string[] {
  const encoder = new TextEncoder(); const chunks: string[] = []; let chunk = ""; let size = 0;
  for (const char of text) {
    const n = encoder.encode(char).length;
    if (size + n > bytes && chunk) { chunks.push(chunk); chunk = ""; size = 0; }
    chunk += char; size += n;
  }
  if (chunk) chunks.push(chunk);
  return chunks;
}

const clip = (text: string, max: number) => text.length <= max ? text : `${text.slice(0, max - 1)}…`;
const shortString = (v: unknown, max = 256): v is string => typeof v === "string" && v.length > 0 && v.length <= max;

type LiveEvent =
  | { type: "session.started" } | { type: "session.closed" } | { type: "error"; code?: string }
  | { type: "transcript"; speaker: "user" | "assistant"; event_id: string; delta: string; start_ms?: number; end_ms?: number }
  | { type: "response"; active: boolean } | { type: "invalid" }
  | { type: "call"; event_id?: string; delegation_id?: string; call_id: unknown; name: unknown; arguments: unknown };

const RESPONSE_STARTED = new Set(["response.created", "response.in_progress"]);
const RESPONSE_ENDED = new Set(["response.completed", "response.done", "response.failed", "response.incomplete", "response.cancelled"]);
const clock = (v: unknown): number | undefined => typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : undefined;

/** Null for events this client ignores, "invalid" for a malformed known event; throws on unparseable input. */
export function parseLiveEvent(raw: unknown): LiveEvent | null {
  if (typeof raw !== "string" || raw.length > LIMITS.eventBytes) throw new Error("invalid-event");
  let value: Record<string, unknown>;
  try { value = JSON.parse(raw) as Record<string, unknown>; } catch { throw new Error("invalid-event"); }
  if (!value || typeof value !== "object" || Array.isArray(value) || typeof value.type !== "string") throw new Error("invalid-event");
  const eventId = shortString(value.event_id) ? value.event_id : undefined;
  switch (value.type) {
    case "session.started": case "session.closed": return { type: value.type };
    case "error": {
      const error = value.error as Record<string, unknown> | undefined;
      const code = typeof error?.code === "string" ? error.code : typeof error?.type === "string" ? error.type : undefined;
      return { type: "error", ...(code ? { code: code.slice(0, 64) } : {}) };
    }
    case "session.input_transcript.delta": case "session.output_transcript.delta":
      if (!eventId || typeof value.delta !== "string" || value.delta.length > 4000) return { type: "invalid" };
      return {
        type: "transcript", speaker: value.type === "session.input_transcript.delta" ? "user" : "assistant", event_id: eventId, delta: value.delta,
        start_ms: clock(value.start_ms), end_ms: clock(value.end_ms),
      };
    case "response.event": case "response.output_item.done": {
      const inner = value.type === "response.event" ? value.event as Record<string, unknown> | undefined : value;
      if (!inner || typeof inner !== "object") return null;
      if (typeof inner.type === "string" && RESPONSE_STARTED.has(inner.type)) return { type: "response", active: true };
      if (typeof inner.type === "string" && RESPONSE_ENDED.has(inner.type)) return { type: "response", active: false };
      if (inner.type !== "response.output_item.done") return null;
      const item = inner.item as Record<string, unknown> | undefined;
      if (!item || typeof item !== "object" || item.type !== "function_call") return null;
      const delegation = shortString(value.delegation_id) ? value.delegation_id : undefined;
      return { type: "call", event_id: eventId, delegation_id: delegation, call_id: item.call_id, name: item.name, arguments: item.arguments };
    }
    default:
      if (RESPONSE_STARTED.has(value.type)) return { type: "response", active: true };
      if (RESPONSE_ENDED.has(value.type)) return { type: "response", active: false };
      return null;
  }
}

function errorText(error: unknown): string {
  const name = error instanceof Error ? error.name : "";
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  if (name === "NotAllowedError" || name === "PermissionDeniedError" || name === "SecurityError") return FA.micDenied;
  if (name === "NotFoundError" || name === "OverconstrainedError") return FA.micMissing;
  if (name === "NotReadableError") return FA.micBusy;
  if (name === "TimeoutError") return FA.timeout;
  if (message === "audio-context") return FA.audio;
  if (message === "unsupported") return FA.unsupported;
  if (message === "invalid-answer") return FA.invalidAnswer;
  const detail = message.replace(/\s+/g, " ").trim().slice(0, 160);
  return detail ? `${FA.start} (${detail})` : `${FA.start}.`;
}

export class LiveVoice {
  private readonly env: LiveEnv;
  private session?: Session;
  private ending?: Promise<void>;
  private listeners = new Set<(s: LiveSnapshot) => void>();
  private state: LiveSnapshot = { phase: "idle", muted: false, transcripts: [], inputLevel: 0, outputLevel: 0 };
  private readonly approvals: ApprovalBox;

  constructor(private readonly deps: LiveVoiceDeps, env?: Partial<LiveEnv>) {
    this.env = { ...defaultEnv(), ...env };
    this.approvals = new ApprovalBox(
      { now: () => this.env.now(), setTimeout: (fn, ms) => this.env.setTimeout(fn, ms), clearTimeout: h => this.env.clearTimeout(h), uuid: () => this.env.uuid() },
      () => { this.state.approval = this.approvals.pending; this.refreshPhase(); this.emit(); },
      undefined,
      utterance => this.isEchoText(utterance),
    );
  }

  snapshot(): LiveSnapshot {
    const s = this.state;
    return { ...s, transcripts: s.transcripts.map(t => ({ ...t })), ...(s.approval ? { approval: cloneCard(s.approval) } : {}) };
  }

  /** Calls `fn` now and on every change. */
  subscribe(fn: (s: LiveSnapshot) => void): () => void {
    this.listeners.add(fn);
    fn(this.snapshot());
    return () => { this.listeners.delete(fn); };
  }

  private emit(): void {
    const snap = this.snapshot();
    for (const fn of this.listeners) { try { fn(snap); } catch (error) { this.env.log(`listener failed: ${String(error).slice(0, 120)}`); } }
  }

  private alive(s: Session): boolean { return this.session === s && !s.closing; }

  private bounded<T>(s: Session, promise: Promise<T>, ms: number): Promise<T> {
    let timer: unknown;
    const timeout = new Promise<never>((_, reject) => { timer = this.env.setTimeout(() => reject(new Timeout()), ms); });
    return Promise.race([promise, timeout, s.cancelled]).finally(() => this.env.clearTimeout(timer));
  }

  private send(s: Session, message: Record<string, unknown>): boolean {
    if (s.channel?.readyState !== "open") return false;
    try { s.channel.send(JSON.stringify({ ...message, event_id: this.env.uuid() })); return true; }
    catch (error) { this.env.log(`send ${String(message.type)} failed: ${String(error).slice(0, 120)}`); return false; }
  }

  /** Starts a session. Never rejects: failures end in phase "error" with a Persian `error`. No-op while active or closing. */
  async start(): Promise<void> {
    if (this.session || this.ending) return;
    const s = newSession();
    this.session = s;
    this.state = { phase: "connecting", muted: false, transcripts: [], inputLevel: 0, outputLevel: 0 };
    this.emit();
    try {
      const media = Promise.resolve().then(() => this.env.getUserMedia(MIC_CONSTRAINTS));
      // A grant that lands after cancellation or timeout must not leave the mic on.
      media.then(late => { if (!this.alive(s)) late.getTracks().forEach(t => t.stop()); }, () => undefined);
      const stream = await this.bounded(s, media, LIMITS.micMs);
      if (!this.alive(s)) return;
      s.stream = stream;
      for (const track of stream.getAudioTracks()) track.onended = () => { if (this.alive(s)) void this.fail(s, FA.micLost); };
      const building = Promise.resolve().then(() => this.env.createGraph(stream, this.env));
      // A graph finished after cancellation is closed here; release() can't see it.
      building.then(late => { if (!this.alive(s)) void late.close().catch(() => undefined); }, () => undefined);
      const graph = await this.bounded(s, building, LIMITS.audioMs);
      if (!this.alive(s)) return;
      s.graph = graph;
      await this.bounded(s, graph.resume(), LIMITS.audioMs);
      if (!graph.running) throw new Error("audio-context");
      const peer = this.env.createPeer(); s.peer = peer;
      const channel = peer.createDataChannel("oai-events"); s.channel = channel;
      const audio = this.env.createAudio(); s.audio = audio; audio.autoplay = true;
      this.wire(s, peer, channel, audio);
      peer.addTrack(graph.track, graph.stream);
      const offer = await this.bounded(s, peer.createOffer(), LIMITS.offerMs);
      await this.bounded(s, peer.setLocalDescription(offer), LIMITS.offerMs);
      await this.bounded(s, this.iceComplete(peer), LIMITS.iceMs);
      const sdp = peer.localDescription?.sdp ?? offer.sdp;
      if (!sdp || sdp.length > 65_536) throw new Error("invalid-offer");
      s.creationRequested = true;
      const created = this.deps.start(sdp);
      // An answer that lands after cancellation still names a session to clear natively.
      created.then(late => { if (!this.alive(s) && shortString(late?.id)) void this.deps.end(late.id).catch(() => undefined); }, () => undefined);
      const answer = await this.bounded(s, created, LIMITS.createMs);
      if (!answer || !shortString(answer.id) || typeof answer.sdp !== "string" || !answer.sdp.startsWith("v=0") || answer.sdp.length > 65_536) throw new Error("invalid-answer");
      s.id = answer.id;
      this.deps.tools.setAppTools?.(validAppTools(answer.appTools));
      await this.bounded(s, peer.setRemoteDescription({ type: "answer", sdp: answer.sdp }), LIMITS.offerMs);
      await this.bounded(s, s.begun, LIMITS.startedMs);
      if (!this.alive(s)) return;
      s.maxTimer = this.env.setTimeout(() => { if (this.alive(s)) void this.teardown(s, undefined, FA.maxTime); }, LIMITS.sessionMs);
      s.meter = this.env.setInterval(() => this.tick(s), LIMITS.meterMs);
      this.state.phase = "listening";
      this.emit();
    } catch (error) {
      if (!this.alive(s)) return;
      if (!(error instanceof Cancelled)) this.env.log(`start failed: ${error instanceof Error ? `${error.name} ${error.message}` : String(error)}`.slice(0, 200));
      await this.fail(s, errorText(error));
    }
  }

  private iceComplete(peer: RTCPeerConnection): Promise<void> {
    if (peer.iceGatheringState === "complete") return Promise.resolve();
    return new Promise(resolve => {
      const check = () => { if (peer.iceGatheringState === "complete") { peer.removeEventListener("icegatheringstatechange", check); resolve(); } };
      peer.addEventListener("icegatheringstatechange", check);
    });
  }

  private wire(s: Session, peer: RTCPeerConnection, channel: RTCDataChannel, audio: HTMLAudioElement): void {
    channel.addEventListener("message", event => this.onMessage(s, (event as MessageEvent).data));
    channel.addEventListener("close", () => {
      if (this.session !== s) return;
      s.resolveClosed(s.finalized);
      if (!s.closing) void this.fail(s, FA.channel);
    });
    channel.addEventListener("error", () => { if (this.alive(s)) void this.fail(s, FA.channel); });
    peer.addEventListener("connectionstatechange", () => {
      if (!this.alive(s)) return;
      const state = peer.connectionState;
      if (state === "connected" || state === "failed" || state === "closed") { if (s.lossTimer !== undefined) this.env.clearTimeout(s.lossTimer); s.lossTimer = undefined; }
      if (state === "failed" || state === "closed") void this.fail(s, FA.network);
      else if (state === "disconnected" && s.lossTimer === undefined) {
        s.lossTimer = this.env.setTimeout(() => {
          s.lossTimer = undefined;
          if (this.alive(s) && peer.connectionState === "disconnected") void this.fail(s, FA.network);
        }, LIMITS.lossMs);
      }
    });
    peer.addEventListener("track", event => {
      const e = event as RTCTrackEvent;
      if (!this.alive(s) || e.track.kind !== "audio") return;
      try {
        s.remote = e.streams[0] ?? new MediaStream([e.track]);
        audio.srcObject = s.remote;
        s.graph?.attachOutput(s.remote);
        this.play(s);
      } catch (error) {
        this.env.log(`remote track failed: ${String(error).slice(0, 120)}`);
        void this.fail(s, FA.playback);
      }
    });
    audio.addEventListener("error", () => { if (this.alive(s)) { this.state.notice = FA.playback; this.emit(); } });
  }

  private play(s: Session): void {
    const audio = s.audio; if (!audio) return;
    Promise.resolve().then(() => audio.play()).then(() => {
      if (!this.alive(s)) return;
      if (this.state.notice === FA.autoplay) { this.state.notice = undefined; this.emit(); }
    }).catch(() => {
      if (!this.alive(s)) return;
      // Autoplay blocked: retry on the next click or key press.
      this.state.notice = FA.autoplay; this.emit();
      s.gestureOff?.();
      s.gestureOff = this.env.onUserGesture(() => { s.gestureOff = undefined; void this.resumeAudio(); });
    });
  }

  /** Retries playback and the audio context (after an autoplay block). */
  async resumeAudio(): Promise<void> {
    const s = this.session; if (!s || s.closing) return;
    try { await this.bounded(s, s.graph?.resume() ?? Promise.resolve(), LIMITS.audioMs); } catch (error) { this.env.log(`audio resume failed: ${String(error).slice(0, 120)}`); }
    if (this.alive(s)) this.play(s);
  }

  private onMessage(s: Session, data: unknown): void {
    if (this.session !== s) return;
    let event: LiveEvent | null;
    try { event = parseLiveEvent(data); } catch { event = { type: "invalid" }; }
    if (!event) return;
    if (event.type === "invalid") {
      // Dropped; only a stream of them means the channel is broken.
      this.env.log("malformed event dropped");
      if (++s.malformed > LIMITS.malformed && !s.closing) void this.fail(s, FA.invalidEvent);
      return;
    }
    if (event.type === "session.closed") {
      s.finalized = true; s.resolveClosed(true);
      if (!s.closing) void this.teardown(s, undefined, FA.closedByService);
      return;
    }
    if (s.closing) return;
    switch (event.type) {
      case "session.started": s.started = true; s.resolveBegun(); return;
      case "error": {
        const fatal = !!event.code && FATAL_ERROR.test(event.code);
        this.env.log(`provider error ${event.code ?? "unknown"}${fatal ? " (fatal)" : ""}`);
        if (fatal) void this.fail(s, FA.provider);
        return;
      }
      case "response":
        s.responseActive = event.active;
        if (!event.active) this.flushResponse(s);
        return;
      case "transcript": {
        if (!remember(s.seenEvents, event.event_id)) return;
        this.appendTranscript(event.speaker, event.event_id, event.delta);
        if (event.end_ms !== undefined) s.audioClock = Math.max(s.audioClock, event.end_ms);
        if (event.speaker === "assistant") this.noteOutput(s, event);
        // Speech during or right after the assistant's own voice may be its echo: it never decides an approval.
        else if (!this.isEchoSpeech(s, event)) this.approvals.hear(event.delta, event.start_ms);
        this.emit();
        return;
      }
      case "call":
        if (event.event_id && !remember(s.seenEvents, event.event_id)) return;
        void this.handleCall(s, event);
        return;
    }
  }

  private noteOutput(s: Session, event: { delta: string; start_ms?: number; end_ms?: number }): void {
    const now = this.env.now();
    if (event.start_ms !== undefined) {
      const last = s.outputSpans.at(-1); const end = event.end_ms ?? event.start_ms;
      if (last && event.start_ms <= last.end + ECHO_SPAN_MARGIN_MS) last.end = Math.max(last.end, end);
      else { s.outputSpans.push({ start: event.start_ms, end }); if (s.outputSpans.length > 200) s.outputSpans.shift(); }
    }
    s.outputText.push({ at: now, text: event.delta });
    while (s.outputText.length && now - s.outputText[0].at > ECHO_TEXT_MS) s.outputText.shift();
  }

  private isEchoSpeech(s: Session, event: { start_ms?: number; end_ms?: number }): boolean {
    if (s.lastOutput > 0 && this.env.now() - s.lastOutput < LIMITS.echoGuardMs) return true;
    if (event.start_ms === undefined) return false;
    const start = event.start_ms; const end = event.end_ms ?? start;
    return s.outputSpans.some(span => start <= span.end + ECHO_SPAN_MARGIN_MS && end >= span.start - ECHO_SPAN_MARGIN_MS);
  }

  /** A settled user utterance that only repeats the assistant's last 10 s of speech. */
  private isEchoText(utterance: string): boolean {
    const s = this.session; if (!s) return false;
    const now = this.env.now();
    const recent = s.outputText.filter(x => now - x.at <= ECHO_TEXT_MS).map(x => x.text).join("");
    return containsWords(recent, utterance);
  }

  private appendTranscript(speaker: LiveTranscript["speaker"], id: string, delta: string): void {
    const list = this.state.transcripts; const last = list.at(-1);
    if (last?.speaker === speaker && last.text.length + delta.length <= LIMITS.transcriptChars) last.text += delta;
    else { list.push({ id, speaker, text: delta }); if (list.length > LIMITS.transcripts) list.splice(0, list.length - LIMITS.transcripts); }
  }

  private async handleCall(s: Session, call: Extract<LiveEvent, { type: "call" }>): Promise<void> {
    if (!shortString(call.call_id) || !shortString(call.name, 128)) { this.env.log("function call without id or name"); return; }
    if (!remember(s.seenCalls, call.call_id)) return;
    s.pendingCalls++; this.refreshPhase(); this.emit();
    try {
      const output = await this.callOutput(s, call.delegation_id, call.name, call.arguments);
      if (output === null || !this.alive(s)) return;
      this.send(s, { type: "response.item.create", item: { type: "function_call_output", call_id: call.call_id, output: clip(output, LIMITS.output) } });
      this.requestResponse(s);
    } finally {
      s.pendingCalls--;
      if (this.alive(s)) { this.refreshPhase(); this.emit(); }
    }
  }

  /** response.create now, or once the active response finishes (bounded), so they never overlap. */
  private requestResponse(s: Session): void {
    if (!s.responseActive) { this.send(s, { type: "response.create" }); return; }
    if (s.createPending) return;
    s.createPending = true;
    s.createTimer = this.env.setTimeout(() => { s.responseActive = false; this.flushResponse(s); }, LIMITS.responseWaitMs);
  }

  private flushResponse(s: Session): void {
    if (!s.createPending) return;
    s.createPending = false;
    if (s.createTimer !== undefined) this.env.clearTimeout(s.createTimer);
    s.createTimer = undefined;
    if (this.alive(s)) this.send(s, { type: "response.create" });
  }

  /** The function_call_output text, or null when the session ended meanwhile. */
  private async callOutput(s: Session, delegationId: string | undefined, name: string, raw: unknown): Promise<string | null> {
    const app = name.startsWith(APP_TOOL_PREFIX);
    const tools = this.deps.tools;
    const parsed = app
      ? tools.knows?.(name) ? parseAppToolArgs(raw) : { ok: false as const, reason: `unknown tool ${name.slice(0, 64)}` }
      : parseToolArgs(name, raw);
    if (!parsed.ok) { this.env.log(`rejected ${name.slice(0, 64)} call: ${parsed.reason}`); return `درخواست نامعتبر بود (${parsed.reason}). هیچ کاری انجام نشد.`; }
    if (!tools.isMutating(name)) return this.runTool(s, name, parsed.args, false);
    if (this.approvals.pending) return FA.busyApproval;
    let args = parsed.args; let summary: string; let preview: LivePreview | undefined;
    try {
      if (tools.prepare) {
        const prepared = await tools.prepare(name, args);
        if (!this.alive(s)) return null;
        if (!prepared.ok) return prepared.output;
        args = prepared.args; summary = prepared.summary; preview = prepared.preview;
      } else summary = tools.summarize(name, args);
    } catch (error) {
      this.env.log(`prepare ${name} failed: ${String(error).slice(0, 120)}`);
      return FA.toolFailed;
    }
    let details: string | undefined;
    if (!preview) {
      details = JSON.stringify(args, null, 2);
      if (new TextEncoder().encode(details).length > MAX_DETAILS_BYTES) return FA.tooLarge;
    }
    if (this.approvals.pending) return FA.busyApproval;
    this.state.lastDecision = undefined;
    const decision = this.approvals.request(name, clip(summary, 400), s.audioClock, preview, details);
    if (!decision) return FA.busyApproval;
    const cardId = (this.approvals.pending as LiveApproval | undefined)?.id ?? ""; // set by request()
    this.say(s, delegationId, approvalQuestion());
    const outcome: ApprovalOutcome = await decision;
    this.state.lastDecision = { id: cardId, outcome };
    this.emit();
    if (!this.alive(s) || outcome === "cancelled") return null;
    if (outcome !== "approved") return outcome === "rejected" ? FA.rejected : FA.expired;
    const result = await this.runResult(s, name, args, true);
    if (this.decisionId() === cardId) { this.state.lastDecision = { id: cardId, outcome, ok: result.ok }; this.emit(); }
    return result.output;
  }

  private decisionId(): string | undefined { return this.state.lastDecision?.id; }

  /** Whether the user's voice may answer the card: the UI says when it is actually on screen. */
  setApprovalVisible(visible: boolean): void { this.approvals.setVisible(visible); }

  private async runTool(s: Session, name: string, args: Record<string, unknown>, approved: boolean): Promise<string | null> {
    return (await this.runResult(s, name, args, approved)).output;
  }

  /** output null: the session ended meanwhile. */
  private async runResult(s: Session, name: string, args: Record<string, unknown>, approved: boolean): Promise<{ output: string | null; ok: boolean }> {
    try {
      const result = await this.deps.tools.run(name, args, s.tools.signal, approved);
      const value = typeof result === "string" ? { output: result, ok: true }
        : result && typeof result.output === "string" ? { output: result.output, ok: result.ok === true } : { output: FA.toolFailed, ok: false };
      if (value.ok) { try { this.deps.tools.record?.(name, args); } catch { this.env.log("memory record failed"); } }
      if (!this.alive(s)) return { output: null, ok: value.ok };
      const remembered = typeof result === "object" && typeof result.remembered === "string" ? result.remembered.slice(0, 300) : undefined;
      if (value.ok && remembered) { this.state.remembered = { text: remembered, at: this.env.now() }; this.emit(); }
      return value;
    } catch (error) {
      this.env.log(`tool ${name} failed: ${String(error).slice(0, 120)}`);
      return { output: this.alive(s) ? FA.toolFailed : null, ok: false };
    }
  }

  /** Makes the voice say/consider `text`: commentary on the delegation, else a session instruction. */
  private say(s: Session, delegationId: string | undefined, text: string): void {
    if (delegationId) for (const content of commentaryChunks(text)) this.send(s, { type: "session.commentary.append", delegation_id: delegationId, content });
    else this.send(s, { type: "session.instructions.append", delegation_id: null, content: text });
  }

  private tick(s: Session): void {
    if (!this.alive(s) || !s.graph) return;
    const now = this.env.now();
    const input = this.state.muted ? 0 : s.graph.inputLevel();
    const raw = s.audio?.paused ? 0 : s.graph.outputLevel();
    if (s.suppressOutput) {
      if (raw > SPEAKING_LEVEL / 2) s.quietSince = 0;
      else if (!s.quietSince) s.quietSince = now;
      else if (now - s.quietSince >= 500) { s.suppressOutput = false; if (s.audio) s.audio.muted = false; }
    }
    const output = s.suppressOutput ? 0 : raw;
    if (output > SPEAKING_LEVEL) s.lastOutput = now;
    const before = this.state;
    const phase = this.phaseOf(s, now);
    const changed = phase !== before.phase || Math.abs(input - before.inputLevel) >= 0.02 || Math.abs(output - before.outputLevel) >= 0.02;
    if (!changed) return;
    this.state.phase = phase; this.state.inputLevel = round(input); this.state.outputLevel = round(output);
    this.emit();
  }

  private phaseOf(s: Session, now = this.env.now()): LivePhase {
    if (s.closing) return "closing";
    if (!s.started || s.meter === undefined) return "connecting";
    if (s.lastOutput && now - s.lastOutput < 400) return "speaking";
    if (s.pendingCalls > 0 || this.approvals.pending) return "thinking";
    return "listening";
  }

  private refreshPhase(): void {
    const s = this.session;
    if (s && !s.closing && s.started && s.meter !== undefined) this.state.phase = this.phaseOf(s);
  }

  toggleMute(): void {
    const s = this.session; if (!s?.started || s.closing) return;
    const muted = !this.state.muted;
    s.stream?.getAudioTracks().forEach(t => { t.enabled = !muted; });
    if (s.graph) s.graph.track.enabled = !muted;
    this.send(s, { type: muted ? "session.input_audio.mute" : "session.input_audio.unmute" });
    this.state.muted = muted; if (muted) this.state.inputLevel = 0;
    this.emit();
  }

  interrupt(): void {
    const s = this.session; if (!s?.started || s.closing) return;
    s.suppressOutput = true; s.quietSince = 0; s.lastOutput = 0;
    if (s.audio) s.audio.muted = true;
    this.send(s, { type: "session.instructions.append", delegation_id: null, content: FA.interrupt });
    this.refreshPhase(); this.state.outputLevel = 0;
    this.emit();
  }

  /** The approval card's buttons. */
  /** `id` (the card's id) guards against a click on a card that was already replaced. */
  decide(approve: boolean, id?: string): void { this.approvals.decide(approve, id); }

  /** Ends the session: session.close, ≤15 s for session.closed, then releases everything. */
  async end(): Promise<void> {
    const s = this.session;
    if (!s) return;
    if (s.closing) { await this.ending; return; }
    await this.teardown(s);
  }

  private fail(s: Session, message: string): Promise<void> { return this.teardown(s, message); }

  private teardown(s: Session, error?: string, notice?: string): Promise<void> {
    if (s.closing) return this.ending ?? Promise.resolve();
    s.closing = true;
    const run = (async () => {
      s.cancel();
      s.tools.abort();
      this.approvals.cancel();
      this.state.phase = "closing"; this.state.inputLevel = 0; this.state.outputLevel = 0;
      if (error) this.state.error = error;
      if (notice) this.state.notice = notice;
      this.emit();
      s.stream?.getTracks().forEach(t => { t.enabled = false; });
      if (s.audio) s.audio.muted = true;
      let confirmed = s.finalized;
      if (!confirmed && this.send(s, { type: "session.close" })) {
        let timer: unknown;
        confirmed = await Promise.race([s.closed, new Promise<boolean>(resolve => { timer = this.env.setTimeout(() => resolve(false), LIMITS.closeMs); })]);
        this.env.clearTimeout(timer);
      }
      await this.release(s);
      if (s.creationRequested) {
        try { await this.deps.end(s.id); } catch (failure) { this.env.log(`voice_end failed: ${String(failure).slice(0, 120)}`); }
      }
      if (!confirmed && s.creationRequested && !error && !notice) this.state.notice = FA.unconfirmed;
      this.state = { ...this.state, phase: error ? "error" : "idle", muted: false, approval: undefined, inputLevel: 0, outputLevel: 0 };
      if (this.session === s) this.session = undefined;
      this.emit();
    })();
    const tracked: Promise<void> = run.finally(() => { if (this.ending === tracked) this.ending = undefined; });
    this.ending = tracked;
    return tracked;
  }

  private async release(s: Session): Promise<void> {
    s.cancel();
    s.tools.abort();
    if (this.session === s) this.deps.tools.setAppTools?.(null);
    s.resolveClosed(s.finalized);
    for (const timer of [s.maxTimer, s.lossTimer, s.createTimer]) if (timer !== undefined) this.env.clearTimeout(timer);
    if (s.meter !== undefined) this.env.clearInterval(s.meter);
    s.maxTimer = s.lossTimer = s.meter = undefined;
    s.gestureOff?.(); s.gestureOff = undefined;
    const safe = (label: string, fn: () => void) => { try { fn(); } catch (error) { this.env.log(`${label} cleanup failed: ${String(error).slice(0, 120)}`); } };
    if (s.audio) { const audio = s.audio; safe("audio", () => { audio.pause(); audio.srcObject = null; }); }
    s.stream?.getTracks().forEach(t => safe("mic", () => { t.onended = null; t.stop(); }));
    s.remote?.getTracks().forEach(t => safe("remote", () => t.stop()));
    if (s.channel) { const channel = s.channel; safe("channel", () => channel.close()); }
    if (s.peer) { const peer = s.peer; safe("peer", () => peer.close()); }
    if (s.graph) { try { await s.graph.close(); } catch (error) { this.env.log(`audio graph cleanup failed: ${String(error).slice(0, 120)}`); } }
  }

  /** Immediate, synchronous-as-possible cleanup (page hide / app shutdown). */
  dispose(): void {
    const s = this.session; if (!s) return;
    if (!s.finalized) this.send(s, { type: "session.close" });
    s.closing = true;
    this.approvals.cancel();
    void this.release(s);
    if (s.creationRequested) void this.deps.end(s.id).catch(error => this.env.log(`voice_end failed: ${String(error).slice(0, 120)}`));
    this.session = undefined;
    this.state = { ...this.state, phase: "idle", muted: false, approval: undefined, inputLevel: 0, outputLevel: 0 };
    this.emit();
  }
}

const round = (n: number) => Math.round(n * 100) / 100;

async function webAudioGraph(mic: MediaStream, timers: GateTimers): Promise<AudioGraph> {
  if (typeof AudioContext === "undefined") throw new Error("unsupported");
  const ctx = new AudioContext();
  let g: GateGraph;
  try {
    try {
      if (!ctx.audioWorklet) throw new Error("no AudioWorklet");
      // Bundled and served from the app's origin, so CSP script-src 'self' allows it.
      const { default: url } = await import("./gate-worklet.ts?worker&url");
      g = await buildGateWorklet(ctx, mic, url);
    } catch (error) {
      console.warn("[voice] audio-thread gate unavailable, using the main-thread gate", error instanceof Error ? error.name : "");
      g = buildGate(ctx, mic, timers);
    }
  } catch (error) { void ctx.close().catch(() => undefined); throw error; }
  let source: MediaStreamAudioSourceNode | undefined; let output: AnalyserNode | undefined;
  const buffer = new Float32Array(1024);
  return {
    track: g.track, stream: g.stream,
    get running() { return ctx.state === "running"; },
    resume: () => ctx.resume(),
    inputLevel: () => g.levels().gated,
    attachOutput(remote) {
      if (output) return;
      // Observe the remote stream only; the <audio> element plays it.
      source = ctx.createMediaStreamSource(remote); output = ctx.createAnalyser(); output.fftSize = 1024;
      source.connect(output);
    },
    outputLevel() { if (!output) return 0; output.getFloatTimeDomainData(buffer); return levelOf(rmsOf(buffer)); },
    async close() {
      g.dispose();
      try { source?.disconnect(); } catch { /* already disconnected */ }
      if (ctx.state !== "closed") await ctx.close();
    },
  };
}

function defaultEnv(): LiveEnv {
  return {
    getUserMedia(constraints) {
      if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) return Promise.reject(new Error("unsupported"));
      return navigator.mediaDevices.getUserMedia(constraints);
    },
    createPeer() {
      if (typeof RTCPeerConnection === "undefined") throw new Error("unsupported");
      return new RTCPeerConnection();
    },
    createAudio: () => new Audio(),
    createGraph: webAudioGraph,
    setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
    clearTimeout: handle => globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>),
    setInterval: (fn, ms) => globalThis.setInterval(fn, ms),
    clearInterval: handle => globalThis.clearInterval(handle as ReturnType<typeof setInterval>),
    now: () => Date.now(),
    uuid: () => crypto.randomUUID(),
    onUserGesture(fn) {
      const handler = () => { off(); fn(); };
      const off = () => { window.removeEventListener("pointerdown", handler, true); window.removeEventListener("keydown", handler, true); };
      window.addEventListener("pointerdown", handler, true); window.addEventListener("keydown", handler, true);
      return off;
    },
    log(message) { console.warn(`[voice] ${message}`); void Bridge.log(`voice: ${message}`); },
  };
}

// ── App singleton ────────────────────────────────────────────────────────────

let instance: LiveVoice | undefined;
let host: ToolHost | undefined;

/** The island wires its views, chat and character here (before or after liveVoice()). */
export function setLiveVoiceHost(next: ToolHost): void { host = next; }

const needHost = (): ToolHost => { if (!host) throw new Error("VOICE_HOST_MISSING"); return host; };
const hostProxy: ToolHost = {
  showView: view => needHost().showView(view),
  ask: (query, signal) => needHost().ask(query, signal),
  setCharacter: patch => needHost().setCharacter(patch),
  characterOptions: () => needHost().characterOptions(),
};

export function liveVoice(): LiveVoice {
  if (!instance) {
    const voice = new LiveVoice({
      start: sdp => BridgeAssistant.voiceStart(sdp),
      end: id => BridgeAssistant.voiceEnd(id),
      tools: createToolRuntime(hostProxy),
    });
    if (typeof window !== "undefined") window.addEventListener("pagehide", () => voice.dispose());
    instance = voice;
  }
  return instance;
}
