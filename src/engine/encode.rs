//! Kodieren, Zielgröße treffen, Farbprofil einbetten, Metadaten entfernen.

use super::{Decoded, EngineError, decode, resize, target_size};
use crate::settings::{MIN_QUALITY, OutFormat, Settings, SrcFormat, resolve_format};
use image::{DynamicImage, Rgb, RgbImage};
use img_parts::jpeg::Jpeg;
use img_parts::png::Png;
use img_parts::webp::WebP;
use img_parts::{Bytes, ImageICC};
use std::path::Path;

/// Hinweis zum Ergebnis eines Bildes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    TargetSizeMissed,
    AlreadyOptimal,
}

/// Fertig kodiertes Ergebnis eines Bildes.
#[derive(Clone, Debug)]
pub struct Encoded {
    pub data: Vec<u8>,
    pub format: OutFormat,
    pub width: u32,
    pub height: u32,
    /// Tatsächlich verwendete Qualität; `None` bei PNG und bei übernommenen Bilddaten.
    pub quality_used: Option<u8>,
    pub note: Option<Note>,
    /// `data` ist bytegleich zur Quelldatei.
    pub source_untouched: bool,
}

pub fn process(path: &Path, s: &Settings) -> Result<Encoded, EngineError> {
    let decoded = decode(path)?;
    let (_resized, encoded) = encode_decoded(&decoded, s)?;
    Ok(encoded)
}

/// Skaliert und kodiert ein bereits eingelesenes Bild. Gibt auch das skalierte Bild zurück.
pub(crate) fn encode_decoded(decoded: &Decoded, s: &Settings) -> Result<(DynamicImage, Encoded), EngineError> {
    let src = &decoded.image;
    let (width, height) = target_size(src.width(), src.height(), s.max_edge);
    let resized = resize(src, width, height);
    let format = resolve_format(s.format, decoded.format, src.color().has_alpha());

    let (mut data, mut quality_used, mut note) = match format {
        OutFormat::Png => {
            let (data, quality, note) = fit_quality(s, |q| encode_png(&resized, q))?;
            // Qualität 100 ist verlustfrei und wird nicht als Qualität ausgewiesen.
            (data, quality.filter(|&q| q < PNG_LOSSLESS), note)
        }
        OutFormat::Jpeg => {
            let rgb = flatten_on_white(&resized);
            fit_quality(s, |q| encode_jpeg(&rgb, q))?
        }
        OutFormat::Webp => {
            let lossy = fit_quality(s, |q| encode_webp(&resized, Some(q)))?;
            // Flächige Grafiken sind verlustfrei oft kleiner und dann auch exakt.
            match has_few_colors(&resized).then(|| encode_webp(&resized, None)).transpose()? {
                Some(lossless) if lossless.len() <= lossy.0.len() => {
                    let fits = s.max_kb.is_none_or(|kb| lossless.len() <= kb as usize * 1024);
                    (lossless, None, if fits { None } else { lossy.2 })
                }
                _ => lossy,
            }
        }
    };
    if let Some(icc) = &decoded.icc {
        data = embed_icc(data, format, icc);
    }

    // „Bereits optimal“: Bilddaten der Quelle übernehmen, Metadaten trotzdem entfernen.
    let mut source_untouched = false;
    let same_format = matches!(
        (decoded.format, format),
        (SrcFormat::Jpeg, OutFormat::Jpeg) | (SrcFormat::Png, OutFormat::Png) | (SrcFormat::Webp, OutFormat::Webp)
    );
    let same_size = (width, height) == (src.width(), src.height());
    if same_format
        && same_size
        && !decoded.rotated
        && data.len() >= decoded.source.len()
        && let Some(stripped) = strip_metadata(&decoded.source, format)
    {
        source_untouched = stripped == decoded.source;
        data = stripped;
        quality_used = None;
        note = Some(Note::AlreadyOptimal);
    }

    Ok((resized, Encoded { data, format, width, height, quality_used, note, source_untouched }))
}

type Fitted = (Vec<u8>, Option<u8>, Option<Note>);

