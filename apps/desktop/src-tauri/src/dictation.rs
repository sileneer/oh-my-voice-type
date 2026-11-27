//! Cancellable Tokio session host. OS hotkey callbacks only enqueue work; one
//! active session owns capture, ASR/LLM, verified delivery and pending recovery.

use crate::audio::CaptureInfo;
use crate::keys::CredentialStore;
use crate::platform::{self, TargetInfo};
use crate::{audio, commands, keys};
use ovt_core::config::{AsrProfile, LlmProfile, SessionMode, Settings};
use ovt_core::dictation::{DictationEngine, DictationInput};
use ovt_core::history::HistoryEntry;
use ovt_core::pending::{PendingStatus, PendingTask};
use ovt_core::prompt::Mode;
use ovt_core::providers::anthropic::AnthropicLlm;
use ovt_core::providers::openai::{OpenAiCompatibleAsr, OpenAiCompatibleLlm};
use ovt_core::providers::openai_responses::OpenAiResponsesLlm;
use ovt_core::providers::u2::U2Asr;
use ovt_core::providers::{AsrProvider, LlmProvider};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

type SessionProviders = (Arc<dyn AsrProvider>, Option<Arc<dyn LlmProvider>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    Hotkey,
    Ui,
    Tray,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionRequest {
    pub mode: SessionMode,
    #[serde(default)]
    pub target_language: Option<String>,
    pub source: SessionSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionStarted {
    pub session_id: String,
    pub mode: SessionMode,
    pub pending_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Starting,
    Recording,
    Processing,
}

#[derive(Debug, Clone)]
struct SessionMeta {
    id: String,
    mode: SessionMode,
    pending_id: String,
    started: Instant,
}

impl SessionMeta {
    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn payload(&self, state: &str) -> serde_json::Value {
        serde_json::json!({
            "state": state,
            "session_id": self.id,
            "mode": self.mode,
            "elapsed_ms": self.elapsed_ms(),
            "pending_id": self.pending_id,
        })
    }
}

struct ActiveSession {
    meta: SessionMeta,
    phase: Phase,
    token: CancellationToken,
    handle: tauri::async_runtime::JoinHandle<()>,
    commands: mpsc::UnboundedSender<RecordingCmd>,
}

enum RecordingCmd {
    Stop,
    Cancel,
}

static ACTIVE: LazyLock<Mutex<Option<ActiveSession>>> = LazyLock::new(|| Mutex::new(None));

fn lock_active() -> std::sync::MutexGuard<'static, Option<ActiveSession>> {
    ACTIVE.lock().unwrap_or_else(|error| error.into_inner())
}

pub fn init_session(_app: &AppHandle) {
    platform::set_session_active(false);
}

pub fn is_active() -> bool {
    lock_active().is_some()
}

fn current_phase() -> Option<Phase> {
    lock_active().as_ref().map(|session| session.phase)
}

pub fn start(app: &AppHandle, request: SessionRequest) -> ovt_core::Result<SessionStarted> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let pending_id = uuid::Uuid::new_v4().to_string();
    let meta = SessionMeta {
        id: session_id.clone(),
        mode: request.mode,
        pending_id: pending_id.clone(),
        started: Instant::now(),
    };
    if lock_active().is_some() {
        return Err(ovt_core::Error::Config(
            "another voice session is already active".into(),
        ));
    }
    let state = app.state::<commands::AppState>();
    let settings = state.config.snapshot().map_err(ovt_core::Error::Config)?;
    let target_language = if request.mode == SessionMode::Translate {
        Some(
            request
                .target_language
                .clone()
                .filter(|language| !language.trim().is_empty())
                .unwrap_or_else(|| settings.dictation.translate_target.clone()),
        )
    } else {
        None
    };
    let providers =
        build_session_providers(&settings, request.mode).map_err(ovt_core::Error::Config)?;
    let target = if request.mode == SessionMode::EditSelection {
        state.inserters.context.capture_selection()?
    } else {
        state.inserters.context.capture()?
    };
    if platform::is_own_target(&target) {
        return Err(ovt_core::Error::Insert(
            "focus the destination app before starting a session; text is never inserted into oh-my-voice-type itself"
                .into(),
        ));
    }
    if request.mode == SessionMode::EditSelection {
        validate_edit_target(&target)?;
    }
    let pending = state
        .pending
        .clone()
        .ok_or_else(|| ovt_core::Error::Db("durable pending queue is unavailable".into()))?;
    let now = ovt_core::dictation::unix_now_ms();
    let mut task = PendingTask::new(pending_id.clone(), session_id.clone(), request.mode);
    task.status = PendingStatus::Capturing;
    task.created_at_ms = now;
    task.updated_at_ms = now;
    task.audio_path = format!("pending-audio/{pending_id}.wav");
    task.target_language = target_language.clone();
    task.app_name = target.app_name.clone();
    task.window_title = target.window_title.clone();
    task.target_id = target.stable_target_id.clone();
    task.selection_revision = target.selection_revision.clone();
    task.selection_hash = target.selection_hash.clone();
    task.selection = target.selection.clone();
    task.secure_field = target.secure_field;
    pending.save(&task)?;
    notify_pending(app);

    let token = CancellationToken::new();
    let (tx, rx) = mpsc::unbounded_channel();
    let app_for_task = app.clone();
    let meta_for_task = meta.clone();
    let token_for_task = token.clone();
    {
        // Register the session and spawn its task under one lock. The task can
        // fail (and call `finish_active`) immediately, so it must never run
        // before it is registered, and two concurrent starts must not both
        // pass the `is_some` check. Never block on the task here: hotkey and
        // tray starts used to run inside the async runtime, where the old
        // `blocking_recv` panicked after the capture had already started.
        let mut active = lock_active();
        if active.is_some() {
            drop(active);
            let _ = pending.delete(&pending_id);
            notify_pending(app);
            return Err(ovt_core::Error::Config(
                "another voice session is already active".into(),
            ));
        }
        // Emitted before the task exists so "recording" can never overtake it.
        emit_state(app, "starting", &meta, None);
        let handle = tauri::async_runtime::spawn(async move {
            run_session(
                app_for_task,
                rx,
                meta_for_task,
                request.mode,
                target_language,
                target,
                settings,
                providers,
                token_for_task,
            )
            .await;
        });
        *active = Some(ActiveSession {
            meta: meta.clone(),
            phase: Phase::Starting,
            token,
            handle,
            commands: tx,
        });
        platform::set_session_active(true);
    }
    let area = platform::target_work_area();
    pill_show(app, area);
    update_tray(app);
    Ok(SessionStarted {
        session_id,
        mode: request.mode,
        pending_id,
    })
}

