//! Zeichnet das Programmsymbol: `cargo run --example icon` schreibt `assets/icon.ico` und `assets/icon.png`.
//!
//! Motiv: großer Bildrahmen (Original), darin ein kleineres Bild, ein Pfeil zeigt die Verkleinerung.

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::{ExtendedColorType, Rgba, RgbaImage};

/// Zeichenfläche in Einheiten; jede Größe wird daraus gerechnet.
const CANVAS: f32 = 256.0;

type Color = [f32; 4];

fn hex(rgb: u32, a: f32) -> Color {
    [((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0, a]
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// Abstand zu einem Rechteck mit runden Ecken (negativ innen).
fn rounded_rect(p: (f32, f32), min: (f32, f32), max: (f32, f32), r: f32) -> f32 {
    let c = ((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0);
    let half = ((max.0 - min.0) / 2.0 - r, (max.1 - min.1) / 2.0 - r);
    let q = ((p.0 - c.0).abs() - half.0, (p.1 - c.1).abs() - half.1);
    let outside = (q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt();
    outside + q.0.max(q.1).min(0.0) - r
}

/// Abstand zu einer Strecke.
fn segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (pa, ba) = ((p.0 - a.0, p.1 - a.1), (b.0 - a.0, b.1 - a.1));
    let h = ((pa.0 * ba.0 + pa.1 * ba.1) / (ba.0 * ba.0 + ba.1 * ba.1)).clamp(0.0, 1.0);
    ((pa.0 - ba.0 * h).powi(2) + (pa.1 - ba.1 * h).powi(2)).sqrt()
}

/// Abstand zu einem Dreieck (negativ innen).
fn triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let d = segment(p, a, b).min(segment(p, b, c)).min(segment(p, c, a));
    let side = |u: (f32, f32), v: (f32, f32)| (v.0 - u.0) * (p.1 - u.1) - (v.1 - u.1) * (p.0 - u.0);
    let (s1, s2, s3) = (side(a, b), side(b, c), side(c, a));
    let inside = (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0) || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0);
    if inside { -d } else { d }
}

fn circle(p: (f32, f32), c: (f32, f32), r: f32) -> f32 {
    ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt() - r
}

/// Legt eine Farbe mit Deckung `alpha` über `dst`.
fn over(dst: Color, src: Color, alpha: f32) -> Color {
    let a = src[3] * alpha;
    let out_a = a + dst[3] * (1.0 - a);
    if out_a <= 0.0 {
        return [0.0; 4];
    }
    let ch = |i: usize| (src[i] * a + dst[i] * dst[3] * (1.0 - a)) / out_a;
    [ch(0), ch(1), ch(2), out_a]
}

fn render(size: u32) -> RgbaImage {
    let unit = CANVAS / size as f32; // Einheiten pro Pixel
    let small = size <= 24;
    // Deckung aus Abstand, kantengeglättet über einen Pixel.
    let cov = |d: f32| (0.5 - d / unit).clamp(0.0, 1.0);
    let white = hex(0xffffff, 1.0);
    let magenta = hex(0xe83e91, 1.0);
    let purple = hex(0x7b2ff7, 1.0);
    RgbaImage::from_fn(size, size, |x, y| {
        let p = ((x as f32 + 0.5) * unit, (y as f32 + 0.5) * unit);
        let mut c: Color = [0.0; 4];

        // Hintergrund: Verlauf von Magenta (oben links) nach Lila (unten rechts).
        let t = ((p.0 + p.1) / (2.0 * CANVAS)).clamp(0.0, 1.0);
        let bg = mix(magenta, purple, t * 0.85);
        c = over(c, bg, cov(rounded_rect(p, (8.0, 8.0), (248.0, 248.0), 56.0)));

        // Großer Rahmen (Original), bei kleinen Größen weggelassen.
        if !small {
            let frame = rounded_rect(p, (44.0, 44.0), (212.0, 212.0), 20.0).abs() - 6.0;
            c = over(c, hex(0xffffff, 0.55), cov(frame));
        }

        // Kleines Bild unten links.
        let (min, max) = if small { ((40.0, 96.0), (160.0, 216.0)) } else { ((44.0, 112.0), (144.0, 212.0)) };
        c = over(c, white, cov(rounded_rect(p, min, max, if small { 18.0 } else { 14.0 })));
        // Darin Berg und Sonne.
        let w = max.0 - min.0;
        let at = |fx: f32, fy: f32| (min.0 + w * fx, min.1 + w * fy);
        c = over(c, magenta, cov(triangle(p, at(0.12, 0.85), at(0.45, 0.38), at(0.78, 0.85))));
        c = over(c, purple, cov(triangle(p, at(0.48, 0.85), at(0.68, 0.56), at(0.9, 0.85))));
        c = over(c, magenta, cov(circle(p, at(0.74, 0.28), w * 0.11)));

        // Pfeil von oben rechts auf die Ecke des kleinen Bildes.
        let stroke = if small { 20.0 } else { 13.0 };
        let (from, to) = if small { ((214.0, 42.0), (176.0, 80.0)) } else { ((198.0, 58.0), (160.0, 96.0)) };
        c = over(c, white, cov(segment(p, from, to) - stroke / 2.0));
        let head = if small { 46.0 } else { 34.0 };
        let tip = (to.0 - 6.0, to.1 + 6.0);
        c = over(c, white, cov(triangle(p, tip, (tip.0 + head, tip.1), (tip.0, tip.1 - head))));

        Rgba([0, 1, 2, 3].map(|i| (c[i].clamp(0.0, 1.0) * 255.0).round() as u8))
    })
}

fn main() {
    std::fs::create_dir_all("assets").unwrap();
    let sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256];
    let frames: Vec<IcoFrame> = sizes
        .iter()
        .map(|&s| {
            let img = render(s);
            IcoFrame::as_png(img.as_raw(), s, s, ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    let file = std::fs::File::create("assets/icon.ico").unwrap();
    IcoEncoder::new(file).encode_images(&frames).unwrap();
    render(256).save("assets/icon.png").unwrap();
    // Übersicht zum Anschauen: alle Größen nebeneinander.
    let total: u32 = sizes.iter().sum::<u32>() + 10 * sizes.len() as u32;
    let mut sheet = RgbaImage::from_pixel(total, 256, Rgba([40, 40, 44, 255]));
    let mut x = 0;
    for &s in &sizes {
        image::imageops::overlay(&mut sheet, &render(s), x as i64, 0);
        x += s + 10;
    }
    sheet.save("target/icon-sheet.png").unwrap();
    println!("assets/icon.ico ({} Größen), assets/icon.png", sizes.len());
}
