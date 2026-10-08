# Roadeep for Windows

A desktop companion for the [Roadeep](https://roadeep.com) AI platform. It lives
in a small "island" at the top of your screen, with an animated character, and
lets you chat with Roadeep, run your agents, approve Claude Code permissions and
keep an eye on your services without leaving what you are doing.

![Windows 10/11](https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows)
![Tauri 2](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=black)
![Rust](https://img.shields.io/badge/Rust-backend-000?logo=rust)
![Code: MIT](https://img.shields.io/badge/code-MIT-green)

> **Licence and assets: read this before sharing a build.** See
> [Licence and assets](#licence-and-assets). In short, this build still uses the
> upstream project's character, icons and sounds, which may not be published or
> distributed without their author's written permission. Use it locally only.

Based on an open-source project by Louis Raillé (MIT licence).

## Features

- **Island** at the top centre of the screen: it peeks out, opens on click and
  retracts again. Drag it to dock it anywhere along the top, left or right edge
  of any display. The UI is Persian (default, right-to-left, Vazirmatn font) or
  English, switchable in Settings.
- **Roadeep account**: sign in with email and password, or with a phone number and
  an SMS code. The tokens are kept in the Windows Credential Manager.
- **Chat with Roadeep**: replies stream in live and are rendered as Markdown;
  past conversations are listed in the history; tool calls that need your approval
  are approved from the island; a chip shows your balance. The model, the answer
  mode (off, web search or reasoning) and deep research follow your Roadeep
  account settings. Drop a file on the island to ask questions about it.
- **Agents**: Roadeep's own agents (exclusive and public) and local agents you
  build in Settings with an AI-assisted wizard. They appear as coloured pills on
  the island.
- **Claude Code**: hooks show your sessions and let you approve permission
  requests from the island (see [Claude Code](#claude-code)).
- **Integrations**: Stripe, GitHub, Vercel, n8n, Resend, Notion and Cal.com, each
  with its own API key, shown as pills on the island.
- **Local MCP server** for Claude Code: Roadeep chat, models and agents, plus the
  Generation Hub tools (see [MCP server](#mcp-server-for-claude-code)). There is
  no generation interface on the island.
- **Global shortcut** (default `Ctrl+Alt+R`) that opens the island chat from
  anywhere, and a right-click menu on the island.
- **Optional auto-update and code signing**, both off in a plain build (see
  [Releasing](#releasing)).

## Install

There is no download yet. [Build it yourself](#build-it-yourself); the installer
installs for the current user only, with no admin prompt. Builds are signed
with a self-signed certificate; after installing, the installer offers to trust
its publisher "Roadeep" on this computer (default No; details in
[RELEASING.md](RELEASING.md#the-self-signed-certificate-current-setup)). A signed build with
automatic updates is possible once the release setup in
[RELEASING.md](RELEASING.md) is done, but the asset licence below has to be
resolved before any build is published.

## Using it

| What you do | What happens |
|---|---|
| Move the mouse to the screen edge where the island is docked (top centre by default) | The island peeks out |
| Click the small island | It opens |
| Drag the island (the small one, or the open one by its header) | It follows the mouse and springs into the nearest top, left or right edge; `Esc` while dragging puts it back |
| `Ctrl+Alt+R` (configurable in Settings → General) | Opens the island chat from anywhere |
| Type in the chat and send | Starts a reply that streams in |
| Drag a file onto the island | Offers to answer questions about it |
| Expand button in the chat header | The chat becomes a large island; it stays open until Restore or Minimize now |
| Right-click the island | Menu: Minimize now, Reset position (once moved), Settings, Quit Roadeep |
| `Esc` | Closes the island |
| Notification-area icon | Open, Settings…, Pause, Quit |

There is no window in the taskbar and no console: the island and the
notification-area icon are the whole app.

Before you chat, open Settings and sign in to Roadeep. In the chat you can pick
an agent (none, one of yours, or a Roadeep exclusive or public agent) and open the
history of earlier conversations.

**Settings** has five sections: Account, Integrations, Claude Code, MCP and
General (language, sound, auto-close, display, island position, shortcut,
updates). Local agents are built
in the settings window with the agent wizard.

The global shortcut can be switched off by clearing it. If another app already
owns the combination, Settings shows that it is not working; the app carries on.

## Claude Code

Open **Settings → Claude Code → Install hooks…**. You get the exact diff of what
will change in `%USERPROFILE%\.claude\settings.json`, the path of the dated backup
that will be taken, and nothing is written until you click. Your own hooks are
never touched, and uninstalling removes only this app's entries.

The relay is a tiny executable, `roadeep-hook.exe`, copied to
`%LOCALAPPDATA%\com.roadeep.desktop\bin\` at launch. It is given 300 ms to reach the app and
exits cleanly if the app is closed, slow or crashed, so a Claude Code session is
never blocked or slowed down. If nobody answers a permission request in time,
Claude Code asks in the terminal as usual. It works from any terminal.

A permission request opens the island with **Deny / Allow**; nothing is approved
without a click.

## MCP server for Claude Code

`roadeep-mcp.exe` is a local MCP server that Claude Code can spawn. It holds no
credentials: it relays each call over a named pipe, restricted to your user
account, to the running app, which uses the Roadeep account signed in there. The
app must therefore be running and signed in.

Install it from **Settings → MCP**, which shows the change to
`%USERPROFILE%\.claude.json` first (entry name `roadeep`). The tools:

- `roadeep_whoami`, `roadeep_list_models`, `roadeep_list_agents`, `roadeep_chat`
- Generation Hub: `roadeep_list_subtypes`, `roadeep_get_subtype`,
  `roadeep_estimate_generation`, `roadeep_start_generation`,
  `roadeep_generation_status`, `roadeep_cancel_generation`

A paid generation always needs a Windows confirmation dialog that you click:
estimate first, then start with the quote it returns. Every call is logged by
name, outcome and duration; arguments, prompts and replies are never logged.

## Settings and privacy

- No telemetry.
- Secrets (the Roadeep session, integration API keys) live in the Windows
  Credential Manager, never on disk and never in the interface.
- The app talks only to: Roadeep (`https://roadeep.com/api`), the integration
  services you configured (Stripe, GitHub, Vercel, n8n, Resend, Notion, Cal.com),
  and GitHub, only if you build with auto-update enabled.
- Roadeep traffic bypasses the system proxy. Set `ROADEEP_PROXY=1` to route
  it through the system proxy again.
- Preferences are in `%APPDATA%\Roadeep\settings.json`; local agents in
  `%APPDATA%\Roadeep\agents.json`.
- Internal names: bundle identifier `com.roadeep.desktop` (local data, relays,
  inbox and log live in `%LOCALAPPDATA%\com.roadeep.desktop`, never in the install
  folder), `%APPDATA%\Roadeep`, `roadeep-hook.exe`, `roadeep-mcp.exe`. Data,
  Credential Manager entries and autostart from builds that used the original
  project's name are migrated at startup (`src-tauri/src/migrate.rs`). Relays
  from those builds keep working (the app also listens on their pipe names);
  registrations that still point at them are offered for update in Settings
  (backup, diff, your confirmation), and the old relay folders are deleted only
  when you ask, once nothing visible references them.

## Build it yourself

You need [Rust](https://rustup.rs), [Node 20+](https://nodejs.org) and the **MSVC
build tools** (Visual Studio Build Tools with "Desktop development with C++").
WebView2 ships with Windows 10/11.

```powershell
cd windows
npm install
npm run tauri dev          # live-reloading development build
npm run pack               # builds the installer into windows/release/
npm test                   # front-end tests (vitest)
npm run test:release       # release-tooling tests
npm run version -- 0.2.0   # sets the version in every file that carries it
```

`npm run dev` alone serves the front end in an ordinary browser, which is enough
to work on the island's looks.

`npm run pack` leaves two copies of the installer in `windows/release/`:

```
Roadeep-Windows-X.Y.Z-setup.exe   the versioned installer
Roadeep-Windows-setup.exe         the same file under the rolling name
```

Installing is optional: `target/release/Roadeep.exe` runs on its own. The app icon
and tray icon are generated by `npm run icons` (`scripts/gen-icons.mjs`).

### Layout

```
windows/
  src/                 island front end (TypeScript, no framework)
    character/         the animated character and the launch greeting (Canvas 2D)
    core/              bridge to Rust, state, i18n and locales, layout, sound
    island/            state machine, hooks, integrations, Roadeep state
    views/             every island view (chat, history, Markdown, upload…)
    settings/          the settings window (account, agents, MCP, updates)
    upload/            the file-drop animation
  src-tauri/           Rust backend: window, named pipe, Roadeep client,
                       integrations, MCP server side, updater, shortcut
    src/roadeep/       auth, chat, threads, WebSocket, generation, HTTP
    src/mcp/           app side of the local MCP server
  hook/                roadeep-hook.exe, the Claude Code relay
  mcp/                 roadeep-mcp.exe, the MCP relay
  scripts/             pack, version and release tooling, icon generator
```

### Log

`%LOCALAPPDATA%\com.roadeep.desktop\roadeep.log`: hook events, permission decisions, poller and
update problems. It stays on your machine.

## Releasing

Code signing and the automatic updater are opt-in, through environment variables
read at build time. A plain `npm run pack` builds an unsigned installer with
updates switched off. The full procedure is in [RELEASING.md](RELEASING.md).

## Licence and assets

- **Code**: MIT. Copyright (c) 2026 Louis Raillé. Based on an open-source project
  by Louis Raillé (MIT licence); see the root `LICENSE`.
- **Assets**: per the root `LICENSE-ASSETS.md`, the upstream names, the character
  design, the app and menu-bar icons, the sounds and the media remain the property
  of their author. You may build and run the project for yourself, but you may not
  publish or distribute an app, a fork or a derivative work with the upstream
  icon, character or sounds without written permission. If you ship your own app,
  it needs its own name, icon, character and sounds.
- **Consequence for this build**: it still uses the upstream character, icons and
  sounds, so it is for local, personal use only until written permission is
  obtained from the author or those assets are replaced with Roadeep's own
  character, icon and sounds. Please read `LICENSE-ASSETS.md` itself for the exact
  terms.