fn validate_edit_target(target: &TargetInfo) -> ovt_core::Result<()> {
    if target.secure_field {
        return Err(ovt_core::Error::Insert(
            "the focused field is secure; selection editing is blocked".into(),
        ));
    }
    if !target.selection_supported {
        return Err(ovt_core::Error::Unsupported(
            "this control does not expose a supported text selection".into(),
        ));
    }
    if target
        .selection
        .as_deref()
        .is_none_or(|selection| selection.is_empty())
    {
        return Err(ovt_core::Error::Insert(
            "select non-empty text before starting selection edit".into(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_session(
    app: AppHandle,
    mut commands: mpsc::UnboundedReceiver<RecordingCmd>,
    meta: SessionMeta,
    mode: SessionMode,
    target_language: Option<String>,
    target: TargetInfo,
    settings: Settings,
    providers: SessionProviders,
    token: CancellationToken,
) {
    let result = run_session_inner(
        &app,
        &mut commands,
        &meta,
        mode,
        target_language,
        &target,
        &settings,
        providers,
        &token,
    )
    .await;
    if let Err(error) = result {
        if token.is_cancelled() {
            cancel_task(&app, &meta);
        } else {
            fail_task(&app, &meta, &error.to_string());
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_session_inner(
    app: &AppHandle,
    commands: &mut mpsc::UnboundedReceiver<RecordingCmd>,
    meta: &SessionMeta,
    mode: SessionMode,
    target_language: Option<String>,
    target: &TargetInfo,
    settings: &Settings,
    providers: SessionProviders,
    token: &CancellationToken,
) -> ovt_core::Result<()> {
    let state = app.state::<commands::AppState>();
    let pending = state
        .pending
        .as_ref()
        .ok_or_else(|| ovt_core::Error::Db("durable pending queue is unavailable".into()))?;
    let audio_path = commands::app_data_dir()
        .join("pending-audio")
        .join(format!("{}.wav", meta.pending_id));
    let capture: CaptureInfo = state
        .inserters
        .audio
        .start_pending(settings.audio.mic_device.as_deref(), audio_path)?;
    let now = ovt_core::dictation::unix_now_ms();
    if let Some(mut task) = pending.get(&meta.pending_id)? {
        task.sample_rate = capture.sample_rate;
        task.updated_at_ms = now;
        pending.save(&task)?;
    }
    set_phase(meta, Phase::Recording);
    if settings.audio.prewarm {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            _ = token.cancelled() => { cancel_task(app, meta); return Ok(()); }
        }
    }
    let mut mute_guard = if settings.audio.mute_output_while_dictating {
        match platform::mute_output() {
            Ok(guard) => Some(guard),
            Err(error) => {
                emit_warning(app, meta, &format!("Output mute unavailable: {error}"));
                None
            }
        }
    } else {
        None
    };
    emit_state(app, "recording", meta, None);
    let recording_started = Instant::now();
    let mut silence_since = Instant::now();
    let mut silence_warned = false;
    let mut duration_warned = false;
    let mut stop = false;
    while !stop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                let level = state.inserters.audio.level();
                emit_level(app, meta, level);
                if level <= 0.003 {
                    if silence_since.elapsed() >= Duration::from_secs(3) && !silence_warned {
                        silence_warned = true;
                        emit_warning(app, meta, "No microphone activity for 3 seconds — check the input device.");
                    }
                } else {
                    silence_since = Instant::now();
                    silence_warned = false;
                }
                let elapsed = recording_started.elapsed().as_secs();
                if elapsed >= 480 && !duration_warned {
                    duration_warned = true;
                    emit_warning(app, meta, "Recording is nearing the configured time limit.");
                }
                if elapsed >= u64::from(settings.audio.max_recording_seconds) {
                    stop = true;
                }
            }
            _ = token.cancelled() => {
                cancel_task(app, meta);
                return Ok(());
            }
            command = commands.recv() => match command {
                Some(RecordingCmd::Stop) => stop = true,
                Some(RecordingCmd::Cancel) | None => {
                    cancel_task(app, meta);
                    return Ok(());
                }
            }
        }
    }
    let (samples, rate) = state.inserters.audio.stop();
    if let Some(error) = state.inserters.audio.pending_error() {
        return Err(ovt_core::Error::Audio(error));
    }
    let now = ovt_core::dictation::unix_now_ms();
    let mut task = pending
        .get(&meta.pending_id)?
        .ok_or_else(|| ovt_core::Error::Db("pending task disappeared".into()))?;
    task.frame_count = state.inserters.audio.pending_frames();
    task.sample_rate = rate;
    task.updated_at_ms = now;
    if !task.transition(PendingStatus::Queued, now) {
        return Err(ovt_core::Error::Db(
            "invalid pending transition to queued".into(),
        ));
    }
    pending.save(&task)?;
    notify_pending(app);
    set_phase(meta, Phase::Processing);
    emit_state(app, "processing", meta, None);

    let (wav, peak) = audio::to_wav(&samples, rate);
    ovt_core::dictation::ensure_has_audio(peak)?;
    let dictionary = state.dictionary.snapshot();
    let history = state.history.clone();
    let engine = DictationEngine::new(
        providers.0,
        providers.1,
        settings.clone(),
        dictionary,
        history.clone(),
    );
    let audio_path = settings
        .history
        .keep_audio
        .then(|| format!("audio/{}.wav", meta.pending_id));
    let input = DictationInput {
        wav,
        sample_rate: ovt_core::wav::TARGET_SAMPLE_RATE,
        mode: Mode::from(mode),
        app_name: target.app_name.clone(),
        window_title: target.window_title.clone(),
        caret_text: target.caret_text.clone(),
        selection: target.selection.clone(),
        target_language: target_language.clone(),
        audio_path,
        source_pending_id: Some(meta.pending_id.clone()),
    };
    let app_for_insert = app.clone();
    let pending_for_insert = pending.clone();
    let pending_id = meta.pending_id.clone();
    let expected_target = target.clone();
    let token_for_insert = token.clone();
    let insert = move |text: &str| {
        if token_for_insert.is_cancelled() {
            return Err(ovt_core::Error::Cancelled);
        }
        let state = app_for_insert.state::<commands::AppState>();
        let current = state.inserters.context.verify_target(&expected_target)?;
        if platform::is_own_target(&current) {
            return Err(ovt_core::Error::Insert(
                "refusing to deliver into oh-my-voice-type".into(),
            ));
        }
        if mode == SessionMode::EditSelection {
            let selected = state.inserters.context.capture_selection()?;
            validate_edit_target(&selected)?;
            if selected.stable_target_id != expected_target.stable_target_id
                || selected.selection_revision != expected_target.selection_revision
                || selected.selection_hash != expected_target.selection_hash
            {
                return Err(ovt_core::Error::Insert(
                    "the target selection changed before replacement".into(),
                ));
            }
        }
        let now = ovt_core::dictation::unix_now_ms();
        pending_for_insert.transition(&pending_id, PendingStatus::Delivering, now)?;
        notify_pending(&app_for_insert);
        if token_for_insert.is_cancelled() {
            return Err(ovt_core::Error::Cancelled);
        }
        if mode == SessionMode::EditSelection {
            state
                .inserters
                .context
                .replace_selection(&expected_target, text)
        } else {
            state.inserters.inserter.insert(text)
        }
    };
    pending.transition(
        &meta.pending_id,
        PendingStatus::Transcribing,
        ovt_core::dictation::unix_now_ms(),
    )?;
    notify_pending(app);
    let pending_for_stage = pending.clone();
    let app_for_stage = app.clone();
    let pending_id_for_stage = meta.pending_id.clone();
    let process = engine.process_observed(input, insert, move |stage| {
        let status = match stage {
            "rewriting" => PendingStatus::Rewriting,
            "needs_delivery" => PendingStatus::NeedsDelivery,
            _ => return,
        };
        if let Err(error) = pending_for_stage.transition(
            &pending_id_for_stage,
            status,
            ovt_core::dictation::unix_now_ms(),
        ) {
            eprintln!("pending stage transition failed: {error}");
        }
        notify_pending(&app_for_stage);
    });
    let mut process = std::pin::pin!(process);
    let outcome = loop {
        tokio::select! {
            result = &mut process => break result,
            _ = token.cancelled() => {
                cancel_task(app, meta);
                return Ok(());
            }
            command = commands.recv() => match command {
                Some(RecordingCmd::Cancel) | None => {
                    cancel_task(app, meta);
                    return Ok(());
                }
                Some(RecordingCmd::Stop) => {}
            }
        }
    }?;
    if let Some(guard) = mute_guard.as_mut() {
        if let Err(error) = guard.restore() {
            emit_warning(
                app,
                meta,
                &format!("Could not restore output mute: {error}"),
            );
        }
    }
    drop(mute_guard);
    if token.is_cancelled() {
        cancel_task(app, meta);
        return Ok(());
    }
    let mut outcome = outcome;
    let delivery_status = if outcome.inserted {
        pending.transition(
            &meta.pending_id,
            PendingStatus::Completed,
            ovt_core::dictation::unix_now_ms(),
        )?;
        "inserted"
    } else {
        state.inserters.inserter.copy(&outcome.final_text)?;
        pending.transition(
            &meta.pending_id,
            PendingStatus::Delivering,
            ovt_core::dictation::unix_now_ms(),
        )?;
        pending.transition(
            &meta.pending_id,
            PendingStatus::Completed,
            ovt_core::dictation::unix_now_ms(),
        )?;
        outcome.warnings.push(
            "Could not replace at the verified selection — text copied for manual paste.".into(),
        );
        "copied"
    };
    if let Some(history) = &history {
        history.set_delivery(&outcome.id, delivery_status)?;
    }
    finalize_audio(app, &meta.pending_id, settings.history.keep_audio)?;
    pending.delete(&meta.pending_id)?;
    notify_pending(app);
    emit_warnings(app, meta, &outcome.warnings);
    let mut payload = serde_json::to_value(&outcome)
        .map_err(|error| ovt_core::Error::Config(error.to_string()))?;
    payload["session_id"] = serde_json::json!(meta.id);
    payload["mode"] = serde_json::json!(mode);
    payload["elapsed_ms"] = serde_json::json!(meta.elapsed_ms());
    payload["pending_id"] = serde_json::json!(meta.pending_id);
    payload["delivery_status"] = serde_json::json!(delivery_status);
    let _ = app.emit("session:outcome", payload);
    finish_active(app, meta);
    emit_state(app, "done", meta, None);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    if !is_active() {
        pill_hide(app);
    }
    emit_state(app, "idle", meta, None);
    Ok(())
}

fn fail_task(app: &AppHandle, meta: &SessionMeta, message: &str) {
    let state = app.state::<commands::AppState>();
    let _ = state.inserters.audio.stop();
    if let Some(pending) = &state.pending {
        if let Ok(Some(mut task)) = pending.get(&meta.pending_id) {
            let now = ovt_core::dictation::unix_now_ms();
            let next = if task.status == PendingStatus::Delivering {
                PendingStatus::NeedsReview
            } else {
                PendingStatus::Failed
            };
            task.transition(next, now);
            task.error = Some(message.to_string());
            let _ = pending.save(&task);
            let _ = app.emit(
                "pending:changed",
                serde_json::json!({ "pending_id": meta.pending_id }),
            );
        }
    }
    let mut payload = meta.payload("error");
    payload["message"] = serde_json::json!(message);
    let _ = app.emit("session:error", payload);
    let _ = app.emit("session:warning", serde_json::json!({
        "session_id": meta.id, "mode": meta.mode, "pending_id": meta.pending_id, "message": message
    }));
    finish_active(app, meta);
    update_tray(app);
    let app_for_hide = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if !is_active() {
            pill_hide(&app_for_hide);
        }
    });
}

fn cancel_task(app: &AppHandle, meta: &SessionMeta) {
    let state = app.state::<commands::AppState>();
    let _ = state.inserters.audio.stop();
    if let Some(pending) = &state.pending {
        if let Ok(Some(task)) = pending.get(&meta.pending_id) {
            if task.status == PendingStatus::Delivering {
                let _ = pending.transition(
                    &meta.pending_id,
                    PendingStatus::NeedsReview,
                    ovt_core::dictation::unix_now_ms(),
                );
            } else {
                let _ = pending.delete(&meta.pending_id);
                remove_pending_audio(app, &meta.pending_id);
            }
        }
        let _ = app.emit(
            "pending:changed",
            serde_json::json!({ "pending_id": meta.pending_id }),
        );
    }
    finish_active(app, meta);
    let mut payload = meta.payload("idle");
    payload["cancelled"] = serde_json::json!(true);
    let _ = app.emit("session:state", payload);
    pill_hide(app);
    update_tray(app);
}

fn finalize_audio(_app: &AppHandle, pending_id: &str, keep: bool) -> ovt_core::Result<()> {
    let dir = commands::app_data_dir();
    let source = dir.join("pending-audio").join(format!("{pending_id}.wav"));
    if keep {
        let destination_dir = dir.join("audio");
        std::fs::create_dir_all(&destination_dir)
            .map_err(|error| ovt_core::Error::Audio(format!("create audio dir: {error}")))?;
        let destination = destination_dir.join(format!("{pending_id}.wav"));
        std::fs::rename(&source, &destination)
            .map_err(|error| ovt_core::Error::Audio(format!("retain audio: {error}")))?;
    } else if source.exists() {
        std::fs::remove_file(source)
            .map_err(|error| ovt_core::Error::Audio(format!("remove temporary audio: {error}")))?;
    }
    Ok(())
}

fn remove_pending_audio(_app: &AppHandle, pending_id: &str) {
    let path = commands::app_data_dir()
        .join("pending-audio")
        .join(format!("{pending_id}.wav"));
    let _ = std::fs::remove_file(path);
}

fn finish_active(app: &AppHandle, meta: &SessionMeta) {
    let remove = lock_active()
        .as_ref()
        .is_some_and(|session| session.meta.id == meta.id);
    if remove {
        lock_active().take();
    }
    if !is_active() {
        platform::set_session_active(false);
    }
    update_tray(app);
}

fn set_phase(meta: &SessionMeta, phase: Phase) {
    if let Some(session) = lock_active().as_mut() {
        if session.meta.id == meta.id {
            session.phase = phase;
        }
    }
    update_tray_from_meta(meta, phase);
}

fn update_tray_from_meta(meta: &SessionMeta, phase: Phase) {
    // The concrete AppHandle is not needed here; state transitions also call
    // update_tray at completion. Menu recording state is refreshed by callers.
    let _ = (meta, phase);
}

fn emit_state(app: &AppHandle, state: &str, meta: &SessionMeta, message: Option<String>) {
    let mut payload = meta.payload(state);
    if let Some(message) = message {
        payload["message"] = serde_json::json!(message);
    }
    let _ = app.emit("session:state", payload);
    if matches!(
        phase_from_state(state),
        Phase::Recording | Phase::Processing
    ) {
        update_tray(app);
    }
}

fn phase_from_state(state: &str) -> Phase {
    match state {
        "recording" => Phase::Recording,
        "processing" => Phase::Processing,
        _ => Phase::Starting,
    }
}

fn emit_level(app: &AppHandle, meta: &SessionMeta, level: f32) {
    let _ = app.emit(
        "session:level",
        serde_json::json!({
            "session_id": meta.id, "mode": meta.mode, "pending_id": meta.pending_id,
            "elapsed_ms": meta.elapsed_ms(), "level": level,
        }),
    );
}

fn emit_warning(app: &AppHandle, meta: &SessionMeta, message: &str) {
    let _ = app.emit(
        "session:warning",
        serde_json::json!({
            "session_id": meta.id, "mode": meta.mode, "pending_id": meta.pending_id,
            "elapsed_ms": meta.elapsed_ms(), "message": message,
        }),
    );
}

fn emit_warnings(app: &AppHandle, meta: &SessionMeta, warnings: &[String]) {
    for warning in warnings {
        emit_warning(app, meta, warning);
    }
}

fn notify_pending(app: &AppHandle) {
    let count = app
        .state::<commands::AppState>()
        .pending
        .as_ref()
        .and_then(|pending| pending.list().ok())
        .map_or(0, |tasks| tasks.len());
    let _ = app.emit("pending:changed", serde_json::json!({ "count": count }));
    update_tray(app);
}

fn pill_show(app: &AppHandle, area: Option<(i32, i32, i32, i32)>) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        let Some(pill) = app.get_webview_window("pill") else {
            return;
        };
        if let Some((x, y, width, height)) = area {
            let size = pill
                .outer_size()
                .unwrap_or(tauri::PhysicalSize::new(340, 64));
            let left = x + (width - size.width as i32).max(0) / 2;
            let top = y + height - size.height as i32 - 24;
            let _ = pill.set_position(tauri::PhysicalPosition::new(left, top));
        }
        let _ = platform::show_inactive(&pill);
    });
}

