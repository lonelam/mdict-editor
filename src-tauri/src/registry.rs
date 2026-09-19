//! The registry: per-source sorted key index, listing and stats.
//!
//! Each source gets a `SourceIndex`: all keys lowercased and sorted, so
//! prefix filters and exact lookups are binary searches. Original-case keys
//! are fetched from the pool only for the visible page, keeping memory at
//! roughly one lowercase key per row.

use serde::Serialize;

use crate::category::Category;
use crate::state::{Overlay, ResourceId, Source, SourcePool};

/// Lowercased key + ordinal, sorted by key.
pub struct SourceIndex {
    pub rows: Vec<(String, u64)>,
}

#[derive(Default)]
pub struct Registry {
    /// Aligned with `SourcePool::sources`; `None` = not built yet (lazy).
    pub indices: Vec<Option<SourceIndex>>,
}

/// A row of the unified resource listing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMeta {
    pub id: ResourceId,
    /// Original-case storage key (entry head / resource path / file name).
    pub key: String,
    pub category: Category,
    pub mime: String,
    pub edited: bool,
    pub deleted: bool,
    pub source_name: String,
    /// Present only when the overlay holds an edit.
    pub size_current: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryStat {
    pub category: Category,
    pub count: u64,
    pub edited: u64,
}

/// Canonical form of a storage key used for all index lookups: lowercased,
/// backslashes folded to slashes, leading separator dropped. MDD keys in the
/// wild come in all three shapes (`\a\b.png`, `/a/b.png`, `a/b.png`).
pub fn normalize_key(key: &str) -> String {
    let folded = key.replace('\\', "/");
    let lowered = folded.to_lowercase();
    lowered
        .strip_prefix('/')
        .unwrap_or(&lowered)
        .to_string()
}

