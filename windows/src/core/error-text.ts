// Turns the coded strings the Rust side sends (src-tauri/src/errors.rs) into
// text in the UI language. Format: `CODE` or `CODE|arg|arg…` on the first
// line; the last argument takes the rest of that line (it may contain `|`);
// any further lines are free text kept as is. Unknown input is shown raw.

import { isolate, registerMessages, t } from "./i18n";
import { miscEn } from "./locales/misc-en";
import { miscFa } from "./locales/misc-fa";
import { updateEn } from "./locales/update-en";
import { updateFa } from "./locales/update-fa";
import { intEn } from "./locales/int-en";
import { intFa } from "./locales/int-fa";
import { mcpcCoreEn } from "./locales/mcpc-core-en";
import { mcpcCoreFa } from "./locales/mcpc-core-fa";
import { mcpcOauthEn } from "./locales/mcpc-oauth-en";
import { mcpcOauthFa } from "./locales/mcpc-oauth-fa";
import { idleEn } from "./locales/idle-en";
import { idleFa } from "./locales/idle-fa";
import { plannerErrorsEn } from "./locales/planner-errors-en";
import { plannerErrorsFa } from "./locales/planner-errors-fa";

registerMessages(miscEn, miscFa);
registerMessages(updateEn, updateFa);
registerMessages(intEn, intFa);
registerMessages(mcpcCoreEn, mcpcCoreFa);
registerMessages(mcpcOauthEn, mcpcOauthFa);
registerMessages(idleEn, idleFa);
registerMessages(plannerErrorsEn, plannerErrorsFa);

