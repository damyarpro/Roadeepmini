# Roadeep name trigger — investigation and verification

Date: 2026-10-05. User requested a spoken «رودیپ» trigger because voice does not understand commands.

## Observed causes

The original continuous detector discarded utterances shorter than 1.2 seconds of voiced audio. A short name could therefore disappear before speaker verification or Whisper. Speaker verification additionally requires at least two seconds of real PCM. The installed app's recent metadata contained three rejected speaker checks, with no transcription afterward. This establishes another failure point; it does not establish that speech recognition itself failed. No personal audio, speaker embedding or biometric profile was read.

The original native name check only routed addressed speech after intent inference; it did not acknowledge a standalone name or open a follow-up request window. The explicit task parser also did not remove the name prefix. Whisper previously used automatic language selection for short Persian clips.

## Intended fix contract

Capture a short voiced name with bounded real quiet context, preserving the native two-second audio minimum and unchanged speaker threshold/tickets. A matched administrator name gets a local acknowledgment without an API or intent model. The next verified utterance has a 20-second, single-request window. Name plus command in one sentence strips the name before normal safe analysis. Cancel/pause/profile/mode/language changes clear the window. Rejected speakers never execute commands and receive a bounded visible status.

Existing ambient task/note proposal behavior and explicit save confirmation remain. The wake word is activation context, not permission to save items or perform sensitive actions.

Worker ownership and verification instructions: [brief](voice-wake-brief.md). Final tests, installed release and exact limits will be recorded below after root review.

## Delivered behavior and verification

The implementation now recognizes an exact normalized name prefix (رودیپ, رو دیپ, Roadeep and observed رودیب variants), acknowledges a verified standalone name locally, and arms a single follow-up request for 20 seconds. A name mentioned in the middle of a sentence does not activate this path. Capture retains real quiet context around short speech; it never fabricates speech or relaxes administrator authentication. Saved Persian/English language is passed explicitly to Whisper. Speaker rejection and unrecognized speech have visible localized status. No database or public API changes are required.

Root verification passed: TypeScript; 497 frontend tests across 58 files; 29 release-staging tests; 479 native library tests (18 opt-in tests ignored) and 20 MCP tests. The native HTTP test fixture needed a one-line Windows correction to restore blocking mode on accepted sockets before its timeout; production HTTP behavior was unchanged. Actual bundled-engine baseline speech synthesis/transcription passed, with transcription errors. Synthetic CAM++ verification accepted a complete administrator phrase and correctly rejected the sparse standalone name. Only synthetic audio and a synthetic profile were used.

## Unresolved acoustic limitation

These passing gates do **not** prove that speaking the isolated name wakes the application. Actual Piper-to-Whisper trials transcribed the name incorrectly (for example «باری»), and the unchanged speaker verifier rejected a sparse standalone name. Full phrases sometimes yielded «رو دیب» but also incorrect prefixes. The wake parser is implemented; reliable acoustic name detection is unresolved. No unreliable vocabulary prompt or weakened speaker threshold was shipped. A full sentence beginning with «رودیپ» provides more context, but is not guaranteed to transcribe correctly. Human microphone recognition has not been verified in this session.

Capture details and negative acoustic results: [worker report](wake-capture-report.md). Preview-only feedback evidence: [waiting](wake-review/wake-waiting.png), [speaker rejection](wake-review/speaker-rejected.png). These screenshots do not claim a real microphone test.

Release 0.1.6 installation and model receipt verification are recorded in [release evidence](wake-review/release-verification.json) after installation.

Installation completed successfully (installer exit 0). Installed version 0.1.6 is responding and its executable matches the release after the NSIS bundle marker is accounted for. All 518 bundled runtime receipt files match their expected sizes/hashes; installed hook relay and rolling/versioned installer also match. Installer: `windows/release/Roadeep-Windows-0.1.6-setup.exe`, 1,641,045,004 bytes, SHA-256 `90457e859ff41c4ba94c037f1d02c53d31b083e12cbd9a0bc50f6a7ad3fd9ef5`.