fn pill_hide(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(pill) = app.get_webview_window("pill") {
            let _ = pill.hide();
        }
    });
}

pub fn request_stop(_app: &AppHandle) -> ovt_core::Result<()> {
    let mut active = lock_active();
    let Some(session) = active.as_mut() else {
        return Ok(());
    };
    if session.phase == Phase::Processing {
        return Ok(());
    }
    let _ = session.commands.send(RecordingCmd::Stop);
    Ok(())
}

pub fn cancel(app: &AppHandle) {
    let mut active = lock_active();
    let Some(session) = active.as_mut() else {
        return;
    };
    emit_state(app, "cancelling", &session.meta, None);
    session.token.cancel();
    let _ = session.commands.send(RecordingCmd::Cancel);
}

pub fn toggle(app: &AppHandle) -> ovt_core::Result<()> {
    let phase = current_phase();
    match phase {
        Some(Phase::Starting | Phase::Recording) => request_stop(app),
        Some(Phase::Processing) => Err(ovt_core::Error::Config(
            "the current session is still processing".into(),
        )),
        None => {
            let settings = app
                .state::<commands::AppState>()
                .config
                .snapshot()
                .map_err(ovt_core::Error::Config)?;
            start(
                app,
                SessionRequest {
                    mode: settings.dictation.default_mode,
                    target_language: Some(settings.dictation.translate_target),
                    source: SessionSource::Ui,
                },
            )?;
            Ok(())
        }
    }
}

