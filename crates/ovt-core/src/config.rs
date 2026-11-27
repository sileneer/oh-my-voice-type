//! User-facing settings model. Serialized to disk by the host (JSON), API
//! keys deliberately *not* part of this file — they live in the OS vault and
//! are injected at request time.

use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SETTINGS_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub asr_profiles: Vec<AsrProfile>,
    pub llm_profiles: Vec<LlmProfile>,
    pub active_asr_profile_id: Option<String>,
    pub active_llm_profile_id: Option<String>,
    pub cleanup: CleanupSettings,
    pub hotkey: HotkeySettings,
    pub dictation: DictationSettings,
    pub context: ContextSettings,
    pub history: HistorySettings,
    pub audio: AudioSettings,
    pub ui: UiSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            asr_profiles: Vec::new(),
            llm_profiles: Vec::new(),
            active_asr_profile_id: None,
            active_llm_profile_id: None,
            cleanup: CleanupSettings::default(),
            hotkey: HotkeySettings::default(),
            dictation: DictationSettings::default(),
            context: ContextSettings::default(),
            history: HistorySettings::default(),
            audio: AudioSettings::default(),
            ui: UiSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    /// Platform default: Right Option on macOS, Right Alt on Windows.
    pub dictate: String,
    /// Smart = hold for push-to-talk, quick tap latches until next press.
    pub smart_gestures: bool,
    /// Rebindable secondary activation, e.g. "right-opt".
    pub dictate_alt: Option<String>,
    /// Translation activation; empty means unbound.
    pub translate: String,
    /// Selection-edit activation; empty means unbound.
    pub edit_selection: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            dictate: default_primary_hotkey().to_string(),
            smart_gestures: true,
            dictate_alt: None,
            translate: String::new(),
            edit_selection: String::new(),
        }
    }
}

#[cfg(target_os = "macos")]
fn default_primary_hotkey() -> &'static str {
    "right-opt"
}
#[cfg(not(target_os = "macos"))]
fn default_primary_hotkey() -> &'static str {
    "right-alt"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    #[default]
    Dictate,
    Translate,
    EditSelection,
}

impl SessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictate => "dictate",
            Self::Translate => "translate",
            Self::EditSelection => "edit_selection",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "dictate" => Self::Dictate,
            "translate" => Self::Translate,
            "edit_selection" => Self::EditSelection,
            _ => return None,
        })
    }
}

impl From<crate::prompt::Mode> for SessionMode {
    fn from(mode: crate::prompt::Mode) -> Self {
        match mode {
            crate::prompt::Mode::Dictate => Self::Dictate,
            crate::prompt::Mode::Translate => Self::Translate,
            crate::prompt::Mode::EditSelection => Self::EditSelection,
        }
    }
}

