use crate::audio::AudioHandle;
use crate::config_store::{ConfigStore, ProfileRole};
use crate::dictation;
use crate::keys::{CredentialStore, SystemCredentialStore};
use crate::platform::{self, HotkeyHandle};
use ovt_core::config::{AsrProfile, LlmProfile, Settings};
use ovt_core::history::HistoryFilter;
use ovt_core::providers::{CleanupRequest, TranscribeRequest};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub config: Arc<ConfigStore>,
    pub history: Option<Arc<ovt_core::history::HistoryRepo>>,
    pub pending: Option<Arc<crate::pending::PendingRepo>>,
    pub dictionary: Arc<crate::personalization::DictionaryStore>,
    pub hotkey: std::sync::Mutex<Vec<Box<dyn HotkeyHandle + Send + Sync>>>,
    pub inserters: platform::Inserters,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestMode {
    Check,
    Request,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileTestResult {
    pub ok: bool,
    pub verified: bool,
    pub message: String,
    pub elapsed_ms: u128,
}

pub fn app_data_dir() -> std::path::PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("OVT_DATA_DIR") {
        if !path.trim().is_empty() {
            return std::path::PathBuf::from(path);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ovt")
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state.config.snapshot()
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    let saved = state.config.save_general(settings)?;
    dictation::reload_hotkey(&app).map_err(|e| e.to_string())?;
    if let Some(history) = &state.history {
        if let Ok(paths) =
            history.prune_with_audio(&saved.history, ovt_core::dictation::unix_now_ms())
        {
            for path in paths {
                remove_history_audio(&path);
            }
        }
    }
    let _ = app.emit("settings:changed", saved.clone());
    Ok(saved)
}

#[tauri::command]
pub fn list_mics() -> Vec<String> {
    AudioHandle::list_devices()
}

#[tauri::command]
pub fn permission_status() -> serde_json::Value {
    platform::permission_status()
}

#[tauri::command]
pub fn dictation_toggle(app: AppHandle) -> Result<(), String> {
    dictation::toggle(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn session_start(
    app: AppHandle,
    request: dictation::SessionRequest,
) -> Result<dictation::SessionStarted, String> {
    dictation::start(&app, request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn session_cancel(app: AppHandle) {
    dictation::cancel(&app);
}

#[tauri::command]
pub fn dictation_cancel(app: AppHandle) {
    dictation::cancel(&app);
}

/// The pill's own auto-hide and close button. `getCurrentWindow().hide()` is a
/// no-op on Windows for a window shown via `show_inactive`.
#[tauri::command]
pub fn pill_hide(app: AppHandle) {
    dictation::pill_hide(&app);
}

/// Tell every window (sidebar badge, home banner, settings count) and the tray
/// that the pending queue changed; deletes made from the UI never did.
fn notify_pending_changed(app: &AppHandle, state: &AppState) {
    let count = state
        .pending
        .as_ref()
        .and_then(|pending| pending.list().ok())
        .map_or(0, |tasks| tasks.len());
    let _ = app.emit("pending:changed", serde_json::json!({ "count": count }));
    crate::tray::update(app);
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn history_list(
    state: State<'_, AppState>,
    limit: u32,
    offset: u32,
    query: Option<String>,
    mode: Option<String>,
    delivery: Option<String>,
    pinned: Option<bool>,
    from_ms: Option<u64>,
    to_ms: Option<u64>,
) -> Result<serde_json::Value, String> {
    let Some(repo) = &state.history else {
        return Ok(serde_json::json!({"entries": [], "total": 0}));
    };
    let page = repo
        .list_filtered(&HistoryFilter {
            query,
            mode,
            delivery,
            pinned,
            from_ms,
            to_ms,
            limit,
            offset,
        })
        .map_err(|e| e.to_string())?;
    serde_json::to_value(page).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let audio = state
        .history
        .as_ref()
        .ok_or("history disabled")?
        .delete_with_audio(&id)
        .map_err(|e| e.to_string())?;
    if let Some(path) = audio {
        remove_history_audio(&path);
    }
    Ok(())
}

#[tauri::command]
pub fn history_delete_all(state: State<'_, AppState>) -> Result<(), String> {
    let paths = state
        .history
        .as_ref()
        .ok_or("history disabled")?
        .delete_all()
        .map_err(|e| e.to_string())?;
    for path in paths {
        remove_history_audio(&path);
    }
    Ok(())
}

#[tauri::command]
pub fn history_set_pinned(
    state: State<'_, AppState>,
    id: String,
    pinned: bool,
) -> Result<(), String> {
    state
        .history
        .as_ref()
        .ok_or("history disabled")?
        .set_pinned(&id, pinned)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingRecord {
    pub id: String,
    pub created_at_ms: u64,
    pub mode: ovt_core::config::SessionMode,
    pub target_language: Option<String>,
    pub stage: String,
    pub attempts: u32,
    pub raw_text: String,
    pub final_text: String,
    pub error: Option<String>,
    pub has_audio: bool,
    pub delivery_uncertain: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingCleanupResult {
    pub deleted_count: usize,
    pub freed_bytes: u64,
}

fn pending_record(task: ovt_core::pending::PendingTask) -> PendingRecord {
    let has_audio = crate::pending::resolve_audio_path(&app_data_dir(), &task.audio_path)
        .map(|path| path.is_file())
        .unwrap_or(false);
    PendingRecord {
        id: task.id,
        created_at_ms: task.created_at_ms,
        mode: task.mode,
        target_language: task.target_language,
        stage: task.status.as_str().to_string(),
        attempts: task.attempts,
        raw_text: task.raw_text,
        final_text: task.final_text,
        error: task.error,
        has_audio,
        delivery_uncertain: task.status == ovt_core::pending::PendingStatus::NeedsReview,
    }
}

#[tauri::command]
pub fn updater_status() -> crate::updater::UpdaterSnapshot {
    crate::updater::status()
}

#[tauri::command]
pub async fn updater_check(app: AppHandle) -> Result<crate::updater::UpdaterSnapshot, String> {
    Ok(crate::updater::check(&app, crate::updater::CheckKind::Manual).await)
}

#[tauri::command]
pub async fn updater_install(app: AppHandle) -> Result<(), String> {
    crate::updater::install(&app).await
}

#[tauri::command]
pub fn updater_restart(app: AppHandle) -> Result<(), String> {
    crate::updater::restart(&app)
}

#[tauri::command]
pub async fn history_reprocess(
    app: AppHandle,
    id: String,
    mode: Option<String>,
    target_language: Option<String>,
) -> Result<ovt_core::history::HistoryEntry, String> {
    dictation::reprocess_history(&app, id, mode, target_language)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_export_audio(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let entry = state
        .history
        .as_ref()
        .ok_or("history disabled")?
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or("history entry not found")?;
    let relative = entry
        .audio_path
        .ok_or("this history entry has no retained audio")?;
    let source = crate::pending::resolve_history_audio(&app_data_dir(), &relative)
        .map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&source).map_err(|e| format!("read retained audio: {e}"))?;
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("WAV audio", &["wav"])
        .set_file_name(format!("{id}.wav"))
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let destination = file.into_path().map_err(|e| e.to_string())?;
    std::fs::write(destination, bytes).map_err(|e| format!("export audio: {e}"))?;
    Ok(true)
}

#[tauri::command]
pub fn history_add_to_dictionary(
    state: State<'_, AppState>,
    id: String,
    term: String,
) -> Result<ovt_core::dictionary::Dictionary, String> {
    let entry = state
        .history
        .as_ref()
        .ok_or("history disabled")?
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or("history entry not found")?;
    if !entry.final_text.contains(&term) && !entry.raw_text.contains(&term) {
        return Err("the term does not occur in this history entry".into());
    }
    state.dictionary.add(&term).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn dictionary_list(state: State<'_, AppState>) -> Vec<ovt_core::dictionary::DictEntry> {
    state.dictionary.snapshot().entries
}

#[tauri::command]
pub fn dictionary_add(
    state: State<'_, AppState>,
    term: String,
) -> Result<Vec<ovt_core::dictionary::DictEntry>, String> {
    state
        .dictionary
        .add(&term)
        .map(|dictionary| dictionary.entries)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn dictionary_rename(
    state: State<'_, AppState>,
    old_term: String,
    new_term: String,
) -> Result<Vec<ovt_core::dictionary::DictEntry>, String> {
    state
        .dictionary
        .rename(&old_term, &new_term)
        .map(|dictionary| dictionary.entries)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn dictionary_remove(
    state: State<'_, AppState>,
    term: String,
) -> Result<Vec<ovt_core::dictionary::DictEntry>, String> {
    state
        .dictionary
        .delete(&term)
        .map(|dictionary| dictionary.entries)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pending_list(state: State<'_, AppState>) -> Result<Vec<PendingRecord>, String> {
    state
        .pending
        .as_ref()
        .ok_or("pending queue disabled")?
        .list()
        .map(|tasks| tasks.into_iter().map(pending_record).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pending_stats(state: State<'_, AppState>) -> Result<crate::pending::PendingStats, String> {
    let pending = state.pending.as_ref().ok_or("pending queue disabled")?;
    let tasks = pending.list().map_err(|e| e.to_string())?;
    let mut bytes = 0u64;
    for task in &tasks {
        if let Ok(path) = crate::pending::resolve_audio_path(&app_data_dir(), &task.audio_path) {
            bytes = bytes.saturating_add(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0));
        }
    }
    Ok(crate::pending::PendingStats {
        count: tasks.len(),
        bytes,
    })
}

#[tauri::command]
pub async fn pending_retry(app: AppHandle, id: String) -> Result<PendingRecord, String> {
    dictation::retry_pending(&app, id)
        .await
        .map(pending_record)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pending_copy(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let task = state
        .pending
        .as_ref()
        .ok_or("pending queue disabled")?
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or("pending task not found")?;
    let text = if task.final_text.is_empty() {
        task.raw_text
    } else {
        task.final_text
    };
    if text.is_empty() {
        return Err("this task has no processed text to copy".into());
    }
    state
        .inserters
        .inserter
        .copy(&text)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pending_discard(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let deleted = state
        .pending
        .as_ref()
        .ok_or("pending queue disabled")?
        .delete(&id)
        .map_err(|e| e.to_string());
    // Refresh even when the row was already gone, so a stale count clears.
    notify_pending_changed(&app, &state);
    let task = deleted?.ok_or("pending task not found")?;
    if let Ok(path) = crate::pending::resolve_audio_path(&app_data_dir(), &task.audio_path) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[tauri::command]
pub fn pending_cleanup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PendingCleanupResult, String> {
    let result = pending_cleanup_inner(&state);
    notify_pending_changed(&app, &state);
    result
}

fn pending_cleanup_inner(state: &AppState) -> Result<PendingCleanupResult, String> {
    let pending = state.pending.as_ref().ok_or("pending queue disabled")?;
    let tasks = pending.list().map_err(|e| e.to_string())?;
    let deleted_count = tasks.len();
    let mut freed_bytes = 0u64;
    for task in tasks {
        pending.delete(&task.id).map_err(|e| e.to_string())?;
        if let Ok(path) = crate::pending::resolve_audio_path(&app_data_dir(), &task.audio_path) {
            if let Ok(metadata) = std::fs::metadata(&path) {
                freed_bytes = freed_bytes.saturating_add(metadata.len());
            }
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(PendingCleanupResult {
        deleted_count,
        freed_bytes,
    })
}

#[tauri::command]
pub fn copy_text(state: State<'_, AppState>, text: String) -> Result<(), String> {
    state
        .inserters
        .inserter
        .copy(&text)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn api_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    serde_json::to_value(state.config.status()?).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_asr_profile(
    state: State<'_, AppState>,
    profile: AsrProfile,
    api_key: Option<String>,
    confirm_host_change: bool,
) -> Result<Settings, String> {
    state
        .config
        .save_asr_profile(profile, api_key, confirm_host_change)
}

#[tauri::command]
pub fn save_llm_profile(
    state: State<'_, AppState>,
    profile: LlmProfile,
    api_key: Option<String>,
    confirm_host_change: bool,
) -> Result<Settings, String> {
    state
        .config
        .save_llm_profile(profile, api_key, confirm_host_change)
}

#[tauri::command]
pub fn delete_profile(
    state: State<'_, AppState>,
    role: ProfileRole,
    id: String,
) -> Result<Settings, String> {
    state.config.delete_profile(role, id)
}

#[tauri::command]
pub fn set_active_profile(
    state: State<'_, AppState>,
    role: ProfileRole,
    id: String,
) -> Result<Settings, String> {
    state.config.set_active_profile(role, id)
}

#[tauri::command]
pub fn export_config(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("oh-my-voice-type config", &["json"])
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let path = file.into_path().map_err(|e| e.to_string())?;
    state.config.export_to(&path)?;
    Ok(true)
}

#[tauri::command]
pub fn import_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Settings>, String> {
    use tauri_plugin_dialog::DialogExt;
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("oh-my-voice-type config", &["json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|e| e.to_string())?;
    state.config.import_from(&path).map(Some)
}

#[tauri::command]
pub async fn test_profile(
    state: State<'_, AppState>,
    role: ProfileRole,
    id: String,
    mode: TestMode,
) -> Result<ProfileTestResult, String> {
    let started = Instant::now();
    let settings = state.config.snapshot()?;
    let (base_url, credential_id, asr, llm) = match role {
        ProfileRole::Asr => {
            let profile = settings
                .asr_profiles
                .iter()
                .find(|profile| profile.id == id)
                .ok_or("ASR profile not found")?;
            (
                profile
                    .resolved_base_url()
                    .ok_or("ASR endpoint is not configured")?,
                profile
                    .credential_id
                    .clone()
                    .ok_or("profile has no API key")?,
                Some(profile.clone()),
                None,
            )
        }
        ProfileRole::Llm => {
            let profile = settings
                .llm_profiles
                .iter()
                .find(|profile| profile.id == id)
                .ok_or("LLM profile not found")?;
            (
                profile
                    .resolved_base_url()
                    .ok_or("LLM endpoint is not configured")?,
                profile
                    .credential_id
                    .clone()
                    .ok_or("profile has no API key")?,
                None,
                Some(profile.clone()),
            )
        }
    };
    let vault: Arc<dyn CredentialStore> = Arc::new(SystemCredentialStore);
    let key = vault
        .load(&credential_id)
        .map_err(|e| e.to_string())?
        .ok_or("bound API key is missing")?;

    let result: Result<ProfileTestResult, String> = match mode {
        TestMode::Check => {
            let primary = ovt_core::providers::http_client();
            let direct = ovt_core::providers::direct_http_client();
            let response = ovt_core::providers::send_with_proxy_fallback(
                &primary,
                &direct,
                "endpoint check",
                |http| Ok(http.get(&base_url)),
            )
            .await
            .map_err(|e| format!("endpoint is not reachable: {e}"))?;
            Ok(ProfileTestResult {
                ok: true,
                verified: false,
                message: format!(
                    "endpoint reachable (HTTP {}); key not verified",
                    response.status().as_u16()
                ),
                elapsed_ms: started.elapsed().as_millis(),
            })
        }
        TestMode::Request => {
            let result = tokio::time::timeout(std::time::Duration::from_secs(45), async {
                if let Some(profile) = asr {
                    let provider = dictation::build_asr(&profile, &key)
                        .ok_or("ASR profile is not configured")?;
                    let samples = vec![0.0; ovt_core::wav::TARGET_SAMPLE_RATE as usize];
                    let audio = ovt_core::wav::encode_wav_16k_mono(
                        &samples,
                        ovt_core::wav::TARGET_SAMPLE_RATE,
                    )
                    .map_err(|e| e.to_string())?;
                    provider
                        .transcribe(TranscribeRequest {
                            audio,
                            ..TranscribeRequest::default()
                        })
                        .await
                        .map(|_| "ASR test request accepted".to_string())
                        .map_err(|e| e.to_string())
                } else if let Some(profile) = llm {
                    let provider = dictation::build_llm(&profile, &key)
                        .ok_or("LLM profile is not configured")?;
                    provider
                        .complete(CleanupRequest {
                            system: "Connection test only.".into(),
                            user: "Reply with OK.".into(),
                            max_tokens: 16,
                            temperature: 0.2,
                        })
                        .await
                        .map(|_| "LLM test request accepted".to_string())
                        .map_err(|e| e.to_string())
                } else {
                    Err("profile not found".to_string())
                }
            })
            .await
            .map_err(|_| "test request timed out".to_string())??;
            Ok(ProfileTestResult {
                ok: true,
                verified: true,
                message: result,
                elapsed_ms: started.elapsed().as_millis(),
            })
        }
    };
    result.map_err(|message| {
        let state = if message.contains("authentication failed") {
            "authentication failed"
        } else if message.contains("rate limited") {
            "rate limited"
        } else {
            "test failed"
        };
        format!("{state}: {message}")
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListModelsArgs {
    pub role: ProfileRole,
    pub protocol: ovt_core::providers::models::ModelListProtocol,
    pub base_url: Option<String>,
    #[serde(default)]
    pub allow_insecure_http: bool,
    #[serde(default)]
    pub allow_unfiltered_fallback: bool,
    pub api_key: Option<String>,
    pub profile_id: Option<String>,
}

fn url_host(url: &str) -> Option<String> {
    reqwest::Url::parse(url.trim())
        .ok()?
        .host_str()
        .map(|h| h.to_ascii_lowercase())
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    args: ListModelsArgs,
) -> Result<ovt_core::providers::models::ModelListResponse, String> {
    use ovt_core::providers::models::{fetch_models, ModelListRequest, ModelListRole};

    let settings = state.config.snapshot()?;
    let saved_profile_info = args
        .profile_id
        .as_deref()
        .and_then(|profile_id| match args.role {
            ProfileRole::Asr => settings
                .asr_profiles
                .iter()
                .find(|item| item.id == profile_id)
                .map(|p| (p.resolved_base_url(), p.credential_id.clone())),
            ProfileRole::Llm => settings
                .llm_profiles
                .iter()
                .find(|item| item.id == profile_id)
                .map(|p| (p.resolved_base_url(), p.credential_id.clone())),
        });

    let resolved_url = args
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| saved_profile_info.as_ref().and_then(|(url, _)| url.clone()))
        .ok_or_else(|| "endpoint base URL is required to fetch models".to_string())?;

    let key = if let Some(explicit) = args
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        explicit.to_string()
    } else if args.profile_id.is_some() {
        let (saved_url, credential_id) = saved_profile_info.ok_or("profile not found")?;
        let saved_host = saved_url.as_deref().and_then(url_host);
        let target_host = url_host(&resolved_url);
        if saved_host != target_host {
            return Err("enter an API key or save the new host before fetching models".into());
        }
        let credential_id = credential_id.ok_or("enter an API key to fetch available models")?;
        let vault: Arc<dyn CredentialStore> = Arc::new(SystemCredentialStore);
        vault
            .load(&credential_id)
            .map_err(|e| e.to_string())?
            .ok_or("bound API key is missing")?
    } else {
        return Err("enter an API key to fetch available models".into());
    };

    let request = ModelListRequest {
        role: match args.role {
            ProfileRole::Asr => ModelListRole::Asr,
            ProfileRole::Llm => ModelListRole::Llm,
        },
        protocol: args.protocol,
        base_url: resolved_url,
        allow_insecure_http: args.allow_insecure_http,
        allow_unfiltered_fallback: args.allow_unfiltered_fallback,
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fetch_models(&request, &key),
    )
    .await
    .map_err(|_| "fetching models timed out".to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_settings_folder() -> Result<(), String> {
    let path = app_data_dir().display().to_string();
    open_path(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "invalid URL".to_string())?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err("only https URLs can be opened externally".into());
    }
    use tauri_plugin_shell::ShellExt;
    #[allow(deprecated)]
    app.shell()
        .open(parsed.as_str(), None)
        .map_err(|e| e.to_string())
}

fn remove_history_audio(relative: &str) {
    if let Ok(path) = crate::pending::resolve_history_audio(&app_data_dir(), relative) {
        let _ = std::fs::remove_file(path);
    }
}

fn open_path(path: &str) -> ovt_core::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| ovt_core::Error::Unsupported(e.to_string()))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| ovt_core::Error::Unsupported(e.to_string()))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
        Err(ovt_core::Error::Unsupported("file manager".into()))
    }
}

pub fn bootstrap(app: &AppHandle) -> ovt_core::Result<()> {
    let dir = app_data_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| ovt_core::Error::Config(format!("app data dir: {e}")))?;
    let config = Arc::new(
        ConfigStore::load(dir.join("settings.json"), Arc::new(SystemCredentialStore))
            .map_err(ovt_core::Error::Config)?,
    );
    let settings = config.snapshot().map_err(ovt_core::Error::Config)?;
    let history = match ovt_core::history::HistoryRepo::open(&dir.join("history.db")) {
        Ok(repo) => {
            let repo = Arc::new(repo);
            match repo.prune_with_audio(&settings.history, ovt_core::dictation::unix_now_ms()) {
                Ok(paths) => {
                    for path in paths {
                        remove_history_audio(&path);
                    }
                }
                Err(error) => eprintln!("history retention failed: {error}"),
            }
            Some(repo)
        }
        Err(error) => {
            eprintln!("history unavailable: {error}");
            None
        }
    };
    let pending = match crate::pending::PendingRepo::open(&dir.join("history.db")) {
        Ok(repo) => {
            let repo = Arc::new(repo);
            let now = ovt_core::dictation::unix_now_ms();
            if let Err(error) = repo.recover_interrupted(now) {
                eprintln!("pending recovery failed: {error}");
            }
            if let Ok(expired) = repo.expired(now, settings.history.pending_retention_days) {
                for task in expired {
                    let _ = repo.delete(&task.id);
                    if let Ok(path) = crate::pending::resolve_audio_path(&dir, &task.audio_path) {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
            Some(repo)
        }
        Err(error) => {
            eprintln!("pending queue unavailable: {error}");
            None
        }
    };
    let dictionary = Arc::new(
        crate::personalization::DictionaryStore::load(dir.join("dictionary.json"))
            .map_err(|error| ovt_core::Error::Config(error.to_string()))?,
    );
    app.manage(AppState {
        config,
        history,
        pending,
        dictionary,
        hotkey: std::sync::Mutex::new(Vec::new()),
        inserters: platform::Inserters::new(),
    });
    Ok(())
}
