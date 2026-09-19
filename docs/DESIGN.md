# mdict-visualizer 设计方案 v1

> 状态：待评审 · 基于已跑通的骨架（Tauri 2 + SvelteKit/Svelte 5 + mdictlib path 链接）

## 0. 目标与非目标

**目标**

| # | 目标 | 验收形态 |
|---|------|----------|
| G1 | 分类管理资源：MDX 词条 / MDD 资源 / 外部 js、css 文件统一纳入一棵可筛选的资源树 | 左侧资源树按类型分组，支持搜索、统计、批量选择 |
| G2 | 高效编辑：按资源类型分派编辑器，统一的标签页 / 脏状态 / 撤销模型 | 文本类 CodeMirror；图片有处理操作；音视频可预览 |
| G3 | 统一压缩与脚本处理：js/css/html 压缩、图片再编码，批量 dry-run → 应用 | 管线对话框：选范围 → 预览前后体积 → 写入改写层 |
| G4 | HTML 内快捷资源跳转：编辑器中 **Ctrl+左键** 点引用即打开目标资源，像 IDE 的符号跳转 | 相对路径 / `entry://` / `sound://` 均可跳；死链有明确反馈 |
| G5 | 外部文件接入：除 .mdx/.mdd 外可直接打开磁盘上的 .js/.css 参与编辑与导出 | 文件对话框新增 js/css 过滤器；外部文件显示为独立分组 |
| G6 | 导出：把改写层 + 外部文件统一构建为新 .mdx/.mdd | 导出对话框 → 用 mdictlib 写出，校验可重新打开 |

**非目标（本期不做）**：音视频转码编辑、二进制资源的字节级编辑器、词典加密、多用户协同、MDX 词条改名（结构上预留）。

---

## 1. 总体架构

```
┌──────────────────────── 前端 (SvelteKit / Svelte 5) ────────────────────────┐
│  Shell(布局/主题/快捷键)                                                   │
│  ├─ Explorer 资源树(分类/搜索/批量)                                        │
│  ├─ EditorHost(标签页) ── EditorRegistry 按类型分派 ↓                       │
│  │    HtmlEditor · CssEditor · JsEditor · ImageEditor · MediaViewer        │
│  │    FontViewer · HexViewer                                               │
│  ├─ PreviewPane(iframe)          ├─ PipelineDialog(压缩/脚本处理)          │
│  └─ Inspector(元数据/体积)       └─ ExportDialog                            │
└──────────▲─────────────────────────────────────────────────────▲───────────┘
           │ invoke() 命令                                    │ iframe 请求
┌──────────┴───────────────────── Rust 后端 ────────────────────┴───────────┐
│  Tauri 命令层  source · registry · overlay · resolver · pipeline · export  │
│  ├─ SourcePool    MdxFile/MddFile 句柄 + 外部文件（js/css）                │
│  ├─ Registry      统一资源索引：ResourceId → 元数据/分类；键哈希索引        │
│  ├─ Overlay       改写层：{原文, 现文, 删除标记, 来源}；撤销栈              │
│  ├─ Resolver      引用解析：路径规范化 → 相对上下文 → 多级回退              │
│  ├─ Processors    lightningcss / minify-html / swc(JS) / oxipng / image    │
│  ├─ Builder       EditSet+rebuild_mdx · MddBuilder 重建 + 外部文件嵌入     │
│  └─ mdres:// 协议  预览资源服务（iframe 相对路径 → Resolver → Overlay 字节） │
│                              ▼                                              │
│                     mdictlib（本地 path 依赖, lzo）                         │
└─────────────────────────────────────────────────────────────────────────────┘
```

数据流单向：**源文件只读**。一切编辑进入 Overlay；预览/跳转读 Overlay；导出时 Overlay + 源 + 外部文件一起构建。源文件永不被原地修改——MDX/MDD 格式本身也不支持原地改写（mdictlib 的编辑模型就是读-改-重建）。

---

## 2. 核心数据模型

### 2.1 Rust 侧

