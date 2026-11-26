//! Framework-independent batch dictation, translation and selection-edit
//! pipeline. The host owns recording and delivery; this engine owns ASR,
//! prompting, history and timing.

use crate::config::Settings;
use crate::dictionary::Dictionary;
use crate::error::{Error, Result};
use crate::history::{HistoryEntry, HistoryRepo};
use crate::prompt::{self, Mode, PromptContext, EMPTY_SENTINEL};
use crate::providers::{AsrProvider, CleanupRequest, LlmProvider, TranscribeRequest};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationInput {
    /// 16-bit PCM mono WAV bytes.
    pub wav: Vec<u8>,
    pub sample_rate: u32,
    pub mode: Mode,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub caret_text: Option<String>,
    pub selection: Option<String>,
    pub target_language: Option<String>,
    /// Relative path in the app audio directory, when retention is enabled.
    pub audio_path: Option<String>,
    pub source_pending_id: Option<String>,
}

impl DictationInput {
    pub fn new(wav: Vec<u8>, mode: Mode) -> Self {
        Self {
            wav,
            sample_rate: crate::wav::TARGET_SAMPLE_RATE,
            mode,
            app_name: None,
            window_title: None,
            caret_text: None,
            selection: None,
            target_language: None,
            audio_path: None,
            source_pending_id: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiTimings {
    pub asr_ms: Option<u64>,
    pub cleanup_ms: Option<u64>,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct AsrVocabulary {
    pub hotwords: Vec<String>,
    pub overflow: Vec<String>,
    pub context: Option<String>,
}

impl AsrVocabulary {
    pub fn pack(
        asr: &(dyn AsrProvider + '_),
        dictionary: &Dictionary,
        ctx: &PromptContext,
    ) -> Self {
        let caps = asr.capabilities();
        let (hotwords, overflow) = caps.hotwords.apply(dictionary.ranked_terms());
        let mut lines: Vec<String> = Vec::new();
        if let (Some(app), Some(title)) = (&ctx.app_name, &ctx.window_title) {
            if title.is_empty() {
                lines.push(format!("App: {app}"));
            } else {
                lines.push(format!("App: {app} — {title}"));
            }
        } else if let Some(app) = &ctx.app_name {
            lines.push(format!("App: {app}"));
        }
        if let Some(style) = &ctx.app_style {
            lines.push(format!("Writing style: {style}"));
        }
        if !overflow.is_empty() {
            lines.push(format!(
                "Vocabulary (use these exact spellings): {}",
                overflow.join(", ")
            ));
        }
        if let Some(caret) = &ctx.caret_text {
            if !caret.trim().is_empty() {
                lines.push(format!("Text near the caret:\n{}", caret.trim()));
            }
        }
        let mut context = lines.join("\n");
        if let Some(max) = caps.max_context_chars {
            if context.chars().count() > max {
                let truncated: String = context.chars().take(max.saturating_sub(1)).collect();
                context = format!("{truncated}…");
            }
        }
        Self {
            hotwords,
            overflow,
            context: if context.is_empty() {
                None
            } else {
                Some(context)
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DictationOutcome {
    pub id: String,
    pub raw_text: String,
    pub final_text: String,
    pub mode: String,
    pub timings: UiTimings,
    pub inserted: bool,
    pub warnings: Vec<String>,
}

pub struct DictationEngine {
    pub asr: Arc<dyn AsrProvider>,
    pub llm: Option<Arc<dyn LlmProvider>>,
    pub settings: Settings,
    pub dictionary: Dictionary,
    pub history: Option<Arc<HistoryRepo>>,
}

impl DictationEngine {
    pub fn new(
        asr: Arc<dyn AsrProvider>,
        llm: Option<Arc<dyn LlmProvider>>,
        settings: Settings,
        dictionary: Dictionary,
        history: Option<Arc<HistoryRepo>>,
    ) -> Self {
        Self {
            asr,
            llm,
            settings,
            dictionary,
            history,
        }
    }

    pub async fn process(
        &self,
        input: DictationInput,
        insert: impl FnOnce(&str) -> Result<()>,
    ) -> Result<DictationOutcome> {
        self.process_observed(input, insert, |_| {}).await
    }

    /// Same pipeline with host-visible stage boundaries for durable queue
    /// transitions. Stage names are `rewriting` and `needs_delivery`.
    pub async fn process_observed(
        &self,
        input: DictationInput,
        insert: impl FnOnce(&str) -> Result<()>,
        on_stage: impl Fn(&'static str),
    ) -> Result<DictationOutcome> {
        let started = std::time::Instant::now();
        let id = Uuid::new_v4().to_string();
        let vocab = AsrVocabulary::pack(
            self.asr.as_ref(),
            &self.dictionary,
            &self.prompt_context(&input),
        );
        let language = self
            .asr
            .capabilities()
            .resolve_language(self.settings.dictation.language.as_deref());
        let asr_start = std::time::Instant::now();
        debug!(
            ?input.mode,
            provider = self.asr.name(),
            bytes = input.wav.len(),
            hotwords = vocab.hotwords.len(),
            overflow = vocab.overflow.len(),
            "asr start"
        );
        let transcript = self
            .asr
            .transcribe(TranscribeRequest {
                audio: input.wav.clone(),
                language,
                hotwords: vocab.hotwords.clone(),
                context: vocab.context.clone(),
            })
            .await?;
        let asr_ms = asr_start.elapsed().as_millis() as u64;
        self.finish(
            id,
            transcript.text.trim().to_string(),
            input,
            started,
            Some(asr_ms),
            insert,
            on_stage,
        )
        .await
    }

    /// Re-run cleanup/translation from a saved transcript without ASR or
    /// insertion side effects. Used by history and pending recovery.
    pub async fn rewrite_raw(
        &self,
        id: String,
        raw_text: String,
        input: DictationInput,
        insert: impl FnOnce(&str) -> Result<()>,
    ) -> Result<DictationOutcome> {
        self.finish(
            id,
            raw_text.trim().to_string(),
            input,
            std::time::Instant::now(),
            None,
            insert,
            |_| {},
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish(
        &self,
        id: String,
        raw_text: String,
        input: DictationInput,
        started: std::time::Instant,
        asr_ms: Option<u64>,
        insert: impl FnOnce(&str) -> Result<()>,
        on_stage: impl Fn(&'static str),
    ) -> Result<DictationOutcome> {
        let mode_label = input.mode.as_str().to_string();
        if raw_text.is_empty() || raw_text == EMPTY_SENTINEL {
            return Ok(DictationOutcome {
                id,
                raw_text,
                final_text: String::new(),
                mode: mode_label,
                timings: UiTimings {
                    asr_ms,
                    cleanup_ms: None,
                    total_ms: started.elapsed().as_millis() as u64,
                },
                inserted: false,
                warnings: vec!["Nothing heard — try again.".into()],
            });
        }

        let ctx = self.prompt_context(&input);
        let cleanup_start = std::time::Instant::now();
        let (system, user_prefix) = match input.mode {
            Mode::Translate => {
                let target = input
                    .target_language
                    .clone()
                    .unwrap_or_else(|| self.settings.dictation.translate_target.clone());
                prompt::build_translate_prompt(&target, &ctx)
            }
            Mode::EditSelection => prompt::build_edit_prompt(input.app_name.as_deref(), &ctx),
            Mode::Dictate => prompt::build_cleanup_prompt(&self.settings, &ctx),
        };
        let mut user = user_prefix;
        user.push_str(&raw_text);

        let requires_llm = input.mode != Mode::Dictate;
        let llm = if requires_llm {
            Some(self.llm.as_ref().ok_or_else(|| {
                Error::Config(format!(
                    "{} requires a configured LLM before recording",
                    mode_label
                ))
            })?)
        } else if self.settings.dictation.cleanup_enabled {
            self.llm.as_ref()
        } else {
            None
        };

        let mut warnings: Vec<String> = Vec::new();
        if llm.is_some() {
            on_stage("rewriting");
        }
        let mut cleanup_ms = None;
        let mut final_text = match llm {
            Some(llm) => {
                let result = llm
                    .complete(CleanupRequest {
                        system,
                        user,
                        max_tokens: self.settings.cleanup.max_tokens,
                        temperature: self.settings.cleanup.temperature,
                    })
                    .await;
                cleanup_ms = Some(cleanup_start.elapsed().as_millis() as u64);
                match result {
                    Ok(text) if !text.trim().is_empty() && text.trim() != EMPTY_SENTINEL => {
                        text.trim().to_string()
                    }
                    Ok(_) if requires_llm => {
                        return Err(Error::ProviderResponse(
                            "LLM returned an empty rewrite; source text was not inserted".into(),
                        ));
                    }
                    Ok(_) => raw_text.clone(),
                    Err(error) if requires_llm => return Err(error),
                    Err(error) => {
                        warn!(error = %error, "dictation cleanup failed; using raw transcript");
                        warnings.push(format!(
                            "AI cleanup unavailable ({error}); inserted raw transcript"
                        ));
                        raw_text.clone()
                    }
                }
            }
            None if input.mode == Mode::Dictate && self.settings.dictation.cleanup_enabled => {
                warnings.push(
                    "AI cleanup is off (no cleanup provider available) — inserted the raw transcript."
                        .into(),
                );
                raw_text.clone()
            }
            None => raw_text.clone(),
        };
        if final_text.is_empty() {
            final_text = raw_text.clone();
        }

        on_stage("needs_delivery");
        let inserted = match insert(&final_text) {
            Ok(()) => true,
            Err(error) => {
                warn!(error = %error, "insertion failed");
                warnings.push(format!("Could not insert text: {error}"));
                false
            }
        };
        let total_ms = started.elapsed().as_millis() as u64;
        if let Some(history) = &self.history {
            let entry = HistoryEntry {
                id: id.clone(),
                created_at_ms: unix_now_ms(),
                mode: mode_label.clone(),
                app_name: input.app_name.clone(),
                raw_text: raw_text.clone(),
                final_text: final_text.clone(),
                asr_ms,
                cleanup_ms,
                insert_ok: inserted,
                pinned: false,
                target_language: input.target_language.clone(),
                audio_path: input.audio_path.clone(),
                delivery_status: if inserted { "inserted" } else { "failed" }.into(),
                warnings_json: serde_json::to_string(&warnings).unwrap_or_else(|_| "[]".into()),
                source_pending_id: input.source_pending_id.clone(),
            };
            if let Err(error) = history.insert(&entry) {
                warn!(error = %error, "history save failed");
                warnings.push("History could not save this dictation.".into());
            }
        }
        Ok(DictationOutcome {
            id,
            raw_text,
            final_text,
            mode: mode_label,
            timings: UiTimings {
                asr_ms,
                cleanup_ms,
                total_ms,
            },
            inserted,
            warnings,
        })
    }

    fn prompt_context(&self, input: &DictationInput) -> PromptContext {
        PromptContext {
            app_name: if self.settings.context.include_app_context {
                input.app_name.clone()
            } else {
                None
            },
            window_title: if self.settings.context.include_app_context {
                input.window_title.clone()
            } else {
                None
            },
            caret_text: if self.settings.context.include_caret_context {
                input.caret_text.clone().map(|text| {
                    let max = self.settings.context.max_caret_chars;
                    text.chars()
                        .rev()
                        .take(max)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                })
            } else {
                None
            },
            selection: input.selection.clone(),
            app_style: self
                .settings
                .dictation
                .tone_for_app(input.app_name.as_deref()),
            dictionary: self.dictionary.prompt_terms(),
            target_language: input.target_language.clone(),
            preserve_wording: self.settings.dictation.preserve_wording,
            no_list_formatting: self.settings.dictation.no_list_formatting,
        }
    }
}

pub fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn prepare_audio(samples: &[f32], in_rate: u32) -> (Vec<u8>, f32) {
    let (trimmed, _) = crate::wav::trim_silence(samples.to_vec(), in_rate, -45.0);
    let resampled = if in_rate != crate::wav::TARGET_SAMPLE_RATE {
        crate::wav::resample(&trimmed, in_rate, crate::wav::TARGET_SAMPLE_RATE)
    } else {
        trimmed
    };
    let peak = crate::wav::peak_dbfs(&resampled);
    (
        crate::wav::encode_wav_mono(&resampled, crate::wav::TARGET_SAMPLE_RATE),
        peak,
    )
}

pub fn ensure_has_audio(peak_dbfs: f32) -> Result<()> {
    if peak_dbfs <= -90.0 {
        return Err(Error::Audio(
            "No sound from the microphone — check the device or permission.".into(),
        ));
    }
    Ok(())
}
