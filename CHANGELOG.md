# Changelog

## 0.1.8 — 2026-10-09

Windows
- Antigravity: install its hooks from Settings. Permission requests are never allowed automatically.
- Answer Claude Code's questions from the island: single and multi-select, one step per question.
- The permission card waits for you and can be folded away; it keeps its agent pill in front.
- Clicks on a fresh permission card are ignored for 600 ms, so a stray click cannot approve.
- Live diff: finished edits appear as ticker steps (file, added and removed lines), with a diff card and Claude's final line.
- Customisable shortcuts in Settings → Shortcuts. The chat shortcut (Ctrl+Alt+R) is unchanged; the new ones start off.
- Open terminal brings the session's own window forward.
- The ticker no longer stops at a session's twentieth step; the auto-close delay applies at once; stale Stop timers are cancelled; animations pause in hidden views.
- The hook relay accepts larger edits, so diffs of big files are kept.
- Security: source-map-js 1.2.2 (GHSA-68fv-2mgg-jv7q). A symlinked settings.json is written through to its target.

Mac
- The auto-close setting is respected; a web-search reply shows its full answer.
- settings.json is never rewritten from an unreadable or unexpected file; a dated backup is taken first.
- Release builds no longer include the debug entitlement.
- The chat persona is Roadeep and uses no hardcoded user name.

## 0.1.6 — 2026-10-05

- Verified administrator speech recognizes an explicit Roadeep name prefix, removes it before task/chat handling and acknowledges a standalone recognized name locally. A 20-second one-request window supports a separate follow-up in manual and always-ready modes.
- Short spoken input now retains real microphone context instead of being discarded by the old 1.2-second voiced minimum. Speaker verification thresholds and one-use audio tickets are unchanged.
- Rejected speakers and unclear transcription show bounded, localized feedback. Native logs record allowlisted rejection reasons without audio, transcript or biometric scores.
- Whisper uses the saved Persian/English language explicitly. Exact name recognition remains limited by the installed speech/speaker models; synthetic isolated-name tests did not establish reliable wake accuracy. No vocabulary prompt or relaxed identity check ships.

## 0.1.5 — 2026-10-05

- Direct coding hooks for Codex, Claude, Gemini CLI, Cursor, Windsurf, Copilot CLI, VS Code Local and Kiro; a project plugin for the documented OpenCode V2 SDK. Provider identity is preserved in activity, the character and permissions.
- Connection settings share reviewed install/remove diffs, fingerprints, dated backups and safe ownership checks. Claude/Codex approval answers require explicit clicks; other adapters never grant permissions. Codex's own hook trust still applies.
- Direct Persian task/note commands produce reviewed proposals without depending on intent inference; incomplete commands ask for clarification and voice failures remain visible.
- Advanced cloud voice and computer controls live in Settings; chat keeps the small administrator microphone. Settings/feedback duplicates were removed from the command palette.
- Chat model selection uses a searchable keyboard-accessible popover with contextual help and visible persistence feedback.

## 0.1.4 — 2026-10-05

- Assistant settings offers manual voice and opt-in always-ready listening. Manual handles one utterance; always-ready listens across the app, with an explicit microphone pause.
- Verified administrator speech is analyzed locally for task and note proposals. Proposals show their text and require Save/Reject or an exact contextual spoken decision before writing to the planner; unrelated ambient speech remains silent.
- Voice requests can open fixed Windows destinations including This PC and use a restricted builtin planner catalogue. Unknown tools, file uploads and sensitive server approvals are blocked for voice turns.
- Voice cloud conversations use isolated standard-model threads without changing the typed chat's agent, files or thread. Failed launches and proposal saves give truthful retry feedback.
- Speaker enrollment trims quiet recording boundaries before independent reference windows, preserving verification thresholds.

## 0.1.3 — 2026-10-04

- Settings opens as a grouped dashboard with individual panes, retained input state, keyboard navigation and contextual help behind the ! button.
- Existing local Codex sessions use the same home activity ticker as Claude without requesting an API key. Active tools survive quiet periods; cancellation and subsequent turns update the character correctly.
- Codex activity details open the selected observed session. Replies and permission decisions remain in Codex.
- Codex step labels keep updating after the retained twenty-step window fills and reset when switching sessions.
- Voice capture shows a live microphone level and separate recording duration, with clear silence and stalled-input feedback. Administrator enrollment saves the validated profile as bounded raw bytes in Windows Credential Manager, fixing the UTF-16 size failure.

## 0.1.1 — 2026-10-02

Roadeep edition of the Windows app.

- Sign in with a Roadeep account (email and password, or phone and SMS code); tokens live in the Windows Credential Manager.
- Chat with Roadeep from the island: live streaming replies, Markdown, conversation history, tool approvals, balance chip, file drop.
- Roadeep agents and local agents built with an AI-assisted wizard, shown as pills on the island.
- Local MCP server (`roadeep-mcp.exe`, entry `roadeep`) with Roadeep chat, models, agents and Generation Hub tools; paid generations need a Windows confirmation dialog.
- Global shortcut (default Ctrl+Alt+Space) and a right-click menu on the island.
- Persian (default) and English interface.
- Opt-in auto-update and code signing at build time (see windows/RELEASING.md).
- No generation view on the island; generation is available through the MCP server only.

## Unreleased

- Compact island on screens without a notch (#22) — thanks @Kamasoutra
- Only web links (http/https) open from the notch; other kinds of links from Claude or integrations are ignored (#16) — thanks @Cris1670
- Hook socket limited to your own user account, with size and time limits; logs no longer keep commands, n8n data or full URLs, and stay under 1 MB (#16) — thanks @Cris1670 and @Vignesh-Thangamariappan
- The island always reopens after folding, and Settings opens below it, resizable — thanks @rouderz
