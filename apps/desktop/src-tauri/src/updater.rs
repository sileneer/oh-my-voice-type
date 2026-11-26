use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdaterStatus {
    Unconfigured,
    Idle,
    Checking,
    Available,
    Downloading,
    Ready,
    Error,
    UpToDate,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub pub_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdaterSnapshot {
    pub status: UpdaterStatus,
    pub current_version: String,
    pub update: Option<UpdateInfo>,
    pub progress: Option<f64>,
    pub message: Option<String>,
}

struct PendingUpdate {
    update: Arc<Update>,
    info: UpdateInfo,
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
        })
    })
}

fn pending() -> &'static Mutex<Option<PendingUpdate>> {
    static PENDING: OnceLock<Mutex<Option<PendingUpdate>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

pub fn status() -> UpdaterSnapshot {
    state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn publish(app: &AppHandle, snapshot: UpdaterSnapshot) {
    *state().lock().unwrap_or_else(|error| error.into_inner()) = snapshot.clone();
    let _ = app.emit("updater:changed", &snapshot);
}

pub async fn check(app: &AppHandle) -> UpdaterSnapshot {
    publish(
        app,
        UpdaterSnapshot {
            status: UpdaterStatus::Checking,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            update: None,
            progress: None,
            message: None,
        },
    );
    let result = async {
        let updater = app
            .updater_builder()
            .build()
            .map_err(|error| error.to_string())?;
        updater.check().await.map_err(|error| error.to_string())
    }
    .await;
    let snapshot = match result {
        Ok(Some(update)) => {
            let info = UpdateInfo {
                version: update.version.clone(),
                notes: update.body.clone().unwrap_or_default(),
                pub_date: update.date.map(|date| date.to_string()),
            };
            *pending().lock().unwrap_or_else(|error| error.into_inner()) = Some(PendingUpdate {
                update: Arc::new(update),
                info: info.clone(),
            });
            UpdaterSnapshot {
                status: UpdaterStatus::Available,
                current_version: env!("CARGO_PKG_VERSION").to_string(),
                update: Some(info),
                progress: None,
                message: None,
            }
        }
        Ok(None) => UpdaterSnapshot {
            status: UpdaterStatus::UpToDate,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            update: None,
            progress: None,
            message: None,
        },
        Err(message) => UpdaterSnapshot {
            status: UpdaterStatus::Error,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            update: None,
            progress: None,
            message: Some(message),
        },
    };
    publish(app, snapshot.clone());
    snapshot
}

pub async fn install(app: &AppHandle) -> Result<(), String> {
    let pending = pending()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .map(|pending| (pending.update.clone(), pending.info.clone()))
        .ok_or_else(|| "No signed update is ready to install.".to_string())?;
    let app_for_progress = app.clone();
    let info = pending.1.clone();
    let info_for_progress = info.clone();
    let info_for_finish = info.clone();
    let mut downloaded = 0u64;
    let result = pending
        .0
        .download_and_install(
            move |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                let progress = total
                    .filter(|value| *value > 0)
                    .map(|value| downloaded as f64 / value as f64);
                publish(
                    &app_for_progress,
                    UpdaterSnapshot {
                        status: UpdaterStatus::Downloading,
                        current_version: env!("CARGO_PKG_VERSION").to_string(),
                        update: Some(info_for_progress.clone()),
                        progress,
                        message: None,
                    },
                );
            },
            || {
                publish(
                    app,
                    UpdaterSnapshot {
                        status: UpdaterStatus::Ready,
                        current_version: env!("CARGO_PKG_VERSION").to_string(),
                        update: Some(info_for_finish.clone()),
                        progress: Some(1.0),
                        message: None,
                    },
                );
            },
        )
        .await
        .map_err(|error| error.to_string());
    if let Err(message) = result {
        publish(
            app,
            UpdaterSnapshot {
                status: UpdaterStatus::Error,
                current_version: env!("CARGO_PKG_VERSION").to_string(),
                update: Some(info),
                progress: None,
                message: Some(message.clone()),
            },
        );
        return Err(message);
    }
    Ok(())
}
