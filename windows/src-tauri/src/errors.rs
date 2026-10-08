// Error (and a few notice) strings the front end translates.
//
// Commands keep returning `String` errors, but the strings Rust writes are
// codes rather than English sentences, so the Persian UI never shows English:
//
//     CODE                      e.g. "E_FILE_IS_FOLDER"
//     CODE|arg|arg…             e.g. "E_FILE_UNREADABLE|C:\a.pdf|Access is denied. (os error 5)"
//
// - The code and its arguments sit on the FIRST line. Anything after a newline
//   is free text shown as is (the n8n detail: a coded header, then raw fields).
// - Arguments are never translated: paths, HTTP statuses, OS error text.
// - Newlines inside arguments become spaces; a `|` inside any argument but the
//   last becomes `¦`. The last argument keeps its `|`s: the reader joins
//   whatever follows the expected arguments back into it.
// - Unknown codes (or plain text) are shown raw by the front end.
//
// The front end's table is src/core/error-text.ts (keys `err.*` in
// src/core/locales/misc-*.ts). A code added here needs a line there.

/// Integration poller: the key was refused (401).
pub const INT_INVALID_KEY: &str = "E_INT_INVALID_KEY";
/// Integration poller: any other HTTP failure. Args: status.
pub const INT_HTTP: &str = "E_INT_HTTP";
/// Stripe 403: a publishable key where a secret key is needed.
pub const INT_STRIPE_SECRET_KEY: &str = "E_INT_STRIPE_SECRET_KEY";
/// GitHub 403: the token lacks a scope.
pub const INT_TOKEN_SCOPE: &str = "E_INT_TOKEN_SCOPE";
/// Vercel 403.
pub const INT_TOKEN_ACCESS: &str = "E_INT_TOKEN_ACCESS";
/// Resend / Cal.com 403.
pub const INT_KEY_ACCESS: &str = "E_INT_KEY_ACCESS";
/// Notion 403: the integration isn't shared with any page.
pub const INT_NOTION_ACCESS: &str = "E_INT_NOTION_ACCESS";
/// Integration poller: the request never got an answer. Args: detail.
pub const INT_NO_CONNECTION: &str = "E_INT_NO_CONNECTION";
/// n8n success detail header (not an error). Args: item count, last node.
pub const N8N_ITEMS: &str = "I_N8N_ITEMS";
/// Integration poller: a required key isn't saved.
pub const INT_NO_KEY: &str = "E_INT_NO_KEY";
/// Connection test while the app is paused (nothing may reach the network).
pub const INT_PAUSED: &str = "E_INT_PAUSED";
/// An instance / region URL that isn't https (or http to this machine).
pub const INT_BAD_URL: &str = "E_INT_BAD_URL";
/// A Sentry organization slug or Cloudflare account ID that is malformed or unknown.
pub const INT_BAD_ID: &str = "E_INT_BAD_ID";
/// The service answered with its own error message. Args: message.
pub const INT_API: &str = "E_INT_API";
/// Cloudflare 403: the token can't read Pages in that account.
pub const INT_CLOUDFLARE_ACCESS: &str = "E_INT_CLOUDFLARE_ACCESS";
/// The Sentry token sees no organization.
pub const INT_SENTRY_NO_ORG: &str = "E_INT_SENTRY_NO_ORG";
/// A connection test for an id no poller knows. Args: id.
pub const INT_UNKNOWN: &str = "E_INT_UNKNOWN";
/// A catalog service field whose value doesn't match its pattern. Args: field label.
pub const INT_BAD_FIELD: &str = "E_INT_BAD_FIELD";
/// 404 from a catalog service: usually a wrong id or site in its fields.
pub const INT_NOT_FOUND: &str = "E_INT_NOT_FOUND";
/// 429 from a catalog service.
pub const INT_RATE_LIMITED: &str = "E_INT_RATE_LIMITED";
/// 5xx from a catalog service. Args: status.
pub const INT_SERVER: &str = "E_INT_SERVER";
/// A catalog service answer over 2 MB.
pub const INT_TOO_LARGE: &str = "E_INT_TOO_LARGE";
/// A catalog service answer that isn't JSON or lacks the list.
pub const INT_BAD_RESPONSE: &str = "E_INT_BAD_RESPONSE";
/// The request would go to a host the service's manifest doesn't declare.
pub const INT_HOST_BLOCKED: &str = "E_INT_HOST_BLOCKED";

