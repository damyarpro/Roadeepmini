// "Open terminal" and the jump-to-terminal shortcut bring forward the window a
// coding session runs in, instead of opening its folder in VS Code (adapted
// from upstream's session_window.rs).
//
// The relay (roadeep-hook) is a grandchild of the coding agent, which runs in
// a shell, which runs in the terminal or editor whose window we want. When the
// relay connects its process is still alive, so walking up from it finds that
// window's process. What is found is kept per session ID, in memory only, and
// looked up when the button or the shortcut asks. Where nothing is found (a
// classic console, whose window belongs to a conhost that is no ancestor) the
// folder opens in VS Code, as before.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

/// One process as a snapshot lists it.
#[derive(Debug, Clone)]
pub struct Proc {
    pub parent: u32,
    /// The executable's file name, as the snapshot gives it (`Code.exe`).
    pub exe: String,
}

/// Never the window of a session: the desktop shell and the services at the
/// top of every process tree. Reaching one means the terminal window was not
/// an ancestor (a classic console window belongs to conhost).
const TREE_TOPS: &[&str] = &[
    "explorer.exe", "services.exe", "wininit.exe", "winlogon.exe", "svchost.exe",
    "smss.exe", "csrss.exe", "system", "sihost.exe", "userinit.exe",
];

/// How far up we look. A session sits a handful of levels below its window.
const MAX_DEPTH: usize = 16;

/// The ancestors of `start`, nearest first, stopping before the top of the
/// tree. `start` itself (the relay) is not included. A loop in the parent
/// links (a parent ID reused by a newer process) ends the walk.
pub fn ancestors(procs: &HashMap<u32, Proc>, start: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = vec![start];
    let mut current = start;
    while out.len() < MAX_DEPTH {
        let Some(proc) = procs.get(&current) else { break };
        let parent = proc.parent;
        if parent == 0 || seen.contains(&parent) {
            break;
        }
        let Some(up) = procs.get(&parent) else { break };
        if TREE_TOPS.contains(&up.exe.to_ascii_lowercase().as_str()) {
            break;
        }
        out.push(parent);
        seen.push(parent);
        current = parent;
    }
    out
}

/// The nearest of `line` (ancestors, nearest first) that owns a window: the
/// terminal or the editor.
pub fn window_owner(line: &[u32], has_window: impl Fn(u32) -> bool) -> Option<u32> {
    line.iter().copied().find(|pid| has_window(*pid))
}

/// Which of a process's windows to bring forward: the one whose title names
/// the session's folder (an editor with several projects open), else the first.
pub fn pick_window<'a, W>(windows: &'a [(W, String)], folder: &str) -> Option<&'a W> {
    let folder = folder.to_lowercase();
    let named = (!folder.is_empty())
        .then(|| windows.iter().find(|(_, title)| title.to_lowercase().contains(&folder)))
        .flatten();
    named.or_else(|| windows.first()).map(|(w, _)| w)
}

/// The last folder name of a path, either separator.
pub fn folder_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default()
}

// ── Sessions seen so far ──────────────────────────────────────────────────────

/// Session ID → the process that owns its window. Small: the oldest session is
/// dropped once there are more than this many.
const MAX_SESSIONS: usize = 64;

/// Remembered for a session whose window could not be found, so it is not
/// looked for on every event. No process has this ID.
pub const NO_WINDOW: u32 = 0;

/// (session, owner pid, owner executable). The executable is checked again
/// before a window is brought forward: a closed terminal's ID may be reused.
static SESSIONS: Mutex<Vec<(String, u32, String)>> = Mutex::new(Vec::new());

/// A session ID arrives in a hook payload: only what the agents send (a UUID
/// or similar) is kept, at most 128 characters.
fn valid_session(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn sessions() -> std::sync::MutexGuard<'static, Vec<(String, u32, String)>> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn remember(session: &str, owner: u32, exe: &str) {
    if !valid_session(session) {
        return;
    }
    let mut list = sessions();
    list.retain(|(s, _, _)| s != session);
    list.push((session.to_string(), owner, exe.to_string()));
    let excess = list.len().saturating_sub(MAX_SESSIONS);
    list.drain(..excess);
}

/// True once the session has been looked at, window found or not.
pub fn known(session: &str) -> bool {
    sessions().iter().any(|(s, _, _)| s == session)
}

