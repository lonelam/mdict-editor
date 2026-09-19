# 词典资源有损压缩调研（目标：整体体积降至 10%~30%）

> 状态：调研结论 + 管线集成设计 · 待评审
> 前置：[DESIGN.md §8 处理管线](DESIGN.md)

## 0. 结论摘要

1. **MDict 格式内已无压缩空间**：mdictlib 写出时逐块 `miniz_oxide zlib level 6`
   （`write/mod.rs::envelope`），格式不允许更换块压缩算法。要达到 10%~30%，
   **必须对资源内容做有损转换**，压缩发生在"字节进块之前"。
2. 词典 MDD 的体积几乎全部由 **图片、发音音频、内嵌字体** 构成（文本词条经 zlib 后
   通常只占 5%~20%）。这三类各有成熟的有损方案，收益 3×~15×。
3. 推荐按四个阶段落地（见 §3），全部做完后，图片/音频/字体主导的词典
   **整体 10%~30% 是可达成目标**；纯文本词典受格式约束，实际下限约 40%~60%。

## 1. 现状盘点：当前管线可压缩的资源

| 处理器 | 作用对象 | 有损性 | 原理 | 典型收益 |
|--------|---------|--------|------|---------|
| `minify-js` (swc) | Js | 无损（含标识符 mangling） | 解析→树优化→紧凑代码生成 | 30%~60% |
| `minify-css` (lightningcss) | Css | 无损 | 解析→紧凑打印 | 20%~40% |
| `minify-html` (minify-html) | Html/Entry | 无损（保守：不省略闭合标签） | 空白折叠、引号省略 | 10%~30% |
| `png-optimize` (oxipng) | Image(.png) | 无损 | 重压探索、块重排 | 5%~30% |
| `img-resize` (image) | Image | **有损**（降分辨率） | Lanczos3 缩放 | 与像素数成正比 |
| `img-convert` (image) | Image | **有损**（jpeg q）/无损（png、webp-无损） | 重编码 | jpeg 60%~80% |

注：词条 HTML 与 MDD 内 CSS/JS 都会被导出时的 zlib L6 再压一遍，minify 的收益
与 zlib 收益部分重叠（minify 去掉的信息熵让 zlib 更有效，实际是相乘关系）。

**缺口**：图片没有有损 WebP/调色板量化，音频完全未处理，字体未子集化，
CSS 没有"死规则清除"（词典 CSS 常一半以上规则用不到）。

## 2. 候选有损技术调研

### 2.1 图片 → 有损 WebP（最大预期收益项）

- **原理**：libwebp 有损模式 q50~70，对词典插图/图标类 PNG 一般缩小 3~6×
  （即降至原来的 15%~30%）。
