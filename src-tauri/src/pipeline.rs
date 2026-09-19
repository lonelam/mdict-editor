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

fn select_targets(
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
}

pub const NO_CTL: JobCtl<'static> = JobCtl {
    progress: &|_, _, _| {},
    cancelled: &|| false,
};

fn run_steps(
    pool: &SourcePool,
    overlay: &Overlay,
    steps: &[Processor],
    metas: Vec<crate::registry::ResourceMeta>,
    ctl: &JobCtl,
    mut write: impl FnMut(ResourceId, Vec<u8>),
) -> Result<Vec<StepReport>, String> {
    let total = metas.len() as u64;
    let mut reports = Vec::new();
    for (i, meta) in metas.into_iter().enumerate() {
        if (ctl.cancelled)() {
            return Err("已取消".into());
        }
        (ctl.progress)(i as u64, total, &meta.key);
        if meta.deleted {
            continue;
        }
        let bytes = match overlay.get(&meta.id) {
            Some(rev) => rev.current.clone(),
            None => match pool.read_original(&meta.id) {
                Ok(b) => b,
                Err(e) => {
                    reports.push(StepReport {
                        id: meta.id,
                        key: meta.key,
                        before: 0,
                        after: 0,
                        delta: 0,
                        status: "error",
                        message: e,
                    });
                    continue;
                }
            },
        };
        // First applicable step wins; later steps of another type still run
        // on their own resources. Multiple steps of matching types chain on
        // the same bytes in order.
        let mut current = bytes.clone();
        let mut applied = false;
        let mut error: Option<String> = None;
        for step in steps {
            if !step.applies_to(meta.category) {
                continue;
            }
            match step.process(&meta.key, &current) {
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
        match (applied, error) {
            (_, Some(e)) => reports.push(StepReport {
                id: meta.id,
                key: meta.key,
                before: bytes.len() as u64,
                after: 0,
                delta: 0,
                status: "error",
                message: e,
            }),
            (true, None) => {
                let before = bytes.len() as u64;
                let after = current.len() as u64;
                let (status, do_write) = if after < before {
                    ("ok", true)
                } else {
                    ("skip", false)
                };
                if do_write {
                    write(meta.id.clone(), current.clone());
                }
                reports.push(StepReport {
                    id: meta.id,
                    key: meta.key,
                    before,
                    after,
                    delta: before as i64 - after as i64,
                    status,
                    message: String::new(),
                });
            }
            (false, None) => reports.push(StepReport {
                id: meta.id,
                key: meta.key,
                before: bytes.len() as u64,
                after: bytes.len() as u64,
                delta: 0,
                status: "skip",
                message: "no applicable processor for this resource".into(),
            }),
        }
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
    let mut applied: Vec<(ResourceId, Vec<u8>)> = Vec::new();
    let reports = run_steps(pool, overlay, steps, metas, ctl, |id, bytes| {
        applied.push((id, bytes));
    })?;
    // Deferred writes keep the closure uniform between dry_run and apply.
    for (id, bytes) in applied {
        let original = match overlay.get(&id) {
            Some(rev) => rev.original.clone(),
            None => pool.read_original(&id)?,
        };
        overlay.write(id, original, bytes);
    }
    Ok(reports)
}