/// Builds the index for one source. External files contribute a single row.
pub fn build_source_index(pool: &SourcePool, source_idx: usize) -> Result<SourceIndex, String> {
    let entry = pool
        .sources
        .get(source_idx)
        .ok_or_else(|| format!("source index {source_idx} not loaded"))?;
    let mut rows = Vec::new();
    if entry.tombstone {
        return Ok(SourceIndex { rows });
    }
    match &entry.source {
        Source::Mdx(file) => {
            for key in file.keys() {
                let key = key.map_err(|e| e.to_string())?;
                rows.push((normalize_key(key.key()), key.ordinal().get()));
            }
        }
        Source::Mdd(file) => {
            for key in file.keys() {
                let key = key.map_err(|e| e.to_string())?;
                rows.push((normalize_key(key.key()), key.ordinal().get()));
            }
        }
        Source::External { .. } => {
            let name = pool
                .key_of(&ResourceId::Ext {
                    file: source_idx as u32,
                })
                .unwrap_or_else(|_| "external".to_string());
            rows.push((name.to_lowercase(), 0));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    Ok(SourceIndex { rows })
}

/// Builds any missing index. `open_sources` pushes `None` placeholders for
/// new sources, so this must fill both absent and placeholder slots.
pub fn ensure_built(pool: &SourcePool, registry: &mut Registry) -> Result<(), String> {
    for idx in 0..pool.sources.len() {
        if idx >= registry.indices.len() {
            registry.indices.push(Some(build_source_index(pool, idx)?));
        } else if registry.indices[idx].is_none() {
            registry.indices[idx] = Some(build_source_index(pool, idx)?);
        }
    }
    Ok(())
}

fn id_for_row(source_idx: usize, kind: &str, ordinal: u64) -> ResourceId {
    match kind {
        "mdx" => ResourceId::Mdx {
            source: source_idx as u32,
            ordinal,
        },
        "mdd" => ResourceId::Mdd {
            source: source_idx as u32,
            ordinal,
        },
        _ => ResourceId::Ext {
            file: source_idx as u32,
        },
    }
}

/// Listing filter: all fields optional; pagination applies after filtering.
pub struct ListFilter<'a> {
    pub source: Option<u32>,
    pub category: Option<Category>,
    pub prefix: &'a str,
    pub offset: u64,
    pub limit: usize,
}

pub fn list_resources(
    pool: &SourcePool,
    overlay: &Overlay,
    registry: &Registry,
    filter: &ListFilter,
) -> Result<Vec<ResourceMeta>, String> {
    let mut out = Vec::new();
    let source_range: Vec<usize> = match filter.source {
        Some(s) => vec![s as usize],
        None => (0..pool.sources.len()).collect(),
    };
    // Keys in the index are stored without a leading slash (MDD convention);
    // accept user input in either form.
    let lower_prefix_raw = filter.prefix.to_lowercase();
    let lower_prefix: &str = lower_prefix_raw
        .strip_prefix('/')
        .unwrap_or(lower_prefix_raw.as_str());
    let mut skipped = 0u64;
    for source_idx in source_range {
        let entry = pool
            .sources
            .get(source_idx)
            .ok_or_else(|| format!("source index {source_idx} not loaded"))?;
        if entry.tombstone {
            continue;
        }
        let Some(Some(index)) = registry.indices.get(source_idx) else {
            continue;
        };
        let kind = entry.kind();
        // Binary-search the prefix range, then scan applying category filter.
        let start = match lower_prefix.is_empty() {
            true => 0,
            false => index.rows.partition_point(|(k, _)| k.as_str() < lower_prefix),
        };
        for (lower_key, ordinal) in &index.rows[start..] {
            if !lower_prefix.is_empty() && !lower_key.starts_with(&lower_prefix) {
                break;
            }
            // Derive category from the row's lowercase key: extension case is
            // irrelevant, and this avoids a positional read per scanned row.
            let category = match kind {
                "mdx" => Category::Entry,
                _ => Category::from_key(lower_key),
            };
            if let Some(want) = filter.category {
                if category != want {
                    continue;
                }
            }
            if skipped < filter.offset {
                skipped += 1;
                continue;
            }
            if out.len() >= filter.limit {
                return Ok(out);
            }
            let id = id_for_row(source_idx, kind, *ordinal);
            let rev = overlay.get(&id);
            out.push(ResourceMeta {
                key: pool.key_of(&id)?,
                category,
                mime: category_mime(&category, &pool.key_of(&id)?),
                edited: rev.is_some(),
                deleted: rev.map(|r| r.deleted).unwrap_or(false),
                source_name: entry.name.clone(),
                size_current: rev.map(|r| r.current.len() as u64),
                id,
            });
        }
    }
    Ok(out)
}

fn category_mime(category: &Category, key: &str) -> String {
    match category {
        Category::Entry => "text/html".to_string(),
        _ => Category::mime_for(key).to_string(),
    }
}

/// Per-category counts across sources (or one source when `source` is set).
pub fn stats(
    pool: &SourcePool,
    overlay: &Overlay,
    registry: &Registry,
    source: Option<u32>,
) -> Result<Vec<CategoryStat>, String> {
    let source_range: Vec<usize> = match source {
        Some(s) => vec![s as usize],
        None => (0..pool.sources.len()).collect(),
    };
    // Category::from_key on every row; also count edited per category.
    let mut counts = std::collections::HashMap::<Category, (u64, u64)>::new();
    for source_idx in source_range {
        let entry = pool
            .sources
            .get(source_idx)
            .ok_or_else(|| format!("source index {source_idx} not loaded"))?;
        if entry.tombstone {
            continue;
        }
        let Some(Some(index)) = registry.indices.get(source_idx) else {
            continue;
        };
        let kind = entry.kind();
        for (lower_key, ordinal) in &index.rows {
            let category = match kind {
                "mdx" => Category::Entry,
                _ => Category::from_key(lower_key),
            };
            let slot = counts.entry(category).or_insert((0, 0));
            slot.0 += 1;
            let id = id_for_row(source_idx, kind, *ordinal);
            if overlay.get(&id).is_some() {
                slot.1 += 1;
            }
        }
    }
    let mut out: Vec<CategoryStat> = counts
        .into_iter()
        .map(|(category, (count, edited))| CategoryStat { category, count, edited })
        .collect();
    out.sort_by(|a, b| a.category.cmp(&b.category));
    Ok(out)
}
