// API keys and the Roadeep session live in the Windows Credential Manager,
// never on disk and never in the front end — the island can only ask whether a
// user-entered key is present.

use keyring::Entry;

use crate::catalog::{Catalog, Kind};

const SERVICE: &str = "com.roadeep.desktop";

/// Every key the user may enter in the settings window for a native service.
/// Catalog services add theirs ("x.<id>.<name>", see `user_key`); anything
/// else is refused by the `secret_*` commands.
pub const KNOWN_KEYS: &[&str] = &[
    "n8n-url",
    "n8n-api-key",
    "vercel-token",
    "github-token",
    "stripe-api-key",
    "resend-api-key",
    "notion-api-key",
    "calcom-api-key",
    "gitlab-token",
    "gitlab-url",
    "sentry-token",
    "sentry-org",
    "sentry-url",
    "linear-api-key",
    "netlify-token",
    "cloudflare-token",
    "cloudflare-account-id",
];

/// The Roadeep session. Written and read by Rust only: these are deliberately
/// not in KNOWN_KEYS, so no command can set, clear or probe them.
pub const SESSION_KEYS: &[&str] = &["roadeep-access-token", "roadeep-refresh-token", "roadeep-user"];

/// Removed in the Roadeep edition; purged once at startup.
pub(crate) const LEGACY_KEYS: &[&str] = &["anthropic-api-key"];

fn entry_in(list: &[&str], key: &str) -> Option<Entry> {
    if !list.contains(&key) {
        return None;
    }
    Entry::new(SERVICE, key).ok()
}

fn read(entry: Option<Entry>) -> Option<String> {
    entry?.get_password().ok().filter(|v| !v.is_empty())
}

fn delete(entry: Entry) -> Result<(), String> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// A native key, or a field of a service in the loaded catalog — so a window
/// can't use the `x.` prefix to park arbitrary entries in the Credential Manager.
fn user_key(key: &str, catalog: &Catalog) -> bool {
    KNOWN_KEYS.contains(&key) || catalog.field_for_key(key).is_some()
}

fn user_entry(key: &str) -> Option<Entry> {
    if !user_key(key, crate::catalog::get()) {
        return None;
    }
    Entry::new(SERVICE, key).ok()
}

pub fn get(key: &str) -> Option<String> {
    read(user_entry(key))
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    let entry = user_entry(key).ok_or_else(|| format!("unknown key {key}"))?;
    if value.is_empty() {
        return delete(entry);
    }
    entry.set_password(value).map_err(|e| e.to_string())
}

pub fn clear(key: &str) -> Result<(), String> {
    delete(user_entry(key).ok_or_else(|| format!("unknown key {key}"))?)
}

pub fn present(key: &str) -> bool {
    get(key).is_some()
}

/// Stored values that are settings, not secrets, and may be shown back to the
/// user so they can check them. Everything else stays write-only from the UI.
pub const PUBLIC_KEYS: &[&str] = &["n8n-url", "gitlab-url", "sentry-org", "sentry-url", "cloudflare-account-id"];

/// A catalog field is public when its kind says so (text, url); a secret-kind
/// field never comes back.
fn public_key(key: &str, catalog: &Catalog) -> bool {
    PUBLIC_KEYS.contains(&key) || catalog.field_for_key(key).is_some_and(|(_, f)| f.kind != Kind::Secret)
}

pub fn public_value(key: &str) -> Option<String> {
    if !public_key(key, crate::catalog::get()) {
        return None;
    }
    get(key)
}

// ── Roadeep session (Rust-only) ───────────────────────────────────────────────

pub fn session_get(key: &str) -> Option<String> {
    read(entry_in(SESSION_KEYS, key))
}

pub fn session_set(key: &str, value: &str) -> Result<(), String> {
    let entry = entry_in(SESSION_KEYS, key).ok_or_else(|| format!("unknown session key {key}"))?;
    if value.is_empty() {
        return delete(entry);
    }
    entry.set_password(value).map_err(|e| e.to_string())
}

pub fn session_clear(key: &str) -> Result<(), String> {
    delete(entry_in(SESSION_KEYS, key).ok_or_else(|| format!("unknown session key {key}"))?)
}

// ── MCP client servers (Rust-only) ────────────────────────────────────────────
//
// Tokens, environment values and OAuth state of the user's MCP servers
// (mcpc/). Entry name `mcpc.<server id>.<slot>`. Like the session, these are
// not reachable through the `secret_*` commands: `user_key` never accepts the
// `mcpc.` prefix, and only the mcpc commands (which validate the server id and
// slot first) write here.

/// `token` (bearer or header value), `oauth` (JSON, mcpc/oauth.rs) or
/// `env:<NAME>` (the value of one environment variable of a stdio server).
pub fn mcpc_slot_valid(slot: &str) -> bool {
    match slot.strip_prefix("env:") {
        Some(name) => mcpc_env_name_valid(name),
        None => matches!(slot, "token" | "oauth"),
    }
}

