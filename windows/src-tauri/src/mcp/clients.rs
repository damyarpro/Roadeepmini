// The coding apps roadeep-mcp.exe can be registered with, and where each one
// keeps its user-level MCP servers on Windows.
//
// Every client gets the same `roadeep` entry pointing at roadeep-mcp.exe; only
// the file, the container key and the entry's shape differ. The editing rules
// (one entry only, backups, fingerprints) live in install.rs and are the same
// for all of them.

use std::path::PathBuf;

use crate::errors;

/// The server name every client shows (`mcp__roadeep__…` in Claude Code).
pub const ENTRY: &str = "roadeep";
/// How an entry is recognised as Roadeep's: its command runs roadeep-mcp.
pub const MARKER: &str = "roadeep-mcp";

/// Fields shown as they are when an existing entry appears in a diff. Anything
/// else (env, headers, tokens…) shows its keys at most: those values can be
/// secrets and must never reach the UI.
pub const SHOWN_FIELDS: &[&str] = &["type", "command", "args", "url", "enabled", "disabled"];

/// Ours today, or from a build under the app's former name (replaced on
/// install, removed on uninstall like any entry of ours).
pub fn is_ours(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    command.contains(MARKER) || command.contains(&crate::migrate::legacy_mcp_marker())
}

/// An entry of ours that still runs the relay from the old folder.
pub fn is_legacy(command: &str) -> bool {
    command.to_ascii_lowercase().contains(&crate::migrate::legacy_mcp_marker())
}

/// What an existing `roadeep` entry with this command is.
pub fn classify(command: Option<&str>) -> Found {
    match command {
        Some(c) if is_legacy(c) => Found::Legacy,
        Some(c) if is_ours(c) => Found::Ours,
        _ => Found::Foreign,
    }
}

/// What a config file holds under our name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    Missing,
    Ours,
    /// Ours, but pointing at the relay a build under the former name staged:
    /// installing updates it, removing removes it.
    Legacy,
    /// A `roadeep` entry somebody else added: installing replaces it, removing refuses.
    Foreign,
}

/// What a format module hands back for one install or removal: the diff the
/// user reviews and the whole new file it stands for.
pub struct Edit {
    pub diff: String,
    pub bytes: Vec<u8>,
}

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// The text of a config file without its UTF-8 BOM, and whether it had one.
pub fn decode<'a>(bytes: &'a [u8], path: &std::path::Path) -> Result<(bool, &'a str), String> {
    let (bom, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    let text = std::str::from_utf8(body)
        .map_err(|e| errors::coded(errors::CFG_UNREADABLE, &[&path.display().to_string(), &e.to_string()]))?;
    Ok((bom, text))
}

/// Text edits (TOML, YAML) give the file back with the BOM it came with.
pub fn encode(bom: bool, text: String) -> Vec<u8> {
    let mut out = if bom { BOM.to_vec() } else { Vec::new() };
    out.extend_from_slice(text.as_bytes());
    out
}

/// "\r\n" when the file already uses it, so an edit doesn't mix line endings.
pub fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The diff line that stands for every other server: names only.
pub fn others_line(indent: &str, names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| format!("{indent}… {} other server(s) unchanged: {}", names.len(), names.join(", ")))
}

/// The JSON object our entry takes in each JSON client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `{type:"stdio",command,args:[],env:{}}` — what `claude mcp add -s user` writes.
    ClaudeCode,
    /// `{command,args:[]}`
    Plain,
    /// `{type:"stdio",command,args:[]}`
    VsCode,
    /// `{command,args:[],disabled:false}`
    Cline,
    /// `{type:"local",command:[exe],enabled:true}`
    OpenCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Servers under `container` (`mcpServers`, `servers`, `mcp`).
    Json { container: &'static str, shape: Shape },
    /// Codex: `[mcp_servers.roadeep]`.
    Toml,
    /// Hermes: a block-style top-level `mcp_servers:` map.
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// %USERPROFILE%
    Home,
    /// %APPDATA% (Roaming)
    AppData,
    /// %CODEX_HOME%, else %USERPROFILE%\.codex
    CodexHome,
}

