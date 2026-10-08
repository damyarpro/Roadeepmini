//! Opt-in local evidence, never a tool executor or an approval authority.
//! Limits: 16 sessions, 256 events, 2 MiB records, 1 MiB reads, bounded discovery.
mod parser;
pub mod export;
pub mod history;
pub mod git;
pub mod handoff;
pub use parser::{CodingEvent, clean};
use parser::Parser;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

const CHUNK: usize = 64 * 1024;
const RECORD: usize = 2 * 1024 * 1024;
const READ_BUDGET: usize = 1024 * 1024;
const BOOTSTRAP: u64 = 2 * 1024 * 1024;
const EVENTS: usize = 256;
const FILES: usize = 16;

#[derive(Clone, Serialize, PartialEq)]
pub struct CodingStatus { enabled: bool, available: bool, #[serde(skip_serializing_if = "Option::is_none")] error: Option<&'static str> }

struct Tail { offset: u64, partial: Vec<u8>, skipping: bool, parser: Parser, modified: SystemTime, epoch: u64 }

impl Tail {
    fn open(path: &Path, size: u64, modified: SystemTime, fresh: bool) -> std::io::Result<Self> {
        let mut tail = Self { offset: if fresh { 0 } else { size }, partial: Vec::new(), skipping: !fresh && size > 0, parser: Parser::default(), modified, epoch: now() };
        // The UUID suffix is identity only, never a path or an execution target.
        if let Some(id) = path.file_stem().and_then(|v| v.to_str()).and_then(|v| v.get(v.len().saturating_sub(36)..)).and_then(|v| uuid::Uuid::parse_str(v).ok()) {
            tail.parser.session = Some(format!("codex:{id}"));
        }
        if !fresh {
            let mut prefix = Vec::new();
            File::open(path)?.take(CHUNK as u64).read_to_end(&mut prefix)?;
            for line in prefix.split(|b| *b == b'\n').take(8) {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) {
                    if v["type"] == "session_meta" { let _ = tail.parser.parse(line, 0, now()); break; }
                }
            }
            // If EOF is a complete line, the next appended line is not a continuation.
            if size > 0 { let mut file = File::open(path)?; file.seek(SeekFrom::End(-1))?; let mut last = [0]; file.read_exact(&mut last)?; tail.skipping = last[0] != b'\n'; }
        }
        Ok(tail)
    }

    // Recover only recent bounded evidence when attaching to an already active session.
    // The prefix supplies identity; the clipped first record is skipped without decoding.
    fn recent(path: &Path, size: u64, modified: SystemTime) -> std::io::Result<Self> {
        let mut tail = Self::open(path, size, modified, false)?;
        tail.offset = size.saturating_sub(BOOTSTRAP);
        tail.skipping = tail.offset != 0;
        Ok(tail)
    }

    #[cfg(test)]
    fn read(&mut self, path: &Path, size: u64, observed: u64) -> std::io::Result<(Vec<CodingEvent>, usize)> {
        self.read_bounded(path, size, observed, READ_BUDGET)
    }

    fn read_bounded(&mut self, path: &Path, size: u64, observed: u64, budget: usize) -> std::io::Result<(Vec<CodingEvent>, usize)> {
        if size < self.offset { self.offset = 0; self.partial.clear(); self.skipping = false; self.parser = Parser::default(); self.epoch += 1; }
        if size == self.offset { return Ok((Vec::new(), 0)); }
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::with_capacity(READ_BUDGET);
        file.take(READ_BUDGET.min(budget) as u64).read_to_end(&mut bytes)?;
        let mut events = Vec::new(); let mut invalid = 0;
        for b in bytes {
            self.offset += 1;
            if b == b'\n' {
                if !self.skipping && !self.partial.is_empty() {
                    match self.parser.parse(&self.partial, self.offset, observed) { Ok(Some(mut e)) => { e.id = format!("{}:{}", e.id, self.epoch); events.push(e); }, Ok(None) => (), Err(()) => invalid += 1 }
                }
                self.partial.clear(); self.skipping = false;
            } else if !self.skipping {
                if self.partial.len() == RECORD { self.partial.clear(); self.skipping = true; invalid += 1; } else { self.partial.push(b); }
            }
        }
        Ok((events, invalid))
    }
}