```rust
/// 唯一定位一个可编辑/可查看的资源
enum ResourceId {
    MdxEntry   { source: u32, ordinal: u64 },   // 词条（HTML 文本）
    MddResource{ source: u32, ordinal: u64 },   // MDD 内资源
    External   { file: u32 },                   // 外部 js/css 等
}

/// 资源分类（树的第一级分组）
enum Category { Entry /*mdx词条*/, Html, Css, Js, Image, Audio, Video, Font, Text, Other }

/// Registry 中每条资源的元数据（驱动 Explorer 与编辑器分派）
struct ResourceMeta {
    id: ResourceId,
    key: String,               // mdd: 资源路径；mdx: 词头；external: 相对文件名
    category: Category,
    mime: String,
    size_orig: u64,            // 源内原始大小
    size_current: u64,         // Overlay 后大小
    edited: bool,              // Overlay 有修订
    deleted: bool,             // Overlay 标记删除
    source_name: String,       // 所属源显示名（溯源）
}

/// 改写层条目：一条 = 一次可撤销的修订链（保留原始字节，随时回滚）
struct Revision {
    original: Bytes,           // 首次编辑时从源读出并缓存
    current: Bytes,            // 当前内容（文本或二进制）
    deleted: bool,
    history: Vec<Bytes>,       // 简化撤销：每次 save 入栈，Ctrl+Z 逐步回退
}
```

- Overlay 持有 `Mutex<HashMap<ResourceId, Revision>>`，与骨架的 `Vec<LoadedSource>` 一起由 Tauri `manage` 托管。
- 文本编辑器保存的是 UTF-8 字节，MDX 词条文本、js/css 互不特殊化。

### 2.2 前端侧（TS 镜像）

```ts
type ResourceId = { t: "mdx"; source: number; ordinal: number }
               | { t: "mdd"; source: number; ordinal: number }
               | { t: "ext"; file: number };

interface ResourceMeta { id: ResourceId; key: string; category: Category;
  mime: string; sizeOrig: number; sizeCurrent: number;
  edited: boolean; deleted: boolean; sourceName: string; }
```

### 2.3 分类规则（G1）

由扩展名 → Category，一次构建全量缓存（`registry.build()`）：

| 分类 | 扩展名 | 默认编辑器 |
|------|--------|-----------|
| Entry | （mdx 词条） | HtmlEditor（词条模式） |
| Html | html, htm, xhtml | HtmlEditor |
| Css | css | CssEditor |
| Js | js, mjs, json（json 归 Text 可议） | JsEditor |
| Image | png jpg jpeg gif webp bmp svg ico | ImageEditor |
| Audio | mp3 wav ogg spx m4a | MediaViewer |
| Video | mp4 m4v webm | MediaViewer |
| Font | ttf otf woff woff2 eot ttc | FontViewer |
| Text | txt xml csv … | TextEditor(CodeMirror 纯文本) |
| Other | 其余 | HexViewer |

未知扩展在 Inspector 里仍显示原始字节与十六进制头部，保证"打开任何东西都不白屏"。

---

## 3. 后端模块与命令表

在骨架三命令（`open_sources/list_keys/read_entry`）之上扩展为六组命令：

| 组 | 命令 | 说明 |
|----|------|------|
| source | `open_sources(paths)` *(已有，扩展过滤器)* | mdx/mdd/js/css 混合打开、追加；js/css 落入 External |
| registry | `list_resources(category?, query?, offset, limit)` | 全局统一列表（替代仅按源列 key），返回 ResourceMeta 分页 |
|          | `resource_stats()` | 各分类 count/size 汇总，驱动树上的角标 |
| overlay  | `read_resource(id)` | 读当前内容（Overlay 优先），文本或 base64 |
|          | `write_resource(id, bytes)` | 保存编辑 → Revision 入栈 |
|          | `revert_resource(id)` | 回滚到 original |
| resolver | `resolve_reference(reference, context_id)` | G4 核心：引用 → Option<ResourceId> + 匹配方式 |
| pipeline | `pipeline_dry_run(steps, targets)` | 只算不写，返回每项前后体积 |
|          | `pipeline_apply(steps, targets)` | 写入 Overlay（可 revert） |
| export   | `export_build(config)` | 见 §10 |
| preview  | `mdres://` 自定义协议 | 见 §7，非命令 |

