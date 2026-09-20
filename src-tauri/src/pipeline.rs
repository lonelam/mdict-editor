//! Pipeline: select resources by scope, run processors in dry-run or apply
//! mode. Apply writes through the overlay, so every pipeline action stays
//! reversible until export.

use serde::{Deserialize, Serialize};

use crate::category::Category;
use crate::processors::Processor;
use crate::registry::{self, Registry};
use crate::state::{Overlay, ResourceId, SourcePool};

/// Which resources a pipeline run targets. `ids` wins when present.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Scope {
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub source: Option<u32>,
    #[serde(default)]
    pub category: Option<Category>,
    #[serde(default)]
    pub ids: Option<Vec<ResourceId>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepReport {
    pub id: ResourceId,
    pub key: String,
    pub before: u64,
    pub after: u64,
    /// before − after in bytes (positive = saved).
    pub delta: i64,
    /// "ok" | "skip" (no gain / not applicable) | "error"
    pub status: &'static str,
    pub message: String,
}

pub fn select_targets(
    pool: &SourcePool,
    overlay: &Overlay,
    registry: &Registry,
    scope: &Scope,
) -> Result<Vec<crate::registry::ResourceMeta>, String> {
    if let Some(ids) = &scope.ids {
        let mut out = Vec::new();
        for id in ids {
            let key = pool.key_of(id)?;
            let category = match id {
                ResourceId::Mdx { .. } => Category::Entry,
                _ => Category::from_key(&registry::normalize_key(&key)),
            };
            let mime = crate::category::Category::mime_for(&key).to_string();
            let rev = overlay.get(id);
            out.push(crate::registry::ResourceMeta {
                id: id.clone(),
                key,
                category,
                mime,
                edited: rev.is_some(),
                deleted: rev.map(|r| r.deleted).unwrap_or(false),
                source_name: pool.get(id)?.name.clone(),
                size_current: rev.map(|r| r.current.len() as u64),
            });
        }
        return Ok(out);
    }
    registry::list_resources(
        pool,
        overlay,
        registry,
        &registry::ListFilter {
            source: scope.source,
            category: scope.category,
            prefix: "",
            offset: 0,
            limit: usize::MAX / 2,
        },
    )
}

/// Progress + cancellation handles threaded through long-running jobs.
/// `progress(done, total, current_item)` fires per resource; `cancelled` is
/// polled between items so the user can stop a multi-minute batch.
pub struct JobCtl<'a> {
    pub progress: &'a (dyn Fn(u64, u64, &str) + Sync),
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    /// Class/id names actually referenced by dictionary entries; css-purge
    /// drops rules whose selectors never appear here. Empty = no corpus.
    pub used_selectors: &'a std::collections::HashSet<String>,
}

static EMPTY_SELECTORS: std::sync::OnceLock<std::collections::HashSet<String>> =
    std::sync::OnceLock::new();

pub fn no_ctl() -> JobCtl<'static> {
    JobCtl {
        progress: &|_, _, _| {},
        cancelled: &|| false,
        used_selectors: EMPTY_SELECTORS.get_or_init(std::collections::HashSet::new),
    }
}

