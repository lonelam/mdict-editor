s = open('src/lib/types.ts', encoding='utf-8').read()
old = """export interface ExportConfig {
  outDir: string;
  mdx: boolean;
  mdd: boolean;
  embedExternals: boolean;
  embedTarget: number | null;
  saveExternals: boolean;
  /** Skip sources without edits/insertions (default: rebuild everything). */
  onlyEdited: boolean;
  /** Lossy chain applied only to the `.lossy.mdd` copy; originals unaffected. */
  lossy: Processor[] | null;
}"""
new = """export interface ExportConfig {
  outDir: string;
  /** Emit the `edited/` folder: all loaded sources, overlay edits applied. */
  edited: boolean;
  /** Emit the `lossy/` folder (chain below); images/audio only, js/css untouched. */
  lossy: Processor[] | null;
  embedExternals: boolean;
  embedTarget: number | null;
  /** Skip mdx/mdd rebuilds without edits in edited/ (externals still copy). */
  onlyEdited: boolean;
}"""
assert old in s
s = s.replace(old, new)
open('src/lib/types.ts', 'w', encoding='utf-8', newline='\n').write(s)
print('types ok')

s = open('src/components/ExportDialog.svelte', encoding='utf-8').read()

s = s.replace("""  let outDir = $state("");
  let rebuildMdx = $state(true);
  let rebuildMdd = $state(true);
  let embed = $state(false);""",
"""  let outDir = $state("");
  let genEdited = $state(true);
  let genLossy = $state(false);
  let embed = $state(false);""")

old = """        outDir,
        mdx: rebuildMdx,
        mdd: rebuildMdd,
        embedExternals: embed,
        embedTarget,
        onlyEdited,
        lossy: lossy
          ? [
              { kind: "img-webp", quality: 75 },
              { kind: "png-quantize", colors: 256 },
              { kind: "css-purge" },
              { kind: "minify-css" },
              { kind: "minify-js" },
              { kind: "audio-opus", bitrateKbps: 24 },
            ]
          : null,"""
new = """        outDir,
        edited: genEdited,
        embedExternals: embed,
        embedTarget,
        onlyEdited,
        // Images/audio only: minifying dictionary js/css broke real-world
        // scripts at import (oaldpex) — they are copied verbatim instead.
        lossy: genLossy
          ? [
              { kind: "img-webp", quality: 75 },
              { kind: "png-quantize", colors: 256 },
              { kind: "audio-opus", bitrateKbps: 24 },
            ]
          : null,"""
assert old in s
s = s.replace(old, new)

old = """      <label>
        <input type="checkbox" bind:checked={rebuildMdx} /> 重建 MDX</label>
      <label><input type="checkbox" bind:checked={rebuildMdd} /> 重建 MDD</label>
      <label>
        <input type="checkbox" bind:checked={onlyEdited} />
        仅导出有修改/待插入的源（默认导出全部）
      </label>"""
new = """      <label>
        <input type="checkbox" bind:checked={genEdited} />
        生成 <b>edited/</b>（保真版：全部已加载文件，原名导出，含编辑）
      </label>
      <label>
        <input type="checkbox" bind:checked={genLossy} />
        生成 <b>lossy/</b>（压缩分发版：同一套文件，图片 WebP + PNG 量化 + 音频 Opus；JS/CSS 不动）
      </label>
      <label>
        <input type="checkbox" bind:checked={onlyEdited} />
        edited/ 跳过无修改的词典（外部文件总是复制）
      </label>"""
assert old in s
s = s.replace(old, new)

old = "当前链：图片转有损 WebP（保透明）+ PNG 调色板量化 + 发音音频转 Opus 24kbps +\n        CSS 死规则清除 + CSS/JS 压缩；字体子集化将随后续版本接入。"
new = "词典 JS/CSS 原样复制（压缩会破坏部分词典脚本）；字体子集化将随后续版本接入。"
assert old in s
s = s.replace(old, new)

s = s.replace("let lossy = $state(false);", "let lossyUnused = $state(false); // superseded by genLossy")
open('src/components/ExportDialog.svelte', 'w', encoding='utf-8', newline='\n').write(s)
print('dialog ok')
