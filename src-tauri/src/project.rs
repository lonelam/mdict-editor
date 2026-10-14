//! Durable editing projects. Sources stay on disk; only the overlay and its
//! undo history are stored. Loading validates everything before replacing state.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::Path,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    registry::{self, Registry},
    state::{AppState, InsertKind, Insertion, Overlay, ResourceId, Revision, SourcePool},
};

const FORMAT: &str = "mdict-editor-project";
const VERSION: u32 = 1;

/// Streaming digest: a large MDD never needs a second full in-memory copy.
pub fn fingerprint(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let len = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    format: String,
    version: u32,
    sources: Vec<SavedSource>,
    revisions: Vec<SavedRevision>,
    insertions: Vec<SavedInsertion>,
}

#[derive(Serialize, Deserialize)]
struct SavedSource {
    path: String,
    fingerprint: String,
}

#[derive(Serialize, Deserialize)]
struct SavedRevision {
    id: ResourceId,
    original: String,
    current: String,
    history: Vec<String>,
    deleted: bool,
}

#[derive(Serialize, Deserialize)]
struct SavedInsertion {
    target: u32,
    kind: InsertKind,
    name: String,
    bytes: String,
}

fn remap(id: &ResourceId, sources: &HashMap<u32, u32>) -> Option<ResourceId> {
    let idx = *sources.get(&id.source_index())?;
    Some(match id {
        ResourceId::Mdx { ordinal, .. } => ResourceId::Mdx {
            source: idx,
            ordinal: *ordinal,
        },
        ResourceId::Mdd { ordinal, .. } => ResourceId::Mdd {
            source: idx,
            ordinal: *ordinal,
        },
        ResourceId::Ext { .. } => ResourceId::Ext { file: idx },
    })
}

/// Writes a complete snapshot via a temporary file in the same directory.
/// A failed write leaves the previous project intact.
pub fn save(path: &Path, pool: &SourcePool, overlay: &Overlay) -> Result<(), String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    let destination = absolute.canonicalize().unwrap_or_else(|_| absolute.clone());
    if pool.sources.iter().any(|s| s.path == destination) {
        return Err("项目文件不能覆盖源词典或资源文件".into());
    }
    let mut sources = Vec::new();
    let mut indices = HashMap::new();
    for (old, source) in pool
        .sources
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.tombstone)
    {
        indices.insert(old as u32, sources.len() as u32);
        sources.push(SavedSource {
            path: source.path.display().to_string(),
            fingerprint: source.fingerprint.clone(),
        });
    }
    let mut revisions: Vec<_> = overlay
        .revisions
        .iter()
        .filter_map(|(id, rev)| {
            Some(SavedRevision {
                id: remap(id, &indices)?,
                original: STANDARD.encode(&rev.original),
                current: STANDARD.encode(&rev.current),
                history: rev.history.iter().map(|h| STANDARD.encode(h)).collect(),
                deleted: rev.deleted,
            })
        })
        .collect();
    // Stable serialization makes snapshots easy to compare.
    revisions.sort_by_key(|r| {
        (
            r.id.source_index(),
            match r.id {
                ResourceId::Mdx { ordinal, .. } | ResourceId::Mdd { ordinal, .. } => ordinal,
                ResourceId::Ext { .. } => 0,
            },
        )
    });
    let project = Project {
        format: FORMAT.into(),
        version: VERSION,
        sources,
        revisions,
        insertions: overlay
            .insertions
            .iter()
            .filter_map(|i| {
                Some(SavedInsertion {
                    target: *indices.get(&i.target)?,
                    kind: i.kind,
                    name: i.name.clone(),
                    bytes: STANDARD.encode(&i.bytes),
                })
            })
            .collect(),
    };
    let parent = absolute.parent().ok_or("项目路径无效")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut temporary, &project).map_err(|e| e.to_string())?;
    temporary.write_all(b"\n").map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary
        .persist(&absolute)
        .map_err(|e| format!("保存项目失败: {e}"))?;
    Ok(())
}

fn decode(text: &str) -> Result<Vec<u8>, String> {
    STANDARD
        .decode(text)
        .map_err(|e| format!("项目内容损坏: {e}"))
}

/// Reopens all sources and validates content identities, resource ids and
/// base64 data before committing any change to the current session.
pub fn load(path: &Path, state: &AppState) -> Result<(), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let project: Project = serde_json::from_reader(std::io::BufReader::new(file))
        .map_err(|e| format!("项目格式无效: {e}"))?;
    if project.format != FORMAT || project.version != VERSION {
        return Err(format!("不支持的项目格式或版本: {}", project.version));
    }
    let mut pool = SourcePool::default();
    for source in project.sources {
        let entry = SourcePool::open_path(&source.path)?;
        if entry.fingerprint != source.fingerprint {
            return Err(format!("源文件已变化，无法恢复项目: {}", source.path));
        }
        if pool.has_active_path(&entry.path) {
            return Err(format!("项目包含重复源文件: {}", source.path));
        }
        pool.sources.push(entry);
    }
    let mut overlay = Overlay::default();
    for revision in project.revisions {
        let original = decode(&revision.original)?;
        if original != pool.read_original(&revision.id)? {
            return Err("项目修订与源文件内容不匹配".into());
        }
        if revision.history.len() > 32 || overlay.revisions.contains_key(&revision.id) {
            return Err("项目修订历史或资源编号无效".into());
        }
        overlay.revisions.insert(
            revision.id,
            Revision {
                original,
                current: decode(&revision.current)?,
                deleted: revision.deleted,
                history: revision
                    .history
                    .iter()
                    .map(|h| decode(h))
                    .collect::<Result<_, _>>()?,
            },
        );
    }
    for insertion in project.insertions {
        let source = pool
            .sources
            .get(insertion.target as usize)
            .ok_or("插入项的源编号无效")?;
        let expected = match insertion.kind {
            InsertKind::Entry => "mdx",
            InsertKind::Resource => "mdd",
        };
        if source.kind() != expected || insertion.name.trim().is_empty() {
            return Err("项目插入项无效".into());
        }
        overlay.insertions.push(Insertion {
            target: insertion.target,
            kind: insertion.kind,
            name: insertion.name,
            bytes: decode(&insertion.bytes)?,
        });
    }
    let mut registry = Registry::default();
    registry::ensure_built(&pool, &mut registry)?;
    // Lock order matches the command and preview paths.
    let mut current_pool = state.pool.write().map_err(|e| e.to_string())?;
    let mut current_overlay = state.overlay.write().map_err(|e| e.to_string())?;
    let mut current_registry = state.registry.write().map_err(|e| e.to_string())?;
    *current_pool = pool;
    *current_overlay = overlay;
    *current_registry = registry;
    Ok(())
}