/// The process owning the session's window and its executable, if one was found.
pub fn lookup(session: &str) -> Option<(u32, String)> {
    sessions()
        .iter()
        .find(|(s, pid, _)| s == session && *pid != NO_WINDOW)
        .map(|(_, pid, exe)| (*pid, exe.clone()))
}

pub fn forget(session: &str) {
    sessions().retain(|(s, _, _)| s != session);
}

/// Finds, once per session, the window it runs in (pipe.rs, on every hook
/// connection). Only while the session is unknown, so the process snapshot is
/// not taken on every event. The relay must still be running for its parents
/// to be found: a permission request always is (it waits for us), a quick
/// event may already have exited, and then a later event tries again.
pub fn note(pipe: &tokio::net::windows::named_pipe::NamedPipeServer, payload: &Value, event: &str) {
    let Some(session) = payload.get("session_id").and_then(Value::as_str) else { return };
    if event == "SessionEnd" {
        forget(session);
        return;
    }
    if !valid_session(session) || known(session) {
        return;
    }
    let Some(relay) = pipe_client_pid(pipe) else { return };
    let procs = process_table();
    let line = ancestors(&procs, relay);
    // No ancestors: the relay was already gone, so try again next time. Some,
    // but none with a window (a classic console): settled, VS Code it is.
    if line.is_empty() {
        return;
    }
    let windows = top_windows();
    match window_owner(&line, |pid| windows.iter().any(|(_, owner, _)| *owner == pid)) {
        Some(owner) => {
            let exe = procs.get(&owner).map(|p| p.exe.clone()).unwrap_or_default();
            crate::log::line(format!("session window: found ({exe})"));
            remember(session, owner, &exe);
        }
        None => remember(session, NO_WINDOW, ""),
    }
}

/// "Open terminal": brings forward the terminal or editor window the session
/// runs in, when it was found; otherwise opens the folder in VS Code, as before.
#[tauri::command]
pub fn open_session(session_id: Option<String>, path: Option<String>) -> bool {
    if let Some((owner, exe)) = session_id.as_deref().and_then(lookup) {
        let alive = process_table().get(&owner).is_some_and(|p| p.exe.eq_ignore_ascii_case(&exe));
        let folder = path.as_deref().map(folder_name).unwrap_or_default();
        if alive && focus_process_window(owner, folder) {
            return true;
        }
        crate::log::line("session window: gone; opening the folder instead");
    }
    crate::open_in_vscode(path)
}

// ── Windows ───────────────────────────────────────────────────────────────────

/// The process on the other end of a relay connection.
fn pipe_client_pid(pipe: &tokio::net::windows::named_pipe::NamedPipeServer) -> Option<u32> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    let mut pid = 0u32;
    unsafe { GetNamedPipeClientProcessId(HANDLE(pipe.as_raw_handle()), &mut pid).ok()? };
    (pid != 0).then_some(pid)
}

/// Every process, by ID: its parent and its executable's name.
fn process_table() -> HashMap<u32, Proc> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut out = HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            out.insert(
                entry.th32ProcessID,
                Proc { parent: entry.th32ParentProcessID, exe: String::from_utf16_lossy(&entry.szExeFile[..len]) },
            );
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

/// Visible top-level windows that are nobody's dialog and have a title:
/// (handle, process ID, title).
fn top_windows() -> Vec<(isize, u32, String)> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
    };
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<(isize, u32, String)>) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || GetWindow(hwnd, GW_OWNER).is_ok() {
                return true.into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
            let mut text = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut text);
            if len > 0 {
                list.push((hwnd.0 as isize, pid, String::from_utf16_lossy(&text[..len as usize])));
            }
        }
        true.into()
    }
    let mut list: Vec<(isize, u32, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

fn bring_forward(raw: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
    let hwnd = HWND(raw as *mut _);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Allowed: the click on the island, or the shortcut, that asked for
        // this was the last input.
        SetForegroundWindow(hwnd).as_bool()
    }
}