#[derive(Debug)]
pub struct Client {
    pub id: &'static str,
    pub name: &'static str,
    pub format: Format,
    /// The config file, relative to `base`.
    pub file: (Base, &'static [&'static str]),
    /// The app counts as present when this folder (or the file) exists.
    pub dir: (Base, &'static [&'static str]),
}

const fn json(container: &'static str, shape: Shape) -> Format {
    Format::Json { container, shape }
}

pub const CLIENTS: &[Client] = &[
    Client {
        id: "claude-code",
        name: "Claude Code",
        format: json("mcpServers", Shape::ClaudeCode),
        file: (Base::Home, &[".claude.json"]),
        dir: (Base::Home, &[".claude"]),
    },
    Client {
        id: "claude-desktop",
        name: "Claude Desktop",
        format: json("mcpServers", Shape::Plain),
        file: (Base::AppData, &["Claude", "claude_desktop_config.json"]),
        dir: (Base::AppData, &["Claude"]),
    },
    Client {
        id: "vscode",
        name: "VS Code",
        format: json("servers", Shape::VsCode),
        file: (Base::AppData, &["Code", "User", "mcp.json"]),
        dir: (Base::AppData, &["Code", "User"]),
    },
    Client {
        id: "cursor",
        name: "Cursor",
        format: json("mcpServers", Shape::Plain),
        file: (Base::Home, &[".cursor", "mcp.json"]),
        dir: (Base::Home, &[".cursor"]),
    },
    Client {
        id: "windsurf",
        name: "Windsurf",
        format: json("mcpServers", Shape::Plain),
        file: (Base::Home, &[".codeium", "windsurf", "mcp_config.json"]),
        dir: (Base::Home, &[".codeium", "windsurf"]),
    },
    Client {
        id: "gemini",
        name: "Gemini CLI",
        format: json("mcpServers", Shape::Plain),
        file: (Base::Home, &[".gemini", "settings.json"]),
        dir: (Base::Home, &[".gemini"]),
    },
    Client {
        id: "cline",
        name: "Cline (VS Code)",
        format: json("mcpServers", Shape::Cline),
        file: (
            Base::AppData,
            &["Code", "User", "globalStorage", "saoudrizwan.claude-dev", "settings", "cline_mcp_settings.json"],
        ),
        dir: (Base::AppData, &["Code", "User", "globalStorage", "saoudrizwan.claude-dev"]),
    },
    Client {
        id: "kiro",
        name: "Kiro",
        format: json("mcpServers", Shape::Plain),
        file: (Base::Home, &[".kiro", "settings", "mcp.json"]),
        dir: (Base::Home, &[".kiro"]),
    },
    Client {
        id: "opencode",
        name: "OpenCode",
        format: json("mcp", Shape::OpenCode),
        file: (Base::Home, &[".config", "opencode", "opencode.json"]),
        dir: (Base::Home, &[".config", "opencode"]),
    },
    Client {
        id: "codex",
        name: "Codex (OpenAI)",
        format: Format::Toml,
        file: (Base::CodexHome, &["config.toml"]),
        dir: (Base::CodexHome, &[]),
    },
    Client {
        id: "hermes",
        name: "Hermes Agent",
        format: Format::Yaml,
        file: (Base::Home, &[".hermes", "config.yaml"]),
        dir: (Base::Home, &[".hermes"]),
    },
];

/// Unknown ids are refused: the id comes from the web view.
pub fn find(id: &str) -> Result<&'static Client, String> {
    CLIENTS.iter().find(|c| c.id == id).ok_or_else(|| errors::coded(errors::MCP_UNKNOWN_CLIENT, &[id]))
}

/// The folders the paths hang off; explicit so tests never touch the real ones.
pub struct Roots {
    pub home: PathBuf,
    pub appdata: PathBuf,
    pub codex_home: Option<PathBuf>,
}

