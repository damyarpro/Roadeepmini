// Tools as the servers describe them → what the chat loop and the settings
// window get: cleaned text, compacted schemas, qualified names, and tool
// results turned into plain text. Everything a server sends is untrusted: names
// and descriptions are cut and stripped of control/bidi characters, results
// are capped, and nothing is ever HTML.

use serde::Serialize;
use serde_json::{Map, Value};

use super::store::{self, ServerConfig};
use super::{ToolMode, ToolOutcome, ToolSpec};
use crate::roadeep::chat::shown_char;

pub const MAX_DESCRIPTION_CHARS: usize = 300;
const MAX_TITLE_CHARS: usize = 80;
/// Of the offered set, per chat message.
pub const MAX_OFFERED: usize = 60;
pub const MAX_RESULT_CHARS: usize = 16_000;
const MAX_QUALIFIED: usize = 64;
const MAX_SLUG: usize = 20;
/// Schema descriptions are hints for the model, not documentation.
const MAX_SCHEMA_DESCRIPTION: usize = 120;
const MAX_SCHEMA_DEPTH: usize = 8;
const MAX_ENUM: usize = 50;
/// Past this (serialized), descriptions go; past it again, only the top-level
/// property names and types stay.
const MAX_SCHEMA_CHARS: usize = 4000;

/// One tool from `tools/list`, already cleaned.
#[derive(Debug, Clone, PartialEq)]
pub struct RawTool {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
    pub destructive: bool,
    /// openWorldHint isn't explicitly false: the tool may reach outside the
    /// server (the web, other services). The spec defaults it to true.
    pub open_world: bool,
}

/// What the settings window shows per tool.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub read_only: bool,
    pub destructive: bool,
    pub mode: ToolMode,
}

/// One line: whitespace collapsed, no control or bidi characters, `max` chars
/// at most (an ellipsis marks a cut).
pub fn one_line(text: &str, max: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let shown: Vec<char> = joined.chars().filter(|c| shown_char(*c)).collect();
    if shown.len() <= max {
        return shown.into_iter().collect();
    }
    let mut out: String = shown[..max.saturating_sub(1)].iter().collect();
    out.push('…');
    out
}

/// Multi-line text from a server (tool results): newlines and tabs kept,
/// other control and bidi characters dropped.
pub fn clean_block(text: &str) -> String {
    text.replace("\r\n", "\n").chars().filter(|c| matches!(c, '\n' | '\t') || shown_char(*c)).collect()
}

/// A `tools/list` entry, or None when it has no usable name.
pub fn parse_tool(value: &Value) -> Option<RawTool> {
    let name = value.get("name")?.as_str()?;
    if !store::valid_tool_name(name) || name.trim() != name {
        return None;
    }
    let annotations = value.get("annotations");
    let hint = |key: &str| annotations.and_then(|a| a.get(key)).and_then(Value::as_bool);
    let read_only = hint("readOnlyHint") == Some(true);
    let open_world = hint("openWorldHint") != Some(false);
    let title = value
        .get("title")
        .or_else(|| annotations.and_then(|a| a.get("title")))
        .and_then(Value::as_str)
        .map(|t| one_line(t, MAX_TITLE_CHARS))
        .filter(|t| !t.is_empty());
    let schema = value.get("inputSchema").filter(|s| s.is_object()).cloned().unwrap_or_else(|| serde_json::json!({ "type": "object" }));
    Some(RawTool {
        name: name.to_string(),
        title,
        description: one_line(value.get("description").and_then(Value::as_str).unwrap_or(""), MAX_DESCRIPTION_CHARS),
        input_schema: compact_schema(&schema),
        read_only,
        // The spec defaults destructiveHint to true for any tool that isn't
        // read-only, which would flag nearly every tool. Only an explicit hint
        // is shown; non-read-only tools default to "ask" anyway.
        destructive: !read_only && hint("destructiveHint") == Some(true),
        open_world,
    })
}

/// The user's choice, else the default: `auto` only for a tool that says it
/// is read-only AND stays inside its server (openWorldHint false). Hints come
/// from the server and are untrusted, so anything less explicit asks first: a
/// "read-only" fetch of a URL the model picked can still send data out.
pub fn mode_for(server: &ServerConfig, tool: &RawTool) -> ToolMode {
    match server.tools.get(&tool.name) {
        Some(mode) => *mode,
        None if tool.read_only && !tool.open_world => ToolMode::Auto,
        None => ToolMode::Ask,
    }
}

