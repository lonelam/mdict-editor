//! Content processors: minification and image optimization. Each processor
//! declares which categories it applies to and is a pure bytes-in/bytes-out
//! transform (never touching sources directly — pipeline decides where the
//! output goes).

use serde::{Deserialize, Serialize};

use crate::category::Category;

/// One pipeline step. Serialized with a `kind` tag; optional params inline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Processor {
    MinifyJs,
    MinifyCss,
    MinifyHtml,
    PngOptimize {
        #[serde(default)]
        level: Option<u8>,
    },
    ImgConvert {
        format: String,
        #[serde(default)]
        quality: Option<u8>,
    },
    ImgResize {
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
    },
    /// Lossy WebP with alpha support (libwebp via webpx) — the highest-yield
    /// image transform for dictionary art.
    ImgWebp {
        /// 0-100, higher = better quality. Default 75.
        #[serde(default)]
        quality: Option<u8>,
    },
    /// pngquant-style palette quantization: RGBA → 8-bit palette PNG.
    PngQuantize {
        /// 2-256 colors. Default 256.
        #[serde(default)]
        colors: Option<u32>,
    },
    /// Drops CSS rules whose class/id selectors never appear in the entry
    /// corpus (collected by the pipeline/export job into JobCtl).
    CssPurge,
}

impl Processor {
    pub fn name(&self) -> &'static str {
        match self {
            Processor::MinifyJs => "minify-js",
            Processor::MinifyCss => "minify-css",
            Processor::MinifyHtml => "minify-html",
            Processor::PngOptimize { .. } => "png-optimize",
            Processor::ImgConvert { .. } => "img-convert",
            Processor::ImgResize { .. } => "img-resize",
            Processor::ImgWebp { .. } => "img-webp",
            Processor::PngQuantize { .. } => "png-quantize",
            Processor::CssPurge => "css-purge",
        }
    }

    pub fn applies_to(&self, category: Category) -> bool {
        match self {
            Processor::MinifyJs => category == Category::Js,
            Processor::MinifyCss | Processor::CssPurge => category == Category::Css,
            Processor::MinifyHtml => matches!(category, Category::Html | Category::Entry),
            Processor::PngOptimize { .. } => category == Category::Image,
            Processor::ImgConvert { .. }
            | Processor::ImgResize { .. }
            | Processor::ImgWebp { .. }
            | Processor::PngQuantize { .. } => category == Category::Image,
        }
    }

    pub fn process(
        &self,
        key: &str,
        input: &[u8],
        used_selectors: &std::collections::HashSet<String>,
    ) -> Result<Vec<u8>, String> {
        match self {
            Processor::MinifyJs => minify_js(input),
            Processor::MinifyCss => minify_css(input),
            Processor::MinifyHtml => minify_html(input),
            Processor::PngOptimize { level } => {
                if !key.to_lowercase().ends_with(".png") {
                    return Err("png-optimize applies to .png only".into());
                }
                oxipng::optimize_from_memory(
                    input,
                    &oxipng::Options::from_preset(level.unwrap_or(2).clamp(0, 6) as u8),
                )
                .map_err(|e| e.to_string())
            }
            Processor::ImgConvert { format, quality } => img_convert(input, format, *quality),
            Processor::ImgResize { width, height } => img_resize(key, input, *width, *height),
            Processor::ImgWebp { quality } => img_webp(input, *quality),
            Processor::PngQuantize { colors } => png_quantize(input, *colors),
            Processor::CssPurge => css_purge(input, used_selectors),
        }
    }
}

