# Voice recording follow-up

User reports recording is broken while Codex parity/settings release is building. Diagnose actual capture, permission, audio processing and enrollment lifecycle before changes. Clarification of exact symptom is pending. Preserve previous changes and voice data; do not activate a real microphone or overwrite the administrator profile during tests.

Ownership: local_assets owns frontend capture in src/local/voice.ts, enrollment.ts, global-voice.ts and related tests/messages. local_engine owns native voice/local_intelligence/local_runtime and native WebView permission integration after coordination. Root audits source, runs final gates, rebuilds and installs the complete offline package. Workers are not alone; do not revert others. Report concrete diagnosis before editing while the existing build runs.

User clarified: speech seems not received; add a live sound meter and filling recording feedback. Meter must use actual PCM RMS, stay bounded and throttled, and reset during processing/off. Keep level updates out of the whole-app layout notification path. Enrollment progress measures received PCM duration, not a fake wall-clock animation. Preserve speaker verification thresholds.

Native diagnosis: 512-dimensional profile JSON fits raw UTF8 but exceeds Windows2560-byte credential limit after keyring password UTF16 encoding. Store raw secret bytes in the same secure entry; load legacy password format compatibly. Test only isolated generated credentials, never the administrator reference.

Success: start and cancel never hang; missing/denied/unavailable input is actionable; bounded capture reaches processing or a truthful error; hidden windows/navigation cannot silently leave a dead listening state; late callbacks cannot revive capture. Synthetic media tests cover failure and cancellation. Browser preview must clearly distinguish native-only capture from installed functionality. No private audio/transcripts in logs. No new cloud service or credential requirement.
