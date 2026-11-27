//! OpenAI-compatible adapters: `POST {base}/audio/transcriptions` and
//! `POST {base}/chat/completions`.
//!
//! Vocabulary goes through the free-text `prompt` field shared with Whisper
//! servers; context text is appended after the hotwords (Whisper treats the
//! prompt as continuing conversation, so both compose naturally).

use super::{
    audio_part, default_capabilities, direct_http_client, format_network_error, http_client,
    map_http_error, send_with_proxy_fallback, with_bearer, AsrCapabilities, AsrProvider,
    CleanupRequest, LlmProvider, TranscribeRequest, Transcript,
};
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Instant;

pub struct OpenAiCompatibleAsr {
    pub client: reqwest::Client,
    /// Root URL including version, e.g. `https://api.example.com/v1`.
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub response_format: Option<String>,
    pub send_temperature: bool,
    pub send_language: bool,
    pub send_prompt: bool,
}

impl OpenAiCompatibleAsr {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: http_client(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            response_format: Some("verbose_json".into()),
            send_temperature: true,
            send_language: true,
            send_prompt: true,
        }
    }

    /// Resolve the `response_format` sent to `/audio/transcriptions`.
    /// OpenAI's `gpt-*-transcribe*` models reject `verbose_json` with HTTP 400,
    /// so fall back to `json` automatically when `verbose_json` is configured.
    fn effective_response_format(&self) -> Option<&str> {
        match self.response_format.as_deref() {
            Some("verbose_json") if is_gpt_transcribe_model(&self.model) => Some("json"),
            other => other,
        }
    }

    /// Compose the `prompt` field: hotwords first, context after.
    fn build_prompt(hotwords: &[String], context: Option<&str>) -> Option<String> {
        let mut prompt = String::new();
        if !hotwords.is_empty() {
            prompt.push_str(&hotwords.join(", "));
        }
        if let Some(context) = context.filter(|c| !c.trim().is_empty()) {
            if !prompt.is_empty() {
                prompt.push_str(". ");
            }
            prompt.push_str(context.trim());
        }
        if prompt.is_empty() {
            None
        } else {
            Some(prompt)
        }
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerboseTranscriptionResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
}

#[async_trait]
impl AsrProvider for OpenAiCompatibleAsr {
    fn name(&self) -> &'static str {
        "openai-compatible"
    }

    fn capabilities(&self) -> AsrCapabilities {
        default_capabilities()
    }

    async fn transcribe(&self, req: TranscribeRequest) -> Result<Transcript> {
        if req.audio.is_empty() {
            return Err(Error::Audio("no audio to transcribe".into()));
        }
        let started = Instant::now();
        let TranscribeRequest {
            audio,
            language,
            hotwords,
            context,
        } = req;
        let url = format!("{}/audio/transcriptions", self.base_url);
        let response_format = self.effective_response_format().map(str::to_string);
        let prompt = if self.send_prompt {
            Self::build_prompt(&hotwords, context.as_deref())
        } else {
            None
        };
        let lang = if self.send_language {
            language
                .as_deref()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
        } else {
            None
        };
        let direct = direct_http_client();
        let resp = send_with_proxy_fallback(&self.client, &direct, "asr", |http| {
            let body = audio_part(audio.clone())?;
            let mut form = body.form.text("model", self.model.clone());
            if let Some(fmt) = &response_format {
                form = form.text("response_format", fmt.clone());
            }
            if self.send_temperature {
                form = form.text("temperature", "0");
            }
            if let Some(language) = &lang {
                form = form.text("language", language.clone());
            }
            if let Some(prompt_text) = &prompt {
                form = form.text("prompt", prompt_text.clone());
            }
            Ok(with_bearer(http.post(&url), &self.api_key).multipart(form))
        })
        .await?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(format_network_error("asr body", &e)))?;
        if !status.is_success() {
            return Err(map_http_error("asr", status.as_u16(), &text));
        }
        // Verbose-json shape first; plain `{"text": …}` servers second.
        if let Ok(v) = serde_json::from_str::<VerboseTranscriptionResponse>(&text) {
            return Ok(Transcript {
                text: v.text.trim().to_string(),
                language: v.language,
                duration_ms: v
                    .duration
                    .map(|d| (d * 1000.0) as u64)
                    .or_else(|| Some(started.elapsed().as_millis() as u64)),
            });
        }
        let plain: TranscriptionResponse = serde_json::from_str(&text)
            .map_err(|_| Error::ProviderResponse(text.chars().take(300).collect()))?;
        Ok(Transcript {
            text: plain.text.trim().to_string(),
            language: plain.language,
            duration_ms: Some(started.elapsed().as_millis() as u64),
        })
    }
}

pub struct OpenAiCompatibleLlm {
    pub client: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl OpenAiCompatibleLlm {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: http_client(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    pub fn payload(&self, req: &CleanupRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user},
            ],
        });
        if uses_max_completion_tokens(&self.model) {
            body["max_completion_tokens"] = serde_json::json!(req.max_tokens);
        } else {
            body["max_tokens"] = serde_json::json!(req.max_tokens);
        }
        if !has_fixed_temperature(&self.model) {
            body["temperature"] = serde_json::json!(req.temperature);
        }
        body
    }
}

