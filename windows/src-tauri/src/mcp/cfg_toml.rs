// Codex: `[mcp_servers.roadeep]` in config.toml.
//
// Edited with toml_edit, which keeps everything it doesn't touch — comments,
// key order, quoting, blank lines — byte for byte. Only the `roadeep` table is
// added, replaced or removed (and `mcp_servers` itself when it ends up empty).

use std::path::Path;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, TableLike, Value};

use super::clients::{classify, decode, encode, eol_of, others_line, Edit, Found, ENTRY, SHOWN_FIELDS};
use crate::errors;

const CONTAINER: &str = "mcp_servers";

fn parse(text: &str, path: &Path) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>().map_err(|e| {
        // The parser's message spans several lines (a caret under the column);
        // its first line names the position, which is what the user needs.
        let detail = e.to_string();
        let detail = detail.lines().next().unwrap_or("").trim().to_string();
        errors::coded(errors::MCP_INVALID_TOML, &[&path.display().to_string(), &detail])
    })
}

fn not_a_table(path: &Path) -> String {
    errors::coded(errors::MCP_SERVERS_NOT_OBJECT, &[CONTAINER, &path.display().to_string()])
}

fn servers<'a>(doc: &'a DocumentMut, path: &Path) -> Result<Option<&'a dyn TableLike>, String> {
    match doc.get(CONTAINER) {
        None => Ok(None),
        Some(item) => item.as_table_like().map(Some).ok_or_else(|| not_a_table(path)),
    }
}

fn command_of(entry: &Item) -> Option<&str> {
    entry.as_table_like()?.get("command")?.as_str()
}

fn found_in(entry: Option<&Item>) -> Found {
    match entry {
        None => Found::Missing,
        Some(e) => classify(command_of(e)),
    }
}

/// Already exactly what we would write: nothing to do, not even a reformat.
fn is_current(entry: &Item, command: &str) -> bool {
    let Some(t) = entry.as_table_like() else { return false };
    let args_empty = t.get("args").and_then(Item::as_array).map(|a| a.is_empty()).unwrap_or(false);
    t.len() == 2 && command_of(entry) == Some(command) && args_empty
}

fn fill(t: &mut dyn TableLike, command: &str) {
    t.insert("command", Item::Value(Value::from(command)));
    t.insert("args", Item::Value(Value::Array(Array::new())));
}

/// `key = value` lines for the diff; values outside SHOWN_FIELDS are masked.
fn entry_lines(entry: &Item, sign: char) -> Vec<String> {
    let mut out = vec![format!("{sign} [{CONTAINER}.{ENTRY}]")];
    let Some(t) = entry.as_table_like() else {
        out.push(format!("{sign} …"));
        return out;
    };
    for (key, item) in t.iter() {
        let shown = match item {
            Item::Value(v) if SHOWN_FIELDS.contains(&key) && !v.is_inline_table() => v.to_string().trim().to_string(),
            other => match other.as_table_like() {
                Some(inner) => {
                    let keys: Vec<String> = inner.iter().map(|(k, _)| format!("{k} = \"…\"")).collect();
                    format!("{{ {} }}", keys.join(", "))
                }
                None => "\"…\"".to_string(),
            },
        };
        out.push(format!("{sign} {key} = {shown}"));
    }
    out
}

fn render_diff(old: Option<&Item>, new: Option<&Item>, others: &[String]) -> String {
    let mut out = vec!["  …".to_string()];
    out.extend(others_line("    ", others));
    if let Some(e) = old {
        out.extend(entry_lines(e, '-'));
    }
    if let Some(e) = new {
        out.extend(entry_lines(e, '+'));
    }
    out.push("  …".into());
    out.join("\n")
}

// ── What install.rs calls ─────────────────────────────────────────────────────

pub fn found(bytes: &[u8], path: &Path) -> Result<Found, String> {
    let (_, text) = decode(bytes, path)?;
    let doc = parse(text, path)?;
    Ok(found_in(servers(&doc, path)?.and_then(|s| s.get(ENTRY))))
}

