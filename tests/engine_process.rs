mod common;

use osulzer_imageoptimizer::engine::{Note, process};
use osulzer_imageoptimizer::settings::{Format, OutFormat, Settings};
use image::ImageDecoder;
use img_parts::{Bytes, ImageEXIF};
use std::io::Cursor;

fn settings(max_edge: Option<u32>, format: Format, quality: u8) -> Settings {
    Settings { max_edge, format, quality, max_kb: None }
}

fn dims(data: &[u8]) -> (u32, u32) {
    let img = image::load_from_memory(data).unwrap();
    (img.width(), img.height())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn resizes_and_encodes_jpeg() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 1200, 800);
    let enc = process(&src, &settings(Some(600), Format::Jpeg, 80)).unwrap();
    assert_eq!((enc.width, enc.height), (600, 400));
    assert_eq!(dims(&enc.data), (600, 400));
    assert_eq!(&enc.data[..2], &[0xFF, 0xD8]);
    assert_eq!(enc.format, OutFormat::Jpeg);
    assert!((enc.data.len() as u64) < std::fs::metadata(&src).unwrap().len());
    assert_eq!(enc.quality_used, Some(80));
    assert_eq!(enc.note, None);
    assert!(!enc.source_untouched);
}

#[test]
fn never_enlarges() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 1200, 800);
    let enc = process(&src, &settings(Some(5000), Format::Jpeg, 80)).unwrap();
    assert_eq!(dims(&enc.data), (1200, 800));
}

#[test]
fn transparency_becomes_white_in_jpeg() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png_alpha(dir.path(), "a.png", 200, 100);
    let enc = process(&src, &settings(None, Format::Jpeg, 90)).unwrap();
    assert_eq!(enc.format, OutFormat::Jpeg);
    let img = image::load_from_memory(&enc.data).unwrap().to_rgb8();
    let p = img.get_pixel(20, 50);
    assert!(p.0.iter().all(|&c| c >= 250), "war {:?}", p);
}

#[test]
fn png_stays_lossless() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png(dir.path(), "a.png", 120, 80);
    let enc = process(&src, &settings(None, Format::Original, 100)).unwrap();
    assert_eq!(enc.format, OutFormat::Png);
    assert_eq!(enc.quality_used, None);
    let back = image::load_from_memory(&enc.data).unwrap().to_rgb8();
    assert_eq!(back.as_raw(), common::photo(120, 80).as_raw());
}

/// Farbtyp aus dem IHDR-Chunk: 3 = Palette.
fn png_color_type(data: &[u8]) -> u8 {
    assert_eq!(&data[12..16], b"IHDR");
    data[25]
}

#[test]
fn png_below_100_reduces_to_a_palette() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png(dir.path(), "a.png", 300, 200);
    let lossless = process(&src, &settings(None, Format::Png, 100)).unwrap();
    let enc = process(&src, &settings(None, Format::Png, 80)).unwrap();
    assert_eq!(enc.format, OutFormat::Png);
    assert_eq!(enc.quality_used, Some(80));
    assert_eq!(png_color_type(&enc.data), 3);
    assert_eq!(dims(&enc.data), (300, 200));
    assert!(enc.data.len() * 2 < lossless.data.len(), "{} gegen {}", enc.data.len(), lossless.data.len());
    // Das Bild bleibt erkennbar: mittlere Abweichung je Kanal ist klein.
    let back = image::load_from_memory(&enc.data).unwrap().to_rgb8();
    let orig = common::photo(300, 200);
    let sum: u64 = back.as_raw().iter().zip(orig.as_raw()).map(|(a, b)| a.abs_diff(*b) as u64).sum();
    let mean = sum as f64 / orig.as_raw().len() as f64;
    assert!(mean < 12.0, "mittlere Abweichung {mean}");
}

#[test]
fn png_lower_quality_means_fewer_colors_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png(dir.path(), "a.png", 300, 200);
    let high = process(&src, &settings(None, Format::Png, 80)).unwrap();
    let low = process(&src, &settings(None, Format::Png, 30)).unwrap();
    assert!(low.data.len() < high.data.len());
}