fn bare_model_name(model: &str) -> String {
    model
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase()
}

fn is_gpt_transcribe_model(model: &str) -> bool {
    let bare = bare_model_name(model);
    bare.starts_with("gpt-") && bare.contains("transcribe")
}

fn is_openai_reasoning_family(bare: &str) -> bool {
    bare == "gpt-5"
        || bare.starts_with("gpt-5-")
        || bare.starts_with("gpt-5.")
        || bare == "gpt-6"
        || bare.starts_with("gpt-6-")
        || bare.starts_with("gpt-6.")
        || bare == "o1"
        || bare.starts_with("o1-")
        || bare == "o3"
        || bare.starts_with("o3-")
        || bare == "o4"
        || bare.starts_with("o4-")
}

/// OpenAI's GPT-5 and o-series models reject `max_tokens` in Chat Completions
/// and require `max_completion_tokens` instead.
fn uses_max_completion_tokens(model: &str) -> bool {
    !model.contains('/') && is_openai_reasoning_family(&bare_model_name(model))
}

/// Models whose APIs reject custom temperatures (e.g. `kimi-k2.5` only allows
/// `temperature=1`, `gpt-5*`/o-series reject non-default temperature, and
/// `MiniMax-M3*` locks temperature to 1).
fn has_fixed_temperature(model: &str) -> bool {
    let bare = bare_model_name(model);
    is_openai_reasoning_family(&bare)
        || bare.starts_with("kimi-k2.5")
        || bare.starts_with("kimi-k2.7")
        || bare.starts_with("minimax-m3")
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleLlm {
    async fn complete(&self, req: CleanupRequest) -> Result<String> {
        let payload = self.payload(&req);
        let url = format!("{}/chat/completions", self.base_url);
        let direct = direct_http_client();
        let resp = send_with_proxy_fallback(&self.client, &direct, "llm", |http| {
            Ok(with_bearer(http.post(&url).json(&payload), &self.api_key))
        })
        .await?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(format_network_error("llm body", &e)))?;
        if !status.is_success() {
            return Err(map_http_error("llm", status.as_u16(), &text));
        }
        let parsed: ChatResponse = serde_json::from_str(&text)
            .map_err(|_| Error::ProviderResponse(text.chars().take(300).collect()))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();
        Ok(content.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_mapping_mentions_auth() {
        let err = map_http_error("asr", 401, r#"{"error":"bad key"}"#);
        match err {
            Error::Provider { status, message } => {
                assert_eq!(status, 401);
                assert!(message.contains("authentication"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn prompt_joins_hotwords_and_context() {
        let hotwords = vec!["K8s".to_string(), "Claude".to_string()];
        assert_eq!(
            OpenAiCompatibleAsr::build_prompt(&hotwords, Some("Talking in #engineering"))
                .as_deref(),
            Some("K8s, Claude. Talking in #engineering")
        );
    }

    #[test]
    fn request_options_default_to_whisper_shape() {
        let provider = OpenAiCompatibleAsr::new("https://asr.example/v1", "key", "whisper-1");
        assert_eq!(provider.response_format.as_deref(), Some("verbose_json"));
        assert_eq!(provider.effective_response_format(), Some("verbose_json"));
        assert!(provider.send_temperature);
        assert!(provider.send_language);
        assert!(provider.send_prompt);
    }

    #[test]
    fn gpt_transcribe_models_avoid_verbose_json() {
        let provider =
            OpenAiCompatibleAsr::new("https://api.openai.com/v1", "key", "gpt-4o-mini-transcribe");
        assert_eq!(provider.effective_response_format(), Some("json"));
    }

    #[test]
    fn llm_payload_adapts_for_fixed_temperature_and_gpt5_models() {
        let req = CleanupRequest {
            system: "sys".into(),
            user: "usr".into(),
            max_tokens: 256,
            temperature: 0.2,
        };

        let standard = OpenAiCompatibleLlm::new("https://api.openai.com/v1", "k", "gpt-4.1-mini")
            .payload(&req);
        assert_eq!(standard["max_tokens"], 256);
        assert!(standard.get("max_completion_tokens").is_none());
        assert!(standard.get("temperature").is_some());

        let gpt5 =
            OpenAiCompatibleLlm::new("https://api.openai.com/v1", "k", "gpt-5-mini").payload(&req);
        assert_eq!(gpt5["max_completion_tokens"], 256);
        assert!(gpt5.get("max_tokens").is_none());
        assert!(gpt5.get("temperature").is_none());

        let kimi =
            OpenAiCompatibleLlm::new("https://api.moonshot.cn/v1", "k", "kimi-k2.5").payload(&req);
        assert_eq!(kimi["max_tokens"], 256);
        assert!(kimi.get("temperature").is_none());

        let minimax =
            OpenAiCompatibleLlm::new("https://api.minimaxi.com/v1", "k", "MiniMax-M3.1-Flash")
                .payload(&req);
        assert_eq!(minimax["max_tokens"], 256);
        assert!(minimax.get("temperature").is_none());
    }

    #[test]
    fn prompt_empty_when_no_vocabulary() {
        assert!(OpenAiCompatibleAsr::build_prompt(&[], None).is_none());
    }
}
