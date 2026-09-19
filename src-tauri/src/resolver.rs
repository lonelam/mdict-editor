//! Reference resolution: turns a link found inside a resource (`src`,
//! `href`, `url()`, `entry://…`, `sound://…`, bare paths in JS strings)
//! into a concrete [`ResourceId`].
//!
//! The lookup sequence mirrors dictionary-authoring conventions: relative to
//! the current resource's directory first, then root-anchored, then bare,
//! and finally a unique-suffix fallback for the classic `/b.png` vs
//! `/img/b.png` mismatch. Keys compare case-insensitively and
//! percent-encoded references are decoded before matching.

use percent_encoding::percent_decode_str;
use serde::Serialize;

use crate::registry::Registry;
use crate::state::{ResourceId, Source, SourcePool};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ResolveOutcome {
    Found {
        target: ResourceId,
        basis: &'static str,
    },
    Ambiguous {
        candidates: Vec<ResourceId>,
        keys: Vec<String>,
    },
    /// http(s)/data/mailto — intentionally not resolved in-app.
    ExternalRef,
    NotFound,
}

/// Normalizes a dictionary path: backslashes → slashes, folds `.`/`..`
/// segments and duplicate separators, keeps exactly one leading slash.
pub fn normalize_path(input: &str) -> String {
    let decoded = percent_decode_str(input).decode_utf8_lossy();
    let trimmed = decoded.trim();
    let mut parts: Vec<&str> = Vec::new();
    for seg in trimmed.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                // Pop, but never pop past the (implicit) root.
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Directory of a normalized path: `/css/style.css` → `/css/`, root stays `/`.
pub fn dir_of(normalized: &str) -> String {
    match normalized.rfind('/') {
        Some(i) => normalized[..=i].to_string(),
        None => "/".to_string(),
    }
}

fn join_ref(dir: &str, reference: &str) -> String {
    normalize_path(&format!("{dir}{reference}"))
}

/// Context directory for resolution: MDD/external resources resolve relative
/// to their own directory; MDX entries resolve from the root.
fn context_dir(pool: &SourcePool, ctx: Option<&ResourceId>) -> Result<String, String> {
    let Some(id) = ctx else {
        return Ok("/".to_string());
    };
    match id {
        ResourceId::Mdd { .. } | ResourceId::Ext { .. } => {
            let key = pool.key_of(id)?;
            Ok(dir_of(&normalize_path(&key)))
        }
        ResourceId::Mdx { .. } => Ok("/".to_string()),
    }
}

/// All exact hits of `lower` across MDD + external indices (sorted rows →
/// binary search). One resource per source per distinct key. MDD keys may or
/// may not carry a leading slash depending on the dictionary, so both forms
/// of the query are probed.
fn exact_hits(
    pool: &SourcePool,
    registry: &Registry,
    lower: &str,
) -> Vec<ResourceId> {
    let mut hits = Vec::new();
    for (idx, entry) in pool.sources.iter().enumerate() {
        if entry.tombstone || !matches!(entry.source, Source::Mdd(_) | Source::External { .. }) {
            continue;
        }
        let Some(Some(index)) = registry.indices.get(idx) else {
            continue;
        };
        let probe = |q: &str| -> Option<u64> {
            let start = index.rows.partition_point(|(k, _)| k.as_str() < q);
            index
                .rows
                .get(start)
                .filter(|(k, _)| k == q)
                .map(|(_, o)| *o)
        };
        if let Some(ordinal) = probe(lower).or_else(|| probe(lower.strip_prefix('/').unwrap_or(lower))) {
            hits.push(match entry.source {
                Source::Mdd(_) => ResourceId::Mdd {
                    source: idx as u32,
                    ordinal,
                },
                _ => ResourceId::Ext {
                    file: idx as u32,
                },
            });
        }
    }
    hits
}

/// Unique-suffix fallback: rows whose key ends with `/suffix` (one hit per
/// source — the lowest ordinal for each matched key).
fn suffix_hits(
    pool: &SourcePool,
    registry: &Registry,
    suffix: &str,
) -> (Vec<ResourceId>, Vec<String>) {
    let mut ids = Vec::new();
    for (idx, entry) in pool.sources.iter().enumerate() {
        if entry.tombstone || !matches!(entry.source, Source::Mdd(_) | Source::External { .. }) {
            continue;
        }
        let Some(Some(index)) = registry.indices.get(idx) else {
            continue;
        };
        for (k, ordinal) in &index.rows {
            if k.ends_with(suffix) {
                ids.push(match entry.source {
                    Source::Mdd(_) => ResourceId::Mdd {
                        source: idx as u32,
                        ordinal: *ordinal,
                    },
                    _ => ResourceId::Ext {
                        file: idx as u32,
                    },
                });
                break;
            }
        }
    }
    let keys = ids
        .iter()
        .map(|id| pool.key_of(id).unwrap_or_default())
        .collect();
    (ids, keys)
}

fn find_entry(pool: &SourcePool, word: &str) -> Option<ResourceId> {
    for (idx, entry) in pool.sources.iter().enumerate() {
        let Source::Mdx(file) = &entry.source else {
            continue;
        };
        if entry.tombstone {
            continue;
        }
        let matches = file.locate(word).ok()??;
        let first = matches.iter().next();
        if let Some(ordinal) = first {
            let value = ordinal.get();
            return Some(ResourceId::Mdx {
                source: idx as u32,
                ordinal: value,
            });
        }
    }
    None
}

/// Resolves `reference` against `ctx`. `registry` must be built (`ensure_built`).
pub fn resolve(
    pool: &SourcePool,
    registry: &Registry,
    reference: &str,
    ctx: Option<&ResourceId>,
) -> Result<ResolveOutcome, String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Ok(ResolveOutcome::NotFound);
    }

    // Scheme split.
    let (scheme, rest) = match reference.split_once(':') {
        Some((s, r)) if r.starts_with("//") && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+') => {
            (s.to_ascii_lowercase(), &r[2..])
        }
        _ => (String::new(), reference),
    };
    match scheme.as_str() {
        "http" | "https" | "data" | "mailto" | "ftp" => return Ok(ResolveOutcome::ExternalRef),
        "entry" | "mdx" => {
            let word = percent_decode_str(rest).decode_utf8_lossy();
            return Ok(match find_entry(pool, &word) {
                Some(target) => ResolveOutcome::Found {
                    target,
                    basis: "entry",
                },
                None => ResolveOutcome::NotFound,
            });
        }
        "sound" => {
            // Sound targets are resource paths, resolved from the root.
            return resolve_path(pool, registry, rest, None);
        }
        _ => {}
    }
    resolve_path(pool, registry, reference, ctx)
}

