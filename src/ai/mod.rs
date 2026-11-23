//! Anbindung an eine OpenAI-kompatible KI, die Bilder beschreibt.

mod anthropic;
pub mod provider;
pub mod secret;

use crate::i18n::{Lang, t, tf};
use provider::Protocol;
use crate::texts::{ImageTexts, MAX_ALT, MAX_DESCRIPTION, MAX_TITLE};
use base64::Engine;
use serde_json::{Value, json};
use std::fmt;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct AiConfig {
    /// Basis-URL, z. B. `http://localhost:1234/v1`.
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    /// Sprache der Texte und Regeln für den Dateinamen.
    pub lang: Lang,
    /// OpenAI-kompatibel oder Anthropic.
    pub protocol: Protocol,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    NotConfigured,
    Unreachable(String),
    Timeout,
    Http(u16, String),
    BadResponse(String),
    /// Das Modell hat die Beschreibung abgelehnt.
    Refused,
    KeyRejected,
    NoCredit,
    ModelNotFound,
    NoVision,
    /// Lokaler Anbieter (Ollama, LM Studio, eigener Rechner) antwortet nicht.
    LocalNotRunning,
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NotConfigured => f.write_str(t("ai.error.not_configured")),
            AiError::Unreachable(e) => f.write_str(&tf("ai.error.unreachable", &[("e", e)])),
            AiError::Timeout => f.write_str(t("ai.error.timeout")),
            AiError::Http(code, e) => f.write_str(&tf("ai.error.http", &[("code", code), ("e", e)])),
            AiError::BadResponse(e) => f.write_str(&tf("ai.error.bad_response", &[("e", e)])),
            AiError::Refused => f.write_str(t("ai.error.refused")),
            AiError::KeyRejected => f.write_str(t("ai.error.key_rejected")),
            AiError::NoCredit => f.write_str(t("ai.error.no_credit")),
            AiError::ModelNotFound => f.write_str(t("ai.error.model_not_found")),
            AiError::NoVision => f.write_str(t("ai.error.no_vision")),
            AiError::LocalNotRunning => f.write_str(t("ai.error.local_not_running")),
        }
    }
}

impl std::error::Error for AiError {}

pub const TIMEOUT: Duration = Duration::from_secs(120);
/// Fehlertexte vom Server werden auf diese Länge gekürzt.
const MAX_ERROR_TEXT: usize = 200;

impl AiConfig {
    fn url(&self, path: &str) -> Result<String, AiError> {
        let base = self.base_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err(AiError::NotConfigured);
        }
        Ok(format!("{base}/{path}"))
    }

    fn key(&self) -> Option<&str> {
        self.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty())
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into()
}

fn transport_error(e: ureq::Error) -> AiError {
    match e {
        ureq::Error::Timeout(_) => AiError::Timeout,
        other => AiError::Unreachable(other.to_string()),
    }
}

/// Liest den Antworttext; Fehlerstatus werden eingeordnet, der Schlüssel nie mit ausgegeben.
fn read_body(response: ureq::http::Response<ureq::Body>, key: Option<&str>) -> Result<String, AiError> {
    read_raw(response)?.map_err(|(status, text)| http_error(status, &text, key))
}

/// Antworttext oder (Status, vollständiger Fehlertext) – noch nicht eingeordnet.
fn read_raw(mut response: ureq::http::Response<ureq::Body>) -> Result<Result<String, (u16, String)>, AiError> {
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(transport_error)?;
    Ok(if (200..300).contains(&status) { Ok(text) } else { Err((status, text)) })
}

fn http_error(status: u16, text: &str, key: Option<&str>) -> AiError {
    classify(status, text).unwrap_or_else(|| AiError::Http(status, redact(text, key)))
}

/// Häufige Fehler der Anbieter in verständliche Ursachen übersetzen.
pub fn classify(status: u16, text: &str) -> Option<AiError> {
    let text = text.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    match status {
        401 | 403 => Some(AiError::KeyRejected),
        _ if status == 402 || has(&["credit", "quota", "billing", "insufficient"]) => Some(AiError::NoCredit),
        404 if text.contains("model") => Some(AiError::ModelNotFound),
        400 if has(&["image", "vision", "multimodal"]) => Some(AiError::NoVision),
        _ => None,
    }
}

/// Kürzt einen Fehlertext und ersetzt einen enthaltenen Schlüssel durch `***`.
pub fn redact(text: &str, key: Option<&str>) -> String {
    let text = match key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => text.replace(k, "***"),
        None => text.to_string(),
    };
    text.chars().take(MAX_ERROR_TEXT).collect()
}

/// „Nicht erreichbar“ bei einer lokalen Adresse heißt meist: Programm läuft nicht.
fn localize(e: AiError, cfg: &AiConfig) -> AiError {
    match e {
        AiError::Unreachable(_) if provider::is_local_url(&cfg.base_url) => AiError::LocalNotRunning,
        other => other,
    }
}