/** Argument names in order; a trailing "#" marks a number (Persian digits in fa). */
const CODES: Record<string, { key: string; args: string[] }> = {
  E_INT_INVALID_KEY: { key: "err.int.invalidKey", args: [] },
  E_INT_HTTP: { key: "err.int.http", args: ["status#"] },
  E_INT_STRIPE_SECRET_KEY: { key: "err.int.stripeSecretKey", args: [] },
  E_INT_TOKEN_SCOPE: { key: "err.int.tokenScope", args: [] },
  E_INT_TOKEN_ACCESS: { key: "err.int.tokenAccess", args: [] },
  E_INT_KEY_ACCESS: { key: "err.int.keyAccess", args: [] },
  E_INT_NOTION_ACCESS: { key: "err.int.notionAccess", args: [] },
  E_INT_NO_CONNECTION: { key: "err.int.noConnection", args: ["detail"] },
  I_N8N_ITEMS: { key: "err.n8n.items", args: ["count#", "node"] },
  E_INT_NO_KEY: { key: "err.int.noKey", args: [] },
  E_INT_PAUSED: { key: "err.int.paused", args: [] },
  E_INT_BAD_URL: { key: "err.int.badUrl", args: [] },
  E_INT_BAD_ID: { key: "err.int.badId", args: [] },
  E_INT_API: { key: "err.int.api", args: ["detail"] },
  E_INT_CLOUDFLARE_ACCESS: { key: "err.int.cloudflareAccess", args: [] },
  E_INT_SENTRY_NO_ORG: { key: "err.int.sentryNoOrg", args: [] },
  E_INT_UNKNOWN: { key: "err.int.unknown", args: ["id"] },
  E_INT_BAD_FIELD: { key: "err.int.badField", args: ["field"] },
  E_INT_NOT_FOUND: { key: "err.int.notFound", args: [] },
  E_INT_RATE_LIMITED: { key: "err.int.rateLimited", args: [] },
  E_INT_SERVER: { key: "err.int.server", args: ["status#"] },
  E_INT_TOO_LARGE: { key: "err.int.tooLarge", args: [] },
  E_INT_BAD_RESPONSE: { key: "err.int.badResponse", args: [] },
  E_INT_HOST_BLOCKED: { key: "err.int.hostBlocked", args: [] },

  E_FILE_NOT_DROPPED: { key: "err.file.notDropped", args: [] },
  E_FILE_IS_FOLDER: { key: "err.file.isFolder", args: [] },
  E_FILE_UNREADABLE: { key: "err.file.unreadable", args: ["path", "detail"] },
  E_FILE_COPY: { key: "err.file.copy", args: ["detail"] },

  E_CFG_UNREADABLE: { key: "err.cfg.unreadable", args: ["path", "detail"] },
  E_CFG_NOT_OBJECT: { key: "err.cfg.notObject", args: ["path"] },
  E_CFG_INVALID_JSON: { key: "err.cfg.invalidJson", args: ["path", "detail"] },
  E_CFG_CHANGED: { key: "err.cfg.changed", args: ["path"] },
  E_CFG_BACKUP: { key: "err.cfg.backup", args: ["detail"] },
  E_CFG_WRITE: { key: "err.cfg.write", args: ["detail"] },
  I_NO_CHANGE: { key: "err.cfg.noChange", args: [] },
  E_MCP_SERVERS_NOT_OBJECT: { key: "err.mcp.containerNotObject", args: ["key", "path"] },
  E_MCP_FOREIGN_ENTRY: { key: "err.mcp.foreignEntryIn", args: ["name", "path"] },
  E_MCP_EXE_MISSING: { key: "err.mcp.exeMissing", args: ["file"] },
  E_MCP_UNKNOWN_CLIENT: { key: "err.mcp.unknownClient", args: ["id"] },
  E_MCP_JSONC: { key: "err.mcp.jsonc", args: ["path"] },
  E_MCP_INVALID_TOML: { key: "err.mcp.invalidToml", args: ["path", "detail"] },
  E_MCP_YAML_MANUAL: { key: "err.mcp.yamlManual", args: ["path", "detail"] },

  E_SHORTCUT_INVALID: { key: "err.shortcut.invalid", args: ["accelerator"] },
  E_SHORTCUT_NO_MODIFIER: { key: "err.shortcut.noModifier", args: [] },
  E_SHORTCUT_TAKEN: { key: "err.shortcut.taken", args: ["detail"] },

  E_UPDATE_DISABLED: { key: "err.update.disabled", args: [] },
  E_UPDATE_NOTHING: { key: "err.update.nothing", args: [] },
  E_UPDATE_BUSY: { key: "err.update.busy", args: [] },
  E_UPDATE_NETWORK: { key: "err.update.network", args: [] },
  E_UPDATE_TIMEOUT: { key: "err.update.timeout", args: [] },
  E_UPDATE_NO_RELEASE: { key: "err.update.noRelease", args: [] },
  E_UPDATE_MANIFEST: { key: "err.update.manifest", args: [] },
  E_UPDATE_SIGNATURE: { key: "err.update.signature", args: [] },
  E_UPDATE_INSTALL: { key: "err.update.install", args: [] },
  E_UPDATE_FAILED: { key: "err.update.failed", args: [] },

  E_MCPC_STORE: { key: "err.mcpc.store", args: ["detail"] },
  E_MCPC_UNKNOWN_SERVER: { key: "err.mcpc.unknownServer", args: ["id"] },
  E_MCPC_INVALID: { key: "err.mcpc.invalid", args: ["field"] },
  E_MCPC_LIMIT: { key: "err.mcpc.limit", args: ["limit#"] },
  E_MCPC_NEEDS_AUTH: { key: "err.mcpc.needsAuth", args: [] },
  E_MCPC_NEEDS_APPROVAL: { key: "err.mcpc.needsApproval", args: [] },
  E_MCPC_DISABLED: { key: "err.mcpc.disabled", args: [] },
  E_MCPC_COMMAND_NOT_FOUND: { key: "err.mcpc.commandNotFound", args: ["command"] },
  E_MCPC_UNSAFE_ARG: { key: "err.mcpc.unsafeArg", args: ["arg"] },
  E_MCPC_SPAWN: { key: "err.mcpc.spawn", args: ["detail"] },
  E_MCPC_EXITED: { key: "err.mcpc.exited", args: [] },
  E_MCPC_TIMEOUT: { key: "err.mcpc.timeout", args: [] },
  E_MCPC_HTTP: { key: "err.mcpc.http", args: ["status#"] },
  E_MCPC_NETWORK: { key: "err.mcpc.network", args: ["detail"] },
  E_MCPC_TOO_LARGE: { key: "err.mcpc.tooLarge", args: [] },
  E_MCPC_PROTOCOL: { key: "err.mcpc.protocol", args: ["detail"] },
  E_MCPC_RPC: { key: "err.mcpc.rpc", args: ["code", "message"] },
  E_MCPC_UNKNOWN_TOOL: { key: "err.mcpc.unknownTool", args: ["tool"] },
  E_MCPC_TOOL_OFF: { key: "err.mcpc.toolOff", args: ["tool"] },
  E_MCPC_TOOL_ASK: { key: "err.mcpc.toolAsk", args: ["tool"] },
  E_MCPC_ARGS_TOO_LARGE: { key: "err.mcpc.argsTooLarge", args: ["limit#"] },
  E_MCPC_COMMAND_CHANGED: { key: "err.mcpc.commandChanged", args: [] },

  E_MCPC_OAUTH_DISCOVERY: { key: "err.mcpc.oauth.discovery", args: ["detail"] },
  E_MCPC_OAUTH_INSECURE: { key: "err.mcpc.oauth.insecure", args: ["host"] },
  E_MCPC_OAUTH_NO_DCR: { key: "err.mcpc.oauth.noDcr", args: [] },
  E_MCPC_OAUTH_REGISTER: { key: "err.mcpc.oauth.register", args: ["detail"] },
  E_MCPC_OAUTH_LISTEN: { key: "err.mcpc.oauth.listen", args: ["detail"] },
  E_MCPC_OAUTH_DENIED: { key: "err.mcpc.oauth.denied", args: ["detail"] },
  E_MCPC_OAUTH_STATE: { key: "err.mcpc.oauth.state", args: [] },
  E_MCPC_OAUTH_TIMEOUT: { key: "err.mcpc.oauth.timeout", args: [] },
  E_MCPC_OAUTH_CANCELLED: { key: "err.mcpc.oauth.cancelled", args: [] },
  E_MCPC_OAUTH_TOKEN: { key: "err.mcpc.oauth.token", args: ["detail"] },
  E_MCPC_OAUTH_KEYRING: { key: "err.mcpc.oauth.keyring", args: ["detail"] },

  E_PLANNER_STORE: { key: "err.planner.store", args: ["detail"] },
  // The field name is for the log and the chat tools; the UI checks its own fields first.
  E_PLANNER_INVALID: { key: "err.planner.invalid", args: ["field"] },
  E_PLANNER_NOT_FOUND: { key: "err.planner.notFound", args: [] },
  E_PLANNER_LIMIT: { key: "err.planner.limit", args: ["limit#"] },
};

