mod common;

use osulzer_imageoptimizer::ai::{AiConfig, AiError, describe, list_models, parse_reply};
use osulzer_imageoptimizer::ai::provider::Protocol;
use osulzer_imageoptimizer::i18n::Lang;
use common::{chat_reply, fake_server};
use std::sync::{Arc, Mutex};

const GOOD: &str = r#"{"dateiname":"Grüner Ofen.JPG","titel":"Grüner Kachelofen","alt_text":"Ein grüner Kachelofen","beschreibung":"Ein Ofen."}"#;

fn cfg(base_url: String, key: Option<&str>) -> AiConfig {
    AiConfig { base_url, model: "test-modell".into(), api_key: key.map(String::from), lang: Lang::De, protocol: Protocol::OpenAiCompatible }
}

fn jpeg() -> Vec<u8> {
    common::jpeg_bytes(32, 24, 80)
}

#[test]
fn parse_reply_accepts_clean_json() {
    let t = parse_reply(GOOD, Lang::De).unwrap();
    assert_eq!(t.file_stem, "gruener-ofen-jpg");
    assert_eq!(t.title, "Grüner Kachelofen");
    assert_eq!(t.alt, "Ein grüner Kachelofen");
    assert_eq!(t.description, "Ein Ofen.");
}

#[test]
fn parse_reply_finds_json_inside_free_text() {
    let wrapped = format!("Hier ist das Ergebnis:\n```json\n{GOOD}\n```\nViel Erfolg!");
    assert_eq!(parse_reply(&wrapped, Lang::De).unwrap().title, "Grüner Kachelofen");
}

