mod common;

use osulzer_imageoptimizer::engine::{ai_input_jpeg, embed_texts, process, xmp_packet};
use osulzer_imageoptimizer::settings::{Format, OutFormat, Settings};
use osulzer_imageoptimizer::i18n::Lang;
use osulzer_imageoptimizer::texts::ImageTexts;

fn texts() -> ImageTexts {
    ImageTexts {
        file_stem: "kachelofen-gruen".into(),
        title: "Grüner Kachelofen".into(),
        alt: "Kachelofen \"Müller\" & <Söhne> im Wohnzimmer".into(),
        description: "Ein grüner Kachelofen mit Sitzbank.".into(),
    }
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|w| *w == needle).count()
}

fn xmp_of(data: &[u8]) -> String {
    let text = String::from_utf8_lossy(data);
    let start = text.find("<x:xmpmeta").expect("kein XMP");
    let end = text.find("</x:xmpmeta>").expect("XMP nicht geschlossen") + "</x:xmpmeta>".len();
    text[start..end].to_string()
}

fn encoded(format: Format) -> (Vec<u8>, OutFormat, (u32, u32)) {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_exif(dir.path(), "a.jpg", 300, 200, 1);
    let enc = process(&src, &Settings { max_edge: None, format, quality: 80, max_kb: None }).unwrap();
    (enc.data, enc.format, (enc.width, enc.height))
}

#[test]
fn xmp_escapes_special_characters_and_has_all_fields() {
    let xmp = xmp_packet(&texts(), Lang::De);
    assert!(xmp.contains("Kachelofen &quot;Müller&quot; &amp; &lt;Söhne&gt; im Wohnzimmer"));
    assert!(!xmp.contains("<Söhne>"));
    assert!(xmp.contains("Iptc4xmpCore:AltTextAccessibility"));
    assert!(xmp.contains("xmlns:Iptc4xmpCore=\"http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/\""));
    assert!(xmp.contains("<dc:title>"));
    assert!(xmp.contains("<dc:description>"));
    assert!(xmp.contains("xml:lang=\"x-default\""));
    assert!(xmp.contains("xml:lang=\"de\""));
}

#[test]
fn every_format_gets_exactly_one_xmp_and_still_decodes() {
    for format in [Format::Jpeg, Format::Webp, Format::Png] {
        let (data, out, dims) = encoded(format);
        let with = embed_texts(&data, out, &texts(), Lang::De).unwrap();
        let img = image::load_from_memory(&with).unwrap_or_else(|e| panic!("{out:?}: {e}"));
        assert_eq!((img.width(), img.height()), dims, "{out:?}");
        assert_eq!(count(&with, b"<x:xmpmeta"), 1, "{out:?}");
        assert!(xmp_of(&with).contains("Söhne"), "{out:?}");
        // Zweimal einsetzen ergibt weiterhin genau ein XMP.
        let twice = embed_texts(&with, out, &texts(), Lang::De).unwrap();
        assert_eq!(count(&twice, b"<x:xmpmeta"), 1, "{out:?} zweimal");
    }
}

#[test]
fn jpeg_gets_the_title_as_exif_image_description_and_nothing_else() {
    let (data, out, _) = encoded(Format::Jpeg);
    let with = embed_texts(&data, out, &texts(), Lang::De).unwrap();
    let exif = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&with)).unwrap();
    let desc = exif.get_field(exif::Tag::ImageDescription, exif::In::PRIMARY).expect("kein ImageDescription");
    match &desc.value {
        exif::Value::Ascii(parts) => assert_eq!(String::from_utf8_lossy(&parts[0]), "Grüner Kachelofen"),
        other => panic!("{other:?}"),
    }
    assert!(exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY).is_none());
    assert!(exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY).is_none());
}

#[test]
fn existing_metadata_of_the_input_is_replaced() {
    // Eingabe mit GPS-EXIF direkt einsetzen (ohne vorheriges Entfernen durch `process`).
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_exif(dir.path(), "gps.jpg", 120, 80, 1);
    let raw = std::fs::read(src).unwrap();
    let with = embed_texts(&raw, OutFormat::Jpeg, &texts(), Lang::De).unwrap();
    let exif = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&with)).unwrap();
    assert!(exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY).is_none());
    assert_eq!(count(&with, b"Exif\0\0"), 1);
}

#[test]
fn webp_is_extended_with_xmp_flag() {
    let (data, out, _) = encoded(Format::Webp);
    let with = embed_texts(&data, out, &texts(), Lang::De).unwrap();
    assert_eq!(&with[12..16], b"VP8X");
    assert_ne!(with[20] & 0x04, 0, "XMP-Flag fehlt");
    assert_eq!(count(&with, b"XMP "), 1);
}

#[test]
fn png_uses_an_uncompressed_itxt_chunk() {
    let (data, out, _) = encoded(Format::Png);
    let with = embed_texts(&data, out, &texts(), Lang::De).unwrap();
    assert_eq!(count(&with, b"iTXtXML:com.adobe.xmp\0\0\0"), 1);
}

#[test]
fn empty_texts_leave_the_data_untouched() {
    let (data, out, _) = encoded(Format::Jpeg);
    assert_eq!(embed_texts(&data, out, &ImageTexts::default(), Lang::De).unwrap(), data);
}

#[test]
fn only_filled_fields_are_written() {
    let (data, out, _) = encoded(Format::Png);
    let only_alt = ImageTexts { alt: "Nur Alt".into(), ..Default::default() };
    let xmp = xmp_of(&embed_texts(&data, out, &only_alt, Lang::De).unwrap());
    assert!(xmp.contains("AltTextAccessibility"));
    assert!(!xmp.contains("dc:title"));
    assert!(!xmp.contains("dc:description"));
}

#[test]
fn ai_input_is_small_rotated_and_clean() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::write_jpeg_exif(dir.path(), "hoch.jpg", 3000, 2000, 6);
    let jpeg = ai_input_jpeg(&src).unwrap();
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    let img = image::load_from_memory(&jpeg).unwrap();
    assert_eq!((img.width(), img.height()), (683, 1024));
    assert_eq!(count(&jpeg, b"Exif\0\0"), 0);
}

#[test]
fn xmp_uses_given_language() {
    let en = xmp_packet(&texts(), Lang::En);
    assert!(en.contains("xml:lang=\"en\""));
    assert!(!en.contains("xml:lang=\"de\""));
    assert!(xmp_packet(&texts(), Lang::De).contains("xml:lang=\"de\""));
}
