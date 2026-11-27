use ovt_core::config::SessionMode;
use tauri::menu::{Menu, MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "ovt-tray";

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, false)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("oh-my-voice-type")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| handle_menu(app, event.id().0.as_str()));
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }
    builder.build(app)?;
    update(app);
    Ok(())
}

pub fn update(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let active = crate::dictation::is_active();
    let pending = app
        .try_state::<crate::commands::AppState>()
        .and_then(|state| state.pending.as_ref().and_then(|repo| repo.list().ok()))
        .map_or(0, |tasks| tasks.len());
    let locale = app
        .try_state::<crate::commands::AppState>()
        .and_then(|state| state.config.snapshot().ok())
        .map_or_else(|| "en".to_string(), |settings| settings.ui.locale);
    let zh = locale == "zh-CN";
    let tooltip = if active {
        if zh {
            "正在录音或处理…".to_string()
        } else {
            "Recording or processing…".to_string()
        }
    } else if pending > 0 {
        if zh {
            format!("oh-my-voice-type · {pending} 个待恢复任务")
        } else {
            format!("oh-my-voice-type · {pending} interrupted sessions")
        }
    } else {
        "oh-my-voice-type".to_string()
    };
    let _ = tray.set_tooltip(Some(tooltip));
    if let Ok(menu) = build_menu(app, active) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn build_menu(app: &AppHandle, active: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let locale = app
        .try_state::<crate::commands::AppState>()
        .and_then(|state| state.config.snapshot().ok())
        .map_or_else(|| "en".to_string(), |settings| settings.ui.locale);
    let zh = locale == "zh-CN";
    let updater = crate::updater::status();
    let update_label = match (updater.status, updater.update.as_ref()) {
        (crate::updater::UpdaterStatus::Ready, _) => {
            if zh {
                "重启以完成更新".to_string()
            } else {
                "Restart to finish update".to_string()
            }
        }
        (crate::updater::UpdaterStatus::Available, Some(info)) => {
            if zh {
                format!("安装更新 {}", info.version)
            } else {
                format!("Install update {}", info.version)
            }
        }
        (
            crate::updater::UpdaterStatus::Downloading | crate::updater::UpdaterStatus::Installing,
            _,
        ) => {
            if zh {
                "正在更新…".to_string()
            } else {
                "Updating…".to_string()
            }
        }
        _ => {
            if zh {
                "检查更新".to_string()
            } else {
                "Check for updates".to_string()
            }
        }
    };
    let item = |id: &'static str, en_text: &'static str, zh_text: &'static str, enabled| {
        MenuItem::with_id(
            app,
            id,
            if zh { zh_text } else { en_text },
            enabled,
            None::<&str>,
        )
    };
    MenuBuilder::new(app)
        .item(&item(
            "show",
            "Open oh-my-voice-type",
            "打开 oh-my-voice-type",
            true,
        )?)
        .separator()
        .item(&item("dictate", "Dictate", "听写", !active)?)
        .item(&item("translate", "Translate", "翻译", !active)?)
        .item(&item("edit", "Edit selection", "编辑选区", !active)?)
        .item(&item("stop", "Stop and transcribe", "停止并转写", active)?)
        .separator()
        .item(&item(
            "recovery",
            "Interrupted sessions",
            "中断的会话",
            true,
        )?)
        .item(&item("settings", "Settings", "设置", true)?)
        .item(&MenuItem::with_id(
            app,
            "update",
            update_label,
            true,
            None::<&str>,
        )?)
        .separator()
        .item(&item("quit", "Quit", "退出", true)?)
        .build()
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "show" => show_main(app),
        "dictate" => spawn_start(app, SessionMode::Dictate),
        "translate" => spawn_start(app, SessionMode::Translate),
        "edit" => spawn_start(app, SessionMode::EditSelection),
        "stop" => {
            let _ = crate::dictation::request_stop(app);
        }
        "recovery" => navigate(app, "history"),
        "settings" => navigate(app, "settings"),
        "update" => handle_update(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn handle_update(app: &AppHandle) {
    use crate::updater::UpdaterStatus;
    let status = crate::updater::status().status;
    navigate_to(app, "settings", Some("about"));
    match status {
        UpdaterStatus::Ready => {
            if let Err(error) = crate::updater::restart(app) {
                let _ = app.emit("session:error", serde_json::json!({ "message": error }));
            }
        }
        // The settings page shows the release notes and the install button.
        UpdaterStatus::Available
        | UpdaterStatus::Checking
        | UpdaterStatus::Downloading
        | UpdaterStatus::Installing => {}
        _ => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::updater::check(&app, crate::updater::CheckKind::Manual).await;
            });
        }
    }
}

fn spawn_start(app: &AppHandle, mode: SessionMode) {
    let app = app.clone();
    // `start` is synchronous and blocks (keychain, UI Automation); running it
    // inside an async task panicked. Keep it off the main thread as well so
    // the tray menu stays responsive.
    tauri::async_runtime::spawn_blocking(move || {
        let target_language = app
            .try_state::<crate::commands::AppState>()
            .and_then(|state| state.config.snapshot().ok())
            .map(|settings| settings.dictation.translate_target);
        if let Err(error) = crate::dictation::start_from_own_ui(
            &app,
            crate::dictation::SessionRequest {
                mode,
                target_language,
                source: crate::dictation::SessionSource::Tray,
            },
        ) {
            let _ = app.emit(
                "session:error",
                serde_json::json!({ "message": error.to_string() }),
            );
        }
    });
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn navigate(app: &AppHandle, tab: &str) {
    navigate_to(app, tab, None);
}

fn navigate_to(app: &AppHandle, tab: &str, section: Option<&str>) {
    show_main(app);
    let _ = app.emit(
        "app:navigate",
        serde_json::json!({ "tab": tab, "section": section }),
    );
}
