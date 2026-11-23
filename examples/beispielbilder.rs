//! Erzeugt Beispielbilder zum Ausprobieren: `cargo run --example beispielbilder -- <ordner> [anzahl]`.
use image::{Rgb, RgbImage, Rgba, RgbaImage};

fn photo(w: u32, h: u32, seed: u32) -> RgbImage {
    let mut s = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    RgbImage::from_fn(w, h, |x, y| {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = (s >> 26) & 0x3f;
        let wave = (((x as f32 / 90.0).sin() + (y as f32 / 70.0).cos()) * 40.0) as i32;
        let c = |v: u32| ((v + n) as i32 + wave).clamp(0, 255) as u8;
        Rgb([c(x * 255 / w), c(y * 255 / h), c((x + y + seed * 40) * 255 / (w + h + seed * 40))])
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("Zielordner angeben"));
    let count: u32 = args.next().and_then(|n| n.parse().ok()).unwrap_or(6);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..count {
        let (w, h) = if i % 3 == 2 { (4000, 6000) } else { (6000, 4000) };
        let mut out = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 97);
        photo(w, h, i).write_with_encoder(enc).unwrap();
        std::fs::write(dir.join(format!("IMG_{:04}.jpg", 412 + i)), out).unwrap();
    }
    let base = photo(2400, 1200, 99);
    let logo = RgbaImage::from_fn(2400, 1200, |x, y| {
        let p = base.get_pixel(x, y);
        let inside = (x as i32 - 1200).pow(2) / 4 + (y as i32 - 600).pow(2) < 500 * 500;
        Rgba([p[0], p[1], p[2], if inside { 255 } else { 0 }])
    });
    logo.save(dir.join("logo.png")).unwrap();
    std::fs::write(dir.join("kaputt.jpg"), b"kein Bild").unwrap();
    println!("{} Dateien in {}", count + 2, dir.display());
}
