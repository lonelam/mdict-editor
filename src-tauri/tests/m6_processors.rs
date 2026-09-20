//! New lossy processors: webp (alpha-safe), png palette quantization,
//! css purge against an entry corpus.

use std::collections::HashSet;

use mdict_editor_lib::processors::Processor;

fn gradient_png(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, if x % 7 == 0 { 128 } else { 255 }]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn img_webp_encodes_with_alpha() {
    let png = gradient_png(64, 48);
    let webp = Processor::ImgWebp { quality: Some(70) }
        .process("logo.png", &png, &HashSet::new())
        .unwrap();
    assert_eq!(&webp[..4], b"RIFF");
    assert_eq!(&webp[8..12], b"WEBP");
    assert!(webp.len() < png.len(), "webp {} vs png {}", webp.len(), png.len());
}

#[test]
fn png_quantize_produces_palette_png() {
    let png = gradient_png(100, 80);
    let out = Processor::PngQuantize { colors: Some(64) }
        .process("logo.png", &png, &HashSet::new())
        .unwrap();
    assert_eq!(image::guess_format(&out).unwrap(), image::ImageFormat::Png);
    // Decodes back with the same geometry (palette + tRNS).
    let img = image::load_from_memory(&out).unwrap();
    assert_eq!((img.width(), img.height()), (100, 80));
    // Palette PNG: at most 1 byte per pixel + palette chunks.
    assert!(out.len() <= 100 * 80 + 4096, "palette png too large: {}", out.len());
}

#[test]
fn css_purge_drops_unused_rules_keeps_used_and_tag_rules() {
    let css = br#"
.used-class { color: red; }
.unused-class { color: blue; }
div p { margin: 0; }
.used-class:hover span { padding: 1px; }
#unused-id { border: 1px; }
"#;
    let mut used = HashSet::new();
    used.insert("used-class".to_string());
    let out = Processor::CssPurge
        .process("style.css", css, &used)
        .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("used-class"), "{text}");
    assert!(!text.contains("unused-class"), "{text}");
    assert!(!text.contains("unused-id"), "{text}");
    assert!(text.contains("div p"), "tag-only rule kept: {text}");
}

#[test]
fn css_purge_empty_corpus_keeps_everything() {
    let css = b".never-appears { color: red; }";
    let out = Processor::CssPurge
        .process("style.css", css, &HashSet::new())
        .unwrap();
    assert!(String::from_utf8(out).unwrap().contains("never-appears"));
}