pub fn shutdown() {
    let active = lock_active().take();
    if let Some(session) = active {
        session.token.cancel();
        session.handle.abort();
    }
    platform::set_session_active(false);
}

pub fn install_hotkey(app: &AppHandle) -> ovt_core::Result<()> {
    let state = app.state::<commands::AppState>();
    let settings = state.config.snapshot().unwrap_or_default();
    let mut slot = state.hotkey.lock().unwrap();
    for mut old in slot.drain(..) {
        old.stop();
    }
    let listener = platform::hotkey_listener();
    for (key, mode) in [
        (settings.hotkey.dictate.clone(), SessionMode::Dictate),
        (settings.hotkey.translate.clone(), SessionMode::Translate),
        (
            settings.hotkey.edit_selection.clone(),
            SessionMode::EditSelection,
        ),
    ] {
        if key.trim().is_empty() {
            continue;
        }
        let events = spawn_hotkey_worker(
            app.clone(),
            mode,
            settings.dictation.translate_target.clone(),
        )?;
        let handle = listener.start(
            &key,
            settings.hotkey.smart_gestures,
            Box::new(move |event| {
                let _ = events.send(event);
            }),
        )?;
        slot.push(handle);
    }
    Ok(())
}

/// One ordered worker per activation key. The OS hook callback only enqueues
/// (it must return within the hook timeout), and processing events in order
/// guarantees a quick Stop or chord can never overtake the Start that created
/// the session. `start` blocks (keychain, UI Automation), so it must run on a
/// plain thread rather than an async worker. The thread exits when the hook
/// handle, and with it the sender, is dropped on reinstall.
fn spawn_hotkey_worker(
    app: AppHandle,
    mode: SessionMode,
    target_language: String,
) -> ovt_core::Result<std::sync::mpsc::Sender<ovt_core::hotkey::HotkeyEvent>> {
    use ovt_core::hotkey::HotkeyEvent;
    let (tx, rx) = std::sync::mpsc::channel::<HotkeyEvent>();
    std::thread::Builder::new()
        .name("ovt-hotkey-actions".into())
        .spawn(move || {
            // The session this key started, so its Stop/chord never ends a
            // session that a different key or the UI started.
            let mut owned: Option<String> = None;
            for event in rx {
                match event {
                    HotkeyEvent::Start => {
                        match start(
                            &app,
                            SessionRequest {
                                mode,
                                target_language: Some(target_language.clone()),
                                source: SessionSource::Hotkey,
                            },
                        ) {
                            Ok(started) => owned = Some(started.session_id),
                            Err(error) => {
                                owned = None;
                                let _ = app.emit(
                                    "session:error",
                                    serde_json::json!({ "message": error.to_string() }),
                                );
                            }
                        }
                    }
                    HotkeyEvent::Stop => {
                        if let Some(id) = owned.take() {
                            request_stop_session(&id);
                        }
                    }
                    HotkeyEvent::Cancel => {
                        owned = None;
                        cancel(&app);
                    }
                    // The activation key was part of a chord (e.g. Alt+Tab): the
                    // decoder dropped the take, so drop the session it started;
                    // otherwise it keeps recording with no key left to stop it.
                    HotkeyEvent::PassThrough => {
                        if let Some(id) = owned.take() {
                            cancel_recording_session(&app, &id);
                        }
                    }
                }
            }
        })
        .map_err(|error| {
            ovt_core::Error::Unsupported(format!("cannot start hotkey worker: {error}"))
        })?;
    Ok(tx)
}