impl From<SessionMode> for crate::prompt::Mode {
    fn from(mode: SessionMode) -> Self {
        match mode {
            SessionMode::Dictate => crate::prompt::Mode::Dictate,
            SessionMode::Translate => crate::prompt::Mode::Translate,
            SessionMode::EditSelection => crate::prompt::Mode::EditSelection,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationSettings {
    /// Spoken language hint passed to ASR; `None` = auto detect.
    pub language: Option<String>,
    /// Run the LLM cleanup/rewrite stage. Off = insert the raw ASR transcript.
    /// User-controllable; on by default.
    pub cleanup_enabled: bool,
    /// Keep the exact wording as much as possible (minimal cleanup).
    pub preserve_wording: bool,
    /// Cleanup only fills/formatting, never restructures into lists.
    pub no_list_formatting: bool,
    /// Per-app tone hints, keyed by foreground app name (case-insensitive).
    pub app_tones: Vec<AppTone>,
    pub translate_target: String,
    pub default_mode: SessionMode,
    pub default_tone: String,
}

impl Default for DictationSettings {
    fn default() -> Self {
        Self {
            language: None,
            cleanup_enabled: true,
            preserve_wording: false,
            no_list_formatting: false,
            app_tones: Vec::new(),
            translate_target: "English".to_string(),
            default_mode: SessionMode::Dictate,
            default_tone: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppTone {
    /// Substring matched against the foreground app name, e.g. "slack".
    pub app: String,
    /// Free-form style instruction injected into the cleanup prompt.
    pub style: String,
}

impl DictationSettings {
    pub fn tone_for_app(&self, app_name: Option<&str>) -> Option<String> {
        let app_lc = app_name.unwrap_or("").to_lowercase();
        // Trim the rule's app name (" Slack" never matched) and skip rules
        // without a style so they fall through to the default tone.
        self.app_tones
            .iter()
            .find(|tone| {
                let app = tone.app.trim().to_lowercase();
                !app.is_empty() && !tone.style.trim().is_empty() && app_lc.contains(&app)
            })
            .map(|tone| tone.style.as_str())
            .or_else(|| {
                (!self.default_tone.trim().is_empty()).then_some(self.default_tone.as_str())
            })
            .map(str::trim)
            .filter(|style| !style.is_empty())
            .map(str::to_string)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrEngine {
    /// Unisound U2-ASR (async upload → task → poll), our default engine.
    #[default]
    UnisoundU2,
    /// Any OpenAI-compatible `/audio/transcriptions` endpoint (Whisper servers).
    OpenaiCompatible,
}

impl AsrEngine {
    pub fn label(self) -> &'static str {
        match self {
            AsrEngine::UnisoundU2 => "Unisound U2-ASR",
            AsrEngine::OpenaiCompatible => "OpenAI-compatible",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AsrProfile {
    pub id: String,
    pub label: String,
    pub engine: AsrEngine,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub extra_options: Vec<(String, String)>,
    pub preset: Option<String>,
    pub credential_id: Option<String>,
    pub credential_hint: Option<String>,
    pub allow_insecure_http: bool,
}

impl Default for AsrProfile {
    fn default() -> Self {
        Self {
            id: "default-asr".into(),
            label: AsrEngine::default().label().into(),
            engine: AsrEngine::default(),
            base_url: None,
            model: None,
            extra_options: Vec::new(),
            preset: None,
            credential_id: None,
            credential_hint: None,
            allow_insecure_http: false,
        }
    }
}

impl AsrProfile {
    pub fn resolved_base_url(&self) -> Option<String> {
        normalized_base_url(&self.base_url).or_else(|| match self.engine {
            AsrEngine::UnisoundU2 => Some(crate::providers::u2_wire::DEFAULT_BASE_URL.into()),
            AsrEngine::OpenaiCompatible => None,
        })
    }

    pub fn resolved_model(&self) -> Option<String> {
        normalized_model(&self.model).or_else(|| match self.engine {
            AsrEngine::UnisoundU2 => Some(crate::providers::u2_wire::MODEL.into()),
            AsrEngine::OpenaiCompatible => None,
        })
    }

    pub fn is_configured(&self) -> bool {
        self.resolved_base_url().is_some() && self.resolved_model().is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmEngine {
    /// Unisound MaaS text generation (default/recommended).
    #[default]
    Unisound,
    /// Any user-supplied endpoint using one of the supported API formats.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmApiFormat {
    /// OpenAI-compatible Chat Completions (`/chat/completions`).
    #[default]
    OpenAiChat,
    /// OpenAI-compatible Responses API (`/responses`).
    OpenAiResponses,
    /// Anthropic-compatible Messages API (`/v1/messages`).
    AnthropicMessages,
}

impl LlmApiFormat {
    /// Path appended to the resolved base URL.
    pub fn endpoint_suffix(self) -> &'static str {
        match self {
            LlmApiFormat::OpenAiChat => "/chat/completions",
            LlmApiFormat::OpenAiResponses => "/responses",
            LlmApiFormat::AnthropicMessages => "/v1/messages",
        }
    }

    /// Documented Unisound base URL for this protocol.
    pub fn unisound_base_url(self) -> &'static str {
        match self {
            LlmApiFormat::AnthropicMessages => "https://maas-api.unisound.com/anthropic",
            _ => "https://maas-api.unisound.com/v1",
        }
    }
}

/// Unisound's default cleanup model (fast general-purpose).
pub const UNISOUND_DEFAULT_MODEL: &str = "u2-flash";
/// Models offered for the Unisound engine (from the API overview).
pub const UNISOUND_MODELS: &[&str] = &["u2-flash", "u2", "u2-med", "u2-radimed"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CleanupSettings {
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Default for CleanupSettings {
    fn default() -> Self {
        Self {
            max_tokens: 1024,
            temperature: 0.2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LlmProfile {
    pub id: String,
    pub label: String,
    pub engine: LlmEngine,
    pub format: LlmApiFormat,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub preset: Option<String>,
    pub credential_id: Option<String>,
    pub credential_hint: Option<String>,
    pub allow_insecure_http: bool,
}

impl Default for LlmProfile {
    fn default() -> Self {
        Self {
            id: "default-llm".into(),
            label: "Unisound".into(),
            engine: LlmEngine::default(),
            format: LlmApiFormat::default(),
            base_url: None,
            model: None,
            preset: None,
            credential_id: None,
            credential_hint: None,
            allow_insecure_http: false,
        }
    }
}

impl LlmProfile {
    pub fn resolved_base_url(&self) -> Option<String> {
        normalized_base_url(&self.base_url).or_else(|| match self.engine {
            LlmEngine::Unisound => Some(self.format.unisound_base_url().into()),
            LlmEngine::Custom => None,
        })
    }

    pub fn resolved_model(&self) -> Option<String> {
        normalized_model(&self.model).or_else(|| match self.engine {
            LlmEngine::Unisound => Some(UNISOUND_DEFAULT_MODEL.into()),
            LlmEngine::Custom => None,
        })
    }

    pub fn is_configured(&self) -> bool {
        self.resolved_base_url().is_some() && self.resolved_model().is_some()
    }
}

fn normalized_base_url(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
}

fn normalized_model(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextSettings {
    /// Foreground app name / window title attached to cleanup prompts.
    pub include_app_context: bool,
    /// Caret-adjacent text read via AX / UI Automation. Off by default.
    pub include_caret_context: bool,
    /// Max characters of caret context.
    pub max_caret_chars: usize,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            include_app_context: true,
            include_caret_context: false,
            max_caret_chars: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// Retention window in days; `None` keeps forever, `Some(0)` never keeps.
    pub retention_days: Option<u32>,
    /// Keep the WAV recording alongside the transcript (off by default).
    pub keep_audio: bool,
    /// Retention for failed/interrupted pending tasks, in days.
    pub pending_retention_days: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            retention_days: Some(30),
            keep_audio: false,
            pending_retention_days: 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// Capture device name; `None` = system default.
    pub mic_device: Option<String>,
    /// Mute other audio output while dictating.
    pub mute_output_while_dictating: bool,
    /// Warm up the microphone before the session begins.
    pub prewarm: bool,
    /// Hard recording limit in seconds (60–900).
    pub max_recording_seconds: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            mic_device: None,
            mute_output_while_dictating: false,
            prewarm: false,
            max_recording_seconds: 540,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub locale: String,
    pub theme: String,
    /// `None` means a legacy v2 install that should not be forced through onboarding.
    pub onboarding_completed: Option<bool>,
    pub close_to_tray: bool,
    /// Check GitHub Releases for a new version in the background.
    pub check_for_updates: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            locale: "auto".to_string(),
            theme: "system".to_string(),
            onboarding_completed: None,
            close_to_tray: true,
            check_for_updates: true,
        }
    }
}

impl Settings {
    pub fn migrate_legacy(legacy: LegacySettings) -> Self {
        let asr_allow_insecure_http = legacy_http_override(&legacy.asr.base_url);
        let llm_allow_insecure_http = legacy_http_override(&legacy.llm.base_url);
        Self {
            asr_profiles: vec![AsrProfile {
                id: "legacy-asr-profile".into(),
                label: "Legacy ASR".into(),
                engine: legacy.asr.engine,
                base_url: legacy_optional(legacy.asr.base_url),
                model: legacy_optional(legacy.asr.model),
                extra_options: legacy.asr.extra_options,
                preset: (legacy.asr.engine == AsrEngine::UnisoundU2)
                    .then_some("unisound-u2".into()),
                credential_id: Some("legacy-asr-credential".into()),
                credential_hint: None,
                allow_insecure_http: asr_allow_insecure_http,
            }],
            llm_profiles: vec![LlmProfile {
                id: "legacy-llm-profile".into(),
                label: "Legacy LLM".into(),
                engine: legacy.llm.engine,
                format: legacy.llm.format,
                base_url: legacy_optional(legacy.llm.base_url),
                model: legacy_optional(legacy.llm.model),
                preset: (legacy.llm.engine == LlmEngine::Unisound).then_some("unisound".into()),
                credential_id: Some("legacy-llm-credential".into()),
                credential_hint: None,
                allow_insecure_http: llm_allow_insecure_http,
            }],
            active_asr_profile_id: Some("legacy-asr-profile".into()),
            active_llm_profile_id: Some("legacy-llm-profile".into()),
            cleanup: CleanupSettings {
                max_tokens: legacy.llm.max_tokens,
                temperature: legacy.llm.temperature,
            },
            hotkey: legacy.hotkey,
            dictation: legacy.dictation,
            context: legacy.context,
            history: legacy.history,
            audio: legacy.audio,
            ui: legacy.ui,
            ..Self::default()
        }
    }

    /// Structurally validate settings before they are applied. Missing API
    /// config is allowed (provider calls fail with a clear error later), but
    /// malformed URLs and empty models are rejected.
    pub fn validate(&self) -> crate::Result<()> {
        if self.schema_version != SETTINGS_SCHEMA_VERSION {
            return Err(config_error(format!(
                "schema_version must be {SETTINGS_SCHEMA_VERSION}"
            )));
        }

        let mut ids = HashSet::new();
        for profile in &self.asr_profiles {
            validate_id_and_label(&mut ids, &profile.id, &profile.label, "asr profile")?;
        }
        for profile in &self.llm_profiles {
            validate_id_and_label(&mut ids, &profile.id, &profile.label, "llm profile")?;
        }

        if let Some(id) = &self.active_asr_profile_id {
            if !self.asr_profiles.iter().any(|profile| &profile.id == id) {
                return Err(config_error(
                    "active_asr_profile_id must reference an ASR profile",
                ));
            }
        }
        if let Some(id) = &self.active_llm_profile_id {
            if !self.llm_profiles.iter().any(|profile| &profile.id == id) {
                return Err(config_error(
                    "active_llm_profile_id must reference an LLM profile",
                ));
            }
        }

        for profile in &self.asr_profiles {
            if let Some(base_url) = &profile.base_url {
                require_http_url("asr base_url", base_url, profile.allow_insecure_http)?;
            }
            match profile.engine {
                AsrEngine::OpenaiCompatible => {
                    require_explicit(&profile.base_url, "ASR base_url")?;
                    require_explicit(&profile.model, "ASR model")?;
                }
                AsrEngine::UnisoundU2 => {
                    if let Some(model) = &profile.model {
                        let trimmed = model.trim();
                        if !trimmed.is_empty()
                            && !trimmed.starts_with(crate::providers::u2_wire::MODEL)
                        {
                            return Err(config_error("Unisound U2 model must be u2-asr"));
                        }
                    }
                }
            }
        }
        for profile in &self.llm_profiles {
            if let Some(base_url) = &profile.base_url {
                require_http_url("llm base_url", base_url, profile.allow_insecure_http)?;
            }
            if profile.engine == LlmEngine::Custom {
                require_explicit(&profile.base_url, "custom LLM base_url")?;
                require_explicit(&profile.model, "custom LLM model")?;
            }
        }

        if self.cleanup.max_tokens == 0 || self.cleanup.max_tokens > 8192 {
            return Err(config_error(
                "cleanup.max_tokens must be between 1 and 8192",
            ));
        }
        if !(0.0..=2.0).contains(&self.cleanup.temperature) {
            return Err(config_error("cleanup.temperature must be within 0.0..=2.0"));
        }
        if self.dictation.translate_target.trim().is_empty() {
            return Err(config_error("dictation.translate_target must not be empty"));
        }
        validate_style("dictation.default_tone", &self.dictation.default_tone)?;
        let mut app_matches = HashSet::new();
        for tone in &self.dictation.app_tones {
            if tone.app.trim().is_empty() {
                return Err(config_error("application tone app must not be empty"));
            }
            validate_style("application tone style", &tone.style)?;
            let normalized = tone.app.trim().to_lowercase();
            if !app_matches.insert(normalized) {
                return Err(config_error("application tone matches must be unique"));
            }
        }
        if self.context.max_caret_chars > 4000 {
            return Err(config_error(
                "context.max_caret_chars must be between 0 and 4000",
            ));
        }
        if !(60..=900).contains(&self.audio.max_recording_seconds) {
            return Err(config_error(
                "audio.max_recording_seconds must be between 60 and 900",
            ));
        }
        if !(1..=30).contains(&self.history.pending_retention_days) {
            return Err(config_error(
                "history.pending_retention_days must be between 1 and 30",
            ));
        }
        if self.hotkey.dictate.trim().is_empty() {
            return Err(config_error("hotkey.dictate must not be empty"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LegacySettings {
    pub hotkey: HotkeySettings,
    pub dictation: DictationSettings,
    pub asr: LegacyAsrSettings,
    pub llm: LegacyLlmSettings,
    pub context: ContextSettings,
    pub history: HistorySettings,
    pub audio: AudioSettings,
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LegacyAsrSettings {
    pub engine: AsrEngine,
    pub base_url: String,
    pub model: String,
    pub extra_options: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LegacyLlmSettings {
    pub engine: LlmEngine,
    pub format: LlmApiFormat,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Default for LegacyLlmSettings {
    fn default() -> Self {
        Self {
            engine: LlmEngine::default(),
            format: LlmApiFormat::default(),
            base_url: String::new(),
            model: String::new(),
            max_tokens: 1024,
            temperature: 0.2,
        }
    }
}

fn legacy_optional(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn legacy_http_override(value: &str) -> bool {
    Url::parse(value.trim())
        .map(|url| url.scheme() == "http" && !is_loopback_host(&url))
        .unwrap_or(false)
}

fn validate_id_and_label(
    ids: &mut HashSet<String>,
    id: &str,
    label: &str,
    kind: &str,
) -> crate::Result<()> {
    if id.trim().is_empty() {
        return Err(config_error(format!("{kind} id must not be empty")));
    }
    if !ids.insert(id.to_string()) {
        return Err(config_error(format!("duplicate id {id:?}")));
    }
    if label.trim().is_empty() {
        return Err(config_error(format!("{kind} label must not be empty")));
    }
    Ok(())
}

fn require_explicit(value: &Option<String>, field: &str) -> crate::Result<()> {
    if value
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        Ok(())
    } else {
        Err(config_error(format!("{field} is required")))
    }
}

pub(crate) fn require_http_url(
    field: &str,
    value: &str,
    allow_insecure_http: bool,
) -> crate::Result<()> {
    let value = value.trim();
    let invalid = || {
        config_error(format!(
            "{field} must be an absolute http/https URL with a host"
        ))
    };
    let Some((scheme, remainder)) = value.split_once("://") else {
        return Err(invalid());
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(invalid());
    }
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(invalid());
    }
    let url = Url::parse(value).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(config_error(format!(
            "{field} must not contain username/password"
        )));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(config_error(format!(
            "{field} must not contain query/fragment"
        )));
    }
    if url.scheme() == "http" && !allow_insecure_http && !is_loopback_host(&url) {
        return Err(config_error(format!(
            "{field} requires HTTPS, loopback HTTP, or allow_insecure_http"
        )));
    }
    Ok(())
}

fn is_loopback_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(ip) => ip.is_loopback(),
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
            }
        })
}

fn validate_style(field: &str, value: &str) -> crate::Result<()> {
    if value.chars().count() > 500 {
        return Err(config_error(format!(
            "{field} must be at most 500 characters"
        )));
    }
    Ok(())
}

fn config_error(message: impl Into<String>) -> crate::Error {
    crate::Error::Config(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_settings() -> Settings {
        let mut settings = Settings::default();
        settings.asr_profiles.push(AsrProfile {
            id: "asr".into(),
            label: "ASR".into(),
            ..AsrProfile::default()
        });
        settings.llm_profiles.push(LlmProfile {
            id: "llm".into(),
            label: "LLM".into(),
            ..LlmProfile::default()
        });
        settings.active_asr_profile_id = Some("asr".into());
        settings.active_llm_profile_id = Some("llm".into());
        settings
    }

    #[test]
    fn defaults_are_valid() {
        let settings = Settings::default();
        assert_eq!(settings.schema_version, 3);
        assert!(settings.asr_profiles.is_empty());
        assert!(settings.llm_profiles.is_empty());
        assert_eq!(settings.active_asr_profile_id, None);
        assert_eq!(settings.active_llm_profile_id, None);
        assert_eq!(settings.cleanup.max_tokens, 1024);
        assert_eq!(settings.cleanup.temperature, 0.2);
        settings.validate().unwrap();
    }

    #[test]
    fn rounds_trip_json() {
        let settings = configured_settings();
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&settings).unwrap()
        );
        assert!(back.dictation.cleanup_enabled);

        let back: Settings =
            serde_json::from_str(r#"{"mutated_by_future_version": true}"#).unwrap();
        assert!(back.hotkey.smart_gestures);
        assert!(back.dictation.cleanup_enabled);
        assert_eq!(back.schema_version, 3);
    }

    #[test]
    fn canary_secret_is_not_serialized() {
        let mut value = serde_json::to_value(configured_settings()).unwrap();
        value["asr_profiles"][0]["api_key"] = "ovt-canary-secret".into();
        value["llm_profiles"][0]["secret"] = "ovt-canary-secret".into();
        let settings: Settings = serde_json::from_value(value).unwrap();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("ovt-canary-secret"));
        assert!(!json.contains("api_key"));
        assert!(!json.contains("secret"));
    }

    #[test]
    fn resolved_defaults() {
        let asr = AsrProfile::default();
        assert_eq!(asr.engine, AsrEngine::UnisoundU2);
        assert_eq!(
            asr.resolved_base_url().as_deref(),
            Some("https://maas-api.unisound.com")
        );
        assert_eq!(asr.resolved_model().as_deref(), Some("u2-asr"));
        assert!(asr.is_configured());

        let llm = LlmProfile::default();
        assert_eq!(llm.engine, LlmEngine::Unisound);
        assert_eq!(llm.format, LlmApiFormat::OpenAiChat);
        assert_eq!(
            llm.resolved_base_url().as_deref(),
            Some("https://maas-api.unisound.com/v1")
        );
        assert_eq!(llm.resolved_model().as_deref(), Some("u2-flash"));
        assert!(llm.is_configured());

        let llm = LlmProfile {
            format: LlmApiFormat::AnthropicMessages,
            ..LlmProfile::default()
        };
        assert_eq!(
            llm.resolved_base_url().as_deref(),
            Some("https://maas-api.unisound.com/anthropic")
        );
        assert_eq!(llm.format.endpoint_suffix(), "/v1/messages");
    }

    #[test]
    fn explicit_builtin_url_and_model_win_and_only_resolution_trims() {
        let asr = AsrProfile {
            base_url: Some(" https://asr.example.com/root/// ".into()),
            model: Some(" u2-asr ".into()),
            ..AsrProfile::default()
        };
        assert_eq!(
            asr.resolved_base_url().as_deref(),
            Some("https://asr.example.com/root")
        );
        assert_eq!(asr.resolved_model().as_deref(), Some("u2-asr"));
        assert_eq!(
            asr.base_url.as_deref(),
            Some(" https://asr.example.com/root/// ")
        );

        let llm = LlmProfile {
            base_url: Some("https://llm.example.com/v1/".into()),
            model: Some("custom-model".into()),
            ..LlmProfile::default()
        };
        assert_eq!(
            llm.resolved_base_url().as_deref(),
            Some("https://llm.example.com/v1")
        );
        assert_eq!(llm.resolved_model().as_deref(), Some("custom-model"));
        assert_eq!(llm.base_url.as_deref(), Some("https://llm.example.com/v1/"));
    }

    #[test]
    fn custom_profiles_require_explicit_base_and_model() {
        let mut settings = Settings::default();
        let mut asr = AsrProfile {
            id: "asr".into(),
            label: "ASR".into(),
            engine: AsrEngine::OpenaiCompatible,
            ..AsrProfile::default()
        };
        assert!(!asr.is_configured());
        settings.asr_profiles.push(asr.clone());
        assert!(settings.validate().is_err());
        asr.base_url = Some("https://api.example.com/v1".into());
        assert!(settings.validate().is_err());
        asr.model = Some("whisper".into());
        assert!(asr.is_configured());

        let mut llm = LlmProfile {
            id: "llm".into(),
            label: "LLM".into(),
            engine: LlmEngine::Custom,
            ..LlmProfile::default()
        };
        assert!(!llm.is_configured());
        settings.llm_profiles.push(llm.clone());
        assert!(settings.validate().is_err());
        llm.base_url = Some("https://api.example.com/v1".into());
        assert!(settings.validate().is_err());
        llm.model = Some("cleanup-model".into());
        llm.format = LlmApiFormat::OpenAiResponses;
        assert!(llm.is_configured());
        assert_eq!(llm.resolved_model().as_deref(), Some("cleanup-model"));
        settings.asr_profiles[0] = asr;
        settings.llm_profiles[0] = llm;
        settings.validate().unwrap();

        settings.llm_profiles[0].base_url = Some("   ".into());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn validates_schema_ids_labels_and_active_profiles() {
        let mut settings = configured_settings();
        for schema_version in [0, 1, 2, 4] {
            settings.schema_version = schema_version;
            assert!(settings.validate().is_err());
        }
        settings.schema_version = 3;

        settings.asr_profiles[0].id = " ".into();
        assert!(settings.validate().is_err());
        settings.asr_profiles[0].id = "asr".into();
        settings.llm_profiles[0].label = " ".into();
        assert!(settings.validate().is_err());
        settings.llm_profiles[0].label = "LLM".into();

        settings.active_asr_profile_id = Some("missing".into());
        assert!(settings.validate().is_err());
        settings.active_asr_profile_id = Some("asr".into());
        settings.active_llm_profile_id = Some("missing".into());
        assert!(settings.validate().is_err());
        settings.active_llm_profile_id = Some("llm".into());
        settings.validate().unwrap();
    }

    #[test]
    fn rejects_duplicate_ids_globally() {
        let mut settings = configured_settings();
        settings.llm_profiles[0].id = "asr".into();
        settings.active_llm_profile_id = Some("asr".into());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn rejects_bad_profile_urls() {
        for url in [
            "api.example.com",
            "/relative",
            "ftp://api.example.com",
            "https:///missing-host",
            "https://user:password@api.example.com",
            "https://api.example.com?token=value",
            "https://api.example.com#fragment",
            "http://api.example.com",
            "http://192.168.1.10",
        ] {
            let mut settings = Settings::default();
            settings.asr_profiles.push(AsrProfile {
                id: "asr".into(),
                label: "ASR".into(),
                base_url: Some(url.into()),
                ..AsrProfile::default()
            });
            assert!(settings.validate().is_err(), "ASR accepted {url}");
            settings.asr_profiles.clear();
            settings.llm_profiles.push(LlmProfile {
                id: "llm".into(),
                label: "LLM".into(),
                base_url: Some(url.into()),
                ..LlmProfile::default()
            });
            assert!(settings.validate().is_err(), "LLM accepted {url}");
        }
    }

    #[test]
    fn allows_loopback_http_and_explicit_insecure_http() {
        for host in ["localhost", "127.0.0.1", "[::1]", "[::ffff:127.0.0.1]"] {
            let mut settings = Settings::default();
            let url = format!("http://{host}/v1");
            settings.asr_profiles.push(AsrProfile {
                id: "asr".into(),
                label: "ASR".into(),
                base_url: Some(url.clone()),
                ..AsrProfile::default()
            });
            settings.llm_profiles.push(LlmProfile {
                id: "llm".into(),
                label: "LLM".into(),
                base_url: Some(url),
                ..LlmProfile::default()
            });
            settings.validate().unwrap();
        }

        let mut settings = Settings::default();
        settings.asr_profiles.push(AsrProfile {
            id: "asr".into(),
            label: "ASR".into(),
            base_url: Some("http://asr.example.com".into()),
            allow_insecure_http: true,
            ..AsrProfile::default()
        });
        settings.llm_profiles.push(LlmProfile {
            id: "llm".into(),
            label: "LLM".into(),
            base_url: Some("http://llm.example.com".into()),
            allow_insecure_http: true,
            ..LlmProfile::default()
        });
        settings.validate().unwrap();
    }

    #[test]
    fn u2_model_is_restricted() {
        let mut settings = Settings::default();
        let mut asr = AsrProfile {
            id: "asr".into(),
            label: "ASR".into(),
            ..AsrProfile::default()
        };
        settings.asr_profiles.push(asr.clone());
        asr.model = Some("whisper".into());
        settings.asr_profiles[0] = asr.clone();
        assert!(settings.validate().is_err());
        asr.model = Some(" u2-asr ".into());
        settings.asr_profiles[0] = asr;
        settings.validate().unwrap();
    }

    #[test]
    fn validates_cleanup_and_general_settings() {
        let mut settings = Settings::default();
        for max_tokens in [0, 8193] {
            settings.cleanup.max_tokens = max_tokens;
            assert!(settings.validate().is_err());
        }
        for max_tokens in [1, 8192] {
            settings.cleanup.max_tokens = max_tokens;
            settings.validate().unwrap();
        }
        for temperature in [-0.1, 2.1, f32::NAN] {
            settings.cleanup.temperature = temperature;
            assert!(settings.validate().is_err());
        }
        for temperature in [0.0, 2.0] {
            settings.cleanup.temperature = temperature;
            settings.validate().unwrap();
        }
        settings.dictation.translate_target = " ".into();
        assert!(settings.validate().is_err());
        settings.dictation.translate_target = "English".into();
        settings.hotkey.dictate = "".into();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn extended_defaults_and_validation() {
        let mut settings = Settings::default();
        assert_eq!(settings.audio.max_recording_seconds, 540);
        assert_eq!(settings.history.pending_retention_days, 7);
        assert!(!settings.audio.prewarm);
        assert!(settings.ui.close_to_tray);
        assert!(settings.ui.check_for_updates);
        let legacy_ui: UiSettings =
            serde_json::from_str(r#"{"locale":"en","theme":"dark","close_to_tray":false}"#)
                .unwrap();
        assert!(
            legacy_ui.check_for_updates,
            "older settings files opt in to update checks"
        );
        assert_eq!(settings.ui.onboarding_completed, None);
        assert_eq!(settings.dictation.default_mode, SessionMode::Dictate);

        settings.context.max_caret_chars = 4001;
        assert!(settings.validate().is_err());
        settings.context.max_caret_chars = 4000;
        settings.audio.max_recording_seconds = 59;
        assert!(settings.validate().is_err());
        settings.audio.max_recording_seconds = 900;
        settings.history.pending_retention_days = 0;
        assert!(settings.validate().is_err());
        settings.history.pending_retention_days = 31;
        assert!(settings.validate().is_err());
        settings.history.pending_retention_days = 30;
        settings.dictation.default_tone = "x".repeat(501);
        assert!(settings.validate().is_err());
        settings.dictation.default_tone.clear();
        settings.dictation.app_tones.push(AppTone {
            app: "Slack".into(),
            style: "brief".into(),
        });
        settings.dictation.app_tones.push(AppTone {
            app: "slack".into(),
            style: "other".into(),
        });
        assert!(settings.validate().is_err());
        settings.dictation.app_tones.pop();
        settings.validate().unwrap();
    }

    #[test]
    fn app_tone_overrides_default_tone() {
        let mut settings = Settings::default();
        settings.dictation.default_tone = "concise".into();
        assert_eq!(
            settings.dictation.tone_for_app(Some("TextEdit")),
            Some("concise".into())
        );
        settings.dictation.app_tones.push(AppTone {
            app: "slack".into(),
            style: "friendly".into(),
        });
        assert_eq!(
            settings.dictation.tone_for_app(Some("Slack")),
            Some("friendly".into())
        );
        // Padded app names still match; empty styles fall through to default.
        settings.dictation.app_tones = vec![
            AppTone {
                app: " Code ".into(),
                style: "technical".into(),
            },
            AppTone {
                app: "notepad".into(),
                style: "  ".into(),
            },
        ];
        assert_eq!(
            settings.dictation.tone_for_app(Some("Code")),
            Some("technical".into())
        );
        assert_eq!(
            settings.dictation.tone_for_app(Some("Notepad")),
            Some("concise".into())
        );
    }

    #[test]
    fn legacy_dto_parses_flat_json() {
        let legacy: LegacySettings = serde_json::from_str(r#"{
            "hotkey": {"dictate": "ctrl", "smart_gestures": false},
            "dictation": {"language": "en-US", "cleanup_enabled": false},
            "asr": {"engine": "openai_compatible", "base_url": "https://asr.example.com/v1", "model": "whisper", "extra_options": [["hotwords", "yes"]]},
            "llm": {"engine": "unisound", "format": "anthropic_messages", "base_url": "https://llm.example.com/anthropic", "model": "u2-flash", "max_tokens": 2048, "temperature": 0.7},
            "context": {"include_caret_context": true},
            "history": {"retention_days": 7},
            "audio": {"mic_device": "USB mic"},
            "ui": {"locale": "zh-CN", "theme": "dark"}
        }"#).unwrap();
        let settings = Settings::migrate_legacy(legacy);
        settings.validate().unwrap();
        assert_eq!(settings.hotkey.dictate, "ctrl");
        assert!(!settings.hotkey.smart_gestures);
        assert_eq!(settings.dictation.language.as_deref(), Some("en-US"));
        assert!(!settings.dictation.cleanup_enabled);
        assert_eq!(settings.asr_profiles[0].engine, AsrEngine::OpenaiCompatible);
        assert_eq!(
            settings.asr_profiles[0].extra_options,
            vec![("hotwords".into(), "yes".into())]
        );
        assert_eq!(
            settings.llm_profiles[0].format,
            LlmApiFormat::AnthropicMessages
        );
        assert_eq!(settings.cleanup.max_tokens, 2048);
        assert_eq!(settings.cleanup.temperature, 0.7);
        assert!(settings.context.include_caret_context);
        assert_eq!(settings.history.retention_days, Some(7));
        assert_eq!(settings.audio.mic_device.as_deref(), Some("USB mic"));
        assert_eq!(settings.ui.locale, "zh-CN");
        assert_eq!(settings.ui.theme, "dark");
    }

    #[test]
    fn legacy_migration_preserves_input_and_is_idempotent() {
        let mut legacy = LegacySettings::default();
        legacy.hotkey.dictate = "right-opt".into();
        legacy.hotkey.smart_gestures = false;
        legacy.hotkey.dictate_alt = Some("right-alt".into());
        legacy.dictation.language = Some("auto".into());
        legacy.dictation.app_tones.push(AppTone {
            app: "slack".into(),
            style: "concise".into(),
        });
        legacy.dictation.translate_target = "Chinese".into();
        legacy.asr = LegacyAsrSettings {
            engine: AsrEngine::OpenaiCompatible,
            base_url: "https://asr.example.com/v1/".into(),
            model: "whisper".into(),
            extra_options: vec![("temperature".into(), "0".into())],
        };
        legacy.llm = LegacyLlmSettings {
            engine: LlmEngine::Unisound,
            format: LlmApiFormat::AnthropicMessages,
            base_url: "https://llm.example.com/anthropic/".into(),
            model: "u2-flash".into(),
            max_tokens: 2048,
            temperature: 0.7,
        };
        legacy.context.include_app_context = false;
        legacy.context.include_caret_context = true;
        legacy.context.max_caret_chars = 123;
        legacy.history.retention_days = None;
        legacy.history.keep_audio = true;
        legacy.audio.mic_device = Some("USB mic".into());
        legacy.audio.mute_output_while_dictating = true;
        legacy.ui.locale = "zh-CN".into();
        legacy.ui.theme = "dark".into();

        let original = serde_json::to_value(&legacy).unwrap();
        let first = Settings::migrate_legacy(legacy.clone());
        let second = Settings::migrate_legacy(legacy.clone());
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
        assert_eq!(serde_json::to_value(&legacy).unwrap(), original);
        first.validate().unwrap();

        assert_eq!(
            first.active_asr_profile_id.as_deref(),
            Some("legacy-asr-profile")
        );
        assert_eq!(
            first.active_llm_profile_id.as_deref(),
            Some("legacy-llm-profile")
        );
        assert_eq!(first.asr_profiles[0].id, "legacy-asr-profile");
        assert_eq!(
            first.asr_profiles[0].credential_id.as_deref(),
            Some("legacy-asr-credential")
        );
        assert_eq!(
            first.asr_profiles[0].base_url.as_deref(),
            Some(legacy.asr.base_url.as_str())
        );
        assert_eq!(first.asr_profiles[0].model.as_deref(), Some("whisper"));
        assert_eq!(
            first.asr_profiles[0].extra_options,
            legacy.asr.extra_options
        );
        assert_eq!(first.llm_profiles[0].id, "legacy-llm-profile");
        assert_eq!(
            first.llm_profiles[0].credential_id.as_deref(),
            Some("legacy-llm-credential")
        );
        assert_eq!(first.llm_profiles[0].engine, legacy.llm.engine);
        assert_eq!(first.llm_profiles[0].format, legacy.llm.format);
        assert_eq!(
            first.llm_profiles[0].base_url.as_deref(),
            Some(legacy.llm.base_url.as_str())
        );
        assert_eq!(first.llm_profiles[0].model.as_deref(), Some("u2-flash"));
        assert_eq!(first.cleanup.max_tokens, legacy.llm.max_tokens);
        assert_eq!(first.cleanup.temperature, legacy.llm.temperature);
        assert_eq!(first.hotkey, legacy.hotkey);
        assert_eq!(first.dictation, legacy.dictation);
        assert_eq!(first.context, legacy.context);
        assert_eq!(first.history, legacy.history);
        assert_eq!(first.audio, legacy.audio);
        assert_eq!(first.ui, legacy.ui);
    }

    #[test]
    fn legacy_builtin_explicit_url_is_preserved() {
        let legacy = LegacySettings {
            asr: LegacyAsrSettings {
                engine: AsrEngine::UnisoundU2,
                base_url: "https://asr.example.com/custom/".into(),
                model: "u2-asr".into(),
                extra_options: Vec::new(),
            },
            llm: LegacyLlmSettings {
                engine: LlmEngine::Unisound,
                format: LlmApiFormat::OpenAiChat,
                base_url: "https://llm.example.com/v1/".into(),
                model: String::new(),
                ..LegacyLlmSettings::default()
            },
            ..LegacySettings::default()
        };
        let settings = Settings::migrate_legacy(legacy);
        settings.validate().unwrap();
        assert_eq!(
            settings.asr_profiles[0].resolved_base_url().as_deref(),
            Some("https://asr.example.com/custom")
        );
        assert_eq!(
            settings.asr_profiles[0].resolved_model().as_deref(),
            Some("u2-asr")
        );
        assert_eq!(
            settings.llm_profiles[0].resolved_base_url().as_deref(),
            Some("https://llm.example.com/v1")
        );
        assert_eq!(
            settings.llm_profiles[0].resolved_model().as_deref(),
            Some("u2-flash")
        );
    }

    #[test]
    fn legacy_migration_ignores_secret_fields() {
        let legacy: LegacySettings = serde_json::from_str(
            r#"{
            "api_key": "ovt-canary-secret",
            "asr": {"api_key": "ovt-canary-secret"},
            "llm": {"secret": "ovt-canary-secret"}
        }"#,
        )
        .unwrap();
        let settings = Settings::migrate_legacy(legacy);
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("ovt-canary-secret"));
        assert!(!json.contains("api_key"));
        assert!(!json.contains("secret"));
    }
}
