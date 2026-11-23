//! Texte als Metadaten (XMP, bei JPEG zusätzlich EXIF-Titel) und das Eingabebild für die KI.
//!
//! WordPress (ab 7.0) liest den Alt-Text aus `Iptc4xmpCore:AltTextAccessibility`, indem es die Datei
//! nach `<x:xmpmeta` … `</x:xmpmeta>` durchsucht; den Titel aus EXIF `ImageDescription` (nur JPEG).

use crate::i18n::Lang;
use super::encode::{encode_jpeg, flatten_on_white};
use super::{EngineError, decode, resize, target_size};
use crate::settings::OutFormat;
use crate::texts::ImageTexts;
use img_parts::jpeg::{Jpeg, JpegSegment};
use img_parts::png::{Png, PngChunk};
use img_parts::riff::{RiffChunk, RiffContent};
use img_parts::webp::WebP;
use img_parts::{Bytes, ImageEXIF};
use std::path::Path;

const XMP_JPEG_ID: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_PNG_KEYWORD: &[u8] = b"XML:com.adobe.xmp";
/// Längste Kante des Bildes, das an die KI geht.
pub const AI_INPUT_EDGE: u32 = 1024;

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // In XML 1.0 nicht erlaubte Steuerzeichen weglassen.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// Sprachabhängiger Wert (`rdf:Alt`) für die Textsprache und als Standard.
fn lang_alt(tag: &str, value: &str, lang: Lang) -> String {
    let v = escape(value.trim());
    let code = lang.code();
    format!(
        "   <{tag}>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">{v}</rdf:li>\n     <rdf:li xml:lang=\"{code}\">{v}</rdf:li>\n    </rdf:Alt>\n   </{tag}>\n"
    )
}

/// XMP-Paket mit Titel, Beschreibung und Alt-Text (nur gefüllte Felder).
pub fn xmp_packet(t: &ImageTexts, lang: Lang) -> String {
    let mut fields = String::new();
    for (tag, value) in [
        ("dc:title", &t.title),
        ("dc:description", &t.description),
        ("Iptc4xmpCore:AltTextAccessibility", &t.alt),
    ] {
        if !value.trim().is_empty() {
            fields += &lang_alt(tag, value, lang);
        }
    }
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
    xmlns:Iptc4xmpCore=\"http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/\">\n\
{fields}  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>"
    )
}

/// EXIF-Block (TIFF, Little Endian) nur mit IFD0 `ImageDescription`.
fn exif_description(title: &str) -> Vec<u8> {
    let mut text = title.trim().as_bytes().to_vec();
    text.push(0);
    let count = text.len() as u32;
    let mut v = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00];
    v.extend([0x0E, 0x01, 0x02, 0x00]); // Tag 0x010E, Typ ASCII
    v.extend(count.to_le_bytes());
    if count <= 4 {
        let mut inline = text.clone();
        inline.resize(4, 0);
        v.extend(inline);
        v.extend(0u32.to_le_bytes());
    } else {
        v.extend(26u32.to_le_bytes()); // Wert direkt hinter dem IFD
        v.extend(0u32.to_le_bytes());
        v.extend(text);
    }
    v
}

fn parse_error(e: img_parts::Error) -> EngineError {
    EngineError::Encode(crate::i18n::tf("error.embed_metadata", &[("e", &e)]))
}

/// Setzt die Texte in die kodierten Bilddaten. Vorhandene XMP-/EXIF-Blöcke werden ersetzt.
/// Sind alle Texte leer, bleiben die Daten unverändert.
pub fn embed_texts(data: &[u8], format: OutFormat, t: &ImageTexts, lang: Lang) -> Result<Vec<u8>, EngineError> {
    if [&t.title, &t.alt, &t.description].iter().all(|f| f.trim().is_empty()) {
        return Ok(data.to_vec());
    }
    let xmp = xmp_packet(t, lang);
    let bytes = Bytes::copy_from_slice(data);
    let out = match format {
        OutFormat::Jpeg => {
            let mut jpeg = Jpeg::from_bytes(bytes).map_err(parse_error)?;
            // Alle APP1 (EXIF und XMP) entfernen, dann neu einsetzen.
            jpeg.segments_mut().retain(|s| s.marker() != 0xE1);
            let mut contents = XMP_JPEG_ID.to_vec();
            contents.extend_from_slice(xmp.as_bytes());
            let segment = JpegSegment::new_with_contents(0xE1, Bytes::from(contents));
            let at = jpeg.segments().iter().take_while(|s| s.marker() == 0xE0).count();
            jpeg.segments_mut().insert(at, segment);
            if t.title.trim().is_empty() {
                jpeg.set_exif(None);
            } else {
                jpeg.set_exif(Some(Bytes::from(exif_description(&t.title))));
            }
            jpeg.encoder().bytes()
        }
        OutFormat::Webp => {
            let mut webp = WebP::from_bytes(bytes).map_err(parse_error)?;
            webp.remove_chunks_by_id(*b"XMP ");
            webp.chunks_mut().push(RiffChunk::new(*b"XMP ", RiffContent::Data(Bytes::from(xmp.into_bytes()))));
            // Entfernt EXIF und bringt den VP8X-Kopf samt XMP-Flag in Ordnung.
            webp.set_exif(None);
            webp.encoder().bytes()
        }
        OutFormat::Png => {
            let mut png = Png::from_bytes(bytes).map_err(parse_error)?;
            png.chunks_mut().retain(|c| !(&c.kind() == b"iTXt" && c.contents().starts_with(XMP_PNG_KEYWORD)));
            let mut contents = XMP_PNG_KEYWORD.to_vec();
            // Ende des Schlüsselworts, unkomprimiert, Methode, leere Sprache, leere Übersetzung.
            contents.extend_from_slice(&[0, 0, 0, 0, 0]);
            contents.extend_from_slice(xmp.as_bytes());
            let chunk = PngChunk::new(*b"iTXt", Bytes::from(contents));
            let at = png.chunks().iter().position(|c| &c.kind() == b"IDAT").unwrap_or(png.chunks().len());
            png.chunks_mut().insert(at, chunk);
            png.encoder().bytes()
        }
    };
    Ok(out.to_vec())
}

/// Das Bild für die KI: gedreht, längste Kante 1024 px, JPEG Qualität 80, ohne Metadaten.
pub fn ai_input_jpeg(path: &Path) -> Result<Vec<u8>, EngineError> {
    let decoded = decode(path)?;
    let (w, h) = target_size(decoded.image.width(), decoded.image.height(), Some(AI_INPUT_EDGE));
    let small = resize(&decoded.image, w, h);
    encode_jpeg(&flatten_on_white(&small), 80)
}
