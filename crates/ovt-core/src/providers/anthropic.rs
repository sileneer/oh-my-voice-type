//! Anthropic-compatible **Messages API** adapter (`POST {base}/v1/messages`).
//! Auth per Unisound docs: `x-api-key`; the standard `anthropic-version`
//! header is included for compatibility with api.anthropic.com.

use super::{http_client, map_http_error, CleanupRequest, LlmProvider};
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct AnthropicLlm {
    pub client: reqwest::Client,
    /// Host (e.g. https://api.anthropic.com) or Unisound's `/anthropic` prefix.
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl AnthropicLlm {
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

    fn payload(&self, req: &CleanupRequest) -> Value {
        json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": [{"role": "user", "content": req.user}],
            "temperature": req.temperature,
        })
    }
}

/// Join `text` content blocks from a Messages payload.
fn extract_message_text(value: &Value) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(content) = value.get("content").and_then(|c| c.as_array()) {
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    if !text.trim().is_empty() {
                        parts.push(text);
                    }
                }
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" ").trim().to_string())
    }
}

#[async_trait]
impl LlmProvider for AnthropicLlm {
    async fn complete(&self, req: CleanupRequest) -> Result<String> {
        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.payload(&req))
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;
        if !status.is_success() {
            return Err(map_http_error("llm (anthropic)", status.as_u16(), &text));
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| Error::ProviderResponse(text.chars().take(300).collect()))?;
        if let Some(err) = value.get("error") {
            return Err(Error::Provider {
                status: 200,
                message: format!("llm (anthropic): {err}"),
            });
        }
        extract_message_text(&value).ok_or_else(|| {
            Error::ProviderResponse(format!(
                "anthropic payload had no text block: {}",
                text.chars().take(300).collect::<String>()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_text_blocks_ignoring_thinking() {
        let v = json!({
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": "world."}
            ]
        });
        assert_eq!(extract_message_text(&v).as_deref(), Some("Hello world."));
    }

    #[test]
    fn rejects_payload_without_text() {
        assert!(extract_message_text(&json!({"content": []})).is_none());
    }

    #[test]
    fn payload_sets_system_and_messages() {
        let llm = AnthropicLlm::new("https://api.anthropic.com", "k", "u2-flash");
        let p = llm.payload(&CleanupRequest {
            system: "sys".into(),
            user: "usr".into(),
            max_tokens: 42,
            temperature: 0.2,
        });
        assert_eq!(p["system"], "sys");
        assert_eq!(p["messages"][0]["content"], "usr");
        assert_eq!(p["max_tokens"], 42);
    }
}