pub fn edit(bytes: &[u8], path: &Path, install: bool, command: &str) -> Result<Edit, String> {
    let (bom, text) = decode(bytes, path)?;
    let mut doc = parse(text, path)?;
    let unchanged = || Edit { diff: errors::CFG_NO_CHANGE.into(), bytes: bytes.to_vec() };

    let (old, others) = match servers(&doc, path)? {
        Some(s) => (
            s.get(ENTRY).cloned(),
            s.iter().map(|(k, _)| k.to_string()).filter(|k| k != ENTRY).collect::<Vec<_>>(),
        ),
        None => (None, Vec::new()),
    };

    if install {
        if old.as_ref().map(|e| is_current(e, command)).unwrap_or(false) {
            return Ok(unchanged());
        }
        match doc.get_mut(CONTAINER) {
            None => {
                // `[mcp_servers]` itself is never printed: only `[mcp_servers.roadeep]`.
                let mut container = Table::new();
                container.set_implicit(true);
                let mut ours = Table::new();
                fill(&mut ours, command);
                container.insert(ENTRY, Item::Table(ours));
                doc.insert(CONTAINER, Item::Table(container));
            }
            Some(item) => {
                let is_table = item.is_table();
                let servers = item.as_table_like_mut().ok_or_else(|| not_a_table(path))?;
                match servers.get_mut(ENTRY) {
                    // Refilled in place: the table keeps its spot in the file.
                    Some(Item::Table(t)) => {
                        t.clear();
                        t.set_implicit(false);
                        fill(t, command);
                    }
                    Some(Item::Value(Value::InlineTable(t))) => {
                        t.clear();
                        fill(t, command);
                    }
                    Some(other) => {
                        let mut t = InlineTable::new();
                        fill(&mut t, command);
                        *other = Item::Value(Value::InlineTable(t));
                    }
                    None if is_table => {
                        let mut t = Table::new();
                        fill(&mut t, command);
                        servers.insert(ENTRY, Item::Table(t));
                    }
                    None => {
                        let mut t = InlineTable::new();
                        fill(&mut t, command);
                        servers.insert(ENTRY, Item::Value(Value::InlineTable(t)));
                    }
                }
            }
        }
    } else {
        match found_in(old.as_ref()) {
            Found::Missing => return Ok(unchanged()),
            Found::Foreign => {
                return Err(errors::coded(errors::MCP_FOREIGN_ENTRY, &[ENTRY, &path.display().to_string()]))
            }
            Found::Ours | Found::Legacy => {}
        }
        let empty = {
            let item = doc.get_mut(CONTAINER).ok_or_else(|| not_a_table(path))?;
            let servers = item.as_table_like_mut().ok_or_else(|| not_a_table(path))?;
            servers.remove(ENTRY);
            servers.is_empty()
        };
        if empty {
            doc.remove(CONTAINER);
        }
    }

    let new = servers(&doc, path)?.and_then(|s| s.get(ENTRY)).cloned();
    let diff = render_diff(old.as_ref(), new.as_ref(), &others);
    // toml_edit writes the lines it adds with "\n"; a CRLF file stays CRLF.
    let mut out = doc.to_string();
    if eol_of(text) == "\r\n" {
        out = out.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    Ok(Edit { diff, bytes: encode(bom, out) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = r"C:\Users\x\AppData\Local\com.roadeep.desktop\bin\roadeep-mcp.exe";

    fn p() -> &'static Path {
        Path::new(r"C:\Users\x\.codex\config.toml")
    }

    const SAMPLE: &str = r#"# Codex settings
model = "gpt-5"   # the default model

[mcp_servers.node_repl]
command = "node"
args = ["repl.js"]

[mcp_servers.node_repl.env]
TOKEN = "SECRET"   # keep this private

# Design tokens server
[mcp_servers.designmd]
url = "https://example.com/mcp"

[projects.'d:\roadeep']
trust_level = "trusted"
"#;

    fn install(text: &str) -> Edit {
        edit(text.as_bytes(), p(), true, CMD).unwrap()
    }

    fn remove(text: &str) -> Result<Edit, String> {
        edit(text.as_bytes(), p(), false, CMD)
    }

    fn s(e: &Edit) -> String {
        String::from_utf8(e.bytes.clone()).unwrap()
    }

    #[test]
    fn adds_next_to_other_servers_keeping_every_byte_of_them() {
        let e = install(SAMPLE);
        let out = s(&e);
        let block = format!("\n[mcp_servers.roadeep]\ncommand = {}\nargs = []\n", Value::from(CMD));
        assert!(out.contains(&block), "{out}");
        assert_eq!(out.replacen(&block, "", 1), SAMPLE, "everything else is untouched");
        // It sits with the other servers, before the next unrelated table.
        assert!(out.find("[mcp_servers.roadeep]").unwrap() < out.find("[projects.").unwrap(), "{out}");

        let doc: DocumentMut = out.parse().unwrap();
        assert_eq!(doc["mcp_servers"]["roadeep"]["command"].as_str(), Some(CMD));
        assert_eq!(found(out.as_bytes(), p()).unwrap(), Found::Ours);

        assert!(e.diff.contains("2 other server(s) unchanged: node_repl, designmd"), "{}", e.diff);
        assert!(e.diff.contains("+ [mcp_servers.roadeep]") && e.diff.contains("+ args = []"), "{}", e.diff);
        assert!(!e.diff.contains("SECRET"), "{}", e.diff);

        // Again: nothing to do. Then removing gives back the original bytes.
        assert_eq!(install(&out).diff, errors::CFG_NO_CHANGE);
        assert_eq!(s(&remove(&out).unwrap()), SAMPLE);
    }

    #[test]
    fn an_empty_or_missing_file_gets_just_our_table() {
        let out = s(&install(""));
        assert_eq!(out, format!("[mcp_servers.roadeep]\ncommand = {}\nargs = []\n", Value::from(CMD)));
        assert_eq!(s(&remove(&out).unwrap()), "", "removing drops the empty mcp_servers too");

        let plain = "model = \"o3\"\n";
        let out = s(&install(plain));
        assert!(out.starts_with(plain), "{out}");
        assert_eq!(s(&remove(&out).unwrap()), plain);
    }

    #[test]
    fn a_stale_entry_of_ours_is_refreshed_in_place() {
        let stale = SAMPLE.replace(
            "# Design tokens server",
            "[mcp_servers.roadeep]\ncommand = 'C:\\old\\roadeep-mcp.exe'\nargs = [\"--x\"]\n\n# Design tokens server",
        );
        let e = install(&stale);
        let out = s(&e);
        assert!(out.contains(&format!("[mcp_servers.roadeep]\ncommand = {}\nargs = []\n", Value::from(CMD))), "{out}");
        assert!(!out.contains("old"), "{out}");
        assert!(out.find("[mcp_servers.roadeep]").unwrap() < out.find("[mcp_servers.designmd]").unwrap(), "{out}");
        assert!(out.contains("# Design tokens server\n[mcp_servers.designmd]"), "{out}");
        assert!(e.diff.contains("- command = 'C:\\old\\roadeep-mcp.exe'"), "{}", e.diff);
    }

    #[test]
    fn a_foreign_entry_is_replaced_on_install_but_never_removed() {
        let foreign = format!(
            "{SAMPLE}\n[mcp_servers.roadeep]\nurl = \"https://roadeep.com/mcp\"\nbearer_token = \"SECRET9\"\n\n[mcp_servers.roadeep.env]\nKEY = \"SECRET8\"\n"
        );
        assert_eq!(found(foreign.as_bytes(), p()).unwrap(), Found::Foreign);
        let err = remove(&foreign).err().unwrap();
        assert!(err.starts_with(errors::MCP_FOREIGN_ENTRY), "{err}");

        let e = install(&foreign);
        assert!(!e.diff.contains("SECRET"), "{}", e.diff);
        assert!(e.diff.contains("- bearer_token = \"…\"") && e.diff.contains("- env = { KEY = \"…\" }"), "{}", e.diff);
        let out = s(&e);
        assert!(!out.contains("SECRET8") && !out.contains("SECRET9"), "{out}");
        assert_eq!(found(out.as_bytes(), p()).unwrap(), Found::Ours);
    }

    #[test]
    fn inline_containers_are_edited_inline() {
        let inline = "mcp_servers = { other = { command = \"x\" } }\n";
        let out = s(&install(inline));
        let doc: DocumentMut = out.parse().unwrap();
        assert_eq!(doc["mcp_servers"]["roadeep"]["command"].as_str(), Some(CMD));
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("x"));
        let back = s(&remove(&out).unwrap());
        let doc: DocumentMut = back.parse().unwrap();
        assert!(doc["mcp_servers"].get("roadeep").is_none());
    }

    #[test]
    fn bad_files_are_refused_and_bom_and_crlf_survive() {
        let err = edit(b"model = = 1", p(), true, CMD).err().unwrap();
        assert!(err.starts_with(errors::MCP_INVALID_TOML) && !err.contains('\n'), "{err}");
        let err = edit(b"mcp_servers = 3\n", p(), true, CMD).err().unwrap();
        assert!(err.starts_with(errors::MCP_SERVERS_NOT_OBJECT), "{err}");

        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"model = \"o3\"\r\n");
        let e = edit(&bytes, p(), true, CMD).unwrap();
        assert!(e.bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8(e.bytes[3..].to_vec()).unwrap();
        assert!(text.contains("[mcp_servers.roadeep]\r\n") && !text.replace("\r\n", "").contains('\n'), "{text:?}");
        let back = edit(&e.bytes, p(), false, CMD).unwrap();
        assert_eq!(back.bytes, bytes);
    }
}
