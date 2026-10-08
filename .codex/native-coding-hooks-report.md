# Native coding hooks implementation — 2026-10-05

Worker ownership: Windows relay, native hook configuration/commands/pipe, structural coding-history provider support. Other workers' voice/UI changes preserved. No user configuration was changed by this worker.

## Implemented providers

| Provider | Scope / file | Native integration |
|---|---|---|
| Claude Code | User `.claude/settings.json` | Existing lifecycle/tool/permission behavior retained; exact legacy relay commands recognized. |
| Codex | User `CODEX_HOME/hooks.json`, otherwise `.codex/hooks.json` | Documented lifecycle, tools, compaction, interrupt and explicit `PermissionRequest` decisions. Exact hooks must be trusted in Codex through `/hooks`; config installation does not bypass trust. |
| Gemini CLI | User `.gemini/settings.json` | Documented grouped agent/tool/lifecycle/notification hooks; millisecond timeouts. Observation only. |
| Cursor | User `.cursor/hooks.json` | Documented version-1 flat lifecycle/tool/subagent hooks. Observation only. `preToolUse`/`subagentStart` deliberately exit 1 with no decision output because Cursor documents empty exit-0 permission-hook output as blocking. |
| Windsurf / Devin | User `.codeium/windsurf/hooks.json` | Windows `powershell` handlers for prompt/read/write/command/MCP/response events. Observation only. |
| Copilot CLI | User `COPILOT_HOME/hooks/roadeep.json`, otherwise `.copilot/hooks/roadeep.json` | Documented version-1 native `exec` + fixed `args` handlers. Observation only; cloud Linux sessions cannot reach this Windows pipe. |
| VS Code Local | Project `.github/hooks/roadeep-vscode.json` | Native Local PascalCase schema with Windows command overrides. Local harness preview feature; observation only. Provider-hosted Claude/Codex use their own adapters. |
| Kiro | Project `.kiro/hooks/roadeep.json` | Documented v1 standalone array schema, IDE 1.x / CLI 3.x; session/stop/tool/file-save events. Observation only. |
| OpenCode | Project `.opencode/plugins/roadeep/index.ts` | Generated plugin for the documented V2 API. Prompt/tool hooks and public lifecycle stream; fixed relay executable, no shell, bounded asynchronous child processes and cleanup. Permission events are notifications, never decisions. |

Cline and Zed remain explicitly **adapter unavailable** in status, rather than being mislabeled as connected. The accessible current Cline documentation redirects without an inspectable schema, and its older official launch article cannot establish current Windows support. No assertion that these products lack hooks intrinsically.

## Native API

`coding_hooks_status()` returns provider metadata; `coding_hooks_preview(provider, projectPath?, remove?)` returns the exact diff, backup path and fingerprint; `coding_hooks_apply(provider, projectPath?, fingerprint, remove?)` returns refreshed provider status. Island/settings authorization; filesystem work runs off the UI thread.

`ProviderStatus`: `provider`, `label`, `supported`, `scope`, `installed`, `detected`, `hookReady`, `settingsPath`, `hookPath`, `events`, `permission`, nullable `reason`. `detected` means the configuration folder exists, not a verified installed executable/version. `installed` means our expected definitions/source are present, not that upstream trust/policy permits execution.

Relay: `roadeep-hook --provider PROVIDER ORIGINAL_EVENT`; legacy `roadeep-hook EVENT` still means Claude. Shared provider/event allowlist normalizes event names while retaining `provider`, `original_event`, original session/call identity. Selected approval fields retained and bounded; raw output, content, patches and transcript locations omitted. Native redaction runs before webview emission.

Only Claude/Codex `PermissionRequest` waits for an explicit island decision. No acknowledgement, paused/missing UI, decline or timeout returns empty output to the upstream approval flow. Approval native commands require the island, bounded generated request IDs and recognized decisions.

## Safe configuration changes

