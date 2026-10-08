//! App-owned, pinned runtimes. No caller-provided URL, path or command is accepted.
mod assets;
mod install;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager};

#[derive(Clone, Debug)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub llama_exe: PathBuf,
    pub llama_cpu_exe: PathBuf,
    pub brain_model: PathBuf,
    pub whisper_exe: PathBuf,
    pub whisper_model: PathBuf,
    pub piper_exe: PathBuf,
    pub piper_model: PathBuf,
    pub piper_config: PathBuf,
    pub piper_espeak_data: PathBuf,
    pub speaker_dll: PathBuf,
    pub speaker_model: PathBuf,
}

#[cfg(test)]
mod lifecycle_tests {
    #[test]
    fn installation_and_active_engine_are_mutually_exclusive() {
        let state = super::RuntimeState::default();
        let operation = state.operation_guard().unwrap();
        assert!(state.activity.clone().try_write_owned().is_err());
        drop(operation);
        let installation = state.activity.clone().try_write_owned().unwrap();
        assert!(state.operation_guard().is_err());
        drop(installation);
        assert!(state.operation_guard().is_ok());
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub ready: bool,
    pub enabled: bool,
    pub phase: String,
    pub component: String,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            ready: false,
            enabled: false,
            phase: "not-installed".into(),
            component: String::new(),
            downloaded: 0,
            total: assets::total(),
            error: None,
        }
    }
}
#[derive(Default)]
pub struct RuntimeState {
    status: Arc<Mutex<Status>>,
    verified: Arc<Mutex<Option<RuntimePaths>>>,
    initialized: AtomicBool,
    initialization: tokio::sync::Mutex<()>,
    activity: Arc<tokio::sync::RwLock<()>>,
    enabled: AtomicBool,
    installing: AtomicBool,
    pub(crate) generation: Arc<AtomicU64>,
    last_event: Mutex<Option<std::time::Instant>>,
}
impl RuntimeState {
    pub(crate) fn operation_guard(&self) -> Result<tokio::sync::OwnedRwLockReadGuard<()>, String> {
        self.activity
            .clone()
            .try_read_owned()
            .map_err(|_| "local-runtime-installing".into())
    }
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    pub fn shutdown(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    fn update(&self, app: &tauri::AppHandle, status: Status) {
        if let Ok(mut current) = self.status.lock() {
            *current = status.clone();
        }
        if let Ok(mut last) = self.last_event.lock() {
            if !matches!(status.phase.as_str(), "ready" | "error" | "cancelled")
                && last.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250))
            {
                return;
            }
            *last = Some(std::time::Instant::now());
        }
        if let Err(e) = app.emit("local-runtime-progress", status) {
            crate::log::line(format!("local runtime progress event failed: {e}"));
        }
    }
}
fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let parent = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "Runtime storage unavailable")?
        .join("local-ai");
    install::safe_directory(&parent)?;
    Ok(parent.join("v1"))
}
pub fn paths(app: &tauri::AppHandle) -> Result<RuntimePaths, String> {
    app.state::<RuntimeState>()
        .verified
        .lock()
        .map_err(|_| "Runtime state unavailable")?
        .clone()
        .ok_or("Local runtime is not verified".into())
}
fn access(window: &tauri::WebviewWindow, mutate: bool) -> Result<(), String> {
    if window.label() == "settings" || (!mutate && window.label() == "island") {
        Ok(())
    } else {
        Err("Local runtime permission denied".into())
    }
}
/// Native inference calls this independently of whether settings has been opened.
pub async fn ensure_ready(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    if state.initialized.load(Ordering::Acquire) {
        return Ok(());
    }
    let _initialization = state.initialization.lock().await;
    if state.initialized.load(Ordering::Acquire) {
        return Ok(());
    }
    if state.installing.load(Ordering::Acquire) {
        return Err("Local runtime installation is active".into());
    }
    let runtime_root = root(app)?;
    let result = tauri::async_runtime::spawn_blocking(move || install::verify(&runtime_root))
        .await
        .map_err(|_| "Runtime verification failed")?;
    match result {
        Ok(Some(paths)) => {
            let enabled = match install::read_enabled(&paths.root) {
                Ok(enabled) => enabled,
                Err(error) => {
                    state.update(
                        app,
                        Status {
                            phase: "error".into(),
                            error: Some(error.clone()),
                            ..Status::default()
                        },
                    );
                    state.initialized.store(true, Ordering::Release);
                    return Err(error);
                }
            };
            *state
                .verified
                .lock()
                .map_err(|_| "Runtime state unavailable")? = Some(paths);
            state.enabled.store(enabled, Ordering::Release);
            state.update(
                app,
                Status {
                    ready: true,
                    enabled,
                    phase: "ready".into(),
                    downloaded: assets::total(),
                    ..Status::default()
                },
            );
        }
        Ok(None) => {
            state.initialized.store(true, Ordering::Release);
            if let Some(bundle) = bundled_directory(app)? {
                start_install(app.clone(), Some(bundle))?;
            }
        }
        Err(error) => {
            state.update(
                app,
                Status {
                    phase: "error".into(),
                    error: Some(error.clone()),
                    ..Status::default()
                },
            );
            state.initialized.store(true, Ordering::Release);
            return Err(error);
        }
    }
    state.initialized.store(true, Ordering::Release);
    Ok(())
}
#[tauri::command]
pub async fn local_runtime_status(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Status, String> {
    access(&window, false)?;
    // Status exposes verification failure and an active installer through its structured state.
    if let Err(error) = ensure_ready(&app).await {
        crate::log::line(format!("local runtime initialization: {error}"));
        let state = app.state::<RuntimeState>();
        if !state.installing.load(Ordering::Acquire) {
            state.update(
                &app,
                Status {
                    phase: "error".into(),
                    error: Some(error),
                    ..Status::default()
                },
            );
        }
    }
    let state = app.state::<RuntimeState>();
    let result = state
        .status
        .lock()
        .map_err(|_| "Runtime state unavailable")?
        .clone();
    Ok(result)
}
fn bundled_directory(app: &tauri::AppHandle) -> Result<Option<PathBuf>, String> {
    let path = app
        .path()
        .resource_dir()
        .map_err(|_| "Application resources unavailable")?
        .join("local-ai-packages");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Bundled runtime directory inaccessible".into()),
    }
}
/// A full installer prepares the per-user runtime once; it never loads a model for inference at launch.
pub fn prepare_bundled(app: tauri::AppHandle) {
    match bundled_directory(&app) {
        Ok(Some(_)) => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = ensure_ready(&app).await {
                    crate::log::line(format!("local bundled runtime preparation failed: {error}"));
                    app.state::<RuntimeState>().update(
                        &app,
                        Status {
                            phase: "error".into(),
                            error: Some(error),
                            ..Status::default()
                        },
                    );
                }
            });
        }
        Ok(None) => (),
        Err(error) => {
            crate::log::line(format!(
                "local bundled runtime resources unavailable: {error}"
            ));
            app.state::<RuntimeState>().update(
                &app,
                Status {
                    phase: "error".into(),
                    error: Some(error),
                    ..Status::default()
                },
            );
        }
    }
}
#[tauri::command]
pub async fn local_runtime_install(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Status, String> {
    access(&window, true)?;
    if let Err(error) = ensure_ready(&app).await {
        crate::log::line(format!("local runtime reinstall initialization: {error}"));
    }
    let state = app.state::<RuntimeState>();
    if state.installing.load(Ordering::Acquire) {
        return state
            .status
            .lock()
            .map_err(|_| "Runtime state unavailable".into())
            .map(|status| status.clone());
    }
    start_install(app.clone(), bundled_directory(&app)?)
}
fn start_install(app: tauri::AppHandle, bundled: Option<PathBuf>) -> Result<Status, String> {
    let state = app.state::<RuntimeState>();
    let installation_guard = state
        .activity
        .clone()
        .try_write_owned()
        .map_err(|_| "Local engine is busy; stop the active operation before installing")?;
    if state.installing.swap(true, Ordering::AcqRel) {
        return Err("Local runtime installation already running".into());
    }
    let runtime_root = match root(&app) {
        Ok(p) => p,
        Err(e) => {
            state.installing.store(false, Ordering::Release);
            return Err(e);
        }
    };
    let generation = state.generation.fetch_add(1, Ordering::AcqRel) + 1;
    let cancelled = state.generation.clone();
    let initial = Status {
        phase: if bundled.is_some() {
            "verifying"
        } else {
            "downloading"
        }
        .into(),
        ..Status::default()
    };
    state.update(&app, initial.clone());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _installation_guard = installation_guard;
        let result = install::install_from(
            &runtime_root,
            cancelled,
            generation,
            |status| handle.state::<RuntimeState>().update(&handle, status),
            bundled.as_deref(),
        )
        .await;
        let state = handle.state::<RuntimeState>();
        match result {
            Ok(p) => {
                *state.verified.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
                let enabled = install::read_enabled(&runtime_root).unwrap_or(false);
                state.enabled.store(enabled, Ordering::Release);
                state.initialized.store(true, Ordering::Release);
                state.update(
                    &handle,
                    Status {
                        ready: true,
                        enabled,
                        phase: "ready".into(),
                        downloaded: assets::total(),
                        ..Status::default()
                    },
                );
                crate::log::line("local runtime installation completed");
            }
            Err(error) => {
                let ready = state.verified.lock().map(|p| p.is_some()).unwrap_or(false);
                let cancelled = error == "Installation cancelled";
                state.update(
                    &handle,
                    Status {
                        ready,
                        enabled: state.enabled(),
                        phase: if cancelled { "cancelled" } else { "error" }.into(),
                        error: if cancelled { None } else { Some(error) },
                        ..Status::default()
                    },
                );
                crate::log::line(if cancelled {
                    "local runtime installation cancelled"
                } else {
                    "local runtime installation failed"
                });
            }
        }
        state.installing.store(false, Ordering::Release);
    });
    Ok(initial)
}
#[tauri::command]
pub fn local_runtime_cancel(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    access(&window, true)?;
    app.state::<RuntimeState>().shutdown();
    Ok(())
}
#[tauri::command]
pub async fn local_runtime_enable(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    enabled: bool,
) -> Result<Status, String> {
    access(&window, false)?;
    ensure_ready(&app).await?;
    let p = paths(&app)?;
    install::write_enabled(&p.root, enabled)?;
    let state = app.state::<RuntimeState>();
    state.enabled.store(enabled, Ordering::Release);
    if !enabled {
        app.state::<crate::local_intelligence::LocalIntelligence>()
            .shutdown();
        crate::local_intelligence::assistant::clear_pending(&app)?;
    }
    let mut status = state
        .status
        .lock()
        .map_err(|_| "Runtime state unavailable")?
        .clone();
    status.enabled = enabled;
    state.update(&app, status.clone());
    Ok(status)
}
