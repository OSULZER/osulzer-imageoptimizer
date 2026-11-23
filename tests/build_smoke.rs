//! Nachweis, dass die C-Bibliotheken bauen und linken.
mod common;

#[test]
fn mozjpeg_encodes() {
    let img = common::photo(64, 48);
    let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    comp.set_size(64, 48);
    comp.set_quality(80.0);
    let mut started = comp.start_compress(Vec::new()).unwrap();
    started.write_scanlines(img.as_raw()).unwrap();
    let data = started.finish().unwrap();
    assert_eq!(&data[..2], &[0xFF, 0xD8]);
}

#[test]
fn webp_encodes() {
    let img = common::photo(64, 48);
    let mem = webp::Encoder::from_rgb(img.as_raw(), 64, 48).encode(80.0);
    assert_eq!(&mem[..4], b"RIFF");
}

#[test]
fn oxipng_optimizes() {
    let img = common::photo(64, 48);
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let out = oxipng::optimize_from_memory(&png, &oxipng::Options::from_preset(2)).unwrap();
    let back = image::load_from_memory(&out).unwrap().to_rgb8();
    assert_eq!(back.as_raw(), img.as_raw());
}