/// Brings `pid`'s window forward: the one titled after `folder` if there are several.
fn focus_process_window(pid: u32, folder: &str) -> bool {
    let candidates: Vec<(isize, String)> = top_windows()
        .into_iter()
        .filter(|(_, owner, _)| *owner == pid)
        .map(|(hwnd, _, title)| (hwnd, title))
        .collect();
    pick_window(&candidates, folder).is_some_and(|hwnd| bring_forward(*hwnd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(entries: &[(u32, u32, &str)]) -> HashMap<u32, Proc> {
        entries.iter().map(|(pid, parent, exe)| (*pid, Proc { parent: *parent, exe: exe.to_string() })).collect()
    }

    #[test]
    fn windows_terminal_is_found_above_the_shells() {
        // WindowsTerminal → pwsh → claude (node) → bash → roadeep-hook
        let procs = tree(&[
            (4, 0, "System"),
            (100, 4, "explorer.exe"),
            (200, 100, "WindowsTerminal.exe"),
            (300, 200, "pwsh.exe"),
            (400, 300, "node.exe"),
            (500, 400, "bash.exe"),
            (600, 500, "roadeep-hook.exe"),
        ]);
        let line = ancestors(&procs, 600);
        assert_eq!(line, [500, 400, 300, 200]);
        assert_eq!(window_owner(&line, |pid| pid == 200 || pid == 100), Some(200));
    }

    #[test]
    fn vs_code_is_found_through_its_pty_host() {
        let procs = tree(&[
            (100, 1, "explorer.exe"),
            (210, 100, "Code.exe"),
            (220, 210, "Code.exe"),
            (300, 220, "powershell.exe"),
            (400, 300, "node.exe"),
            (600, 400, "roadeep-hook.exe"),
        ]);
        assert_eq!(window_owner(&ancestors(&procs, 600), |pid| pid == 210), Some(210));
    }

    #[test]
    fn a_classic_console_finds_nothing_rather_than_the_desktop() {
        // cmd.exe's window belongs to conhost, which is not an ancestor; the
        // walk must stop at explorer instead of picking the taskbar.
        let procs = tree(&[
            (100, 1, "explorer.exe"),
            (300, 100, "cmd.exe"),
            (400, 300, "node.exe"),
            (600, 400, "roadeep-hook.exe"),
        ]);
        assert_eq!(window_owner(&ancestors(&procs, 600), |pid| pid == 100), None);
    }

    #[test]
    fn a_relay_already_gone_or_a_loop_ends_the_walk() {
        assert!(ancestors(&tree(&[]), 600).is_empty());
        let looped = tree(&[(1, 2, "a.exe"), (2, 1, "b.exe"), (3, 1, "roadeep-hook.exe")]);
        assert_eq!(ancestors(&looped, 3), [1, 2]);
        let deep: Vec<(u32, u32, &str)> = (1..100).map(|i| (i, i + 1, "x.exe")).collect();
        assert_eq!(ancestors(&tree(&deep), 1).len(), MAX_DEPTH);
    }

    #[test]
    fn the_window_named_after_the_project_wins() {
        let windows = vec![
            (1, "notes — Visual Studio Code".to_string()),
            (2, "app.ts — roadeep — Visual Studio Code".to_string()),
        ];
        assert_eq!(pick_window(&windows, "Roadeep"), Some(&2));
        assert_eq!(pick_window(&windows, "other"), Some(&1));
        assert_eq!(pick_window(&windows, ""), Some(&1));
        assert_eq!(pick_window::<i32>(&[], "x"), None);
        assert_eq!(folder_name(r"C:\Users\me\roadeep\"), "roadeep");
        assert_eq!(folder_name("/home/me/proj"), "proj");
        assert_eq!(folder_name(""), "");
    }

    #[test]
    fn sessions_are_remembered_by_id_and_only_plausible_ids_are_kept() {
        remember("s-test-1", 42, "WindowsTerminal.exe");
        remember("s-test-1", 43, "Code.exe");
        assert_eq!(lookup("s-test-1"), Some((43, "Code.exe".to_string())));
        remember("not a session/../x", 7, "x.exe");
        assert!(!known("not a session/../x"));
        forget("s-test-1");
        assert!(!known("s-test-1"));

        // A session without a window is settled, with nothing to show.
        remember("s-test-2", NO_WINDOW, "");
        assert!(known("s-test-2"));
        assert_eq!(lookup("s-test-2"), None);
        forget("s-test-2");

        // Only the latest sessions are kept. (One test: the list is shared.)
        for i in 0..(MAX_SESSIONS + 5) {
            remember(&format!("s-cap-{i}"), 1, "x.exe");
        }
        assert!(!known("s-cap-0"));
        assert!(known(&format!("s-cap-{}", MAX_SESSIONS + 4)));
        for i in 0..(MAX_SESSIONS + 5) {
            forget(&format!("s-cap-{i}"));
        }
    }

    #[test]
    fn an_unknown_session_falls_back_to_the_folder() {
        assert_eq!(lookup("never-seen"), None);
        assert_eq!(lookup(""), None);
    }
}