/// A drop the island didn't really receive (DropRegistry refusal).
pub const FILE_NOT_DROPPED: &str = "E_FILE_NOT_DROPPED";
pub const FILE_IS_FOLDER: &str = "E_FILE_IS_FOLDER";
/// Args: path, detail.
pub const FILE_UNREADABLE: &str = "E_FILE_UNREADABLE";
/// Args: detail.
pub const FILE_COPY: &str = "E_FILE_COPY";

/// A Claude Code config file (settings.json, .claude.json). Args: path, detail.
pub const CFG_UNREADABLE: &str = "E_CFG_UNREADABLE";
/// Args: path.
pub const CFG_NOT_OBJECT: &str = "E_CFG_NOT_OBJECT";
/// Args: path, detail.
pub const CFG_INVALID_JSON: &str = "E_CFG_INVALID_JSON";
/// The file changed between the preview and the click. Args: path.
pub const CFG_CHANGED: &str = "E_CFG_CHANGED";
/// Args: detail.
pub const CFG_BACKUP: &str = "E_CFG_BACKUP";
/// Args: detail.
pub const CFG_WRITE: &str = "E_CFG_WRITE";
/// The diff is empty (not an error: shown in the diff box).
pub const CFG_NO_CHANGE: &str = "I_NO_CHANGE";
/// The servers container of an MCP client config (`mcpServers`, `servers`,
/// `mcp`, `mcp_servers`) is not a map. Args: key, path.
pub const MCP_SERVERS_NOT_OBJECT: &str = "E_MCP_SERVERS_NOT_OBJECT";
/// A "roadeep" MCP entry someone else added. Args: entry name, path.
pub const MCP_FOREIGN_ENTRY: &str = "E_MCP_FOREIGN_ENTRY";
/// roadeep-mcp.exe is missing. Args: file name.
pub const MCP_EXE_MISSING: &str = "E_MCP_EXE_MISSING";
/// An MCP client id the app doesn't know. Args: id.
pub const MCP_UNKNOWN_CLIENT: &str = "E_MCP_UNKNOWN_CLIENT";
/// JSON with comments or trailing commas: never rewritten. Args: path; the
/// entry to paste by hand follows on the next lines.
pub const MCP_JSONC: &str = "E_MCP_JSONC";
/// Codex's config.toml doesn't parse. Args: path, detail.
pub const MCP_INVALID_TOML: &str = "E_MCP_INVALID_TOML";
/// A YAML shape the line editor won't touch. Args: path, detail; the entry to
/// paste by hand follows on the next lines.
pub const MCP_YAML_MANUAL: &str = "E_MCP_YAML_MANUAL";

/// Not an accelerator at all. Args: what was given.
pub const SHORTCUT_INVALID: &str = "E_SHORTCUT_INVALID";
/// No Ctrl, Alt or Win in it.
pub const SHORTCUT_NO_MODIFIER: &str = "E_SHORTCUT_NO_MODIFIER";
/// The OS refused it, usually because another app holds it. Args: detail.
pub const SHORTCUT_TAKEN: &str = "E_SHORTCUT_TAKEN";

/// Auto-update (updater.rs). None of these take arguments.
/// This build was made without the updater variables.
pub const UPDATE_DISABLED: &str = "E_UPDATE_DISABLED";
/// Install clicked with no update found.
pub const UPDATE_NOTHING: &str = "E_UPDATE_NOTHING";
/// A check or a download is already running.
pub const UPDATE_BUSY: &str = "E_UPDATE_BUSY";
pub const UPDATE_NETWORK: &str = "E_UPDATE_NETWORK";
pub const UPDATE_TIMEOUT: &str = "E_UPDATE_TIMEOUT";
/// latest.json missing (no release yet, or a private repository).
pub const UPDATE_NO_RELEASE: &str = "E_UPDATE_NO_RELEASE";
/// latest.json unreadable or without a Windows entry.
pub const UPDATE_MANIFEST: &str = "E_UPDATE_MANIFEST";
/// The download does not match the signature (or the signed version).
pub const UPDATE_SIGNATURE: &str = "E_UPDATE_SIGNATURE";
/// The installer could not be started.
pub const UPDATE_INSTALL: &str = "E_UPDATE_INSTALL";
pub const UPDATE_FAILED: &str = "E_UPDATE_FAILED";