**MDD 前缀索引**：骨架里 MDD 前缀过滤是线性扫描，设计改为 Registry 构建时对每个 MDD 收集 `HashMap<规范化key, ordinal>`（惰性、首次访问时建，10 万键量级内存无压力），同时服务 Resolver 的精确匹配。

---

## 4. 前端模块与组件树

```
Shell（三栏 + 底部状态栏，可调分栏宽度）
├─ ExplorerPane
│  ├─ Toolbar: [打开/追加 ▾(词典|外部js/css)] [搜索框] [展开/折叠]
│  ├─ SourceTree: 按源分组的根节点（mdx/mdd/外部文件），显示条目数
│  └─ CategoryTree: 每源下按 Category 分组（数量+总体积）
│     └─ 资源项: 图标 · key · [已编辑●] [已删∅]
│        （列表虚拟滚动，键 10w+ 不卡）
├─ CenterPane: EditorHost
│  ├─ TabBar: 标签页(图标+key+脏点●, 中键关闭, 溢出滚动)
│  └─ <当前编辑器> —— 见 §5
├─ RightPane: Inspector / Preview（可切换，可折叠）
│  ├─ Inspector: 所属源 · 路径/词头 · MIME · 原始/当前体积 · 修订历史
│  └─ Preview: HtmlEditor 的实时渲染（iframe, mdres://）
└─ StatusBar: 就绪/耗时 · 跳转栈(← →) · 行:列 · 编码 · 应用内消息
```

全局状态（Svelte 5 runes class store）：

```ts
class AppStore {
  sources: SourceInfo[];            // 含外部文件
  selection: ResourceMeta[];        // 批量选择（管线作用域）
  openTabs: Tab[];                  // { id: ResourceId, editorKey }
  activeTabId; jumpStack: ResourceId[];  // Alt+←/→ 跳转历史
  overlayVersion: number;           // 写入后自增，驱动树/Inspector 刷新
}
```

---

## 5. 编辑器架构（G2）

### 5.1 分派与生命周期

```
EditorRegistry: (category, mime) → EditorComponent
openResource(id) → 已开标签则激活；否则按 meta 分派创建组件
Editor 通用契约（TS 接口，宿主通过 props/回调通信）:
  interface EditorProps { id: ResourceId }
  load() → read_resource；save() → write_resource；canSave/dirty
  onNavigate(id)      // 编辑器内部发起跳转（Ctrl+Click / 面包屑）
```

所有编辑器共用：**Ctrl+S 保存到 Overlay**（不写盘）、脏点、`revert`（回到 original）、Inspector 同步。区别只在内容呈现与工具条。

### 5.2 各编辑器：交互与样式

**HtmlEditor（含 MDX 词条）** — CodeMirror 6 + `@codemirror/lang-html`
- 布局：左右分栏 `源码 | 预览`，可拖动分隔条，可单边全屏；预览开关 `Ctrl+P`（面板级）。
- 工具条：保存 · 格式化(重新缩进) · 压缩(minify-html) · 预览刷新 · 在资源树中定位。
- Ctrl+Click 跳转：见 §6。状态栏显示悬停引用的解析目标。
- 样式：代码区 13px 等宽（`ui-monospace` 行高 1.6），标签/属性/字符串语法着色；预览区白底卡片，外框 1px 分隔。

**CssEditor** — CodeMirror + `lang-css`
- 工具条：保存 · 格式化 · 压缩(lightningcss)；底部统计"选择器数 / 规则数 / 体积"。
- `url(...)` 内资源同样支持 Ctrl+Click 跳转。

**JsEditor** — CodeMirror + `lang-javascript`
- 工具条：保存 · 压缩(swc)；统计"行数 / 体积"。
- 字符串字面量里**形如路径的值**（启发式：`/` 开头或匹配资源扩展名结尾）渲染为可跳转引用——词典 JS 里大量 `"/img/a.png"` 拼路径，这是实际刚需。