/// Applies a processor chain to one resource's bytes under the pipeline's
/// semantics: text processors run in order; image re-encodes are mutually
/// exclusive (the first matching one wins — layering webp onto quantize
/// onto jpeg only grows files); png-quantize only touches .png keys; jpeg
/// conversion skips transparent inputs. Returns the new bytes and whether
/// anything changed.
pub fn apply_chain(
    steps: &[Processor],
    key: &str,
    bytes: &[u8],
    used_selectors: &std::collections::HashSet<String>,
) -> (Vec<u8>, bool) {
    let lower_key = key.to_lowercase();
    let mut current = bytes.to_vec();
    let mut image_step_taken = false;
    for step in steps {
        let applies = match step {
            Processor::PngQuantize { .. } => lower_key.ends_with(".png"),
            other => other.applies_to(crate::category::Category::from_key(&lower_key)),
        };
        if !applies {
            continue;
        }
        if matches!(
            step,
            Processor::ImgWebp { .. } | Processor::ImgConvert { .. } | Processor::PngQuantize { .. }
        ) {
            if image_step_taken {
                continue; // one image re-encode per resource
            }
            if let Processor::ImgConvert { format, .. } = step {
                if format.eq_ignore_ascii_case("jpeg") && has_alpha_bytes(bytes) {
                    continue;
                }
            }
            image_step_taken = true;
        }
        if let Ok(out) = step.process(key, &current, used_selectors) {
            current = out;
        }
    }
    let changed = current != bytes;
    (current, changed)
}

fn has_alpha_bytes(bytes: &[u8]) -> bool {
    let Ok(img) = image::load_from_memory(bytes) else {
        return false;
    };
    img.to_rgba8().pixels().any(|p| p.0[3] != 255)
}

/// Lossy WebP encode (alpha-safe). Keys keep their original extension —
/// browsers sniff content, so `xxx.png` holding WebP bytes still renders.
fn img_webp(input: &[u8], quality: Option<u8>) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(input).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let config =
        webpx::EncoderConfig::new().quality(quality.unwrap_or(75).clamp(1, 100) as f32);
    config
        .encode_rgba(rgba.as_raw(), w, h, webpx::Unstoppable)
        .map_err(|e| format!("{e:?}"))
}

/// pngquant-style quantization: RGBA -> palette PNG with alpha (tRNS).
fn png_quantize(input: &[u8], colors: Option<u32>) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(input).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut attrs = imagequant::Attributes::new();
    attrs
        .set_max_colors(colors.unwrap_or(256).clamp(2, 256))
        .map_err(|e| format!("{e:?}"))?;
    let pixels: Vec<imagequant::RGBA> = rgba
        .pixels()
        .map(|p| imagequant::RGBA {
            r: p.0[0],
            g: p.0[1],
            b: p.0[2],
            a: p.0[3],
        })
        .collect();
    let mut liq_img = attrs
        .new_image(pixels, w as usize, h as usize, 0.0)
        .map_err(|e| format!("{e:?}"))?;
    let mut quant = attrs.quantize(&mut liq_img).map_err(|e| format!("{e:?}"))?;
    quant.set_dithering_level(1.0).map_err(|e| format!("{e:?}"))?;
    let (palette, indexes) = quant.remapped(&mut liq_img).map_err(|e| format!("{e:?}"))?;
    if palette.len() > 256 {
        return Err("palette exceeded 256 entries".into());
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        let mut flat = Vec::with_capacity(palette.len() * 3);
        let mut trns = Vec::with_capacity(palette.len());
        for c in &palette {
            flat.extend_from_slice(&[c.r, c.g, c.b]);
            trns.push(c.a);
        }
        encoder.set_palette(flat);
        encoder.set_trns(trns);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&indexes).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Removes CSS rules whose `.class`/`#id` selectors never appear in the
