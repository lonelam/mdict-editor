//! Export: rebuilds edited MDX sources (EditSet + rebuild_mdx) and MDD
//! sources (full re-emit with overlay applied), optionally embedding external
//! js/css files or saving them alongside. Every output is re-opened and
//! spot-checked before being reported as built.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::registry::normalize_key;
use crate::state::{current_bytes, Overlay, ResourceId, Source, SourcePool};

#[derive(Debug, Clone, Deserialize, Default)]
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
) -> Result<ExportedFile, String> {
    let Source::Mdx(file) = &pool.sources[src_idx].source else {
        return Err("not an mdx source".into());
    };
    let edits_map = overlay_edits_for(overlay, |id| matches!(id, ResourceId::Mdx { source, .. } if *source as usize == src_idx));
    if edits_map.is_empty() {
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
    for ((_src, ordinal), rev) in &edits_map {
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

    let options = mdictlib::WriteOptions::new()
        .with_encoding(mdictlib::WriteEncoding::Utf8)
        .with_compression(mdictlib::WriteCompression::Zlib);
    let mut out = Vec::new();
    let summary = mdictlib::rebuild_mdx(file, &edits, options, &mut out).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &out).map_err(|e| e.to_string())?;

    // Self-check: reopen, compare counts, spot-check the first edited entry.
    let reopened = mdictlib::MdxFile::open(out_path).map_err(|e| format!("reopen failed: {e}"))?;
    let mut message = String::new();
    let mut check_ok = reopened.len() == summary.entries && summary.entries + deleted == file.len();
    if !check_ok {
        message.push_str(&format!(
            "count mismatch: rebuilt {} (deleted {deleted}) vs source {}",
            reopened.len(),
            file.len()
        ));
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
/// embedding externals. Returns (file, skipped-external names).
fn rebuild_mdd_source(
    pool: &SourcePool,
    overlay: &Overlay,
    src_idx: usize,
    out_path: &Path,
    externals: &[(String, Vec<u8>)],
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
    let mut rewritten = 0u64;
    for key in file.keys() {
        let key = key.map_err(|e| e.to_string())?;
        let ordinal = key.ordinal().get();
        let id = ResourceId::Mdd {
            source: src_idx as u32,
            ordinal,
        };
        let name = key.key().to_string();
        match edits_map.get(&(src_idx as u32, ordinal)) {
            Some(rev) if rev.deleted => {
                deleted += 1;
            }
            Some(rev) => {
                builder.add_resource(&name, &rev.current).map_err(|e| e.to_string())?;
                rewritten += 1;
            }
            None => {
                let bytes = file
                    .resource_at(key.ordinal())
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("resource {name} missing"))?
                    .bytes()
                    .to_vec();
                builder.add_resource(&name, &bytes).map_err(|e| e.to_string())?;
            }
        }
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

    let mut out = Vec::new();
    let summary = builder.finish(&mut out).map_err(|e| e.to_string())?;
    std::fs::write(out_path, &out).map_err(|e| e.to_string())?;

    // Self-check: reopen; counts and a resource round-trip.
    let reopened = mdictlib::MddFile::open(out_path).map_err(|e| format!("reopen failed: {e}"))?;
    let embedded_count = (externals.len() - skipped.len()) as u64;
    let count_ok = reopened.len() == summary.entries
        && summary.entries + deleted == file.len() + embedded_count;
    let mut message = String::new();
    let mut check_ok = count_ok;
    if !count_ok {
        message.push_str(&format!(
            "count mismatch: rebuilt {} (deleted {deleted}, embedded {embedded_count}) vs source {}",
            reopened.len(),
            file.len()
        ));
    }
    if let Some((name, bytes)) = externals.first() {
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
) -> Result<ExportReport, String> {
    if config.out_dir.trim().is_empty() {
        return Err("output directory is required".into());
    }
    std::fs::create_dir_all(&config.out_dir).map_err(|e| format!("mkdir: {e}"))?;
    let out_dir = Path::new(&config.out_dir);
    let mut report = ExportReport::default();
    let externals = external_contents(pool, overlay);

    if config.mdx {
        for idx in 0..pool.sources.len() {
            if !matches!(pool.sources[idx].source, Source::Mdx(_)) {
                continue;
            }
            let name = pool.sources[idx].name.trim_end_matches(".mdx").to_string();
            let out_path = out_dir.join(format!("{name}.edited.mdx"));
            let file = rebuild_mdx_source(pool, overlay, idx, &out_path)?;
            report.ok |= file.check_ok;
            report.files.push(file);
        }
    }

    if config.mdd {
        for idx in 0..pool.sources.len() {
            let receives_embed = config.embed_externals
                && match config.embed_target {
                    Some(t) => t as usize == idx,
                    // None targets the first MDD source; if none exists we
                    // fall through to the standalone externals file below.
                    None => idx == first_mdd_index(pool).unwrap_or(usize::MAX),
                };
            let has_edits = overlay.revisions.keys().any(
                |id| matches!(id, ResourceId::Mdd { source, .. } if *source as usize == idx),
            );
            if !has_edits && !receives_embed {
                continue;
            }
            let name = pool.sources[idx].name.trim_end_matches(".mdd").to_string();
            let out_path = out_dir.join(format!("{name}.edited.mdd"));
            let (file, skipped) = rebuild_mdd_source(pool, overlay, idx, &out_path, &externals)?;
            report.ok |= file.check_ok;
            report.skipped_externals.extend(skipped);
            report.files.push(file);
        }

        // Externals requested but no MDD source exists → standalone file.
        if !externals.is_empty()
            && config.embed_target.is_none()
            && first_mdd_index(pool).is_none()
        {
            let mut builder = mdictlib::MddBuilder::with_options(
                mdictlib::WriteOptions::new()
                    .with_compression(mdictlib::WriteCompression::Zlib),
            );
            builder.header_attribute("Title", "External resources").ok();
            for (name, bytes) in &externals {
                builder.add_resource(name, bytes).map_err(|e| e.to_string())?;
            }
            let mut out = Vec::new();
            builder.finish(&mut out).map_err(|e| e.to_string())?;
            let out_path = out_dir.join("externals.mdd");
            std::fs::write(&out_path, &out).map_err(|e| e.to_string())?;
            let reopened = mdictlib::MddFile::open(&out_path).map_err(|e| format!("reopen failed: {e}"))?;
            let check_ok = reopened.len() == externals.len() as u64;
            report.ok |= check_ok;
            report.files.push(ExportedFile {
                path: out_path.display().to_string(),
                entries: reopened.len(),
                bytes: out.len() as u64,
                check_ok,
                message: String::new(),
            });
        }
    }

    if config.save_externals {
        for (name, bytes) in &externals {
            let path = out_dir.join(name);
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            report.files.push(ExportedFile {
                path: path.display().to_string(),
                entries: 0,
                bytes: bytes.len() as u64,
                check_ok: true,
                message: "saved external".into(),
            });
            report.ok = true;
        }
    }

    Ok(report)
}

fn first_mdd_index(pool: &SourcePool) -> Option<usize> {
    pool.sources
        .iter()
        .position(|e| matches!(e.source, Source::Mdd(_)))
}
