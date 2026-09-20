s = open('src/processors.rs', encoding='utf-8').read()

# 新 Processor 变体
old = """#[derive(Debug, Clone, Serialize, Deserialize)]
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
}"""
new = """#[derive(Debug, Clone, Serialize, Deserialize)]
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
}"""
assert old in s
s = s.replace(old, new)

old = """    pub fn name(&self) -> &'static str {
        match self {
            Processor::MinifyJs => "minify-js",
            Processor::MinifyCss => "minify-css",
            Processor::MinifyHtml => "minify-html",
            Processor::PngOptimize { .. } => "png-optimize",
            Processor::ImgConvert { .. } => "img-convert",
            Processor::ImgResize { .. } => "img-resize",
        }
    }"""
new = """    pub fn name(&self) -> &'static str {
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
    }"""
assert old in s
s = s.replace(old, new)

old = """    pub fn applies_to(&self, category: Category) -> bool {
        match self {
            Processor::MinifyJs => category == Category::Js,
            Processor::MinifyCss => category == Category::Css,
            Processor::MinifyHtml => matches!(category, Category::Html | Category::Entry),
            Processor::PngOptimize { .. } => category == Category::Image,
            Processor::ImgConvert { .. } | Processor::ImgResize { .. } => {
                category == Category::Image
            }
        }
    }"""
new = """    pub fn applies_to(&self, category: Category) -> bool {
        match self {
            Processor::MinifyJs => category == Category::Js,
            Processor::MinifyCss | Processor::CssPurge => category == Category::Css,
            Processor::MinifyHtml => matches!(category, Category::Html | Category::Entry),
            Processor::PngOptimize { .. } => category == Category::Image,
            Processor::ImgConvert { .. } | Processor::ImgResize { .. } => category == Category::Image,
            Processor::ImgWebp { .. } | Processor::PngQuantize { .. } => category == Category::Image,
        }
    }"""
assert old in s
s = s.replace(old, new)

old = """    pub fn process(&self, key: &str, input: &[u8]) -> Result<Vec<u8>, String> {
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
}"""
new = """    pub fn process(
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

/// Lossy WebP encode (alpha-safe). Keys keep their original extension —
/// browsers sniff content, so `xxx.png` holding WebP bytes still renders.
fn img_webp(input: &[u8], quality: Option<u8>) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(input).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let config = webpx::EncoderConfig::new()
        .quality(quality.unwrap_or(75).clamp(1, 100) as f32);
    config
        .encode_rgba(rgba.as_raw(), w, h, webpx::Unstoppable)
        .map_err(|e| format!("{e:?}"))
}

/// pngquant-style quantization: RGBA → palette PNG with alpha (tRNS).
fn png_quantize(input: &[u8], colors: Option<u32>) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(input).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut attrs = imagequant::Attributes::new();
    attrs.set_max_colors(colors.unwrap_or(256).clamp(2, 256)).map_err(|e| format!("{e:?}"))?;
    let mut liq_img = attrs
        .new_image(rgba.as_raw().clone(), w as usize, h as usize, 0.0)
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
        let mut flat = Vec::with_capacity(palette.len() * 3);
        let mut trns = Vec::
