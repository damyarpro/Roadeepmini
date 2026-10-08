# Roadeep local coding activity implementation

User authorized implementing recommendations from three-project report; immediate scope Dotpals recommendations: Codex session observation, compact diff/test freshness/evidence recap, advisory overlapping edits, portable Markdown handoff, Canvas appearance customization. Do not import whole projects, Electron/Node bridge, remote AI checker, documents platform or new permission system.

Architecture: local Rust observer emits `coding-activity` events and exposes bounded snapshot. Frontend pure bounded reducer supplies activity view. Existing Claude approvals and relay data minimization remain intact. No automatic approvals, shell execution, settings installation, network calls, raw transcript persistence. All observations are evidence, not proof of correctness. No new dependencies unless justified.

Wire contract camelCase:
```
interface CodingEvent {
 id: string; sessionId: string; harness: 'codex' | 'claude';
 at: number; // UTC epoch milliseconds
 kind: 'session' | 'prompt' | 'tool' | 'finished' | 'error';
 cwd?: string; title?: string;
 tool?: string; callId?: string;
 phase?: 'started' | 'completed' | 'failed';
 command?: string; output?: string; exitCode?: number;
 files?: string[]; patch?: string;
}
```
IDs stable, no secrets in events/logs. Validate shape/length at both boundaries. All fields optional only if unknown. Never synthesize successful exit code for unknown results. Invalid JSON lines safely ignored with bounded contextual counters/logs; never log raw content. Session paths are display-only, no execution. Treat tool output/patch as untrusted text; render textContent. Tests use fabricated fixtures, never personal logs.

Backend commands: `coding_snapshot() -> CodingEvent[]`, `coding_clear() -> ()` clearing bounded buffered evidence, `coding_status() -> {enabled:boolean, available:boolean, error?:string}`. Settings `observeCodex: boolean` default false, live apply by existing save_settings. Opt-in UI clearly states reads local Codex logs; no cloud. Snapshot replay must not announce historic notifications. Disabling clears observer buffers + frontend evidence, blocks race rehydration. Observer bounded recent file discovery, chunks/line limits/session cap, truncation/rotation handling, no replay full historical logs, cancellation on disable. Quiet observer background work independent of hidden rendering; avoid tight polling.

Ownership:
- backend worker: windows/src-tauri/src/coding/** (new), lib.rs registration/setup, settings.rs observeCodex only; own backend tests/docs in coding. No frontend/hook changes.
- evidence worker: windows/src/activity/{types,store,tests,diff,test-output,recap}.ts and tests (new), ACTIVITY.md docs. Own pure frontend model, no UI/global state changes. Exports CodingEvent, ActivityStore class + singleton Activity, normalizeEvent, diffPreview, parseTestOutput, recapMarkdown. Store API ingest(event), clear(), subscribe(listener), sessions() sorted snapshot. Session shape export/document for UI with id,harness,cwd,title,status,events,changedFiles,lastPatch,test verdict+freshness,conflicts, counts. Coordinate exact model with UI worker promptly. Need call pairing, dedupe IDs, unknown vs failed tests, latest mutation invalidates tests, bounded sessions/events/text, advisory normalized paths for Windows, absence of attribution certainty, no arbitrary elapsed-time based proven conflict. Include robust tests failures/late output/truncation/zero/skipped/secret redaction/malformed.
- UI worker: frontend wiring, Bridge methods, Settings TS observeCodex, main.ts, activity view/CSS, views.ts/layout/palette/locales, island integration as needed, browser dev activity preview. Own no evidence pure modules/backend. Preserve existing source changes animation. Add accessible menu entry `activity`, session switcher, empty/off/privacy/status states, diff/test/recap/export/copy handoff/clear controls, explicit monitor toggle via settings or activity view. Integrate `coding-activity` subscribe before snapshot, dedupe, clean pause/off; add Claude status observations from hooks WITHOUT changing approvals (no command outputs currently available: show unknown tests). Don't falsely show evidence for Claude outputs. Snapshot errors visible but no raw log. Do not hijack user focus on every tool event. Work with fixed640px island using scroll and keyboard; light/dark not needed desktop dark tokens. All strings fa/en. Add Canvas customization second phase after UI stable; request ownership expansion before editing engine/settings.rs.

Workers are not alone in codebase. Never revert others' edits. Coordinate contracts via messages. Existing animation engine/motion files, island ensureRunning fixes, gallery must remain. Read architecture before implementation. Do not copy third-party code unless licensing notices added; prefer fresh implementation based on patterns.

Main orchestrator reviews diffs/contracts independently, runs TS tests/build + cargo test/check, browser feature checks, minimum3 fresh ui-critic iterations per user AGENTS. No false claims of passing production tests from static sources. Can fix blocking defects itself per AGENTS if needed, otherwise route changes to owner.

Design brief: compact desktop companion for Persian developers, quiet local session evidence, primary action selecting session/reviewing last change/test freshness. Existing dark island, existing typography/font/token colors/radii. Fit640px width <=440px height, scroll inner content; code LTR isolates, copy/status accessible, keyboard focus visible. Empty/off/error states intentional. No second design system, externalfonts, unnecessary deps.