fn request_stop_session(session_id: &str) {
    let active = lock_active();
    if let Some(session) = active.as_ref() {
        if session.meta.id == session_id && session.phase != Phase::Processing {
            let _ = session.commands.send(RecordingCmd::Stop);
        }
    }
}

fn cancel_recording_session(app: &AppHandle, session_id: &str) {
    let mut active = lock_active();
    let Some(session) = active.as_mut() else {
        return;
    };
    if session.meta.id != session_id || session.phase == Phase::Processing {
        return;
    }
    emit_state(app, "cancelling", &session.meta, None);
    session.token.cancel();
    let _ = session.commands.send(RecordingCmd::Cancel);
}

pub fn reload_hotkey(app: &AppHandle) -> ovt_core::Result<()> {
    install_hotkey(app)
}

fn build_session_providers(
    settings: &Settings,
    mode: SessionMode,
) -> Result<SessionProviders, String> {
    let asr = settings
        .asr_profiles
        .iter()
        .find(|profile| Some(&profile.id) == settings.active_asr_profile_id.as_ref())
        .ok_or("select an ASR profile in Settings")?;
    let asr_key_id = asr.credential_id.as_deref().ok_or("ASR API key missing")?;
    let vault = keys::SystemCredentialStore;
    let asr_key = vault
        .load(asr_key_id)
        .map_err(|error| error.to_string())?
        .ok_or("ASR API key missing — open Settings and add it")?;
    let asr_provider = build_asr(asr, &asr_key).ok_or("ASR profile is not configured")?;
    let strictly_requires_llm = mode != SessionMode::Dictate;
    if !strictly_requires_llm && !settings.dictation.cleanup_enabled {
        return Ok((asr_provider, None));
    }
    let Some(llm) = settings
        .llm_profiles
        .iter()
        .find(|profile| Some(&profile.id) == settings.active_llm_profile_id.as_ref())
    else {
        if strictly_requires_llm {
            return Err("LLM is required for this mode; select an LLM profile in Settings".into());
        }
        return Ok((asr_provider, None));
    };
    let Some(llm_key_id) = llm.credential_id.as_deref() else {
        if strictly_requires_llm {
            return Err("LLM API key missing — add it in Settings".into());
        }
        return Ok((asr_provider, None));
    };
    let Some(llm_key) = vault.load(llm_key_id).map_err(|error| error.to_string())? else {
        if strictly_requires_llm {
            return Err("bound LLM API key is missing".into());
        }
        return Ok((asr_provider, None));
    };
    let Some(provider) = build_llm(llm, &llm_key) else {
        if strictly_requires_llm {
            return Err("LLM profile is not configured; translation/editing cannot start".into());
        }
        return Ok((asr_provider, None));
    };
    Ok((asr_provider, Some(provider)))
}

