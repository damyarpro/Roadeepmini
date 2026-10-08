// Hermes Agent: a top-level `mcp_servers:` map in ~/.hermes/config.yaml.
//
// There is no YAML crate that rewrites a file without dropping its comments,
// so this is a careful line-based edit of a block-style map:
//
//     mcp_servers:
//       other:            ← a child key per server, all at one indent
//         command: npx
//       roadeep:          ← ours: these lines only are added, replaced or removed
//         command: 'C:\…\roadeep-mcp.exe'
//         args: []
//
// Every other line is kept byte for byte. Anything this can't read with
// certainty — flow style with content, anchors, aliases, tags, merge keys,
// tabs, several documents — is refused with the snippet to paste by hand.

use std::path::Path;

use super::clients::{classify, decode, encode, eol_of, others_line, Edit, Found, ENTRY, SHOWN_FIELDS};
use crate::errors;

const CONTAINER: &str = "mcp_servers";
/// Indent used when the file gives none to copy.
const DEFAULT_STEP: usize = 2;

fn quoted(command: &str) -> String {
    // Single quotes: backslashes in a Windows path stay literal.
    format!("'{}'", command.replace('\'', "''"))
}

fn our_lines(command: &str, child: usize, step: usize) -> Vec<String> {
    let (c, f) = (" ".repeat(child), " ".repeat(child + step));
    vec![format!("{c}{ENTRY}:"), format!("{f}command: {}", quoted(command)), format!("{f}args: []")]
}

/// What to paste by hand when the file can't be edited safely.
pub fn snippet(command: &str) -> String {
    let mut lines = vec![format!("{CONTAINER}:")];
    lines.extend(our_lines(command, DEFAULT_STEP, DEFAULT_STEP));
    lines.join("\n")
}

// ── Reading lines ─────────────────────────────────────────────────────────────

fn content(raw: &str) -> &str {
    raw.trim_end_matches(['\r', '\n'])
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_skip(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// `key: rest` → (key, rest after the colon). Plain, 'single' or "double"
/// quoted keys; None for anything that isn't a mapping key.
fn split_key(s: &str) -> Option<(String, &str)> {
    let (key, after) = if let Some(body) = s.strip_prefix('"') {
        let mut end = None;
        let mut escaped = false;
        for (i, ch) in body.char_indices() {
            match ch {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => {
                    end = Some(i);
                    break;
                }
                _ => escaped = false,
            }
        }
        let end = end?;
        (body[..end].replace("\\\"", "\"").replace("\\\\", "\\"), &body[end + 1..])
    } else if let Some(body) = s.strip_prefix('\'') {
        let bytes = body.as_bytes();
        let mut i = 0;
        loop {
            if i >= bytes.len() {
                return None;
            }
            if bytes[i] == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                break;
            }
            i += 1;
        }
        (body[..i].replace("''", "'"), &body[i + 1..])
    } else {
        let bytes = s.as_bytes();
        let colon = (0..bytes.len()).find(|&i| bytes[i] == b':' && bytes.get(i + 1).is_none_or(|b| *b == b' ' || *b == b'\t'))?;
        let key = s[..colon].trim_end();
        if key.is_empty() || key.starts_with(['-', '[', '{', '?', '&', '*', '!', '|', '>', '%', '@', '`', '#']) {
            return None;
        }
        (key.to_string(), &s[colon..])
    };
    let rest = after.trim_start_matches([' ', '\t']).strip_prefix(':')?;
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    Some((key, rest))
}

/// The value after `key:`, without a trailing comment.
fn value_part(rest: &str) -> &str {
    let t = rest.trim();
    if t.starts_with(['\'', '"']) {
        return t;
    }
    match t.find(" #") {
        Some(i) => t[..i].trim_end(),
        None if t.starts_with('#') => "",
        None => t,
    }
}

fn unquote(v: &str) -> String {
    if let Some(inner) = v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return inner.replace("\\\\", "\\").replace("\\\"", "\"");
    }
    v.to_string()
}

/// Anchors (&a), aliases (*a), tags (!t) and merge keys (<<) can tie lines
/// together in ways a line edit can't see.
fn is_linked(line: &str) -> bool {
    let mut t = line.trim();
    while let Some(rest) = t.strip_prefix('-') {
        t = rest.trim_start();
    }
    if t.starts_with(['&', '*', '!']) {
        return true;
    }
    match split_key(t) {
        Some((k, rest)) => k == "<<" || value_part(rest).starts_with(['&', '*', '!']),
        None => false,
    }
}

// ── The shape of the file ─────────────────────────────────────────────────────