/// Kodiert mit der gewünschten Qualität und senkt sie bei gesetzter Zielgröße so weit wie nötig,
/// höchstens bis `MIN_QUALITY`.
fn fit_quality(s: &Settings, encode: impl Fn(u8) -> Result<Vec<u8>, EngineError>) -> Result<Fitted, EngineError> {
    let top = s.quality.clamp(1, 100);
    let first = encode(top)?;
    let Some(limit) = s.max_kb.map(|kb| kb as usize * 1024) else {
        return Ok((first, Some(top), None));
    };
    if first.len() <= limit {
        return Ok((first, Some(top), None));
    }
    let floor = MIN_QUALITY.min(top);
    if floor == top {
        return Ok((first, Some(top), Some(Note::TargetSizeMissed)));
    }
    let lowest = encode(floor)?;
    if lowest.len() > limit {
        return Ok((lowest, Some(floor), Some(Note::TargetSizeMissed)));
    }
    // Invariante: `lo` passt, `hi` passt nicht.
    let (mut lo, mut hi, mut best) = (floor, top, lowest);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        let candidate = encode(mid)?;
        if candidate.len() <= limit {
            lo = mid;
            best = candidate;
        } else {
            hi = mid;
        }
    }
    Ok((best, Some(lo), None))
}

/// Legt transparente Bereiche auf Weiß.
pub(super) fn flatten_on_white(img: &DynamicImage) -> RgbImage {
    match img {
        DynamicImage::ImageRgb8(rgb) => rgb.clone(),
        other => {
            let rgba = other.to_rgba8();
            RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                let p = rgba.get_pixel(x, y);
                let a = p[3] as u32;
                let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                Rgb([blend(p[0]), blend(p[1]), blend(p[2])])
            })
        }
    }
}

pub(super) fn encode_jpeg(rgb: &RgbImage, quality: u8) -> Result<Vec<u8>, EngineError> {
    let (w, h) = rgb.dimensions();
    // mozjpeg meldet manche Fehler als Panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> std::io::Result<Vec<u8>> {
        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        comp.set_size(w as usize, h as usize);
        comp.set_quality(quality as f32);
        comp.set_progressive_mode();
        let mut started = comp.start_compress(Vec::new())?;
        started.write_scanlines(rgb.as_raw())?;
        started.finish()
    }));
    match result {
        Ok(Ok(data)) => Ok(data),
        Ok(Err(e)) => Err(EngineError::Encode(e.to_string())),
        Err(_) => Err(EngineError::Encode(crate::i18n::t("error.jpeg_aborted").into())),
    }
}

/// Gründlichste Kodierstufe: gemessen 2–7 % kleiner als die Voreinstellung, bei weniger als doppelter Zeit.
const WEBP_METHOD: i32 = 6;

/// `quality` = `None` kodiert verlustfrei.
fn encode_webp(img: &DynamicImage, quality: Option<u8>) -> Result<Vec<u8>, EngineError> {
    let failed = || EngineError::Encode("WebP".into());
    let mut config = webp::WebPConfig::new().map_err(|_| failed())?;
    config.method = WEBP_METHOD;
    match quality {
        Some(q) => config.quality = q as f32,
        None => config.lossless = 1,
    }
    let (w, h) = (img.width(), img.height());
    let encode = |encoder: webp::Encoder| encoder.encode_advanced(&config).map(|m| m.to_vec()).map_err(|_| failed());
    match img {
        DynamicImage::ImageRgba8(rgba) => encode(webp::Encoder::from_rgba(rgba.as_raw(), w, h)),
        other => encode(webp::Encoder::from_rgb(other.to_rgb8().as_raw(), w, h)),
    }
}

/// Kommt das Bild mit höchstens 256 Farben aus? Bricht ab, sobald es mehr sind.
fn has_few_colors(img: &DynamicImage) -> bool {
    let bytes_per_pixel = if img.color().has_alpha() { 4 } else { 3 };
    let mut seen = std::collections::HashSet::new();
    img.as_bytes().chunks_exact(bytes_per_pixel).all(|p| {
        seen.insert(p);
        seen.len() <= 256
    })
}

