# Roadeep 0.1.5 — coding hooks, voice actions and chat controls

## Delivered behavior

- Text model selection is now a compact app button with a searchable popover, current selection marker, keyboard navigation and a contextual `!` explanation. Catalogue failures can be retried; saving is visible and conflicting actions are disabled. Switching models still starts a new conversation and retains the previous conversation in history.
- Advanced cloud voice and isolated computer controls are available in Settings. They no longer occupy the chat. The small global admin-voice microphone remains.
- Settings and Feedback were removed from the shortcut palette. The top settings gear remains.
- Explicit Persian task/note commands are handled before inference. Complete commands show a reviewable proposal; incomplete commands ask for the missing content. Analysis/capture errors and replies are visible. A pending proposal is revealed instead of silently replaced. Saving still requires an explicit click or exact contextual confirmation, with admin voice verification retained.
- Direct coding hooks carry their true provider identity to the character, activity timeline and integration status. Pausing Codex log reading retains direct hook events. Hook/log tool events are deduplicated only when actual call identity matches; richer observed evidence is preserved.
- Settings provides consistent hook status, user/project scope, contextual help, preview, backup, install and removal. The native writer preserves foreign configuration and rejects a stale preview, malformed config or unsafe path.

## Supported integration scope

Native adapters: Claude Code, Codex, Gemini CLI, Cursor, Windsurf / Devin, Copilot CLI, VS Code Local, Kiro and documented OpenCode V2 plugin API. Cline and Zed have no implemented adapter and are displayed honestly as unavailable.

Claude and Codex permission requests can show an explicit approval card. Other adapters observe documented events without permission grants. Upstream-only permission notifications explain where the user should respond. No hook automatically approves a command or sends a response to a foreign chat.

OpenCode was verified against a strict, explicitly labeled documented-SDK fixture and isolated mock transport. A matching installed host was unavailable, and its public V2 documentation differs from current development SDK context; compatibility with every host release is not claimed. Kiro targets IDE 1.x / CLI 3.x, and VS Code Local hooks depend on its local hooks feature. Hosts may need a fresh session to load changed definitions.

Codex additionally requires upstream trust of the exact installed definitions through `/hooks`. Roadeep does not bypass that trust step. The existing current-session log observer remains the fallback for already running sessions.

## Verification and release

Root independently ran TypeScript, all 457 frontend tests, the full native library suite and 20 MCP tests, 5 relay tests, 29 release/staging tests, the actual pinned Qwen voice-intent test, and the OpenCode generated-template check. Tests used synthetic speech text, metadata payloads and temporary configuration. Actual microphone capture, personal speaker profile and paid API calls were not used in the checks.

Three fresh visual critics reviewed successive builds. Every final criterion scored 9/10 with no Critical/Major defect; root also corrected the remaining minor hover/focus findings. Screenshots, scoring and browser behavior checks: [UI review](coding-hooks-review/RESULT.md).

The full offline installer includes the pinned Qwen brain, Whisper speech recognition, Persian Piper speech, CAM++ speaker recognition, runtimes and licenses. [Installed binary / runtime / installer verification](coding-hooks-review/release-verification.json) records the final version, byte count, hashes and 518 verified runtime files. The user-specific Codex configuration setup outcome is recorded in [local hook setup](coding-hooks-review/hook-setup-result.json).

Implementation contracts, safety decisions and primary provider references: [native coding hooks report](native-coding-hooks-report.md), [coding activity guide](../windows/CODING-ACTIVITY.md), [local voice guide](../windows/src/local/README.md), [release notes](../CHANGELOG.md).
