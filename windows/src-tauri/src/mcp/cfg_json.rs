// The JSON clients (Claude Code, Claude Desktop, VS Code, Cursor, Windsurf,
// Gemini CLI, Cline, Kiro, OpenCode): one code path, parametrised by the key
// the servers live under and the shape of our entry.
//
// The file is parsed, our entry is set or removed, and the whole document is
// written back in its key order with the indentation (tabs or N spaces) and
// line endings it came with. JSON with comments (JSONC) is refused, never
// rewritten: serde_json would drop the comments.

use std::path::Path;

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::clients::{classify, decode, eol_of, is_ours, others_line, Edit, Found, Shape, ENTRY, SHOWN_FIELDS};
use crate::errors;

pub fn entry_for(shape: Shape, command: &str) -> Value {
    match shape {
        Shape::ClaudeCode => json!({ "type": "stdio", "command": command, "args": [], "env": {} }),
        Shape::Plain => json!({ "command": command, "args": [] }),
        Shape::VsCode => json!({ "type": "stdio", "command": command, "args": [] }),
        Shape::Cline => json!({ "command": command, "args": [], "disabled": false }),
        Shape::OpenCode => json!({ "type": "local", "command": [command], "enabled": true }),
    }
}

/// `command` is a string everywhere but OpenCode, where it is `[exe, args…]`.
fn entry_command(entry: &Value) -> Option<&str> {
    let command = match entry.get("command") {
        Some(Value::Array(parts)) => parts.first(),
        other => other,
    };
    command.and_then(Value::as_str)
}

fn entry_is_ours(entry: &Value) -> bool {
    entry_command(entry).map(is_ours).unwrap_or(false)
}

/// What to paste by hand when the file can't be edited safely.
pub fn snippet(container: &str, shape: Shape, command: &str) -> String {
    serde_json::to_string_pretty(&json!({ container: { ENTRY: entry_for(shape, command) } })).unwrap_or_default()
}

/// Same rules as hooks.rs: a missing or blank file is `{}`, a UTF-8 BOM is
/// stripped, and anything unparseable is an error — never an empty object
/// that would then be written over the user's state.
fn parse(bytes: &[u8], path: &Path, manual: impl FnOnce() -> String) -> Result<(Value, String), String> {
    let (_, text) = decode(bytes, path)?;
    if text.trim().is_empty() {
        return Ok((json!({}), text.to_string()));
    }
    let shown = path.display().to_string();
    match serde_json::from_str::<Value>(text) {
        Ok(v) if v.is_object() => Ok((v, text.to_string())),
        Ok(_) => Err(errors::coded(errors::CFG_NOT_OBJECT, &[&shown])),
        Err(err) => {
            // Valid once comments and trailing commas are gone: it's JSONC,
            // which the app reads fine but we can't write without losing the
            // comments. The user gets the entry to paste instead.
            if serde_json::from_str::<Value>(&strip_jsonc(text)).is_ok() {
                Err(format!("{}\n{}", errors::coded(errors::MCP_JSONC, &[&shown]), manual()))
            } else {
                Err(errors::coded(errors::CFG_INVALID_JSON, &[&shown, &err.to_string()]))
            }
        }
    }
}

/// Drops `//` and `/* */` comments and trailing commas outside strings. Only
/// used to tell JSONC from broken JSON; its output is never written.
fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                i += 1;
                if chars[i - 1] == '"' {
                    break;
                }
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn servers<'a>(config: &'a Value, container: &str, path: &Path) -> Result<Option<&'a Map<String, Value>>, String> {
    match config.get(container) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => Err(errors::coded(errors::MCP_SERVERS_NOT_OBJECT, &[container, &path.display().to_string()])),
    }
}

/// The config with our entry added (or refreshed); every other key, and every
/// other server, untouched and in place.
fn merged(existing: &Value, container: &str, entry: Value, path: &Path) -> Result<Value, String> {
    let mut root = existing.as_object().cloned().unwrap_or_default();
    let mut map = servers(existing, container, path)?.cloned().unwrap_or_default();
    map.insert(ENTRY.into(), entry);
    root.insert(container.into(), Value::Object(map));
    Ok(Value::Object(root))
}

