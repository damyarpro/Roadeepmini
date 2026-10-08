# Codex parity / key warning fix

Local Codex observation previously entered the generic API integration card and displayed an irrelevant missing-key warning. The overview now uses Claude's existing activity ticker for Codex, with a Codex label and no credential check. The defensive direct integration card also presents observed activity instead of a credential action.

Tool start/result phases are shared by the character and details view; active sessions survive long silent tools; terminal cancellation is neutral rather than success. Official Codex0.160.0 turn_started, turn_complete, turn_aborted and standalone error variants are normalized. Error text is not retained. Finite retained step history uses event revisions, so the twenty-step window filling cannot freeze subsequent labels. Session/provider changes reset queued labels.

The details button selects the existing observed Codex session. User explicitly chose that replies and permission decisions remain in Codex. No interactive server, message dispatch, permission approval or Codex configuration change was introduced.

Root verification: TypeScript379 tests48 files; release29 tests3 files; Codex Rust35 tests; read-only actual desktop log harness passed, preserving private content. Counts:16 files,256 retained events; current root session observed. Browser-only conflict fixture verified the overview has Codex ticker and no missing-key message and details selects that same fixture session. Fixture screenshots do not prove native interaction.

Installed CLI public source reference: https://raw.githubusercontent.com/openai/codex/rust-v0.160.0/codex-rs/protocol/src/protocol.rs . The desktop's existing stdio app-server is not a verified interactive transport for this application.
