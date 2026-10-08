// The bundled directory of well-known MCP servers the settings window offers
// under «افزودن سرور» (src-tauri/mcp-directory.json, embedded at build time).
//
// The file is checked once, on first use: an entry that fails validation is
// logged and left out, the rest still show. The tests below hold every bundled
// entry to the same rules, so a bad entry fails CI instead of vanishing.
//
// argFields: values the user types when adding the entry (a folder…). They are
// appended to `args` in order, so field `i` lands at `args[args.len() + i]`;
// the indexes say so explicitly and the validation insists on it, which keeps
// "insert at index" and "set args[index]" readings of the contract identical.

use std::collections::HashSet;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const RAW: &str = include_str!("../../mcp-directory.json");

const CATEGORIES: &[&str] = &["dev", "work", "data", "web", "files", "design", "commerce", "docs", "other"];
const MAX_NAME_CHARS: usize = 60;
const MAX_DESC_CHARS: usize = 200;
const MAX_HELP_CHARS: usize = 240;
const MAX_LABEL_CHARS: usize = 60;
const MAX_PLACEHOLDER_CHARS: usize = 80;
const MAX_ARGS: usize = 40;
const MAX_URL_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub fa: String,
    pub en: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvField {
    pub name: String,
    pub label: Text,
    pub secret: bool,
    pub placeholder: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgKind {
    Path,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArgField {
    pub index: usize,
    pub label: Text,
    pub placeholder: String,
    pub kind: ArgKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum DirTransport {
    Http {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<EnvField>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        arg_fields: Vec<ArgField>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    None,
    Bearer,
    Oauth,
}

/// `"none" | "bearer" | "oauth" | { "header": "X-Api-Key" }` on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DirAuth {
    Kind(AuthKind),
    Header { header: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryEntry {
    pub id: String,
    pub name: String,
    pub category: String,
    pub desc: Text,
    pub docs_url: Option<String>,
    pub transport: DirTransport,
    pub auth: DirAuth,
    pub auth_help: Option<Text>,
    /// What has to be installed first for a local server ("Node.js", "uv").
    pub needs: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryFile {
    version: u32,
    entries: Vec<serde_json::Value>,
}

// ── Validation ────────────────────────────────────────────────────────────────

/// Text shown in the settings window: non-empty, bounded, no control or bidi
/// characters (the same rule as everything else the UI shows).
fn text_ok(text: &str, max: usize) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.len() == text.len() && text.chars().count() <= max && text.chars().all(crate::roadeep::chat::shown_char)
}

fn bilingual_ok(text: &Text, max: usize) -> bool {
    text_ok(&text.fa, max) && text_ok(&text.en, max)
}

/// The same shape as a server id in mcp-servers.json, short enough that
/// `directory:<id>` is a valid `source` there.
fn id_ok(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && (2..=40).contains(&id.len())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn https_ok(raw: &str) -> bool {
    raw.len() <= MAX_URL_CHARS
        && !raw.chars().any(char::is_control)
        && reqwest::Url::parse(raw).is_ok_and(|u| {
            u.scheme() == "https" && u.host_str().is_some() && u.username().is_empty() && u.password().is_none() && u.fragment().is_none()
        })
}

/// A command-line word: no control characters (newline, NUL) and nothing a
/// `.cmd` launcher would read as syntax, so every bundled command is runnable
/// through W1's cmd.exe path without tripping its refusal.
fn arg_ok(text: &str) -> bool {
    !text.is_empty() && text.len() <= 200 && !text.chars().any(|c| c.is_control() || "\"%^&|<>".contains(c))
}

fn env_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn header_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Every problem of one entry; empty when it may be shown.
pub fn problems(entry: &DirectoryEntry) -> Vec<String> {
    let mut out = Vec::new();
    let mut check = |ok: bool, what: &str| {
        if !ok {
            out.push(what.to_string());
        }
    };
    check(id_ok(&entry.id), "id");
    check(text_ok(&entry.name, MAX_NAME_CHARS), "name");
    check(CATEGORIES.contains(&entry.category.as_str()), "category");
    check(bilingual_ok(&entry.desc, MAX_DESC_CHARS), "desc");
    check(entry.docs_url.as_deref().is_none_or(https_ok), "docsUrl");
    check(entry.auth_help.as_ref().is_none_or(|t| bilingual_ok(t, MAX_HELP_CHARS)), "authHelp");
    match &entry.transport {
        DirTransport::Http { url } => {
            check(https_ok(url), "transport.url");
            check(entry.needs.is_none(), "needs (a remote server needs nothing installed)");
        }
        DirTransport::Stdio { command, args, env, arg_fields } => {
            check(arg_ok(command) && !command.contains(['\\', '/', ' ']), "transport.command (a bare program name)");
            check(args.len() + arg_fields.len() <= MAX_ARGS && args.iter().all(|a| arg_ok(a)), "transport.args");
            let mut names = HashSet::new();
            check(
                env.iter().all(|e| {
                    env_name_ok(&e.name)
                        && names.insert(e.name.as_str())
                        && bilingual_ok(&e.label, MAX_LABEL_CHARS)
                        && text_ok(&e.placeholder, MAX_PLACEHOLDER_CHARS)
                }),
                "transport.env",
            );
            check(
                arg_fields.iter().enumerate().all(|(i, f)| {
                    f.index == args.len() + i && bilingual_ok(&f.label, MAX_LABEL_CHARS) && text_ok(&f.placeholder, MAX_PLACEHOLDER_CHARS)
                }),
                "transport.argFields (appended in order after args)",
            );
            check(entry.needs.as_deref().is_some_and(|n| text_ok(n, 40)), "needs");
            // A local server authenticates through its environment, never HTTP.
            check(entry.auth == DirAuth::Kind(AuthKind::None), "auth (stdio takes none)");
        }
    }
    if let DirAuth::Header { header } = &entry.auth {
        check(header_ok(header), "auth.header");
    }
    out
}

/// Parses the directory file; returns the valid entries and, per rejected
/// entry (or the whole file), what was wrong.
pub fn parse(raw: &str) -> (Vec<DirectoryEntry>, Vec<String>) {
    let file: DirectoryFile = match serde_json::from_str(raw) {
        Ok(file) => file,
        Err(err) => return (Vec::new(), vec![format!("mcp-directory.json: {err}")]),
    };
    let mut problems_found = Vec::new();
    if file.version != 1 {
        problems_found.push(format!("mcp-directory.json: unknown version {}", file.version));
        return (Vec::new(), problems_found);
    }
    let mut entries: Vec<DirectoryEntry> = Vec::new();
    for (i, value) in file.entries.into_iter().enumerate() {
        let label = value.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| format!("#{i}"));
        let entry: DirectoryEntry = match serde_json::from_value(value) {
            Ok(entry) => entry,
            Err(err) => {
                problems_found.push(format!("{label}: {err}"));
                continue;
            }
        };
        let mut found = problems(&entry);
        if entries.iter().any(|e| e.id == entry.id) {
            found.push("duplicate id".into());
        }
        if found.is_empty() {
            entries.push(entry);
        } else {
            problems_found.push(format!("{label}: {}", found.join(", ")));
        }
    }
    (entries, problems_found)
}

static DIRECTORY: LazyLock<Vec<DirectoryEntry>> = LazyLock::new(|| {
    let (entries, problems_found) = parse(RAW);
    // Tests reach this too; they must not write the user's log.
    if cfg!(not(test)) {
        for problem in &problems_found {
            crate::log::line(format!("mcpc directory: skipped {problem}"));
        }
    }
    entries
});

/// Settings → MCP servers → «افزودن سرور»: the bundled, validated directory.
#[tauri::command]
pub fn mcpc_directory() -> Vec<DirectoryEntry> {
    DIRECTORY.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_entry_is_valid() {
        let (entries, problems_found) = parse(RAW);
        assert!(problems_found.is_empty(), "{problems_found:#?}");
        assert!(entries.len() >= 20, "{} entries", entries.len());
        assert_eq!(mcpc_directory().len(), entries.len());
        let ids: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), entries.len(), "ids are unique");
        for category in CATEGORIES {
            if *category != "design" && *category != "files" {
                assert!(entries.iter().any(|e| e.category == *category), "no entry in {category}");
            }
        }
    }

    /// What the settings window sends to mcpc_add for an entry must pass the
    /// store's own validation, or "Add" would fail for a bundled server.
    #[test]
    fn every_entry_is_accepted_by_the_store() {
        use super::super::store::{self, Auth, Transport};
        let (entries, _) = parse(RAW);
        for entry in entries {
            let transport = match &entry.transport {
                DirTransport::Http { url } => Transport::Http { url: url.clone() },
                DirTransport::Stdio { command, args, env, arg_fields } => {
                    let mut args = args.clone();
                    args.extend(arg_fields.iter().map(|_| r"C:\Users\me\Documents".to_string()));
                    Transport::Stdio { command: command.clone(), args, env: env.iter().map(|e| e.name.clone()).collect() }
                }
            };
            let auth = match &entry.auth {
                DirAuth::Kind(AuthKind::None) => Auth::None,
                DirAuth::Kind(AuthKind::Bearer) => Auth::Bearer,
                DirAuth::Kind(AuthKind::Oauth) => Auth::Oauth,
                DirAuth::Header { header } => Auth::Header { name: header.clone() },
            };
            let transport = store::clean_transport(&transport).unwrap_or_else(|e| panic!("{}: {e}", entry.id));
            store::clean_auth(&auth, &transport).unwrap_or_else(|e| panic!("{}: {e}", entry.id));
            assert!(crate::secrets::mcpc_slot_valid("token"));
            if let DirTransport::Stdio { env, .. } = &entry.transport {
                for e in env {
                    assert!(crate::secrets::mcpc_slot_valid(&format!("env:{}", e.name)), "{}: {}", entry.id, e.name);
                }
            }
        }
    }

    #[test]
    fn serializes_to_the_front_end_shape() {
        let (entries, _) = parse(RAW);
        let fs = entries.iter().find(|e| e.id == "filesystem").unwrap();
        let json = serde_json::to_value(fs).unwrap();
        assert_eq!(json["auth"], "none");
        assert_eq!(json["docsUrl"], "https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem");
        assert_eq!(json["transport"]["type"], "stdio");
        assert_eq!(json["transport"]["argFields"][0]["index"], 2);
        assert_eq!(json["transport"]["argFields"][0]["kind"], "path");
        assert_eq!(json["needs"], "Node.js");
        let github = serde_json::to_value(entries.iter().find(|e| e.id == "github").unwrap()).unwrap();
        assert_eq!(github["transport"], serde_json::json!({ "type": "http", "url": "https://api.githubcopilot.com/mcp/" }));
        assert_eq!(github["auth"], "bearer");
        assert!(github["transport"].get("argFields").is_none());
        let header: DirAuth = serde_json::from_str(r#"{"header":"X-Api-Key"}"#).unwrap();
        assert_eq!(serde_json::to_value(&header).unwrap(), serde_json::json!({ "header": "X-Api-Key" }));
    }

    fn base() -> serde_json::Value {
        serde_json::json!({
            "id": "demo", "name": "Demo", "category": "dev",
            "desc": { "en": "A demo.", "fa": "نمونه." }, "docsUrl": null,
            "transport": { "type": "http", "url": "https://mcp.example.com/mcp" },
            "auth": "oauth", "authHelp": null, "needs": null
        })
    }

    fn check(entry: serde_json::Value) -> Vec<String> {
        let file = serde_json::json!({ "version": 1, "entries": [entry] }).to_string();
        parse(&file).1
    }

    #[test]
    fn bad_entries_are_refused() {
        assert!(check(base()).is_empty());
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("/id", "Demo".into()),
            ("/id", "a".into()),
            ("/category", "games".into()),
            ("/desc/fa", "".into()),
            ("/desc/en", "evil \u{202E}txt".into()),
            ("/name", "x".repeat(61).into()),
            ("/transport/url", "http://mcp.example.com/mcp".into()),
            ("/transport/url", "https://user:pw@mcp.example.com/".into()),
            ("/docsUrl", "javascript:alert(1)".into()),
            ("/auth", "basic".into()),
            ("/auth", serde_json::json!({ "header": "Bad Header" })),
            ("/needs", "Node.js".into()),
        ];
        for (pointer, value) in cases {
            let mut entry = base();
            *entry.pointer_mut(pointer).unwrap() = value.clone();
            assert!(!check(entry).is_empty(), "{pointer} = {value} should be refused");
        }
        let mut unknown = base();
        unknown["extra"] = true.into();
        assert!(!check(unknown).is_empty(), "unknown fields are refused");
    }

    #[test]
    fn bad_local_entries_are_refused() {
        let stdio = |transport: serde_json::Value, needs: serde_json::Value, auth: &str| {
            let mut entry = base();
            entry["transport"] = transport;
            entry["needs"] = needs;
            entry["auth"] = auth.into();
            check(entry)
        };
        let good = serde_json::json!({ "type": "stdio", "command": "npx", "args": ["-y", "pkg"], "env": [],
            "argFields": [{ "index": 2, "label": { "en": "Folder", "fa": "پوشه" }, "placeholder": "C:\\x", "kind": "path" }] });
        assert!(stdio(good.clone(), "Node.js".into(), "none").is_empty());
        assert!(!stdio(good.clone(), serde_json::Value::Null, "none").is_empty(), "stdio says what it needs");
        assert!(!stdio(good.clone(), "Node.js".into(), "bearer").is_empty(), "stdio has no HTTP auth");
        let mut gap = good.clone();
        gap["argFields"][0]["index"] = 1.into();
        assert!(!stdio(gap, "Node.js".into(), "none").is_empty(), "argFields come after args");
        let mut meta = good.clone();
        meta["args"] = serde_json::json!(["-y", "pkg & calc"]);
        assert!(!stdio(meta, "Node.js".into(), "none").is_empty(), "shell syntax in an argument");
        let mut path = good.clone();
        path["command"] = r"C:\tools\x.exe".into();
        assert!(!stdio(path, "Node.js".into(), "none").is_empty(), "bare program names only");
        let mut env = good.clone();
        env["env"] = serde_json::json!([
            { "name": "API_KEY", "label": { "en": "Key", "fa": "کلید" }, "secret": true, "placeholder": "sk-…" },
            { "name": "API_KEY", "label": { "en": "Key", "fa": "کلید" }, "secret": true, "placeholder": "sk-…" }
        ]);
        assert!(!stdio(env, "Node.js".into(), "none").is_empty(), "duplicate env names");
    }

    #[test]
    fn duplicates_and_broken_files_are_reported() {
        let file = serde_json::json!({ "version": 1, "entries": [base(), base()] }).to_string();
        let (entries, problems_found) = parse(&file);
        assert_eq!(entries.len(), 1);
        assert_eq!(problems_found.len(), 1);
        assert!(!parse("not json").1.is_empty());
        assert!(parse(&serde_json::json!({ "version": 2, "entries": [base()] }).to_string()).0.is_empty());
    }
}
