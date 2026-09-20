//! Export: rebuilds edited MDX sources (EditSet + rebuild_mdx) and MDD
//! sources (full re-emit with overlay applied), optionally embedding external
//! js/css files or saving them alongside. Every output is re-opened and
//! spot-checked before being reported as built.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::category::Category;
use crate::pipeline::JobCtl;
use crate::processors;
use crate::processors::Processor;
use crate::registry::normalize_key;
use crate::state::{current_bytes, InsertKind, Overlay, ResourceId, Source, SourcePool};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportConfig {
    pub out_dir: String,
    /// Emit the `edited/` folder: every loaded source (mdx/mdd under their
    /// original names, external js/css copied verbatim), with overlay edits
    /// applied. Nothing irreversible happens here.
    pub edited: bool,
    /// Emit the `lossy/` folder: the same complete file set, with this lossy
    /// chain applied to MDD resources (images/audio only — js/css are never
    /// touched: minifiers have broken real-world dictionary scripts).
    /// None = do not generate the folder.
    /// Embed external js/css files into the edited MDD output as well.
    pub embed_externals: bool,
    /// MDD source index to embed into; None → the first MDD source.
    pub embed_target: Option<u32>,
    /// Skip mdx/mdd rebuilds without edits/insertions in the edited folder
    /// (external files still copy). Default false: complete output set.
    pub only_edited: bool,
    /// Lossy transform chain for the lossy folder.
    pub lossy: Option<Vec<Processor>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFile {
    pub path: String,
    pub entries: u64,
    pub bytes: u64,
    pub check_ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub files: Vec<ExportedFile>,
    /// Externals that collided with existing keys and were skipped.
    pub skipped_externals: Vec<String>,
    pub ok: bool,
}

fn overlay_edits_for<'a>(
    overlay: &'a Overlay,
    f: impl Fn(&ResourceId) -> bool,
) -> BTreeMap<(u32, u64), &'a crate::state::Revision> {
    overlay
        .revisions
        .iter()
        .filter(|(id, _)| f(id))
        .map(|(id, rev)| {
            let (source, ordinal) = match id {
                ResourceId::Mdx { source, ordinal } | ResourceId::Mdd { source, ordinal } => {
                    (*source, *ordinal)
                }
                ResourceId::Ext { .. } => return None,
            };
            Some(((source, ordinal), rev))
        })
        .collect::<Option<BTreeMap<(u32, u64), &'a crate::state::Revision>>>()
        .unwrap_or_default()
}