/// An environment variable name a server may ask for: `[A-Za-z_][A-Za-z0-9_]{0,63}`.
pub fn mcpc_env_name_valid(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Same rule as the config file's server ids (mcpc/store.rs).
fn mcpc_id_valid(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && (2..=41).contains(&id.len())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn mcpc_entry(id: &str, slot: &str) -> Option<Entry> {
    if !mcpc_id_valid(id) || !mcpc_slot_valid(slot) {
        return None;
    }
    Entry::new(SERVICE, &format!("mcpc.{id}.{slot}")).ok()
}

pub fn mcpc_get(id: &str, slot: &str) -> Option<String> {
    read(mcpc_entry(id, slot))
}

/// An empty value deletes the entry.
pub fn mcpc_set(id: &str, slot: &str, value: &str) -> Result<(), String> {
    let entry = mcpc_entry(id, slot).ok_or_else(|| format!("unknown mcpc slot {slot}"))?;
    if value.is_empty() {
        return delete(entry);
    }
    entry.set_password(value).map_err(|e| e.to_string())
}

/// Best effort; a failure is logged (without the value, which is never read here).
pub fn mcpc_clear(id: &str, slot: &str) {
    let Some(entry) = mcpc_entry(id, slot) else {
        crate::log::line(format!("secrets: refused to clear mcpc slot {slot} of {id}"));
        return;
    };
    if let Err(e) = delete(entry) {
        crate::log::line(format!("secrets: could not clear mcpc slot {slot} of {id}: {e}"));
    }
}

/// The fixed slots of a server. The Credential Manager can't be listed by
/// prefix through keyring, so `env:<NAME>` slots are cleared by the caller,
/// which knows the names (mcpc::remove does both).
pub fn mcpc_clear_all(id: &str) {
    for slot in ["token", "oauth"] {
        mcpc_clear(id, slot);
    }
}

/// Best effort: deletes credentials an older build left behind. Returns the
/// keys that were actually removed so the caller can log them.
pub fn purge_legacy() -> Vec<String> {
    let mut removed = Vec::new();
    for key in LEGACY_KEYS {
        let Ok(entry) = Entry::new(SERVICE, key) else { continue };
        match entry.delete_credential() {
            Ok(()) => removed.push((*key).to_string()),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => crate::log::line(format!("secrets: could not purge {key}: {e}")),
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys_are_not_user_keys() {
        for key in SESSION_KEYS {
            assert!(!KNOWN_KEYS.contains(key), "{key} must stay out of reach of the secret_* commands");
        }
        assert!(!KNOWN_KEYS.contains(&"anthropic-api-key"));
    }

    #[test]
    fn only_non_secret_settings_can_be_read_back() {
        assert_eq!(PUBLIC_KEYS, &["n8n-url", "gitlab-url", "sentry-org", "sentry-url", "cloudflare-account-id"]);
        for key in PUBLIC_KEYS {
            assert!(KNOWN_KEYS.contains(key));
            assert!(!SESSION_KEYS.contains(key));
        }
        assert_eq!(public_value("github-token"), None);
        assert_eq!(public_value("cloudflare-token"), None);
        assert_eq!(public_value("roadeep-access-token"), None);
    }

    #[test]
    fn catalog_keys_are_accepted_only_for_loaded_fields() {
        let catalog = crate::catalog::fixtures::catalog();
        for key in ["x.supa.token", "x.supa.org", "x.shop-woo.siteUrl", "github-token"] {
            assert!(user_key(key, &catalog), "{key}");
        }
        for key in ["x.supa.nope", "x.unknown.token", "x.supa", "x.", "roadeep-access-token", "anthropic-api-key"] {
            assert!(!user_key(key, &catalog), "{key}");
        }
        // Text and url fields read back; secret ones never.
        assert!(public_key("x.supa.org", &catalog));
        assert!(public_key("x.shop-woo.siteUrl", &catalog));
        assert!(!public_key("x.supa.token", &catalog));
        assert!(!public_key("x.shop-woo.secret", &catalog));
        assert!(!public_key("x.nope.org", &catalog));
        for key in SESSION_KEYS {
            assert!(!user_key(key, &catalog) && !public_key(key, &catalog));
        }
    }

    #[test]
    fn mcpc_slots_are_validated_and_out_of_reach_of_the_ui() {
        for slot in ["token", "oauth", "env:API_KEY", "env:_x1"] {
            assert!(mcpc_slot_valid(slot), "{slot}");
        }
        for slot in ["", "env:", "env:1A", "env:A-B", "env:A B", "tok", "token.x", &format!("env:{}", "A".repeat(65))] {
            assert!(!mcpc_slot_valid(slot), "{slot}");
        }
        for id in ["gh-3f9a", "a1", "notion"] {
            assert!(mcpc_id_valid(id), "{id}");
        }
        for id in ["", "a", "-ab", "Gh", "a.b", "a_b", &"a".repeat(42)] {
            assert!(!mcpc_id_valid(id), "{id}");
        }
        assert!(mcpc_entry("..", "token").is_none());
        assert_eq!(mcpc_get("gh", "nope"), None);
        assert!(mcpc_set("gh", "env:", "x").is_err());
        let catalog = crate::catalog::fixtures::catalog();
        assert!(!user_key("mcpc.gh.token", &catalog));
        assert!(!public_key("mcpc.gh.token", &catalog));
    }
}
