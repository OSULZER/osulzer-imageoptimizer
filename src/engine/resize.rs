//! Zielabmessungen und Skalieren.

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::DynamicImage;

/// Abmessungen, wenn die längste Kante höchstens `max_edge` sein darf. Vergrößert nie.
pub fn target_size(w: u32, h: u32, max_edge: Option<u32>) -> (u32, u32) {
    let longest = w.max(h);
    let Some(max) = max_edge.filter(|&m| m > 0 && m < longest) else {
        return (w, h);
    };
    let scale = |v: u32| ((v as f64 * max as f64 / longest as f64).round() as u32).max(1);
    (scale(w), scale(h))
}

/// Skaliert auf genau `w`×`h` (Lanczos3). Erwartet 8 Bit RGB oder RGBA.
pub fn resize(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    if (img.width(), img.height()) == (w, h) {
        return img.clone();
    }
    let mut dst = DynamicImage::new(w, h, img.color());
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    match Resizer::new().resize(img, &mut dst, &options) {
        Ok(()) => dst,
        // Nur bei nicht unterstütztem Pixeltyp; `decode` liefert immer 8 Bit.
        Err(_) => img.resize_exact(w, h, image::imageops::FilterType::Lanczos3),
    }
}