/// The user's MCP servers (mcpc/).
/// mcp-servers.json could not be read or written. Args: detail.
pub const MCPC_STORE: &str = "E_MCPC_STORE";
/// A server id that isn't in the list. Args: id.
pub const MCPC_UNKNOWN_SERVER: &str = "E_MCPC_UNKNOWN_SERVER";
/// A field that fails validation. Args: field (name, url, command, args, env,
/// auth, header, source, transport, slot, value, tool, arguments).
pub const MCPC_INVALID: &str = "E_MCPC_INVALID";
/// Too many servers (or tool settings). Args: limit.
pub const MCPC_LIMIT: &str = "E_MCPC_LIMIT";
/// The server needs a token or a sign-in (missing, or refused with 401).
pub const MCPC_NEEDS_AUTH: &str = "E_MCPC_NEEDS_AUTH";
/// A local command the user hasn't confirmed (or that changed since).
pub const MCPC_NEEDS_APPROVAL: &str = "E_MCPC_NEEDS_APPROVAL";
/// The server is switched off.
pub const MCPC_DISABLED: &str = "E_MCPC_DISABLED";
/// The command isn't on PATH (or isn't a program). Args: command.
pub const MCPC_COMMAND_NOT_FOUND: &str = "E_MCPC_COMMAND_NOT_FOUND";
/// An argument a .cmd/.bat launcher can't receive safely (`"`, `%`, `^`, `&`,
/// `|`, `<`, `>`). Args: argument.
pub const MCPC_UNSAFE_ARG: &str = "E_MCPC_UNSAFE_ARG";
/// The process could not be started. Args: detail.
pub const MCPC_SPAWN: &str = "E_MCPC_SPAWN";
/// The process ended (or closed its output). The server's last stderr lines
/// follow on the next lines.
pub const MCPC_EXITED: &str = "E_MCPC_EXITED";
/// No answer in time.
pub const MCPC_TIMEOUT: &str = "E_MCPC_TIMEOUT";
/// An HTTP status we can't use. Args: status.
pub const MCPC_HTTP: &str = "E_MCPC_HTTP";
/// The request never got an answer. Args: detail.
pub const MCPC_NETWORK: &str = "E_MCPC_NETWORK";
/// An answer over 4 MB.
pub const MCPC_TOO_LARGE: &str = "E_MCPC_TOO_LARGE";
/// The server broke the protocol. Args: detail.
pub const MCPC_PROTOCOL: &str = "E_MCPC_PROTOCOL";
/// The server answered with a JSON-RPC error. Args: code, message.
pub const MCPC_RPC: &str = "E_MCPC_RPC";
/// A tool name the model used that no enabled server offers. Args: tool.
pub const MCPC_UNKNOWN_TOOL: &str = "E_MCPC_UNKNOWN_TOOL";
/// A tool the user switched off. Args: tool.
pub const MCPC_TOOL_OFF: &str = "E_MCPC_TOOL_OFF";
/// A tool in "ask" mode called without the user's click (its mode changed
/// after the turn started). Args: tool.
pub const MCPC_TOOL_ASK: &str = "E_MCPC_TOOL_ASK";
/// Tool arguments too long to show in full on the approval card, so the call
/// was refused instead of asked. Args: limit.
pub const MCPC_ARGS_TOO_LARGE: &str = "E_MCPC_ARGS_TOO_LARGE";
/// The local command changed after the settings window showed it; it was not
/// approved.
pub const MCPC_COMMAND_CHANGED: &str = "E_MCPC_COMMAND_CHANGED";

