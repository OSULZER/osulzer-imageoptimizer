//! Anthropic (Claude) über die eigene Messages-Schnittstelle.

use super::{AiConfig, AiError, agent, model_ids, parse_reply, prompt, read_body, schema, transport_error, with_retry};
use crate::texts::ImageTexts;
use base64::Engine;
use serde_json::{Value, json};

const VERSION: &str = "2026-10-21";
/// Großzügig, weil aktuelle Modelle vor der Antwort nachdenken und das mitzählt.
const MAX_TOKENS: u32 = 8000;

fn with_headers<B>(request: ureq::RequestBuilder<B>, cfg: &AiConfig) -> ureq::RequestBuilder<B> {
    let request = request.header("anthropic-version", VERSION);
    match cfg.key() {
        Some(key) => request.header("x-api-key", key),
        None => request,
    }
}

pub(super) fn list_models(cfg: &AiConfig) -> Result<Vec<String>, AiError> {
    let url = cfg.url("models?limit=100")?;
    let text = with_retry(|| read_body(with_headers(agent().get(&url), cfg).call().map_err(transport_error)?, cfg.key()))?;
    model_ids(&text)
}

fn request_body(cfg: &AiConfig, jpeg: &[u8], hint: &str) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(jpeg);
    json!({
        "model": cfg.model,
        "max_tokens": MAX_TOKENS,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": data } },
                { "type": "text", "text": prompt(hint, cfg.lang) }
            ]
        }],
        "output_config": { "format": { "type": "json_schema", "schema": schema() } }
    })
}

/// Text der Antwort: alle Text-Blöcke (Denk-Blöcke zählen nicht).
pub(super) fn reply_text(json: &Value) -> Result<String, AiError> {
    if json["stop_reason"] == "refusal" {
        return Err(AiError::Refused);
    }
    let text: String = json["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    if text.trim().is_empty() {
        return Err(AiError::BadResponse(crate::i18n::t("ai.error.no_message").into()));
    }
    Ok(text)
}

pub(super) fn describe(cfg: &AiConfig, jpeg: &[u8], hint: &str) -> Result<ImageTexts, AiError> {
    let url = cfg.url("messages")?;
    let body = request_body(cfg, jpeg, hint);
    let text = with_retry(|| read_body(with_headers(agent().post(&url), cfg).send_json(&body).map_err(transport_error)?, cfg.key()))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| AiError::BadResponse(e.to_string()))?;
    parse_reply(&reply_text(&json)?, cfg.lang)
}
