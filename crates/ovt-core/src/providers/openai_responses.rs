//! OpenAI-compatible **Responses API** adapter (`POST {base}/responses`),
//! as offered by Unisound (`/v1/responses`) and OpenAI.
//!
//! Request shape: `instructions` carries the system prompt, `input` the user
//! text, `max_output_tokens` the cap. Output is a structured `output` array;
//! we join `output_text` blocks.

use super::{http_client, map_http_error, with_bearer, CleanupRequest, LlmProvider};
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct OpenAiResponsesLlm {
    pub client: reqwest::Client,
    /// Root including version, e.g. https://maas-api.unisound.com/v1
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl OpenAiResponsesLlm {
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
            "instructions": req.system,
            "input": [{"role": "user", "content": req.user}],
            "max_output_tokens": req.max_tokens,
            "temperature": req.temperature,
        })
    }
}

/// Join assistant text blocks from a Responses payload. Tolerates the
/// `output_text` convenience field some gateways add.
fn extract_output_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(|t| t.as_str()) {
        if !text.trim().is_empty() {
            return Some(text.trim().to_string());
        }
    }
    let mut parts: Vec<&str> = Vec::new();
    if let Some(output) = value.get("output").and_then(|o| o.as_array()) {
        for item in output {
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    let kind = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if kind == "output_text" || kind == "text" {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !text.trim().is_empty() {
                                parts.push(text);
                            }
                        }
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
impl LlmProvider for OpenAiResponsesLlm {
    async fn complete(&self, req: CleanupRequest) -> Result<String> {
        let resp = with_bearer(
            self.client
                .post(format!("{}/responses", self.base_url))
                .json(&self.payload(&req)),
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
            return Err(map_http_error("llm (responses)", status.as_u16(), &text));
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| Error::ProviderResponse(text.chars().take(300).collect()))?;
        if let Some(err) = value.get("error") {
            return Err(Error::Provider {
                status: 200,
                message: format!("llm (responses): {err}"),
            });
        }
        extract_output_text(&value).ok_or_else(|| {
            Error::ProviderResponse(format!(
                "responses payload had no output text: {}",
                text.chars().take(300).collect::<String>()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_output_blocks() {
        let v = json!({
            "output": [
                {"type": "reasoning", "summary": []},
                {"type": "message", "content": [
                    {"type": "output_text", "text": "Hello"},
                    {"type": "output_text", "text": "world."}
                ]}
            ]
        });
        assert_eq!(extract_output_text(&v).as_deref(), Some("Hello world."));
    }

    #[test]
    fn parses_convenience_output_text() {
        let v = json!({"output_text": "  direct text  "});
        assert_eq!(extract_output_text(&v).as_deref(), Some("direct text"));
    }

    #[test]
    fn rejects_payload_without_text() {
        assert!(extract_output_text(&json!({"output": []})).is_none());
    }

    #[test]
    fn payload_uses_instructions_and_input() {
        let llm = OpenAiResponsesLlm::new("https://x/v1", "k", "u2-flash");
        let p = llm.payload(&CleanupRequest {
            system: "sys".into(),
            user: "usr".into(),
            max_tokens: 100,
            temperature: 0.2,
        });
        assert_eq!(p["instructions"], "sys");
        assert_eq!(p["input"][0]["content"], "usr");
        assert_eq!(p["max_output_tokens"], 100);
    }
}