/// The config without our entry. A `roadeep` entry that isn't ours is refused,
/// not removed: it belongs to whoever added it.
fn without_ours(existing: &Value, container: &str, path: &Path) -> Result<Value, String> {
    let mut root = existing.as_object().cloned().unwrap_or_default();
    let Some(current) = servers(existing, container, path)? else { return Ok(Value::Object(root)) };
    let mut map = current.clone();
    match map.get(ENTRY) {
        None => return Ok(Value::Object(root)),
        Some(entry) if !entry_is_ours(entry) => {
            return Err(errors::coded(errors::MCP_FOREIGN_ENTRY, &[ENTRY, &path.display().to_string()]))
        }
        Some(_) => {
            map.remove(ENTRY);
        }
    }
    if map.is_empty() {
        root.remove(container);
    } else {
        root.insert(container.into(), Value::Object(map));
    }
    Ok(Value::Object(root))
}

/// Only the fields in SHOWN_FIELDS keep their values; anything else (env,
/// headers, tokens) shows its keys at most.
fn redacted(entry: &Value) -> Value {
    let Value::Object(map) = entry else { return json!("…") };
    let mut out = Map::new();
    for (key, value) in map {
        let shown = if SHOWN_FIELDS.contains(&key.as_str()) {
            value.clone()
        } else if let Value::Object(inner) = value {
            Value::Object(inner.keys().map(|k| (k.clone(), json!("…"))).collect())
        } else {
            json!("…")
        };
        out.insert(key.clone(), shown);
    }
    Value::Object(out)
}

fn entry_lines(entry: &Value, sign: char) -> Vec<String> {
    let text = serde_json::to_string_pretty(&json!({ ENTRY: redacted(entry) })).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    // Drop the wrapping braces; keep the `"roadeep": {…}` lines.
    lines[1..lines.len().saturating_sub(1)].iter().map(|l| format!("{sign}   {l}")).collect()
}

/// A diff of the one entry that changes, inside a sketch of where it lives.
/// Other servers are summarised by name: their contents are not ours to show.
fn render_diff(before: &Value, after: &Value, container: &str, path: &Path) -> String {
    let empty = Map::new();
    let old = servers(before, container, path).ok().flatten().unwrap_or(&empty);
    let new = servers(after, container, path).ok().flatten().unwrap_or(&empty);
    let (old_entry, new_entry) = (old.get(ENTRY), new.get(ENTRY));
    if old_entry == new_entry {
        return errors::CFG_NO_CHANGE.into();
    }
    let others: Vec<String> = old.keys().filter(|k| k.as_str() != ENTRY).cloned().collect();

    let mut out = vec!["  {".to_string(), "    …".to_string()];
    let sign = match (old.is_empty(), new.is_empty()) {
        (true, false) => '+',
        (false, true) => '-',
        _ => ' ',
    };
    out.push(format!("{sign}   \"{container}\": {{"));
    out.extend(others_line("      ", &others));
    if let Some(entry) = old_entry {
        out.extend(entry_lines(entry, '-'));
    }
    if let Some(entry) = new_entry {
        out.extend(entry_lines(entry, '+'));
    }
    out.push(format!("{sign}   }}"));
    out.push("    …".into());
    out.push("  }".into());
    out.join("\n")
}

/// The indent of the first indented line: a tab, or that many spaces
/// (2, like Claude Code, when the file has none yet).
fn indent_of(text: &str) -> Vec<u8> {
    for line in text.lines().skip(1) {
        let ws: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        if ws.starts_with('\t') {
            return b"\t".to_vec();
        }
        if !ws.is_empty() {
            return vec![b' '; ws.len()];
        }
    }
    b"  ".to_vec()
}