**ImageEditor**
- 内容：大图预览（适应/1:1 切换、滚轮缩放、拖拽平移）。
- 操作面板（右侧 Inspector 内）：显示 W×H、格式、原体积；操作 = `resize(宽/高/等比)`、`convert(png|jpeg|webp)`、`quality` 滑杆、`png 无损优化(oxipng)`。
- 交互：参数调整 → "应用" → 走 pipeline 单项 dry-run 显示预估体积 → 确认写 Overlay；不满意 Ctrl+Z 回退（Editor 内 undo 与 Revision 栈打通）。

**MediaViewer（音/视频）**：原生 `<audio>/<video controls>` + 元数据（时长/码率可选）。本期只读。

**FontViewer**：字形预览（"AaBb 字典 123" 渲染）+ 字重/名称元数据。本期只读。

**HexViewer（Other）**：前 64KB 十六进制 + ASCII 列，只读；提供"导出此文件到磁盘"。

### 5.3 标签页与快捷键（全局）

| 键 | 动作 |
|----|------|
| Ctrl+S | 保存当前编辑器 → Overlay |
| Ctrl+W | 关闭标签 |
| Ctrl+Shift+F | 聚焦全局搜索 |
| Alt+← / Alt+→ | 跳转栈后退/前进（配 §6） |
| Ctrl+Click | 编辑器内资源跳转 |
| Del（树中，多选） | 标记删除（Overlay，可撤销） |

---

## 6. 资源跳转（G4，Ctrl+Click）详细设计

### 6.1 引用识别（CodeMirror 扩展，前端）

语言感知而非正则全文匹配：

- HTML：语法树中属性名 ∈ {`src`, `href`, `poster`, `data-src`} 的**属性值节点**；
- CSS：`url(...)` 函数参数；
- JS：字符串字面量且内容匹配启发式（以 `/`、`./`、`../` 开头，或以已知资源扩展名结尾，或 `entry://`、`sound://`、`mdx://` 前缀）。

交互：

1. 按住 Ctrl（keydown/keyup 监听）→ 当前可视区内引用 token 加 `res-link` 装饰：虚线下划线 + `cursor: pointer`；悬停时 tooltip 显示**解析结果预览**（目标 key + 所属源）。
2. Ctrl+mousedown 命中引用 → `preventDefault` → `resolve_reference(ref, 当前资源id)` → 成功则 `openResource` 并压入跳转栈；解析失败 → token 闪红 + 状态栏消息（区分"外链 http(s)://" 与"未找到"）。
3. 多义命中（同相对名在多个 MDD 中存在）→ 弹出轻量选择列表（显示各源），Esc 取消。

### 6.2 解析算法（后端 `resolve_reference`）

```
输入: reference, context: ResourceId
1. scheme 分流:
   entry:// / mdx://X   → 在各 MDX 源 locate(X)，raw-exact 优先，规范化回退 → MdxEntry
   sound://[p] / sound://word → p 即资源路径 → 走 3 的路径匹配
   http(s):// data:     → 外链，前端直接不响应跳转（tooltip 标"外部链接"）
2. 路径规范化: '\'→'/'，去重复分隔符，PercentDecode，大小写不敏感比较键（MDict 键习惯混乱，命中层全部 lowercase 化）
3. 候选序列（依序取第一个命中）:
   a. 规范化( contextDir + ref )     —— 相对当前资源目录
   b. 规范化( ref )                   —— 根路径形式 /a/b.png
   c. 规范化( '/' + ref )
   d. 外部文件表按相对路径匹配
   e. 唯一后缀匹配: 只有一个资源的 key 以 '/'+ref 结尾 → 命中（词典里写 /b.png 实际存 /img/b.png 的常见错位）
      多个 → 返回歧义列表；0 个 → 未找到
输出: Resolved { target: ResourceId, basis: "exact-rel|exact-root|external|suffix|ambiguous" }
```

### 6.3 跳转栈

`jumpStack: ResourceId[]` + 指针，Alt+←/→ 移动；跳转高亮目标编辑器首个匹配行（文本类）。状态栏常驻 ←/→ 可用态。

---

## 7. 预览子系统：`mdres://` 自定义协议（G2/G4 的保真前提）

问题：HTML 词条里 `<img src="/b/logo.png">`、`<link href="style.css">`——直接 iframe srcdoc 时这些全是死链，预览失真。

方案：Tauri `register_uri_scheme_protocol("mdres")`：