fn run_steps(
    pool: &SourcePool,
    overlay: &Overlay,
    steps: &[Processor],
    metas: Vec<crate::registry::ResourceMeta>,
    ctl: &JobCtl,
    mut write: impl FnMut(ResourceId, Vec<u8>),
) -> Result<Vec<StepReport>, String> {
    let total = metas.len() as u64;
    // CPU-bound transforms run in parallel (rayon keeps report order);
    // progress is an atomic counter; cancellation is polled per item.
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    let done = AtomicU64::new(0);
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let writes: std::sync::Mutex<Vec<(ResourceId, Vec<u8>)>> = std::sync::Mutex::new(Vec::new());

    let reports: Vec<StepReport> = metas
        .into_par_iter()
        .map(|meta| {
            if cancelled.load(Ordering::Relaxed) || (ctl.cancelled)() {
                cancelled.store(true, Ordering::Relaxed);
                return StepReport {
                    id: meta.id,
                    key: meta.key,
                    before: 0,
                    after: 0,
                    delta: 0,
                    status: "skip",
                    message: "已取消".into(),
                };
            }
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            let latest_key = meta.key.clone();
            (ctl.progress)(n, total, &latest_key);
            if meta.deleted {
                return StepReport {
                    id: meta.id,
                    key: meta.key,
                    before: 0,
                    after: 0,
                    delta: 0,
                    status: "skip",
                    message: "deleted".into(),
                };
            }
            let bytes: Vec<u8> = match overlay.get(&meta.id) {
                Some(rev) => rev.current.clone(),
                None => match pool.read_original(&meta.id) {
                    Ok(b) => b,
                    Err(e) => {
                        return StepReport {
                            id: meta.id,
                            key: meta.key,
                            before: 0,
                            after: 0,
                            delta: 0,
                            status: "error",
                            message: e,
                        }
                    }
                },
            };
            let (current, applied): (Vec<u8>, Option<()>) = {
                let mut current = bytes.clone();
                let mut error: Option<String> = None;
                let mut applied = false;
                for step in steps {
                    if !step.applies_to(meta.category) {
                        continue;
                    }
                    match step.process(&meta.key, &current, ctl.used_selectors) {
                        Ok(out) => {
                            current = out;
                            applied = true;
                        }
                        Err(e) => {
                            error = Some(format!("{}: {}", step.name(), e));
                            break;
                        }
                    }
                }
                if let Some(e) = error {
                    return StepReport {
                        id: meta.id,
                        key: meta.key,
                        before: bytes.len() as u64,
                        after: 0,
                        delta: 0,
                        status: "error",
                        message: e,
                    };
                }
                (current, applied.then_some(()))
            };
            match applied {
                Some(()) => {
                    let before = bytes.len() as u64;
                    let after = current.len() as u64;
                    if after < before {
                        writes
                            .lock()
                            .expect("writes")
                            .push((meta.id.clone(), current.clone()));
                        StepReport {
                            id: meta.id,
                            key: meta.key,
                            before,
                            after,
                            delta: before as i64 - after as i64,
                            status: "ok",
                            message: String::new(),
                        }
                    } else {
                        StepReport {
                            id: meta.id,
                            key: meta.key,
                            before,
                            after,
                            delta: before as i64 - after as i64,
                            status: "skip",
                            message: "no size gain".into(),
                        }
                    }
                }
                None => StepReport {
                    id: meta.id,
                    key: meta.key,
                    before: bytes.len() as u64,
                    after: bytes.len() as u64,
                    delta: 0,
                    status: "skip",
                    message: "no applicable processor for this resource".into(),
                },
            }
        })
        .collect();

    if cancelled.load(Ordering::Relaxed) {
        return Err("已取消".into());
    }
    for (id, bytes) in writes.into_inner().expect("writes") {
        write(id, bytes);
    }
    Ok(reports)
}

pub fn dry_run(
    pool: &SourcePool,
    overlay: &Overlay,
    registry: &Registry,
    steps: &[Processor],
    scope: &Scope,
    ctl: &JobCtl,
) -> Result<Vec<StepReport>, String> {
    let metas = select_targets(pool, overlay, registry, scope)?;
    dry_run_selected(pool, overlay, steps, metas, ctl)
}

/// Runs on a pre-selected target snapshot — callers drop their registry lock
/// before the (potentially minutes-long) walk so UI reads stay live.
pub fn dry_run_selected(
    pool: &SourcePool,
    overlay: &Overlay,
    steps: &[Processor],
    metas: Vec<crate::registry::ResourceMeta>,
    ctl: &JobCtl,
) -> Result<Vec<StepReport>, String> {
    run_steps(pool, overlay, steps, metas, ctl, |_, _| {})
}

pub fn apply(
    pool: &SourcePool,
    overlay: &mut Overlay,
    registry: &Registry,
    steps: &[Processor],
    scope: &Scope,
    ctl: &JobCtl,
) -> Result<Vec<StepReport>, String> {
    let metas = select_targets(pool, overlay, registry, scope)?;
    let (reports, applied) = apply_selected(pool, overlay, steps, metas, ctl)?;
    commit_applied(pool, overlay, applied)?;
    Ok(reports)
}

/// Runs the walk with a read-only overlay and returns the pending writes
/// alongside the reports; the caller commits them under a short write lock
/// so long applies never block preview readers.
pub fn apply_selected(
    pool: &SourcePool,
    overlay: &Overlay,
    steps: &[Processor],
    metas: Vec<crate::registry::ResourceMeta>,
    ctl: &JobCtl,
) -> Result<(Vec<StepReport>, Vec<(ResourceId, Vec<u8>)>), String> {
    let mut applied: Vec<(ResourceId, Vec<u8>)> = Vec::new();
    let reports = run_steps(pool, overlay, steps, metas, ctl, |id, bytes| {
        applied.push((id, bytes));
    })?;
    Ok((reports, applied))
}

/// Bulk-commits pipeline results; hold the overlay write lock only here.
pub fn commit_applied(
    pool: &SourcePool,
    overlay: &mut Overlay,
    applied: Vec<(ResourceId, Vec<u8>)>,
) -> Result<(), String> {
    for (id, bytes) in applied {
        let original = match overlay.get(&id) {
            Some(rev) => rev.original.clone(),
            None => pool.read_original(&id)?,
        };
        overlay.write(id, original, bytes);
    }
    Ok(())
}
