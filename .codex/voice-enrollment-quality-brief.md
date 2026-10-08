# Enrollment quality follow-up

After installing0.1.3, user attempted registration: live level moved and received duration reached12 seconds; displayed registration verification failure. Safe local metadata records a663ms native enrollment failure. Capture is proven by user's direct report; rejection reason is inferred from the exact frontend message and nested native operation, not yet logged.

local_engine owns native speaker_worker.rs/speaker.rs and related tests: reproduce wall-clock three-way splitting with public same-speaker audio and boundary pauses; only adopt a signal-selection change supported by evidence. Keep speaker calibration/matching thresholds and ticket authorization unchanged. Log only allowlisted rejection codes, never audio, vectors or arbitrary error content. Do not touch administrator credentials or microphone.

local_assets owns messages.ts and message tests only: distinguish inconsistent samples, insufficient speech and short duration with concise actionable Persian/English copy. No capture, duration, meter or layout changes.

Root audits evidence, runs actual pinned model tests with public fixtures, then the necessary full native/frontend/release gates, builds and installs the complete offline patch. Existing UI loop has passed; label-only changes do not require redesign. Preserve all other edits. Test realistic pause/silence boundaries, insufficient signal, different speakers, invalid data and old profile compatibility.
