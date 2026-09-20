s = open('src/export.rs', encoding='utf-8').read()

old = """    /// Emit the `lossy/` folder: the same complete file set, with the lossy
    /// chain applied to MDD resources (images/audio only — js/css are never
    /// touched: minifiers have broken real-world dictionary scripts).
    pub lossy: bool,
    /// Embed external js/css files into the edited MDD output as well."""
new = """    /// Embed external js/css files into the edited MDD output as well."""
assert old in s
s = s.replace(old, new)
open('src/export.rs', 'w', encoding='utf-8', newline='\n').write(s)
print('dedup ok')

# ---- export_build 主体重写为 edited/ + lossy/ 目录 ----
i = s.find("pub fn export_build(")
j = s.find("fn first_mdd_index(")
assert i > 0 and j > i
new_body = '''pub fn export_build(
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
    let mut claim = |folder_lossy: bool, name: &str| -> Result<(), String> {
        if !name_seen.insert((folder_lossy, name.to_string())) {
            return Err(format!(
                "导出文件名冲突: {name}（同一{}目录中存在同名源）",
                if folder_lossy { "lossy" } else { "edited" }
            ));
        }
        Ok(())
    };

    // ---- edited/: every loaded source under its original name ----
    if config.edited {
        let edited_dir = out_dir.join("edited");
        std::fs::create_dir_all(&edited_dir).map_err(|e| format!("mkdir edited: {e}"))?;
        for idx in 0..pool.sources.len() {
            let entry = &pool.sources[idx];
            if entry.tombstone {
                continue;
            }
            match &entry.source {
                Source::Mdx(_) => {
                    let has_work = overlay.revisions.keys().any(
                        |id| matches!(id, ResourceId::Mdx { source, .. } if *source as usize == idx),
                    ) || overlay.insertions.iter().any(|i| i.target as usize == idx);
                    if config.only_edited && !has_work {
                        continue;
                    }
                    claim(false, &entry.name)?;
                    let out_path = edited_dir.join(&entry.name);
                    let file = rebuild_mdx_source(pool, overlay, idx, &out_path, config.only_edited, ctl)?;
                    report.ok |= file.check_ok;
                    report.files.push(file);
                }
                Source::Mdd(_) => {
                    let has_work = overlay.revisions.keys().any(
                        |id| matches!(id, ResourceId::Mdd { source, .. } if *source as usize == idx),
                    ) || overlay.insertions.iter().any(|i| i.target as usize == idx);
                    if config.only_edited && !has_work && !receives_embed(idx) {
                        continue;
                    }
                    claim(false, &entry.name)?;
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
    // (images/audio only), mdx and externals are copied verbatim from the
    // edited build (or from source when edited/ was not requested). ----
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
                    claim(true, &entry.name)?;
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
                    claim(true, &entry.name)?;
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

'''
s = s[:i] + new_body + s[j:]

# rebuild_mdx_source 的 only_edited 早退去掉（外层已判断）——保留签名防连锁改动，
# 但空编辑时让它照常全量重建（edited 目录语义就是全量）：
old = """    if only_edited && edits_map.is_empty() && insertions.is_empty() {
        return Ok(ExportedFile {
            path: out_path.display().to_string(),
            entries: 0,
            bytes: 0,
            check_ok: false,
            message: "no edits; skipped".into(),
        });
    }"""
new = """    if only_edited && edits_map.is_empty() && insertions.is_empty() {
        return Ok(ExportedFile {
            path: out_path.display().to_string(),
            entries: 0,
            bytes: 0,
            check_ok: false,
            message: "no edits; skipped".into(),
        });
    }
    // Full rebuild path (edited/ default): an empty EditSet still rebuilds
    // the whole dictionary under one fixed collation."""
assert old in s
s = s.replace(old, new)

open('src/export.rs', 'w', encoding='utf-8', newline='\n').write(s)
print('export_build ok')