#[test]
fn parse_reply_rejects_missing_fields_and_garbage() {
    assert!(matches!(parse_reply(r#"{"titel":"x"}"#, Lang::De), Err(AiError::BadResponse(_))));
    assert!(matches!(parse_reply("keine Ahnung", Lang::De), Err(AiError::BadResponse(_))));
    assert!(matches!(parse_reply(r#"{"dateiname":"a","titel":"b","alt_text":"c""#, Lang::De), Err(AiError::BadResponse(_))));
}

#[test]
fn describe_sends_image_model_and_hint_without_key() {
    let seen = Arc::new(Mutex::new(None));
    let store = seen.clone();
    let base = fake_server(move |r| {
        *store.lock().unwrap() = Some((r.method.clone(), r.path.clone(), r.header("authorization").map(String::from), r.body.clone()));
        (200, chat_reply(GOOD))
    });
    let t = describe(&cfg(base, None), &jpeg(), "Hafnermeister Müller").unwrap();
    assert_eq!(t.file_stem, "gruener-ofen-jpg");
    let (method, path, auth, body) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(method, "POST");
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(auth, None);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["model"], "test-modell");
    assert!(body.contains("data:image/jpeg;base64,"));
    assert!(body.contains("Hafnermeister Müller"));
    assert_eq!(json["response_format"]["type"], "json_schema");
}

#[test]
fn describe_sends_bearer_key_and_trailing_slash_is_ignored() {
    let seen = Arc::new(Mutex::new(None));
    let store = seen.clone();
    let base = fake_server(move |r| {
        *store.lock().unwrap() = Some((r.path.clone(), r.header("authorization").map(String::from)));
        (200, chat_reply(GOOD))
    });
    describe(&cfg(format!("{base}/"), Some("geheim")), &jpeg(), "").unwrap();
    let (path, auth) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(auth.as_deref(), Some("Bearer geheim"));
}

#[test]
fn server_errors_are_reported_without_the_key() {
    osulzer_imageoptimizer::i18n::set_lang(osulzer_imageoptimizer::i18n::Lang::De);
    let base = fake_server(|_| (500, r#"{"error":"Modell kaputt"}"#.into()));
    let err = describe(&cfg(base, Some("geheim")), &jpeg(), "").unwrap_err();
    assert!(matches!(err, AiError::Http(500, _)), "{err:?}");
    let text = err.to_string();
    assert!(text.starts_with("KI-Fehler 500"), "{text}");
    assert!(!text.contains("geheim"));
}

#[test]
fn unusable_reply_is_bad_response() {
    let base = fake_server(|_| (200, chat_reply("Ich sehe ein Bild.")));
    assert!(matches!(describe(&cfg(base, None), &jpeg(), ""), Err(AiError::BadResponse(_))));
}

#[test]
fn unreachable_server_is_reported() {
    osulzer_imageoptimizer::i18n::set_lang(osulzer_imageoptimizer::i18n::Lang::De);
    // Freien Port holen und wieder schließen: dort lauscht niemand.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let err = describe(&cfg(format!("http://127.0.0.1:{port}/v1"), None), &jpeg(), "").unwrap_err();
    assert_eq!(err, AiError::LocalNotRunning);
    assert!(err.to_string().contains("LM Studio"), "{err}");
}

#[test]
fn empty_address_is_not_configured() {
    assert_eq!(describe(&cfg(String::new(), None), &jpeg(), "").unwrap_err(), AiError::NotConfigured);
}

#[test]
fn models_are_listed() {
    let base = fake_server(|r| {
        assert_eq!(r.path, "/v1/models");
        (200, r#"{"data":[{"id":"qwen/qwen3.8-27b"},{"id":"gemma"}]}"#.into())
    });
    assert_eq!(list_models(&cfg(base, None)).unwrap(), vec!["qwen/qwen3.8-27b", "gemma"]);
}

#[test]
fn old_config_without_ai_section_still_loads_and_key_is_never_stored() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.json");
    std::fs::write(&file, r#"{"user_presets":[],"current":{"max_edge":1920,"format":"Jpeg","quality":80,"max_kb":null},"target":"WebSubfolder","last_preset":null}"#).unwrap();
    let mut c = osulzer_imageoptimizer::config::load_from(&file);
    assert_eq!(c.ai, osulzer_imageoptimizer::config::AiSettings::default());
    c.ai.provider = osulzer_imageoptimizer::ai::provider::Provider::Custom;
    c.ai.set_active(osulzer_imageoptimizer::config::ProviderSettings { base_url: "http://x/v1".into(), model: "m".into() });
    osulzer_imageoptimizer::config::save_to(&file, &c).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("http://x/v1"));
    assert!(!text.to_lowercase().contains("key"));
}

#[test]
fn prompt_and_slug_follow_language() {
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let store = bodies.clone();
    let base = fake_server(move |r| {
        store.lock().unwrap().push(r.body.clone());
        (200, chat_reply(GOOD))
    });
    let en = AiConfig { lang: Lang::En, ..cfg(base.clone(), None) };
    assert_eq!(describe(&en, &jpeg(), "").unwrap().file_stem, "gruner-ofen-jpg");
    assert_eq!(describe(&cfg(base, None), &jpeg(), "").unwrap().file_stem, "gruener-ofen-jpg");
    let bodies = bodies.lock().unwrap();
    assert!(bodies[0].contains("Answer in English"), "englischer Prompt");
    assert!(bodies[1].contains("Antworte auf Deutsch"), "deutscher Prompt");
}

fn acfg(base_url: String, key: Option<&str>) -> AiConfig {
    AiConfig { protocol: Protocol::Anthropic, ..cfg(base_url, key) }
}

fn anthropic_reply(text: &str) -> String {
    serde_json::json!({
        "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": text}],
        "stop_reason": "end_turn"
    })
    .to_string()
}

#[test]
fn anthropic_request_has_headers_image_and_schema() {
    let seen = Arc::new(Mutex::new(None));
    let store = seen.clone();
    let base = fake_server(move |r| {
        *store.lock().unwrap() = Some((r.path.clone(), r.header("x-api-key").map(String::from), r.header("anthropic-version").map(String::from), r.header("authorization").map(String::from), r.body.clone()));
        (200, anthropic_reply(GOOD))
    });
    describe(&acfg(base, Some("geheim")), &jpeg(), "").unwrap();
    let (path, key, version, auth, body) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(path, "/v1/messages");
    assert_eq!(key.as_deref(), Some("geheim"));
    assert_eq!(version.as_deref(), Some("2026-10-21"));
    assert_eq!(auth, None);
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["model"], "test-modell");
    assert_eq!(body["max_tokens"], 8000);
    assert!(body.get("temperature").is_none());
    let content = body["messages"][0]["content"].as_array().unwrap();
    let image = content.iter().find(|c| c["type"] == "image").unwrap();
    assert_eq!(image["source"]["type"], "base64");
    assert_eq!(image["source"]["media_type"], "image/jpeg");
    assert!(image["source"]["data"].as_str().unwrap().len() > 100);
    assert!(content.iter().any(|c| c["type"] == "text"));
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    assert_eq!(body["output_config"]["format"]["schema"]["required"], serde_json::json!(["dateiname", "titel", "alt_text", "beschreibung"]));
}

#[test]
fn anthropic_reply_skips_thinking_blocks() {
    let base = fake_server(|_| (200, anthropic_reply(GOOD)));
    let t = describe(&acfg(base, Some("k")), &jpeg(), "").unwrap();
    assert_eq!(t.title, "Grüner Kachelofen");
}

#[test]
fn anthropic_refusal_is_reported() {
    let base = fake_server(|_| (200, r#"{"content":[],"stop_reason":"refusal"}"#.into()));
    assert_eq!(describe(&acfg(base, Some("k")), &jpeg(), "").unwrap_err(), AiError::Refused);
}

#[test]
fn anthropic_models_are_listed() {
    let base = fake_server(|r| {
        assert_eq!(r.path, "/v1/models?limit=100");
        assert_eq!(r.header("x-api-key"), Some("k"));
        assert_eq!(r.header("anthropic-version"), Some("2026-10-21"));
        (200, r#"{"data":[{"id":"claude-haiku-5-5","type":"model"},{"id":"claude-opus-5-5","type":"model"}],"has_more":false}"#.into())
    });
    assert_eq!(list_models(&acfg(base, Some("k"))).unwrap(), vec!["claude-haiku-5-5", "claude-opus-5-5"]);
}

#[test]
fn schema_rejected_then_retried_without() {
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let store = bodies.clone();
    let base = fake_server(move |r| {
        store.lock().unwrap().push(r.body.clone());
        if r.body.contains("response_format") {
            (400, r#"{"error":"response_format json_schema is not supported"}"#.into())
        } else {
            (200, chat_reply(GOOD))
        }
    });
    assert_eq!(describe(&cfg(base.clone(), None), &jpeg(), "").unwrap().title, "Grüner Kachelofen");
    assert_eq!(bodies.lock().unwrap().len(), 2);
    describe(&cfg(base, None), &jpeg(), "").unwrap();
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 3, "zweiter Aufruf fragt gleich ohne Struktur");
    assert!(!bodies[2].contains("response_format"));
}

#[test]
fn other_400_is_not_retried() {
    let count = Arc::new(Mutex::new(0));
    let c = count.clone();
    let base = fake_server(move |_| {
        *c.lock().unwrap() += 1;
        (400, r#"{"error":"bad model"}"#.into())
    });
    assert!(describe(&cfg(base, None), &jpeg(), "").is_err());
    assert_eq!(*count.lock().unwrap(), 1);
}

#[test]
fn schema_retry_happens_only_once() {
    let count = Arc::new(Mutex::new(0));
    let c = count.clone();
    let base = fake_server(move |_| {
        *c.lock().unwrap() += 1;
        (400, r#"{"error":"Response_Format not allowed"}"#.into())
    });
    assert!(describe(&cfg(base, None), &jpeg(), "").is_err());
    assert_eq!(*count.lock().unwrap(), 2);
}

#[test]
fn server_error_never_shows_key() {
    let base = fake_server(|_| (500, r#"{"error":"invalid header sk-geheim-123 received"}"#.into()));
    let err = describe(&cfg(base, Some("sk-geheim-123")), &jpeg(), "").unwrap_err();
    assert!(!format!("{err} {err:?}").contains("sk-geheim-123"), "{err:?}");
    assert!(err.to_string().contains("***"));
}

#[test]
fn rejected_key_is_explained() {
    let base = fake_server(|_| (401, r#"{"error":"invalid api key"}"#.into()));
    assert_eq!(describe(&cfg(base, Some("x")), &jpeg(), "").unwrap_err(), AiError::KeyRejected);
}

#[test]
fn schema_rejection_mentioning_images_is_still_retried() {
    let base = fake_server(|r| {
        if r.body.contains("response_format") {
            (400, r#"{"error":"response_format json_schema is not supported for image inputs"}"#.into())
        } else {
            (200, chat_reply(GOOD))
        }
    });
    assert_eq!(describe(&cfg(base, None), &jpeg(), "").unwrap().title, "Grüner Kachelofen");
}

#[test]
fn schema_rejection_after_long_text_is_still_retried() {
    let base = fake_server(|r| {
        if r.body.contains("response_format") {
            (400, format!(r#"{{"error":{{"provider":"{}","message":"param response_format not supported"}}}}"#, "x".repeat(300)))
        } else {
            (200, chat_reply(GOOD))
        }
    });
    assert!(describe(&cfg(base, None), &jpeg(), "").is_ok());
}

#[test]
fn temperature_rejection_is_retried_without_it() {
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let store = bodies.clone();
    let base = fake_server(move |r| {
        store.lock().unwrap().push(r.body.clone());
        if r.body.contains("temperature") {
            (400, r#"{"error":{"message":"Unsupported value: 'temperature' does not support 0.2 with this model."}}"#.into())
        } else {
            (200, chat_reply(GOOD))
        }
    });
    assert!(describe(&cfg(base, None), &jpeg(), "").is_ok());
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert!(bodies[1].contains("response_format"), "Struktur bleibt");
}