/** The code at the start of `raw`, when it is one we know. */
export function errorCode(raw: unknown): string | null {
  const text = typeof raw === "string" ? raw : String(raw ?? "");
  const code = text.split("\n", 1)[0].split("|", 1)[0].trim();
  return code in CODES ? code : null;
}

/**
 * Text for an error (or notice) from Rust. Accepts what a rejected invoke
 * gives (a string, or an Error whose message is one) and anything else.
 */
export function localizeError(raw: unknown): string {
  const text = (typeof raw === "string" ? raw : String(raw ?? "")).replace(/^Error:\s*/, "");
  const newline = text.indexOf("\n");
  const first = newline < 0 ? text : text.slice(0, newline);
  const rest = newline < 0 ? "" : text.slice(newline);
  const parts = first.split("|");
  const spec = CODES[parts[0].trim()];
  if (!spec) return text;

  let args = parts.slice(1);
  if (spec.args.length > 0 && args.length > spec.args.length) {
    args = [...args.slice(0, spec.args.length - 1), args.slice(spec.args.length - 1).join("|")];
  }
  const vars: Record<string, string | number> = {};
  spec.args.forEach((name, i) => {
    const value = args[i] ?? "";
    if (name.endsWith("#")) {
      const n = Number(value);
      vars[name.slice(0, -1)] = value !== "" && Number.isFinite(n) ? n : value;
    } else {
      // Paths and OS messages are Latin: keep them from reordering a Persian sentence.
      vars[name] = isolate(value);
    }
  });
  return t(spec.key, vars) + rest;
}