/// Rebuilds one edited MDX source into `out_path` and self-checks it.
fn rebuild_mdx_source(
    pool: &SourcePool,
    overlay: &Overlay,
    src_idx: usize,
    out_path: &Path,
    only_edited: bool,
    ctl: &JobCtl,
) -> Result<ExportedFile, String> {
    let Source::Mdx(file) = &pool.sources[src_idx].source else {
        return Err("not an mdx source".into());
    };
    let edits_map = overlay_edits_for(overlay, |id| matches!(id, ResourceId::Mdx { source, .. } if *source as usize == src_idx));
    let insertions: Vec<&crate::state::Insertion> = overlay
        .insertions
        .iter()
        .filter(|i| i.target as usize == src_idx && i.kind == InsertKind::Entry)
        .collect();
    if only_edited && edits_map.is_empty() && insertions.is_empty() {
        return Ok(ExportedFile {
            path: out_path.display().to_string(),
            entries: 0,
            bytes: 0,
            check_ok: false,
            message: "no edits; skipped".into(),
        });
    }

    let mut edits = mdictlib::EditSet::new();
    let mut deleted = 0u64;
    let mut first_edited: Option<(String, String)> = None;
    let total = edits_map.len() as u64;
    for (i, ((_src, ordinal), rev)) in edits_map.iter().enumerate() {
        if (ctl.cancelled)() {
            return Err("已取消".into());
        }
        let key_name = file
            .key_at(mdictlib::KeyOrdinal::new(*ordinal))
            .map_err(|e| e.to_string())?
            .map(|k| k.key().to_string())
            .unwrap_or_default();
        (ctl.progress)(i as u64, total, &key_name);
        let ordinal = mdictlib::KeyOrdinal::new(*ordinal);
        let key = file
            .key_at(ordinal)
            .map_err(|e| e.to_string())?
            .ok_or("missing key")?
            .key()
            .to_string();
        if rev.deleted {
            edits.delete(ordinal);
            deleted += 1;
        } else {
            let body = String::from_utf8_lossy(&rev.current).into_owned();
            if first_edited.is_none() {
                first_edited = Some((key.clone(), body.clone()));
            }
            edits.replace_body(ordinal, body);
        }
    }

    let mut inserted = 0u64;
    for ins in &insertions {
        let body = String::from_utf8_lossy(&ins.bytes).into_owned();
        edits.insert(ins.name.clone(), body);
        inserted += 1;
    }

    let options = mdictlib::WriteOptions::new()
        .with_encoding(mdictlib::WriteEncoding::Utf8)
        .with_compression(mdictlib::WriteCompression::Zlib);
    let mut out = Vec::new();
    let summary = mdictlib::rebuild_mdx(file, &edits, options, &mut out).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &out).map_err(|e| e.to_string())?;

    // Self-check: reopen, compare counts, spot-check the first edited entry.
    let reopened = mdictlib::MdxFile::open(out_path).map_err(|e| format!("reopen failed: {e}"))?;
    let mut message = String::new();
    let mut check_ok =
        reopened.len() == summary.entries && summary.entries + deleted == file.len() + inserted;
    if !check_ok {
        message.push_str(&format!(
            "count mismatch: rebuilt {} (deleted {deleted}) vs source {}",
            reopened.len(),
            file.len()
        ));
    }
    if let Some(ins) = insertions.first() {
        match reopened.lookup(&ins.name) {
            Ok(Some(entry)) if entry.text().as_bytes() == ins.bytes.as_slice() => {}
            Ok(_) => {
                check_ok = false;
                message.push_str(&format!("insert spot-check failed for {:?}", ins.name));
            }
            Err(e) => {
                check_ok = false;
                message.push_str(&format!("insert lookup failed: {e}"));
            }
        }
    }
    if let Some((key, body)) = &first_edited {
        match reopened.lookup(key) {
            Ok(Some(entry)) if entry.text() == body => {}
            Ok(_) => {
                check_ok = false;
                message.push_str(&format!("spot-check failed for {key:?}"));
            }
            Err(e) => {
                check_ok = false;
                message.push_str(&format!("lookup failed: {e}"));
            }
        }
    }
    Ok(ExportedFile {
        path: out_path.display().to_string(),
        entries: reopened.len(),
        bytes: out.len() as u64,
        check_ok,
        message,
    })
}

