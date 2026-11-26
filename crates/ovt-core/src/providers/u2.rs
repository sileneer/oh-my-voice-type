//! Unisound U2-ASR adapter: asynchronous workflow
//! `upload → create task → poll`, per https://maas.unisound.com/docs/api/overview
//! and the SkillHub `u2-asr-pro` skill implementation.
//!
//! Vocabulary mapping (this is the win over the Whisper-style path):
//! - dictionary terms that fit the hotword rules (≤5 chars, ≤200 items) →
//!   the task's `hotwords` array (acoustic-level recognition guidance);
//! - overflowing terms (e.g. "Kubernetes", "Codestral-CLI") + app/tone/caret
//!   context → the task's `context` field (language-model guidance, ≤500 chars).
//!
//! Error shapes: HTTP errors and `{"base_resp": {"status_code": …}}` non-zero
//! payloads both surface as `Error::Provider` with the raw message preserved.

use super::{
    job_client, map_http_error, u2_wire, with_bearer, AsrCapabilities, AsrProvider,
    TranscribeRequest, Transcript,
};
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde_json::json;
use std::time::{Duration, Instant};

/// U2 statuses that mean "keep polling".
const PENDING_STATUSES: &[&str] = &[
    "pending",
    "queued",
    "created",
    "waiting",
    "running",
    "processing",
    "in_progress",
    "started",
];

pub struct U2Asr {
    pub client: reqwest::Client,
    /// Root URL without version, e.g. `https://maas-api.unisound.com`.
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub poll_interval: Duration,
    pub timeout: Duration,
}

impl U2Asr {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: job_client(),
            base_url: u2_wire::DEFAULT_BASE_URL.to_string(),
            api_key: api_key.into(),
            model: u2_wire::MODEL.to_string(),
            poll_interval: Duration::from_secs(1),
            timeout: Duration::from_secs(120),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    pub fn with_timing(mut self, poll_interval: Duration, timeout: Duration) -> Self {
        self.poll_interval = poll_interval;
        self.timeout = timeout;
        self
    }

    pub fn capabilities() -> AsrCapabilities {
        AsrCapabilities {
            hotwords: super::HotwordRules::List {
                max_items: u2_wire::MAX_HOTWORDS,
                max_item_chars: u2_wire::MAX_HOTWORD_CHARS,
            },
            max_context_chars: Some(u2_wire::MAX_CONTEXT_CHARS),
            languages: u2_wire::LANGUAGES,
        }
    }

    /// Envelope check: U2 signals app-level failures in `base_resp`.
    fn ensure_api_success(operation: &str, value: &serde_json::Value) -> Result<()> {
        let base = value.get("base_resp").cloned().unwrap_or_default();
        let code = base
            .get("status_code")
            .and_then(|c| c.as_i64())
            .unwrap_or(0);
        if code == 0 {
            return Ok(());
        }
        let message = base
            .get("status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown U2 API error");
        Err(Error::Provider {
            status: 200,
            message: format!("u2-asr {operation} failed (status_code={code}): {message}"),
        })
    }

