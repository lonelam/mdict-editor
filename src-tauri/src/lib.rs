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

use crate::registry::{ListFilter, Registry, ResourceMeta};
use crate::state::{AppState, Overlay, ResourceId, SourcePool};

#[derive(Serialize, Clone)]
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

/// Opens paths (mdx/mdd/js/css mixed) and appends them as sources.
#[tauri::command]
fn open_sources(paths: Vec<String>, state: tauri::State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let mut pool = state.pool.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    let before = source_infos(&pool).len() as u32;
    for path in paths {
        let entry = SourcePool::open_path(&path)?;
        pool.sources.push(entry);
    }
    let infos = source_infos(&pool);
    // Lazy-index new sources on first browse instead of blocking the open.
    while registry.indices.len() < pool.sources.len() {
        registry.indices.push(None);
    }
    Ok(infos[before as usize..].to_vec())
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

/// Runs the pipeline without writing: reports per-resource before/after.
#[tauri::command]
fn pipeline_dry_run(
    steps: Vec<processors::Processor>,
    scope: pipeline::Scope,
    state: tauri::State<AppState>,
) -> Result<Vec<pipeline::StepReport>, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    registry::ensure_built(&pool, &mut registry)?;
    pipeline::dry_run(&pool, &overlay, &registry, &steps, &scope)
}

/// Applies the pipeline to the scope's resources through the overlay.
#[tauri::command]
fn pipeline_apply(
    steps: Vec<processors::Processor>,
    scope: pipeline::Scope,
    state: tauri::State<AppState>,
) -> Result<Vec<pipeline::StepReport>, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    let mut registry = state.registry.lock().map_err(|e| e.to_string())?;
    registry::ensure_built(&pool, &mut registry)?;
    pipeline::apply(&pool, &mut overlay, &registry, &steps, &scope)
}

/// Builds exported mdx/mdd files from the current overlay state.
#[tauri::command]
fn export_build(
    config: export::ExportConfig,
    state: tauri::State<AppState>,
) -> Result<export::ExportReport, String> {
    let pool = state.pool.lock().map_err(|e| e.to_string())?;
    let overlay = state.overlay.lock().map_err(|e| e.to_string())?;
    export::export_build(&pool, &overlay, &config)
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
            pipeline_dry_run,
            pipeline_apply,
            export_build
        ])
        .register_uri_scheme_protocol("mdres", |ctx, request| {
            use tauri::Manager;
            let state = ctx.app_handle().state::<AppState>();
            handle_mdres_request(&state, request.uri().path())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
