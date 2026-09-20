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
        }
    }

    pub fn applies_to(&self, category: Category) -> bool {
        match self {
            Processor::MinifyJs => category == Category::Js,
            Processor::MinifyCss => category == Category::Css,
            Processor::MinifyHtml => matches!(category, Category::Html | Category::Entry),
            Processor::PngOptimize { .. } => category == Category::Image,
            Processor::ImgConvert { .. } | Processor::ImgResize { .. } => {
                category == Category::Image
            }
        }
    }

    pub fn process(&self, key: &str, input: &[u8]) -> Result<Vec<u8>, String> {
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
        }
    }
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
