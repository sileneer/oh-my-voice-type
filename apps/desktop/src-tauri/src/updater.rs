//! In-app updates backed by GitHub Releases.
//!
//! The release workflow publishes a signed `latest.json` manifest; the updater
//! plugin verifies every download against the public key in `tauri.conf.json`.
//! This module keeps one shared snapshot of the update state so the settings
//! page, the tray menu and background checks always agree.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Delay before the first background check so startup stays fast.
const STARTUP_DELAY: Duration = Duration::from_secs(20);
/// How often a long-running app looks for a new release.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Overrides the manifest URL, for testing a staged `latest.json`.
const ENDPOINT_ENV: &str = "OVT_UPDATER_ENDPOINT";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdaterStatus {
    Idle,
    Checking,
    Available,
    Downloading,
    Installing,
    /// Installed on disk; the app must restart to run the new version (macOS).
    Ready,
    UpToDate,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub pub_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdaterSnapshot {
    pub status: UpdaterStatus,
    pub current_version: String,
    pub update: Option<UpdateInfo>,
    pub progress: Option<f64>,
    pub message: Option<String>,
    pub checked_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    /// Started by the user; failures are shown.
    Manual,
    /// Started by the scheduler; failures are logged and the previous state is kept.
    Background,
}

fn state() -> &'static Mutex<UpdaterSnapshot> {
    static STATE: OnceLock<Mutex<UpdaterSnapshot>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(UpdaterSnapshot {
            status: UpdaterStatus::Idle,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            update: None,
            progress: None,
            message: None,
            checked_at_ms: None,
        })
    })
}

fn pending() -> &'static Mutex<Option<Arc<Update>>> {
    static PENDING: OnceLock<Mutex<Option<Arc<Update>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

/// Serialises checks and installs so two never run at once.
static BUSY: AtomicBool = AtomicBool::new(false);

struct BusyGuard;

