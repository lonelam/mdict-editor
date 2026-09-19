//! Tauri command layer. All business logic lives in the sibling modules;
//! these handlers only lock state, delegate and map errors to strings.

pub mod category;
pub mod export;
#[doc(hidden)]
pub mod fixtures;
pub mod pipeline;
pub mod processors;
pub mod registry;
pub mod resolver;
pub mod state;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::pipeline::JobCtl;
use crate::registry::{ListFilter, Registry, ResourceMeta};
use crate::state::{AppState, Overlay, ResourceId, SourcePool};

/// Background job registry: cancellation flags keyed by job id. Workers emit
/// `job-progress` / `job-done` events; the UI listens instead of holding a
/// blocking invoke for multi-minute batches.
#[derive(Default)]
struct Jobs {
    counter: std::sync::atomic::AtomicU64,
    cancels: Mutex<HashMap<String, std::sync::Arc<AtomicBool>>>,
}

fn start_job<T, F>(app: &AppHandle, name: &str, work: F) -> String
where
    T: Serialize + 'static,
    F: FnOnce(&AppState, &JobCtl) -> Result<T, String> + Send + 'static,
{
    let jobs = app.state::<Jobs>();
    let n = jobs.counter.fetch_add(1, Ordering::Relaxed);
    let id = format!("{name}-{}", n);
    let flag = Arc::new(AtomicBool::new(false));
    jobs.cancels.lock().expect("jobs").insert(id.clone(), flag.clone());
    let app_progress = app.clone();
    let id_progress = id.clone();
    let app_done = app.clone();
    let id_done = id.clone();
    let app_cleanup = app.clone();
    let id_cleanup = id.clone();
    std::thread::spawn(move || {
        let state = app_progress.state::<AppState>();
        let ctl = JobCtl {
            progress: &|done, total, item| {
                let _ = app_progress.emit(
                    "job-progress",
                    serde_json::json!({"job": id_progress, "done": done, "total": total, "item": item}),
                );
            },
            cancelled: &|| flag.load(Ordering::Relaxed),
        };
        let (ok, value, error) = match work(&state, &ctl) {
            Ok(value) => (true, Some(serde_json::to_value(&value).ok()), None),
            Err(error) => (false, None, Some(error)),
        };
        let _ = app_done.emit(
            "job-done",
            serde_json::json!({"job": id_done, "ok": ok, "value": value, "error": error}),
        );
        app_cleanup
            .state::<Jobs>()
            .cancels
            .lock()
            .expect("jobs")
            .remove(&id_cleanup);
    });
    id
}

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceInfo {
    id: u32,
    kind: &'static str,
    name: String,
    title: Option<String>,
    entry_count: u64,
}

/// Content of one resource: text inline for text categories, base64 otherwise.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceContent {
    meta: ResourceMeta,
    text: Option<String>,
    data_b64: Option<String>,
    /// Current byte size (overlay-aware).
    size_current: u64,
    /// Source byte size when an edit exists.
    size_original: Option<u64>,
    history_len: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionInfo {
    history_len: usize,
    size_original: u64,
    size_current: u64,
}

fn source_infos(pool: &SourcePool) -> Vec<SourceInfo> {
    pool.sources
        .iter()
        .enumerate()
        .map(|(id, e)| SourceInfo {
            id: id as u32,
            kind: e.kind(),
            name: e.name.clone(),
            title: e.title.clone(),
            entry_count: e.entry_count(),
        })
        .collect()
}

/// Locks pool + registry, ensures indices, runs `f`.
fn with_registry<T>(
    state: &AppState,
    f: impl FnOnce(&SourcePool, &Registry) -> Result<T, String>,
) -> Result<T, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    registry::ensure_built(&pool, &mut registry)?;
    f(&pool, &registry)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenResult {
    added: Vec<SourceInfo>,
    /// Same-file re-opens that were skipped (deduped by canonical path).
    skipped: Vec<String>,
    /// Files that failed to open, with reason; the batch is not aborted.
    errors: Vec<String>,
}