pub(crate) fn build_asr(profile: &AsrProfile, key: &str) -> Option<Arc<dyn AsrProvider>> {
    match profile.engine {
        ovt_core::config::AsrEngine::UnisoundU2 => {
            let mut provider = U2Asr::new(key.to_string());
            if let Some(base_url) = profile.resolved_base_url() {
                provider = provider.with_base_url(base_url);
            }
            if let Some(model) = profile.resolved_model() {
                provider = provider.with_model(model);
            }
            Some(Arc::new(provider))
        }
        ovt_core::config::AsrEngine::OpenaiCompatible => {
            let mut provider = OpenAiCompatibleAsr::new(
                profile.resolved_base_url()?,
                key.to_string(),
                profile.resolved_model()?,
            );
            for (key, value) in &profile.extra_options {
                match key.as_str() {
                    "response_format" => {
                        provider.response_format =
                            Some(value.trim().to_string()).filter(|value| !value.is_empty());
                    }
                    "send_temperature" => provider.send_temperature = value != "false",
                    "send_language" => provider.send_language = value != "false",
                    "send_prompt" => provider.send_prompt = value != "false",
                    _ => {}
                }
            }
            Some(Arc::new(provider))
        }
    }
}

pub(crate) fn build_llm(profile: &LlmProfile, key: &str) -> Option<Arc<dyn LlmProvider>> {
    let base = profile.resolved_base_url()?;
    let model = profile.resolved_model()?;
    Some(match profile.format {
        ovt_core::config::LlmApiFormat::OpenAiChat => {
            Arc::new(OpenAiCompatibleLlm::new(base, key.to_string(), model))
        }
        ovt_core::config::LlmApiFormat::OpenAiResponses => {
            Arc::new(OpenAiResponsesLlm::new(base, key.to_string(), model))
        }
        ovt_core::config::LlmApiFormat::AnthropicMessages => {
            Arc::new(AnthropicLlm::new(base, key.to_string(), model))
        }
    })
}

