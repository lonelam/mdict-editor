//! Tauri command layer. All business logic lives in the sibling modules;
//! these handlers only lock state, delegate and map errors to strings.

pub mod assets;
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
    start_job_with_selectors(app, name, std::collections::HashSet::new(), work)
}

/// Collects class/id names actually used across all MDX entries — the
/// corpus css-purge keeps rules against.
fn collect_used_selectors(pool: &crate::state::SourcePool) -> std::collections::HashSet<String> {
    let mut used = std::collections::HashSet::new();
    for entry in pool.sources.iter().filter(|e| !e.tombstone) {
        let crate::state::Source::Mdx(file) = &entry.source else {
            continue;
        };
        for entry_item in file.entries() {
            let Ok(entry_item) = entry_item else { continue };
            collect_from_html(entry_item.text(), &mut used);
        }
    }
    used
}

/// Extracts `class="a b"` and `id="x"` values (best-effort regex-free scan).
fn collect_from_html(html: &str, used: &mut std::collections::HashSet<String>) {
    let bytes = html.as_bytes();
    let mut i = 0;
    while i + 6 < bytes.len() {
        let lower_window: &[u8; 5] = match bytes[i..i + 5].try_into() {
            Ok(w) => w,
            Err(_) => break,
        };
        let is_class = lower_window.eq_ignore_ascii_case(b"class");
        let is_id = !is_class
            && bytes[i..i + 3].eq_ignore_ascii_case(b"id=")
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-'));
        if is_class || is_id {
            let mut j = i + if is_class { 5 } else { 2 };
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'=') {
                j += 1;
            }
            let quote = if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                j += 1;
                Some(bytes[j - 1])
            } else {
                None
            };
            let start = j;
            while j < bytes.len() {
                let stop = match quote {
                    Some(q) => bytes[j] == q,
                    None => bytes[j].is_ascii_whitespace(),
                };
                if stop {
                    break;
                }
                j += 1;
            }
            let value = &html[start..j.min(html.len())];
            for token in value.split_whitespace() {
                used.insert(token.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_').to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

/// Entry-corpus selectors for css-purge; empty when the pool is locked.
fn corpus_for(app: &AppHandle) -> std::collections::HashSet<String> {
    let state = app.state::<AppState>();
    let pool = match state.pool.read() {
        Ok(pool) => pool,
        Err(_) => return Default::default(),
    };
    collect_used_selectors(&pool)
}

/// Same as [`start_job`], carrying the entry-corpus selector set collected
/// by the caller (css-purge input).
fn start_job_with_selectors<T, F>(
    app: &AppHandle,
    name: &str,
    used_selectors: std::collections::HashSet<String>,
    work: F,
) -> String
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
    // Progress events are throttled to ~12/s: a 793MB export walks tens of
    // thousands of resources and an event per item floods the webview.
    let last_emit_ms = std::sync::Mutex::new(0u128);
    let clock = std::time::Instant::now();
    let selectors = std::sync::Arc::new(used_selectors);
    let selectors_ctl = std::sync::Arc::clone(&selectors);
    std::thread::spawn(move || {
        let state = app_progress.state::<AppState>();
        let selectors_ref: &std::collections::HashSet<String> = &selectors_ctl;
        let ctl = JobCtl {
            progress: &|done, total, item| {
                let now = clock.elapsed().as_millis();
                {
                    let mut last = last_emit_ms.lock().expect("throttle");
                    if now - *last < 80 && done < total {
                        return;
                    }
                    *last = now;
                }
                let _ = app_progress.emit(
                    "job-progress",
                    serde_json::json!({"job": id_progress, "done": done, "total": total, "item": item}),
                );
            },
            cancelled: &|| flag.load(Ordering::Relaxed),
            used_selectors: selectors_ref,
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
    /// Parent directory of the source file (export default location).
    dir: Option<String>,
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

/// Locks pool + registry, ensures indices, runs `f`.
fn with_registry<T>(
    state: &AppState,
    f: impl FnOnce(&SourcePool, &Registry) -> Result<T, String>,
) -> Result<T, String> {
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
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

/// Registers an already-opened [`SourceEntry`] in the pool (dedupe by
/// canonical path) and returns its [`SourceInfo`].
fn register_source(
    pool: &mut SourcePool,
    registry: &mut Registry,
    entry: crate::state::SourceEntry,
) -> Result<SourceInfo, String> {
    if pool.has_active_path(&entry.path) {
        return Err(format!("{}: 已加载（重复打开已跳过）", entry.name));
    }
    let info = SourceInfo {
        id: pool.sources.len() as u32,
        kind: entry.kind(),
        name: entry.name.clone(),
        title: entry.title.clone(),
        entry_count: entry.entry_count(),
        dir: entry.path.parent().map(|p| p.display().to_string()),
    };
    pool.sources.push(entry);
    registry.indices.push(None);
    Ok(info)
}

/// Opens paths (mdx/mdd/js/css mixed) and appends them as active sources.
/// Same-path re-opens are skipped; per-file errors do not abort the batch.
#[tauri::command]
fn open_sources(paths: Vec<String>, state: tauri::State<AppState>) -> Result<OpenResult, String> {
    let mut pool = state.pool.write().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
    let mut result = OpenResult {
        added: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };
    for path in paths {
        match SourcePool::open_path(&path) {
            Ok(entry) => {
                let name = entry.name.clone();
                match register_source(&mut pool, &mut registry, entry) {
                    Ok(info) => result.added.push(info),
                    Err(_) => result.skipped.push(name),
                }
            }
            Err(e) => result.errors.push(e),
        }
    }
    Ok(result)
}

/// Creates a brand-new empty dictionary file at `path` (`kind` = "mdx" |
/// "mdd") and opens it as a source. Existing files are never overwritten.
#[tauri::command]
fn create_source(
    kind: String,
    path: String,
    state: tauri::State<AppState>,
) -> Result<SourceInfo, String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err("路径不能为空".into());
    }
    let p = std::path::PathBuf::from(&path);
    if p.exists() {
        return Err(format!("文件已存在，未覆盖: {path}"));
    }
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut bytes = Vec::new();
    match kind.as_str() {
        "mdx" => {
            let mut builder = mdictlib::MdxBuilder::with_options(
                mdictlib::WriteOptions::new()
                    .with_encoding(mdictlib::WriteEncoding::Utf8)
                    .with_compression(mdictlib::WriteCompression::Zlib),
            );
            if !stem.is_empty() {
                builder.header_attribute("Title", &stem).map_err(|e| e.to_string())?;
            }
            builder
                .finish(&mut bytes)
                .map_err(|e| format!("生成 MDX 失败: {e}"))?;
        }
        "mdd" => {
            let mut builder = mdictlib::MddBuilder::with_options(
                mdictlib::WriteOptions::new()
                    .with_compression(mdictlib::WriteCompression::Zlib),
            );
            if !stem.is_empty() {
                builder.header_attribute("Title", &stem).map_err(|e| e.to_string())?;
            }
            builder
                .finish(&mut bytes)
                .map_err(|e| format!("生成 MDD 失败: {e}"))?;
        }
        other => return Err(format!("未知的源类型 {other:?}（mdx / mdd）")),
    }
    if let Some(parent) = p.parent() {
        // The save dialog should have picked an existing directory, but a
        // hand-typed path may not have one yet.
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(&p, &bytes).map_err(|e| format!("写入失败: {e}"))?;
    let entry = SourcePool::open_path(&path)?;
    let mut pool = state.pool.write().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
    register_source(&mut pool, &mut registry, entry)
}

/// Removes (tombstones) a source. Ids stay stable; overlay edits that point
/// into the removed source are dropped.
#[tauri::command]
fn remove_source(id: u32, state: tauri::State<AppState>) -> Result<bool, String> {
    let mut pool = state.pool.write().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    pool.remove(id as usize)?;
    overlay.revisions.retain(|k, _| k.source_index() != id);
    Ok(true)
}

#[tauri::command]
fn resource_stats(
    source: Option<u32>,
    state: tauri::State<AppState>,
) -> Result<Vec<registry::CategoryStat>, String> {
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let overlay = state.overlay.read().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
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
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let overlay = state.overlay.read().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
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
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let overlay = state.overlay.read().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
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
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let overlay = state.overlay.read().map_err(|e| e.to_string())?;
    read_content(&pool, &overlay, &id)
}

#[tauri::command]
fn write_resource(
    id: ResourceId,
    bytes: Vec<u8>,
    state: tauri::State<AppState>,
) -> Result<RevisionInfo, String> {
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
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
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    Ok(overlay.undo(&id))
}

/// Removes the whole edit chain; the resource returns to source content.
#[tauri::command]
fn revert_resource(id: ResourceId, state: tauri::State<AppState>) -> Result<bool, String> {
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    Ok(overlay.revert(&id))
}

#[tauri::command]
fn delete_resource(id: ResourceId, state: tauri::State<AppState>) -> Result<(), String> {
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    let original = match overlay.get(&id) {
        Some(rev) => rev.original.clone(),
        None => pool.read_original(&id)?,
    };
    overlay.delete(id, original);
    Ok(())
}

/// Real dictionary entries rarely link their companion CSS — reader apps
/// load it globally. The preview reproduces that by injecting `<link>` tags
/// for every loaded CSS (external files first, then MDD css resources) into
/// MDX entry HTML, skipping stylesheets the entry already references.
fn inject_preview_css(
    pool: &SourcePool,
    overlay: &Overlay,
    registry: &Registry,
    html: &[u8],
) -> Vec<u8> {
    let text = match std::str::from_utf8(html) {
        Ok(t) => t,
        Err(_) => return html.to_vec(),
    };
    let mut links: Vec<String> = Vec::new();
    for (idx, entry) in pool.sources.iter().enumerate() {
        if entry.tombstone {
            continue;
        }
        match &entry.source {
            crate::state::Source::External { path, .. } => {
                let is_css = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("css"));
                if is_css {
                    let id = ResourceId::Ext { file: idx as u32 };
                    if overlay.get(&id).is_some_and(|r| !r.deleted) {
                        if let Ok(name) = pool.key_of(&id) {
                            links.push(name);
                        }
                    }
                }
            }
            crate::state::Source::Mdd(file) => {
                // Original-case hrefs: one representative key per css name.
                if registry.indices.get(idx).is_some_and(|o| o.is_some()) {
                    for key in file.keys().flatten() {
                        if key.key().to_lowercase().ends_with(".css") {
                            let id = ResourceId::Mdd {
                                source: idx as u32,
                                ordinal: key.ordinal().get(),
                            };
                            if overlay.get(&id).is_some_and(|r| !r.deleted) {
                                links.push(key.key().to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    links.sort();
    links.dedup();
    let tags: Vec<String> = links
        .into_iter()
        .filter(|name| {
            let encoded = name.replace(' ', "%20");
            !text.contains(&encoded) && !text.contains(name.as_str())
        })
        .map(|name| {
            let encoded = name.replace(' ', "%20");
            format!("<link rel=\"stylesheet\" href=\"{}\">", encoded)
        })
        .collect();
    if tags.is_empty() {
        return html.to_vec();
    }
    let injection = tags.join("
");
    let lower = text.to_lowercase();
    let patched = if let Some(pos) = lower.find("</head>") {
        let cut = pos + "</head>".len().min(text.len());
        format!("{}
{}{}", &text[..cut.min(text.len())], injection, &text[cut.min(text.len())..])
    } else if let Some(pos) = lower.find("<body") {
        let insert_at = text[pos..].find('>').map(|o| pos + o + 1).unwrap_or(pos);
        format!("{}{}
{}", &text[..insert_at], injection, &text[insert_at..])
    } else {
        format!("{}
{}", injection, text)
    };
    patched.into_bytes()
}

/// Final HTML for the MDX entry preview: follows `@@@LINK=word` redirect
/// chains the way reader apps do (each hop overlay-aware), injects companion
/// CSS, and when a redirect was actually followed prepends a small badge
/// naming the hop path so the previewed content is not mistaken for the
/// entry's own body. Dead targets and cycles stop the walk and fall back to
/// the last resolved entry's raw text.
fn preview_entry_html(
    pool: &SourcePool,
    overlay: &Overlay,
    registry: &Registry,
    ctx: &ResourceId,
    mut bytes: Vec<u8>,
) -> Vec<u8> {
    const MAX_HOPS: usize = 16;
    let mut hops: Vec<String> = Vec::new();
    let mut visited: Vec<ResourceId> = Vec::new();
    let mut current = ctx.clone();
    loop {
        if visited.len() >= MAX_HOPS || visited.contains(&current) {
            break;
        }
        let Some(word) = resolver::link_redirect_target(&bytes) else {
            break;
        };
        visited.push(current.clone());
        let Some(next) = resolver::find_entry(pool, &word) else {
            break;
        };
        if overlay.get(&next).is_some_and(|r| r.deleted) {
            break;
        }
        match state::current_bytes(pool, overlay, &next) {
            Ok(b) => bytes = b,
            Err(_) => break,
        }
        hops.push(word);
        current = next;
    }
    let bytes = inject_preview_css(pool, overlay, registry, &bytes);
    if hops.is_empty() {
        return bytes;
    }
    let start = pool.key_of(ctx).unwrap_or_default();
    let path = std::iter::once(start)
        .chain(hops)
        .collect::<Vec<_>>()
        .join(" → ");
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return bytes;
    };
    let badge = format!(
        "<div style=\"position:absolute;top:6px;right:8px;z-index:2147483647;\
pointer-events:none;font:11px/1.6 sans-serif;color:#666;\
background:rgba(127,127,127,0.08);border:1px solid rgba(127,127,127,0.3);\
border-radius:4px;padding:1px 8px;\">@@@LINK 跳转: {path}</div>"
    );
    inject_after_body_open(&text, &badge).into_bytes()
}

/// Inserts `fragment` right after the opening `<body>` tag (or prepends it
/// when the document has no body tag).
fn inject_after_body_open(text: &str, fragment: &str) -> String {
    let lower = text.to_lowercase();
    if let Some(pos) = lower.find("<body") {
        let insert_at = text[pos..]
            .find('>')
            .map(|o| pos + o + 1)
            .unwrap_or(pos);
        format!("{}{}{}", &text[..insert_at], fragment, &text[insert_at..])
    } else {
        format!("{fragment}{text}")
    }
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
    // Root-absolute references (`src="/arts/x.png"`) bypass the iframe's
    // /preview/<token>/ base entirely — resolve them against the root.
    let Some(rest) = path.strip_prefix("preview/") else {
        return serve_resolved(&state, path, None);
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

    // Empty rel = the context resource itself (MDX entry HTML preview),
    // with companion CSS auto-injected so the preview looks like a reader.
    // All locks are scoped and dropped before serve_resolved takes its own.
    {
        let pool = match state.pool.read() {
            Ok(p) => p,
            Err(_) => return not_found("state poisoned"),
        };
        let overlay = match state.overlay.read() {
            Ok(o) => o,
            Err(_) => return not_found("state poisoned"),
        };
        {
            let mut registry = match state.registry.write() {
                Ok(r) => r,
                Err(_) => return not_found("state poisoned"),
            };
            if registry::ensure_built(&pool, &mut registry).is_err() {
                return not_found("registry build failed");
            }
        }
        let registry = match state.registry.read() {
            Ok(r) => r,
            Err(_) => return not_found("state poisoned"),
        };
        if rel.is_empty() {
            let bytes = match state::current_bytes(&pool, &overlay, &ctx) {
                Ok(b) => b,
                Err(e) => return not_found(&e),
            };
            if matches!(ctx, ResourceId::Mdx { .. }) {
                let bytes = preview_entry_html(&pool, &overlay, &registry, &ctx, bytes);
                return ok("text/html; charset=utf-8", bytes);
            }
            return ok("text/html; charset=utf-8", bytes);
        }
    }

    serve_resolved(&state, rel, Some(&ctx))
}

/// Shared resolution for preview and root-absolute requests: ambiguous keys
/// serve the first (external-first) hit; resources marked deleted 404; the
/// overlay's current bytes win for edited resources.
fn serve_resolved(state: &AppState, rel: &str, ctx: Option<&ResourceId>) -> tauri::http::Response<Vec<u8>> {
    let not_found = |msg: &str| {
        tauri::http::Response::builder()
            .status(404)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(msg.as_bytes().to_vec())
            .unwrap()
    };
    let pool = match state.pool.read() {
        Ok(p) => p,
        Err(_) => return not_found("state poisoned"),
    };
    let overlay = match state.overlay.read() {
        Ok(o) => o,
        Err(_) => return not_found("state poisoned"),
    };
    let registry = match state.registry.read() {
        Ok(r) => r,
        Err(_) => return not_found("state poisoned"),
    };
    // AALookup parity: a safe file beside the source dictionary wins over
    // same-named MDD resources.
    if let Some(bytes) = assets::loose_file_lookup(&pool, &overlay, ctx, rel) {
        let mime = std::path::Path::new(rel)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| crate::category::Category::mime_for(&format!("x.{e}")))
            .unwrap_or("application/octet-stream");
        return tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", "no-store")
            .body(bytes)
            .unwrap();
    }
    let resolved = match resolver::resolve_path(&pool, &registry, rel, ctx) {
        Ok(resolver::ResolveOutcome::Found { target, .. }) => Some(target),
        Ok(resolver::ResolveOutcome::Ambiguous { candidates, .. }) => {
            candidates.into_iter().next()
        }
        _ => None,
    };
    let Some(target) = resolved else {
        return not_found("resource not found");
    };
    if overlay.get(&target).is_some_and(|r| r.deleted) {
        return not_found("resource deleted");
    }
    match state::current_bytes(&pool, &overlay, &target) {
        Ok(bytes) => {
            let key = pool.key_of(&target).unwrap_or_default();
            tauri::http::Response::builder()
                .status(200)
                .header("Content-Type", crate::category::Category::mime_for(&key))
                .header("Cache-Control", "no-store")
                .body(bytes)
                .unwrap()
        }
        Err(e) => not_found(&e),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceProps {
    id: u32,
    kind: &'static str,
    name: String,
    /// Filesystem path of the backing file.
    path: String,
    /// File size in bytes (0 when the file vanished).
    file_size: u64,
    title: Option<String>,
    entry_count: u64,
    /// Raw header attributes (encoding, encrypted, GeneratedByEngineVersion…)
    /// for dictionary files; empty for externals.
    attributes: Vec<(String, String)>,
}

/// Full property sheet of one source (right-click → properties).
#[tauri::command]
fn source_props(id: u32, state: tauri::State<AppState>) -> Result<SourceProps, String> {
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let entry = pool.sources.get(id as usize).ok_or("source not loaded")?;
    let attributes = match &entry.source {
        crate::state::Source::Mdx(file) => file
            .header()
            .attributes()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        crate::state::Source::Mdd(file) => file
            .header()
            .attributes()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        crate::state::Source::External { .. } => Vec::new(),
    };
    Ok(SourceProps {
        id,
        kind: entry.kind(),
        name: entry.name.clone(),
        path: entry.path.display().to_string(),
        file_size: std::fs::metadata(&entry.path).map(|m| m.len()).unwrap_or(0),
        title: entry.title.clone(),
        entry_count: entry.entry_count(),
        attributes,
    })
}

/// Active sources (tombstoned excluded) — lets the frontend rehydrate its
/// list after an HMR refresh while the backend keeps its state.
#[tauri::command]
fn list_sources(state: tauri::State<AppState>) -> Result<Vec<SourceInfo>, String> {
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    Ok(pool
        .sources
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.tombstone)
        .map(|(id, e)| SourceInfo {
            id: id as u32,
            kind: e.kind(),
            name: e.name.clone(),
            title: e.title.clone(),
            entry_count: e.entry_count(),
            dir: e.path.parent().map(|p| p.display().to_string()),
        })
        .collect())
}

/// Starts a dry-run job; progress/done arrive via events. Returns job id.
#[tauri::command]
fn pipeline_dry_run_start(
    steps: Vec<processors::Processor>,
    scope: pipeline::Scope,
    app: AppHandle,
) -> String {
    let needs_corpus = steps
        .iter()
        .any(|s| matches!(s, processors::Processor::CssPurge));
    let selectors = if needs_corpus {
corpus_for(&app)
    } else {
        Default::default()
    };
    start_job_with_selectors(&app, "pipeline-dry", selectors, move |st, ctl| {
        // Snapshot phase (short-lived locks): build indices + select targets,
        // then drop the registry lock before the long walk so the preview
        // protocol and UI reads never wait on us.
        let metas = {
            let pool = st.pool.read().map_err(|e| e.to_string())?;
            let overlay = st.overlay.read().map_err(|e| e.to_string())?;
            let mut registry = st.registry.write().map_err(|e| e.to_string())?;
            registry::ensure_built(&pool, &mut registry)?;
            pipeline::select_targets(&pool, &overlay, &registry, &scope)?
        };
        let pool = st.pool.read().map_err(|e| e.to_string())?;
        let overlay = st.overlay.read().map_err(|e| e.to_string())?;
        pipeline::dry_run_selected(&pool, &overlay, &steps, metas, ctl)
    })
}

/// Starts a pipeline apply job; writes go through the overlay.
#[tauri::command]
fn pipeline_apply_start(
    steps: Vec<processors::Processor>,
    scope: pipeline::Scope,
    app: AppHandle,
) -> String {
    let needs_corpus = steps
        .iter()
        .any(|s| matches!(s, processors::Processor::CssPurge));
    let selectors = if needs_corpus {
corpus_for(&app)
    } else {
        Default::default()
    };
    start_job_with_selectors(&app, "pipeline-apply", selectors, move |st, ctl| {
        let metas = {
            let pool = st.pool.read().map_err(|e| e.to_string())?;
            let overlay = st.overlay.read().map_err(|e| e.to_string())?;
            let mut registry = st.registry.write().map_err(|e| e.to_string())?;
            registry::ensure_built(&pool, &mut registry)?;
            pipeline::select_targets(&pool, &overlay, &registry, &scope)?
        };
        let (reports, applied) = {
            let pool = st.pool.read().map_err(|e| e.to_string())?;
            let overlay = st.overlay.read().map_err(|e| e.to_string())?;
            pipeline::apply_selected(&pool, &overlay, &steps, metas, ctl)?
        };
        {
            let pool = st.pool.read().map_err(|e| e.to_string())?;
            let mut overlay = st.overlay.write().map_err(|e| e.to_string())?;
            pipeline::commit_applied(&pool, &mut overlay, applied)?;
        }
        Ok(reports)
    })
}

/// Starts an export job; outputs land in `config.out_dir`.
#[tauri::command]
fn export_start(config: export::ExportConfig, app: AppHandle) -> String {
    let needs_corpus = config
        .lossy
        .as_ref()
        .is_some_and(|steps| steps.iter().any(|s| matches!(s, processors::Processor::CssPurge)));
    let selectors = if needs_corpus {
corpus_for(&app)
    } else {
        Default::default()
    };
    start_job_with_selectors(&app, "export", selectors, move |st, ctl| {
        let pool = st.pool.read().map_err(|e| e.to_string())?;
        let overlay = st.overlay.read().map_err(|e| e.to_string())?;
        export::export_build(&pool, &overlay, &config, ctl)
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InsertionInfo {
    index: usize,
    /// "entry" | "resource"
    kind: &'static str,
    name: String,
    target: u32,
    size: u64,
}

use crate::state::InsertKind;

fn insertion_kind_str(kind: InsertKind) -> &'static str {
    match kind {
        InsertKind::Entry => "entry",
        InsertKind::Resource => "resource",
    }
}

/// Inserts a new dictionary entry into an MDX source (materialized at export
/// via `EditSet::insert`). Rejects empty keys and duplicates against both
/// existing entries and pending insertions.
#[tauri::command]
fn insert_entry(
    source: u32,
    key: String,
    html: String,
    state: tauri::State<AppState>,
) -> Result<usize, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("词条词头不能为空".into());
    }
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    let entry = pool.get(&ResourceId::Mdx {
        source,
        ordinal: 0,
    })?;
    if !matches!(entry.source, crate::state::Source::Mdx(_)) {
        return Err(format!("{} 不是 MDX 源", entry.name));
    }
    registry::ensure_built(&pool, &mut registry)?;
    let lower = key.to_lowercase();
    if let Some(index) = registry.indices.get(source as usize).and_then(|o| o.as_ref()) {
        if index.rows.iter().any(|(k, _)| k == &lower) {
            return Err(format!("词条 {key:?} 已存在于 {}", entry.name));
        }
    }
    if overlay
        .insertions
        .iter()
        .any(|i| i.target == source && i.kind == InsertKind::Entry && i.name.to_lowercase() == lower)
    {
        return Err(format!("待插入列表中已有词条 {key:?}"));
    }
    overlay.insertions.push(crate::state::Insertion {
        target: source,
        kind: InsertKind::Entry,
        name: key,
        bytes: html.into_bytes(),
    });
    Ok(overlay.insertions.len() - 1)
}

/// Inserts resource files into an MDD source. `paths` are read server-side;
/// names come from the file names (leading separators stripped). Duplicate
/// keys are rejected; per-file errors do not abort the batch.
#[tauri::command]
fn insert_resources(
    source: u32,
    paths: Vec<String>,
    #[allow(unused_variables)] path_prefix: Option<String>,
    state: tauri::State<AppState>,
) -> Result<(usize, Vec<String>), String> {
    let pool = state.pool.read().map_err(|e| e.to_string())?;
    let mut registry = state.registry.write().map_err(|e| e.to_string())?;
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    let entry = pool.get(&ResourceId::Mdd {
        source,
        ordinal: 0,
    })?;
    if !matches!(entry.source, crate::state::Source::Mdd(_)) {
        return Err(format!("{} 不是 MDD 源", entry.name));
    }
    registry::ensure_built(&pool, &mut registry)?;
    let mut added = 0usize;
    let mut errors = Vec::new();
    for path in paths {
        let p = std::path::Path::new(&path);
        let name = match p.file_name().map(|n| n.to_string_lossy().into_owned()) {
            Some(file_name) => match path_prefix.as_deref().unwrap_or("").trim() {
                "" => file_name,
                prefix => format!("{}/{}", prefix.trim_matches('/'), file_name),
            },
            None => {
                errors.push(format!("invalid path: {path}"));
                continue;
            }
        };
        let normalized = registry::normalize_key(&name);
        let dup_source = registry
            .indices
            .get(source as usize)
            .and_then(|o| o.as_ref())
            .map(|index| index.rows.iter().any(|(k, _)| k == &normalized))
            .unwrap_or(false);
        let dup_pending = overlay.insertions.iter().any(|i| {
            i.target == source
                && i.kind == InsertKind::Resource
                && registry::normalize_key(&i.name) == normalized
        });
        if dup_source || dup_pending {
            errors.push(format!("{name}: 键已存在，跳过"));
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                overlay.insertions.push(crate::state::Insertion {
                    target: source,
                    kind: InsertKind::Resource,
                    name,
                    bytes,
                });
                added += 1;
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    Ok((added, errors))
}

/// Lists pending insertions (optionally filtered by target source).
#[tauri::command]
fn list_insertions(
    source: Option<u32>,
    state: tauri::State<AppState>,
) -> Result<Vec<InsertionInfo>, String> {
    let overlay = state.overlay.read().map_err(|e| e.to_string())?;
    Ok(overlay
        .insertions
        .iter()
        .filter(|i| source.is_none_or(|s| i.target == s))
        .enumerate()
        .map(|(index, i)| InsertionInfo {
            index,
            kind: insertion_kind_str(i.kind),
            name: i.name.clone(),
            target: i.target,
            size: i.bytes.len() as u64,
        })
        .collect())
}

/// Updates an insertion's body (index-based; removing shifts ids, the UI
/// refreshes the list right after any mutation). Text-content insertions of
/// both kinds (entry HTML, text resources) are editable.
#[tauri::command]
fn update_insertion(
    index: usize,
    html: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    let ins = overlay
        .insertions
        .get_mut(index)
        .ok_or_else(|| format!("插入项 {index} 不存在"))?;
    ins.bytes = html.into_bytes();
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InsertionContent {
    index: usize,
    kind: &'static str,
    name: String,
    /// UTF-8 text for text categories (entry/html/css/js/text); None for
    /// binary resources, which the UI does not edit inline.
    text: Option<String>,
}

/// Returns one pending insertion's content for inline editing.
#[tauri::command]
fn read_insertion(
    index: usize,
    state: tauri::State<AppState>,
) -> Result<InsertionContent, String> {
    let overlay = state.overlay.read().map_err(|e| e.to_string())?;
    let ins = overlay
        .insertions
        .get(index)
        .ok_or_else(|| format!("插入项 {index} 不存在"))?;
    let category = match ins.kind {
        InsertKind::Entry => crate::category::Category::Entry,
        InsertKind::Resource => crate::category::Category::from_key(&ins.name),
    };
    let text = category
        .is_text()
        .then(|| String::from_utf8_lossy(&ins.bytes).into_owned());
    Ok(InsertionContent {
        index,
        kind: insertion_kind_str(ins.kind),
        name: ins.name.clone(),
        text,
    })
}

/// Removes a pending insertion (swap-remove; UI refreshes indices after).
#[tauri::command]
fn remove_insertion(index: usize, state: tauri::State<AppState>) -> Result<bool, String> {
    let mut overlay = state.overlay.write().map_err(|e| e.to_string())?;
    if index >= overlay.insertions.len() {
        return Err(format!("插入项 {index} 不存在"));
    }
    overlay.insertions.remove(index);
    Ok(true)
}

/// Hands exported dictionary files to AALookup — the reader app this editor
/// shares its parsing core (mdictlib) and resource-resolution order with.
/// AALookup imports and enables .mdx files handed to it on the command line
/// (its single-instance plugin forwards to a running copy).
#[tauri::command]
fn import_to_aalookup(paths: Vec<String>) -> Result<String, String> {
    if paths.is_empty() {
        return Err("没有可导入的 .mdx 文件（先完成一次导出）".into());
    }
    #[cfg(target_os = "windows")]
    {
        let exe = find_aalookup_exe_windows()
            .ok_or("未找到 AALookup：请先安装 AALookup，或将 .mdx 关联到 AALookup")?;
        std::process::Command::new(&exe)
            .args(&paths)
            .spawn()
            .map_err(|e| format!("启动 AALookup 失败: {e}"))?;
        Ok(format!("已交给 AALookup 导入 {} 个词典", paths.len()))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = paths;
        Err("一键导入当前仅支持 Windows；请在 AALookup 中手动打开导出的 .mdx".into())
    }
}

#[cfg(target_os = "windows")]
fn find_aalookup_exe_windows() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let reg_query = |key: &str| -> Option<String> {
        let out = std::process::Command::new("reg")
            .args(["query", key, "/ve"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines().find_map(|l| {
            let l = l.trim_end();
            let idx = l.find("REG_SZ")?;
            let v = l[idx + 6..].trim();
            (!v.is_empty()).then(|| v.to_string())
        })
    };

    // 1) .mdx shell association, accepted only when it is AALookup itself.
    if let Some(progid) = reg_query("HKCR\\.mdx") {
        if let Some(cmdline) = reg_query(&format!("HKCR\\{progid}\\shell\\open\\command")) {
            let candidate = if let Some(stripped) = cmdline.strip_prefix('"') {
                stripped.split('"').next().unwrap_or("").to_string()
            } else if let Some(pos) = cmdline.to_lowercase().find(".exe") {
                cmdline[..pos + 4].to_string()
            } else {
                cmdline.clone()
            };
            let path = PathBuf::from(&candidate);
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.to_lowercase().contains("aalookup"))
                && path.exists()
            {
                return Some(path);
            }
        }
    }

    // 2) Common install locations.
    let locals = std::env::var("LOCALAPPDATA").ok()?;
    for p in [
        PathBuf::from(&locals).join("Programs").join("AALookup").join("AALookup.exe"),
        PathBuf::from(&locals).join("AALookup").join("AALookup.exe"),
        PathBuf::from(r"C:\Program Files").join("AALookup").join("AALookup.exe"),
    ] {
        if p.exists() {
            return Some(p);
        }
    }
    None
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
        .manage(Jobs::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            open_sources,
            create_source,
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
            list_sources,
            source_props,
            pipeline_dry_run_start,
            pipeline_apply_start,
            export_start,
            cancel_job,
            import_to_aalookup,
            insert_entry,
            insert_resources,
            list_insertions,
            update_insertion,
            read_insertion,
            remove_insertion
        ])
        .register_uri_scheme_protocol("mdres", |ctx, request| {
            use tauri::Manager;
            match ctx.app_handle().try_state::<AppState>() {
                Some(state) => handle_mdres_request(&state, request.uri().path()),
                None => tauri::http::Response::builder()
                    .status(503)
                    .header("Content-Type", "text/plain; charset=utf-8")
                    .body("state not ready".as_bytes().to_vec())
                    .unwrap(),
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