#[test]
fn png_palette_keeps_transparency() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png_alpha(dir.path(), "a.png", 200, 100);
    let enc = process(&src, &settings(None, Format::Png, 70)).unwrap();
    assert_eq!(png_color_type(&enc.data), 3);
    let img = image::load_from_memory(&enc.data).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(20, 50)[3], 0);
    assert_eq!(img.get_pixel(180, 50)[3], 255);
}

#[test]
fn png_with_few_colors_stays_exact_at_any_quality() {
    let dir = tempfile::tempdir().unwrap();
    let img = image::RgbImage::from_fn(64, 64, |x, y| if (x / 8 + y / 8) % 2 == 0 { image::Rgb([200, 30, 30]) } else { image::Rgb([20, 20, 120]) });
    let src = dir.path().join("a.png");
    img.save(&src).unwrap();
    let enc = process(&src, &settings(Some(64), Format::Png, 50)).unwrap();
    let back = image::load_from_memory(&enc.data).unwrap().to_rgb8();
    assert_eq!(back.as_raw(), img.as_raw());
}

#[test]
fn max_kb_lowers_png_quality_until_it_fits() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png(dir.path(), "a.png", 300, 200);
    let lossless = process(&src, &settings(None, Format::Png, 100)).unwrap();
    let limit_kb = (lossless.data.len() as u32 / 1024) / 3;
    let s = Settings { max_edge: None, format: Format::Png, quality: 100, max_kb: Some(limit_kb) };
    let enc = process(&src, &s).unwrap();
    assert!(enc.data.len() <= limit_kb as usize * 1024, "{} > {} KB", enc.data.len(), limit_kb);
    assert!(enc.quality_used.is_some_and(|q| (40..100).contains(&q)));
    assert_eq!(enc.note, None);
}

#[test]
fn webp_output_has_webp_container_and_keeps_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png_alpha(dir.path(), "a.png", 200, 100);
    let enc = process(&src, &settings(None, Format::Webp, 80)).unwrap();
    assert_eq!(&enc.data[..4], b"RIFF");
    assert_eq!(&enc.data[8..12], b"WEBP");
    let img = image::load_from_memory(&enc.data).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(20, 50)[3], 0);
    assert_eq!(img.get_pixel(180, 50)[3], 255);
}

#[test]
fn webp_of_a_flat_graphic_is_lossless_when_that_is_smaller() {
    let dir = tempfile::tempdir().unwrap();
    let img = image::RgbImage::from_fn(256, 256, |x, y| {
        if (x / 16 + y / 16) % 2 == 0 { image::Rgb([200, 30, 30]) } else { image::Rgb([20, 20, 120]) }
    });
    let src = dir.path().join("a.png");
    img.save(&src).unwrap();
    let enc = process(&src, &settings(None, Format::Webp, 80)).unwrap();
    assert!(contains(&enc.data, b"VP8L"), "nicht verlustfrei");
    assert_eq!(enc.quality_used, None);
    let back = image::load_from_memory(&enc.data).unwrap().to_rgb8();
    assert_eq!(back.as_raw(), img.as_raw());
}

#[test]
fn webp_of_a_photo_stays_lossy() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png(dir.path(), "a.png", 300, 200);
    let enc = process(&src, &settings(None, Format::Webp, 80)).unwrap();
    assert!(contains(&enc.data, b"VP8 "));
    assert_eq!(enc.quality_used, Some(80));
}

#[test]
fn max_kb_lowers_quality_until_it_fits() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 1200, 800);
    let unlimited = process(&src, &settings(None, Format::Jpeg, 90)).unwrap();
    let limit_kb = (unlimited.data.len() as u32 / 1024) * 2 / 3;
    let s = Settings { max_edge: None, format: Format::Jpeg, quality: 90, max_kb: Some(limit_kb) };
    let enc = process(&src, &s).unwrap();
    assert!(enc.data.len() <= limit_kb as usize * 1024, "{} > {} KB", enc.data.len(), limit_kb);
    let q = enc.quality_used.unwrap();
    assert!((40..90).contains(&q), "Qualität {q}");
    assert_eq!(enc.note, None);
    // Die höchste passende Qualität: eine Stufe mehr passt nicht mehr.
    let above = process(&src, &settings(None, Format::Jpeg, q + 1)).unwrap();
    assert!(above.data.len() > limit_kb as usize * 1024);
}

