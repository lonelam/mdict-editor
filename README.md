# mdict-editor


[![build](https://github.com/lonelam/mdict-editor/actions/workflows/build.yml/badge.svg)](https://github.com/lonelam/mdict-editor/actions/workflows/build.yml)
基于 Tauri 2 + SvelteKit(Svelte 5) 的 MDict 词典可视化编辑工具，通过本地 path 依赖链接
[`mdictlib`](../mdictlib)（`src-tauri/Cargo.toml` 中 `../../mdictlib`，启用 `lzo` feature）。

设计文档：[docs/DESIGN.md](docs/DESIGN.md)。当前已实现设计的全部核心功能。

## 功能

- **多源加载**：.mdx / .mdd / 外部 .js / .css 混合打开，可随时追加
- **分类管理**：资源树按「源 → 分类」组织（词条/HTML/CSS/JS/图片/音频/视频/字体/文本/其他），
  全局前缀搜索、批量多选、分页懒加载
- **类型分流编辑器**：
  - 词条/HTML/CSS/JS/文本 → CodeMirror 6，Ctrl+S 保存进改写层
  - HTML 编辑器带 **mdres:// 实时预览**（相对路径的 css/js/图片真实解析到 MDD 资源）
  - 图片 → 预览 + resize/格式转换/PNG 无损优化
  - 音频/视频/字体 → 播放与字形预览；其他 → HexViewer
- **Ctrl+Click 资源跳转**：编辑器中按住 Ctrl 点击 `src="..."`、`url(...)`、`entry://`、
  `sound://` 或像路径的字符串字面量即跳转到目标资源；Alt+←/→ 在跳转历史中前进后退。
  解析支持相对路径、根路径、百分号编码、大小写不敏感、反斜杠键、唯一后缀回退
- **处理管线**：CSS(lightningcss) / JS(swc) / HTML(minify-html 保守模式) / PNG(oxipng) /
  尺寸与格式转换(image)。作用范围可选全部/选择/源/分类；先 dry-run 看每项前后体积再应用
- **导出**：词条修订经 `EditSet + rebuild_mdx` 重建；MDD 全量重建（Overlay 替换、删除跳过、
  外部 js/css 可嵌入或另存）；每个产物自动重新打开做条目数与抽查校验。
  可选**有损压缩副本**（`.lossy.mdd`）：有损转换不可逆，因此只在导出时对副本即时套用
  （当前链：不透明图片转 JPEG q75），原始版本照常导出，Overlay 与源文件不受影响
- **撤销模型**：所有编辑（手动/管线/删除标记）只写入内存 Overlay（每资源 32 版历史），
  源文件永不被修改；支持逐版撤销与整链还原

## 下载

打 `v*` 标签或手动触发 [build workflow](https://github.com/lonelam/mdict-editor/actions/workflows/build.yml)，Actions 会构建三端安装包并自动发布到 GitHub Releases：Windows（NSIS/MSI）、macOS（Universal DMG，Apple Silicon + Intel）、Linux（AppImage/deb）。

## 运行

```bash
npm install
npm run tauri dev     # 开发
npm run tauri build   # 打包
```

要求：Node 22+，Rust 1.97+（mdictlib `rust-version = 1.97`），同级目录存在 `../mdictlib`。
裁剪 swc（JS 压缩）编译：`cargo build --no-default-features`（在 src-tauri 下）。

## 测试

```bash
cd src-tauri
cargo test                                  # 后端模块/集成测试（合成词典 fixtures）
cargo test --test m2_real_dicts -- --ignored --nocapture   # 真实词典（C:\Dictionaries，需本机存在）
cd ..
npx vitest run                              # 前端纯逻辑（引用分类等）
npm run check                               # svelte-check
```

## 结构

```
src-tauri/src/
  state.rs       SourcePool / Overlay(Revision) / AppState
  registry.rs    规范化键索引（排序+二分）、统一资源列表、分类统计
  resolver.rs    引用解析（scheme 分流 / 相对路径回退 / 唯一后缀）
  processors.rs  六个处理器（swc / lightningcss / minify-html / oxipng / image）
  pipeline.rs    Scope 选择 + dry-run / apply
  export.rs      MDX/MDD 重建 + 外部文件嵌入/另存 + 自检
  lib.rs         Tauri 命令层 + mdres:// 预览协议
  fixtures.rs    合成测试词典（gen_fixtures example 亦使用）
src/
  lib/           types / api(invoke+mdres URL) / store(runes) / refs / cm/reflinks
  components/    Explorer / TabBar / CodeEditor(含预览) / ImageEditor / Media /
                 Font / Hex / Inspector / Pipeline / Export / StatusBar / Toasts
```

## 已知限制

- 大 MDD 重建为内存全量重建（数百 MB 级有峰值，流式方案见设计文档 §13）
- 标签页切换即销毁重建编辑器（未保存内容会丢失，保存进改写层后不丢）
- 音视频/字体为只读查看器；`entry://` 在预览 iframe 内点击不跳转（编辑器内可跳）