/// Opens paths (mdx/mdd/js/css mixed) and appends them as active sources.
/// Same-path re-opens are skipped; per-file errors do not abort the batch.
#[tauri::command]
fn open_sources(paths: Vec<String>, state: tauri::State<AppState>) -> Result<OpenResult, String> {
    let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    let mut result = OpenResult {
        added: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };
    for path in paths {
        match SourcePool::open_path(&path) {
            Ok(entry) => {
                if pool.has_active_path(&entry.path) {
                    result.skipped.push(entry.name);
                    continue;
                }
                let info = SourceInfo {
                    id: pool.sources.len() as u32,
                    kind: entry.kind(),
                    name: entry.name.clone(),
                    title: entry.title.clone(),
                    entry_count: entry.entry_count(),
                };
                pool.sources.push(entry);
                registry.indices.push(None);
                result.added.push(info);
            }
            Err(e) => result.errors.push(e),
        }
    }
    Ok(result)
}

/// Removes (tombstones) a source. Ids stay stable; overlay edits that point
/// into the removed source are dropped.
#[tauri::command]
fn remove_source(id: u32, state: tauri::State<AppState>) -> Result<bool, String> {
    let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    pool.remove(id as usize)?;
    overlay.revisions.retain(|k, _| k.source_index() != id);
    Ok(true)
}

#[tauri::command]
fn resource_stats(
    source: Option<u32>,
    state: tauri::State<AppState>,
) -> Result<Vec<registry::CategoryStat>, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    registry::ensure_built(&pool, &mut registry)?;
    registry::stats(&pool, &overlay, &registry, source)
}

#[tauri::command]
fn list_resources(
    source: Option<u32>,
    category: Option<String>,
    prefix: String,
    offset: u64,
    limit: u32,
    state: tauri::State<AppState>,
) -> Result<Vec<ResourceMeta>, String> {
    let category = match category {
        None => None,
        Some(c) => Some(
            serde_json::from_value::<crate::category::Category>(c.into())
                .map_err(|e| e.to_string())?,
        ),
    };
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    registry::ensure_built(&pool, &mut registry)?;
    registry::list_resources(
        &pool,
        &overlay,
        &registry,
        &ListFilter {
            source,
            category,
            prefix: &prefix,
            offset,
            limit: (limit as usize).clamp(1, 1000),
        },
    )
}

#[tauri::command]
fn resource_meta(id: ResourceId, state: tauri::State<AppState>) -> Result<ResourceMeta, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    registry::ensure_built(&pool, &mut registry)?;
    let key = pool.key_of(&id)?;
    let entry = pool.get(&id)?;
    let (category, mime) = match id {
        ResourceId::Mdx { .. } => (
            crate::category::Category::Entry,
            "text/html".to_string(),
        ),
        _ => {
            let category = crate::category::Category::from_key(&key.to_lowercase());
            let mime = crate::category::Category::mime_for(&key).to_string();
            (category, mime)
        }
    };
    let rev = overlay.get(&id);
    Ok(ResourceMeta {
        key,
        category,
        mime,
        edited: rev.is_some(),
        deleted: rev.map(|r| r.deleted).unwrap_or(false),
        source_name: entry.name.clone(),
        size_current: rev.map(|r| r.current.len() as u64),
        id,
    })
}

fn read_content(pool: &SourcePool, overlay: &Overlay, id: &ResourceId) -> Result<ResourceContent, String> {
    let meta = {
        let key = pool.key_of(id)?;
        let (category, mime) = match id {
            ResourceId::Mdx { .. } => (
                crate::category::Category::Entry,
                "text/html".to_string(),
            ),
            _ => {
                let category = crate::category::Category::from_key(&key.to_lowercase());
                let mime = crate::category::Category::mime_for(&key).to_string();
                (category, mime)
            }
        };
        let rev = overlay.get(id);
        ResourceMeta {
            key: key.clone(),
            category,
            mime,
            edited: rev.is_some(),
            deleted: rev.map(|r| r.deleted).unwrap_or(false),
            source_name: pool.get(id)?.name.clone(),
            size_current: rev.map(|r| r.current.len() as u64),
            id: id.clone(),
        }
    };
    let bytes = match overlay.get(id) {
        Some(rev) => rev.current.clone(),
        None => pool.read_original(id)?,
    };
    let (text, data_b64) = if meta.category.is_text() {
        (Some(String::from_utf8_lossy(&bytes).into_owned()), None)
    } else {
        (
            None,
            Some(base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &bytes,
            )),
        )
    };
    let history_len = overlay.get(id).map(|r| r.history.len()).unwrap_or(0);
    Ok(ResourceContent {
        size_original: overlay.get(id).map(|r| r.original.len() as u64),
        size_current: bytes.len() as u64,
        meta,
        text,
        data_b64,
        history_len,
    })
}