/// entry corpus. A selector survives when every class/id it mentions is in
/// the corpus; selectors with no class/id at all (tag, `*`) are always kept.
/// An empty corpus keeps everything.
fn css_purge(input: &[u8], used: &std::collections::HashSet<String>) -> Result<Vec<u8>, String> {
    if used.is_empty() {
        return Ok(input.to_vec());
    }
    let mut stylesheet = match lightningcss::stylesheet::StyleSheet::parse(
        std::str::from_utf8(input).map_err(|_| "css is not utf-8".to_string())?,
        lightningcss::stylesheet::ParserOptions::default(),
    ) {
        Ok(s) => s,
        Err(_) => return Ok(input.to_vec()),
    };
    use lightningcss::rules::CssRule;
    stylesheet
        .rules
        .0
        .retain(|rule| match rule {
            CssRule::Style(style) => style
                .selectors
                .0
                .iter()
                .all(|c| {
                    use lightningcss::traits::ToCss;
                    let mut buf = String::new();
                    {
                        let mut printer = lightningcss::printer::Printer::new(
                            &mut buf,
                            Default::default(),
                        );
                        if c.to_css(&mut printer).is_err() {
                            return true; // cannot serialize — keep the rule
                        }
                    }
                    selector_used(&buf, used)
                }),
            _ => true, // keep at-rules verbatim
        });
    match stylesheet.to_css(lightningcss::stylesheet::PrinterOptions {
        minify: true,
        ..Default::default()
    }) {
        Ok(out) => Ok(out.code.into_bytes()),
        Err(_) => Ok(input.to_vec()),
    }
}

fn selector_used(selector_text: &str, used: &std::collections::HashSet<String>) -> bool {
    let mut chars = selector_text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '.' || c == '#' {
            let mut name = String::new();
            for n in chars.by_ref() {
                if n.is_alphanumeric() || n == '-' || n == '_' {
                    name.push(n);
                } else {
                    break;
                }
            }
            if !name.is_empty() && !used.contains(&name) {
                return false;
            }
        }
    }
    true
}

fn minify_css(input: &[u8]) -> Result<Vec<u8>, String> {
    let src = std::str::from_utf8(input).map_err(|_| "css is not utf-8")?;
    let sheet = lightningcss::stylesheet::StyleSheet::parse(
        src,
        lightningcss::stylesheet::ParserOptions::default(),
    )
    .map_err(|e| format!("{e}"))?;
    let out = sheet
        .to_css(lightningcss::stylesheet::PrinterOptions {
            minify: true,
            ..Default::default()
        })
        .map_err(|e| format!("{e}"))?;
    Ok(out.code.into_bytes())
}

fn minify_html(input: &[u8]) -> Result<Vec<u8>, String> {
    // Conservative config: dictionary HTML is often malformed. We strip
    // whitespace but never omit closing tags (that would change semantics of
    // mismatched markup); embedded css/js are left to dedicated processors.
    let cfg = minify_html::Cfg {
        keep_closing_tags: true,
        ..Default::default()
    };
    Ok(minify_html::minify(input, &cfg))
}

#[cfg(feature = "processors-js")]
fn minify_js(input: &[u8]) -> Result<Vec<u8>, String> {
    use swc_core::common::{
        comments::{Comments, SingleThreadedComments},
        sync::Lrc,
        FileName, Globals, SourceMap, GLOBALS,
    };
    use swc_core::ecma::ast::EsVersion;
    use swc_core::ecma::codegen::{text_writer::JsWriter, Config as CodegenConfig, Emitter};
    use swc_core::ecma::minifier::{
        optimize,
        option::{CompressOptions, ExtraOptions, MangleOptions, MinifyOptions},
    };
    use swc_core::ecma::parser::{lexer::Lexer, Parser, StringInput, EsSyntax, Syntax};

    let src = String::from_utf8_lossy(input).into_owned();
    let cm: Lrc<SourceMap> = Lrc::default();
    let globals = Globals::new();
    GLOBALS.set(&globals, || {
        let fm = cm.new_source_file(Lrc::new(FileName::Custom("input.js".into())), src);
        let comments = SingleThreadedComments::default();
        let lexer = Lexer::new(
            Syntax::Es(EsSyntax::default()),
            EsVersion::EsNext,
            StringInput::from(&*fm),
            Some(&comments),
        );
        let mut parser = Parser::new_from(lexer);
        let program = parser.parse_program().map_err(|e| format!("{e:?}"))?;

        let unresolved_mark = swc_core::common::Mark::new();
        let top_level_mark = swc_core::common::Mark::new();
        let options = MinifyOptions {
            compress: Some(CompressOptions::default()),
            mangle: Some(MangleOptions::default()),
            ..Default::default()
        };
        let extra = ExtraOptions {
            unresolved_mark,
            top_level_mark,
            mangle_name_cache: Default::default(),
        };
        let optimized = optimize(
            program,
            cm.clone(),
            Some(&comments as &dyn Comments),
            None,
            &options,
            &extra,
        );

        let mut buf: Vec<u8> = Vec::new();
        {
            let wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
            let mut emitter = Emitter {
                cfg: CodegenConfig::default().with_minify(true),
                cm: cm.clone(),
                comments: None,
                wr,
            };
            emitter
                .emit_program(&optimized)
                .map_err(|e| format!("codegen: {e:?}"))?;
        }
        Ok(buf)
    })
}