struct Server {
    name: String,
    start: usize,
    /// Exclusive; trailing blank and comment lines are left outside.
    end: usize,
}

struct Layout {
    /// The `mcp_servers:` line, when there is one.
    key: Option<usize>,
    /// `{}`, `null` or `~` on the key line: an empty map written inline.
    inline_empty: bool,
    child: usize,
    step: usize,
    servers: Vec<Server>,
}

impl Layout {
    fn entry(&self) -> Option<&Server> {
        self.servers.iter().find(|s| s.name == ENTRY)
    }
}

fn analyze(lines: &[String], path: &Path, command: &str) -> Result<Layout, String> {
    let manual = |detail: &str| {
        format!("{}\n{}", errors::coded(errors::MCP_YAML_MANUAL, &[&path.display().to_string(), detail]), snippet(command))
    };

    let mut key = None;
    let mut seen_key = false;
    for (i, raw) in lines.iter().enumerate() {
        let c = content(raw);
        if is_skip(c) || c.starts_with(' ') {
            continue;
        }
        if c.starts_with('\t') {
            return Err(manual("tab indentation"));
        }
        if c.starts_with("---") && !seen_key {
            continue;
        }
        if c.starts_with('%') && !seen_key {
            continue;
        }
        if c.starts_with("---") || c.starts_with("...") {
            return Err(manual("more than one YAML document"));
        }
        let Some((k, _)) = split_key(c) else {
            return Err(manual("the top level is not a key: value map"));
        };
        seen_key = true;
        if k == CONTAINER {
            if key.is_some() {
                return Err(manual("mcp_servers appears twice"));
            }
            key = Some(i);
        }
    }
    let Some(key) = key else {
        return Ok(Layout { key: None, inline_empty: false, child: DEFAULT_STEP, step: DEFAULT_STEP, servers: Vec::new() });
    };

    let (_, rest) = split_key(content(&lines[key])).unwrap_or_default();
    let value = value_part(rest);
    if value.starts_with(['&', '*', '!']) {
        return Err(manual("anchors, aliases or tags on mcp_servers"));
    }
    let inline_empty = matches!(value, "{}" | "null" | "Null" | "NULL" | "~");
    if !value.is_empty() && !inline_empty {
        return Err(manual("mcp_servers is not written in block style"));
    }

    let block_end = (key + 1..lines.len())
        .find(|&i| {
            let c = content(&lines[i]);
            !is_skip(c) && !c.starts_with([' ', '\t'])
        })
        .unwrap_or(lines.len());
    let body: Vec<usize> = (key + 1..block_end).filter(|&i| !is_skip(content(&lines[i]))).collect();
    if inline_empty && !body.is_empty() {
        return Err(manual("mcp_servers has both an inline value and a block"));
    }

    let child = body.first().map(|&i| indent(content(&lines[i]))).unwrap_or(DEFAULT_STEP);
    let mut starts: Vec<(String, usize)> = Vec::new();
    let mut step = None;
    for &i in &body {
        let c = content(&lines[i]);
        let ws = &c[..c.len() - c.trim_start().len()];
        if ws.contains('\t') {
            return Err(manual("tab indentation"));
        }
        if is_linked(c) {
            return Err(manual("anchors, aliases, tags or merge keys"));
        }
        let ind = indent(c);
        if ind < child {
            return Err(manual("uneven indentation under mcp_servers"));
        }
        if ind == child {
            let t = c.trim_start();
            if t.starts_with('-') {
                return Err(errors::coded(errors::MCP_SERVERS_NOT_OBJECT, &[CONTAINER, &path.display().to_string()]));
            }
            let Some((name, _)) = split_key(t) else {
                return Err(manual("a server name that can't be read"));
            };
            if name == ENTRY && starts.iter().any(|(n, _)| n == ENTRY) {
                return Err(manual("roadeep appears twice"));
            }
            starts.push((name, i));
        } else if step.is_none() {
            step = Some(ind - child);
        }
    }

    let mut servers = Vec::new();
    for (n, (name, start)) in starts.iter().enumerate() {
        let next = starts.get(n + 1).map(|(_, s)| *s).unwrap_or(block_end);
        let last = (*start..next).rev().find(|&i| !is_skip(content(&lines[i]))).unwrap_or(*start);
        servers.push(Server { name: name.clone(), start: *start, end: last + 1 });
    }
    Ok(Layout { key: Some(key), inline_empty, child, step: step.unwrap_or(DEFAULT_STEP), servers })
}