/// Pretty JSON in the file's own indent and line endings, keeping whether it
/// ended with a newline.
fn serialize(value: &Value, original: &str) -> Result<Vec<u8>, String> {
    let indent = indent_of(original);
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(&indent);
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser).map_err(|e| errors::coded(errors::CFG_WRITE, &[&e.to_string()]))?;
    let mut text = String::from_utf8(buf).map_err(|e| errors::coded(errors::CFG_WRITE, &[&e.to_string()]))?;
    if original.ends_with('\n') {
        text.push('\n');
    }
    let eol = eol_of(original);
    if eol != "\n" {
        text = text.replace('\n', eol);
    }
    Ok(text.into_bytes())
}

// ── What install.rs calls ─────────────────────────────────────────────────────

pub fn found(bytes: &[u8], path: &Path, container: &str, shape: Shape) -> Result<Found, String> {
    let (config, _) = parse(bytes, path, || snippet(container, shape, "roadeep-mcp.exe"))?;
    Ok(match servers(&config, container, path)?.and_then(|m| m.get(ENTRY)) {
        Some(entry) => classify(entry_command(entry)),
        None => Found::Missing,
    })
}

pub fn edit(bytes: &[u8], path: &Path, container: &str, shape: Shape, install: bool, command: &str) -> Result<Edit, String> {
    let (current, text) = parse(bytes, path, || snippet(container, shape, command))?;
    let next = if install {
        merged(&current, container, entry_for(shape, command), path)?
    } else {
        without_ours(&current, container, path)?
    };
    Ok(Edit { diff: render_diff(&current, &next, container, path), bytes: serialize(&next, &text)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = r"C:\Users\x\AppData\Local\com.roadeep.desktop\bin\roadeep-mcp.exe";
    const P: &str = r"C:\cfg.json";

    fn p() -> &'static Path {
        Path::new(P)
    }

    fn sample() -> Value {
        json!({
            "numStartups": 12,
            "projects": { "D:/a": { "allowedTools": [] }, "d:/a": { "history": [] } },
            "mcpServers": {
                "github": { "type": "http", "url": "https://x", "headers": { "Authorization": "Bearer SECRET" } },
                "local": { "type": "stdio", "command": "node", "args": ["s.js"], "env": { "API_KEY": "SECRET2" } }
            },
            "oauthAccount": { "emailAddress": "a@b.c" }
        })
    }

    fn cc(existing: &Value) -> Value {
        merged(existing, "mcpServers", entry_for(Shape::ClaudeCode, CMD), p()).unwrap()
    }

    #[test]
    fn entry_shapes_match_each_clients_docs() {
        assert_eq!(entry_for(Shape::ClaudeCode, "x"), json!({ "type": "stdio", "command": "x", "args": [], "env": {} }));
        assert_eq!(entry_for(Shape::Plain, "x"), json!({ "command": "x", "args": [] }));
        assert_eq!(entry_for(Shape::VsCode, "x"), json!({ "type": "stdio", "command": "x", "args": [] }));
        assert_eq!(entry_for(Shape::Cline, "x"), json!({ "command": "x", "args": [], "disabled": false }));
        assert_eq!(entry_for(Shape::OpenCode, "x"), json!({ "type": "local", "command": ["x"], "enabled": true }));
        for shape in [Shape::ClaudeCode, Shape::Plain, Shape::VsCode, Shape::Cline, Shape::OpenCode] {
            assert!(entry_is_ours(&entry_for(shape, CMD)), "{shape:?}");
        }
    }

    #[test]
    fn install_adds_only_roadeep_and_keeps_everything_else() {
        let before = sample();
        let after = cc(&before);
        assert_eq!(after["mcpServers"]["roadeep"], entry_for(Shape::ClaudeCode, CMD));
        for key in ["numStartups", "projects", "oauthAccount"] {
            assert_eq!(after[key], before[key], "{key}");
        }
        assert_eq!(after["mcpServers"]["github"], before["mcpServers"]["github"]);
        assert_eq!(after["mcpServers"]["local"], before["mcpServers"]["local"]);
        // Order preserved: our entry goes last.
        let keys: Vec<&String> = after["mcpServers"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["github", "local", "roadeep"]);
        let top: Vec<&String> = after.as_object().unwrap().keys().collect();
        assert_eq!(top, ["numStartups", "projects", "mcpServers", "oauthAccount"]);
    }

    #[test]
    fn install_is_idempotent_and_uninstall_restores_the_original() {
        let before = sample();
        let once = cc(&before);
        assert_eq!(cc(&once), once);
        assert_eq!(without_ours(&once, "mcpServers", p()).unwrap(), before);
        assert_eq!(without_ours(&before, "mcpServers", p()).unwrap(), before, "nothing to remove is a no-op");

        // No container at all: install creates it, uninstall removes it again.
        let bare = json!({ "numStartups": 1 });
        let installed = cc(&bare);
        assert_eq!(installed["mcpServers"], json!({ "roadeep": entry_for(Shape::ClaudeCode, CMD) }));
        assert_eq!(without_ours(&installed, "mcpServers", p()).unwrap(), bare);
    }

    #[test]
    fn a_foreign_roadeep_entry_is_never_removed() {
        let mut foreign = sample();
        foreign["mcpServers"]["roadeep"] = json!({ "type": "http", "url": "https://roadeep.com/mcp" });
        let err = without_ours(&foreign, "mcpServers", p()).unwrap_err();
        assert!(err.starts_with(errors::MCP_FOREIGN_ENTRY), "{err}");
        let bytes = foreign.to_string().into_bytes();
        assert_eq!(found(&bytes, p(), "mcpServers", Shape::ClaudeCode).unwrap(), Found::Foreign);
        // Installing replaces it, and the diff shows exactly that.
        let after = cc(&foreign);
        let diff = render_diff(&foreign, &after, "mcpServers", p());
        assert!(diff.contains("-     \"roadeep\"") && diff.contains("+     \"roadeep\""), "{diff}");
    }

    #[test]
    fn a_non_object_container_is_refused() {
        let odd = json!({ "mcpServers": [1, 2] });
        let err = merged(&odd, "mcpServers", json!({}), p()).unwrap_err();
        assert!(err.starts_with(errors::MCP_SERVERS_NOT_OBJECT) && err.contains("mcpServers"), "{err}");
        assert!(without_ours(&odd, "mcpServers", p()).is_err());
    }

    #[test]
    fn the_diff_never_shows_other_servers_secrets() {
        let before = sample();
        let after = cc(&before);
        let diff = render_diff(&before, &after, "mcpServers", p());
        assert!(!diff.contains("SECRET"), "{diff}");
        assert!(diff.contains("2 other server(s) unchanged: github, local"), "{diff}");
        assert!(diff.contains("+       \"type\": \"stdio\""), "{diff}");
        assert!(diff.contains("roadeep-mcp.exe"));
        assert_eq!(render_diff(&after, &after, "mcpServers", p()), errors::CFG_NO_CHANGE);

        // A replaced foreign entry's own secrets are masked too.
        let mut foreign = sample();
        foreign["mcpServers"]["roadeep"] =
            json!({ "command": "x", "env": { "TOKEN": "SECRET3" }, "apiKey": "SECRET4", "headers": { "X": "SECRET5" } });
        let diff = render_diff(&foreign, &cc(&foreign), "mcpServers", p());
        assert!(!diff.contains("SECRET") && diff.contains("\"TOKEN\"") && diff.contains("\"apiKey\""), "{diff}");
    }

    #[test]
    fn vscode_keeps_inputs_tabs_and_crlf() {
        let original = "{\r\n\t\"servers\": {\r\n\t\t\"a\": {\r\n\t\t\t\"type\": \"stdio\",\r\n\t\t\t\"command\": \"node\",\r\n\t\t\t\"args\": []\r\n\t\t}\r\n\t},\r\n\t\"inputs\": [\r\n\t\t{\r\n\t\t\t\"id\": \"k\"\r\n\t\t}\r\n\t]\r\n}\r\n";
        let e = edit(original.as_bytes(), p(), "servers", Shape::VsCode, true, CMD).unwrap();
        let text = String::from_utf8(e.bytes).unwrap();
        assert!(text.starts_with("{\r\n\t\"servers\": {\r\n\t\t\"a\": {"), "{text}");
        assert!(text.ends_with("}\r\n") && !text.replace("\r\n", "").contains('\n'), "{text:?}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["inputs"], json!([{ "id": "k" }]));
        assert_eq!(v["servers"]["roadeep"], entry_for(Shape::VsCode, CMD));
        assert!(e.diff.contains("\"servers\": {") && e.diff.contains("1 other server(s) unchanged: a"), "{}", e.diff);

        // And removing it again gives back the very same bytes.
        let back = edit(text.as_bytes(), p(), "servers", Shape::VsCode, false, CMD).unwrap();
        assert_eq!(String::from_utf8(back.bytes).unwrap(), original);
    }

    #[test]
    fn opencode_uses_the_mcp_key_and_a_command_array() {
        let original = "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"theme\": \"x\"\n}\n";
        let e = edit(original.as_bytes(), p(), "mcp", Shape::OpenCode, true, CMD).unwrap();
        let v: Value = serde_json::from_slice(&e.bytes).unwrap();
        assert_eq!(v["mcp"]["roadeep"], json!({ "type": "local", "command": [CMD], "enabled": true }));
        assert_eq!(found(&e.bytes, p(), "mcp", Shape::OpenCode).unwrap(), Found::Ours);
        let back = edit(&e.bytes, p(), "mcp", Shape::OpenCode, false, CMD).unwrap();
        assert_eq!(String::from_utf8(back.bytes).unwrap(), original);
    }

    #[test]
    fn jsonc_is_refused_with_the_snippet_to_paste() {
        let jsonc = "{\n  // my servers\n  \"mcpServers\": {\n    \"a\": { \"command\": \"x\", },\n  },\n}\n";
        let err = edit(jsonc.as_bytes(), p(), "mcpServers", Shape::Plain, true, CMD).err().unwrap();
        let (first, rest) = err.split_once('\n').unwrap();
        assert!(first.starts_with(errors::MCP_JSONC), "{err}");
        let pasted: Value = serde_json::from_str(rest).unwrap();
        assert_eq!(pasted["mcpServers"]["roadeep"], entry_for(Shape::Plain, CMD));
        // A comment-looking sequence inside a string is not a comment.
        assert_eq!(strip_jsonc(r#"{"u": "http://x/*y*/", "a": [1,]}"#), r#"{"u": "http://x/*y*/", "a": [1]}"#);

        // Plain broken JSON is reported as such.
        let err = edit(b"{ broken", p(), "mcpServers", Shape::Plain, true, CMD).err().unwrap();
        assert!(err.starts_with(errors::CFG_INVALID_JSON), "{err}");
        let err = edit(b"[1]", p(), "mcpServers", Shape::Plain, true, CMD).err().unwrap();
        assert!(err.starts_with(errors::CFG_NOT_OBJECT), "{err}");
    }

    #[test]
    fn a_bom_and_a_blank_file_are_handled() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"a":1}"#);
        let e = edit(&bytes, p(), "mcpServers", Shape::Plain, true, CMD).unwrap();
        let v: Value = serde_json::from_slice(&e.bytes).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(found(b"  ", p(), "mcpServers", Shape::Plain).unwrap(), Found::Missing);
        let e = edit(b"", p(), "mcpServers", Shape::Cline, true, CMD).unwrap();
        let v: Value = serde_json::from_slice(&e.bytes).unwrap();
        assert_eq!(v, json!({ "mcpServers": { "roadeep": entry_for(Shape::Cline, CMD) } }));
    }
}