#[tauri::command]
fn read_resource(
    id: ResourceId,
    state: tauri::State<AppState>,
) -> Result<ResourceContent, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    read_content(&pool, &overlay, &id)
}

#[tauri::command]
fn write_resource(
    id: ResourceId,
    bytes: Vec<u8>,
    state: tauri::State<AppState>,
) -> Result<RevisionInfo, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    let original = match overlay.get(&id) {
        Some(rev) => rev.original.clone(),
        None => pool.read_original(&id)?,
    };
    let (history_len, size_original) = overlay.write(id.clone(), original, bytes);
    let rev = overlay.get(&id).expect("just written");
    Ok(RevisionInfo {
        history_len,
        size_original: size_original as u64,
        size_current: rev.current.len() as u64,
    })
}

#[tauri::command]
fn undo_resource(id: ResourceId, state: tauri::State<AppState>) -> Result<bool, String> {
    let mut overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    Ok(overlay.undo(&id))
}

/// Removes the whole edit chain; the resource returns to source content.
#[tauri::command]
fn revert_resource(id: ResourceId, state: tauri::State<AppState>) -> Result<bool, String> {
    let mut overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    Ok(overlay.revert(&id))
}

#[tauri::command]
fn delete_resource(id: ResourceId, state: tauri::State<AppState>) -> Result<(), String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    let original = match overlay.get(&id) {
        Some(rev) => rev.original.clone(),
        None => pool.read_original(&id)?,
    };
    overlay.delete(id, original);
    Ok(())
}

/// Resolves a reference found inside `context` (Ctrl+Click jump support).
#[tauri::command]
fn resolve_reference(
    reference: String,
    context: Option<ResourceId>,
    state: tauri::State<AppState>,
) -> Result<resolver::ResolveOutcome, String> {
    with_registry(&state, |pool, registry| {
        resolver::resolve(pool, registry, &reference, context.as_ref())
    })
}

/// Serves `mdres://preview/<token>/<relative path>` for the preview iframe:
/// relative refs inside HTML/CSS resolve to real resource bytes via the same
/// resolver the jump feature uses. `<token>` = `mdx-<src>-<ord>` /
/// `mdd-<src>-<ord>` / `ext-<file>-0` identifies the context resource.
pub fn handle_mdres_request(state: &AppState, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::Response;

    let not_found = |msg: &str| {
        Response::builder()
            .status(404)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(msg.as_bytes().to_vec())
            .unwrap()
    };
    let ok = |mime: &str, bytes: Vec<u8>| {
        Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", "no-store")
            .body(bytes)
            .unwrap()
    };

    let decoded = percent_encoding::percent_decode_str(uri_path)
        .decode_utf8_lossy()
        .into_owned();
    let path = decoded.trim_start_matches('/');
    let Some(rest) = path.strip_prefix("preview/") else {
        return not_found("expected /preview/<token>/<path>");
    };
    let (token, rel) = match rest.split_once('/') {
        Some((t, r)) => (t, r),
        None => (rest, ""),
    };
    let parts: Vec<&str> = token.split('-').collect();
    let ctx = match parts.as_slice() {
        ["mdx", s, o] => ResourceId::Mdx {
            source: s.parse().unwrap_or(0),
            ordinal: o.parse().unwrap_or(0),
        },
        ["mdd", s, o] => ResourceId::Mdd {
            source: s.parse().unwrap_or(0),
            ordinal: o.parse().unwrap_or(0),
        },
        ["ext", f, _] => ResourceId::Ext {
            file: f.parse().unwrap_or(0),
        },
        _ => return not_found("bad preview token"),
    };

    let pool = match state.pool.lock() {
        Ok(p) => p,
        Err(_) => return not_found("state poisoned"),
    };
    let overlay = match state.overlay.lock() {
        Ok(o) => o,
        Err(_) => return not_found("state poisoned"),
    };

    // Empty rel = the context resource itself (MDX entry HTML preview).
    if rel.is_empty() {
        let bytes = match state::current_bytes(&pool, &overlay, &ctx) {
            Ok(b) => b,
            Err(e) => return not_found(&e),
        };
        return ok("text/html; charset=utf-8", bytes);
    }

    let mut registry = match state.registry.lock() {
        Ok(r) => r,
        Err(_) => return not_found("state poisoned"),
    };
    if registry::ensure_built(&pool, &mut registry).is_err() {
        return not_found("registry build failed");
    }
    match resolver::resolve_path(&pool, &registry, rel, Some(&ctx)) {
        Ok(resolver::ResolveOutcome::Found { target, .. }) => {
            match state::current_bytes(&pool, &overlay, &target) {
                Ok(bytes) => {
                    let key = pool.key_of(&target).unwrap_or_default();
                    ok(crate::category::Category::mime_for(&key), bytes)
                }
                Err(e) => not_found(&e),
            }
        }
        _ => not_found("resource not found"),
    }
}

