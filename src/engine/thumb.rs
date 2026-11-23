//! Kleine quadratische Vorschaubilder für die Liste.

use super::{EngineError, decode, resize};
use image::metadata::Orientation;
use image::{DynamicImage, RgbaImage};
use std::io::Read;
use std::path::Path;

/// So viel vom Dateianfang wird nach dem EXIF-Block durchsucht (APP1 ist höchstens 64 KB groß).
const HEAD_BYTES: u64 = 256 * 1024;

/// Quadratisches Vorschaubild mit `size` Pixeln Kantenlänge, mittig zugeschnitten.
/// Nutzt bei JPEGs das eingebettete EXIF-Vorschaubild, wenn vorhanden.
pub fn thumbnail(path: &Path, size: u32) -> Result<RgbaImage, EngineError> {
    if let Some(img) = embedded_thumbnail(path, size) {
        return Ok(cover(&img, size));
    }
    let decoded = decode(path)?;
    Ok(cover(&decoded.image, size))
}

/// Das EXIF-Vorschaubild, gedreht wie das Hauptbild. `None`, wenn keins brauchbar ist.
fn embedded_thumbnail(path: &Path, size: u32) -> Option<DynamicImage> {
    let mut head = Vec::new();
    std::fs::File::open(path).ok()?.take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    let tiff = exif_segment(&head)?;
    let (orientation, offset, len) = parse_tiff(tiff)?;
    let data = tiff.get(offset..offset.checked_add(len)?)?;
    let mut img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg).ok()?;
    // Zu kleine Vorschaubilder würden in der Liste unscharf.
    if img.width().min(img.height()) < size / 2 {
        return None;
    }
    img.apply_orientation(Orientation::from_exif(orientation).unwrap_or(Orientation::NoTransforms));
    Some(DynamicImage::ImageRgba8(img.into_rgba8()))
}

/// Inhalt des APP1-Segments „Exif“ (ohne Kennung), gesucht bis zum ersten Scan.
fn exif_segment(data: &[u8]) -> Option<&[u8]> {
    if data.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut pos = 2;
    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            return None;
        }
        let marker = data[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0xDA {
            return None;
        }
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        let body = data.get(pos + 4..pos + 2 + len)?;
        if marker == 0xE1 && body.starts_with(b"Exif\0\0") {
            return Some(&body[6..]);
        }
        pos += 2 + len;
    }
    None
}

/// Liest Drehung (IFD0) sowie Lage und Länge des Vorschaubilds (IFD1) aus einem TIFF-Block.
fn parse_tiff(t: &[u8]) -> Option<(u8, usize, usize)> {
    let le = match t.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |p: usize| -> Option<u16> {
        let b = [*t.get(p)?, *t.get(p + 1)?];
        Some(if le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    };
    let u32_at = |p: usize| -> Option<u32> {
        let b = [*t.get(p)?, *t.get(p + 1)?, *t.get(p + 2)?, *t.get(p + 3)?];
        Some(if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    };
    // Wert eines Eintrags: SHORT steht linksbündig im 4-Byte-Feld, LONG füllt es.
    let value = |entry: usize| -> Option<u32> {
        match u16_at(entry + 2)? {
            3 => u16_at(entry + 8).map(u32::from),
            4 => u32_at(entry + 8),
            _ => None,
        }
    };
    let ifd0 = u32_at(4)? as usize;
    let count0 = u16_at(ifd0)? as usize;
    let mut orientation = 1u8;
    for i in 0..count0 {
        let entry = ifd0 + 2 + i * 12;
        if u16_at(entry)? == 0x0112 {
            orientation = value(entry)?.try_into().ok()?;
        }
    }
    let ifd1 = u32_at(ifd0 + 2 + count0 * 12)? as usize;
    if ifd1 == 0 {
        return None;
    }
    let (mut offset, mut len) = (None, None);
    for i in 0..u16_at(ifd1)? as usize {
        let entry = ifd1 + 2 + i * 12;
        match u16_at(entry)? {
            0x0201 => offset = value(entry),
            0x0202 => len = value(entry),
            _ => {}
        }
    }
    Some((orientation, offset? as usize, len? as usize))
}

/// Füllt ein Quadrat: kürzere Seite auf `size` skalieren, Überstand mittig abschneiden.
fn cover(img: &DynamicImage, size: u32) -> RgbaImage {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let scale = size as f64 / w.min(h) as f64;
    let (sw, sh) = (((w as f64 * scale).ceil() as u32).max(size), ((h as f64 * scale).ceil() as u32).max(size));
    let rgba = DynamicImage::ImageRgba8(img.to_rgba8());
    let scaled = resize(&rgba, sw, sh).into_rgba8();
    image::imageops::crop_imm(&scaled, (sw - size) / 2, (sh - size) / 2, size, size).to_image()
}