struct Inner {
    enabled: bool, available: bool, error: Option<&'static str>, enabled_at: SystemTime,
    files: HashMap<PathBuf, Tail>, events: VecDeque<CodingEvent>, invalid: usize,
    cleared_paths: VecDeque<PathBuf>,
    discovery_at: Option<SystemTime>, discovery_paths: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct CodingObserver { inner: Arc<Mutex<Inner>>, root: Option<PathBuf> }

impl CodingObserver {
    pub fn new(enabled: bool) -> Self {
        let root = std::env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex"))).map(|p| p.join("sessions"));
        Self { inner: Arc::new(Mutex::new(Inner { enabled, available: false, error: None, enabled_at: SystemTime::now(), files: HashMap::new(), events: VecDeque::new(), invalid: 0, cleared_paths: VecDeque::new(), discovery_at: None, discovery_paths: Vec::new() })), root }
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut inner = self.inner.lock().unwrap();
        if inner.enabled != enabled {
            inner.enabled = enabled; inner.enabled_at = SystemTime::now(); inner.files.clear(); inner.events.clear(); inner.cleared_paths.clear(); inner.error = None; inner.available = false;
        }
    }

    pub fn status(&self) -> CodingStatus {
        let i = self.inner.lock().unwrap(); CodingStatus { enabled: i.enabled, available: i.available, error: i.error }
    }

    fn poll(&self, app: &AppHandle) {
        self.poll_with(|event| app.emit("coding-activity", event).map_err(|_| ()));
    }

