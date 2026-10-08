// English strings for the global shortcut (Settings → General) and for the
// error codes the Rust side sends (src-tauri/src/errors.rs, read by
// core/error-text.ts). Merged into the main `en` dictionary by the orchestrator.

export const miscEn: Record<string, string> = {
  "misc.shortcut": "Chat shortcut",
  "misc.shortcutHint": "Opens the island chat from any app.",
  "misc.shortcutNone": "Off",
  "misc.shortcutRecord": "Record a new shortcut",
  "misc.shortcutRecording": "Press the keys…",
  "misc.shortcutRecordingHint": "Esc cancels · Backspace turns it off",
  "misc.shortcutLabel": "Chat shortcut: {combo}. Click to change.",
  "misc.shortcutActive": "Active",
  "misc.shortcutOff": "Off",
  "misc.shortcutTaken": "Not available — another app has taken it",
  "misc.shortcutSaved": "Shortcut saved: {combo}",
  "misc.shortcutCleared": "Shortcut turned off",
  "misc.keyWin": "Win",

  "err.int.invalidKey": "Invalid API key (401)",
  "err.int.http": "API error {status}",
  "err.int.stripeSecretKey": "Use a secret key (sk_live_…, not pk_live_…)",
  "err.int.tokenScope": "The token lacks the needed scope",
  "err.int.tokenAccess": "The token lacks access",
  "err.int.keyAccess": "The key lacks access",
  "err.int.notionAccess": "The integration lacks access — share a page with it in Notion",
  "err.int.noConnection": "No connection: {detail}",
  "err.n8n.items": "→ {node} · {count} item(s)",
  "err.int.badField": "Check “{field}” — it doesn't look right",
  "err.int.notFound": "Not found (404) — check the IDs and addresses you entered",
  "err.int.rateLimited": "Too many requests (429). It will retry later.",
  "err.int.server": "The service is having trouble ({status})",
  "err.int.tooLarge": "The answer was too large to read",
  "err.int.badResponse": "The service answered in an unexpected shape",
  "err.int.hostBlocked": "Blocked: the request would go to an address this service doesn't use",

  "err.file.notDropped": "Drop the file on the island again.",
  "err.file.isFolder": "Folders can't be dropped yet.",
  "err.file.unreadable": "Can't read {path}: {detail}",
  "err.file.copy": "Couldn't copy the file: {detail}",

  "err.cfg.unreadable": "Can't read {path}: {detail}",
  "err.cfg.notObject": "{path} isn't a JSON object — Roadeep won't touch it.",
  "err.cfg.invalidJson": "{path} isn't valid JSON ({detail}). Fix or move it, then try again — Roadeep won't overwrite it.",
  "err.cfg.changed": "{path} changed since the preview. Nothing was written — review the new diff.",
  "err.cfg.backup": "Backup failed: {detail}",
  "err.cfg.write": "Write failed: {detail}",
  "err.cfg.noChange": "No change.",
  "err.mcp.exeMissing": "{file} isn't in place yet. Restart Roadeep and try again.",

  "err.shortcut.invalid": "That isn't a shortcut Windows can use.",
  "err.shortcut.noModifier": "Use Ctrl, Alt or Win with the key.",
  "err.shortcut.taken": "Not available — another app has taken it",
};