/// Höchstens ein erneuter Versuch, wenn der Server gar nicht erreichbar war.
fn with_retry<T>(mut call: impl FnMut() -> Result<T, AiError>) -> Result<T, AiError> {
    match call() {
        Err(AiError::Unreachable(_)) => {
            std::thread::sleep(Duration::from_millis(500));
            call()
        }
        other => other,
    }
}

/// Modelle, die der Anbieter anbietet.
pub fn list_models(cfg: &AiConfig) -> Result<Vec<String>, AiError> {
    match cfg.protocol {
        Protocol::OpenAiCompatible => openai_list_models(cfg),
        Protocol::Anthropic => anthropic::list_models(cfg),
    }
    .map_err(|e| localize(e, cfg))
}

/// Liste `data[].id`, wie sie OpenAI-kompatible Server und Anthropic liefern.
fn model_ids(text: &str) -> Result<Vec<String>, AiError> {
    let json: Value = serde_json::from_str(text).map_err(|e| AiError::BadResponse(e.to_string()))?;
    let models = json["data"]
        .as_array()
        .ok_or_else(|| AiError::BadResponse(t("ai.error.no_model_list").into()))?
        .iter()
        .filter_map(|m| m["id"].as_str().map(String::from))
        .collect();
    Ok(models)
}

fn openai_list_models(cfg: &AiConfig) -> Result<Vec<String>, AiError> {
    let url = cfg.url("models")?;
    let text = with_retry(|| {
        let mut request = agent().get(&url);
        if let Some(key) = cfg.key() {
            request = request.header("Authorization", &format!("Bearer {key}"));
        }
        read_body(request.call().map_err(transport_error)?, cfg.key())
    })?;
    model_ids(&text)
}

fn prompt(hint: &str, lang: Lang) -> String {
    let hint = hint.trim();
    if lang == Lang::En {
        let context = if hint.is_empty() { String::new() } else { format!("Note from the client: {hint}\n") };
        return format!(
            "Describe this photo for a website so that it is found on Google. Answer in English.\n\
{context}\
Use the note for technical terms, company names and location, but do not invent anything that is not visible.\n\
dateiname: 3 to 6 English words, lower case, separated by hyphens, without extension.\n\
titel: at most {MAX_TITLE} characters.\n\
alt_text: at most {MAX_ALT} characters, describes factually what is visible, without \"image of\" or \"photo of\".\n\
beschreibung: 1 to 2 sentences, at most {MAX_DESCRIPTION} characters."
        );
    }
    let context = if hint.is_empty() { String::new() } else { format!("Hinweis des Auftraggebers: {hint}\n") };
    format!(
        "Beschreibe dieses Foto für eine Website, damit es bei Google gefunden wird. Antworte auf Deutsch.\n\
{context}\
Nutze den Hinweis für Fachbegriffe, Firmennamen und Ort, aber erfinde nichts, was nicht zu sehen ist.\n\
dateiname: 3 bis 6 deutsche Wörter, klein geschrieben, mit Bindestrichen, ohne Endung.\n\
titel: höchstens {MAX_TITLE} Zeichen.\n\
alt_text: höchstens {MAX_ALT} Zeichen, beschreibt sachlich, was zu sehen ist, ohne „Bild von“ oder „Foto von“.\n\
beschreibung: 1 bis 2 Sätze, höchstens {MAX_DESCRIPTION} Zeichen."
    )
}

/// Felder, die manche OpenAI-kompatiblen Server ablehnen.
#[derive(Clone, Copy)]
struct Optional {
    schema: bool,
    temperature: bool,
}

fn request_body(cfg: &AiConfig, jpeg: &[u8], hint: &str, opt: Optional) -> Value {
    let image = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(jpeg));
    let mut body = json!({
        "model": cfg.model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": prompt(hint, cfg.lang) },
                { "type": "image_url", "image_url": { "url": image } }
            ]
        }]
    });
    if opt.temperature {
        body["temperature"] = json!(0.2);
    }
    if opt.schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": "bildtexte", "strict": true, "schema": schema() }
        });
    }
    body
}

/// (Basis-Adresse, Feld) – Felder, die ein Server abgelehnt hat (für die Laufzeit gemerkt).
static REJECTED: std::sync::Mutex<Vec<(String, &'static str)>> = std::sync::Mutex::new(Vec::new());

fn rejected(base: &str, field: &str) -> bool {
    REJECTED.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|(b, f)| b == base && *f == field)
}

fn remember_rejected(base: &str, field: &'static str) {
    let mut list = REJECTED.lock().unwrap_or_else(|p| p.into_inner());
    if !list.iter().any(|(b, f)| b == base && *f == field) {
        list.push((base.to_string(), field));
    }
}

/// Welches optionale Feld lehnt der Server mit diesem 400-Fehler ab? Geprüft am vollständigen Text,
/// vor jeder Einordnung (sonst würde z. B. „… for image inputs“ als „kann keine Bilder“ enden).
fn rejected_field(status: u16, text: &str, opt: Optional) -> Option<&'static str> {
    if status != 400 {
        return None;
    }
    let text = text.to_ascii_lowercase();
    if opt.schema && (text.contains("response_format") || text.contains("json_schema")) {
        Some("schema")
    } else if opt.temperature && text.contains("temperature") {
        Some("temperature")
    } else {
        None
    }
}