    fn poll_with(&self, mut emit: impl FnMut(CodingEvent) -> Result<(), ()>) {
        // The mutex spans emission so disable/clear cannot overtake an in-flight batch.
        let mut inner = self.inner.lock().unwrap();
        if !inner.enabled { return; }
        let Some(root) = self.root.as_ref() else { inner.error = Some("home-unavailable"); return; };
        match fs::metadata(root) {
            Ok(meta) if meta.is_dir() => { inner.available = true; inner.error = None; }
            Ok(_) => { inner.available = false; inner.error = Some("sessions-unavailable"); return; }
            Err(e) => { inner.available = false; inner.error = if e.kind() == std::io::ErrorKind::NotFound { None } else { Some("sessions-unreadable") }; return; }
        }
        let mut found = Vec::new();
        let day = (now() / 86_400_000) as i64;
        let clock = SystemTime::now();
        let discover_all = inner.discovery_at.is_none_or(|at| clock.duration_since(at).unwrap_or_default() >= Duration::from_secs(30));
        let older_folders = if discover_all { inner.discovery_at = Some(clock); date_folders(root) } else { Vec::new() };
        for path in &inner.discovery_paths {
            if let Ok(meta) = fs::symlink_metadata(path) {
                if !meta.is_file() || meta.file_type().is_symlink() { continue; }
                #[cfg(windows)] {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 { continue; }
                }
                found.push((path.clone(), meta.len(), meta.modified().unwrap_or(UNIX_EPOCH), meta.created().unwrap_or(UNIX_EPOCH)));
            }
        }
        let folders = prioritized_folders(root, day, older_folders);
        let mut discovery_budget = 8192usize;
        for folder in folders {
            if discovery_budget == 0 { break; }
            let entries = match fs::read_dir(folder) { Ok(v) => v, Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue, Err(_) => { inner.error = Some("discovery-unreadable"); continue; } };
            for entry in entries.take(512.min(discovery_budget)) {
                discovery_budget -= 1;
                let Ok(entry) = entry else { inner.error = Some("discovery-unreadable"); continue; };
                let path = entry.path();
                if path.extension().is_none_or(|s| s != "jsonl") || !entry.file_name().to_string_lossy().starts_with("rollout-") { continue; }
                let Ok(meta) = entry.metadata() else { inner.error = Some("session-unreadable"); continue; };
                if !meta.is_file() || entry.file_type().is_ok_and(|t| t.is_symlink()) { continue; }
                #[cfg(windows)] {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 { continue; }
                }
                let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                found.push((path, meta.len(), modified, meta.created().unwrap_or(UNIX_EPOCH)));
            }
        }
        found.sort_by(|a,b| b.2.cmp(&a.2));
        let mut seen = std::collections::HashSet::new(); found.retain(|(path,_,_,_)| seen.insert(path.clone())); found.truncate(FILES);
        inner.discovery_paths = found.iter().map(|(p,_,_,_)| p.clone()).collect();
        inner.files.retain(|path,_| found.iter().any(|(p,_,_,_)| p == path));
        let mut read_budget = 8 * READ_BUDGET;
        for (path, size, modified, created) in found {
            if read_budget == 0 { break; }
            if !inner.files.contains_key(&path) {
                let fresh = created > inner.enabled_at && !inner.cleared_paths.contains(&path);
                let recent = !inner.cleared_paths.contains(&path) && clock.duration_since(modified).unwrap_or_default() <= Duration::from_secs(300);
                let opened = if !fresh && recent { Tail::recent(&path, size, modified) } else { Tail::open(&path, size, modified, fresh) };
                match opened { Ok(tail) => { inner.files.insert(path.clone(), tail); }, Err(_) => { inner.error = Some("session-unreadable"); continue; } }
            }
            let tail = inner.files.get_mut(&path).unwrap();
            // Rewritten same-length logs are a rotation too.
            if size == tail.offset && modified != tail.modified { tail.offset = 0; tail.partial.clear(); tail.parser = Parser::default(); tail.skipping = false; tail.epoch += 1; }
            tail.modified = modified;
            let before = if size < tail.offset { 0 } else { tail.offset };
            let result = tail.read_bounded(&path, size, now(), read_budget);
            read_budget = read_budget.saturating_sub(tail.offset.saturating_sub(before) as usize);
            match result {
                Ok((events, invalid)) => {
                    inner.invalid = inner.invalid.saturating_add(invalid);
                    for event in events {
                        if inner.events.iter().any(|e| e.id == event.id) { continue; }
                        // Bootstrap visits files by modification time, not event time. Keep
                        // the newest evidence globally so old worker tails cannot evict the
                        // active parent from a frontend's initial snapshot.
                        let position = inner.events.iter().position(|e| e.at > event.at).unwrap_or(inner.events.len());
                        inner.events.insert(position, event.clone());
                        while inner.events.len() > EVENTS { inner.events.pop_front(); }
                        if emit(event).is_err() { inner.error = Some("event-delivery-failed"); }
                    }
                }
                Err(_) => inner.error = Some("session-unreadable"),
            }
        }
        if inner.invalid > 0 { crate::log::line(format!("coding: ignored {} malformed/oversized records", inner.invalid)); inner.invalid = 0; }
    }
}

fn prioritized_folders(root: &Path, day: i64, older: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for delta in [0, -1, 1] {
        let (y,m,d) = civil_from_days(day + delta);
        folders.push(root.join(format!("{y:04}")).join(format!("{m:02}")).join(format!("{d:02}")));
    }
    for folder in older { if !folders.contains(&folder) { folders.push(folder); } }
    folders
}

fn date_folders(root: &Path) -> Vec<PathBuf> {
    fn children(path: &Path, digits: usize, limit: usize) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(path) else { return Vec::new(); };
        entries.take(limit).filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name(); let name = name.to_str()?;
            if name.len() != digits || !name.bytes().all(|b| b.is_ascii_digit()) { return None; }
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            if !meta.is_dir() || meta.file_type().is_symlink() { return None; }
            #[cfg(windows)] {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 { return None; }
            }
            Some(entry.path())
        }).collect()
    }
    let mut folders = Vec::new();
    let mut years = children(root, 4, 32); years.sort();
    for year in years.into_iter().rev().take(4) {
        let mut months = children(&year, 2, 12); months.sort();
        for month in months.into_iter().rev() {
            let mut days = children(&month, 2, 31); days.sort();
            for day in days.into_iter().rev() {
                folders.push(day);
            }
        }
    }
    folders
}