#[test]
fn max_kb_unreachable_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 1200, 800);
    let s = Settings { max_edge: None, format: Format::Jpeg, quality: 90, max_kb: Some(1) };
    let enc = process(&src, &s).unwrap();
    assert_eq!(enc.note, Some(Note::TargetSizeMissed));
    assert_eq!(enc.quality_used, Some(40));
}

#[test]
fn max_kb_unreachable_is_reported_for_png() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png(dir.path(), "a.png", 300, 200);
    let s = Settings { max_edge: None, format: Format::Png, quality: 90, max_kb: Some(1) };
    let enc = process(&src, &s).unwrap();
    assert_eq!(enc.note, Some(Note::TargetSizeMissed));
    assert_eq!(enc.quality_used, Some(40));
    assert_eq!(enc.format, OutFormat::Png);
}

#[test]
fn rotation_is_applied_and_metadata_removed() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_exif(dir.path(), "a.jpg", 1200, 800, 6);
    assert!(contains(&std::fs::read(&src).unwrap(), b"Exif\0\0"));
    let enc = process(&src, &settings(Some(600), Format::Jpeg, 80)).unwrap();
    assert_eq!((enc.width, enc.height), (400, 600));
    assert_eq!(dims(&enc.data), (400, 600));
    let jpeg = img_parts::jpeg::Jpeg::from_bytes(Bytes::from(enc.data.clone())).unwrap();
    assert_eq!(jpeg.exif(), None);
    assert!(!contains(&enc.data, b"Exif\0\0"));
}

#[test]
fn already_optimal_source_is_kept_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_q(dir.path(), "a.jpg", 600, 400, 30);
    let source = std::fs::read(&src).unwrap();
    let enc = process(&src, &settings(None, Format::Jpeg, 95)).unwrap();
    assert_eq!(enc.note, Some(Note::AlreadyOptimal));
    assert_eq!(enc.quality_used, None);
    assert!(enc.source_untouched);
    assert_eq!(enc.data, source);
    assert_eq!((enc.width, enc.height), (600, 400));
}

#[test]
fn already_optimal_source_still_loses_its_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_exif_q(dir.path(), "a.jpg", 600, 400, 1, 30);
    let source = std::fs::read(&src).unwrap();
    let enc = process(&src, &settings(None, Format::Jpeg, 95)).unwrap();
    assert_eq!(enc.note, Some(Note::AlreadyOptimal));
    assert!(!enc.source_untouched);
    assert!(enc.data.len() < source.len());
    assert!(!contains(&enc.data, b"Exif\0\0"));
    // Bilddaten unverändert: gleiche Pixel wie die Quelle.
    let a = image::load_from_memory(&enc.data).unwrap().to_rgb8();
    let b = image::load_from_memory(&source).unwrap().to_rgb8();
    assert_eq!(a.as_raw(), b.as_raw());
}

#[test]
fn rotation_forces_reencoding_even_if_larger() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_exif_q(dir.path(), "a.jpg", 600, 400, 6, 30);
    let enc = process(&src, &settings(None, Format::Jpeg, 95)).unwrap();
    assert_ne!(enc.note, Some(Note::AlreadyOptimal));
    assert_eq!(dims(&enc.data), (400, 600));
    assert!(!contains(&enc.data, b"Exif\0\0"));
}

#[test]
fn format_change_is_never_already_optimal() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_q(dir.path(), "a.jpg", 600, 400, 30);
    let enc = process(&src, &settings(None, Format::Png, 95)).unwrap();
    assert_eq!(enc.note, None);
    assert_eq!(enc.format, OutFormat::Png);
}