/// Starts a dry-run job; progress/done arrive via events. Returns job id.
#[tauri::command]
fn pipeline_dry_run_start(
    steps: Vec<processors::Processor>,
    scope: pipeline::Scope,
    app: AppHandle,
) -> String {
    start_job(&app, "pipeline-dry", move |st, ctl| {
        let pool = st.pool.lock().map_err(|e| e.to_string())?;
        let overlay = st.overlay.lock().map_err(|e| e.to_string())?;
        let mut registry = st.registry.lock().map_err(|e| e.to_string())?;
        registry::ensure_built(&pool, &mut registry)?;
        pipeline::dry_run(&pool, &overlay, &registry, &steps, &scope, ctl)
    })
}

/// Starts a pipeline apply job; writes go through the overlay.
#[tauri::command]
fn pipeline_apply_start(
    steps: Vec<processors::Processor>,
    scope: pipeline::Scope,
    app: AppHandle,
) -> String {
    start_job(&app, "pipeline-apply", move |st, ctl| {
        let pool = st.pool.lock().map_err(|e| e.to_string())?;
        let mut overlay = st.overlay.lock().map_err(|e| e.to_string())?;
        let mut registry = st.registry.lock().map_err(|e| e.to_string())?;
        registry::ensure_built(&pool, &mut registry)?;
        pipeline::apply(&pool, &mut overlay, &registry, &steps, &scope, ctl)
    })
}

/// Starts an export job; outputs land in `config.out_dir`.
#[tauri::command]
fn export_start(config: export::ExportConfig, app: AppHandle) -> String {
    start_job(&app, "export", move |st, ctl| {
        let pool = st.pool.lock().map_err(|e| e.to_string())?;
        let overlay = st.overlay.lock().map_err(|e| e.to_string())?;
        export::export_build(&pool, &overlay, &config, ctl)
    })
}

/// Flags a running job for cancellation between items.
#[tauri::command]
fn cancel_job(job: String, app: AppHandle) -> bool {
    let jobs = app.state::<Jobs>();
    let flag = jobs.cancels.lock().expect("jobs").get(&job).cloned();
    match flag {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            open_sources,
            resource_stats,
            list_resources,
            resource_meta,
            read_resource,
            write_resource,
            undo_resource,
            revert_resource,
            delete_resource,
            resolve_reference,
            remove_source,
            pipeline_dry_run_start,
            pipeline_apply_start,
            export_start,
            cancel_job
        ])
        .register_uri_scheme_protocol("mdres", |ctx, request| {
            use tauri::Manager;
            let state = ctx.app_handle().state::<AppState>();
            handle_mdres_request(&state, request.uri().path())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
