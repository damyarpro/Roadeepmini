import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { matchApproval } from "./approval";
import { LIMITS, LiveVoice, MIC_CONSTRAINTS, approvalQuestion, commentaryChunks, parseLiveEvent, type AudioGraph, type LiveEnv, type LiveToolRuntime } from "./live";
import { toolSpec } from "./schema";

const flush = async (n = 30) => { for (let i = 0; i < n; i++) await Promise.resolve(); };
function deferred<T>() { let resolve!: (v: T) => void; let reject!: (e: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; }

class FakeChannel extends EventTarget {
  readyState: RTCDataChannelState = "connecting";
  send = vi.fn();
  close = vi.fn(() => { this.readyState = "closed"; });
  get sent(): Record<string, unknown>[] { return this.send.mock.calls.map(c => JSON.parse(c[0] as string) as Record<string, unknown>); }
  emit(value: unknown) { this.dispatchEvent(new MessageEvent("message", { data: typeof value === "string" ? value : JSON.stringify(value) })); }
}
class FakePeer extends EventTarget {
  order: string[] = [];
  channel = new FakeChannel();
  iceGatheringState: RTCIceGatheringState = "complete";
  connectionState: RTCPeerConnectionState = "new";
  localDescription: RTCSessionDescriptionInit | null = null;
  addTrack = vi.fn();
  close = vi.fn();
  setRemoteDescription = vi.fn(async () => { this.order.push("remote"); });
  createDataChannel(label: string) { this.order.push(`channel:${label}`); return this.channel; }
  async createOffer() { this.order.push("offer"); return { type: "offer", sdp: "v=0\r\nm=audio offer" } as RTCSessionDescriptionInit; }
  async setLocalDescription(d: RTCSessionDescriptionInit) { this.localDescription = d; }
  setConnection(state: RTCPeerConnectionState) { this.connectionState = state; this.dispatchEvent(new Event("connectionstatechange")); }
}
class FakeAudio extends EventTarget {
  autoplay = false; muted = false; paused = false; srcObject: unknown = null;
  play = vi.fn(async () => undefined); pause = vi.fn();
}

function fixture(options: { tools?: Partial<LiveToolRuntime>; visible?: boolean } = {}) {
  const micTrack = { kind: "audio", enabled: true, stop: vi.fn(), onended: null as null | (() => void) };
  const mic = { getTracks: () => [micTrack], getAudioTracks: () => [micTrack] } as unknown as MediaStream;
  const gatedTrack = { kind: "audio", enabled: true, stop: vi.fn() } as unknown as MediaStreamTrack;
  const levels = { input: 0, output: 0 };
  const graph = {
    track: gatedTrack, stream: { id: "gated" } as unknown as MediaStream, running: true,
    resume: vi.fn(async () => undefined), inputLevel: () => levels.input, outputLevel: () => levels.output,
    attachOutput: vi.fn(), close: vi.fn(async () => undefined),
  } satisfies AudioGraph;
  const peer = new FakePeer(); const audio = new FakeAudio();
  let gesture: (() => void) | undefined;
  const getUserMedia = vi.fn(async () => mic);
  let uuid = 0;
  const env: Partial<LiveEnv> = {
    getUserMedia, createPeer: () => peer as unknown as RTCPeerConnection, createAudio: () => audio as unknown as HTMLAudioElement,
    createGraph: () => graph, uuid: () => `u${++uuid}`, onUserGesture: fn => { gesture = fn; return () => { gesture = undefined; }; }, log: vi.fn(),
  };
  const start = vi.fn(async (sdp: string) => { peer.order.push(`start:${sdp.slice(0, 3)}`); return { id: "live_1", sdp: "v=0\r\nanswer" }; });
  const end = vi.fn(async (_id?: string) => undefined);
  const run = vi.fn(async (name: string, _args?: Record<string, unknown>, _signal?: AbortSignal) => `ran ${name}`);
  const tools: LiveToolRuntime = { isMutating: name => toolSpec(name)?.mutating !== false, summarize: name => `do ${name}`, run, ...options.tools };
  const voice = new LiveVoice({ start, end, tools }, env);
  // Stands in for the island: the card is "on screen" as soon as it appears.
  if (options.visible !== false) voice.subscribe(snap => { if (snap.approval) voice.setApprovalVisible(true); });
  const channel = peer.channel;
  async function connect() {
    const starting = voice.start(); await flush();
    channel.readyState = "open"; channel.emit({ type: "session.started", event_id: "e0", session: { id: "live_1" } });
    await starting;
  }
  const call = (callId: string, name: string, args: unknown, eventId = `ev_${callId}`) => channel.emit({
    type: "response.event", event_id: eventId, delegation_id: "del_1",
    event: { type: "response.output_item.done", item: { type: "function_call", call_id: callId, name, arguments: typeof args === "string" ? args : JSON.stringify(args) } },
  });
  let n = 0;
  // Each delta is 100 ms later on the session audio clock unless `at` is given.
  const said = (delta: string, speaker: "input" | "output" = "input", at?: number) => {
    const start = at ?? (n + 1) * 1000;
    channel.emit({ type: `session.${speaker}_transcript.delta`, event_id: `t${++n}`, delta, start_ms: start, end_ms: start + 50 });
  };
  const outputs = () => channel.sent.filter(m => m.type === "response.item.create").map(m => (m.item as { call_id: string; output: string }));
  return { voice, env, mic, micTrack, gatedTrack, graph, levels, peer, channel, audio, start, end, run, getUserMedia, connect, call, said, outputs, gesture: () => gesture };
}

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

describe("live voice lifecycle", () => {
  it("opens the mic with focus constraints, creates the data channel before the offer and becomes listening", async () => {
    const f = fixture(); await f.connect();
    expect(f.getUserMedia).toHaveBeenCalledWith(MIC_CONSTRAINTS);
    expect(MIC_CONSTRAINTS.audio).toMatchObject({ echoCancellation: true, noiseSuppression: true, autoGainControl: false, channelCount: 1 });
    expect(f.peer.order.slice(0, 3)).toEqual(["channel:oai-events", "offer", "start:v=0"]);
    expect(f.peer.addTrack).toHaveBeenCalledWith(f.gatedTrack, f.graph.stream);
    expect(f.peer.setRemoteDescription).toHaveBeenCalledWith({ type: "answer", sdp: "v=0\r\nanswer" });
    expect(f.voice.snapshot().phase).toBe("listening");
  });

  it("ends with session.close, waits for session.closed and releases everything", async () => {
    const f = fixture(); await f.connect();
    const ending = f.voice.end(); await flush();
    expect(f.voice.snapshot().phase).toBe("closing");
    expect(f.channel.sent.at(-1)).toMatchObject({ type: "session.close" });
    expect(f.micTrack.stop).not.toHaveBeenCalled();
    f.channel.emit({ type: "session.closed", event_id: "c1" });
    await ending;
    expect(f.micTrack.stop).toHaveBeenCalledOnce(); expect(f.peer.close).toHaveBeenCalledOnce(); expect(f.channel.close).toHaveBeenCalledOnce();
    expect(f.graph.close).toHaveBeenCalledOnce(); expect(f.audio.srcObject).toBeNull(); expect(f.end).toHaveBeenCalledWith("live_1");
    expect(f.voice.snapshot().phase).toBe("idle"); expect(f.voice.snapshot().notice).toBeUndefined();
  });

  it("gives up waiting for session.closed after 15 s and says termination is unconfirmed", async () => {
    const f = fixture(); await f.connect();
    const ending = f.voice.end(); await vi.advanceTimersByTimeAsync(LIMITS.closeMs); await ending;
    expect(f.micTrack.stop).toHaveBeenCalledOnce(); expect(f.voice.snapshot().phase).toBe("idle");
    expect(f.voice.snapshot().notice).toContain("تأیید پایان");
  });

  it("concurrent end calls share one teardown and start is refused while closing", async () => {
    const f = fixture(); await f.connect();
    const a = f.voice.end(); const b = f.voice.end(); await f.voice.start();
    f.channel.emit({ type: "session.closed" }); await Promise.all([a, b]);
    expect(f.end).toHaveBeenCalledOnce(); expect(f.start).toHaveBeenCalledOnce();
  });

  it("reports microphone denial without contacting the provider", async () => {
    const f = fixture(); f.getUserMedia.mockRejectedValueOnce(new DOMException("denied", "NotAllowedError"));
    await f.voice.start();
    expect(f.start).not.toHaveBeenCalled(); expect(f.end).not.toHaveBeenCalled();
    expect(f.voice.snapshot()).toMatchObject({ phase: "error", error: expect.stringContaining("میکروفون") });
  });

  it("cleans up mic, graph and native lifecycle when session creation fails", async () => {
    const f = fixture(); f.start.mockRejectedValueOnce(new Error("VOICE_NOT_CONFIGURED"));
    await f.voice.start();
    expect(f.micTrack.stop).toHaveBeenCalledOnce(); expect(f.graph.close).toHaveBeenCalledOnce(); expect(f.peer.close).toHaveBeenCalledOnce();
    expect(f.end).toHaveBeenCalledWith(undefined);
    expect(f.voice.snapshot()).toMatchObject({ phase: "error", error: expect.stringContaining("VOICE_NOT_CONFIGURED") });
    // A new attempt is allowed after an error.
    f.start.mockResolvedValueOnce({ id: "live_2", sdp: "v=0" }); const again = f.voice.start(); await flush(80);
    expect(f.start).toHaveBeenCalledTimes(2); f.voice.dispose(); await again;
  });

  it("rejects an invalid provider answer", async () => {
    const f = fixture(); f.start.mockResolvedValueOnce({ id: "x", sdp: "not sdp" });
    await f.voice.start();
    expect(f.voice.snapshot().phase).toBe("error"); expect(f.peer.setRemoteDescription).not.toHaveBeenCalled(); expect(f.end).toHaveBeenCalledWith(undefined);
  });

  it("times out when session.started never arrives", async () => {
    const f = fixture(); const starting = f.voice.start(); await flush();
    await vi.advanceTimersByTimeAsync(LIMITS.startedMs); await starting;
    expect(f.voice.snapshot()).toMatchObject({ phase: "error", error: expect.stringContaining("طول کشید") });
    expect(f.micTrack.stop).toHaveBeenCalledOnce(); expect(f.end).toHaveBeenCalledWith("live_1");
  });

  it("stops a microphone granted after cancellation and never creates a session", async () => {
    const f = fixture(); const grant = deferred<MediaStream>(); f.getUserMedia.mockReturnValueOnce(grant.promise);
    const starting = f.voice.start(); await flush(); await f.voice.end(); await starting;
    grant.resolve(f.mic); await flush();
    expect(f.micTrack.stop).toHaveBeenCalled(); expect(f.start).not.toHaveBeenCalled(); expect(f.voice.snapshot().phase).toBe("idle");
  });

  it("clears a provider session created after cancellation", async () => {
    const f = fixture(); const created = deferred<{ id: string; sdp: string }>(); f.start.mockReturnValueOnce(created.promise);
    const starting = f.voice.start(); await flush(); await f.voice.end(); await starting;
    created.resolve({ id: "late_1", sdp: "v=0" }); await flush();
    expect(f.peer.setRemoteDescription).not.toHaveBeenCalled(); expect(f.end).toHaveBeenCalledWith("late_1");
  });

  it("ends when the service closes the session", async () => {
    const f = fixture(); await f.connect();
    f.channel.emit({ type: "session.closed", usage: { seconds: 3 } }); await flush();
    expect(f.voice.snapshot()).toMatchObject({ phase: "idle", notice: expect.stringContaining("سرویس") });
    expect(f.micTrack.stop).toHaveBeenCalledOnce(); expect(f.channel.sent.some(m => m.type === "session.close")).toBe(false);
  });

  it("drops malformed events and survives request-level provider errors", async () => {
    const f = fixture(); await f.connect();
    f.channel.emit("{not json"); f.channel.emit({ type: "session.input_transcript.delta", delta: "no id" });
    f.channel.emit({ type: "session.output_transcript.delta", event_id: "big", delta: "x".repeat(5000) });
    f.channel.emit({ type: "error", error: { code: "conversation_already_has_active_response", message: "secret text" } });
    f.channel.emit({ type: "error" });
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.voice.snapshot().phase).toBe("listening"); expect(f.micTrack.stop).not.toHaveBeenCalled();
    expect(f.voice.snapshot().transcripts).toEqual([]);
    expect(JSON.stringify(vi.mocked(f.env.log!).mock.calls)).not.toContain("secret text");
    f.voice.dispose();
  });

  it("ends on a session-level provider error or a stream of malformed events", async () => {
    const f = fixture(); await f.connect();
    f.channel.emit({ type: "error", error: { code: "session_expired" } }); await vi.advanceTimersByTimeAsync(LIMITS.closeMs);
    expect(f.voice.snapshot().phase).toBe("error"); expect(f.micTrack.stop).toHaveBeenCalledOnce();
    const g = fixture(); await g.connect();
    for (let i = 0; i <= LIMITS.malformed; i++) g.channel.emit("{bad");
    await vi.advanceTimersByTimeAsync(LIMITS.closeMs);
    expect(g.voice.snapshot()).toMatchObject({ phase: "error", error: expect.stringContaining("نامعتبر") });
  });

  it("caps a session at 10 minutes", async () => {
    const f = fixture(); await f.connect();
    await vi.advanceTimersByTimeAsync(LIMITS.sessionMs); f.channel.emit({ type: "session.closed" }); await flush();
    expect(f.voice.snapshot()).toMatchObject({ phase: "idle", notice: expect.stringContaining("۱۰ دقیقه") });
  });

  it("tolerates a short network drop but fails after 10 s disconnected", async () => {
    const f = fixture(); await f.connect();
    f.peer.setConnection("disconnected"); await vi.advanceTimersByTimeAsync(5000); f.peer.setConnection("connected");
    await vi.advanceTimersByTimeAsync(LIMITS.lossMs); expect(f.voice.snapshot().phase).toBe("listening");
    f.peer.setConnection("disconnected"); await vi.advanceTimersByTimeAsync(LIMITS.lossMs + LIMITS.closeMs);
    expect(f.voice.snapshot().phase).toBe("error"); expect(f.peer.close).toHaveBeenCalledOnce();
  });

  it("fails when the data channel closes or the microphone track ends", async () => {
    const f = fixture(); await f.connect();
    f.channel.readyState = "closed"; f.channel.dispatchEvent(new Event("close")); await flush();
    expect(f.voice.snapshot().phase).toBe("error");
    const g = fixture(); await g.connect(); g.micTrack.onended?.(); await vi.advanceTimersByTimeAsync(LIMITS.closeMs);
    expect(g.voice.snapshot()).toMatchObject({ phase: "error", error: expect.stringContaining("میکروفون") });
  });

  it("recovers from a blocked autoplay on the next user gesture", async () => {
    const f = fixture(); await f.connect();
    f.audio.play.mockRejectedValueOnce(new DOMException("blocked", "NotAllowedError"));
    const remote = { getTracks: () => [] } as unknown as MediaStream;
    const track = new Event("track") as RTCTrackEvent; Object.assign(track, { track: { kind: "audio" }, streams: [remote] });
    f.peer.dispatchEvent(track); await flush();
    expect(f.graph.attachOutput).toHaveBeenCalledWith(remote); expect(f.voice.snapshot().notice).toContain("مسدود");
    f.gesture()?.(); await flush();
    expect(f.graph.resume).toHaveBeenCalledTimes(2); expect(f.audio.play).toHaveBeenCalledTimes(2); expect(f.voice.snapshot().notice).toBeUndefined();
    f.voice.dispose();
  });

  it("mutes, unmutes and interrupts through the data channel", async () => {
    const f = fixture(); await f.connect();
    f.voice.toggleMute();
    expect(f.micTrack.enabled).toBe(false); expect(f.gatedTrack.enabled).toBe(false); expect(f.voice.snapshot().muted).toBe(true);
    f.voice.toggleMute(); expect(f.micTrack.enabled).toBe(true);
    f.voice.interrupt(); expect(f.audio.muted).toBe(true);
    expect(f.channel.sent.map(m => m.type)).toEqual(["session.input_audio.mute", "session.input_audio.unmute", "session.instructions.append"]);
    expect(f.channel.sent[2]).toMatchObject({ delegation_id: null });
    // Output stays muted until the interrupted answer has been quiet for 500 ms.
    await vi.advanceTimersByTimeAsync(700); expect(f.audio.muted).toBe(false);
    f.voice.dispose();
  });

  it("exposes levels and derives speaking from the output level", async () => {
    const f = fixture(); await f.connect();
    f.levels.input = 0.5; f.levels.output = 0.6; await vi.advanceTimersByTimeAsync(LIMITS.meterMs);
    expect(f.voice.snapshot()).toMatchObject({ phase: "speaking", inputLevel: 0.5, outputLevel: 0.6 });
    f.levels.output = 0; await vi.advanceTimersByTimeAsync(600);
    expect(f.voice.snapshot().phase).toBe("listening");
    f.voice.dispose();
  });

  it("merges transcript deltas per speaker and drops duplicate event ids", async () => {
    const f = fixture(); await f.connect();
    f.channel.emit({ type: "session.input_transcript.delta", event_id: "a", delta: "سلام ", start_ms: 0, end_ms: 1 });
    f.channel.emit({ type: "session.input_transcript.delta", event_id: "a", delta: "سلام ", start_ms: 0, end_ms: 1 });
    f.channel.emit({ type: "session.input_transcript.delta", event_id: "b", delta: "خوبی؟", start_ms: 1, end_ms: 2 });
    f.said("سلام!", "output");
    expect(f.voice.snapshot().transcripts).toEqual([
      { id: "a", speaker: "user", text: "سلام خوبی؟" }, { id: "t1", speaker: "assistant", text: "سلام!" },
    ]);
    f.voice.dispose();
  });

  it("notifies subscribers immediately and on changes until unsubscribed", async () => {
    const f = fixture(); const seen: string[] = [];
    const off = f.voice.subscribe(s => seen.push(s.phase));
    await f.connect(); off(); f.voice.dispose();
    expect(seen[0]).toBe("idle"); expect(seen).toContain("connecting"); expect(seen.at(-1)).toBe("listening");
  });
});

