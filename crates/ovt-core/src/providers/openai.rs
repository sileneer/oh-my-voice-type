//! OpenAI-compatible adapters: `POST {base}/audio/transcriptions` and
//! `POST {base}/chat/completions`.
//!
//! Vocabulary goes through the free-text `prompt` field shared with Whisper
//! servers; context text is appended after the hotwords (Whisper treats the
//! prompt as continuing conversation, so both compose naturally).

use super::{
    audio_part, default_capabilities, http_client, map_http_error, with_bearer, AsrCapabilities,
    AsrProvider, CleanupRequest, LlmProvider, TranscribeRequest, Transcript,
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
        let body = audio_part(audio)?;
        let mut form = body
            .form
            .text("model", self.model.clone())
            .text("response_format", "verbose_json")
            .text("temperature", "0");
        if let Some(lang) = language.as_deref().filter(|l| !l.is_empty()) {
            form = form.text("language", lang.to_string());
        }
        if let Some(prompt) = Self::build_prompt(&hotwords, context.as_deref()) {
            form = form.text("prompt", prompt);
        }

        let resp = with_bearer(
            self.client
                .post(format!("{}/audio/transcriptions", self.base_url)),
            &self.api_key,
        )
        .multipart(form)
        .send()
        .await
        .map_err(|e| Error::Network(e.to_string()))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;
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
        let payload = serde_json::json!({
            "model": self.model,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user},
            ],
        });
        let resp = with_bearer(
            self.client
                .post(format!("{}/chat/completions", self.base_url))
                .json(&payload),
            &self.api_key,
        )
        .send()
        .await
        .map_err(|e| Error::Network(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;
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
    fn prompt_empty_when_no_vocabulary() {
        assert!(OpenAiCompatibleAsr::build_prompt(&[], None).is_none());
    }
}