/// Rebuilds one MDD source (overlay applied) into `out_path`, optionally
/// embedding externals. When `lossy` steps are given, every resource is
/// transformed on the fly while copying — the overlay itself is never fed
/// lossy bytes. Returns (file, skipped-external names).
fn rebuild_mdd_source(
    pool: &SourcePool,
    overlay: &Overlay,
    src_idx: usize,
    out_path: &Path,
    externals: &[(String, Vec<u8>)],
    lossy: Option<&[Processor]>,
    ctl: &JobCtl,
) -> Result<(ExportedFile, Vec<String>), String> {
    let Source::Mdd(file) = &pool.sources[src_idx].source else {
        return Err("not an mdd source".into());
    };
    let edits_map = overlay_edits_for(overlay, |id| matches!(id, ResourceId::Mdd { source, .. } if *source as usize == src_idx));

    let mut builder = mdictlib::MddBuilder::with_options(
        mdictlib::WriteOptions::new().with_compression(mdictlib::WriteCompression::Zlib),
    );
    let existing: std::collections::HashSet<String> = {
        let mut set = std::collections::HashSet::new();
        for key in file.keys() {
            let key = key.map_err(|e| e.to_string())?;
            set.insert(normalize_key(key.key()));
        }
        set
    };

    let mut deleted = 0u64;
    let mut _rewritten = 0u64;
    let mut lossy_changed = 0u64;
    let mut lossy_total = 0u64;
    let total = file.len();
    let mut seen = 0u64;
    // Phase A — serial read (IO + block decode): collect (name, bytes).
    let mut items: Vec<(String, Vec<u8>)> = Vec::new();
    for key in file.keys() {
        if (ctl.cancelled)() {
            return Err("已取消".into());
        }
        seen += 1;
        let key = key.map_err(|e| e.to_string())?;
        (ctl.progress)(seen, total, key.key());
        let ordinal = key.ordinal().get();
        let name = key.key().to_string();
        let raw: Vec<u8> = match edits_map.get(&(src_idx as u32, ordinal)) {
            Some(rev) if rev.deleted => {
                deleted += 1;
                continue;
            }
            Some(rev) => {
                _rewritten += 1;
                rev.current.clone()
            }
            None => file
                .resource_at(key.ordinal())
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("resource {name} missing"))?
                .bytes()
                .to_vec(),
        };
        items.push((name, raw));
    }

    // Phase B — parallel lossy transform (CPU-bound; rayon keeps item order),
    // then serial builder writes. One pass-through: image re-encodes never
    // stack (see processors::apply_chain).
    let transformed: Vec<(String, Vec<u8>)> = match lossy {
        Some(steps) => {
            lossy_total = items.len() as u64;
            use rayon::prelude::*;
            let changed_count = std::sync::atomic::AtomicU64::new(0);
            let out: Vec<(String, Vec<u8>)> = items
                .into_par_iter()
                .map(|(name, raw)| {
                    let (out, changed) = apply_lossy(steps, &name, &raw, ctl);
                    if changed {
                        changed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    (name, out)
                })
                .collect();
            lossy_changed = changed_count.into_inner();
            out
        }
        None => items,
    };
    for (name, bytes) in transformed {
        builder.add_resource(&name, &bytes).map_err(|e| e.to_string())?;
    }

    // Embed externals, skipping keys that collide with existing resources.
    let mut skipped = Vec::new();
    for (name, bytes) in externals {
        if existing.contains(&normalize_key(name)) {
            skipped.push(name.clone());
            continue;
        }
        builder.add_resource(name, bytes).map_err(|e| e.to_string())?;
    }

    // Pending resource insertions (validated against `existing` at insert
    // time); lossy chain applies on the fly like every other resource.
    let mut inserted_res = 0u64;
    for ins in overlay
        .insertions
        .iter()
        .filter(|i| i.target as usize == src_idx && i.kind == InsertKind::Resource)
    {
        let bytes = match lossy {
            Some(steps) => {
                lossy_total += 1;
                let (out, changed) = apply_lossy(steps, &ins.name, &ins.bytes, ctl);
                if changed {
                    lossy_changed += 1;
                }
                out
            }
            None => ins.bytes.clone(),
        };
        builder.add_resource(&ins.name, &bytes).map_err(|e| e.to_string())?;
        inserted_res += 1;
    }

    let mut out = Vec::new();
    let summary = builder.finish(&mut out).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &out).map_err(|e| e.to_string())?;

    // Self-check: reopen; counts and a resource round-trip.
    let reopened = mdictlib::MddFile::open(out_path).map_err(|e| format!("reopen failed: {e}"))?;
    let embedded_count = (externals.len() - skipped.len()) as u64;
    let count_ok = reopened.len() == summary.entries
        && summary.entries + deleted == file.len() + embedded_count + inserted_res;
    let mut message = String::new();
    let mut check_ok = count_ok;
    if !count_ok {
        message.push_str(&format!(
            "count mismatch: rebuilt {} (deleted {deleted}, embedded {embedded_count}) vs source {}",
            reopened.len(),
            file.len()
        ));
    }
    // Spot-check only externals that were actually embedded; skipped ones
    // (key collision with an existing MDD resource) are absent by design.
    let embedded_externals: Vec<&(String, Vec<u8>)> = externals
        .iter()
        .filter(|(name, _)| !skipped.iter().any(|s| s == name))
        .collect();
    if let Some((name, bytes)) = embedded_externals.first() {
        match reopened.lookup(name) {
            Ok(Some(res)) if res.bytes() == bytes.as_slice() => {}
            Ok(_) => {
                check_ok = false;
                message.push_str(&format!("embed spot-check failed for {name:?}"));
            }
            Err(e) => {
                check_ok = false;
                message.push_str(&format!("lookup failed: {e}"));
            }
        }
    }
    if lossy.is_some() {
        if !message.is_empty() {
            message.push_str("; ");
        }
        message.push_str(&format!(
            "lossy: {lossy_changed}/{lossy_total} resources transformed"
        ));
    }
    Ok((
        ExportedFile {
            path: out_path.display().to_string(),
            entries: reopened.len(),
            bytes: out.len() as u64,
            check_ok,
            message,
        },
        skipped,
    ))
}