```
iframe.src = mdres://preview/<context资源的稳定id编码>/<context的key路径>
   例: mdres://preview/mdd-0-1234/css/dict.css
协议处理器:
   取 URL path → 相对 context 用 §6.2 同一 Resolver 解析（复用！）
   → Overlay/源 读字节 → 200 + Content-Type(mime) + Cache-Control: no-store
   → 解析失败 → 404 占位图/空 css（预览不中断）
```

- 同 scheme 同 host ⇒ iframe、其内 css/js、XHR 同源，脚本可执行，样式可加载。
- **写 Overlay 后预览即时变化**（no-store），形成"编辑 → Ctrl+S → 预览刷新"闭环。
- 只注册该 scheme，iframe 加 `sandbox` 白名单 + 拦截顶层导航跳出到 http(s)（点击外链默认不响应）。

HtmlEditor 预览与 RightPane Preview 共用此通道。

---

## 8. 处理管线：压缩与脚本处理（G3）

### 8.1 处理器清单（全部 Rust 后端执行，避免 webview 卡顿与 wasm 加载）

| 处理器 | crate | 作用对象 | 关键参数 |
|--------|-------|---------|---------|
| minify-js | swc (swc_core::ecma_minifier) | Js | 压缩级别/保留版权注释 |
| minify-css | lightningcss | Css | 兼容目标/是否合并规则 |
| minify-html | minify-html | Html/Entry | 保守模式（词典 HTML 常畸形，默认不解析 JS） |
| img-optimize-png | oxipng | Image(png) | 级别 `-o2` 默认 |
| img-convert | image | Image | 目标格式 jpeg/webp + quality |
| img-resize | image | Image | 宽/高/等比 |

JS 进一步的"脚本处理"（去 console、ES 目标降级）作为 swc 参数扩展位，不在首期 UI 暴露。

### 8.2 管线模型与交互

```
PipelineDialog
 ① 作用域: [当前选择(n项) / 某分类(某源或全局) / 全部]
 ② 步骤:   按 scope 的合法处理器自动列出，可勾选 + 调参（处理器按类型自动匹配资源）
 ③ Dry-run: 后端对每项 处理→记字节→丢弃，返回表:
      资源 · 原 → 预估 · Δ% · 状态(可压/无收益/失败原因)
    （负收益即变大 → 标记"跳过建议"）
 ④ 应用:   勾选项写入 Overlay（= 一次 Revision），树与 Inspector 刷新
 ⑤ 撤销:   单项 revert 或整批 revert（Overlay 的 Revision 天然支持）
```

要点：管线**只写 Overlay 不落盘**，与手动编辑同一套撤销/导出通道，无双轨。文本处理器失败（语法错误）必须逐项报错而非中断整批。

---

## 9. 分类管理交互细节（G1）

- 树两层：**源 → 分类**，分类内资源项虚拟滚动；根节点显示源类型徽标（沿用骨架 MDX 蓝 / MDD 绿，外部文件紫 `EXT`）。
- 搜索：一次性过滤（前缀，复用后端索引），结果跨源平铺并保留分类分组；输入去抖 200ms。
- 批量：Ctrl/Shift 多选、右键菜单（打开 · 跳转引用了它的资源(反向引用，首期可只对 css/js 做) · 标记删除 · 送入管线）。
- 体积账本：每个分类节点显示 `Σ原始 → Σ当前`（Overlay 后），"共节省 x KB"常驻 Explorer 底部——压缩收益可视化。

---

## 10. 导出与构建（G6）

```
ExportDialog
  输出目录 + 命名（默认 <原名>.edited.mdx / .mdd）
  内容:
   □ MDX 重建   —— 有词条修订的源；EditSet: replace_body(改) / delete(删) / insert(增, 外部html词条可选)
                    rebuild_mdx(source, edits, WriteOptions{utf8, zlib})
   □ MDD 重建   —— 新 MddBuilder；遍历 resources() 流式搬运，Overlay 替换、deleted 跳过；
                    勾选的外部 js/css 以 add_resource 嵌入指定前缀路径(默认 /)
   □ 外部文件另存 —— 不嵌入而写到输出目录旁
  构建后自检: 重新 MdxFile::open/MddFile::open 校验条目数与抽查 lookup → 绿勾报告（字节数/条目数/耗时）
```