    async fn upload(&self, audio: Vec<u8>) -> Result<String> {
        let part = reqwest::multipart::Part::bytes(audio)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| Error::ProviderResponse(e.to_string()))?;
        let form = reqwest::multipart::Form::new()
            .text("purpose", "a2t_async_input")
            .part("file", part);
        let resp = with_bearer(
            self.client
                .post(format!("{}{}", self.base_url, u2_wire::UPLOAD_PATH)),
            &self.api_key,
        )
        .multipart(form)
        .send()
        .await
        .map_err(|e| Error::Network(format!("u2-asr upload: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(format!("u2-asr upload body: {e}")))?;
        if !status.is_success() {
            return Err(map_http_error("u2-asr upload", status.as_u16(), &text));
        }
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| Error::ProviderResponse(text.chars().take(300).collect()))?;
        Self::ensure_api_success("upload", &value)?;
        value
            .get("file")
            .and_then(|f| f.get("file_id"))
            .and_then(|id| id.as_i64())
            .map(|id| id.to_string())
            .ok_or_else(|| {
                Error::ProviderResponse(format!(
                    "u2-asr upload returned no file_id: {}",
                    text.chars().take(300).collect::<String>()
                ))
            })
    }

    async fn create_task(&self, file_id: &str, req: &TranscribeRequest) -> Result<String> {
        let mut payload = json!({
            "model": self.model,
            "format": "wav",
            "sample_rate": crate::wav::TARGET_SAMPLE_RATE,
            "channel": 1,
            "enable_itn": true,
            "word_info": false,
            "file_id": file_id.parse::<i64>().unwrap_or_default(),
        });
        match req.language.as_deref() {
            Some(lang) => payload["language"] = json!(lang),
            None => payload["enable_auto_lang"] = json!(true),
        }
        if !req.hotwords.is_empty() {
            payload["hotwords"] = json!(req.hotwords);
        }
        if let Some(context) = req.context.as_deref().filter(|c| !c.trim().is_empty()) {
            let bounded: String = context.chars().take(u2_wire::MAX_CONTEXT_CHARS).collect();
            payload["context"] = json!(bounded);
        }
        let resp = with_bearer(
            self.client
                .post(format!("{}{}", self.base_url, u2_wire::TASK_PATH))
                .json(&payload),
            &self.api_key,
        )
        .send()
        .await
        .map_err(|e| Error::Network(format!("u2-asr task create: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(format!("u2-asr task body: {e}")))?;
        if !status.is_success() {
            return Err(map_http_error("u2-asr task", status.as_u16(), &text));
        }
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| Error::ProviderResponse(text.chars().take(300).collect()))?;
        Self::ensure_api_success("task create", &value)?;
        extract_task_id(&value).ok_or_else(|| {
            Error::ProviderResponse(format!(
                "u2-asr task create returned no task_id: {}",
                text.chars().take(300).collect::<String>()
            ))
        })
    }

    async fn poll(&self, task_id: &str) -> Result<serde_json::Value> {
        let deadline = Instant::now() + self.timeout;
        let mut last: Option<serde_json::Value> = None;
        loop {
            if Instant::now() >= deadline {
                let snippet = last
                    .map(|v| v.to_string().chars().take(300).collect::<String>())
                    .unwrap_or_default();
                return Err(Error::Timeout(format!(
                    "u2-asr task {task_id} still running after {}s: {snippet}",
                    self.timeout.as_secs()
                )));
            }
            let resp = with_bearer(
                self.client.get(format!(
                    "{}{}/{}",
                    self.base_url,
                    u2_wire::TASK_PATH,
                    url_encode(task_id)
                )),
                &self.api_key,
            )
            .send()
            .await
            .map_err(|e| Error::Network(format!("u2-asr poll: {e}")))?;
            let http_status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| Error::Network(format!("u2-asr poll body: {e}")))?;
            if !http_status.is_success() {
                return Err(map_http_error("u2-asr poll", http_status.as_u16(), &text));
            }
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|_| Error::ProviderResponse(text.chars().take(300).collect()))?;
            Self::ensure_api_success("task poll", &value)?;

            let status = detect_status(&value);
            if is_terminal_success(&status) {
                return Ok(value);
            }
            if is_terminal_failure(&status) {
                return Err(Error::Provider {
                    status: 200,
                    message: format!("u2-asr task ended in state '{status}': {text}"),
                });
            }
            if !PENDING_STATUSES.contains(&status.as_str()) && status != "unknown" {
                tracing::debug!(status, "u2-asr: unrecognized status, still polling");
            }
            last = Some(value);
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn detect_status(value: &serde_json::Value) -> String {
    for candidate in [
        value.get("status"),
        value.get("task").and_then(|t| t.get("status")),
        value.get("data").and_then(|d| d.get("status")),
    ] {
        if let Some(status) = candidate.and_then(|s| s.as_str()) {
            if !status.is_empty() {
                return status.to_ascii_lowercase();
            }
        }
    }
    "unknown".to_string()
}

fn is_terminal_success(status: &str) -> bool {
    matches!(
        status,
        "success" | "succeeded" | "completed" | "done" | "finished"
    )
}

fn is_terminal_failure(status: &str) -> bool {
    matches!(status, "failed" | "error" | "cancelled" | "canceled")
}

fn extract_task_id(value: &serde_json::Value) -> Option<String> {
    if let Some(id) = value.get("task_id") {
        if let Some(id) = id.as_i64() {
            return Some(id.to_string());
        }
        if let Some(id) = id.as_str().filter(|s| !s.is_empty()) {
            return Some(id.to_string());
        }
    }
    value
        .get("task")
        .and_then(|t| t.get("task_id"))
        .and_then(|id| match id {
            serde_json::Value::Number(n) => n.as_i64().map(|n| n.to_string()),
            serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        })
}

fn join_transcript_text(value: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(results) = value.get("results").and_then(|r| r.as_array()) {
        for item in results {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                let text = text.trim();
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
        }
    }
    if parts.is_empty() {
        // Tolerate single-field shapes some deployments return.
        for key in ["text", "transcript"] {
            if let Some(text) = value.get(key).and_then(|t| t.as_str()) {
                let text = text.trim();
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }
    }
    parts.join(" ")
}

#[async_trait]
impl AsrProvider for U2Asr {
    fn name(&self) -> &'static str {
        "u2-asr"
    }

    fn capabilities(&self) -> AsrCapabilities {
        Self::capabilities()
    }

    async fn transcribe(&self, req: TranscribeRequest) -> Result<Transcript> {
        if req.audio.is_empty() {
            return Err(Error::Audio("no audio to transcribe".into()));
        }
        let started = Instant::now();
        let file_id = self.upload(req.audio.clone()).await?;
        let task_id = self.create_task(&file_id, &req).await?;
        let final_resp = self.poll(&task_id).await?;
        let text = join_transcript_text(&final_resp);
        if text.is_empty() {
            return Err(Error::ProviderResponse(format!(
                "u2-asr task {task_id} finished without transcript text: {}",
                final_resp.to_string().chars().take(300).collect::<String>()
            )));
        }
        let language = final_resp
            .get("language")
            .and_then(|l| l.as_str())
            .or(req.language.as_deref())
            .map(|l| l.to_string());
        Ok(Transcript {
            text,
            language,
            duration_ms: final_resp
                .get("duration")
                .and_then(|d| d.as_f64())
                .map(|d| (d * 1000.0) as u64)
                .or_else(|| Some(started.elapsed().as_millis() as u64)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::HotwordRules;

    #[test]
    fn capabilities_match_u2_limits() {
        let caps = U2Asr::capabilities();
        assert_eq!(
            caps.hotwords,
            HotwordRules::List {
                max_items: 200,
                max_item_chars: 5
            }
        );
        assert_eq!(caps.max_context_chars, Some(500));
        assert_eq!(caps.languages.len(), 15);
    }

    #[test]
    fn status_detection_prefers_top_level() {
        let v = json!({"status": "running", "task": {"status": "success"}});
        assert_eq!(detect_status(&v), "running");
        let v = json!({"task": {"status": "SUCCESS"}});
        assert_eq!(detect_status(&v), "success");
        assert_eq!(detect_status(&json!({})), "unknown");
    }

    #[test]
    fn pending_statuses_do_not_terminate() {
        for status in PENDING_STATUSES {
            assert!(!is_terminal_success(status));
            assert!(!is_terminal_failure(status));
        }
        assert!(is_terminal_success("succeeded"));
        assert!(is_terminal_failure("failed"));
    }

    #[test]
    fn transcript_join_handles_both_shapes() {
        let v = json!({"results": [{"text": " hello "}, {"text": "world"}]});
        assert_eq!(join_transcript_text(&v), "hello world");
        let v = json!({"text": "direct"});
        assert_eq!(join_transcript_text(&v), "direct");
        let v = json!({"results": []});
        assert_eq!(join_transcript_text(&v), "");
    }

    #[test]
    fn api_error_surfaces_status_message() {
        let v = json!({"base_resp": {"status_code": 40001, "status_msg": "bad parameter"}});
        let err = U2Asr::ensure_api_success("task create", &v).unwrap_err();
        match err {
            Error::Provider { message, .. } => {
                assert!(message.contains("bad parameter"));
                assert!(message.contains("task create"));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(U2Asr::ensure_api_success("ok", &json!({"base_resp": {"status_code": 0}})).is_ok());
        assert!(U2Asr::ensure_api_success("ok", &json!({})).is_ok());
    }

    #[test]
    fn task_id_extraction_variants() {
        assert_eq!(
            extract_task_id(&json!({"task_id": 7})).as_deref(),
            Some("7")
        );
        assert_eq!(
            extract_task_id(&json!({"task": {"task_id": "abc"}})).as_deref(),
            Some("abc")
        );
        assert!(extract_task_id(&json!({})).is_none());
    }

    #[test]
    fn url_encoding_escapes_reserved_chars() {
        assert_eq!(url_encode("abc-123"), "abc-123");
        assert_eq!(url_encode("a/b c"), "a%2Fb%20c");
    }
}