pub fn views(server: &ServerConfig, tools: &[RawTool]) -> Vec<ToolView> {
    let mut out: Vec<ToolView> = tools
        .iter()
        .map(|t| ToolView {
            name: t.name.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            read_only: t.read_only,
            destructive: t.destructive,
            mode: mode_for(server, t),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── Qualified names ───────────────────────────────────────────────────────────

/// A short `[a-z0-9-]` slug for the server: from its name, else its id.
pub fn slug(name: &str, id: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out = out.trim_matches('-').to_string();
    if out.is_empty() {
        out = id.to_string();
    }
    out.truncate(MAX_SLUG);
    out.trim_end_matches('-').to_string()
}

fn sanitize_tool(tool: &str) -> String {
    tool.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn fit(base: &str, suffix: &str) -> String {
    let room = MAX_QUALIFIED - suffix.len();
    let cut: String = base.chars().take(room).collect();
    format!("{cut}{suffix}")
}

/// Every offered tool, in a stable order (server order of the file, then tool
/// name), modes applied, `off` left out, capped at MAX_OFFERED.
pub fn offered(servers: &[(ServerConfig, Vec<RawTool>)]) -> Vec<ToolSpec> {
    let mut out: Vec<ToolSpec> = Vec::new();
    // The planner's built-in tools are `roadeep__…`: a server the user named
    // "Roadeep" must not shadow them.
    let mut slugs: Vec<String> = vec![crate::planner::tools::SLUG.to_string(), "computer".to_string(), crate::desktop_actions::SLUG.to_string()];
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (server, tools) in servers {
        let base = slug(&server.name, &server.id);
        let mut server_slug = base.clone();
        let mut n = 2;
        while slugs.contains(&server_slug) {
            server_slug = format!("{}-{n}", &base[..base.len().min(MAX_SLUG - 3)]);
            n += 1;
        }
        slugs.push(server_slug.clone());

        let mut sorted: Vec<&RawTool> = tools.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for tool in sorted {
            let mode = mode_for(server, tool);
            if mode == ToolMode::Off {
                continue;
            }
            if out.len() >= MAX_OFFERED {
                return out;
            }
            let base = format!("{server_slug}__{}", sanitize_tool(&tool.name));
            let mut qualified = fit(&base, "");
            let mut n = 2;
            while names.contains(&qualified) {
                qualified = fit(&base, &format!("_{n}"));
                n += 1;
            }
            names.insert(qualified.clone());
            out.push(ToolSpec {
                server_id: server.id.clone(),
                server_name: one_line(&server.name, store::MAX_NAME_CHARS),
                tool: tool.name.clone(),
                qualified,
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                mode,
                read_only: tool.read_only,
                destructive: tool.destructive,
            });
        }
    }
    out
}

// ── Schema compaction ─────────────────────────────────────────────────────────

/// Keywords dropped from every schema level: none of them changes what a valid
/// call looks like, and they cost tokens in every message.
const DROPPED: &[&str] = &["$schema", "$id", "$comment", "examples", "example", "title", "markdownDescription", "deprecated", "readOnly", "writeOnly"];

pub fn compact_schema(schema: &Value) -> Value {
    let full = compact(schema, 0, true);
    if serde_json::to_string(&full).map(|s| s.len()).unwrap_or(0) <= MAX_SCHEMA_CHARS {
        return full;
    }
    let bare = compact(schema, 0, false);
    if serde_json::to_string(&bare).map(|s| s.len()).unwrap_or(0) <= MAX_SCHEMA_CHARS {
        return bare;
    }
    skeleton(schema)
}

/// Last resort for a huge schema: the top-level property names and types.
fn skeleton(schema: &Value) -> Value {
    let mut props = Map::new();
    if let Some(p) = schema.get("properties").and_then(Value::as_object) {
        for (name, sub) in p.iter().take(60) {
            let mut kept = Map::new();
            if let Some(t) = sub.get("type").filter(|t| t.is_string() || t.is_array()) {
                kept.insert("type".into(), t.clone());
            }
            props.insert(name.clone(), Value::Object(kept));
        }
    }
    let mut out = Map::new();
    out.insert("type".into(), Value::String("object".into()));
    out.insert("properties".into(), Value::Object(props));
    if let Some(r) = schema.get("required").filter(|r| r.is_array()) {
        out.insert("required".into(), r.clone());
    }
    Value::Object(out)
}

fn compact(schema: &Value, depth: usize, descriptions: bool) -> Value {
    let Some(obj) = schema.as_object() else {
        // `true` / `false` schemas stay; anything else isn't a schema.
        return if schema.is_boolean() { schema.clone() } else { Value::Object(Map::new()) };
    };
    if depth >= MAX_SCHEMA_DEPTH {
        return Value::Object(Map::new());
    }
    let mut out = Map::new();
    for (key, value) in obj {
        if DROPPED.contains(&key.as_str()) {
            continue;
        }
        let kept = match key.as_str() {
            "description" => {
                if !descriptions {
                    continue;
                }
                match value.as_str().map(|d| one_line(d, MAX_SCHEMA_DESCRIPTION)) {
                    Some(d) if !d.is_empty() => Value::String(d),
                    _ => continue,
                }
            }
            // name → schema maps: the keys are property names, never keywords.
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas" => match value.as_object() {
                Some(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), compact(v, depth + 1, descriptions))).collect()),
                None => continue,
            },
            "items" | "additionalProperties" | "not" | "contains" | "if" | "then" | "else" | "propertyNames" | "unevaluatedProperties" | "additionalItems" => {
                if value.is_array() {
                    Value::Array(value.as_array().unwrap().iter().map(|v| compact(v, depth + 1, descriptions)).collect())
                } else {
                    compact(value, depth + 1, descriptions)
                }
            }
            "anyOf" | "oneOf" | "allOf" | "prefixItems" => match value.as_array() {
                Some(items) => Value::Array(items.iter().map(|v| compact(v, depth + 1, descriptions)).collect()),
                None => continue,
            },
            "enum" => match value.as_array() {
                Some(items) => Value::Array(items.iter().take(MAX_ENUM).cloned().collect()),
                None => continue,
            },
            "default" | "const" => {
                // A small default helps; a big one is noise.
                if serde_json::to_string(value).map(|s| s.len() > 200).unwrap_or(true) {
                    continue;
                }
                value.clone()
            }
            _ => value.clone(),
        };
        out.insert(key.clone(), kept);
    }
    Value::Object(out)
}