/// Path-mode resolution used for bare refs, `sound://` and preview requests.
pub fn resolve_path(
    pool: &SourcePool,
    registry: &Registry,
    reference: &str,
    ctx: Option<&ResourceId>,
) -> Result<ResolveOutcome, String> {
    // sound:// paths live in MDDs, never relative to an MDX entry's (empty)
    // context — but keeping the caller's ctx is harmless and more precise.
    let dir = context_dir(pool, ctx)?;
    let decoded = percent_decode_str(reference).decode_utf8_lossy().into_owned();

    let mut candidates: Vec<(&'static str, String)> = Vec::new();
    // A root context makes "rel" identical to "root"; only add it when the
    // context directory is meaningful.
    if ctx.is_some() && dir != "/" {
        candidates.push(("rel", join_ref(&dir, &decoded)));
    }
    candidates.push(("root", normalize_path(&format!("/{decoded}"))));
    candidates.push(("bare", normalize_path(&decoded)));

    for (basis, candidate) in &candidates {
        if candidate == "/" {
            continue;
        }
        let hits = exact_hits(pool, registry, &candidate.to_lowercase());
        match hits.len() {
            1 => {
                return Ok(ResolveOutcome::Found {
                    target: hits.into_iter().next().expect("len 1"),
                    basis,
                })
            }
            0 => {}
            _ => {
                let keys = hits
                    .iter()
                    .map(|id| pool.key_of(id).unwrap_or_default())
                    .collect();
                return Ok(ResolveOutcome::Ambiguous {
                    candidates: hits,
                    keys,
                });
            }
        }
    }

    // Unique-suffix fallback.
    let suffix = format!("/{}", decoded.trim_start_matches('/').to_lowercase());
    let (ids, keys) = suffix_hits(pool, registry, &suffix);
    match ids.len() {
        1 => Ok(ResolveOutcome::Found {
            target: ids.into_iter().next().expect("len 1"),
            basis: "suffix",
        }),
        0 => Ok(ResolveOutcome::NotFound),
        _ => Ok(ResolveOutcome::Ambiguous { candidates: ids, keys }),
    }
}