/// Applies the lossy chain to one resource on the fly (shared semantics
/// with the pipeline — see `processors::apply_chain`).
fn apply_lossy(steps: &[Processor], key: &str, bytes: &[u8], ctl: &JobCtl) -> (Vec<u8>, bool) {
    processors::apply_chain(steps, key, bytes, ctl.used_selectors)
}

/// Current content of all external files (overlay-aware), sorted by name.
fn external_contents(pool: &SourcePool, overlay: &Overlay) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for (idx, entry) in pool.sources.iter().enumerate() {
        if !matches!(entry.source, Source::External { .. }) {
            continue;
        }
        let id = ResourceId::Ext { file: idx as u32 };
        let name = pool.key_of(&id).unwrap_or_else(|_| entry.name.clone());
        let bytes = current_bytes(pool, overlay, &id).unwrap_or_default();
        out.push((name, bytes));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn export_build(
    pool: &SourcePool,
    overlay: &Overlay,
    config: &ExportConfig,
    ctl: &JobCtl,
) -> Result<ExportReport, String> {
    if config.out_dir.trim().is_empty() {
        return Err("output directory is required".into());
    }
    std::fs::create_dir_all(&config.out_dir).map_err(|e| format!("mkdir: {e}"))?;
    let out_dir = Path::new(&config.out_dir);
    let externals = external_contents(pool, overlay);

    // A source receives the embedded externals when explicitly targeted, or
    // implicitly as "the first MDD source" when no target was chosen.
    let receives_embed = |idx: usize| -> bool {
        config.embed_externals
            && match config.embed_target {
                Some(t) => t as usize == idx,
                None => first_mdd_index(pool) == Some(idx),
            }
    };

    let mut report = ExportReport::default();
    let mut name_seen: std::collections::HashSet<(bool, String)> =
        std::collections::HashSet::new();

    // ---- edited/: every loaded source under its original name ----
    if config.edited {
        let edited_dir = out_dir.join("edited");
        std::fs::create_dir_all(&edited_dir).map_err(|e| format!("mkdir edited: {e}"))?;
        for idx in 0..pool.sources.len() {
            let entry = &pool.sources[idx];
            if entry.tombstone {
                continue;
            }
            let has_work = || {
                overlay.revisions.keys().any(|id| id.source_index() as usize == idx)
                    || overlay.insertions.iter().any(|i| i.target as usize == idx)
            };
            match &entry.source {
                Source::Mdx(_) => {
                    if config.only_edited && !has_work() {
                        continue;
                    }
                    if !name_seen.insert((false, entry.name.clone())) {
                        return Err(format!("edited/ 文件名冲突: {}", entry.name));
                    }
                    let out_path = edited_dir.join(&entry.name);
                    let file = rebuild_mdx_source(pool, overlay, idx, &out_path, config.only_edited, ctl)?;
                    report.ok |= file.check_ok;
                    report.files.push(file);
                }
                Source::Mdd(_) => {
                    if config.only_edited && !has_work() && !receives_embed(idx) {
                        continue;
                    }
                    if !name_seen.insert((false, entry.name.clone())) {
                        return Err(format!("edited/ 文件名冲突: {}", entry.name));
                    }
                    let out_path = edited_dir.join(&entry.name);
                    let exts: Vec<(String, Vec<u8>)> = if receives_embed(idx) {
                        externals.clone()
                    } else {
                        Vec::new()
                    };
                    let (file, skipped) =
                        rebuild_mdd_source(pool, overlay, idx, &out_path, &exts, None, ctl)?;
                    report.ok |= file.check_ok;
                    report.skipped_externals.extend(skipped);
                    report.files.push(file);
                }
                Source::External { .. } => {
                    let id = ResourceId::Ext { file: idx as u32 };
                    let bytes = crate::state::current_bytes(pool, overlay, &id)?;
                    let out_path = edited_dir.join(&entry.name);
                    std::fs::write(&out_path, &bytes).map_err(|e| e.to_string())?;
                    report.files.push(ExportedFile {
                        path: out_path.display().to_string(),
                        entries: 0,
                        bytes: bytes.len() as u64,
                        check_ok: true,
                        message: "copied external".into(),
                    });
                    report.ok = true;
                }
            }
        }
    }

    // ---- lossy/: the same complete set; MDD resources run the lossy chain
    // (images/audio only), mdx and externals are copied verbatim. ----
    if let Some(lossy_steps) = config.lossy.clone().filter(|s| !s.is_empty()) {
        let lossy_dir = out_dir.join("lossy");
        std::fs::create_dir_all(&lossy_dir).map_err(|e| format!("mkdir lossy: {e}"))?;
        for idx in 0..pool.sources.len() {
            let entry = &pool.sources[idx];
            if entry.tombstone {
                continue;
            }
            match &entry.source {
                Source::Mdd(_) => {
                    if !name_seen.insert((true, entry.name.clone())) {
                        return Err(format!("lossy/ 文件名冲突: {}", entry.name));
                    }
                    let out_path = lossy_dir.join(&entry.name);
                    let (file, skipped) = rebuild_mdd_source(
                        pool,
                        overlay,
                        idx,
                        &out_path,
                        &[],
                        Some(&lossy_steps),
                        ctl,
                    )?;
                    report.ok |= file.check_ok;
                    report.skipped_externals.extend(skipped);
                    report.files.push(file);
                }
                Source::Mdx(_) => {
                    // Entries are not lossy-processed; reuse the edited build
                    // when present, otherwise a plain rebuild.
                    if !name_seen.insert((true, entry.name.clone())) {
                        return Err(format!("lossy/ 文件名冲突: {}", entry.name));
                    }
                    let out_path = lossy_dir.join(&entry.name);
                    let edited_path = out_dir.join("edited").join(&entry.name);
                    if config.edited && edited_path.exists() {
                        std::fs::copy(&edited_path, &out_path).map_err(|e| e.to_string())?;
                        let bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
                        report.files.push(ExportedFile {
                            path: out_path.display().to_string(),
                            entries: 0,
                            bytes,
                            check_ok: true,
                            message: "same as edited".into(),
                        });
                    } else {
                        let file =
                            rebuild_mdx_source(pool, overlay, idx, &out_path, false, ctl)?;
                        report.ok |= file.check_ok;
                        report.files.push(file);
                    }
                }
                Source::External { .. } => {
                    let id = ResourceId::Ext { file: idx as u32 };
                    let bytes = crate::state::current_bytes(pool, overlay, &id)?;
                    let out_path = lossy_dir.join(&entry.name);
                    std::fs::write(&out_path, &bytes).map_err(|e| e.to_string())?;
                    report.files.push(ExportedFile {
                        path: out_path.display().to_string(),
                        entries: 0,
                        bytes: bytes.len() as u64,
                        check_ok: true,
                        message: "copied external".into(),
                    });
                }
            }
        }
    }

    Ok(report)
}

fn first_mdd_index(pool: &SourcePool) -> Option<usize> {
    pool.sources
        .iter()
        .position(|e| !e.tombstone && matches!(e.source, Source::Mdd(_)))
}