impl BusyGuard {
    fn acquire() -> Option<Self> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

pub fn status() -> UpdaterSnapshot {
    state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

/// Applies `change` to the shared snapshot, then notifies the UI and tray.
fn publish(app: &AppHandle, change: impl FnOnce(&mut UpdaterSnapshot)) -> UpdaterSnapshot {
    let (snapshot, status_changed) = {
        let mut guard = state().lock().unwrap_or_else(|error| error.into_inner());
        let before = (guard.status, guard.update.clone());
        change(&mut guard);
        let changed = before != (guard.status, guard.update.clone());
        (guard.clone(), changed)
    };
    let _ = app.emit("updater:changed", &snapshot);
    // Progress ticks only touch the UI; the tray label depends on status and version.
    if status_changed {
        crate::tray::update(app);
    }
    snapshot
}

/// Records the running version and starts the background check schedule.
pub fn init(app: &AppHandle) {
    let version = app.package_info().version.to_string();
    state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .current_version = version;
    spawn_background_checks(app);
}

fn spawn_background_checks(app: &AppHandle) {
    // Development builds keep the placeholder version; checking would only
    // ever offer the latest public release.
    if cfg!(debug_assertions) && std::env::var_os(ENDPOINT_ENV).is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            if auto_check_enabled(&app) {
                check(&app, CheckKind::Background).await;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

fn auto_check_enabled(app: &AppHandle) -> bool {
    app.try_state::<crate::commands::AppState>()
        .and_then(|state| state.config.snapshot().ok())
        .is_none_or(|settings| settings.ui.check_for_updates)
}

fn build_updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder().timeout(Duration::from_secs(60));
    if let Some(endpoint) = std::env::var(ENDPOINT_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let url = endpoint
            .trim()
            .parse()
            .map_err(|error| format!("{ENDPOINT_ENV} is not a valid URL: {error}"))?;
        builder = builder
            .endpoints(vec![url])
            .map_err(|error| error.to_string())?;
    }
    let handle = app.clone();
    builder
        // Windows exits the process to run the installer; release hooks first.
        .on_before_exit(move || {
            crate::shutdown(&handle);
            handle.cleanup_before_exit();
        })
        .build()
        .map_err(|error| error.to_string())
}

fn update_info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone().unwrap_or_default().trim().to_string(),
        // Keep the manifest's RFC 3339 string so the UI can parse it directly.
        pub_date: update
            .raw_json
            .get("pub_date")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }
}

pub async fn check(app: &AppHandle, kind: CheckKind) -> UpdaterSnapshot {
    let current = status();
    // A downloaded update waiting for restart must not be replaced.
    if current.status == UpdaterStatus::Ready {
        return current;
    }
    let Some(_guard) = BusyGuard::acquire() else {
        return current;
    };
    if kind == CheckKind::Manual {
        publish(app, |snapshot| {
            snapshot.status = UpdaterStatus::Checking;
            snapshot.progress = None;
            snapshot.message = None;
        });
    }
    let result = match build_updater(app) {
        Ok(updater) => updater.check().await.map_err(|error| error.to_string()),
        Err(error) => Err(error),
    };
    match result {
        Ok(Some(update)) => {
            let info = update_info(&update);
            let is_new = current.update.as_ref().map(|known| &known.version) != Some(&info.version);
            *pending().lock().unwrap_or_else(|error| error.into_inner()) = Some(Arc::new(update));
            let snapshot = publish(app, |snapshot| {
                snapshot.status = UpdaterStatus::Available;
                snapshot.update = Some(info.clone());
                snapshot.progress = None;
                snapshot.message = None;
                snapshot.checked_at_ms = Some(now_ms());
            });
            if kind == CheckKind::Background && is_new {
                let _ = app.emit("updater:available", &info);
            }
            snapshot
        }
        Ok(None) => {
            *pending().lock().unwrap_or_else(|error| error.into_inner()) = None;
            publish(app, |snapshot| {
                snapshot.status = UpdaterStatus::UpToDate;
                snapshot.update = None;
                snapshot.progress = None;
                snapshot.message = None;
                snapshot.checked_at_ms = Some(now_ms());
            })
        }
        Err(message) => {
            tracing::warn!("update check failed: {message}");
            match kind {
                CheckKind::Manual => publish(app, |snapshot| {
                    snapshot.status = UpdaterStatus::Error;
                    snapshot.progress = None;
                    snapshot.message = Some(friendly_error(&message));
                }),
                CheckKind::Background => current,
            }
        }
    }
}

/// Turns the plugin's most common failures into actionable text.
fn friendly_error(message: &str) -> String {
    if message.contains("valid release JSON") {
        return format!("No published release was found on GitHub yet. ({message})");
    }
    if message.contains("signature") {
        return format!(
            "The downloaded update failed signature verification and was discarded. ({message})"
        );
    }
    message.to_string()
}

pub async fn install(app: &AppHandle) -> Result<(), String> {
    if crate::dictation::is_active() {
        return Err("Finish or cancel the current dictation before installing the update.".into());
    }
    let update = pending()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .ok_or_else(|| "No update is ready to install. Check for updates first.".to_string())?;
    let Some(_guard) = BusyGuard::acquire() else {
        return Err("An update check or install is already running.".into());
    };
    let info = update_info(&update);
    publish(app, |snapshot| {
        snapshot.status = UpdaterStatus::Downloading;
        snapshot.update = Some(info.clone());
        snapshot.progress = Some(0.0);
        snapshot.message = None;
    });

    let app_for_progress = app.clone();
    let app_for_finish = app.clone();
    let mut downloaded = 0u64;
    let mut last_reported = -1.0f64;
    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                let progress = total
                    .filter(|value| *value > 0)
                    .map(|value| (downloaded as f64 / value as f64).min(1.0));
                // Throttle UI events to whole percentage points.
                let bucket = progress.map_or(-1.0, |value| (value * 100.0).floor());
                if bucket != last_reported {
                    last_reported = bucket;
                    publish(&app_for_progress, |snapshot| {
                        snapshot.status = UpdaterStatus::Downloading;
                        snapshot.progress = progress;
                    });
                }
            },
            move || {
                publish(&app_for_finish, |snapshot| {
                    snapshot.status = UpdaterStatus::Installing;
                    snapshot.progress = Some(1.0);
                });
            },
        )
        .await
        .map_err(|error| friendly_error(&error.to_string()));

    // On Windows a successful install never returns: the installer replaces the
    // running app and relaunches it.
    match result {
        Ok(()) => {
            *pending().lock().unwrap_or_else(|error| error.into_inner()) = None;
            publish(app, |snapshot| {
                snapshot.status = UpdaterStatus::Ready;
                snapshot.progress = Some(1.0);
                snapshot.message = None;
            });
            Ok(())
        }
        Err(message) => {
            tracing::warn!("update install failed: {message}");
            publish(app, |snapshot| {
                snapshot.status = UpdaterStatus::Error;
                snapshot.progress = None;
                snapshot.message = Some(message.clone());
            });
            Err(message)
        }
    }
}

/// Restarts into the freshly installed version.
pub fn restart(app: &AppHandle) -> Result<(), String> {
    if status().status != UpdaterStatus::Ready {
        return Err("No installed update is waiting for a restart.".into());
    }
    if crate::dictation::is_active() {
        return Err("Finish or cancel the current dictation before restarting.".into());
    }
    crate::shutdown(app);
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serialises_in_kebab_case() {
        let json = serde_json::to_string(&UpdaterStatus::UpToDate).unwrap();
        assert_eq!(json, "\"up-to-date\"");
        let json = serde_json::to_string(&UpdaterStatus::Installing).unwrap();
        assert_eq!(json, "\"installing\"");
    }

    #[test]
    fn busy_guard_is_exclusive() {
        let first = BusyGuard::acquire().expect("first guard");
        assert!(BusyGuard::acquire().is_none());
        drop(first);
        assert!(BusyGuard::acquire().is_some());
    }

    #[test]
    fn friendly_error_explains_missing_release() {
        let message = friendly_error("Could not fetch a valid release JSON from the remote");
        assert!(message.starts_with("No published release"));
    }
}