pub fn start(app: AppHandle) {
    let observer = app.state::<CodingObserver>().inner.clone();
    let root = app.state::<CodingObserver>().root.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let handle = app.clone(); let observer = CodingObserver { inner: observer.clone(), root: root.clone() };
            let enabled = observer.inner.lock().unwrap().enabled;
            if enabled {
                if tauri::async_runtime::spawn_blocking(move || {
                    let before = observer.status(); observer.poll(&handle); let after = observer.status();
                    if before != after { let _ = handle.emit("coding-status", after); }
                }).await.is_err() { crate::log::line("coding: observer worker failed"); }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

#[tauri::command]
pub async fn coding_snapshot(observer: State<'_, CodingObserver>) -> Result<Vec<CodingEvent>, String> {
    let observer = observer.inner().clone();
    tauri::async_runtime::spawn_blocking(move || observer.inner.lock().unwrap().events.iter().cloned().collect()).await.map_err(|_| worker_error())
}
#[tauri::command]
pub async fn coding_clear(observer: State<'_, CodingObserver>, history: State<'_, history::CodingHistory>) -> Result<(), String> {
    let observer = observer.inner().clone();
    let history = history.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { clear(&observer); history.clear().map(|_| ()) }).await.map_err(|_| worker_error())?
}
fn clear(observer: &CodingObserver) {
    let mut inner = observer.inner.lock().unwrap(); inner.events.clear();
    inner.enabled_at = SystemTime::now();
    let paths: Vec<_> = inner.files.keys().cloned().collect();
    for path in paths {
        if !inner.cleared_paths.contains(&path) { inner.cleared_paths.push_back(path); }
    }
    while inner.cleared_paths.len() > FILES * 2 { inner.cleared_paths.pop_front(); }
    let mut unreadable = 0;
    inner.files.retain(|path, tail| {
        // Discard partial evidence even if the file vanished or became unreadable.
        tail.partial.clear(); tail.skipping = false; tail.epoch += 1;
        if let Ok(meta) = fs::metadata(path) {
            tail.offset = meta.len(); tail.modified = meta.modified().unwrap_or(UNIX_EPOCH);
            if tail.offset > 0 {
                match File::open(path) {
                    Ok(mut file) => {
                    let mut last = [0];
                    if file.seek(SeekFrom::End(-1)).and_then(|_| file.read_exact(&mut last)).is_ok() { tail.skipping = last[0] != b'\n'; }
                    else { unreadable += 1; return false; }
                    }
                    Err(_) => { unreadable += 1; return false; }
                }
            }
            true
        } else {
            unreadable += 1;
            false
        }
    });
    if unreadable > 0 {
        inner.error = Some("clear-tail-unreadable");
        crate::log::line(format!("coding: clear discarded {unreadable} unreadable tails"));
    }
}
#[tauri::command]
pub async fn coding_status(observer: State<'_, CodingObserver>) -> Result<CodingStatus, String> {
    let observer = observer.inner().clone();
    tauri::async_runtime::spawn_blocking(move || observer.status()).await.map_err(|_| worker_error())
}
fn worker_error() -> String { crate::log::line("coding: command worker failed"); "observer-worker-failed".into() }
fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128) as u64 }
fn civil_from_days(days: i64) -> (i64,i64,i64) {
    let z = days + 719468; let era = z.div_euclid(146097); let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5*doy+2)/153; let d = doy-(153*mp+2)/5+1; let m = mp+if mp<10 {3} else {-9};
    (y+i64::from(m<=2),m,d)
}

#[cfg(test)]
mod tests;
