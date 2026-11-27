use ovt_core::config::SessionMode;
use std::sync::Mutex;
use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

const TRAY_ID: &str = "ovt-tray";

/// The tray menu is built once and its items are updated in place. Building a
/// new menu and swapping it in with `set_menu` on every state change (from
/// worker threads) left a menu whose clicks never reached the handler on
/// Windows: "Stop and transcribe" did nothing during a recording.
struct TrayItems {
    show: MenuItem<Wry>,
    dictate: MenuItem<Wry>,
    translate: MenuItem<Wry>,
    edit: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    recovery: MenuItem<Wry>,
    settings: MenuItem<Wry>,
    update: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

static ITEMS: Mutex<Option<TrayItems>> = Mutex::new(None);

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &'static str| MenuItem::with_id(app, id, id, true, None::<&str>);
    let items = TrayItems {
        show: item("show")?,
        dictate: item("dictate")?,
        translate: item("translate")?,
        edit: item("edit")?,
        stop: item("stop")?,
        recovery: item("recovery")?,
        settings: item("settings")?,
        update: item("update")?,
        quit: item("quit")?,
    };
    let menu = MenuBuilder::new(app)
        .item(&items.show)
        .separator()
        .item(&items.dictate)
        .item(&items.translate)
        .item(&items.edit)
        .item(&items.stop)
        .separator()
        .item(&items.recovery)
        .item(&items.settings)
        .item(&items.update)
        .separator()
        .item(&items.quit)
        .build()?;
    *ITEMS.lock().unwrap_or_else(|error| error.into_inner()) = Some(items);
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
    let zh = app
        .try_state::<crate::commands::AppState>()
        .and_then(|state| state.config.snapshot().ok())
        .is_some_and(|settings| settings.ui.locale == "zh-CN");
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

    let guard = ITEMS.lock().unwrap_or_else(|error| error.into_inner());
    let Some(items) = guard.as_ref() else {
        return;
    };
    let label = |en: &str, zh_text: &str| {
        if zh {
            zh_text.to_string()
        } else {
            en.to_string()
        }
    };
    let set = |item: &MenuItem<Wry>, text: String, enabled: bool| {
        let _ = item.set_text(text);
        let _ = item.set_enabled(enabled);
    };
    set(
        &items.show,
        label("Open oh-my-voice-type", "打开 oh-my-voice-type"),
        true,
    );
    set(&items.dictate, label("Dictate", "听写"), !active);
    set(&items.translate, label("Translate", "翻译"), !active);
    set(&items.edit, label("Edit selection", "编辑选区"), !active);
    set(
        &items.stop,
        label("Stop and transcribe", "停止并转写"),
        active,
    );
    set(
        &items.recovery,
        label("Interrupted sessions", "中断的会话"),
        true,
    );
    set(&items.settings, label("Settings", "设置"), true);
    set(&items.update, update_label(zh), true);
    set(&items.quit, label("Quit", "退出"), true);
}

fn update_label(zh: bool) -> String {
    let updater = crate::updater::status();
    let (en, zh_text) = match (updater.status, updater.update.as_ref()) {
        (crate::updater::UpdaterStatus::Ready, _) => (
            "Restart to finish update".to_string(),
            "重启以完成更新".to_string(),
        ),
        (crate::updater::UpdaterStatus::Available, Some(info)) => (
            format!("Install update {}", info.version),
            format!("安装更新 {}", info.version),
        ),
        (
            crate::updater::UpdaterStatus::Downloading | crate::updater::UpdaterStatus::Installing,
            _,
        ) => ("Updating…".to_string(), "正在更新…".to_string()),
        _ => ("Check for updates".to_string(), "检查更新".to_string()),
    };
    if zh {
        zh_text
    } else {
        en
    }
}

fn handle_menu(app: &AppHandle, id: &str) {
    tracing::info!(menu = id, "tray menu item selected");
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
            crate::dictation::report_start_error(&app, &error.to_string());
        }
    });
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Starting from the main window minimizes it (focus goes back to the
        // target app); show() alone leaves a minimized window minimized.
        let _ = window.unminimize();
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
