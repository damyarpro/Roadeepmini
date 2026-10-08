# Codex live observation fix

Real-log diagnosis (metadata/type counts only): current desktop emits task_started and item_completed(UserMessage), rather than legacy user_message. Pre-existing sessions previously attached at EOF, and 64KiB limits discarded tool results up to 1.1MB. Coding evidence previously drove only the activity view, not the default compact character task.

Native fixes in windows/src-tauri/src/coding: task_started normalization; bounded 2MiB recent bootstrap; validated filename UUID fallback for inherited oversized metadata; 2MiB record limit; 1MiB per-file read and 8MiB total poll budget; newest16 rollouts; daily folders first and historical discovery every30sec (four years/8192 entries); symlink/reparse rejection including cached entries; chronological256-event snapshot so older worker tails cannot evict current parent; clear/disable safeguards retained. No Codex settings, hooks, credentials or prompts changed.

Frontend: new activity/island-adapter.ts aggregates Codex sessions into a dedicated task and connects thinking/working/error/finished to the compact island. Active parent outranks completed worker. It respects active Claude, chat, selected agents and pending approvals. Archived/stale evidence does not animate as running. It cannot execute tools or create/approve a permission. main.ts imports/registers this adapter; global voice owner authorized this two-line change.

Validation: all44 activity TypeScript tests passed; focused4 adapter tests passed again after UTF8 correction. Final native coding run19 passed/1 ignored (includes an unrelated substring-matched computer test); oversizedmetadata, dailypriority and snapshot ordering regressions passed. Actual read-only harness passed against explicitly authorized Codex sessions:16 files,256 retained events; finished6,prompt7,tool353,usage198; expected current root session identity present. No private content printed. git diff --check clean. Root must independently run final current-source tests and actual harness, then build/install combined release.

Actual harness from windows: set ROADEEP_CODEX_TEST_ROOT=C:/Users/Amir/.codex/sessions and optional ROADEEP_CODEX_TEST_SESSION=01a10421-6e06-7c32-b533-4ba74752679e; cargo test --lib coding::tests::observe_actual_desktop_sessions_without_content_output --offline -- --ignored --nocapture.

The plain-browser development preview cannot read native session files (IS_TAURI=false). Real live observation is in the installed/native Tauri app; browser fixtures are never claimed as live proof.
