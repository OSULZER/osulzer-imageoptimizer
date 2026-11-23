mod common;

use osulzer_imageoptimizer::engine::{EngineError, thumbnail};

fn is_red(p: &image::Rgba<u8>) -> bool {
    p[0] > 180 && p[1] < 60 && p[2] < 60
}

fn is_green(p: &image::Rgba<u8>) -> bool {
    p[1] > 160 && p[0] < 60 && p[2] < 60
}

#[test]
fn uses_the_embedded_exif_thumbnail() {
    let dir = tempfile::tempdir().unwrap();
    let p = common::write_jpeg_with_thumb(dir.path(), "a.jpg", 1);
    let t = thumbnail(&p, 100).unwrap();
    assert_eq!(t.dimensions(), (100, 100));
    assert!(is_red(t.get_pixel(50, 50)), "war {:?}", t.get_pixel(50, 50));
}

#[test]
fn falls_back_to_the_full_image() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("b.jpg");
    std::fs::write(&p, common::solid_jpeg(1200, 800, [0, 200, 0])).unwrap();
    let t = thumbnail(&p, 100).unwrap();
    assert_eq!(t.dimensions(), (100, 100));
    assert!(is_green(t.get_pixel(50, 50)), "war {:?}", t.get_pixel(50, 50));
}

#[test]
fn works_for_png_with_transparency_and_portrait_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let png = common::write_png_alpha(dir.path(), "a.png", 300, 100);
    assert_eq!(thumbnail(&png, 64).unwrap().dimensions(), (64, 64));
    let hoch = common::write_jpeg_exif(dir.path(), "h.jpg", 600, 300, 6);
    assert_eq!(thumbnail(&hoch, 64).unwrap().dimensions(), (64, 64));
}

#[test]
fn embedded_thumbnail_is_rotated_like_the_main_image() {
    let dir = tempfile::tempdir().unwrap();
    // Linke Hälfte rot, rechte blau; nach Drehung um 90° im Uhrzeigersinn ist oben rot.
    let mut half = Vec::new();
    let img = image::RgbImage::from_fn(160, 120, |x, _| if x < 80 { image::Rgb([220, 0, 0]) } else { image::Rgb([0, 0, 220]) });
    img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut half, 95)).unwrap();
    let mut jpeg = img_parts::jpeg::Jpeg::from_bytes(img_parts::Bytes::from(common::solid_jpeg(1200, 800, [0, 200, 0]))).unwrap();
    img_parts::ImageEXIF::set_exif(&mut jpeg, Some(img_parts::Bytes::from(common::exif_with_thumbnail(6, &half))));
    let p = dir.path().join("r.jpg");
    std::fs::write(&p, jpeg.encoder().bytes()).unwrap();
    let t = thumbnail(&p, 100).unwrap();
    assert!(is_red(t.get_pixel(50, 10)), "oben war {:?}", t.get_pixel(50, 10));
}

#[test]
fn unsupported_files_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jpg");
    std::fs::write(&p, "kein Bild").unwrap();
    assert!(matches!(thumbnail(&p, 64), Err(EngineError::Unsupported)));
}
