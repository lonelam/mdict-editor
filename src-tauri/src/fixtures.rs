//! Programmatic fixtures shared by the gen_fixtures example and integration
//! tests. Builds small real mdx/mdd files plus external css/js on disk.
#![doc(hidden)]

use std::{fs, path::Path};

use image::{Rgb, RgbImage};

fn make_png(path: &Path, w: u32, h: u32, color: [u8; 3]) {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let grad = ((x + y) % 32) as u8;
            img.put_pixel(
                x,
                y,
                Rgb([color[0].saturating_add(grad / 2), color[1], color[2]]),
            );
        }
    }
    img.save(path).expect("save png");
}

/// 440 Hz, 16-bit mono PCM WAV, ~0.4 s.
pub fn make_wav() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 22050;
    let samples = SAMPLE_RATE * 2 / 5;
    let data_len = samples * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let v = (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        let amp = (1.0 - i as f32 / samples as f32) * 20000.0;
        out.extend_from_slice(&((v * amp) as i16).to_le_bytes());
    }
    out
}

pub const CSS_MAIN: &str = r#"@import url("extra.css");
.entry { font-family: sans-serif; color: #123456; }
.logo { background: url(../img/logo.png) no-repeat; width: 64px; height: 64px; }
.speaker { cursor: pointer; color: #2f6fed; }
"#;

pub const APP_JS: &str = r#"// dictionary helper
console.log("debug: app.js loaded");
function play(path) {
  var base = "/audio/";
  var audio = new Audio(base + path.replace(/^\//, ""));
  audio.play();
}
var ICON = "/img/heart.png";
"#;

/// Writes `ocean.mdx`, `assets.mdd`, `style-ext.css`, `script-ext.js` into
/// `dir`. Returns the three main paths (mdx, mdd, ext-css, ext-js).
pub fn write_all(dir: &Path) -> (String, String, String, String) {
    fs::create_dir_all(dir).expect("mkdir");

    let hello = r#"<div class="entry"><img src="/img/logo.png" alt="logo"><p>Hello!
See also: <a href="entry://world">world</a>.</p>
<span class="speaker" onclick="play('sound://audio/hello.wav')">&#9654; play</span>
<script src="/js/app.js"></script>
</div>"#;
    let world = r#"<div class="entry"><link rel="stylesheet" href="css/style.css">
<p>A world of <img src="../img/heart.png"> love.</p>
<p>Notes: <a href="docs/my%20file.txt">my file</a></p>
<span class="speaker" onclick="play('/audio/hello.wav')">&#9654;</span>
</div>"#;
    let apple = r#"<div class="entry"><p>An apple a day. <img src="/img/heart.png"></p></div>"#;
    let banana = r#"<div class="entry"><p>Banana <b>split</b>.</p></div>"#;
    let pear = r#"<div class="entry"><p>Pear. <a href="entry://banana">banana</a></p></div>"#;

    let mut mdx = mdictlib::MdxBuilder::with_options(
        mdictlib::WriteOptions::new()
            .with_encoding(mdictlib::WriteEncoding::Utf8)
            .with_compression(mdictlib::WriteCompression::Zlib),
    );
    mdx.header_attribute("Title", "Ocean Dictionary").unwrap();
    for (k, v) in [
        ("apple", apple),
        ("banana", banana),
        ("hello", hello),
        ("pear", pear),
        ("world", world),
        ("苹果", "<p>apple in Chinese</p>"),
    ] {
        mdx.add_entry(k, v).unwrap();
    }
    let mut mdx_bytes = Vec::new();
    mdx.finish(&mut mdx_bytes).unwrap();
    let mdx_path = dir.join("ocean.mdx");
    fs::write(&mdx_path, &mdx_bytes).unwrap();

    let css_extra = ".extra { border: 1px solid #eee; }\n";
    let mut mdd = mdictlib::MddBuilder::with_options(
        mdictlib::WriteOptions::new().with_compression(mdictlib::WriteCompression::Zlib),
    );
    mdd.header_attribute("Title", "Ocean Assets").unwrap();
    mdd.add_resource("css/style.css", CSS_MAIN.as_bytes()).unwrap();
    mdd.add_resource("css/extra.css", css_extra.as_bytes()).unwrap();
    mdd.add_resource("js/app.js", APP_JS.as_bytes()).unwrap();
    mdd.add_resource("audio/hello.wav", &make_wav()).unwrap();
    mdd.add_resource("docs/my file.txt", b"spaces in the middle").unwrap();
    mdd.add_resource("fonts/fake.ttf", &[0u8; 512]).unwrap();
    mdd.add_resource("data/blob.bin", &(0u8..=255).collect::<Vec<u8>>()).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    make_png(&tmp.path().join("logo.png"), 64, 64, [47, 111, 237]);
    make_png(&tmp.path().join("heart.png"), 32, 32, [176, 38, 30]);
    mdd.add_resource(
        "img/logo.png",
        &fs::read(tmp.path().join("logo.png")).unwrap(),
    )
    .unwrap();
    mdd.add_resource(
        "img/heart.png",
        &fs::read(tmp.path().join("heart.png")).unwrap(),
    )
    .unwrap();

    let mut mdd_bytes = Vec::new();
    mdd.finish(&mut mdd_bytes).unwrap();
    let mdd_path = dir.join("assets.mdd");
    fs::write(&mdd_path, &mdd_bytes).unwrap();

    let ext_css = dir.join("style-ext.css");
    fs::write(
        &ext_css,
        ".ext { padding: 4px; background: url(\"img/logo.png\"); }\n",
    )
    .unwrap();
    let ext_js = dir.join("script-ext.js");
    fs::write(
        &ext_js,
        "console.warn(\"external script\");\nwindow.MDICT_EXT = \"/img/logo.png\";\n",
    )
    .unwrap();

    (
        mdx_path.to_string_lossy().into_owned(),
        mdd_path.to_string_lossy().into_owned(),
        ext_css.to_string_lossy().into_owned(),
        ext_js.to_string_lossy().into_owned(),
    )
}