- **Rust 生态**：[`webpx`](https://github.com/imazen/webpx)（libwebp 完整绑定，
  有损+无损+预设，维护活跃）。注意旧 `webp` crate 有
  [RUSTSEC-2024-0443](https://rustsec.org)（编码时可能暴露内存）——避开。
- **词典适配**：词条 HTML 里 `<img>` 由浏览器渲染，**浏览器按内容嗅探 MIME**，
  字节换成 WebP 而键名保持 `xxx.png` 也能正常显示 → 键名/引用完整性零成本保持。
- **风险**：libwebp 为 C 依赖（MSVC 可编译，需加 feature gate）。
- **收益量级**：图片主导的 MDD 整体可降至 **20%~35%**。

### 2.2 图片 → 256 色调色板量化（pngquant 同源）

- **原理**：libimagequant 把 RGBA 量化为 8-bit 调色板 PNG，保透明度，典型再省 ~70%，
  与 oxipng 组合是 pngquant 的标准玩法。
- **Rust 生态**：[`imagequant`](https://crates.io/crates/imagequant)（pngquant 作者
  维护的安全封装；pngquant 2.13+ 的 libimagequant 本身就是 Rust 写的）。
- **许可**：本项目为 MIT 开源 demo，GPL 组件无分发顾虑，可直接使用。
- **收益量级**：作为 WebP 的补充（需要保 PNG 格式时），单图 60%~75%。

### 2.3 发音音频 → Opus（词典音频的标准答案）

- **原理**：语音场景 Opus 16 kHz 采样、16~32 kbps 单声道即可达到接近原始清晰度，
  相对 WAV **5~10×** 缩减；相对 128kbps MP3 也有 3~5×。
- **依据**：[移动端短语音选型](https://cloud.baidu.com)（16kHz+32kbps 近 PCM 质量、
  解码仅 ~15ms/s）、[Opus 官方](https://en.wikipedia.org/wiki/Opus_(audio_format))。
- **Rust 生态**：解码用 [`symphonia`](https://docs.rs)（纯 Rust，mp3/wav/ogg 全解）；
  编码需 `libopus`（`audiopus`/`opus-sys` 绑定，C 依赖）+ `ogg` 容器封装。
- **词典适配与风险**：
  - 键名保持 `.wav`/`.mp3` 内容换 Opus/Ogg：Chromium 内核（WebView2、GoldenDict-ng、
    DictTango）按内容嗅探可播；**旧版 MDict PC 可能不认** → 做成显式勾选的处理器，
    文档标注兼容性。
  - 后续可加"引用重写"（把词条里 `sound://x.wav` 改写为 `x.opus`），彻底解决兼容性，
    复用现有 Resolver 的引用扫描即可。
- **收益量级**：音频主导的 MDD（发音词典）整体可降至 **10%~20%**。

### 2.4 内嵌字体 → 按码点子集化

- **原理**：词典内嵌 CJK 字体常 5~20MB，而词典实际用到的字符往往只有几万个甚至更少；
  子集化只保留出现过的码点，案例实测 **~87% 缩减**（[bytes.zone 实测](https://bytes.zone)）。
- **Rust 生态**（三选一）：
  - [`allsorts`](https://lib.rs)（Yeslogic/PrinceXML 团队，纯 Rust，支持 OpenType/WOFF/WOFF2
    子集化，最成熟）；
  - `font-subset`（纯 Rust no_std，输出 OTF/WOFF2，轻量）；
  - `hb_subset`（HarfBuzz C 绑定，保真度最高）。
- **词典适配**：子集码点集 = 全部词条文本 ∪ 词条 HTML 内可见字符 ∪ CSS content 字符。
  我们已有全量键索引与词条读取能力，码点收集是一次遍历。**风险点**：动态拼接的
  字符（JS 生成）无法预收集 → 提供"常用字兜底包"（如通用规范汉字表 8105 字）选项。
- **收益量级**：内嵌字体词典整体可降至 **15%~40%**。

### 2.5 CSS 死规则清除（语义级有损）

- **原理**：扫描全部词条 HTML，收集实际出现的 class/tag/id，删除 CSS 中未被引用的
  规则。词典 CSS 常从通用模板带来 50%~90% 死规则。
- **Rust 生态**：lightningcss 已在依赖内（有
  [`unusedSymbols`](https://lightningcss.dev) 最小化选项）；完整 purge 需自实现
  "HTML 收集选择器 → 过滤规则"，参考 [DropCSS](https://github.com/leeoniya/dropcss)
  的做法，用 lightningcss 的解析 AST 过滤即可（数天工作量）。
- **风险**：JS 动态添加的 class 会被误删 → 保守模式：只删"整个选择器列表均未出现"
  的规则；提供白名单。
- **收益量级**：CSS 文件本身 50%~90%，对整体贡献取决于 CSS 占比（5%~15%）。

### 2.6 容器级：zstd 共享字典（需放弃/扩展 MDict 格式，仅记录）

- **原理**：zstd 训练字典对**小记录**（词条典型 ~1KB）提升巨大：
  官方基准 2.8×→6.9×（[zstd](https://github.com/facebook/zstd)、
  [DebugBear](https://www.debugbear.com/blog/shared-compression-dictionaries)、
  HTTP 字典传输实测 [up to 90%](https://httptoolkit.com/blog/http-dictionary-compression/)）。
- **限制**：MDict 块压缩固定 zlib，**格式内不可用**。仅当未来提供自有
  `.zdict` 导出格式（zstd -22 + 512KB 训练字典 + 键索引）时启用，
  是达成"文本部分也到 10%"的唯一路径。`zstd` crate 支持
  `train_dictionary`/`with_dictionary`，落地成本低，但生态兼容性归零。

## 3. 推荐落地路线（管线集成设计）

### 3.0 执行位置：有损压缩只在导出时进行（架构约束）

有损操作不可逆，因此**绝不进入改写层（Overlay）**，也不出现在交互式批量管线中。
导出时生成两套产物：

- `<名>.edited.mdx / .edited.mdd` —— **原始版本**：只含 Overlay 中的显式编辑，内容保真；
- `<名>.lossy.mdd` —— **压缩副本**：对每个 MDD 源重建时即时套用有损转换链
  （含保护规则：透明图不转 JPEG；单步失败保留原字节）。

Overlay 与源文件永远拿不到有损后的字节。图片编辑器里的缩放/转格式属于显式的
单资源编辑（原始字节保存在修订链 `original` 中，可随时还原），与导出有损副本
是两个通道。有损副本对**所有** MDD 源生成（无论有无编辑），适合直接分发。

新增 `Processor` 变体（有损链在 `ExportConfig.lossy` 传入，复用 dry-run 式
"单项失败不断批"的语义；无损处理器仍走交互管线）：

| 阶段 | 处理器 | 依赖 | 应用时机 | 默认 |
|------|--------|------|---------|------|
| P1 | `img-webp { quality: 40..95=65 }` | webpx（feature `proc-webp`） | 导出 | 开 |
| P1 | `png-quantize { colors: 256 }` | imagequant | 导出 | 开 |
| P2 | `audio-opus { bitrate_kbps: 16..64=24, mono: true }` | symphonia + libopus（feature `proc-opus`） | 导出 | 关（兼容性提示） |
| P2 | `font-subset { codepoints: "from-entries", fallback: "常用字表" }` | allsorts | 导出 | 开（检测到字体时） |
| P3 | `css-purge { scan: "all-entries", keep: [] }` | lightningcss（已有） | 导出 | 开 |
| P4 | `export-zdict { level: 22, dict: 512KB }`（新格式） | zstd | 导出 | 关（自有格式） |

管线 UI 增加"激进压缩 / 均衡 / 兼容优先"三档导出预设，一键映射到上述组合；
前端首版已内置 `img-convert jpeg q75` 的保守链。

## 4. 达标测算（10%~30%）

以典型中型学习词典（200MB MDD）估算体积构成与处理后占比：

| 构成 | 原始占比 | 处理 | 处理后占比 |
|------|---------|------|-----------|
| 插图/图标 PNG | 45% | →WebP q65 | 8%~15% |
| 发音 WAV/MP3 | 30% | →Opus 24kbps | 3%~6% |
| 内嵌字体 | 10% | 子集化 | 1%~3% |
| CSS/JS | 5% | minify+purge | 2%~3% |
| 词条 HTML | 10% | minify+zlib | 6%~8% |
| **合计** | 100% | | **20%~35%** |

图片占比更高的图解词典、音频占比更高的发音词典可达 **10%~20%**；
纯文本词典在 MDict 格式约束下约 40%~60%（除非走 §2.6 自有格式路线）。

## 5. 参考

- [webpx (libwebp bindings)](https://github.com/imazen/webpx) ·
  [RUSTSEC-2024-0443 (webp crate)](https://rustsec.org)
- [imagequant crate](https://crates.io/crates/imagequant) ·
  [pngquant](https://github.com/kornelski/pngquant)
- [Opus 16kHz 语音实测（百度云）](https://cloud.baidu.com) ·
  [Opus — Wikipedia](https://en.wikipedia.org/wiki/Opus_(audio_format))
- [allsorts（字体子集化）](https://lib.rs) ·
  [font-subset](https://crates.io) ·
  [字体子集化实测 ~87%](https://bytes.zone)
- [zstd 共享字典](https://github.com/facebook/zstd) ·
  [DebugBear 基准](https://www.debugbear.com/blog/shared-compression-dictionaries) ·
  [HTTP 字典传输](https://httptoolkit.com/blog/http-dictionary-compression/)
- [lightningcss unusedSymbols](https://lightningcss.dev) ·
  [DropCSS（purge 参考）](https://github.com/leeoniya/dropcss)
- [mdict4j 格式规范](https://mdict4j.readthedocs.io)