- SHA-256 preview fingerprint binds original bytes, provider, exact destination and install/remove operation; rechecked immediately before replacement.
- Dated backup path is the one shown in preview. Unknown/oversized/malformed configs and reparse ancestors rejected.
- Foreign hooks/config keys preserved, including foreign handlers sharing a grouped entry. Removal matches exact executable/arguments, not substring markers.
- Windows `ReplaceFileW` preserves the original target security descriptor; temporary write handle closed before replacement. No-op changes do not rewrite files.
- OpenCode never edits `opencode.json(c)`. Only exact generated plugin bytes are owned; an existing unknown/modified plugin file is rejected (`hook-plugin-conflict`). Removal backs up then removes that one verified file; other plugin files/folders remain.
- Pipe concurrency bounded to 64 connections / 32 permission cards; whole input read deadline 2 seconds / maximum 1 MiB. Relay input and reply bounded.
- OpenCode relay spawn uses a fixed absolute executable and allowlisted arguments, `shell: false`, hidden window, maximum 16 in-flight children, 2-second child deadline. Payload contains only bounded session/tool/call/project metadata. Server-wide events are filtered to the plugin's project/known sessions, with a 128-session identity bound. No input/model/tool mutation or permission output. Logs contain allowlisted failure codes only.

## Explicit local setup commands

Run on the newly built/installed executable. These execute before the single-instance plugin and use the exact same native implementation as settings:

```text
Roadeep.exe --coding-hooks-preview codex
Roadeep.exe --coding-hooks-apply codex FINGERPRINT
Roadeep.exe --coding-hooks-status
Roadeep.exe --coding-hooks-preview-remove PROVIDER [PROJECT]
Roadeep.exe --coding-hooks-remove PROVIDER FINGERPRINT [PROJECT]
```

Success JSON on stdout, coded error on stderr / exit 2. Redirect output when launching a Windows subsystem executable. Preview diffs may contain existing local configuration values: inspect locally, do not dump them into logs/reports. No automatic Codex trust or external-agent approval.

## Verification and limits

- Native configuration/protocol: **10 tests passed**, including all provider event normalization, idempotence, foreign-handler preservation, invalid configs/paths, stale preview refusal, real temporary-file backup/atomic replacement, Kiro array and OpenCode owned-file install/remove/conflict.
- Relay: **5 tests passed**; decision shape, unknown decision silence, UTF-8 truncation, provider allowlist and private-output omission.
- Native pipe: **4 tests passed**; authorized reply bounds, decline/dropped UI, acknowledgement without grant, explicit decision, unacknowledged-card fallback.
- Coding history: **8 tests passed** before OpenCode extension; provider roundtrip fixture extended to OpenCode for root's final full gate.
- Actual OpenCode template: strict TypeScript check against explicitly labeled documented SDK fixtures plus isolated VM/mock transport **passed**. It exercises non-mutation, metadata privacy, fixed executable/no shell, task/tool/lifecycle events, foreign-project filtering, 16-process bound, failure recovery, timeout kill and cleanup. Run `node --experimental-vm-modules hook/opencode/verify.mjs` from `windows`.
- Upstream OpenCode V2 docs and current GitHub `dev` plugin context differ. No matching installed host SDK was available; the fixture test validates the documented contract, not every OpenCode release. The generated adapter catches unavailable APIs without blocking the host and status says `opencode-v2-required`.
- Native focused gates exercise synthetic payloads/temp configs only; no private transcript, microphone or biometric access, paid API, current hook trust or real provider configuration write.
- Root owns final full checks, UI review, all-model release/install and explicitly authorized current Codex configuration activation.

## Primary contracts consulted

[Codex hooks](https://learn.chatgpt.com/docs/hooks), [Gemini CLI hooks](https://geminicli.com/docs/hooks/), [Cursor hooks](https://cursor.com/docs/hooks), [Windsurf / Devin Cascade hooks](https://docs.devin.ai/desktop/cascade/hooks), [Copilot hook reference](https://docs.github.com/en/copilot/reference/hooks-reference), [VS Code Local hooks](https://code.visualstudio.com/docs/agents/reference/hooks-reference), [Kiro hooks](https://kiro.dev/docs/hooks/), [Kiro actions](https://kiro.dev/docs/hooks/actions/), [OpenCode V2 plugins](https://opencode.ai/v2/docs/build/plugins/), [OpenCode primary SDK event types](https://github.com/anomalyco/opencode/blob/dev/packages/sdk/js/src/v2/gen/types.gen.ts), [OpenCode current plugin context](https://github.com/anomalyco/opencode/blob/dev/packages/plugin/src/v2/promise/context.ts).
