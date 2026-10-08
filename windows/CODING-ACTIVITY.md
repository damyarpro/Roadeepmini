# Local coding activity

Open the island menu (`/` in an empty chat field, or the grid button), then choose **Coding activity / فعالیت کدنویسی**.

- **Read Codex logs** explicitly starts read-only observation of local Codex session logs. Observation defaults off. A bounded recent tail of existing logs supplies initial activity; new records are followed afterward. This log-reading control does not install hooks. Direct hook setup is a separate Settings connection. Local observation requires no API key.
- Codex uses the same home activity ticker and character phases as Claude. Started tools show working, tool results return to thinking, and terminal events show completion, interruption or failure. Active sessions remain visible through silent periods. The details button opens the same observed session; replies stay in Codex. Direct trusted Codex PermissionRequest hooks can additionally show a card in Roadeep; answering it requires an explicit click.
- Select a session to review its observed changed files, last patch, recent tools, failures and latest test result. Sessions remain separate from chat agents and from permission cards.
- Test success requires passing-test output and a known successful exit. A later observed edit marks the evidence stale. Missing output, unsupported runners and incomplete results remain unknown. Shell-side file changes and some long-running command results cannot be fully attributed.
- Overlap warnings compare observed edits in active sessions. They are advisory and do not lock files, block tools or authorize anything.
- **Copy handoff** or **Export Markdown** produces a local summary of the retained evidence. Exports are user-initiated and saved as a unique Markdown file under the application local data directory (`exports/`). The saved path appears only after the native write succeeds. The application never sends them to another assistant or service.
- **Keep local history** is optional and defaults off. It retains structural evidence for seven days (up to 512 events / 2 MiB) in `coding-history-v1.json` under the application local data directory. It omits prompts, commands, outputs and patches. Restored sessions are archived; their test freshness is unknown. Disabling retention deletes the saved file. A corrupt file produces a visible error and remains untouched until an explicit clear.
- **Clear evidence** erases memory and saved history and skips already buffered Codex log data. **Pause log reading** stops the log observer and archives log-only sessions when retention is enabled, otherwise clears log-only evidence; direct coding hooks remain available.
- Context and quota indicators appear only when Codex reports them. Context uses the latest input-token count and observed model window; quota percentages are usage, not account credit or remaining context.
- **Project tools → Inspect Git** reads the current repository status and staged/unstaged diff. The snapshot covers the whole working tree, not changes attributed to the selected session. It never writes Git state or runs configured diff/filter helpers. Untracked file contents are omitted. Results have output/time limits and an explicit timestamp; click again to refresh.
- **Project tools → Prepare terminal handoff** lists installed Codex, Claude Code and Gemini CLI programs. Select one and click the launch button to save a redacted handoff note and open an interactive terminal in the observed project directory. This starts a terminal, not a verified continuation of the original session. Evidence is passed through a file rather than interpolated into shell commands; approvals remain interactive.

## Direct coding hooks

Settings → Coding tools & MCP shows each provider connection separately. Expand a row, review its exact destination/diff/dated backup and confirm installation. Stale previews, malformed files and reparse paths are rejected. Existing foreign hooks survive installation and removal. No API key is required. Configuration presence does not prove a running host has loaded or trusted it.

| Provider | Scope | Permission handling |
|---|---|---|
| Claude Code, Codex | Windows user | Explicit Roadeep click for actual PermissionRequest only |
| Gemini CLI, Cursor, Windsurf, Copilot CLI | Windows user | In the source coding tool |
| VS Code Local, Kiro | Selected project | In the source coding tool |
| OpenCode V2 | Selected project plugin | In OpenCode |

Codex requires its own `/hooks` trust after installing new definitions; Roadeep never bypasses this. OpenCode requires the documented V2 plugin API, Kiro IDE 1.x/CLI 3.x, and VS Code Local requires upstream hook support. Those versioned hosts were verified against documented contracts and synthetic adapters, not every installed release. Cline/Zed adapters are currently unavailable and clearly labeled.

All adapters share the bounded same-user Windows pipe, preserve provider/session/call identity, omit raw outputs/transcripts, and fail back to the original coding tool when Roadeep is unavailable. Cursor observation hooks use its documented fail-open exit status rather than inventing a default permission grant. OpenCode spawns only the fixed relay without a shell, with a bounded queue/deadline and lifecycle cleanup. Matching Codex hook/log call IDs merge tool phases while preserving richer log evidence. Calls without reliable shared identity remain distinct; prompt timing is never guessed for deduplication.

## Text models in chat

After sign-in, the searchable chat model popover loads the account's current text-model catalog. Choosing a model saves the preference and starts a new conversation; the previous conversation remains in server history. The current thread's known model is shown separately; the ! button explains how switching starts a new chat. The picker is disabled during a reply and when the selected local agent pins its own model. Catalog failures offer retry; failed preference saves preserve the current conversation and choice. Automatic selection lets the service choose its default. No guessed model identifiers are included.

In **Settings → General → Character appearance**, choose body, idle eyes, idle color and accessory. The Canvas preview updates immediately. Restore original returns to the existing character identity. Emotional eyes, status colors, existing motions and mini-agent colors take precedence over personal idle appearance.

## Privacy and limits

No raw activity transcript is written by this feature. Local structural retention requires opt-in; otherwise evidence disappears after restart. The existing Codex log files are never altered. Observation, history and Git inspection send no activity data to a cloud service. An explicitly launched CLI is a separate configured application and controls its own processing. Recognizable credential patterns are redacted, but redaction cannot recognize every private business value; review a Markdown export before sharing it yourself.

The Rust observer retains at most 256 normalized events from 16 recent session files, reads bounded chunks with a 1 MiB per-file and 8 MiB per-poll budget, caps records at 2 MiB, and polls every three seconds only when enabled. The frontend retains at most 16 sessions and 160 events per session. A prompt begins a new evidence turn. Summaries cover the retained observed window, not the entire working tree or conversation. The island's hidden rendering loop remains stopped; enabled log observation continues independently.

Claude observations use existing hook metadata. Tool responses remain omitted by the relay, so Claude test results stay unknown unless evidence is available through a supported source. Codex observation requires local rollout logs; cloud-only sessions without local logs are unavailable.

## Verification

Tests use fabricated data rather than personal transcripts. Rust fixtures cover wire contracts, secret patterns, bounded tailing, partial/oversized records, rotation, opt-in defaults, clear and missing-file recovery. TypeScript tests cover validation, redaction, event pairing, observed freshness, failed edits, turn boundaries, advisory overlaps, Markdown escaping, monitor races and Canvas appearance.

Browser-only development scenes: `/?view=activity&activity=live&lang=fa`, plus `off`, `empty`, `error`, `stale`, `conflict`, `usage`, `history`, `git` and `handoff` scenes. Chat picker fixtures use `?view=prompt&models=ready|failure|busy|pinned|historical`; append `zoom=1` to inspect the island at native scale. These fabricated fixtures are never seeded in Tauri.
