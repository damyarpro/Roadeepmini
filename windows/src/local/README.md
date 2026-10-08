# Integrated local brain and speech

## Header microphone: live conversation

The header microphone (`global-voice.ts`) starts and ends the app's single GPT-Live
session owned by `src/voice/live.ts`. No wake word, no always-ready mode and no
speaker verification are involved; the local Whisper/Piper path is not used by the
microphone. The button and the compact-island chip render the live snapshot
(phase, latest transcript line, a ring driven by the input/output level).

`voice-host.ts` is the tool host given to `setLiveVoiceHost`: it maps `show_view`
(chat → prompt, planner → today, sessions → activity, integrations → overview), sends
`ask_roadeep` through the chat's `submit(query, true, true)` — the live flag skips the
desktop launcher, admin-voice analysis and voice proposals, so nothing acts without the
live approval — and applies character changes once the character module binds
`bindVoiceCharacter`. A pending approval pins the island on the `voiceApproval` view
(`live-ui.ts` card: summary, countdown, Approve/Decline, the user's latest words); it
unpins when the approval clears. Settings → Live conversation stores the OpenAI key in
Windows Credential Manager and picks the voice and backend model.

The sections below describe the earlier admin-voice path (enrollment, always-ready
mode, name activation). Its modules remain in this folder but the microphone no
longer uses them.

`BridgeLocal` is the typed IPC boundary. Settings explicitly install or cancel
the app-owned runtime and enable it. Status events update readiness and byte
progress; controls remain honest when the installation or enable operation fails.
Advanced provider voice remains separate and requires its configured API key.

For each chat turn, `localRoute` asks native readiness and then the constrained
small-task brain. Attachments and selected agents force the existing API path.
The selected API text model is preserved and used when a request goes to the API;
selecting it does not disable small local replies. Native policy additionally
rejects tool/current-information/coding/complex work locally. Native errors return
an explicit API fallback. Cancellation during verification, inference or a native
failure never becomes an API handoff. No local response runs host tools or approves
permissions.

Recent conversation is bounded to twelve items, 4,000 UTF-8 bytes per item and
12,000 bytes total in one pass over bounded prefixes, without splitting Unicode
characters. Malformed lone UTF-16 surrogates become replacement characters before
native JSON serialization. Local exchanges not yet
seen by the API are sent as labelled untrusted context on the next API request.
The visible user query stays unchanged. Provenance is stored on `ChatMessage`,
so view rebuilds keep local/API/CPU/app distinctions; restored server replies
default to API provenance. A login explanation is an app message, not an API reply.
Simple local requests work while signed out; complex requests explain that login
is needed rather than pretending an API answer was produced.

`LocalVoiceController` owns a turn-based recording (explicit click, at most thirty
seconds), microphone tracks, AudioContext, native request ID and playback. It
encodes mono 16 kHz PCM WAV, rejects silent/nonfinite capture, uses native Whisper,
calls the same chat routing/approval path, then uses native Piper. Playback uses
a validated WAV Blob URL compatible with the existing production CSP, revoked
after completion or cancellation. There is no browser speech-recognition/cloud
fallback and no hidden native file path exposed. Startup capture is allowed only
by the separately persisted, opt-in always-ready administrator voice mode.

End/hide/pagehide/sign-out transition/model-agent change/disable/rebuild closes
capture, cancels native work and cancels an active voice-origin chat. Generations
ignore late permission, AudioContext resume, transcription, chat and speech output.
Cleanup from an older operation cannot close a newer recording or overwrite its
phase. Typed API tool approvals remain in the existing chat implementation;
voice-origin server approvals are blocked and the job is cancelled.

Tests cover byte-bounded routing/context, cancellation before/after native calls,
PCM WAV and resampling, denied/silent/late microphone input, stale resume and
overlapping cleanup, speech Blob lifecycle, settings failure/readiness/progress,
retry and listener disposal. Browser fixtures are explicitly labelled previews;
they do not download models or capture microphones. `dev/chat-mock.html?local=1`
uses fake native routing and denies microphone access; `signedout=1` checks local
chat and the complex-request login explanation without an external API call.


## Admin voice across the app

Assistant settings persists `adminVoiceMode`: `manual` by default or `always`.
Manual handles one verified utterance per click. Always-ready starts after settings
load only when the runtime and administrator reference are ready. The shared
header microphone pauses it for the current app session; unrelated state updates
cannot reactivate it. Missing runtime or admin voice opens Assistant settings.
The microphone owner is outside chat views: navigation, hiding the island and
rebuilding language views do not end listening. App pause, shutdown, runtime
disable, profile deletion and explicit off release capture and cancel work.

Audio segmentation retains at most 300 ms idle pre-roll and accepts at least
250 ms of voiced input, so a short assistant name is not discarded. It finishes
after 800 ms silence and at least two seconds of real captured PCM, or 29 seconds.
For a short name, the additional duration is the actual microphone quiet tail;
no voiced samples are repeated or fabricated. One utterance runs at a
time. Capture stops during speaker verification, transcription, reply and speech
playback, preventing synthetic replies feeding back into recognition. A rejected
speaker rearms only in always-ready mode, without transcription or cloud dispatch. Matching requires the
native one-use speaker ticket tied to the exact WAV and current voice profile.
Existing chat submission handles local/cloud routing, history and approvals.

Speaker rejection now displays a four-second localized reason (insufficient
audio, uncertain match, different speaker, missing reference, or enrollment in
progress); it never displays scores, profile data or arbitrary backend text.
Native logs record only allowlisted reason codes. Whisper receives explicit
`-l fa` or `-l en` from the saved app language instead of detecting the language
of a short name. The pinned CLI's language option is validated against those
two values. An unrecognized transcript produces recoverable feedback.

The name trigger still depends on transcription and the existing speaker match.
Synthetic pinned Piper/Whisper/CAM++ characterization did not reliably recognize
or verify the isolated Persian proper name. This is a model/fixture limitation,
not permission to accept unverified commands or add fuzzy word aliases. The
ordinary Persian speech roundtrip and controller security tests are separate
from the diagnostic wake characterization. A full sentence gives the speaker
matcher more context; actual microphone quality is not proven by these fixtures.

Settings enrollment explicitly acquires 12 seconds of local audio after native
speaker_enrollment_begin pauses global listening. Only the native embedding is
persisted; frontend raw audio is discarded after submitting. Cancellation,
permission denial, window removal and pagehide release audio and end the native
lock. Listening intent resumes after enrollment if the admin profile still
exists and the user has not switched voice off or disabled the runtime.

The admin voice fixture at dev/admin-voice-preview.html has scene=missing,
recording,verifying,thinking,speaking,error,idle and theme=light. It never obtains
microphone permission or invokes inference. New tests cover VAD bounds, speaker
rejection/ticket/cancellation, global navigation/profile/runtime races, enrollment
permission cancellation and bounded synthetic enrollment capture.

The live microphone meter measures bounded PCM RMS/peak and updates at most ten
times per second, independently of the application's layout notifications. The
displayed meter uses RMS; its level can rise and fall and is distinct from the
received twelve-second enrollment progress. Silence is labeled explicitly.
Suspended audio startup and missing callbacks produce recoverable errors and
release capture. Enrollment never submits a partial wall-clock recording.

Administrator reference JSON is stored as bounded raw UTF-8 secret bytes in
Windows Credential Manager. Compatible older UTF-16 password references can be
read without rewriting them. Tests cover the actual Windows blob size regression
and an explicitly invoked isolated credential roundtrip; they never access the
administrator reference or activate a microphone.

## Voice intent, proposals and safe tools

Explicit anchored task/note requests are recognized before inference. Colloquial
Persian «تسک بزار/بذار»، «کار اضافه کن» and «یادداشت کن» preserve the bounded body
verbatim for review; a missing body returns a concise `reply` clarification even
in always mode. Unrelated ambient speech remains silent. Analysis/capture errors
reveal persistent chat feedback rather than disappearing in the microphone title.
Advanced cloud voice and isolated computer controls are mounted only in Settings;
the small administrator microphone remains global. Settings permits only the
existing remote session and proposal analyze/decide contracts, not local STT,
local brain or direct host desktop dispatch. Proposals are shared across windows
and still require an explicit confirmation.

Verified speech is classified by the installed local model through bounded JSON
(`none`, `task`, `note`). Always-ready ignores unrelated remarks; addressed chat
and exact fixed desktop commands use the normal safe voice routing. Manual voice
also analyzes for proposals before chat. Analysis never saves a planner item.

Native state holds one proposal with a UUID, kind, full text and five-minute TTL.
The shared chat card offers Save/Reject. Exact contextual yes/no speech can decide
only that proposal. A synchronous transactional write under the proposal lock
prevents duplicate saves; a failed write retains it for retry. Off, profile removal
and runtime disable invalidate generations so late inference or tool callbacks
cannot restore a cleared proposal. Proposed dates remain text, not inferred schedules.

Fixed desktop destinations are This PC, Downloads, Documents, Windows Settings,
Calculator and Notepad. Native dispatch uses trusted Windows executables and fixed
arguments; arbitrary shell commands, paths and URLs are not accepted. Dispatch
success does not assert completion of work inside another application.

Voice API turns use a fresh standard-model thread, an explicitly restricted
builtin catalogue and no file uploads, inherited agent instructions or external
MCP discovery. Execution rechecks native tool identity. Task/note tools return a
pending proposal rather than writing. Sensitive server approvals cancel the job
without approving it. Typed conversations retain their existing thread and tools
preference. Complex requests need the configured Roadeep account and connection.

Fixtures use synthetic PCM and mocked IPC only. Unit and integration checks cover
mode migration, startup/pause, cancellation, silent replies, proposal expiration,
save failures, concurrent decisions, denied tools, and typed chat isolation. Explicit
model/speaker smoke checks use public or synthetic inputs, never administrator audio.

## Name activation

After administrator verification and local transcription, a strict name-prefix
parser recognizes `رودیپ`, `رو دیپ`, `Roadeep` and the bounded Persian P/B spelling
variants emitted by the pinned recognizer. It does not match substrings, unrelated
words or arbitrary fuzzy names. The original command body is preserved. A name
with a command routes that command as addressed speech. A standalone recognized
name replies locally and opens one 20-second follow-up window; it does not call
the intent model or cloud API. Manual mode remains listening for that one follow-up.
Off, pause, enrollment, language/mode changes and page exit cancel the window.

VAD accepts at least 250ms voiced input and captures a real quiet tail until the
unchanged two-second speaker-input minimum is reached. No production samples are
invented or repeated. Short/uncertain/non-admin speech never bypasses matching.
Rejection and unclear transcription expose a four-second allowlisted notice,
including after automatic rearming. The saved UI language selects Whisper `fa`
or `en`; no untrusted language or vocabulary text becomes a process argument.

This activation flow does not establish acoustic accuracy. Actual synthetic
Piper/Whisper experiments missed isolated names and some full phrases; CAM++
rejected sparse names while accepting a full synthetic administrator phrase.
Human microphone accuracy remains unverified. A full phrase containing the name
provides more context, but reliable detection of the name alone remains unresolved.