/// Antwortformat: die vier Textfelder.
fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "dateiname": { "type": "string" },
            "titel": { "type": "string" },
            "alt_text": { "type": "string" },
            "beschreibung": { "type": "string" }
        },
        "required": ["dateiname", "titel", "alt_text", "beschreibung"],
        "additionalProperties": false
    })
}

/// Lässt ein Bild beschreiben (JPEG, bereits verkleinert).
pub fn describe(cfg: &AiConfig, jpeg: &[u8], hint: &str) -> Result<ImageTexts, AiError> {
    match cfg.protocol {
        Protocol::OpenAiCompatible => openai_describe(cfg, jpeg, hint),
        Protocol::Anthropic => anthropic::describe(cfg, jpeg, hint),
    }
    .map_err(|e| localize(e, cfg))
}

fn openai_describe(cfg: &AiConfig, jpeg: &[u8], hint: &str) -> Result<ImageTexts, AiError> {
    let url = cfg.url("chat/completions")?;
    let base = cfg.url("")?;
    let send = |body: &Value| {
        with_retry(|| {
            let mut request = agent().post(&url);
            if let Some(key) = cfg.key() {
                request = request.header("Authorization", &format!("Bearer {key}"));
            }
            read_raw(request.send_json(body).map_err(transport_error)?)
        })
    };
    let mut opt = Optional { schema: !rejected(&base, "schema"), temperature: !rejected(&base, "temperature") };
    // Lehnt der Server ein optionales Feld ab, ohne dieses Feld erneut fragen (je Feld höchstens einmal);
    // der Prompt verlangt ohnehin JSON.
    let text = loop {
        match send(&request_body(cfg, jpeg, hint, opt))? {
            Ok(text) => break text,
            Err((status, text)) => match rejected_field(status, &text, opt) {
                Some(field) => {
                    remember_rejected(&base, field);
                    match field {
                        "schema" => opt.schema = false,
                        _ => opt.temperature = false,
                    }
                }
                None => return Err(http_error(status, &text, cfg.key())),
            },
        }
    };
    let json: Value = serde_json::from_str(&text).map_err(|e| AiError::BadResponse(e.to_string()))?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| AiError::BadResponse(t("ai.error.no_message").into()))?;
    parse_reply(content, cfg.lang)
}

/// Erstes vollständiges `{…}` im Text, unter Beachtung von Zeichenketten.
fn first_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (i, c) in text[start..].char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn parse_reply(content: &str, lang: Lang) -> Result<ImageTexts, AiError> {
    let value: Value = serde_json::from_str(content.trim())
        .or_else(|_| {
            first_json_object(content)
                .ok_or_else(|| AiError::BadResponse(t("ai.error.no_json").into()))
                .and_then(|j| serde_json::from_str(j).map_err(|e| AiError::BadResponse(e.to_string())))
        })?;
    let field = |name: &str| -> Result<String, AiError> {
        value[name].as_str().map(String::from).ok_or_else(|| AiError::BadResponse(tf("ai.error.missing_field", &[("name", &name)])))
    };
    let texts = ImageTexts {
        file_stem: field("dateiname")?,
        title: field("titel")?,
        alt: field("alt_text")?,
        description: field("beschreibung")?,
    };
    Ok(texts.normalized_in(lang))
}

/// Lässt ein kleines erzeugtes Bild beschreiben und misst die Zeit.
pub fn test_connection(cfg: &AiConfig) -> Result<Duration, AiError> {
    let img = image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([(x * 4) as u8, (y * 4) as u8, 160]));
    let mut jpeg = Vec::new();
    img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80))
        .map_err(|e| AiError::BadResponse(e.to_string()))?;
    let start = Instant::now();
    describe(cfg, &jpeg, t("ai.test_hint"))?;
    Ok(start.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_maps_common_errors() {
        assert_eq!(classify(401, "nope"), Some(AiError::KeyRejected));
        assert_eq!(classify(403, ""), Some(AiError::KeyRejected));
        assert_eq!(classify(402, ""), Some(AiError::NoCredit));
        assert_eq!(classify(429, "You exceeded your current quota"), Some(AiError::NoCredit));
        assert_eq!(classify(400, "Your credit balance is too low"), Some(AiError::NoCredit));
        assert_eq!(classify(404, "model 'x' not found"), Some(AiError::ModelNotFound));
        assert_eq!(classify(400, "this model does not support image input"), Some(AiError::NoVision));
        assert_eq!(classify(400, "Vision is not enabled"), Some(AiError::NoVision));
        assert_eq!(classify(500, "boom"), None);
        assert_eq!(classify(404, "not here"), None);
    }

    #[test]
    fn redact_hides_key_and_shortens() {
        assert_eq!(redact("bad key sk-abc123 given", Some("sk-abc123")), "bad key *** given");
        assert_eq!(redact("x", None), "x");
        assert_eq!(redact(&"a".repeat(500), None).chars().count(), 200);
    }
}
