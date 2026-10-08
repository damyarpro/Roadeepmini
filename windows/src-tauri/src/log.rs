// Small append-only log at %LOCALAPPDATA%\com.roadeep.desktop\roadeep.log — the Windows
// equivalent of nbLog() in HookServer.swift. Nothing leaves the machine.

use std::io::Write;

use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::settings;

pub fn line(message: impl AsRef<str>) {
    let t = unsafe { GetLocalTime() };
    let stamp = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    let dir = settings::local_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("roadeep.log");
    // Keep it from growing forever: start fresh past ~1 MB.
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        // One write per line: `writeln!` issues several, and two threads logging
        // at once then interleave their lines.
        let _ = file.write_all(format!("{stamp} {}\n", message.as_ref()).as_bytes());
    }
}