#[cfg(not(feature = "processors-js"))]
fn minify_js(input: &[u8]) -> Result<Vec<u8>, String> {
    Err("built without the processors-js feature".into())
}

// ---- image helpers ----

fn image_format_of(key_lower: &str) -> Option<image::ImageFormat> {
    let ext = key_lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    Some(match ext {
        "png" => image::ImageFormat::Png,
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "gif" => image::ImageFormat::Gif,
        "bmp" => image::ImageFormat::Bmp,
        "webp" => image::ImageFormat::WebP,
        _ => return None,
    })
}

fn img_convert(input: &[u8], format: &str, quality: Option<u8>) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(input).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let cursor = std::io::Cursor::new(&mut out);
    match format.to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(cursor, quality.unwrap_or(80));
            img.write_with_encoder(enc).map_err(|e| e.to_string())?;
        }
        "webp" => {
            let enc = image::codecs::webp::WebPEncoder::new_lossless(cursor);
            img.write_with_encoder(enc).map_err(|e| e.to_string())?;
        }
        "png" => {
            let enc = image::codecs::png::PngEncoder::new(cursor);
            img.write_with_encoder(enc).map_err(|e| e.to_string())?;
        }
        other => return Err(format!("unsupported target format: {other}")),
    }
    Ok(out)
}

fn img_resize(key: &str, input: &[u8], width: Option<u32>, height: Option<u32>) -> Result<Vec<u8>, String> {
    if width.is_none() && height.is_none() {
        return Err("resize needs width and/or height".into());
    }
    let img = image::load_from_memory(input).map_err(|e| e.to_string())?;
    // Image::resize scales to fit within the (w, h) bound, keeping the
    // aspect ratio; a None bound means "no limit in that dimension".
    let resized = img.resize(
        width.unwrap_or(u32::MAX),
        height.unwrap_or(u32::MAX),
        image::imageops::FilterType::Lanczos3,
    );
    let fmt = image_format_of(&key.to_lowercase()).unwrap_or(image::ImageFormat::Png);
    let mut out = Vec::new();
    let cursor = std::io::Cursor::new(&mut out);
    match fmt {
        image::ImageFormat::Jpeg => {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(cursor, 85);
            resized.write_with_encoder(enc).map_err(|e| e.to_string())?;
        }
        image::ImageFormat::Gif => {
            let mut enc = image::codecs::gif::GifEncoder::new(cursor);
            let frame = image::Frame::new(resized.to_rgba8());
            enc.encode_frame(frame).map_err(|e| e.to_string())?;
        }
        _ => {
            let enc = image::codecs::png::PngEncoder::new(cursor);
            resized.write_with_encoder(enc).map_err(|e| e.to_string())?;
        }
    }
    Ok(out)
}