/// Ab dieser Qualität bleibt PNG verlustfrei.
const PNG_LOSSLESS: u8 = 100;
/// Ab dieser Qualität nutzt die Farbreduktion die volle Palette.
const PNG_FULL_PALETTE: u8 = 80;
/// Stärke der Rasterung. Volle Rasterung (1.0) rauscht bei kleinen Paletten so stark, dass die Datei
/// mit sinkender Qualität wieder wächst; gemessen fällt die Größe bei 0.7 stetig.
const PNG_DITHERING: f32 = 0.7;

/// Größe der Palette für eine Qualität unter 100: 256 Farben ab 80, darunter quadratisch fallend
/// (60 → 144, 40 → 64, 20 → 16).
fn png_colors(quality: u8) -> i32 {
    let q = quality.min(PNG_FULL_PALETTE) as f32 / PNG_FULL_PALETTE as f32;
    ((256.0 * q * q).round() as i32).clamp(2, 256)
}

/// Qualität 100: verlustfrei. Darunter: Reduktion auf eine Palette mit Rasterung, wie pngquant.
/// Bilder, die ohnehin mit der Palette auskommen, bleiben dabei exakt.
fn encode_png(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, EngineError> {
    let err = |e: &dyn std::fmt::Display| EngineError::Encode(e.to_string());
    let (w, h) = (img.width(), img.height());
    let raw = if quality >= PNG_LOSSLESS {
        match img {
            DynamicImage::ImageRgba8(rgba) => {
                oxipng::RawImage::new(w, h, oxipng::ColorType::RGBA, oxipng::BitDepth::Eight, rgba.as_raw().clone())
            }
            other => oxipng::RawImage::new(
                w,
                h,
                oxipng::ColorType::RGB { transparent_color: None },
                oxipng::BitDepth::Eight,
                other.to_rgb8().into_raw(),
            ),
        }
    } else {
        let rgba = img.to_rgba8();
        let (palette, indices) = quantize(rgba.as_raw(), w as usize, h as usize, png_colors(quality))?;
        oxipng::RawImage::new(w, h, oxipng::ColorType::Indexed { palette }, oxipng::BitDepth::Eight, indices)
    }
    .map_err(|e| err(&e))?;
    raw.create_optimized_png(&oxipng::Options::from_preset(2)).map_err(|e| err(&e))
}

/// Reduziert RGBA-Pixel auf höchstens `colors` Farben. Gibt die Palette und einen Index je Pixel zurück.
fn quantize(rgba: &[u8], w: usize, h: usize, colors: i32) -> Result<(Vec<oxipng::RGBA8>, Vec<u8>), EngineError> {
    let err = |e: quantizr::Error| EngineError::Encode(format!("{e:?}"));
    let image = quantizr::Image::new(rgba, w, h).map_err(err)?;
    let mut options = quantizr::Options::default();
    options.set_max_colors(colors).map_err(err)?;
    let mut result = quantizr::QuantizeResult::quantize(&image, &options);
    result.set_dithering_level(PNG_DITHERING).map_err(err)?;
    let mut indices = vec![0u8; w * h];
    result.remap_image(&image, &mut indices).map_err(err)?;
    let pal = result.get_palette();
    let palette = pal.entries[..pal.count as usize].iter().map(|c| oxipng::RGBA8::new(c.r, c.g, c.b, c.a)).collect();
    Ok((palette, indices))
}

/// Setzt das Farbprofil der Quelle ins Ergebnis. Gelingt das nicht, bleibt das Ergebnis unverändert.
fn embed_icc(data: Vec<u8>, format: OutFormat, icc: &[u8]) -> Vec<u8> {
    let bytes = Bytes::from(data);
    let profile = Some(Bytes::copy_from_slice(icc));
    let out = match format {
        OutFormat::Jpeg => Jpeg::from_bytes(bytes.clone()).map(|mut j| {
            j.set_icc_profile(profile);
            j.encoder().bytes()
        }),
        OutFormat::Png => Png::from_bytes(bytes.clone()).map(|mut p| {
            p.set_icc_profile(profile);
            p.encoder().bytes()
        }),
        OutFormat::Webp => WebP::from_bytes(bytes.clone()).map(|mut w| {
            w.set_icc_profile(profile);
            w.encoder().bytes()
        }),
    };
    out.unwrap_or(bytes).to_vec()
}

/// Entfernt EXIF, XMP, IPTC und Kommentare ohne Neukodierung.
/// `None`, wenn das für dieses Format nicht sicher möglich ist.
fn strip_metadata(source: &[u8], format: OutFormat) -> Option<Vec<u8>> {
    let bytes = Bytes::copy_from_slice(source);
    match format {
        OutFormat::Jpeg => {
            // Angehängte Daten (Vorschaubilder, Bewegtfotos) können eigene Exif-Daten tragen.
            if has_trailing_data(source) {
                return None;
            }
            let mut jpeg = Jpeg::from_bytes(bytes).ok()?;
            let before = jpeg.segments().len();
            // Behalten: APP0 (JFIF), APP2 nur mit Farbprofil, APP14 (Adobe-Farbangabe) und alle Bilddaten.
            jpeg.segments_mut().retain(|seg| {
                let m = seg.marker();
                let is_app = (0xE0..=0xEF).contains(&m);
                let is_icc = m == 0xE2 && seg.contents().starts_with(b"ICC_PROFILE\0");
                let keep_app = matches!(m, 0xE0 | 0xEE) || is_icc;
                !(m == 0xFE || (is_app && !keep_app))
            });
            if jpeg.segments().len() == before {
                return Some(source.to_vec());
            }
            Some(jpeg.encoder().bytes().to_vec())
        }
        OutFormat::Png => {
            let mut png = Png::from_bytes(bytes).ok()?;
            let before = png.chunks().len();
            // Nur Bild- und Farbangaben behalten; alles andere kann Metadaten tragen.
            png.chunks_mut().retain(|c| {
                matches!(
                    &c.kind(),
                    b"IHDR" | b"PLTE" | b"IDAT" | b"IEND" | b"tRNS" | b"iCCP" | b"sRGB" | b"gAMA" | b"cHRM" | b"sBIT" | b"pHYs"
                )
            });
            if png.chunks().len() == before {
                return Some(source.to_vec());
            }
            Some(png.encoder().bytes().to_vec())
        }
        OutFormat::Webp => {
            let has_metadata = [b"EXIF", b"XMP "].iter().any(|id| source.windows(4).any(|w| w == *id));
            if has_metadata { None } else { Some(source.to_vec()) }
        }
    }
}

/// Vorher und nachher für die Vorschau, dazu das tatsächliche Ergebnis.
pub struct PreviewResult {
    /// Gedrehte Quelle in Ergebnisgröße, höchstens `max_tex` je Kante.
    pub before: image::RgbaImage,
    /// Das dekodierte tatsächliche Ergebnis, gleich groß wie `before`.
    pub after: image::RgbaImage,
    pub encoded: Encoded,
}

pub fn preview(path: &Path, s: &Settings, max_tex: u32) -> Result<PreviewResult, EngineError> {
    let decoded = decode(path)?;
    let (resized, encoded) = encode_decoded(&decoded, s)?;
    drop(decoded);
    let (w, h) = target_size(resized.width(), resized.height(), Some(max_tex));
    let before = resize(&resized, w, h).to_rgba8();
    let after_full = image::load_from_memory(&encoded.data).map_err(|e| EngineError::Decode(e.to_string()))?;
    let after_full = if after_full.color().has_alpha() {
        DynamicImage::ImageRgba8(after_full.into_rgba8())
    } else {
        DynamicImage::ImageRgb8(after_full.into_rgb8())
    };
    let after = resize(&after_full, w, h).to_rgba8();
    Ok(PreviewResult { before, after, encoded })
}

/// Folgen nach dem Ende des Hauptbildes noch Daten (Nullbytes als Füllung ausgenommen)?
fn has_trailing_data(data: &[u8]) -> bool {
    match super::decode::jpeg_primary_end(data) {
        Some(end) => data[end..].iter().any(|&b| b != 0),
        None => true,
    }
}
