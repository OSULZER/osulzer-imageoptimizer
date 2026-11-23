//! Fehler und Meldungen gibt es auf Deutsch und Englisch.
//! Integrationstests stellen die Sprache nur auf Deutsch (siehe Plan, Review Focus 4).

use osulzer_imageoptimizer::ai::AiError;
use osulzer_imageoptimizer::engine::EngineError;
use osulzer_imageoptimizer::i18n::{Lang, set_lang, t_in};
use osulzer_imageoptimizer::job::{JobError, SaveNote};
use osulzer_imageoptimizer::state::{format_bytes, message_for};

#[test]
fn german_texts_are_unchanged() {
    set_lang(Lang::De);
    assert_eq!(EngineError::Unsupported.to_string(), "Kein unterstütztes Bildformat");
    assert_eq!(AiError::Timeout.to_string(), "KI hat nicht rechtzeitig geantwortet");
    let io = || std::io::Error::other("x");
    assert_eq!(JobError::Io(io()).to_string(), "Ziel nicht beschreibbar: x");
    assert_eq!(message_for(None, Some(SaveNote::Skipped)).unwrap(), "übersprungen (Datei vorhanden)");
    assert_eq!(format_bytes(10 * 1024 * 1024 + 200 * 1024), "10,2 MB");
}

#[test]
fn english_texts_exist_and_differ() {
    for key in ["error.unsupported", "ai.error.timeout", "job.error.io", "msg.skipped", "msg.target_size_missed", "error.embed_metadata", "error.jpeg_aborted"] {
        let (en, de) = (t_in(Lang::En, key), t_in(Lang::De, key));
        assert_ne!(en, key, "{key} fehlt");
        assert_ne!(en, de, "{key} nicht übersetzt");
    }
}