// ── Results ───────────────────────────────────────────────────────────────────

/// A `tools/call` result as text for the model. Text, embedded text resources
/// and links become text; images, audio and binary resources are listed in
/// `omitted` by MIME type. `structuredContent` is used when there is no text.
pub fn outcome(result: &Value) -> ToolOutcome {
    let is_error = result.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let mut parts: Vec<String> = Vec::new();
    let mut omitted: Vec<String> = Vec::new();
    let mime = |v: &Value| one_line(v.get("mimeType").and_then(Value::as_str).unwrap_or("application/octet-stream"), 80);
    for item in result.get("content").and_then(Value::as_array).into_iter().flatten() {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
            Some("resource") => {
                let resource = item.get("resource").unwrap_or(&Value::Null);
                match resource.get("text").and_then(Value::as_str) {
                    Some(text) => parts.push(text.to_string()),
                    None => omitted.push(mime(resource)),
                }
            }
            Some("resource_link") => {
                let uri = item.get("uri").and_then(Value::as_str).unwrap_or("");
                let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                parts.push(one_line(&format!("[resource] {name} {uri}"), 600));
            }
            Some("image" | "audio") => omitted.push(mime(item)),
            _ => omitted.push("unknown".into()),
        }
    }
    if parts.is_empty() {
        if let Some(structured) = result.get("structuredContent").filter(|v| !v.is_null()) {
            parts.push(serde_json::to_string(structured).unwrap_or_default());
        }
    }
    omitted.truncate(20);
    ToolOutcome { is_error, text: cap(&clean_block(&parts.join("\n\n"))), omitted }
}