fn found_in(lines: &[String], layout: &Layout) -> Found {
    let Some(entry) = layout.entry() else { return Found::Missing };
    let (_, rest) = split_key(content(&lines[entry.start]).trim_start()).unwrap_or_default();
    let inline = value_part(rest);
    if !inline.is_empty() {
        // `roadeep: {command: …}` — the command is somewhere in that one line.
        return classify(Some(inline));
    }
    let field = (entry.start + 1..entry.end)
        .map(|i| content(&lines[i]))
        .filter(|c| !is_skip(c))
        .map(indent)
        .min();
    let command = (entry.start + 1..entry.end).find_map(|i| {
        let c = content(&lines[i]);
        if Some(indent(c)) != field {
            return None;
        }
        match split_key(c.trim_start()) {
            Some((k, rest)) if k == "command" => Some(unquote(value_part(rest))),
            _ => None,
        }
    });
    classify(command.as_deref())
}

// ── The diff ──────────────────────────────────────────────────────────────────

/// An existing entry's lines with every value outside SHOWN_FIELDS masked.
fn redacted(lines: &[&str], sign: char) -> Vec<String> {
    let mut out = Vec::new();
    let Some(head) = lines.first() else { return out };
    if !value_part(split_key(head.trim_start()).map(|(_, r)| r).unwrap_or_default()).is_empty() {
        // An inline `roadeep: {…}` can hold anything: shown as a placeholder.
        out.push(format!("{sign} {}{ENTRY}: {{…}}", " ".repeat(indent(head))));
        return out;
    }
    out.push(format!("{sign} {head}"));
    let body: Vec<&str> = lines[1..].iter().copied().filter(|l| !is_skip(l)).collect();
    let field = body.iter().map(|l| indent(l)).min().unwrap_or(0);
    let mut shown = false;
    for line in body {
        let (ind, t) = (indent(line), line.trim_start());
        let pad = " ".repeat(ind);
        let text = if ind == field {
            match split_key(t) {
                Some((k, rest)) => {
                    shown = SHOWN_FIELDS.contains(&k.as_str());
                    if shown {
                        line.to_string()
                    } else if value_part(rest).is_empty() {
                        format!("{pad}{k}:")
                    } else {
                        format!("{pad}{k}: …")
                    }
                }
                None => {
                    shown = false;
                    format!("{pad}…")
                }
            }
        } else if shown {
            line.to_string()
        } else {
            match split_key(t.trim_start_matches(['-', ' '])) {
                Some((k, rest)) if value_part(rest).is_empty() => format!("{pad}{k}:"),
                Some((k, _)) => format!("{pad}{k}: …"),
                None if t.starts_with('-') => format!("{pad}- …"),
                None => format!("{pad}…"),
            }
        };
        out.push(format!("{sign} {text}"));
    }
    out
}

fn render_diff(container: char, others: &[String], old: &[&str], new: &[String]) -> String {
    let mut out = vec!["  …".to_string(), format!("{container} {CONTAINER}:")];
    out.extend(others_line("    ", others));
    out.extend(redacted(old, '-'));
    out.extend(new.iter().map(|l| format!("+ {l}")));
    out.push("  …".into());
    out.join("\n")
}

// ── What install.rs calls ─────────────────────────────────────────────────────

fn split_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(str::to_string).collect()
}

pub fn found(bytes: &[u8], path: &Path) -> Result<Found, String> {
    let (_, text) = decode(bytes, path)?;
    let lines = split_lines(text);
    let layout = analyze(&lines, path, "roadeep-mcp.exe")?;
    Ok(found_in(&lines, &layout))
}

