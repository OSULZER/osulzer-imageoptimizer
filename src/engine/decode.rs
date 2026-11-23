//! Einlesen nach Dateiinhalt, Kamera-Drehung, Farbprofil.

use super::EngineError;
use crate::settings::SrcFormat;
use image::codecs::{bmp::BmpDecoder, jpeg::JpegDecoder, png::PngDecoder, tiff::TiffDecoder, webp::WebPDecoder};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::Path;

/// Obergrenze für den Speicher eines einzelnen dekodierten Bildes.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

/// Kennzahlen einer Bilddatei, ohne sie vollständig zu dekodieren.
#[derive(Clone, Debug)]
pub struct Probe {
    /// Breite nach eingerechneter Drehung.
    pub width: u32,
    /// Höhe nach eingerechneter Drehung.
    pub height: u32,
    pub bytes: u64,
    pub format: SrcFormat,
    pub has_alpha: bool,
}

/// Vollständig dekodiertes Bild, gedreht, 8 Bit RGB oder RGBA.
pub struct Decoded {
    pub image: DynamicImage,
    pub format: SrcFormat,
    pub icc: Option<Vec<u8>>,
    /// Die EXIF-Drehung hat die Pixel verändert.
    pub rotated: bool,
    /// Die unveränderten Bytes der Quelldatei.
    pub source: Vec<u8>,
}

pub fn probe(path: &Path) -> Result<Probe, EngineError> {
    let bytes = std::fs::metadata(path).map_err(EngineError::Io)?.len();
    let file = File::open(path).map_err(EngineError::Io)?;
    let (mut decoder, format) = open(BufReader::new(file))?;
    let (w, h) = decoder.dimensions();
    let has_alpha = decoder.color_type().has_alpha();
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (width, height) = if swaps_axes(orientation) { (h, w) } else { (w, h) };
    Ok(Probe { width, height, bytes, format, has_alpha })
}

pub fn decode(path: &Path) -> Result<Decoded, EngineError> {
    let source = std::fs::read(path).map_err(EngineError::Io)?;
    let (mut decoder, format) = open(Cursor::new(source.as_slice()))?;
    // Der JPEG-Dekoder füllt fehlende Bildteile stillschweigend grau auf.
    if format == SrcFormat::Jpeg && !jpeg_is_complete(&source) {
        return Err(EngineError::Decode(crate::i18n::t("error.truncated").into()));
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let icc = decoder.icc_profile().ok().flatten().filter(|p| !p.is_empty());
    let mut image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    image.apply_orientation(orientation);
    let image = if image.color().has_alpha() {
        DynamicImage::ImageRgba8(image.into_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.into_rgb8())
    };
    Ok(Decoded { image, format, icc, rotated: orientation != Orientation::NoTransforms, source })
}

fn swaps_axes(o: Orientation) -> bool {
    matches!(
        o,
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH
    )
}

fn decode_error(e: image::ImageError) -> EngineError {
    match e {
        image::ImageError::IoError(e) if e.kind() != std::io::ErrorKind::UnexpectedEof => EngineError::Io(e),
        other => EngineError::Decode(other.to_string()),
    }
}

/// Öffnet die Datei, erkennt das Format am Inhalt und lehnt animierte Bilder ab.
fn open<'a, R: BufRead + Seek + 'a>(source: R) -> Result<(Box<dyn ImageDecoder + 'a>, SrcFormat), EngineError> {
    let reader = ImageReader::new(source).with_guessed_format().map_err(EngineError::Io)?;
    let format = match reader.format() {
        Some(ImageFormat::Jpeg) => SrcFormat::Jpeg,
        Some(ImageFormat::Png) => SrcFormat::Png,
        Some(ImageFormat::WebP) => SrcFormat::Webp,
        Some(ImageFormat::Bmp) => SrcFormat::Bmp,
        Some(ImageFormat::Tiff) => SrcFormat::Tiff,
        _ => return Err(EngineError::Unsupported),
    };
    let inner = reader.into_inner();
    let mut decoder: Box<dyn ImageDecoder + 'a> = match format {
        SrcFormat::Jpeg => Box::new(JpegDecoder::new(inner).map_err(decode_error)?),
        SrcFormat::Png => {
            let d = PngDecoder::new(inner).map_err(decode_error)?;
            if d.is_apng().map_err(decode_error)? {
                return Err(EngineError::Animated);
            }
            Box::new(d)
        }
        SrcFormat::Webp => {
            let d = WebPDecoder::new(inner).map_err(decode_error)?;
            if d.has_animation() {
                return Err(EngineError::Animated);
            }
            Box::new(d)
        }
        SrcFormat::Bmp => Box::new(BmpDecoder::new(inner).map_err(decode_error)?),
        SrcFormat::Tiff => Box::new(TiffDecoder::new(inner).map_err(decode_error)?),
    };
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    decoder.set_limits(limits).map_err(decode_error)?;
    Ok((decoder, format))
}

/// Position direkt hinter dem Ende (EOI) des Hauptbildes, `None` bei unvollständiger Datei.
/// Läuft die Segmente bis zum ersten Scan (SOS) über ihre Längen ab, damit eingebettete
/// Vorschaubilder in Exif-Segmenten nicht stören. In den Bilddaten selbst kommt FF D9 nicht vor.
pub(crate) fn jpeg_primary_end(data: &[u8]) -> Option<usize> {
    let mut pos = 2; // hinter SOI
    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            return None;
        }
        let marker = data[pos + 1];
        if marker == 0xFF {
            pos += 1; // Füllbyte
            continue;
        }
        if marker == 0xDA {
            return data[pos..].windows(2).position(|w| w == [0xFF, 0xD9]).map(|i| pos + i + 2);
        }
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 2 + len;
    }
    None
}

fn jpeg_is_complete(data: &[u8]) -> bool {
    jpeg_primary_end(data).is_some()
}