describe("live voice tools", () => {
  it("runs a non-mutating tool, then sends its output and response.create", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "list_tasks", { filter: "today" }); await flush();
    expect(f.run).toHaveBeenCalledWith("list_tasks", { filter: "today" }, expect.any(AbortSignal), false);
    const sent = f.channel.sent;
    expect(sent.at(-2)).toMatchObject({ type: "response.item.create", item: { type: "function_call_output", call_id: "c1", output: "ran list_tasks" } });
    expect(sent.at(-1)).toMatchObject({ type: "response.create" });
    expect(new Set(sent.map(m => m.event_id)).size).toBe(sent.length);
    f.voice.dispose();
  });

  it("executes a call_id once even when it repeats under new event ids", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "get_focus", {}, "e1"); f.call("c1", "get_focus", {}, "e2"); f.call("c2", "get_focus", {}, "e2"); await flush();
    expect(f.run).toHaveBeenCalledOnce(); expect(f.outputs()).toHaveLength(1);
    f.voice.dispose();
  });

  it("refuses invalid, extra or unknown arguments without running anything", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "list_tasks", { filter: "soon" }); f.call("c2", "add_task", { title: "x", owner: "me" });
    f.call("c3", "run_shell", {}); f.call("c4", "add_task", "{oops"); f.call("c5", "add_task", { title: "" });
    await flush();
    expect(f.run).not.toHaveBeenCalled();
    expect(f.outputs().map(o => o.call_id)).toEqual(["c1", "c2", "c3", "c4", "c5"]);
    for (const o of f.outputs()) expect(o.output).toContain("هیچ کاری انجام نشد");
    expect(f.voice.snapshot().approval).toBeUndefined();
    f.voice.dispose();
  });

  it("ignores nested events that are not finished function calls", async () => {
    const f = fixture(); await f.connect();
    f.channel.emit({ type: "response.event", delegation_id: "d", event: { type: "response.output_text.delta", delta: "x" } });
    f.channel.emit({ type: "response.event", delegation_id: "d", event: { type: "response.output_item.done", item: { type: "message" } } });
    f.channel.emit({ type: "response.event", delegation_id: "d", event: { type: "response.output_item.done", item: { type: "function_call", name: "get_focus" } } });
    await flush(); expect(f.run).not.toHaveBeenCalled(); expect(f.voice.snapshot().phase).toBe("listening");
    f.voice.dispose();
  });

  it("asks for approval, speaks the question and runs after the user says yes", async () => {
    const f = fixture(); await f.connect();
    f.said("بله "); // said before the card: must not count
    f.call("c1", "add_task", { title: "نان بخر" }); await flush();
    const card = f.voice.snapshot().approval!;
    expect(card).toMatchObject({ tool: "add_task", summary: "do add_task" });
    expect(card.expiresAt).toBe(Date.now() + 60_000);
    expect(f.voice.snapshot().phase).toBe("thinking");
    expect(f.channel.sent.find(m => m.type === "session.commentary.append")).toMatchObject({ delegation_id: "del_1", content: approvalQuestion() });
    expect(JSON.stringify(f.channel.sent)).not.toContain("نان بخر"); // no argument value is ever spoken
    await vi.advanceTimersByTimeAsync(1000); expect(f.run).not.toHaveBeenCalled();
    f.said("آره", "output"); await vi.advanceTimersByTimeAsync(1000); expect(f.run).not.toHaveBeenCalled(); // the assistant's words never approve
    f.said("با"); f.said("شه، انجامش بده"); await vi.advanceTimersByTimeAsync(700);
    expect(f.run).toHaveBeenCalledWith("add_task", { title: "نان بخر" }, expect.any(AbortSignal), true);
    expect(f.outputs()).toEqual([{ type: "function_call_output", call_id: "c1", output: "ran add_task" }]);
    expect(f.voice.snapshot().approval).toBeUndefined();
    f.voice.dispose();
  });

  it("uses prepared arguments and summary when the runtime resolves targets first", async () => {
    const prepare = vi.fn(async () => ({ ok: true as const, args: { id: "t1", title: "نان" }, summary: "حذف کار «نان»" }));
    const f = fixture({ tools: { prepare } }); await f.connect();
    f.call("c1", "delete_task", { query: "نان" }); await flush();
    expect(f.voice.snapshot().approval?.summary).toBe("حذف کار «نان»");
    f.voice.decide(true); await flush();
    expect(f.run).toHaveBeenCalledWith("delete_task", { id: "t1", title: "نان" }, expect.any(AbortSignal), true);
    f.voice.dispose();
  });

  it("returns the prepare refusal (e.g. an ambiguous title) without an approval card", async () => {
    const f = fixture({ tools: { prepare: async () => ({ ok: false as const, output: "چند کار پیدا شد" }) } }); await f.connect();
    f.call("c1", "complete_task", { query: "x" }); await flush();
    expect(f.voice.snapshot().approval).toBeUndefined(); expect(f.outputs()[0].output).toBe("چند کار پیدا شد"); expect(f.run).not.toHaveBeenCalled();
    f.voice.dispose();
  });

  it("is approved by a click", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush(); f.voice.decide(true); await flush();
    expect(f.run).toHaveBeenCalledOnce(); expect(f.outputs()[0].output).toBe("ran add_note");
    f.voice.dispose();
  });

  it("is rejected by voice or by a click", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush(); f.said("نه، انجام نده"); await vi.advanceTimersByTimeAsync(700);
    f.call("c2", "add_note", { text: "y" }); await flush(); f.voice.decide(false); await flush();
    expect(f.run).not.toHaveBeenCalled();
    expect(f.outputs().map(o => o.output)).toEqual([expect.stringContaining("تأیید نکرد"), expect.stringContaining("تأیید نکرد")]);
    f.voice.dispose();
  });

  it("keeps waiting on an ambiguous answer and expires after 60 s", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush(); f.said("بله نه"); await vi.advanceTimersByTimeAsync(5000);
    expect(f.voice.snapshot().approval).toBeDefined();
    await vi.advanceTimersByTimeAsync(55_000);
    expect(f.voice.snapshot().approval).toBeUndefined(); expect(f.run).not.toHaveBeenCalled();
    expect(f.outputs()[0].output).toContain("۶۰ ثانیه"); expect(f.channel.sent.at(-1)).toMatchObject({ type: "response.create" });
    f.voice.dispose();
  });

  it("refuses a second mutating call while one approval is pending, but still runs reads", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    f.call("c2", "delete_task", { query: "y" }); f.call("c3", "get_focus", {}); await flush();
    expect(f.outputs().find(o => o.call_id === "c2")?.output).toContain("منتظر تأیید");
    expect(f.outputs().find(o => o.call_id === "c3")?.output).toBe("ran get_focus");
    expect(f.voice.snapshot().approval?.tool).toBe("add_note");
    f.voice.decide(true); await flush();
    expect(f.run.mock.calls.map(c => c[0])).toEqual(["get_focus", "add_note"]);
    f.voice.dispose();
  });

  it("drops the pending approval and sends nothing when the session ends", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    const ending = f.voice.end(); f.channel.emit({ type: "session.closed" }); await ending;
    f.voice.decide(true); await flush();
    expect(f.run).not.toHaveBeenCalled(); expect(f.outputs()).toEqual([]); expect(f.voice.snapshot().approval).toBeUndefined();
  });

  it("never publishes a late tool result after the session ended", async () => {
    const f = fixture(); const late = deferred<string>(); f.run.mockReturnValueOnce(late.promise); await f.connect();
    f.call("c1", "ask_roadeep", { query: "?" }); await flush();
    expect(f.voice.snapshot().phase).toBe("thinking");
    f.voice.dispose(); late.resolve("secret"); await flush();
    expect(JSON.stringify(f.channel.sent)).not.toContain("secret");
  });

  it("answers with a safe text when a tool throws", async () => {
    const f = fixture(); f.run.mockRejectedValueOnce(new Error("boom")); await f.connect();
    f.call("c1", "get_settings", {}); await flush();
    expect(f.outputs()[0].output).toContain("ناموفق"); expect(f.outputs()[0].output).not.toContain("boom");
    f.voice.dispose();
  });

  it("never lets speech from before the card approve it, even when its delta arrives late", async () => {
    const f = fixture(); await f.connect();
    f.said("می‌خوام یه کار اضافه کنم", "input", 1000); f.said("باشه حتما", "output", 1200);
    f.call("c1", "add_task", { title: "x" }); await flush();
    f.said("بله", "input", 900); await vi.advanceTimersByTimeAsync(1000); // started before the card's clock (1250)
    expect(f.run).not.toHaveBeenCalled();
    f.said("بله", "input", 2000); await vi.advanceTimersByTimeAsync(700);
    expect(f.run).toHaveBeenCalledOnce();
    f.voice.dispose();
  });

  it("ignores user speech while the assistant is audible and for 600 ms after", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    f.levels.output = 0.8; await vi.advanceTimersByTimeAsync(LIMITS.meterMs);
    f.said("بله"); await vi.advanceTimersByTimeAsync(LIMITS.meterMs); // echo of the question
    f.levels.output = 0; await vi.advanceTimersByTimeAsync(300);
    f.said("بله"); await vi.advanceTimersByTimeAsync(800);
    expect(f.run).not.toHaveBeenCalled();
    f.said("بله"); await vi.advanceTimersByTimeAsync(700);
    expect(f.run).toHaveBeenCalledOnce();
    f.voice.dispose();
  });

  it("asks the approval question without any yes/no word", () => {
    expect(matchApproval(approvalQuestion())).toBeNull();
    const words = approvalQuestion().split(/[\s.،؟«»:]+/);
    for (const word of ["تأیید", "تایید", "بله", "نه", "باشه", "آره"]) expect(words).not.toContain(word);
    expect(approvalQuestion()).not.toContain("انجام بده");
  });

  it("decides after an unclear utterance once a clear one follows", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    f.said("بله نه"); await vi.advanceTimersByTimeAsync(700); expect(f.voice.snapshot().approval).toBeDefined();
    f.said("نه"); await vi.advanceTimersByTimeAsync(700);
    expect(f.outputs()[0].output).toContain("تأیید نکرد");
    f.voice.dispose();
  });

  it("ignores a click for a card that is no longer current", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    const id = f.voice.snapshot().approval!.id;
    f.voice.decide(true, "stale"); await flush(); expect(f.run).not.toHaveBeenCalled();
    f.voice.decide(true, id); await flush(); expect(f.run).toHaveBeenCalledOnce();
    f.voice.dispose();
  });

  it("waits for the active response to finish before response.create", async () => {
    const f = fixture(); await f.connect();
    f.channel.emit({ type: "response.event", delegation_id: "del_1", event: { type: "response.created" } });
    f.call("c1", "get_focus", {}); f.call("c2", "list_notes", {}); await flush();
    expect(f.outputs()).toHaveLength(2); expect(f.channel.sent.some(m => m.type === "response.create")).toBe(false);
    f.channel.emit({ type: "response.event", delegation_id: "del_1", event: { type: "response.completed" } });
    expect(f.channel.sent.filter(m => m.type === "response.create")).toHaveLength(1);
    // Without an end signal it is sent after a bounded wait.
    f.channel.emit({ type: "response.event", delegation_id: "del_1", event: { type: "response.created" } });
    f.call("c3", "get_focus", {}); await flush();
    await vi.advanceTimersByTimeAsync(LIMITS.responseWaitMs);
    expect(f.channel.sent.filter(m => m.type === "response.create")).toHaveLength(2);
    f.voice.dispose();
  });

  it("aborts a running tool's signal when the session ends", async () => {
    const f = fixture(); const late = deferred<string>(); f.run.mockReturnValueOnce(late.promise); await f.connect();
    f.call("c1", "ask_roadeep", { query: "?" }); await flush();
    const signal = f.run.mock.calls[0][2]!;
    expect(signal.aborted).toBe(false);
    const ending = f.voice.end(); await flush(); expect(signal.aborted).toBe(true);
    f.channel.emit({ type: "session.closed" }); await ending; late.resolve("late"); await flush();
    expect(JSON.stringify(f.channel.sent)).not.toContain("late");
  });

  it("takes the session's app tools from voice_start and clears them at the end", async () => {
    const appTools = [{ name: "app__gh__create", title: "Create issue", server: "GitHub", readOnly: false }, { name: "app__gh__list", title: "List", server: "GitHub", readOnly: true }];
    const known = new Set<string>(); const setAppTools = vi.fn((t: readonly { name: string }[] | null) => { known.clear(); t?.forEach(x => known.add(x.name)); });
    const f = fixture({ tools: { setAppTools, knows: (n: string) => known.has(n), isMutating: (n: string) => n === "app__gh__create" } });
    f.start.mockResolvedValueOnce({ id: "live_1", sdp: "v=0", appTools: [...appTools, { name: "bogus" }] } as never);
    await f.connect();
    expect(setAppTools).toHaveBeenLastCalledWith(appTools);
    f.call("c1", "app__gh__list", { q: 1 }); f.call("c2", "app__gh__nope", {}); f.call("c3", "app__gh__list", "[1]"); await flush();
    expect(f.run).toHaveBeenCalledWith("app__gh__list", { q: 1 }, expect.any(AbortSignal), false);
    expect(f.outputs().filter(o => o.call_id !== "c1").every(o => o.output.includes("هیچ کاری انجام نشد"))).toBe(true);
    f.call("c4", "app__gh__create", { title: "Bug" }); await flush();
    expect(f.voice.snapshot().approval).toMatchObject({ tool: "app__gh__create" }); expect(f.voice.snapshot().approval?.preview).toBeUndefined();
    f.voice.decide(true); await flush();
    expect(f.run).toHaveBeenLastCalledWith("app__gh__create", { title: "Bug" }, expect.any(AbortSignal), true);
    const ending = f.voice.end(); f.channel.emit({ type: "session.closed" }); await ending;
    expect(setAppTools).toHaveBeenLastCalledWith(null);
  });

  it("puts the runtime's preview on the approval card", async () => {
    const preview = { view: "tasks" as const, kind: "task" as const, action: "create" as const, fields: [{ label: "عنوان", value: "نان" }] };
    const f = fixture({ tools: { prepare: async (_n: string, args: Record<string, unknown>) => ({ ok: true as const, args, summary: "افزودن کار «نان»", preview }) } });
    await f.connect();
    f.call("c1", "add_task", { title: "نان" }); await flush();
    const card = f.voice.snapshot().approval!;
    expect(card.preview).toEqual(preview);
    card.preview!.fields[0].value = "mutated"; expect(f.voice.snapshot().approval!.preview!.fields[0].value).toBe("نان");
    f.voice.dispose();
  });

  it("ignores user speech overlapping the assistant's speech on the audio clock", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    f.said("خب، الان می‌پرسم", "output", 5000);           // span 5000–5050
    f.said("بله", "input", 5200); await vi.advanceTimersByTimeAsync(700);  // within the 300 ms margin
    expect(f.run).not.toHaveBeenCalled();
    f.said("بله", "input", 6000); await vi.advanceTimersByTimeAsync(700);
    expect(f.run).toHaveBeenCalledOnce();
    f.voice.dispose();
  });

  it("ignores a user utterance that repeats the assistant's last 10 s of speech", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    f.said("اگر موافقید بگویید بله", "output", 1000);
    f.said("بله", "input", 5000); await vi.advanceTimersByTimeAsync(700);
    expect(f.run).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(10_000);
    f.said("بله", "input", 20_000); await vi.advanceTimersByTimeAsync(700);
    expect(f.run).toHaveBeenCalledOnce();
    f.voice.dispose();
  });

  it("only counts spoken answers once the UI shows the card", async () => {
    const f = fixture({ visible: false }); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    f.said("بله"); await vi.advanceTimersByTimeAsync(700); expect(f.run).not.toHaveBeenCalled();
    f.voice.setApprovalVisible(true); f.said("بله"); await vi.advanceTimersByTimeAsync(700);
    expect(f.run).toHaveBeenCalledOnce();
    f.voice.dispose();
  });

  it("shows full pretty-printed details for non-planner tools and refuses oversized ones", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "update_settings", { soundEnabled: false }); await flush();
    expect(f.voice.snapshot().approval?.details).toBe(JSON.stringify({ soundEnabled: false }, null, 2));
    f.voice.decide(false); await flush();
    const big = "ب".repeat(9000); // 18 KB of UTF-8
    const g = fixture({ tools: { prepare: async (_n: string, args: Record<string, unknown>) => ({ ok: true as const, args: { ...args, big }, summary: "s" }) } });
    await g.connect(); g.call("c1", "update_settings", { soundEnabled: false }); await flush();
    expect(g.voice.snapshot().approval).toBeUndefined(); expect(g.outputs()[0].output).toContain("بزرگ");
    const h = fixture({ tools: { prepare: async (_n: string, args: Record<string, unknown>) => ({ ok: true as const, args, summary: "s", preview: { view: "notes" as const, kind: "note" as const, action: "create" as const, fields: [] } }) } });
    await h.connect(); h.call("c1", "add_note", { text: "x" }); await flush();
    expect(h.voice.snapshot().approval?.details).toBeUndefined();
    f.voice.dispose(); g.voice.dispose(); h.voice.dispose();
  });

  it("reports the last decision and whether the approved tool succeeded", async () => {
    const f = fixture(); await f.connect();
    f.call("c1", "add_note", { text: "x" }); await flush();
    const id = f.voice.snapshot().approval!.id;
    f.voice.decide(true); await flush();
    expect(f.voice.snapshot().lastDecision).toEqual({ id, outcome: "approved", ok: true });
    f.run.mockResolvedValueOnce({ output: "انجام نشد: x", ok: false } as never);
    f.call("c2", "add_note", { text: "y" }); await flush();
    expect(f.voice.snapshot().lastDecision).toBeUndefined(); // cleared by the next card
    const id2 = f.voice.snapshot().approval!.id; f.voice.decide(true); await flush();
    expect(f.voice.snapshot().lastDecision).toEqual({ id: id2, outcome: "approved", ok: false });
    expect(f.outputs().at(-1)?.output).toBe("انجام نشد: x");
    f.run.mockRejectedValueOnce(new Error("boom"));
    f.call("c3", "add_note", { text: "z" }); await flush(); f.voice.decide(true); await flush();
    expect(f.voice.snapshot().lastDecision).toMatchObject({ outcome: "approved", ok: false });
    f.call("c4", "add_note", { text: "w" }); await flush(); f.voice.decide(false); await flush();
    expect(f.voice.snapshot().lastDecision).toMatchObject({ outcome: "rejected" });
    expect(f.voice.snapshot().lastDecision?.ok).toBeUndefined();
    f.call("c5", "add_note", { text: "v" }); await flush(); await vi.advanceTimersByTimeAsync(60_000);
    expect(f.voice.snapshot().lastDecision).toMatchObject({ outcome: "expired" });
    f.voice.dispose();
  });

  it("records executed tools and shows what was remembered", async () => {
    const record = vi.fn();
    const f = fixture({ tools: { record } }); await f.connect();
    f.run.mockResolvedValueOnce({ output: "به خاطر سپرده شد", ok: true, remembered: "قهوه دوست دارد" } as never);
    f.call("c1", "remember_about_user", { text: "قهوه دوست دارد", category: "preference" }); await flush();
    expect(f.voice.snapshot().remembered).toEqual({ text: "قهوه دوست دارد", at: Date.now() });
    expect(f.voice.snapshot().approval).toBeUndefined(); // non-mutating: no card
    f.call("c2", "add_note", { text: "x" }); await flush(); f.voice.decide(true); await flush();
    f.run.mockResolvedValueOnce({ output: "انجام نشد", ok: false } as never);
    f.call("c3", "get_focus", {}); await flush();
    f.call("c4", "add_note", { text: "y" }); await flush(); f.voice.decide(false); await flush();
    expect(record.mock.calls.map(c => c[0])).toEqual(["remember_about_user", "add_note"]);
    f.voice.dispose();
  });

  it("clips tool outputs to 6000 characters", async () => {
    const f = fixture(); f.run.mockResolvedValueOnce("x".repeat(9000)); await f.connect();
    f.call("c1", "list_notes", {}); await flush();
    expect(f.outputs()[0].output.length).toBe(LIMITS.output);
    f.voice.dispose();
  });
});

