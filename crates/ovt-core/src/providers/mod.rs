//! Provider traits and adapters.
//!
//! Two ASR dialects ship today:
//! - **OpenAI-compatible** (`audio/transcriptions`; also our baseline for
//!   Whisper-style servers), vocabulary via a free-text `prompt` field;
//! - **Unisound U2-ASR** (async workflow: upload → create task → poll),
//!   vocabulary via discrete `hotwords` + a `context` field.
//!
//! Vocabulary mapping is capability-driven: the engine asks the provider how
//! many hotwords it accepts and how much context it takes, then packs
//! dictionary terms accordingly (`HotwordRules::apply`).

use crate::error::{Error, Result};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub mod anthropic;
pub mod capabilities;
pub mod openai;
pub mod openai_responses;
pub mod u2;

pub use capabilities::{AsrCapabilities, HotwordRules};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscribeRequest {
    /// WAV bytes (16-bit PCM mono).
    pub audio: Vec<u8>,
    /// Language hint in provider-native format (already resolved through
    /// `AsrCapabilities::resolve_language`); `None` = provider auto-detect.
    pub language: Option<String>,
    /// Short vocabulary terms (fitting the provider's hotword rules).
    pub hotwords: Vec<String>,
    /// Everything the provider should know beyond the audio: app/tone/style
    /// hints and vocabulary terms that overflowed the hotword rules. Each
    /// provider maps this onto its context facility (Whisper: part of
    /// `prompt`; U2: the `context` field).
    pub context: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    /// Detected language when the provider reports one.
    pub language: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanupRequest {
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[async_trait]
pub trait AsrProvider: Send + Sync {
    async fn transcribe(&self, req: TranscribeRequest) -> Result<Transcript>;
    /// Vocabulary/context/language rules for this provider; defaults to the
    /// conservative OpenAI-compatible shape.
    fn capabilities(&self) -> AsrCapabilities {
        default_capabilities()
    }
    /// Stable provider label for logs and UI ("u2-asr", "openai-compatible").
    fn name(&self) -> &'static str;
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, req: CleanupRequest) -> Result<String>;
}

pub fn default_capabilities() -> AsrCapabilities {
    AsrCapabilities {
        hotwords: HotwordRules::PromptField {
            max_chars: crate::dictionary::HOTWORD_BUDGET_CHARS,
        },
        max_context_chars: None,
        languages: &[],
    }
}

/// Common HTTP client construction (rustls, timeouts, gzip off for simplicity).
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("ovt/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest client")
}

/// HTTP client for async-job providers (upload + polling can outlive the
/// per-call request timeout, which then applies per request).
pub fn job_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("ovt/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest client")
}

pub fn map_http_error(context: &str, status: u16, _body: &str) -> Error {
    let message = match status {
        401 | 403 => "authentication failed — check the API key".to_string(),
        429 => "rate limited — retry shortly".to_string(),
        500..=599 => "provider server error".to_string(),
        _ => format!("HTTP error {status}"),
    };
    Error::Provider {
        status,
        message: format!("{context}: {message}"),
    }
}

/// Attach the API key to a builder.
pub fn with_bearer(builder: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
    builder.bearer_auth(api_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_errors_are_status_specific_and_omit_response_bodies() {
        for (status, expected) in [
            (401, "authentication failed"),
            (403, "authentication failed"),
            (429, "rate limited"),
            (500, "provider server error"),
            (503, "provider server error"),
            (418, "HTTP error 418"),
        ] {
            let error = map_http_error("provider", status, "secret-canary-response-body");
            match error {
                Error::Provider {
                    status: actual_status,
                    message,
                } => {
                    assert_eq!(actual_status, status);
                    assert!(message.contains(expected), "{message}");
                    assert!(!message.contains("secret-canary-response-body"));
                    assert!(!message.contains("response-body"));
                }
                other => panic!("unexpected error: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn redirects_are_not_followed_with_credentials() {
        use std::io::{BufRead, BufWriter, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            assert!(request.starts_with("GET /redirect "));
            assert!(request
                .to_lowercase()
                .contains("authorization: bearer canary-key"));
            let mut writer = BufWriter::new(stream);
            write!(
                writer,
                "HTTP/1.1 302 Found\r\nLocation: http://{address}/other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            writer.flush().unwrap();
        });

        let response = http_client()
            .get(format!("http://{address}/redirect"))
            .bearer_auth("canary-key")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 302);
        server.join().unwrap();
    }
}

pub struct MultipartBody {
    pub form: Form,
}

pub fn audio_part(audio: Vec<u8>) -> Result<MultipartBody> {
    let part = Part::bytes(audio)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| Error::ProviderResponse(e.to_string()))?;
    Ok(MultipartBody {
        form: Form::new().part("file", part),
    })
}

/// U2-ASR wire protocol constants (from the SkillHub skill's implementation
/// and https://maas.unisound.com/docs/api/overview):
/// - upload: `POST /v1/files/upload` (multipart, `purpose=a2t_async_input`)
/// - task:   `POST /v1/audio/asr/tasks`
/// - poll:   `GET  /v1/audio/asr/tasks/{task_id}`
pub mod u2_wire {
    pub const DEFAULT_BASE_URL: &str = "https://maas-api.unisound.com";
    pub const UPLOAD_PATH: &str = "/v1/files/upload";
    pub const TASK_PATH: &str = "/v1/audio/asr/tasks";
    pub const MODEL: &str = "u2-asr";
    pub const MAX_HOTWORDS: usize = 200;
    pub const MAX_HOTWORD_CHARS: usize = 5;
    pub const MAX_CONTEXT_CHARS: usize = 500;

    /// Code + English label, matching the skill's supported set.
    pub const LANGUAGES: &[(&str, &str)] = &[
        ("zh-CN", "Chinese (Mandarin)"),
        ("en-US", "English (US)"),
        ("ar-SA", "Arabic"),
        ("de-DE", "German"),
        ("es-MX", "Spanish (Mexico)"),
        ("fr-FR", "French"),
        ("id-ID", "Indonesian"),
        ("ja-JP", "Japanese"),
        ("ko-KR", "Korean"),
        ("pt-BR", "Portuguese (Brazil)"),
        ("ru-RU", "Russian"),
        ("tr-TR", "Turkish"),
        ("vi-VN", "Vietnamese"),
        ("th-TH", "Thai"),
        ("it-IT", "Italian"),
    ];
}