fn icc_of(data: &[u8], format: OutFormat) -> Option<Vec<u8>> {
    let c = Cursor::new(data);
    match format {
        OutFormat::Jpeg => image::codecs::jpeg::JpegDecoder::new(c).unwrap().icc_profile().unwrap(),
        OutFormat::Png => image::codecs::png::PngDecoder::new(c).unwrap().icc_profile().unwrap(),
        OutFormat::Webp => image::codecs::webp::WebPDecoder::new(c).unwrap().icc_profile().unwrap(),
    }
}

#[test]
fn icc_profile_survives_in_every_output_format() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_icc(dir.path(), "a.jpg", 300, 200);
    for (format, out) in [(Format::Jpeg, OutFormat::Jpeg), (Format::Png, OutFormat::Png), (Format::Webp, OutFormat::Webp)] {
        let enc = process(&src, &settings(Some(150), format, 80)).unwrap();
        assert_eq!(icc_of(&enc.data, out), Some(common::dummy_icc()), "{out:?}");
        assert_eq!(dims(&enc.data), (150, 100), "{out:?}");
    }
}

#[test]
fn preview_shows_before_and_after_at_result_size() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 1200, 800);
    let s = settings(Some(600), Format::Jpeg, 80);
    let p = osulzer_imageoptimizer::engine::preview(&src, &s, 4096).unwrap();
    assert_eq!(p.before.dimensions(), (600, 400));
    assert_eq!(p.after.dimensions(), (600, 400));
    assert_eq!(p.encoded.data, process(&src, &s).unwrap().data);
}

#[test]
fn preview_textures_are_limited_but_numbers_stay_real() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg(dir.path(), "a.jpg", 1200, 800);
    let p = osulzer_imageoptimizer::engine::preview(&src, &settings(Some(600), Format::Jpeg, 80), 256).unwrap();
    assert_eq!(p.before.dimensions(), (256, 171));
    assert_eq!(p.after.dimensions(), (256, 171));
    assert_eq!((p.encoded.width, p.encoded.height), (600, 400));
}

#[test]
fn preview_after_shows_white_where_jpeg_flattened_transparency() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_png_alpha(dir.path(), "a.png", 200, 100);
    let p = osulzer_imageoptimizer::engine::preview(&src, &settings(None, Format::Jpeg, 90), 4096).unwrap();
    assert_eq!(p.before.get_pixel(20, 50)[3], 0);
    let after = p.after.get_pixel(20, 50);
    assert!(after[0] >= 250 && after[3] == 255, "war {:?}", after);
}

/// JPEG mit MPF-Segment (APP2) und angehängtem Vorschaubild samt Exif, wie es Handys schreiben.
fn jpeg_with_mpf_and_trailer(dir: &std::path::Path) -> std::path::PathBuf {
    let mut jpeg = img_parts::jpeg::Jpeg::from_bytes(Bytes::from(common::jpeg_bytes(600, 400, 30))).unwrap();
    let mpf = img_parts::jpeg::JpegSegment::new_with_contents(0xE2, Bytes::from_static(b"MPF\0II*\0geheim"));
    jpeg.segments_mut().insert(1, mpf);
    let mut data = jpeg.encoder().bytes().to_vec();
    let mut trailer = img_parts::jpeg::Jpeg::from_bytes(Bytes::from(common::jpeg_bytes(64, 48, 50))).unwrap();
    trailer.set_exif(Some(Bytes::from(common::exif_blob(1))));
    data.extend_from_slice(&trailer.encoder().bytes());
    let p = dir.join("handy.jpg");
    std::fs::write(&p, data).unwrap();
    p
}

#[test]
fn already_optimal_never_keeps_mpf_or_trailing_images() {
    let dir = tempfile::tempdir().unwrap();
    let src = jpeg_with_mpf_and_trailer(dir.path());
    let enc = process(&src, &settings(None, Format::Jpeg, 100)).unwrap();
    assert!(!contains(&enc.data, b"Exif\0\0"), "Exif aus angehängtem Bild");
    assert!(!contains(&enc.data, b"MPF\0"), "MPF-Segment");
    assert!(!contains(&enc.data, b"geheim"));
}
