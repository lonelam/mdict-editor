s = open('src/export.rs', encoding='utf-8').read()

# ---- 1) ExportConfig: edited 目录开关；去掉 saveExternals（外部文件总是随目录落盘）----
old = """#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportConfig {
    pub out_dir: String,
    /// Rebuild MDX sources that have entry edits.
    pub mdx: bool,
    /// Rebuild MDD sources that have resource edits (or receive embeds).
    pub mdd: bool,
    /// Embed external js/css files into the MDD output.
    pub embed_externals: bool,
    /// MDD source index to embed into; None → a new `<externals>.mdd`.
    pub embed_target: Option<u32>,
    /// Copy external files' current content next to the outputs.
    pub save_externals: bool,
    /// Skip sources without edits/insertions. Default false: every active
    /// mdx/mdd is rebuilt so the output set is complete.
    pub only_edited: bool,
    /// Lossy transform chain applied **only** to a compressed copy
    /// (`<name>.lossy.mdd`) emitted alongside the original rebuild. The
    /// overlay and every other output never see lossy bytes — lossy
    /// compression is irreversible, so it happens exclusively at export.
    pub lossy: Option<Vec<Processor>>,
}"""
new = """#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportConfig {
    pub out_dir: String,
    /// Emit the `edited/` folder: every loaded source (mdx/mdd under their
    /// original names, external js/css copied verbatim), with overlay edits
    /// applied. Nothing irreversible happens here.
    pub edited: bool,
    /// Emit the `lossy/` folder: the same complete file set, with the lossy
    /// chain applied to MDD resources (images/audio only — js/css are never
    /// touched: minifiers have broken real-world dictionary scripts).
    pub lossy: bool,
    /// Embed external js/css files into the edited MDD output as well.
    pub embed_externals: bool,
    /// MDD source index to embed into; None → the first MDD source.
    pub embed_target: Option<u32>,
    /// Skip mdx/mdd rebuilds without edits/insertions in the edited folder
    /// (external files still copy). Default false: complete output set.
    pub only_edited: bool,
    /// Lossy transform chain for the lossy folder.
    pub lossy: Option<Vec<Processor>>,
}"""
assert old in s
s = s.replace(old, new)
open('src/export.rs', 'w', encoding='utf-8', newline='\n').write(s)
print('config ok')
