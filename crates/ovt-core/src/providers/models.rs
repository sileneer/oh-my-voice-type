//! Live model discovery via provider `GET /models` endpoints.
//!
//! Supported dialects:
//! - **OpenAI-compatible** (`GET {base_url}/models` with `Authorization: Bearer <key>`),
//!   covering OpenAI, Groq, DeepSeek, Mistral, Moonshot, OpenRouter, SiliconFlow,
//!   StepFun, Together AI, xAI, Alibaba DashScope, Baidu Qianfan, Volcengine Ark,
//!   Zhipu GLM, Google Gemini OpenAI compat, Cohere OpenAI compat, MiniMax,
//!   Unisound LLM, and self-hosted gateways (Ollama, vLLM, One-API, etc.).
//! - **Anthropic Messages** (`GET {base_url}/v1/models?limit=100` with
//!   `x-api-key` + `anthropic-version: 2023-06-01`).

use super::{
    direct_http_client, format_network_error, http_client, map_http_error,
    send_with_proxy_fallback, with_bearer,
};
use crate::config::require_http_url;
use crate::error::{Error, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelListRole {
    Asr,
    Llm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelListProtocol {
    OpenAi,
    Anthropic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelListOutcome {
    /// Live models were returned for the requested role.
    Ok,
    /// The endpoint responded with 404/405/501 (does not expose `/models`).
    Unsupported,
    /// The endpoint returned a catalog, but none matched the requested role
    /// (for example, `/models` only lists chat models when configuring ASR).
    NoMatchingModels,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub outcome: ModelListOutcome,
    pub models: Vec<ProviderModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelListRequest {
    pub role: ModelListRole,
    pub protocol: ModelListProtocol,
    pub base_url: String,
    pub allow_insecure_http: bool,
    /// Allow falling back to the unfiltered model catalog when heuristics match
    /// nothing (intended for custom / self-hosted servers with arbitrary names).
    #[serde(default)]
    pub allow_unfiltered_fallback: bool,
}

/// Build the provider's model-listing URL, applying host-specific query filters
/// documented by providers that expose categorized `/models` catalogs.
pub fn build_models_url(req: &ModelListRequest) -> Result<String> {
    build_models_target(req).map(|(url, _)| url)
}

fn build_models_target(req: &ModelListRequest) -> Result<(String, bool)> {
    let trimmed = req.base_url.trim().trim_end_matches('/');
    require_http_url("models base_url", trimmed, req.allow_insecure_http)?;
    let parsed = Url::parse(trimmed).map_err(|e| Error::Config(format!("invalid URL: {e}")))?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();

    let (url, pre_filtered) = match req.protocol {
        ModelListProtocol::Anthropic => {
            let url = if trimmed.ends_with("/v1") {
                format!("{trimmed}/models?limit=100")
            } else {
                format!("{trimmed}/v1/models?limit=100")
            };
            (url, false)
        }
        ModelListProtocol::OpenAi => {
            let is_root_path = parsed.path().trim_matches('/').is_empty();
            let base = if is_root_path {
                format!("{trimmed}/v1/models")
            } else {
                format!("{trimmed}/models")
            };
            if host.ends_with("siliconflow.cn") || host.ends_with("siliconflow.com") {
                let url = match req.role {
                    ModelListRole::Asr => format!("{base}?sub_type=speech-to-text"),
                    ModelListRole::Llm => format!("{base}?sub_type=chat"),
                };
                (url, true)
            } else if host == "openrouter.ai" && req.role == ModelListRole::Asr {
                (format!("{base}?output_modalities=transcription"), true)
            } else {
                (base, false)
            }
        }
    };
    Ok((url, pre_filtered))
}

#[derive(Debug)]
struct RawModelEntry {
    id: String,
    label: Option<String>,
    kind: Option<String>,
}

/// Parse a `GET /models` JSON response from OpenAI-compatible, Anthropic,
/// Together, Gemini, or custom gateways and filter appropriately for `role`.
pub fn parse_models_response(body: &str, role: ModelListRole) -> Result<Vec<ProviderModel>> {
    parse_models_response_with_options(body, role, false, true).map(|res| res.models)
}

fn parse_models_response_with_options(
    body: &str,
    role: ModelListRole,
    pre_filtered: bool,
    allow_unfiltered_fallback: bool,
) -> Result<ModelListResponse> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| Error::ProviderResponse(body.chars().take(300).collect()))?;

    let items = if let Some(arr) = value.get("data").and_then(Value::as_array) {
        arr
    } else if let Some(arr) = value.as_array() {
        arr
    } else if let Some(arr) = value.get("models").and_then(Value::as_array) {
        arr
    } else {
        return Err(Error::ProviderResponse(
            "models response did not contain a model list".into(),
        ));
    };

    let mut seen = HashSet::new();
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let raw_id = item
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| item.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(raw_id) = raw_id else {
            continue;
        };
        let id = raw_id.strip_prefix("models/").unwrap_or(raw_id).to_string();
        if !seen.insert(id.clone()) {
            continue;
        }
        let label = item
            .get("display_name")
            .and_then(Value::as_str)
            .or_else(|| item.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|l| !l.is_empty() && *l != id && *l != raw_id)
            .map(str::to_string);
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .map(|t| t.to_ascii_lowercase());
        entries.push(RawModelEntry { id, label, kind });
    }

    let selected: Vec<&RawModelEntry> = if pre_filtered {
        entries.iter().collect()
    } else {
        let filtered: Vec<&RawModelEntry> = match role {
            ModelListRole::Asr => entries
                .iter()
                .filter(|entry| is_asr_model(&entry.id, entry.kind.as_deref()))
                .collect(),
            ModelListRole::Llm => entries
                .iter()
                .filter(|entry| !is_non_chat_model(&entry.id, entry.kind.as_deref()))
                .collect(),
        };

        // For custom / self-hosted servers (e.g. local Whisper serving `large-v3`),
        // fall back to the full list when heuristics match nothing. For known
        // preset endpoints, avoid polluting an ASR dropdown with chat LLM models.
        if filtered.is_empty() && allow_unfiltered_fallback {
            entries.iter().collect()
        } else {
            filtered
        }
    };

    let mut models: Vec<ProviderModel> = selected
        .into_iter()
        .map(|entry| ProviderModel {
            id: entry.id.clone(),
            label: entry.label.clone(),
        })
        .collect();
    models.sort_by_key(|a| a.id.to_ascii_lowercase());
    let outcome = if models.is_empty() {
        ModelListOutcome::NoMatchingModels
    } else {
        ModelListOutcome::Ok
    };
    Ok(ModelListResponse { outcome, models })
}

fn is_asr_model(id: &str, kind: Option<&str>) -> bool {
    let lc = id.to_ascii_lowercase();
    if lc.contains("tts")
        || lc.contains("text-to-speech")
        || lc.contains("realtime")
        || lc.contains("embed")
        || lc.contains("rerank")
    {
        return false;
    }
    if matches!(kind, Some("audio" | "asr" | "stt" | "transcription")) {
        return true;
    }
    [
        "whisper",
        "transcribe",
        "asr",
        "voxtral",
        "sensevoice",
        "telespeech",
        "paraformer",
        "parakeet",
        "scribe-v",
        "chirp-",
        "speech-to-text",
        "stt",
    ]
    .iter()
    .any(|kw| lc.contains(kw))
}

fn is_non_chat_model(id: &str, kind: Option<&str>) -> bool {
    if matches!(
        kind,
        Some("embedding" | "image" | "audio" | "moderation" | "rerank" | "video" | "asr" | "tts")
    ) {
        return true;
    }
    let lc = id.to_ascii_lowercase();
    [
        "embedding",
        "embed-",
        "-embed",
        "rerank",
        "whisper",
        "transcribe",
        "tts",
        "dall-e",
        "imagen",
        "sora",
        "moderation",
        "flux",
        "stable-diffusion",
        "sensevoice",
        "telespeech",
        "paraformer",
        "parakeet",
        "scribe-v",
        "chirp-",
        "-asr",
        "asr-",
        "-stt",
        "voxtral",
        "-ocr",
        "ocr-",
        "text-to-image",
        "text-to-video",
        "image-to-",
        "veo-",
        "kling-",
        "babbage-",
        "davinci-",
        "realtime",
        "audio-preview",
        "imagine-image",
        "grok-voice",
    ]
    .iter()
    .any(|kw| lc.contains(kw))
}

/// Query the provider's live `/models` endpoint using the supplied API key.
pub async fn fetch_models(req: &ModelListRequest, api_key: &str) -> Result<ModelListResponse> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(Error::Config("API key is required to list models".into()));
    }
    if !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::Config(
            "API key contains invalid characters or whitespace".into(),
        ));
    }
    let (url, pre_filtered) = build_models_target(req)?;
    let client = http_client();
    let direct = direct_http_client();
    let resp = send_with_proxy_fallback(&client, &direct, "list models", |http| {
        Ok(match req.protocol {
            ModelListProtocol::OpenAi => with_bearer(http.get(&url), key),
            ModelListProtocol::Anthropic => http
                .get(&url)
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
        })
    })
    .await?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| Error::Network(format_network_error("list models body", &e)))?;
    if !status.is_success() {
        if matches!(status.as_u16(), 404 | 405 | 501) {
            return Ok(ModelListResponse {
                outcome: ModelListOutcome::Unsupported,
                models: Vec::new(),
            });
        }
        return Err(map_http_error("models", status.as_u16(), &text));
    }
    parse_models_response_with_options(&text, req.role, pre_filtered, req.allow_unfiltered_fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(role: ModelListRole, protocol: ModelListProtocol, base_url: &str) -> ModelListRequest {
        ModelListRequest {
            role,
            protocol,
            base_url: base_url.into(),
            allow_insecure_http: false,
            allow_unfiltered_fallback: false,
        }
    }

    #[test]
    fn builds_provider_specific_urls() {
        let openai = build_models_url(&req(
            ModelListRole::Llm,
            ModelListProtocol::OpenAi,
            "https://api.openai.com/v1/",
        ))
        .unwrap();
        assert_eq!(openai, "https://api.openai.com/v1/models");

        let root_host = build_models_url(&req(
            ModelListRole::Asr,
            ModelListProtocol::OpenAi,
            "https://api.example.com",
        ))
        .unwrap();
        assert_eq!(root_host, "https://api.example.com/v1/models");

        let versioned = build_models_url(&req(
            ModelListRole::Llm,
            ModelListProtocol::OpenAi,
            "https://open.bigmodel.cn/api/paas/v4",
        ))
        .unwrap();
        assert_eq!(versioned, "https://open.bigmodel.cn/api/paas/v4/models");

        let anthropic = build_models_url(&req(
            ModelListRole::Llm,
            ModelListProtocol::Anthropic,
            "https://api.anthropic.com",
        ))
        .unwrap();
        assert_eq!(anthropic, "https://api.anthropic.com/v1/models?limit=100");

        let sf_asr = build_models_url(&req(
            ModelListRole::Asr,
            ModelListProtocol::OpenAi,
            "https://api.siliconflow.cn/v1",
        ))
        .unwrap();
        assert_eq!(
            sf_asr,
            "https://api.siliconflow.cn/v1/models?sub_type=speech-to-text"
        );

        let sf_llm = build_models_url(&req(
            ModelListRole::Llm,
            ModelListProtocol::OpenAi,
            "https://api.siliconflow.com/v1",
        ))
        .unwrap();
        assert_eq!(
            sf_llm,
            "https://api.siliconflow.com/v1/models?sub_type=chat"
        );

        let or_asr = build_models_url(&req(
            ModelListRole::Asr,
            ModelListProtocol::OpenAi,
            "https://openrouter.ai/api/v1",
        ))
        .unwrap();
        assert_eq!(
            or_asr,
            "https://openrouter.ai/api/v1/models?output_modalities=transcription"
        );
    }

    #[test]
    fn filters_asr_and_llm_models_from_mixed_openai_catalog() {
        let payload = r#"{
            "object": "list",
            "data": [
                {"id": "gpt-5.4-mini"},
                {"id": "gpt-transcribe"},
                {"id": "whisper-1"},
                {"id": "tts-1"},
                {"id": "text-embedding-3-small"},
                {"id": "dall-e-3"},
                {"id": "gpt-4o-mini-transcribe"}
            ]
        }"#;

        let asr = parse_models_response(payload, ModelListRole::Asr).unwrap();
        let asr_ids: Vec<&str> = asr.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            asr_ids,
            vec!["gpt-4o-mini-transcribe", "gpt-transcribe", "whisper-1"]
        );

        let llm = parse_models_response(payload, ModelListRole::Llm).unwrap();
        let llm_ids: Vec<&str> = llm.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(llm_ids, vec!["gpt-5.4-mini"]);
    }

    #[test]
    fn parses_anthropic_display_names_and_strips_gemini_models_prefix() {
        let anthropic = r#"{
            "data": [
                {"id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5"},
                {"id": "claude-sonnet-5-5", "display_name": "Claude Sonnet 5.5"}
            ]
        }"#;
        let parsed = parse_models_response(anthropic, ModelListRole::Llm).unwrap();
        assert_eq!(parsed[0].id, "claude-haiku-5-5");
        assert_eq!(parsed[0].label.as_deref(), Some("Claude Haiku 5.5"));

        let gemini = r#"{"data": [{"id": "models/gemini-3.1-flash-lite"}]}"#;
        let parsed = parse_models_response(gemini, ModelListRole::Llm).unwrap();
        assert_eq!(parsed[0].id, "gemini-3.1-flash-lite");
    }

    #[test]
    fn preset_asr_does_not_return_chat_models_when_no_asr_models_exist() {
        let chat_only = r#"{"data": [{"id": "chat-flash"}, {"id": "chat-pro"}]}"#;
        let preset_res =
            parse_models_response_with_options(chat_only, ModelListRole::Asr, false, false)
                .unwrap();
        assert_eq!(preset_res.outcome, ModelListOutcome::NoMatchingModels);
        assert!(preset_res.models.is_empty());

        let custom_res =
            parse_models_response_with_options(chat_only, ModelListRole::Asr, false, true).unwrap();
        assert_eq!(custom_res.outcome, ModelListOutcome::Ok);
        assert_eq!(custom_res.models.len(), 2);
    }

    #[test]
    fn falls_back_to_all_models_for_custom_self_hosted_asr_names() {
        let local_whisper = r#"[{"id": "large-v3"}, {"id": "medium"}]"#;
        let parsed = parse_models_response(local_whisper, ModelListRole::Asr).unwrap();
        let ids: Vec<&str> = parsed.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["large-v3", "medium"]);
    }

    #[test]
    fn respects_together_type_field_for_llm() {
        let together = r#"{
            "data": [
                {"id": "deepseek-ai/DeepSeek-V4.1-Flash", "type": "chat"},
                {"id": "BAAI/bge-large-en-v1.5", "type": "embedding"},
                {"id": "black-forest-labs/FLUX.1-schnell", "type": "image"}
            ]
        }"#;
        let parsed = parse_models_response(together, ModelListRole::Llm).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "deepseek-ai/DeepSeek-V4.1-Flash");
    }

    #[test]
    fn preserves_server_prefiltered_transcription_catalogs() {
        let openrouter = r#"{
            "data": [
                {"id": "moondream/parakeet-ultra", "name": "Parakeet Ultra"},
                {"id": "assemblyai/universal-3-6-pro", "name": "Universal 3.6 Pro"},
                {"id": "elevenlabs/scribe-v2", "name": "Scribe v2"},
                {"id": "openai/gpt-transcribe", "name": "GPT Transcribe"}
            ]
        }"#;
        let parsed =
            parse_models_response_with_options(openrouter, ModelListRole::Asr, true, false)
                .unwrap();
        assert_eq!(parsed.outcome, ModelListOutcome::Ok);
        let ids: Vec<&str> = parsed.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "assemblyai/universal-3-6-pro",
                "elevenlabs/scribe-v2",
                "moondream/parakeet-ultra",
                "openai/gpt-transcribe",
            ]
        );
    }

    #[tokio::test]
    async fn http_404_returns_unsupported_instead_of_error() {
        use std::io::{BufRead, BufWriter, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
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
                "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found"
            )
            .unwrap();
            writer.flush().unwrap();
        });

        let res = fetch_models(
            &ModelListRequest {
                role: ModelListRole::Asr,
                protocol: ModelListProtocol::OpenAi,
                base_url: format!("http://{addr}/v1"),
                allow_insecure_http: false,
                allow_unfiltered_fallback: false,
            },
            "sk-test-key-1234",
        )
        .await
        .unwrap();
        assert_eq!(res.outcome, ModelListOutcome::Unsupported);
        assert!(res.models.is_empty());
        server.join().unwrap();
    }
}
