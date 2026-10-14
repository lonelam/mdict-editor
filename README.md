# mdict-editor


[![build](https://github.com/lonelam/mdict-editor/actions/workflows/build.yml/badge.svg)](https://github.com/lonelam/mdict-editor/actions/workflows/build.yml)
基于 Tauri 2 + SvelteKit(Svelte 5) 的 MDict 词典可视化编辑工具，使用
[`mdictlib`](https://crates.io/crates/mdictlib) 解析与重建词典（启用 `lzo` feature）。

设计文档：[docs/DESIGN.md](docs/DESIGN.md)。当前已实现设计的全部核心功能。

## 功能

- **多源加载**：.mdx / .mdd / 外部 .js / .css 混合打开，可随时追加
- **分类管理**：资源树按「源 → 分类」组织（词条/HTML/CSS/JS/图片/音频/视频/字体/文本/其他），
  全局前缀搜索、批量多选、分页懒加载
- **类型分流编辑器**：
  - 词条/HTML/CSS/JS/文本 → CodeMirror 6，Ctrl+S 一次保存所有词条与资源草稿
  - HTML 编辑器带 **mdres:// 实时预览**（相对路径的 css/js/图片真实解析到 MDD 资源）
  - **预览反向定位**：在预览中选中文字，自动选中对应的 HTML 代码并滚动到位置；
    支持重复文字、嵌套标签和实体编码，重定向词条会打开实际目标词条
  - **实体文本工具**：选中文字或属性值，解码为可读文字后修改，可写回 Unicode、
    十六进制/十进制实体或命名优先的 HTML 实体；操作可撤销
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
- **跨天校对项目**：保存为 `.mdict-project`，保留源文件列表、修改、删除、词头重命名、
  新增词条/资源和撤销历史；重新打开项目即可继续编辑，无须每天重建整个 MDX/MDD。
  切换或关闭标签后保留文本草稿，保存项目时一起写入；未保存项目退出时会提示

## 保存并继续校对

简单改错的四步：**打开文件 → 修改词条 → Ctrl+S → 导出**。
同一词条可连续修改多处；多个词条也可来回切换、修改后最后按一次 Ctrl+S，
所有标签（包括已关闭标签）的草稿会一起保存。导出开始前会再次补存草稿。

工具栏默认开启 **自动保存**：输入停顿约 800 ms 后保存到改写层，预览在输入后自动更新。
可取消勾选改为手动 Ctrl+S。**尚未选择项目文件时，这些保存仅保留在本次会话中**；
需要跨天继续编辑时，点击 **保存项目**（或 Ctrl+Shift+S）选择项目文件。
之后自动保存和 Ctrl+S 都会同步保存当前项目；Ctrl+Shift+S 可另存。
下次启动点击 **打开项目** 恢复后继续校对，再通过 **导出** 生成阅读器可用的词典。

实体编码改错：先在右侧预览选中文字（或直接在左侧选中文字/属性值），点击
**实体文本工具**，修改解码后的文字，选择写回格式，再点 **写回代码**。
无需复制到外部工具，也不要选择 HTML 标签。命名、十进制、十六进制实体均可解码。
预览中的文字若由脚本动态生成或节点被脚本改变，无法保证源码位置，会提示改用代码搜索。

项目文件引用原始词典的绝对路径，只包含编辑内容与历史，**不包含完整原词典**。
请保留原文件与路径；源文件丢失、内容变化或项目损坏时，打开会报错并保留当前会话。
项目采用临时文件写入后替换的方式保存；保存失败不会覆盖已有项目，也不能覆盖源文件。

## 下载

打 `v*` 标签触发 [build workflow](https://github.com/lonelam/mdict-editor/actions/workflows/build.yml)，Actions 会构建并发布到 GitHub Releases：Windows（NSIS/MSI 安装包、portable ZIP）、macOS（Apple Silicon DMG）、Linux（AppImage/deb）。手动触发时可从该次运行的 Artifacts 下载产物，其中 Windows 便携版为 `mdict-editor-windows-portable`。

Windows 便携包命名为 `mdict-editor-<版本>-windows-<架构>-portable.zip`，并提供 `.zip.sha256` 校验文件；当前 CI 构建 x64。
解压后双击 `mdict-editor.exe` 即可使用，无须安装本应用。运行需要系统中的
[Microsoft Edge WebView2 Runtime](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options)，
压缩包内含使用说明与官方下载链接。浏览器缓存使用当前用户的应用数据目录；词典、项目与导出位置自行选择。

在 Windows 本地只构建便携版：

```bash
npm run tauri build -- --no-bundle
npm run package:portable
```

产物位于 `src-tauri/target/portable/`。完成正常的 `npm run tauri build` 后也可以直接执行第二条命令。

## 与 AALookup 的关系（同构）

本编辑器与 [AALookup](https://aalookup.com) 词典阅读器**同源同构**：

- **同一解析核心**：两者都构建在开源 Rust 庐 [mdictlib](https://crates.io/crates/mdictlib) 之上——同一套 MDX/MDD 解析、定位与块解码逻辑；
- **同一资源解析顺序**：词条内资源引用按与 AALookup 阅读器一致的顺序解析（词典目录旁的松散文件优先，其次 MDD 内资源，键大小写与分隔符归一化，唯一后缀回退），编辑预览即阅读效果；
- **一键衔接**：导出对话框提供 **"一键导入 AALookup"** 按钮——导出的 .mdx 直接交给 AALookup 打开并启用（AALookup 正在运行时即时出现在词库中），用阅读视角检验编辑成果。

编辑用 mdict-editor，阅读用 AALookup，一对同构工具覆盖词典工作流两端。

## 运行

```bash
npm install
npm run tauri dev     # 开发
npm run tauri build   # 打包
```

要求：Node 22+，Rust 1.97+（mdictlib `rust-version = 1.97`）。如需本地开发 mdictlib，
可在 `src-tauri/Cargo.toml` 中启用注释里的 path 依赖，并准备同级 `../mdictlib`。
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
  project.rs     项目快照保存/恢复 + 源文件内容校验
  lib.rs         Tauri 命令层 + mdres:// 预览协议
  fixtures.rs    合成测试词典（gen_fixtures example 亦使用）
src/
  lib/           types / api(invoke+mdres URL) / store(runes) / refs / cm/reflinks
  components/    Explorer / TabBar / CodeEditor(含预览) / ImageEditor / Media /
                 Font / Hex / Inspector / Pipeline / Export / StatusBar / Toasts
```

## 已知限制

- 大 MDD 重建为内存全量重建（数百 MB 级有峰值，流式方案见设计文档 §13）
- 尚未选择项目文件时，自动保存与 Ctrl+S 只保留本次会话，修改不能跨应用重启恢复
- 音视频/字体为只读查看器；`entry://` 在预览 iframe 内点击不跳转（编辑器内可跳）
