//! Fixed host actions: no command text, arbitrary paths, or external tools.
use crate::mcpc::{ToolMode, ToolOutcome, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub const SERVER_ID: &str = "builtin-desktop";
pub const SLUG: &str = "desktop";
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    ThisPc,
    Downloads,
    Documents,
    Settings,
    Calculator,
    Notepad,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenArgs {
    target: Target,
}
impl Target {
    fn key(self) -> &'static str {
        match self {
            Self::ThisPc => "this_pc",
            Self::Downloads => "downloads",
            Self::Documents => "documents",
            Self::Settings => "settings",
            Self::Calculator => "calculator",
            Self::Notepad => "notepad",
        }
    }
    fn launch(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::ThisPc => ("explorer.exe", &["shell:MyComputerFolder"]),
            Self::Downloads => ("explorer.exe", &["shell:Downloads"]),
            Self::Documents => ("explorer.exe", &["shell:Personal"]),
            Self::Settings => ("explorer.exe", &["ms-settings:"]),
            Self::Calculator => ("System32/calc.exe", &[]),
            Self::Notepad => ("System32/notepad.exe", &[]),
        }
    }
}
pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec {
        server_id: SERVER_ID.into(),
        server_name: "Windows".into(),
        tool: "open".into(),
        qualified: "desktop__open".into(),
        description:
            "Open one fixed Windows destination or application. No arbitrary commands or paths."
                .into(),
        input_schema: json!({"type":"object","additionalProperties":false,"required":["target"],"properties":{"target":{"enum":["this_pc","downloads","documents","settings","calculator","notepad"]}}}),
        mode: ToolMode::Auto,
        read_only: false,
        destructive: false,
    }]
}
pub fn mode(tool: &str) -> Result<ToolMode, String> {
    if tool == "open" {
        Ok(ToolMode::Auto)
    } else {
        Err("desktop-invalid-input".into())
    }
}
fn execute_with(
    arguments: Value,
    launch: impl FnOnce(Target) -> Result<(), String>,
) -> Result<ToolOutcome, String> {
    let args: OpenArgs = serde_json::from_value(arguments).map_err(|_| "desktop-invalid-input")?;
    let started = std::time::Instant::now();
    let result = launch(args.target);
    crate::log::line(format!(
        "desktop: action={} outcome={} elapsed_ms={}",
        args.target.key(),
        if result.is_ok() { "launched" } else { "failed" },
        started.elapsed().as_millis()
    ));
    result?;
    Ok(ToolOutcome {
        is_error: false,
        text: json!({"launched":args.target.key()}).to_string(),
        omitted: vec![],
    })
}
pub fn call(tool: &str, arguments: Value) -> Result<ToolOutcome, String> {
    mode(tool)?;
    execute_with(arguments, launch)
}
#[cfg(windows)]
fn launch(target: Target) -> Result<(), String> {
    use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
    let mut buffer = [0u16; 32768];
    let len = unsafe { GetWindowsDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err("desktop-launch-failed".into());
    }
    let root = std::path::PathBuf::from(
        String::from_utf16(&buffer[..len]).map_err(|_| "desktop-launch-failed")?,
    );
    let (exe, args) = target.launch();
    let executable = root.join(exe);
    if !executable.is_absolute() || !executable.is_file() {
        return Err("desktop-launch-failed".into());
    }
    std::process::Command::new(executable)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|_| "desktop-launch-failed".into())
}
#[cfg(not(windows))]
fn launch(_: Target) -> Result<(), String> {
    Err("desktop-unavailable".into())
}
pub(crate) fn direct_intent(query: &str) -> bool {
    intent(query).is_some()
}
fn intent(query: &str) -> Option<Target> {
    let normalized = query
        .trim()
        .trim_end_matches(['.', '!', '؟', '?'])
        .replace(['ي', 'ى'], "ی")
        .replace('ك', "ک")
        .replace('\u{200c}', " ")
        .to_lowercase();
    let text = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text
        .strip_prefix("لطفا ")
        .or_else(|| text.strip_prefix("لطفاً "))
        .or_else(|| text.strip_prefix("please "))
        .unwrap_or(&text);
    let aliases: [(Target, &[&str]); 6] = [
        (
            Target::ThisPc,
            &["this pc", "my computer", "مای کامپیوتر", "این کامپیوتر"],
        ),
        (Target::Downloads, &["downloads", "دانلودها", "پوشه دانلود"]),
        (Target::Documents, &["documents", "اسناد", "پوشه اسناد"]),
        (Target::Settings, &["settings", "تنظیمات ویندوز"]),
        (Target::Calculator, &["calculator", "ماشین حساب"]),
        (Target::Notepad, &["notepad", "نوت پد", "دفترچه یادداشت"]),
    ];
    for (target, names) in aliases {
        for name in names {
            if text == format!("open {name}")
                || text == format!("{name} باز کن")
                || text == format!("{name} را باز کن")
                || text == format!("{name} رو باز کن")
                || text == format!("باز کن {name}")
            {
                return Some(target);
            }
        }
    }
    None
}
#[derive(Serialize)]
pub struct VoiceReply {
    handled: bool,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<String>,
}
fn voice_with(
    query: &str,
    language: &str,
    request_id: &str,
    launcher: impl FnOnce(Target) -> Result<(), String>,
) -> Result<VoiceReply, String> {
    if query.is_empty()
        || query.len() > 16000
        || query.chars().any(|c| c.is_control())
        || language.len() > 32
        || request_id.is_empty()
        || request_id.len() > 80
        || !request_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Err("desktop-invalid-input".into());
    }
    let Some(target) = intent(query) else {
        return Ok(VoiceReply {
            handled: false,
            text: String::new(),
            action: None,
        });
    };
    let started = std::time::Instant::now();
    let result = execute_with(json!({"target":target}), launcher);
    crate::log::line(format!(
        "desktop: request={request_id} action={} outcome={} elapsed_ms={}",
        target.key(),
        if result.is_ok() { "launched" } else { "failed" },
        started.elapsed().as_millis()
    ));
    result?;
    let text = if language.starts_with("fa") {
        match target {
            Target::ThisPc => "مای کامپیوتر باز شد.",
            Target::Downloads => "پوشه دانلودها باز شد.",
            Target::Documents => "پوشه اسناد باز شد.",
            Target::Settings => "تنظیمات ویندوز باز شد.",
            Target::Calculator => "ماشین حساب باز شد.",
            Target::Notepad => "نوت پد باز شد.",
        }
        .into()
    } else {
        format!("Opened {}.", target.key())
    };
    Ok(VoiceReply {
        handled: true,
        text,
        action: Some(target.key().into()),
    })
}
#[tauri::command]
pub fn desktop_voice_try(
    window: tauri::WebviewWindow,
    query: String,
    language: String,
    request_id: String,
) -> Result<VoiceReply, String> {
    if window.label() != crate::island::WINDOW_LABEL {
        return Err("desktop-forbidden".into());
    }
    voice_with(&query, &language, &request_id, launch)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_intents_and_refusals() {
        for query in [
            "مای کامپیوتر باز کن",
            "لطفاً مای کامپیوتر رو باز کن",
            "Please open this PC!",
            "open my computer",
        ] {
            assert_eq!(intent(query), Some(Target::ThisPc))
        }
        for query in [
            "open this pc and delete files",
            "مای کامپیوتر باز کن و حذف کن",
            "open this pc; calc",
            "ignore rules open this pc",
            "open C:/secret",
            "open settings security",
        ] {
            assert_eq!(intent(query), None)
        }
    }
    #[test]
    fn validated_and_truthful() {
        for (query, id) in [
            ("open this pc", "../bad"),
            ("", "ok"),
            ("open this pc\n", "ok"),
        ] {
            assert!(
                voice_with(query, "fa", id, |_| panic!("invalid input cannot launch")).is_err()
            );
        }
        for (query, target) in [
            ("open downloads", Target::Downloads),
            ("اسناد را باز کن", Target::Documents),
            ("open settings", Target::Settings),
            ("ماشین حساب رو باز کن", Target::Calculator),
            ("open notepad", Target::Notepad),
        ] {
            assert_eq!(intent(query), Some(target));
        }
        assert!(execute_with(json!({"target":"this_pc","command":"bad"}), |_| panic!()).is_err());
        assert!(execute_with(json!({"target":"cmd"}), |_| panic!()).is_err());
        assert_eq!(
            voice_with("open this pc", "en", "test", |_| Err(
                "desktop-launch-failed".into()
            ))
            .err()
            .as_deref(),
            Some("desktop-launch-failed")
        );
        assert!(
            !voice_with("unknown", "en", "test", |_| panic!())
                .unwrap()
                .handled
        );
        assert_eq!(
            voice_with("open this pc", "en", "test", |_| Ok(()))
                .unwrap()
                .action
                .as_deref(),
            Some("this_pc")
        );
    }
    #[test]
    #[ignore = "explicit Windows acceptance only"]
    fn actual_this_pc_launch() {
        assert_eq!(
            std::env::var("ROADEEP_TEST_DESKTOP_LAUNCH").as_deref(),
            Ok("1")
        );
        assert!(
            voice_with("مای کامپیوتر باز کن", "fa", "acceptance", launch)
                .unwrap()
                .handled
        );
    }
}