pub fn edit(bytes: &[u8], path: &Path, install: bool, command: &str) -> Result<Edit, String> {
    let (bom, text) = decode(bytes, path)?;
    let eol = eol_of(text);
    let mut lines = split_lines(text);
    let layout = analyze(&lines, path, command)?;
    let found = found_in(&lines, &layout);
    let unchanged = || Edit { diff: errors::CFG_NO_CHANGE.into(), bytes: bytes.to_vec() };
    let others: Vec<String> = layout.servers.iter().map(|s| s.name.clone()).filter(|n| n != ENTRY).collect();
    let old: Vec<String> = layout
        .entry()
        .map(|e| lines[e.start..e.end].iter().map(|l| content(l).to_string()).collect())
        .unwrap_or_default();
    let old_refs: Vec<&str> = old.iter().map(String::as_str).collect();
    let with_eol = |l: &String| format!("{l}{eol}");
    // A last line without a newline gets one before anything goes after it.
    let terminate = |lines: &mut Vec<String>, at: usize| {
        if at == lines.len() && !lines.is_empty() && !lines[at - 1].ends_with('\n') {
            lines[at - 1].push_str(eol);
        }
    };

    let diff = if install {
        let new = our_lines(command, layout.child, layout.step);
        if found == Found::Ours && old == new {
            return Ok(unchanged());
        }
        match (layout.key, layout.entry()) {
            (None, _) => {
                let at = lines.len();
                terminate(&mut lines, at);
                lines.push(format!("{CONTAINER}:{eol}"));
                lines.extend(new.iter().map(with_eol));
                render_diff('+', &others, &[], &new)
            }
            (Some(_), Some(entry)) => {
                let (start, end) = (entry.start, entry.end);
                terminate(&mut lines, end);
                lines.splice(start..end, new.iter().map(with_eol));
                render_diff(' ', &others, &old_refs, &new)
            }
            (Some(key), None) => {
                let at = layout.servers.last().map(|s| s.end).unwrap_or(key + 1);
                if layout.inline_empty {
                    // `mcp_servers: {}  # note` → `mcp_servers:  # note`
                    let c = content(&lines[key]);
                    let (_, rest) = split_key(c).unwrap_or_default();
                    let head = &c[..c.len() - rest.len()];
                    let value = value_part(rest);
                    let note = rest.find(value).map(|i| &rest[i + value.len()..]).unwrap_or("");
                    let ending = &lines[key][c.len()..];
                    lines[key] = format!("{head}{note}{ending}");
                }
                terminate(&mut lines, at);
                lines.splice(at..at, new.iter().map(with_eol));
                render_diff(' ', &others, &[], &new)
            }
        }
    } else {
        match found {
            Found::Missing => return Ok(unchanged()),
            Found::Foreign => {
                return Err(errors::coded(errors::MCP_FOREIGN_ENTRY, &[ENTRY, &path.display().to_string()]))
            }
            Found::Ours | Found::Legacy => {}
        }
        let (Some(key), Some(entry)) = (layout.key, layout.entry()) else { return Ok(unchanged()) };
        lines.drain(entry.start..entry.end);
        let emptied = others.is_empty();
        if emptied {
            lines.remove(key);
        }
        render_diff(if emptied { '-' } else { ' ' }, &others, &old_refs, &[])
    };
    Ok(Edit { diff, bytes: encode(bom, lines.concat()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = r"C:\Users\x\AppData\Local\com.roadeep.desktop\bin\roadeep-mcp.exe";

    fn p() -> &'static Path {
        Path::new(r"C:\Users\x\.hermes\config.yaml")
    }

    const SAMPLE: &str = "# Hermes config\nmodel:\n  default: hermes-4   # pinned\n\nmcp_servers:\n  github:\n    command: npx\n    args: [\"-y\", \"@modelcontextprotocol/server-github\"]\n    env:\n      GITHUB_TOKEN: \"SECRET\"   # never share\n  # a remote one\n  docs:\n    url: https://example.com/mcp\n\n# terminal settings\nterminal:\n  backend: local\n";

    fn ours(child: usize, step: usize) -> String {
        our_lines(CMD, child, step).iter().map(|l| format!("{l}\n")).collect()
    }

    fn run(text: &str, install: bool) -> Result<Edit, String> {
        edit(text.as_bytes(), p(), install, CMD)
    }

    fn s(e: &Edit) -> String {
        String::from_utf8(e.bytes.clone()).unwrap()
    }

    #[test]
    fn adds_after_the_other_servers_keeping_every_byte_of_them() {
        let e = run(SAMPLE, true).unwrap();
        let out = s(&e);
        let expected = SAMPLE.replace(
            "    url: https://example.com/mcp\n",
            &format!("    url: https://example.com/mcp\n{}", ours(2, 2)),
        );
        assert_eq!(out, expected);
        assert_eq!(found(out.as_bytes(), p()).unwrap(), Found::Ours);
        assert!(e.diff.contains("2 other server(s) unchanged: github, docs"), "{}", e.diff);
        assert!(e.diff.contains("+   roadeep:") && e.diff.contains("+     args: []"), "{}", e.diff);
        assert!(!e.diff.contains("SECRET"), "{}", e.diff);

        assert_eq!(run(&out, true).unwrap().diff, errors::CFG_NO_CHANGE);
        assert_eq!(s(&run(&out, false).unwrap()), SAMPLE, "removing gives back the original");
    }

    #[test]
    fn a_missing_container_is_appended_and_dropped_again() {
        let plain = "model:\n  default: x\n";
        let out = s(&run(plain, true).unwrap());
        assert_eq!(out, format!("{plain}mcp_servers:\n{}", ours(2, 2)));
        assert_eq!(s(&run(&out, false).unwrap()), plain);

        let out = s(&run("", true).unwrap());
        assert_eq!(out, format!("mcp_servers:\n{}", ours(2, 2)));
        assert_eq!(s(&run(&out, false).unwrap()), "");

        // No final newline: one is added before our lines.
        let out = s(&run("model: x", true).unwrap());
        assert!(out.starts_with("model: x\nmcp_servers:\n"), "{out}");
    }

    #[test]
    fn indentation_and_line_endings_follow_the_file() {
        let four = "mcp_servers:\r\n    a:\r\n        command: x\r\nother: 1\r\n";
        let out = s(&run(four, true).unwrap());
        assert_eq!(
            out,
            format!("mcp_servers:\r\n    a:\r\n        command: x\r\n{}other: 1\r\n", ours(4, 4).replace('\n', "\r\n"))
        );
        assert_eq!(s(&run(&out, false).unwrap()), four);
    }

    #[test]
    fn an_inline_empty_map_becomes_a_block() {
        let text = "mcp_servers: {}  # none yet\nother: 1\n";
        let out = s(&run(text, true).unwrap());
        assert_eq!(out, format!("mcp_servers:  # none yet\n{}other: 1\n", ours(2, 2)));
    }

    #[test]
    fn a_stale_entry_is_refreshed_in_place_and_a_foreign_one_is_never_removed() {
        let stale = SAMPLE.replace("  # a remote one\n", "  roadeep:\n    command: 'C:\\old\\roadeep-mcp.exe'\n  # a remote one\n");
        let e = run(&stale, true).unwrap();
        assert_eq!(s(&e), SAMPLE.replace("  # a remote one\n", &format!("{}  # a remote one\n", ours(2, 2))));
        assert!(e.diff.contains("-     command: 'C:\\old\\roadeep-mcp.exe'"), "{}", e.diff);

        let foreign = SAMPLE.replace(
            "  # a remote one\n",
            "  roadeep:\n    url: https://roadeep.com/mcp\n    headers:\n      Authorization: Bearer SECRET7\n    api_key: SECRET6\n  # a remote one\n",
        );
        assert_eq!(found(foreign.as_bytes(), p()).unwrap(), Found::Foreign);
        let err = run(&foreign, false).err().unwrap();
        assert!(err.starts_with(errors::MCP_FOREIGN_ENTRY), "{err}");
        let e = run(&foreign, true).unwrap();
        assert!(!e.diff.contains("SECRET"), "{}", e.diff);
        assert!(e.diff.contains("-       Authorization: …") && e.diff.contains("-     api_key: …"), "{}", e.diff);
        assert_eq!(found(&e.bytes, p()).unwrap(), Found::Ours);
    }

    #[test]
    fn what_cant_be_edited_safely_is_refused_with_the_snippet() {
        for text in [
            "mcp_servers: {a: {command: x}}\n",
            "mcp_servers: &servers\n  a:\n    command: x\n",
            "base: &b\n  command: x\nmcp_servers:\n  a: *b\n",
            "mcp_servers:\n  a:\n    <<: *b\n",
            "mcp_servers:\n\ta:\n",
            "a: 1\n---\nb: 2\n",
            "- just\n- a list\n",
            "mcp_servers:\n  a: 1\nmcp_servers:\n  b: 2\n",
            "mcp_servers:\n    a:\n      command: x\n  b:\n    command: y\n",
        ] {
            let err = run(text, true).err().unwrap_or_else(|| panic!("accepted: {text:?}"));
            let (first, rest) = err.split_once('\n').unwrap_or((&err, ""));
            assert!(first.starts_with(errors::MCP_YAML_MANUAL), "{text:?} → {err}");
            assert!(rest.contains("roadeep:") && rest.contains("roadeep-mcp.exe"), "{err}");
        }
        let err = run("mcp_servers:\n  - a\n", true).err().unwrap();
        assert!(err.starts_with(errors::MCP_SERVERS_NOT_OBJECT), "{err}");
    }

    #[test]
    fn a_bom_survives_and_quoted_keys_and_paths_are_read() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"\"mcp_servers\":\n  'roadeep':\n    command: \"C:\\\\x\\\\roadeep-mcp.exe\"\n");
        assert_eq!(found(&bytes, p()).unwrap(), Found::Ours);
        let e = edit(&bytes, p(), false, CMD).unwrap();
        assert_eq!(e.bytes, vec![0xEF, 0xBB, 0xBF]);
        assert_eq!(quoted("C:\\it's"), "'C:\\it''s'");
        assert_eq!(snippet(CMD), format!("mcp_servers:\n  roadeep:\n    command: '{CMD}'\n    args: []"));
    }
}
