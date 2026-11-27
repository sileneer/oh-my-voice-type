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
pub mod models;
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

/// Common HTTP client construction (rustls + native roots + system proxy, timeouts).
pub fn http_client() -> reqwest::Client {
    http_client_with_proxy(true)
}

/// Direct HTTP client with proxy disabled, used as an automatic fallback when a
/// configured proxy is unreachable (e.g. a stopped local proxy).
pub fn direct_http_client() -> reqwest::Client {
    http_client_with_proxy(false)
}

fn http_client_with_proxy(use_proxy: bool) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("ovt/{}", env!("CARGO_PKG_VERSION")));
    if !use_proxy {
        builder = builder.no_proxy();
    }
    builder.build().expect("reqwest client")
}

/// HTTP client for async-job providers (upload + polling can outlive the
/// per-call request timeout, which then applies per request).
pub fn job_client() -> reqwest::Client {
    job_client_with_proxy(true)
}

/// Direct async-job HTTP client with proxy disabled for unreachable-proxy fallback.
pub fn direct_job_client() -> reqwest::Client {
    job_client_with_proxy(false)
}

fn job_client_with_proxy(use_proxy: bool) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("ovt/{}", env!("CARGO_PKG_VERSION")));
    if !use_proxy {
        builder = builder.no_proxy();
    }
    builder.build().expect("reqwest client")
}

/// Returns true when `err` failed while connecting to a configured proxy
/// (for example a stale `HTTPS_PROXY` pointing to a stopped local proxy that
/// actively refuses TCP connections).
pub fn is_proxy_tunnel_error(err: &reqwest::Error) -> bool {
    if !err.is_connect() {
        return false;
    }
    use std::error::Error as StdError;
    let mut current: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(source) = current {
        let text = source.to_string().to_ascii_lowercase();
        if text.contains("tunnel error") || text.contains("proxy") || text.contains("socks") {
            return true;
        }
        current = source.source();
    }
    false
}

/// Execute an HTTP request via `primary`, retrying once directly via `direct`
/// (`no_proxy`) if connecting to a configured proxy fails.
pub async fn send_with_proxy_fallback<F>(
    primary: &reqwest::Client,
    direct: &reqwest::Client,
    context: &str,
    mut build: F,
) -> Result<reqwest::Response>
where
    F: FnMut(&reqwest::Client) -> Result<reqwest::RequestBuilder>,
{
    match build(primary)?.send().await {
        Ok(resp) => Ok(resp),
        Err(err) if is_proxy_tunnel_error(&err) => {
            tracing::warn!(
                context,
                error = %format_network_error(context, &err),
                "configured proxy is unreachable; retrying directly without proxy"
            );
            build(direct)?
                .send()
                .await
                .map_err(|direct_err| Error::Network(format_network_error(context, &direct_err)))
        }
        Err(err) => Err(Error::Network(format_network_error(context, &err))),
    }
}

pub fn map_http_error(context: &str, status: u16, body: &str) -> Error {
    let message = match status {
        401 | 403 => "authentication failed — check the API key".to_string(),
        400 if looks_like_auth_error(body) => {
            "authentication failed — check the API key".to_string()
        }
        404 => "endpoint not found (HTTP 404)".to_string(),
        429 => "rate limited — retry shortly".to_string(),
        500..=599 => "provider server error".to_string(),
        _ => format!("HTTP error {status}"),
    };
    Error::Provider {
        status,
        message: format!("{context}: {message}"),
    }
}

/// Some OpenAI-compatible providers (such as Google Gemini and xAI) return
/// HTTP 400 instead of 401 when the supplied API key is invalid.
fn looks_like_auth_error(body: &str) -> bool {
    let lc = body.to_ascii_lowercase();
    lc.contains("api key")
        || lc.contains("api_key")
        || lc.contains("apikey")
        || lc.contains("invalid_api_key")
        || lc.contains("unauthorized")
        || lc.contains("authentication")
}

/// Format a `reqwest::Error` including its root cause chain (DNS, TLS, proxy,
/// timeout, connection reset) instead of only "error sending request for url".
pub fn format_network_error(context: &str, err: &reqwest::Error) -> String {
    use std::error::Error as StdError;
    let mut parts = vec![err.to_string()];
    let mut current = err.source();
    while let Some(source) = current {
        let text = source.to_string();
        if !parts.iter().any(|existing| existing.contains(&text)) {
            parts.push(text);
        }
        current = source.source();
    }
    format!("{context}: {}", parts.join(": "))
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

    #[test]
    fn http_400_with_api_key_message_maps_to_authentication_error_without_leaking_body() {
        let gemini = map_http_error(
            "models",
            400,
            r#"{"error":{"code":400,"message":"Please pass a valid API key","status":"INVALID_ARGUMENT"}}"#,
        );
        match gemini {
            Error::Provider { status, message } => {
                assert_eq!(status, 400);
                assert!(message.contains("authentication failed"), "{message}");
                assert!(!message.contains("INVALID_ARGUMENT"));
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let not_found = map_http_error("models", 404, "secret-body");
        match not_found {
            Error::Provider { status, message } => {
                assert_eq!(status, 404);
                assert!(message.contains("not found"), "{message}");
                assert!(!message.contains("secret-body"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn dead_https_proxy_falls_back_to_direct_connection() {
        use std::io::{BufRead, BufWriter, Write};
        use std::net::TcpListener;

        // Bind then drop a listener to reserve an unused local port that refuses connections.
        let dead_port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };

        // Start a real target server on loopback to confirm direct fallback works.
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        let target_addr = target.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = target.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let mut writer = BufWriter::new(stream);
            write!(
                writer,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            )
            .unwrap();
            writer.flush().unwrap();
        });

        let broken_proxy_client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{dead_port}")).unwrap())
            .connect_timeout(Duration::from_secs(3))
            .build()
            .unwrap();

        // HTTPS via dead HTTP proxy produces a tunnel connect error.
        let https_err = broken_proxy_client
            .get("https://example.invalid/v1/models")
            .send()
            .await
            .unwrap_err();
        assert!(
            is_proxy_tunnel_error(&https_err),
            "expected proxy tunnel error, got: {}",
            format_network_error("test", &https_err)
        );

        // Using a fallback client recovers when primary hits a tunnel error;
        // verify direct client reaches the target cleanly.
        let direct = direct_http_client();
        let resp = direct
            .get(format!("http://{target_addr}/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "ok");
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

/// U2-ASR wire protocol constants (per https://maas.unisound.com/docs/api/overview):
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
