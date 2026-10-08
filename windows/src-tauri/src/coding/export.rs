//! Explicit redacted handoff export; generated names and bounded content.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_CONTENT: usize = 128 * 1024;

pub(super) fn save(directory: &Path, content: &str) -> Result<PathBuf, &'static str> {
    if content.is_empty() || content.len() > MAX_CONTENT || content.contains('\0') {
        return Err("export-invalid-content");
    }
    fs::create_dir_all(directory).map_err(|_| "export-write-failed")?;
    let directory = if directory.is_absolute() { directory.to_path_buf() } else { std::env::current_dir().map_err(|_| "export-write-failed")?.join(directory) };
    let path = directory.join(format!("roadeep-handoff-{}.md", uuid::Uuid::new_v4()));
    // The filename is generated here, never accepted from the frontend.
    let mut sanitized = super::parser::clean(content, MAX_CONTENT);
    if sanitized.len() > MAX_CONTENT {
        let marker = "\n[truncated]";
        let mut boundary = MAX_CONTENT - marker.len();
        while !sanitized.is_char_boundary(boundary) { boundary -= 1; }
        sanitized.truncate(boundary); sanitized.push_str(marker);
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|_| "export-write-failed")?;
    if file.write_all(sanitized.as_bytes()).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        if fs::remove_file(&path).is_err() { crate::log::line("coding: failed to remove incomplete export"); }
        return Err("export-write-failed");
    }
    Ok(path)
}

#[tauri::command]
pub async fn coding_export(content: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = crate::settings::local_dir().join("exports");
        match save(&directory, &content) {
            Ok(path) => {
                crate::log::line("coding: explicit handoff export saved");
                Ok(path.to_string_lossy().into_owned())
            }
            Err(code) => { crate::log::line(format!("coding: explicit handoff export failed ({code})")); Err(code.to_owned()) }
        }
    }).await.map_err(|_| { crate::log::line("coding: export worker failed"); "export-worker-failed".to_owned() })?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf { std::env::temp_dir().join(format!("roadeep-export-test-{}", uuid::Uuid::new_v4())) }

    #[test]
    fn explicit_export_uses_safe_unique_filename_and_redacts() {
        let dir = fixture();
        let first = save(&dir, "# Handoff\nAPI_KEY=private-value\nTests: unknown").unwrap();
        let second = save(&dir, "second handoff").unwrap();
        assert_ne!(first, second);
        assert!(first.is_absolute()); assert_eq!(first.parent(), Some(dir.as_path()));
        let name = first.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("roadeep-handoff-")); assert!(name.ends_with(".md"));
        assert!(uuid::Uuid::parse_str(name.strip_prefix("roadeep-handoff-").unwrap().strip_suffix(".md").unwrap()).is_ok());
        assert!(!fs::read_to_string(&first).unwrap().contains("private-value"));
        assert!(OpenOptions::new().write(true).create_new(true).open(&first).is_err());
        assert_eq!(fs::read_to_string(second).unwrap(), "second handoff");
        let expanded = save(&dir, &"token=a\n".repeat(MAX_CONTENT / 8)).unwrap();
        assert!(fs::metadata(expanded).unwrap().len() <= MAX_CONTENT as u64);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_or_unwritable_exports_fail_without_saving() {
        let dir = fixture();
        for content in [String::new(), "a".repeat(MAX_CONTENT + 1), "null\0byte".into()] {
            assert_eq!(save(&dir, &content).unwrap_err(), "export-invalid-content");
        }
        assert!(!dir.exists());
        fs::write(&dir, "file blocks directory creation").unwrap();
        assert_eq!(save(&dir, "# Handoff").unwrap_err(), "export-write-failed");
        assert_eq!(fs::read_to_string(&dir).unwrap(), "file blocks directory creation");
        fs::remove_file(dir).unwrap();
    }
}
