//! Testbilder, die zur Laufzeit erzeugt werden.
#![allow(dead_code)]

use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use img_parts::{Bytes, ImageEXIF, ImageICC};
use std::path::{Path, PathBuf};

/// Farbverlauf mit deterministischem Rauschen, damit sich das Bild wie ein Foto komprimiert.
pub fn photo(w: u32, h: u32) -> RgbImage {
    let mut seed: u32 = 0x1234_5678;
    RgbImage::from_fn(w, h, |x, y| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = (seed >> 24) & 0x1f;
        let r = (x * 255 / w.max(1) + n).min(255) as u8;
        let g = (y * 255 / h.max(1) + n).min(255) as u8;
        let b = ((x + y) * 255 / (w + h).max(1) + n).min(255) as u8;
        Rgb([r, g, b])
    })
}

pub fn jpeg_bytes(w: u32, h: u32, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    photo(w, h).write_with_encoder(enc).unwrap();
    out
}

pub fn write_jpeg(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    write_jpeg_q(dir, name, w, h, 92)
}

pub fn write_jpeg_q(dir: &Path, name: &str, w: u32, h: u32, quality: u8) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, jpeg_bytes(w, h, quality)).unwrap();
    p
}

/// PNG, dessen linke Hälfte voll transparent (schwarz) ist.
pub fn write_png_alpha(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let base = photo(w, h);
    let img = RgbaImage::from_fn(w, h, |x, y| {
        if x < w / 2 {
            Rgba([0, 0, 0, 0])
        } else {
            let p = base.get_pixel(x, y);
            Rgba([p[0], p[1], p[2], 255])
        }
    });
    let p = dir.join(name);
    img.save_with_format(&p, ImageFormat::Png).unwrap();
    p
}

pub fn write_png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let p = dir.join(name);
    photo(w, h).save_with_format(&p, ImageFormat::Png).unwrap();
    p
}

/// TIFF-Block mit Orientation und einem GPS-IFD (GPSLatitudeRef = "N").
pub fn exif_blob(orientation: u16) -> Vec<u8> {
    let o = orientation.to_le_bytes();
    let mut v = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x02, 0x00];
    v.extend([0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, o[0], o[1], 0x00, 0x00]);
    v.extend([0x25, 0x88, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x26, 0x00, 0x00, 0x00]);
    v.extend([0x00, 0x00, 0x00, 0x00]);
    v.extend([0x01, 0x00]);
    v.extend([0x01, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00, 0x4E, 0x00, 0x00, 0x00]);
    v.extend([0x00, 0x00, 0x00, 0x00]);
    v
}

pub fn write_jpeg_exif(dir: &Path, name: &str, w: u32, h: u32, orientation: u16) -> PathBuf {
    write_jpeg_exif_q(dir, name, w, h, orientation, 92)
}

pub fn write_jpeg_exif_q(dir: &Path, name: &str, w: u32, h: u32, orientation: u16, q: u8) -> PathBuf {
    let mut jpeg = img_parts::jpeg::Jpeg::from_bytes(Bytes::from(jpeg_bytes(w, h, q))).unwrap();
    jpeg.set_exif(Some(Bytes::from(exif_blob(orientation))));
    let p = dir.join(name);
    std::fs::write(&p, jpeg.encoder().bytes()).unwrap();
    p
}

/// Kein gültiges Profil, aber ein eindeutig wiedererkennbarer Block.
pub fn dummy_icc() -> Vec<u8> {
    (0..600u32).map(|i| (i % 251) as u8).collect()
}

pub fn write_jpeg_icc(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let mut jpeg = img_parts::jpeg::Jpeg::from_bytes(Bytes::from(jpeg_bytes(w, h, 92))).unwrap();
    jpeg.set_icc_profile(Some(Bytes::from(dummy_icc())));
    let p = dir.join(name);
    std::fs::write(&p, jpeg.encoder().bytes()).unwrap();
    p
}

/// EXIF-Block (TIFF) mit IFD0 (Orientation) und IFD1, das auf ein eingebettetes Vorschaubild zeigt.
pub fn exif_with_thumbnail(orientation: u16, thumb: &[u8]) -> Vec<u8> {
    let o = orientation.to_le_bytes();
    let len = (thumb.len() as u32).to_le_bytes();
    let mut v = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
    // IFD0 bei 8: ein Eintrag, nächstes IFD bei 26
    v.extend([0x01, 0x00]);
    v.extend([0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, o[0], o[1], 0x00, 0x00]);
    v.extend(26u32.to_le_bytes());
    // IFD1 bei 26: Offset und Länge des Vorschaubilds, das bei 56 beginnt
    v.extend([0x02, 0x00]);
    v.extend([0x01, 0x02, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00]);
    v.extend(56u32.to_le_bytes());
    v.extend([0x02, 0x02, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00]);
    v.extend(len);
    v.extend(0u32.to_le_bytes());
    assert_eq!(v.len(), 56);
    v.extend_from_slice(thumb);
    v
}

pub fn solid_jpeg(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    RgbImage::from_pixel(w, h, Rgb(color)).write_with_encoder(enc).unwrap();
    out
}

/// Grünes Hauptbild mit rotem EXIF-Vorschaubild.
pub fn write_jpeg_with_thumb(dir: &Path, name: &str, orientation: u16) -> PathBuf {
    let main = solid_jpeg(1200, 800, [0, 200, 0]);
    let thumb = solid_jpeg(160, 120, [220, 0, 0]);
    let mut jpeg = img_parts::jpeg::Jpeg::from_bytes(Bytes::from(main)).unwrap();
    jpeg.set_exif(Some(Bytes::from(exif_with_thumbnail(orientation, &thumb))));
    let p = dir.join(name);
    std::fs::write(&p, jpeg.encoder().bytes()).unwrap();
    p
}

/// Eine Anfrage an den Test-Server.
pub struct FakeRequest {
    pub method: String,
    pub path: String,
    /// Kopfzeilen, Namen klein geschrieben.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl FakeRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// Minimaler HTTP-Server für KI-Tests. Liefert die Basis-URL `http://127.0.0.1:<port>/v1`.
pub fn fake_server(handler: impl Fn(&FakeRequest) -> (u16, String) + Send + Sync + 'static) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = std::sync::Arc::new(handler);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let handler = handler.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    return;
                }
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let mut headers = Vec::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if let Some((n, v)) = line.split_once(':') {
                        headers.push((n.trim().to_lowercase(), v.trim().to_string()));
                    }
                }
                let len = headers.iter().find(|(n, _)| n == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let request = FakeRequest { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() };
                let (status, reply) = handler(&request);
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = stream.flush();
            });
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

/// Antwort im Format von `chat/completions` mit diesem Inhalt.
pub fn chat_reply(content: &str) -> String {
    serde_json::json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] }).to_string()
}
