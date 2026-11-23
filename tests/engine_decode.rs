mod common;

use osulzer_imageoptimizer::engine::{EngineError, decode, probe, target_size};
use osulzer_imageoptimizer::settings::SrcFormat;

#[test]
fn target_size_scales_longest_edge() {
    assert_eq!(target_size(6000, 4000, Some(1920)), (1920, 1280));
    assert_eq!(target_size(4000, 6000, Some(1920)), (1280, 1920));
}

#[test]
fn target_size_never_enlarges() {
    assert_eq!(target_size(800, 600, Some(1920)), (800, 600));
    assert_eq!(target_size(800, 600, None), (800, 600));
}

#[test]
fn target_size_extreme_aspect_keeps_at_least_one_pixel() {
    assert_eq!(target_size(1, 5000, Some(100)), (1, 100));
    assert_eq!(target_size(5000, 1, Some(100)), (100, 1));
}

#[test]
fn probe_reports_dimensions_after_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let p = common::write_jpeg_exif(dir.path(), "hoch.jpg", 80, 40, 6);
    let pr = probe(&p).unwrap();
    assert_eq!((pr.width, pr.height), (40, 80));
    assert_eq!(pr.format, SrcFormat::Jpeg);
    assert!(!pr.has_alpha);
    assert_eq!(pr.bytes, std::fs::metadata(&p).unwrap().len());
}

#[test]
fn decode_applies_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let p = common::write_jpeg_exif(dir.path(), "hoch.jpg", 80, 40, 6);
    let d = decode(&p).unwrap();
    assert!(d.rotated);
    assert_eq!((d.image.width(), d.image.height()), (40, 80));
}

#[test]
fn decode_without_exif_is_not_rotated() {
    let dir = tempfile::tempdir().unwrap();
    let p = common::write_jpeg(dir.path(), "quer.jpg", 80, 40);
    let d = decode(&p).unwrap();
    assert!(!d.rotated);
    assert_eq!((d.image.width(), d.image.height()), (80, 40));
}

#[test]
fn format_is_detected_by_content_not_extension() {
    let dir = tempfile::tempdir().unwrap();
    let p = common::write_png(dir.path(), "eigentlich-png.jpg", 20, 10);
    assert_eq!(probe(&p).unwrap().format, SrcFormat::Png);
    assert_eq!(decode(&p).unwrap().format, SrcFormat::Png);
}

#[test]
fn probe_detects_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let p = common::write_png_alpha(dir.path(), "a.png", 20, 10);
    assert!(probe(&p).unwrap().has_alpha);
}

#[test]
fn empty_file_is_rejected_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("leer.jpg");
    std::fs::write(&p, b"").unwrap();
    assert!(matches!(probe(&p), Err(EngineError::Unsupported | EngineError::Decode(_))));
    assert!(matches!(decode(&p), Err(EngineError::Unsupported | EngineError::Decode(_))));
}

#[test]
fn truncated_jpeg_is_a_decode_error() {
    let dir = tempfile::tempdir().unwrap();
    let full = common::jpeg_bytes(400, 300, 90);
    let p = dir.path().join("halb.jpg");
    std::fs::write(&p, &full[..full.len() / 2]).unwrap();
    let r = decode(&p);
    assert!(matches!(r, Err(EngineError::Decode(_))), "war: {:?}", r.map(|d| (d.image.width(), d.image.height())));
}

#[test]
fn text_file_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("text.jpg");
    std::fs::write(&p, "das ist kein Bild, sondern nur Text").unwrap();
    assert!(matches!(probe(&p), Err(EngineError::Unsupported)));
    assert!(matches!(decode(&p), Err(EngineError::Unsupported)));
}

#[test]
fn missing_file_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("gibt-es-nicht.jpg");
    assert!(matches!(probe(&p), Err(EngineError::Io(_))));
    assert!(matches!(decode(&p), Err(EngineError::Io(_))));
}

#[test]
fn error_texts_are_german() {
    osulzer_imageoptimizer::i18n::set_lang(osulzer_imageoptimizer::i18n::Lang::De);
    assert_eq!(EngineError::Unsupported.to_string(), "Kein unterstütztes Bildformat");
    assert_eq!(EngineError::Animated.to_string(), "Animierte Bilder werden nicht unterstützt");
}
