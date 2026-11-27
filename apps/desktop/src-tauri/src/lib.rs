//! oh-my-voice-type desktop app: Tauri 2 host over `ovt-core`.

mod audio;
mod commands;
mod config_store;
mod dictation;
mod keys;
mod pending;
mod personalization;
mod platform;
mod tray;
mod updater;

use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ovt=info".into()),
        )
        .init();

    let updater = tauri_plugin_updater::Builder::new();
    #[cfg(target_os = "macos")]
    let updater = updater.target("darwin-universal");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(updater.build())
        .setup(|app| {
            use tauri_plugin_dialog::DialogExt;
            let handle = app.handle().clone();
            if let Err(error) = commands::bootstrap(&handle) {
                app.dialog()
                    .message(format!(
                        "Settings could not be loaded. The original file was kept.\n\n{error}"
                    ))
                    .blocking_show();
                return Err(Box::new(error));
            }
            dictation::init_session(&handle);
            updater::init(&handle);
            tray::init(&handle).map_err(|error| format!("tray initialization failed: {error}"))?;
            if let Err(e) = dictation::install_hotkey(&handle) {
                eprintln!("hotkey unavailable: {e}");
                let _ = handle.emit(
                    "session:error",
                    serde_json::json!({
                        "message": format!("Global hotkey unavailable: {e}")
                    }),
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_mics,
            commands::permission_status,
            commands::dictation_toggle,
            commands::session_start,
            commands::session_cancel,
            commands::dictation_cancel,
            commands::history_list,
            commands::history_delete,
            commands::history_delete_all,
            commands::history_set_pinned,
            commands::history_reprocess,
            commands::history_export_audio,
            commands::history_add_to_dictionary,
            commands::dictionary_list,
            commands::dictionary_add,
            commands::dictionary_rename,
            commands::dictionary_remove,
            commands::pending_list,
            commands::pending_stats,
            commands::pending_retry,
            commands::pending_copy,
            commands::pending_discard,
            commands::pending_cleanup,
            commands::copy_text,
            commands::api_status,
            commands::save_asr_profile,
            commands::save_llm_profile,
            commands::delete_profile,
            commands::set_active_profile,
            commands::export_config,
            commands::import_config,
            commands::test_profile,
            commands::list_models,
            commands::updater_status,
            commands::updater_check,
            commands::updater_install,
            commands::updater_restart,
            commands::open_settings_folder,
            commands::open_external_url,
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray = window
                    .app_handle()
                    .try_state::<commands::AppState>()
                    .and_then(|state| state.config.snapshot().ok())
                    .is_none_or(|settings| settings.ui.close_to_tray);
                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, RunEvent::Exit) {
                shutdown(app);
            }
        });
}

pub(crate) fn shutdown(app: &AppHandle) {
    dictation::shutdown();
    if let Some(state) = app.try_state::<commands::AppState>() {
        state.inserters.audio.shutdown();
        let mut hotkeys = state.hotkey.lock().unwrap();
        for mut hotkey in hotkeys.drain(..) {
            hotkey.stop();
        }
    }
}
