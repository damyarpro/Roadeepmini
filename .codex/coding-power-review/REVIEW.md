# Coding power and chat models — final review

The authorized batch adds account text-model selection in chat, opt-in structural coding history, observed Codex context/quota, explicit read-only Git inspection and explicit CLI terminal handoff. Existing desktop island tokens and character identity remain the baseline. Source projects supplied by the user were reference material; no third-party application or code was imported.

## Independent visual review

Three fresh `ui-critic` agents reviewed rendered screenshots and implementation at desktop widths 768, 1280 and 1440, at native island scale (`zoom=1`). The product is a fixed 640px desktop island, not a mobile page. All final criteria reached 9/10; no Critical or Major defects remained.

| Criterion | Iteration 1 | Iteration 2 | Final |
|---|---:|---:|---:|
| Hierarchy | 8 | 9 | 9 |
| Spacing | 7 | 9 | 9 |
| Typography | 8 | 9 | 9 |
| Color/contrast | 9 | 9 | 9 |
| Consistency | 7 | 9 | 9 |
| Interaction states | 8 | 8 | 9 |
| Usability | 8 | 9 | 9 |
| Responsiveness | 9 | 9 | 9 |
| Accessibility | 7 | 9 | 9 |
| Distinctiveness | 9 | 9 | 9 |

1. Grouped Git/handoff controls; preserved keyboard focus across live updates with regression tests; clarified usage weights and retry feedback.
2. Removed stale catalog failure feedback after successful retry; added a regression test. History copy now explicitly excludes raw prompts, commands, outputs and diffs; archive fixtures use the actual structural restore path.
3. All criteria reached 9; final audit accepted. Minor polish suggestions remain: a 3px picker gap, dense expanded history path text, and native disabled styling on retry. These do not block the scoped desktop flow.

Final screenshots: [Persian chat, 768px](iter3-chat-768.png), [Persian activity, 1280px](iter3-activity-1280.png), [English historical chat, 1440px](iter3-chat-1440.png), [Git](iter3-git.png), [history](iter3-history.png), [terminal preparation](iter3-handoff.png). All account, project and activity values in screenshots are fabricated fixtures, never personal transcripts.

## Final verification

- Orchestrator ran the full TypeScript suite: 262 passed across 32 files.
- Orchestrator ran the full Rust library suite: 399 passed, 4 existing ignored, 0 failures.
- Typecheck and Vite production build passed; `git diff --check` passed (line-ending notices only).
- Browser checks verified model change, historical model labeling, pinned/busy locks, catalog failure/retry controls, observed usage, structural archive, explicit Git inspection and two-step terminal preparation/launch feedback using fixtures.
- Native fixture tests exercised corrupt/expired archives, clear/disable races, Unicode wire bounds, helper-contained Git subprocesses, staged/unstaged/unborn repositories and a mock terminal launcher. No actual coding agents were launched for tests.

Native UI automation is unavailable in this environment. Live account message submission and a real CLI continuation were not performed; model catalog and chat behavior use the existing account API plus tested native contracts. The release executable is built and its process launch is verified separately.

Release build succeeded (`npm run tauri -- build --no-bundle`, optimized Rust build 3m31s). Updated `D:/Roadeep/Roadeep-mini/windows/target/release/Roadeep.exe` launched on 2026-10-04 09:40:13 local time; PID 9796 was verified alive and responding.

User guide: `windows/CODING-ACTIVITY.md`. Native contract notes: `windows/src-tauri/src/coding/README.md`.