/// OAuth sign-in to an MCP server (mcpc/oauth.rs).
/// The server's sign-in details (metadata) could not be found or are unusable. Args: detail.
pub const MCPC_OAUTH_DISCOVERY: &str = "E_MCPC_OAUTH_DISCOVERY";
/// A sign-in endpoint that isn't https. Args: host.
pub const MCPC_OAUTH_INSECURE: &str = "E_MCPC_OAUTH_INSECURE";
/// The service doesn't let apps register themselves: use a token instead.
pub const MCPC_OAUTH_NO_DCR: &str = "E_MCPC_OAUTH_NO_DCR";
/// The service refused to register the app. Args: detail.
pub const MCPC_OAUTH_REGISTER: &str = "E_MCPC_OAUTH_REGISTER";
/// The local port for the browser's answer could not be opened. Args: detail.
pub const MCPC_OAUTH_LISTEN: &str = "E_MCPC_OAUTH_LISTEN";
/// The user (or the service) declined in the browser. Args: detail.
pub const MCPC_OAUTH_DENIED: &str = "E_MCPC_OAUTH_DENIED";
/// The browser's answer didn't belong to this sign-in (state mismatch).
pub const MCPC_OAUTH_STATE: &str = "E_MCPC_OAUTH_STATE";
/// The sign-in wasn't finished in the browser within 5 minutes.
pub const MCPC_OAUTH_TIMEOUT: &str = "E_MCPC_OAUTH_TIMEOUT";
/// Replaced by a newer sign-in, signed out, or the server changed meanwhile.
pub const MCPC_OAUTH_CANCELLED: &str = "E_MCPC_OAUTH_CANCELLED";
/// The code could not be exchanged for tokens. Args: detail.
pub const MCPC_OAUTH_TOKEN: &str = "E_MCPC_OAUTH_TOKEN";
/// The Credential Manager refused to store or delete the tokens. Args: detail.
pub const MCPC_OAUTH_KEYRING: &str = "E_MCPC_OAUTH_KEYRING";

/// The planner (planner/).
/// planner.json could not be read or written. Args: detail.
pub const PLANNER_STORE: &str = "E_PLANNER_STORE";
/// A field that fails validation. Args: field (title, note, text, due, at,
/// repeat, minutes, enabled, nudge, dayKey, phase, scope, inMinutes).
pub const PLANNER_INVALID: &str = "E_PLANNER_INVALID";
/// A task, note, reminder or habit that no longer exists.
pub const PLANNER_NOT_FOUND: &str = "E_PLANNER_NOT_FOUND";
/// The list is full. Args: limit.
pub const PLANNER_LIMIT: &str = "E_PLANNER_LIMIT";

/// Builds `CODE|arg|arg…` (see the top of this file).
pub fn coded(code: &str, args: &[&str]) -> String {
    let mut out = String::from(code);
    let last = args.len().saturating_sub(1);
    for (i, arg) in args.iter().enumerate() {
        out.push('|');
        let flat = arg.replace("\r\n", " ").replace(['\r', '\n'], " ");
        if i < last {
            out.push_str(&flat.replace('|', "¦"));
        } else {
            out.push_str(&flat);
        }
    }
    out
}

