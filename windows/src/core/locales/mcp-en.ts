// English strings for the Roadeep MCP section (settings window).
// Merged into the main `en` dictionary by the orchestrator.

export const mcpEn: Record<string, string> = {
  "mcp.loading": "Checking your coding apps…",
  "mcp.unavailable": "The MCP status isn't available outside the Roadeep app.",
  "mcp.desc": "A local Roadeep MCP server for your coding apps, run by this app.",
  "mcp.installedHint":
    "Your coding apps can use Roadeep through this app: chat with Roadeep models and agents, and run Roadeep generations, with the account signed in here.",
  "mcp.notInstalledHint":
    "Add this app as a Roadeep MCP server to Claude Code, Cursor, VS Code and other coding apps, so they can chat with Roadeep models and agents and run Roadeep generations with the account signed in here. Your password and tokens never leave the app.",
  "mcp.tools":
    "Tools: account, models, agents, chat, generation products, cost estimate, start generation, status, cancel.",
  "mcp.costNote":
    "Generations spend Roadeep credits. The AI has to get a cost estimate first, and the Roadeep app must be open and signed in for any tool to work.",
  "mcp.server": "MCP server",
  "mcp.account": "Roadeep account",
  "mcp.signedOut": "Not signed in — the tools will ask you to sign in",
  "mcp.exeMissing": "roadeep-mcp.exe is not in place yet. Restart the Roadeep app; if it still fails, build it with {cmd}.",
  "mcp.exeNotInstalled": "The MCP server isn't installed yet.",
  "mcp.connectedCount": "{count} connected",

  "mcp.appsTitle": "Apps",
  "mcp.appsHint":
    "Add Roadeep to an app in one click: you see the exact change first, and the file is backed up. Then restart that app (or reload its MCP servers).",
  "mcp.noneDetected": "No coding app found on this PC. You can still add Roadeep to one below.",
  "mcp.otherApps": "Other apps ({count})",
  "mcp.otherAppsHint": "Not found on this PC. You can still add Roadeep; its config file is created.",
  "mcp.state.connected": "Connected",
  "mcp.state.notConnected": "Not connected",
  "mcp.state.conflict": "Conflict",
  "mcp.state.notFound": "Not found",
  "mcp.state.legacy": "Needs update — installed by the previous version",
  "mcp.add": "Add…",
  "mcp.update": "Update…",
  "mcp.remove": "Remove…",
  "mcp.addTo": "Add Roadeep to {app}",
  "mcp.updateIn": "Update Roadeep in {app}",
  "mcp.removeFrom": "Remove Roadeep from {app}",
  "mcp.legacyCleanup": "Remove old helper files",
  "mcp.legacyCleanupHint": "Deletes the relay files left by the previous version once nothing uses them.",
  "mcp.legacyCleanupBusy": "Removing old helper files…",
  "mcp.legacyReferenced": "Nothing was removed: update the entries marked “Needs update” above (and the coding tool connections) first.",
  "mcp.legacyNone": "No old helper files were found.",
  "mcp.legacyRemoved": "Removed:",
  "mcp.legacyKept": "Kept (in use; try again after closing the coding apps):",
  "mcp.legacyFailed": "Old helper files could not be removed: {err}",
  "mcp.configFile": "Config file",

  "mcp.conflict":
    "{app} already has an MCP server named \"roadeep\" that this app didn't add. Installing replaces it — the diff shows exactly what changes.",
  "mcp.previewInstall":
    "This is exactly what will change in {file}. Only the \"roadeep\" entry is added; other MCP servers and settings are left untouched.",
  "mcp.previewRemove": "This removes only the \"roadeep\" entry this app added to {file}. Everything else is left untouched.",
  "mcp.backup": "Backup →",
  "mcp.confirmWrite": "Back up and write",
  "mcp.confirmRemove": "Back up and remove",
  "mcp.doneInstall": "Done. Previous file saved as {backup}. Restart {app} (or reload its MCP servers) to see the Roadeep tools.",
  "mcp.doneInstallNoBackup": "Done. Restart {app} (or reload its MCP servers) to see the Roadeep tools.",
  "mcp.doneRemove": "Removed. Previous file saved as {backup}.",
  "mcp.doneRemoveNoBackup": "Removed.",
  "mcp.writeFailed": "Could not write: {err}",
  "mcp.manualTitle": "The entry to add by hand",

  "err.mcp.unknownClient": "Unknown app: {id}",
  "err.mcp.containerNotObject": "{key} in {path} isn't a map of servers — Roadeep won't touch it.",
  "err.mcp.foreignEntryIn":
    "The \"{name}\" MCP server in {path} wasn't added by Roadeep, so it's left alone. Remove it in that app if you no longer want it.",
  "err.mcp.jsonc":
    "{path} has comments, which would be lost if Roadeep rewrote it — so it won't. Add (or remove) the entry below by hand.",
  "err.mcp.invalidToml": "{path} isn't valid TOML ({detail}). Fix it, then try again — Roadeep won't overwrite it.",
  "err.mcp.yamlManual": "Roadeep can't safely edit {path} ({detail}). Add (or remove) the entry below by hand.",
};