impl Roots {
    pub fn from_env() -> Roots {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("USERPROFILE").unwrap_or_else(|| PathBuf::from("."));
        let appdata = var("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
        Roots { home, appdata, codex_home: var("CODEX_HOME") }
    }

    fn base(&self, base: Base) -> PathBuf {
        match base {
            Base::Home => self.home.clone(),
            Base::AppData => self.appdata.clone(),
            Base::CodexHome => self.codex_home.clone().unwrap_or_else(|| self.home.join(".codex")),
        }
    }

    fn join(&self, (base, parts): (Base, &[&str])) -> PathBuf {
        parts.iter().fold(self.base(base), |p, part| p.join(part))
    }
}

impl Client {
    pub fn config_path(&self, roots: &Roots) -> PathBuf {
        roots.join(self.file)
    }

    pub fn detect_dir(&self, roots: &Roots) -> PathBuf {
        roots.join(self.dir)
    }

    pub fn detected(&self, roots: &Roots) -> bool {
        self.config_path(roots).is_file() || self.detect_dir(roots).is_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(codex: Option<&str>) -> Roots {
        Roots {
            home: PathBuf::from(r"C:\Users\a"),
            appdata: PathBuf::from(r"C:\Users\a\AppData\Roaming"),
            codex_home: codex.map(PathBuf::from),
        }
    }

    #[test]
    fn ids_are_unique_and_unknown_ids_are_refused() {
        let mut seen = std::collections::HashSet::new();
        for c in CLIENTS {
            assert!(seen.insert(c.id), "duplicate {}", c.id);
            assert_eq!(find(c.id).unwrap().id, c.id);
        }
        let err = find("../etc").unwrap_err();
        assert!(err.starts_with(errors::MCP_UNKNOWN_CLIENT), "{err}");
    }

    #[test]
    fn paths_follow_the_documented_locations() {
        let r = roots(None);
        let path = |id: &str| find(id).unwrap().config_path(&r).display().to_string();
        assert_eq!(path("claude-code"), r"C:\Users\a\.claude.json");
        assert_eq!(path("claude-desktop"), r"C:\Users\a\AppData\Roaming\Claude\claude_desktop_config.json");
        assert_eq!(path("vscode"), r"C:\Users\a\AppData\Roaming\Code\User\mcp.json");
        assert_eq!(path("cursor"), r"C:\Users\a\.cursor\mcp.json");
        assert_eq!(path("windsurf"), r"C:\Users\a\.codeium\windsurf\mcp_config.json");
        assert_eq!(path("gemini"), r"C:\Users\a\.gemini\settings.json");
        assert_eq!(
            path("cline"),
            r"C:\Users\a\AppData\Roaming\Code\User\globalStorage\saoudrizwan.claude-dev\settings\cline_mcp_settings.json"
        );
        assert_eq!(path("kiro"), r"C:\Users\a\.kiro\settings\mcp.json");
        assert_eq!(path("opencode"), r"C:\Users\a\.config\opencode\opencode.json");
        assert_eq!(path("codex"), r"C:\Users\a\.codex\config.toml");
        assert_eq!(path("hermes"), r"C:\Users\a\.hermes\config.yaml");

        let custom = roots(Some(r"D:\codex"));
        assert_eq!(find("codex").unwrap().config_path(&custom), PathBuf::from(r"D:\codex\config.toml"));
        assert_eq!(find("codex").unwrap().detect_dir(&custom), PathBuf::from(r"D:\codex"));
    }

    #[test]
    fn a_client_is_detected_by_its_file_or_its_folder() {
        let dir = std::env::temp_dir().join(format!("roadeep-mcp-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let r = Roots { home: dir.clone(), appdata: dir.join("Roaming"), codex_home: None };
        let cursor = find("cursor").unwrap();
        assert!(!cursor.detected(&r));
        std::fs::create_dir_all(dir.join(".cursor")).unwrap();
        assert!(cursor.detected(&r), "the folder alone is enough");

        let code = find("claude-code").unwrap();
        assert!(!code.detected(&r));
        std::fs::write(dir.join(".claude.json"), "{}").unwrap();
        assert!(code.detected(&r), "the file alone is enough");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