注意点：`MddBuilder` 逐资源全量重建在大 MDD（数百 MB）下内存峰值高，首版接受；优化路径（流式 copy_to 到临时文件再拼装）记入风险清单。

---

## 11. 交互与样式规范

**布局**：三栏可拖拽分栏（默认 280 / 360 / flex），分栏位置持久化到 localStorage；最小宽度约束；底部状态栏 24px。

**主题**：跟随系统 light/dark，设计令牌（CSS custom properties，沿用骨架的浅色基色）：

```css
--bg-app / --bg-pane / --bg-hover / --bg-active     表面层级
--border-subtle                                     #e5e5ea
--text-1 / --text-2 / --text-3                      主/次/弱文字
--accent: #2f6fed; --danger: #b3261e; --ok: #30a14e
--font-ui: system-ui; --font-code: ui-monospace 13px/1.6
--radius: 6px; --space-1..4: 4/8/12/16px
```

**状态反馈**：脏点 `●`（橙）；已删除 `∅` + 划线灰；管线已处理 `⚡`角标；跳转可点引用 = 蓝色虚线下划线，死链 = 红波浪线；所有长操作（dry-run/导出）走进度条 + 可取消（后端任务句柄）。

**空/错状态**：无资源时 Explorer 引导文案 + 打开按钮；打开失败逐文件 toast（文件名 + mdictlib 错误），不整批失败。

---

## 12. 技术选型新增依赖

| 端 | 依赖 | 用途 |
|----|------|------|
| 前端 | codemirror 6 + lang-html/css/javascript + @codemirror/search | 代码编辑 |
| 前端 | lucide-svelte（或内联 SVG） | 图标 |
| 后端 | lightningcss, minify-html, oxipng, image | 处理器 |
| 后端 | swc_core（仅 minifier feature） | JS 压缩（编译重，独立 feature gate `processors-js`，可选编译） |

swc 编译时间/体积代价大 → 放 cargo feature，默认开、可 `--no-default-features` 裁剪。前端零运行时 UI 框架加成，维持纯 CSS 令牌方案。

## 13. 性能与风险

| 风险 | 缓解 |
|------|------|
| 大 MDD 逐资源重建内存峰值 | 首版接受 + 进度反馈；后续流式临时文件方案 |
| base64 读写大资源的 IPC 膨胀 | `read_resource` 超过 256KB 的二进制改走 `mdres://` 直接取字节；编辑器保存仍走 base64（可接受，编辑资源通常不大） |
| 畸形词典 HTML 压缩失败 | minify-html 保守模式 + 单项失败不阻断 |
| MDD 键索引构建耗时（首开） | 惰性构建 + 后台线程 + 树上"索引中…"状态 |
| swc 拖慢构建 | feature gate |

## 14. 里程碑

| 阶段 | 内容 | 交付判据 |
|------|------|---------|
| M1 | Registry/Overlay/分类树/虚拟列表/EditorHost 标签页/外部 js/css 打开 | 分类浏览 + 文本可编辑保存（内存）+ 撤销 |
| M2 | CodeMirror 三编辑器 + mdres:// 预览 + Resolver + Ctrl+Click + 跳转栈 | G4 全链路：点 `/b.png` 打开图片编辑器 |
| M3 | 处理器集成 + PipelineDialog（dry-run/apply/批量） | 对一个真实词典整体压缩出 Δ% 报告 |
| M4 | 导出构建 + 自检 | 编辑→导出→重开校验通过 |
| M5 | 打磨：反向引用、进度取消、主题令牌全覆盖、错误面 | — |

## 15. 骨架 → 设计的映射（改造点）

- `open_sources`：过滤器加 js/css；登记 External；触发 registry 增量构建。
- `list_keys` → 升级为 `list_resources`（全局分类分页）；MDD 前缀线性扫描 → 哈希索引。
- `read_entry` → `read_resource`（Overlay 优先 + External 支持）。
- 前端 `+page.svelte` 单文件 → 拆 Shell/Explorer/EditorHost 组件化（保持 runes store）。