fn cap(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_RESULT_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_RESULT_CHARS).collect();
    format!("{kept}\n…[truncated: {} more characters]", total - MAX_RESULT_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcpc::store::{Auth, Transport};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn server(id: &str, name: &str, tools: &[(&str, ToolMode)]) -> ServerConfig {
        ServerConfig {
            id: id.into(),
            name: name.into(),
            source: "custom".into(),
            transport: Transport::Http { url: "https://x.com/mcp".into() },
            auth: Auth::None,
            enabled: true,
            approved_command: None,
            tools: tools.iter().map(|(n, m)| (n.to_string(), *m)).collect::<BTreeMap<_, _>>(),
            created_at: 0,
            updated_at: 0,
        }
    }

    /// `read_only` tools here also say openWorldHint: false.
    fn tool(name: &str, read_only: bool) -> RawTool {
        RawTool { name: name.into(), title: None, description: String::new(), input_schema: json!({}), read_only, destructive: false, open_world: !read_only }
    }

    #[test]
    fn tools_are_parsed_and_cleaned() {
        let t = parse_tool(&json!({
            "name": "search_code",
            "description": format!("Find\ncode \u{202E}fast {}", "x".repeat(400)),
            "inputSchema": { "type": "object", "properties": { "q": { "type": "string" } } },
            "annotations": { "readOnlyHint": true, "title": "Search" }
        }))
        .unwrap();
        assert!(t.read_only && !t.destructive);
        assert_eq!(t.title.as_deref(), Some("Search"));
        assert!(t.description.starts_with("Find code fast x"));
        assert_eq!(t.description.chars().count(), MAX_DESCRIPTION_CHARS);
        let d = parse_tool(&json!({ "name": "delete_repo", "annotations": { "destructiveHint": true } })).unwrap();
        assert!(d.destructive && !d.read_only);
        assert_eq!(d.input_schema, json!({ "type": "object" }));
        assert!(!parse_tool(&json!({ "name": "plain" })).unwrap().destructive, "only an explicit hint counts");
        for bad in [json!({}), json!({ "name": "" }), json!({ "name": "a\nb" }), json!({ "name": " a" }), json!({ "name": "x".repeat(129) })] {
            assert!(parse_tool(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn default_modes_follow_read_only_and_overrides_win() {
        let s = server("gh-1", "GitHub", &[("create_issue", ToolMode::Auto), ("delete_repo", ToolMode::Off)]);
        assert_eq!(mode_for(&s, &tool("search", true)), ToolMode::Auto);
        assert_eq!(mode_for(&s, &tool("push", false)), ToolMode::Ask);
        assert_eq!(mode_for(&s, &tool("create_issue", false)), ToolMode::Auto);
        assert_eq!(mode_for(&s, &tool("delete_repo", true)), ToolMode::Off);
        // Read-only alone isn't enough: it must also stay inside its server.
        let open = RawTool { open_world: true, ..tool("fetch", true) };
        assert_eq!(mode_for(&s, &open), ToolMode::Ask);
        let parsed = |annotations: serde_json::Value| mode_for(&s, &parse_tool(&json!({ "name": "x", "annotations": annotations })).unwrap());
        assert_eq!(parsed(json!({ "readOnlyHint": true, "openWorldHint": false })), ToolMode::Auto);
        assert_eq!(parsed(json!({ "readOnlyHint": true })), ToolMode::Ask, "openWorldHint defaults to true");
        assert_eq!(parsed(json!({ "readOnlyHint": true, "openWorldHint": true })), ToolMode::Ask);
        assert_eq!(parsed(json!({ "openWorldHint": false })), ToolMode::Ask);
        assert_eq!(parsed(json!({ "readOnlyHint": "true", "openWorldHint": false })), ToolMode::Ask, "only a real boolean counts");
    }

    #[test]
    fn qualified_names_are_valid_unique_and_stable() {
        let valid = |q: &str| !q.is_empty() && q.len() <= 64 && q.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let gh = server("gh-1", "GitHub (work)", &[("delete_repo", ToolMode::Off)]);
        let gh2 = server("gh-2", "GitHub work", &[]);
        let fa = server("fa-1", "فایل‌ها", &[]);
        let long = "a".repeat(100);
        let servers = vec![
            (gh.clone(), vec![tool("search.code", true), tool("search_code", true), tool("delete_repo", false), tool(&long, false)]),
            (gh2, vec![tool("search_code", true)]),
            (fa, vec![tool("read file", true)]),
        ];
        let specs = offered(&servers);
        let names: Vec<&str> = specs.iter().map(|s| s.qualified.as_str()).collect();
        assert_eq!(names[0], format!("github-work__{}", &long[..64 - 13]));
        assert_eq!(names[1], "github-work__search_code");
        assert_eq!(names[2], "github-work__search_code_2", "search.code sanitizes onto search_code");
        assert_eq!(names[3], "github-work-2__search_code", "same slug, second server");
        assert_eq!(names[4], "fa-1__read_file", "no ASCII in the name: the id");
        assert!(!names.iter().any(|n| n.contains("delete_repo")), "off tools are not offered");
        assert!(names.iter().all(|n| valid(n)), "{names:?}");
        assert_eq!(specs[2].tool, "search_code");
        assert_eq!(specs[1].tool, "search.code", "sorted by the server's own name");
        assert_eq!(names, offered(&servers).iter().map(|s| s.qualified.as_str()).collect::<Vec<_>>());

        let many: Vec<RawTool> = (0..100).map(|i| tool(&format!("t{i:03}"), true)).collect();
        assert_eq!(offered(&[(gh, many)]).len(), MAX_OFFERED);

        let mine = server("rd-1", "Roadeep", &[]);
        assert_eq!(offered(&[(mine, vec![tool("add_task", true)])])[0].qualified, "roadeep-2__add_task", "the built-in prefix is reserved");
        let desktop = server("custom-desktop", "Desktop", &[]);
        assert_eq!(offered(&[(desktop, vec![tool("open", true)])])[0].qualified, "desktop-2__open");
        let computer = server("computer", "Computer", &[]);
        let offered = offered(&[(computer, vec![tool("terminal", true)])]);
        assert_eq!(offered[0].qualified,"computer-2__terminal");
        assert_eq!(offered[0].server_id,"computer");
        assert_ne!(offered[0].server_id,crate::computer::tools::SERVER_ID,"configured server is never a native computer");
    }

    #[test]
    fn schemas_are_compacted_without_touching_property_names() {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Args",
            "description": "d".repeat(500),
            "properties": {
                "title": { "type": "string", "description": "Issue title", "examples": ["x"] },
                "examples": { "type": "array", "items": { "type": "string", "$comment": "c" } },
                "big": { "type": "string", "default": "z".repeat(300) },
                "mode": { "enum": (0..80).collect::<Vec<_>>() }
            },
            "required": ["title"],
            "additionalProperties": false
        });
        let c = compact_schema(&schema);
        assert!(c.get("$schema").is_none() && c.get("title").is_none());
        assert_eq!(c["description"].as_str().unwrap().chars().count(), MAX_SCHEMA_DESCRIPTION);
        assert_eq!(c["properties"]["title"], json!({ "type": "string", "description": "Issue title" }));
        assert_eq!(c["properties"]["examples"], json!({ "type": "array", "items": { "type": "string" } }));
        assert!(c["properties"]["big"].get("default").is_none());
        assert_eq!(c["properties"]["mode"]["enum"].as_array().unwrap().len(), MAX_ENUM);
        assert_eq!(c["required"], json!(["title"]));
        assert_eq!(c["additionalProperties"], json!(false));

        let mut props = Map::new();
        for i in 0..300 {
            props.insert(format!("p{i}"), json!({ "type": "string", "description": "a fairly long description that adds up" }));
        }
        let huge = compact_schema(&json!({ "type": "object", "properties": props }));
        assert!(serde_json::to_string(&huge).unwrap().len() <= MAX_SCHEMA_CHARS || huge["properties"]["p0"] == json!({ "type": "string" }));
        assert!(huge["properties"]["p0"].get("description").is_none());
    }

    #[test]
    fn results_become_capped_clean_text() {
        let r = outcome(&json!({
            "content": [
                { "type": "text", "text": "line 1\r\nline\u{202E} 2\u{0007}" },
                { "type": "image", "data": "AAAA", "mimeType": "image/png" },
                { "type": "resource", "resource": { "uri": "file:///a.txt", "text": "file body" } },
                { "type": "resource", "resource": { "uri": "file:///a.bin", "blob": "AA==", "mimeType": "application/pdf" } },
                { "type": "resource_link", "uri": "https://x.com/a", "name": "a" }
            ],
            "isError": true
        }));
        assert!(r.is_error);
        assert_eq!(r.text, "line 1\nline 2\n\nfile body\n\n[resource] a https://x.com/a");
        assert_eq!(r.omitted, vec!["image/png".to_string(), "application/pdf".to_string()]);

        let structured = outcome(&json!({ "content": [], "structuredContent": { "n": 1 } }));
        assert_eq!(structured.text, "{\"n\":1}");
        assert!(!structured.is_error);

        let long = outcome(&json!({ "content": [{ "type": "text", "text": "x".repeat(MAX_RESULT_CHARS + 10) }] }));
        assert!(long.text.ends_with("…[truncated: 10 more characters]"));
        assert!(long.text.chars().count() < MAX_RESULT_CHARS + 50);
    }
}