describe("live protocol helpers", () => {
  it("splits commentary into ≤480-byte UTF-8 chunks without breaking characters", () => {
    const text = "سلام ".repeat(200);
    const chunks = commentaryChunks(text);
    expect(chunks.join("")).toBe(text);
    for (const c of chunks) expect(new TextEncoder().encode(c).length).toBeLessThanOrEqual(480);
  });

  it("parses the events it uses and rejects malformed ones", () => {
    expect(parseLiveEvent(JSON.stringify({ type: "session.updated" }))).toBeNull();
    expect(() => parseLiveEvent("[]")).toThrow(); expect(() => parseLiveEvent(42)).toThrow();
    expect(parseLiveEvent(JSON.stringify({ type: "session.input_transcript.delta", delta: "x" }))).toEqual({ type: "invalid" });
    expect(parseLiveEvent(JSON.stringify({ type: "session.input_transcript.delta", event_id: "e", delta: "x", start_ms: 5, end_ms: 9 })))
      .toMatchObject({ type: "transcript", start_ms: 5, end_ms: 9 });
    expect(parseLiveEvent(JSON.stringify({ type: "response.event", event: { type: "response.created" } }))).toEqual({ type: "response", active: true });
    expect(parseLiveEvent(JSON.stringify({ type: "response.event", event: { type: "response.completed" } }))).toEqual({ type: "response", active: false });
    expect(() => parseLiveEvent("x".repeat(LIMITS.eventBytes + 1))).toThrow();
    expect(parseLiveEvent(JSON.stringify({ type: "response.output_item.done", item: { type: "function_call", call_id: "c", name: "n", arguments: "{}" } })))
      .toMatchObject({ type: "call", call_id: "c", name: "n" });
  });
});