pub async fn retry_pending(app: &AppHandle, id: String) -> ovt_core::Result<PendingTask> {
    if is_active() {
        return Err(ovt_core::Error::Config("another session is active".into()));
    }
    let state = app.state::<commands::AppState>();
    let pending = state
        .pending
        .as_ref()
        .ok_or_else(|| ovt_core::Error::Db("pending unavailable".into()))?;
    let mut task = pending
        .get(&id)?
        .ok_or_else(|| ovt_core::Error::Db("pending task not found".into()))?;
    if !task.status.allows_retry() {
        return Err(ovt_core::Error::Config(
            "this pending task cannot be retried".into(),
        ));
    }
    let settings = state.config.snapshot().map_err(ovt_core::Error::Config)?;
    let providers =
        build_session_providers(&settings, task.mode).map_err(ovt_core::Error::Config)?;
    let audio_path =
        crate::pending::resolve_audio_path(&commands::app_data_dir(), &task.audio_path)?;
    let wav = std::fs::read(&audio_path)
        .map_err(|error| ovt_core::Error::Audio(format!("read pending audio: {error}")))?;
    let now = ovt_core::dictation::unix_now_ms();
    task.attempts = task.attempts.saturating_add(1);
    if !task.transition(PendingStatus::Queued, now) {
        return Err(ovt_core::Error::Db(
            "invalid pending retry transition".into(),
        ));
    }
    pending.save(&task)?;
    if !task.transition(PendingStatus::Transcribing, now) {
        return Err(ovt_core::Error::Db(
            "invalid pending transcription transition".into(),
        ));
    }
    pending.save(&task)?;
    notify_pending(app);
    let engine = DictationEngine::new(
        providers.0,
        providers.1,
        settings.clone(),
        state.dictionary.snapshot(),
        state.history.clone(),
    );
    let input = DictationInput {
        wav,
        sample_rate: task.sample_rate,
        mode: Mode::from(task.mode),
        app_name: task.app_name.clone(),
        window_title: task.window_title.clone(),
        caret_text: None,
        selection: task.selection.clone(),
        target_language: task.target_language.clone(),
        audio_path: settings
            .history
            .keep_audio
            .then(|| format!("audio/{}.wav", task.id)),
        source_pending_id: Some(task.id.clone()),
    };
    let result = if task.raw_text.is_empty() {
        engine
            .process(input, |_| {
                Err(ovt_core::Error::Insert(
                    "recovery never auto-inserts".into(),
                ))
            })
            .await
    } else {
        engine
            .rewrite_raw(
                uuid::Uuid::new_v4().to_string(),
                task.raw_text.clone(),
                input,
                |_| {
                    Err(ovt_core::Error::Insert(
                        "recovery never auto-inserts".into(),
                    ))
                },
            )
            .await
    };
    match result {
        Ok(outcome) => {
            if let Some(history) = &state.history {
                history.set_delivery(&outcome.id, "needs_review")?;
            }
            task.raw_text = outcome.raw_text;
            task.final_text = outcome.final_text;
            task.warnings = outcome.warnings;
            if !task.transition(
                PendingStatus::NeedsDelivery,
                ovt_core::dictation::unix_now_ms(),
            ) {
                return Err(ovt_core::Error::Db(
                    "invalid pending delivery transition".into(),
                ));
            }
            pending.save(&task)?;
            finalize_audio(app, &task.id, settings.history.keep_audio)?;
            pending.delete(&task.id)?;
            notify_pending(app);
            Ok(task)
        }
        Err(error) => {
            task.error = Some(error.to_string());
            task.status = PendingStatus::Failed;
            task.updated_at_ms = ovt_core::dictation::unix_now_ms();
            pending.save(&task)?;
            notify_pending(app);
            Err(error)
        }
    }
}

