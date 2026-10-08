# Live voice (GPT-Live)

The header microphone starts and stops one full-duplex GPT-Live conversation over
WebRTC. There is no wake word, no "always listening" mode and no local
Whisper/Piper path behind this button, and no speaker verification.

## Pieces

| File | Role |
| --- | --- |
| `live.ts` | `LiveVoice` (one session's lifecycle) and the app singleton `liveVoice()` |
| `gate.ts` | Nearest/loudest-speaker noise gate: pure `gateStep()`, `GateKernel`, `buildGateWorklet()` (audio thread) and the main-thread fallback `buildGate()` |
| `gate-worklet.ts` | AudioWorklet processor, loaded with `?worker&url` (bundled, same origin, so CSP `script-src 'self'` allows it) |
| `approval.ts` | `matchApproval()` (Persian/English yes/no) and `ApprovalBox` (one pending card, 60 s) |
| `schema.ts` | Loads `tools.json`, validates model arguments against its JSON-schema subset |
| `tools.ts` | `createToolRuntime(host)`: every tool in `tools.json` on the existing bridges |
| `tools.json` | Tool declarations; the native gateway sends the same file to the provider (`include_str!`). Owned by the manager |

## Session lifecycle (`LiveVoice`)

1. `getUserMedia` with `echoCancellation`, `noiseSuppression`, no `autoGainControl`,
   mono. The mic goes through the gate (highpass ≈90 Hz → gain → MediaStreamDestination);
   only the gated track is sent.
2. `RTCPeerConnection`, data channel `oai-events` created **before** the offer,
   full ICE gathering, then `voice_start(sdp)` (Rust posts `/v1/live/sessions` with
   the key from Windows Credential Manager and returns only `{id, sdp}`).
3. Remote answer installed, then wait for `session.started` (≤15 s). Session is
   capped at 10 minutes.
4. `end()` sends `session.close` and waits ≤15 s for `session.closed`, then
   releases mic, gate, AudioContext, peer, channel, `<audio>`, timers, the pending
   approval, and calls `voice_end(id)`.

Every await is bounded (mic 30 s, audio 5 s, offer/ICE 10 s, creation 30 s,
started 15 s). Every failure path releases everything and leaves phase `error`
with a Persian `error`; `start()` never rejects and is a no-op while a session is
active or closing. A microphone grant or a provider answer that arrives after
cancellation is stopped/cleared. Malformed events are dropped (more than 20
end the call); provider `error` events are logged by code only and end the call
only when session-level (session/auth/quota). `response.create` waits for an
active delegated response to finish (≤8 s). Connection `failed` ends the call;
`disconnected` ends it after 10 s unless it reconnects. A closed data channel, an
ended mic track ends it too.
Blocked autoplay shows a notice and retries on the next click/key
(`resumeAudio()` does the same on demand). `dispose()` is the synchronous
pagehide/shutdown cleanup.

Events used: `session.started`, `session.closed`, `session.input_transcript.delta`,
`session.output_transcript.delta`, `error`, and function calls from
`response.event` → `response.output_item.done` (`item.type === "function_call"`).
Event ids and call ids are deduplicated.

Snapshot (`subscribe` calls back immediately and on every change):
`phase` (`idle | connecting | listening | thinking | speaking | closing | error`),
`muted`, `error`, `notice`, `transcripts`, `approval`, `inputLevel`, `outputLevel`
(0…1, gated mic and remote audio). `speaking` comes from the remote audio level,
`thinking` while a tool runs or an approval waits.

`toggleMute()` disables the tracks and sends `session.input_audio.mute|unmute`.
`interrupt()` silences playback until the interrupted answer has been quiet for
500 ms and sends `session.instructions.append` (`delegation_id: null`).

## Tools and approval

Arguments are untrusted: `parseToolArgs` JSON-parses and validates them against
`tools.json` (type, enum, min/max, length, pattern, required, no extra keys)
before anything runs. Unknown tools and invalid arguments are answered with a
"nothing was done" output.

* Non-mutating tool → run → `response.item.create` (`function_call_output`, ≤6000
  chars) → `response.create`.
* Mutating tool → `prepare()` resolves fuzzy titles and checks values (an
  ambiguous title returns the candidates and asks which, without a card) → an
  approval card `{id, tool, summary, expiresAt}` appears in the snapshot and the
  voice is asked, through `session.commentary.append` (≤480-byte chunks), to ask
  the user. Approved → run. Rejected / 60 s expired → an output saying the user
  did not approve. Then `response.create`.
* Only one approval at a time; another mutating call meanwhile is answered with
  "another request is waiting for approval".
* The **app** decides, never the model: the card's buttons (`decide(approve)`) or
  the user's own input transcript said **after** the card appeared. Once the
  transcript has been quiet for 600 ms, `matchApproval` reads it: whole words only,
  ی/ي ک/ك, diacritics, ZWNJ and digits normalised, negations such as «انجام نده»
  or "don't do it" win over the affirmative inside them, both answers together
  keep waiting; an unclear utterance is forgotten so a later clear one decides.
  The assistant's own transcript is never read; user speech that started before
  the card (session audio clock, `start_ms`) or within 600 ms of audible assistant
  output (echo) is ignored; the spoken question (`approvalQuestion`) contains no
  yes/no word. `decide(approve, id?)` ignores a click on a stale card.

**Echo and visibility.** The spoken question is one fixed sentence
(`APPROVAL_QUESTION`): no argument value, no yes/no word. A user delta is ignored
when its `[start_ms, end_ms]` overlaps an output-transcript span ±300 ms, and a
settled utterance is ignored when its words repeat the last 10 s of the
assistant's transcript. Spoken answers count only while the UI reports the card
on screen (`setApprovalVisible(true)`, reset for every new card); clicks always
count. **To verify on a real session:** that input and output `start_ms`/`end_ms`
share one session audio clock (the span and before-the-card checks rely on it).

**Card fields.** `summary` is one short line; `details` holds the full
pretty-printed arguments (≤16 KB, never truncated; larger calls are refused) for
every tool without a planner `preview`. `snapshot.lastDecision`
(`{id, outcome, ok?}`) keeps the last card's outcome until the next card; `ok`
is set once an approved tool ran (a runtime may return `{output, ok}`).

**Vocabulary.** `matchApproval(text, {action?})` knows formal and colloquial
Persian and English answers (table-tested), collapses stretched letters, lets
negations win over their roots, returns null for mixed answers and questions
(«انجام بده یا نه؟»), counts «پاکش کن» only for a delete, «اوهوم» only alone and
«صبر کن» only when nothing else was said. «تأیید» and «رد» (the button labels)
always work.

**Notes and memory.** `list_notes` (word query, newest first, ≤20),
`update_note`/`delete_note` (fuzzy, previews with `targetId`), and the local user
memory: `get_user_profile`, `remember_about_user` (sets `snapshot.remembered`),
`forget_about_user`. Every successfully executed tool is recorded with
`memory_record(tool, summary ≤120)` in the background; failures are logged
without content.

**App tools.** `voice_start` also returns `appTools` (`{name: "app__…", title,
server, readOnly}`, validated, ≤60) for MCP servers, desktop and computer tools.
They are known only for that session (`setAppTools`, cleared at the end), their
arguments are checked as a JSON object ≤16 KB (schemas live natively),
read-only ones run immediately, others need approval; both go through
`voice_app_tool(name, arguments, approved)`.

**Previews.** Planner changes (add/complete/delete task, add note, add reminder,
log habit, focus control) carry `approval.preview` (`LivePreview`: view, kind,
action create/update/delete, `targetId` of the resolved row, label/value fields)
so the island can show the draft in place on that page.

Tool outputs are short Persian text or compact JSON. Tool failures become a
Persian sentence; nothing is thrown to the model. `ask_roadeep` goes through the
normal chat (`host.ask(query, signal)`), times out after 90 s and is aborted when
the session ends; its answer is labelled as data.

`createToolRuntime(host)` needs a `ToolHost` (`showView`, `ask`, `setCharacter`,
`characterOptions`); the island registers it with `setLiveVoiceHost()`. Settings
changes copy `State.settings`, apply the validated patch (planner bounds, eye
motion on/off ↔ `normal`/`still`, web search and reasoning exclusive) and save
with `Bridge.saveSettingsChecked`.

## Tests

`npx vitest run src/voice` — fake media, peer, channel, audio graph and timers;
no real microphone, network or paid API. Real-device verification needs the
user's OpenAI key, network access, microphone permission and WebView2.