/// The reading side, as error-text.ts does it: code, then `arity` arguments
/// with the last one taking the rest of the first line.
#[cfg(test)]
pub fn split(text: &str, arity: usize) -> (String, Vec<String>) {
    let first = text.lines().next().unwrap_or("");
    let mut parts: Vec<String> = first.split('|').map(str::to_string).collect();
    let code = parts.remove(0);
    if arity > 0 && parts.len() > arity {
        let rest = parts.split_off(arity - 1).join("|");
        parts.push(rest);
    }
    (code, parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_code_is_just_the_code() {
        assert_eq!(coded(FILE_IS_FOLDER, &[]), "E_FILE_IS_FOLDER");
    }

    #[test]
    fn arguments_follow_the_code() {
        let s = coded(FILE_UNREADABLE, &[r"C:\a.pdf", "Access is denied. (os error 5)"]);
        assert_eq!(s, r"E_FILE_UNREADABLE|C:\a.pdf|Access is denied. (os error 5)");
        assert_eq!(split(&s, 2), ("E_FILE_UNREADABLE".into(), vec![r"C:\a.pdf".into(), "Access is denied. (os error 5)".into()]));
    }

    #[test]
    fn newlines_and_pipes_cannot_break_the_format() {
        let s = coded(CFG_INVALID_JSON, &["a|b.json", "line 1\r\nline 2 | col 3\nend"]);
        assert!(!s.contains('\n') && !s.contains('\r'), "{s}");
        let (code, args) = split(&s, 2);
        assert_eq!(code, CFG_INVALID_JSON);
        assert_eq!(args, vec!["a¦b.json".to_string(), "line 1 line 2 | col 3 end".to_string()]);
    }

    #[test]
    fn last_argument_keeps_its_pipes_and_text_after_the_first_line_is_free() {
        let header = coded(N8N_ITEMS, &["3", "Set | Merge"]);
        let detail = format!("{header}\nname: Ada\nid: 7");
        let (code, args) = split(&detail, 2);
        assert_eq!(code, N8N_ITEMS);
        assert_eq!(args, vec!["3".to_string(), "Set | Merge".to_string()]);
    }

    #[test]
    fn codes_are_unique_and_well_formed() {
        let all = [
            INT_INVALID_KEY, INT_HTTP, INT_STRIPE_SECRET_KEY, INT_TOKEN_SCOPE, INT_TOKEN_ACCESS, INT_KEY_ACCESS,
            INT_NOTION_ACCESS, INT_NO_CONNECTION, N8N_ITEMS, INT_NO_KEY, INT_PAUSED, INT_BAD_URL, INT_BAD_ID,
            INT_API, INT_CLOUDFLARE_ACCESS, INT_SENTRY_NO_ORG, INT_UNKNOWN, INT_BAD_FIELD, INT_NOT_FOUND,
            INT_RATE_LIMITED, INT_SERVER, INT_TOO_LARGE, INT_BAD_RESPONSE, INT_HOST_BLOCKED, FILE_NOT_DROPPED, FILE_IS_FOLDER, FILE_UNREADABLE,
            FILE_COPY, CFG_UNREADABLE, CFG_NOT_OBJECT, CFG_INVALID_JSON, CFG_CHANGED, CFG_BACKUP, CFG_WRITE,
            CFG_NO_CHANGE, MCP_SERVERS_NOT_OBJECT, MCP_FOREIGN_ENTRY, MCP_EXE_MISSING, MCP_UNKNOWN_CLIENT,
            MCP_JSONC, MCP_INVALID_TOML, MCP_YAML_MANUAL, SHORTCUT_INVALID,
            SHORTCUT_NO_MODIFIER, SHORTCUT_TAKEN, UPDATE_DISABLED, UPDATE_NOTHING, UPDATE_BUSY, UPDATE_NETWORK,
            UPDATE_TIMEOUT, UPDATE_NO_RELEASE, UPDATE_MANIFEST, UPDATE_SIGNATURE, UPDATE_INSTALL, UPDATE_FAILED,
            MCPC_STORE, MCPC_UNKNOWN_SERVER, MCPC_INVALID, MCPC_LIMIT, MCPC_NEEDS_AUTH, MCPC_NEEDS_APPROVAL,
            MCPC_DISABLED, MCPC_COMMAND_NOT_FOUND, MCPC_UNSAFE_ARG, MCPC_SPAWN, MCPC_EXITED, MCPC_TIMEOUT, MCPC_HTTP,
            MCPC_NETWORK, MCPC_TOO_LARGE, MCPC_PROTOCOL, MCPC_RPC, MCPC_UNKNOWN_TOOL, MCPC_TOOL_OFF,
            MCPC_TOOL_ASK, MCPC_ARGS_TOO_LARGE, MCPC_COMMAND_CHANGED,
            MCPC_OAUTH_DISCOVERY, MCPC_OAUTH_INSECURE, MCPC_OAUTH_NO_DCR, MCPC_OAUTH_REGISTER, MCPC_OAUTH_LISTEN,
            MCPC_OAUTH_DENIED, MCPC_OAUTH_STATE, MCPC_OAUTH_TIMEOUT, MCPC_OAUTH_CANCELLED, MCPC_OAUTH_TOKEN,
            MCPC_OAUTH_KEYRING, PLANNER_STORE, PLANNER_INVALID, PLANNER_NOT_FOUND, PLANNER_LIMIT,
        ];
        let mut seen = std::collections::HashSet::new();
        for c in all {
            assert!(seen.insert(c), "duplicate {c}");
            assert!(c.starts_with("E_") || c.starts_with("I_"), "{c}");
            assert!(c.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_'), "{c}");
        }
    }
}