pub async fn reprocess_history(
    app: &AppHandle,
    id: String,
    requested_mode: Option<String>,
    requested_target: Option<String>,
) -> ovt_core::Result<HistoryEntry> {
    let state = app.state::<commands::AppState>();
    let history = state
        .history
        .as_ref()
        .ok_or_else(|| ovt_core::Error::Db("history unavailable".into()))?;
    let entry = history
        .get(&id)?
        .ok_or_else(|| ovt_core::Error::Db("history entry not found".into()))?;
    let mode = match requested_mode.as_deref() {
        Some(value) => SessionMode::parse(value)
            .ok_or_else(|| ovt_core::Error::Config("unknown history mode".into()))?,
        None => SessionMode::parse(&entry.mode)
            .ok_or_else(|| ovt_core::Error::Config("unknown history mode".into()))?,
    };
    if mode == SessionMode::EditSelection {
        return Err(ovt_core::Error::Unsupported(
            "selection-edit history does not retain enough context to safely reprocess".into(),
        ));
    }
    let settings = state.config.snapshot().map_err(ovt_core::Error::Config)?;
    let providers = build_session_providers(&settings, mode).map_err(ovt_core::Error::Config)?;
    let engine = DictationEngine::new(
        providers.0,
        providers.1,
        settings,
        state.dictionary.snapshot(),
        Some(history.clone()),
    );
    let new_id = uuid::Uuid::new_v4().to_string();
    let input = DictationInput {
        wav: Vec::new(),
        sample_rate: ovt_core::wav::TARGET_SAMPLE_RATE,
        mode: Mode::from(mode),
        app_name: entry.app_name.clone(),
        window_title: None,
        caret_text: None,
        selection: None,
        target_language: requested_target.or(entry.target_language.clone()),
        audio_path: entry.audio_path.clone(),
        source_pending_id: None,
    };
    let outcome = engine
        .rewrite_raw(new_id, entry.raw_text.clone(), input, |_| {
            Err(ovt_core::Error::Insert(
                "reprocessing never auto-inserts".into(),
            ))
        })
        .await?;
    history.set_delivery(&outcome.id, "needs_review")?;
    history
        .get(&outcome.id)?
        .ok_or_else(|| ovt_core::Error::Db("new history entry was not saved".into()))
}

fn update_tray(app: &AppHandle) {
    crate::tray::update(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_asr_passes_model_and_base_url_to_u2() {
        let profile = AsrProfile {
            id: "u2".into(),
            label: "Unisound".into(),
            engine: ovt_core::config::AsrEngine::UnisoundU2,
            base_url: Some("https://maas-api.unisound.com/v1".into()),
            model: Some("u2-asr-pro".into()),
            ..AsrProfile::default()
        };
        let provider = build_asr(&profile, "test-key").expect("provider");
        assert_eq!(provider.name(), "u2-asr");
    }
}
