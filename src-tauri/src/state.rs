//! App state: the pool of loaded sources plus the overlay of edits.
//!
//! Everything here is plain library code (no `tauri` types) so it can be
//! exercised by integration tests directly.

use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    path::PathBuf,
    sync::RwLock,
};

use mdictlib::{MddFile, MdxFile};
use serde::{Deserialize, Serialize};

use crate::registry::Registry;

/// Uniquely locates one editable/viewable resource across all sources.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum ResourceId {
    /// An MDX term entry.
    Mdx { source: u32, ordinal: u64 },
    /// A resource stored inside an MDD.
    Mdd { source: u32, ordinal: u64 },
    /// A file loaded from disk (js/css/...).
    Ext { file: u32 },
}

impl ResourceId {
    pub fn source_index(&self) -> u32 {
        match self {
            ResourceId::Mdx { source, .. } | ResourceId::Mdd { source, .. } => *source,
            ResourceId::Ext { file } => *file,
        }
    }
}

pub enum Source {
    Mdx(MdxFile),
    Mdd(MddFile),
    External { path: PathBuf, bytes: Vec<u8> },
}

pub struct SourceEntry {
    pub name: String,
    pub title: Option<String>,
    /// Canonical filesystem path; identical re-opens are deduped on it.
    pub path: PathBuf,
    /// Tombstone: removed sources keep their slot (ids stay stable) but are
    /// skipped by every iteration and refused by [`SourcePool::get`].
    pub tombstone: bool,
    pub source: Source,
}

impl SourceEntry {
    pub fn kind(&self) -> &'static str {
        match self.source {
            Source::Mdx(_) => "mdx",
            Source::Mdd(_) => "mdd",
            Source::External { .. } => "ext",
        }
    }

    pub fn entry_count(&self) -> u64 {
        match &self.source {
            Source::Mdx(f) => f.len(),
            Source::Mdd(f) => f.len(),
            Source::External { .. } => 1,
        }
    }
}

/// All opened sources; index in this Vec is the `source`/`file` field of ids.
#[derive(Default)]
pub struct SourcePool {
    pub sources: Vec<SourceEntry>,
}

impl SourcePool {
    /// Opens one path as a source: .mdx / .mdd via mdictlib, anything else
    /// (js/css/…) as an external file held in memory. `path` is canonicalized
    /// so re-opens of the same file can be deduped.
    pub fn open_path(path: &str) -> Result<SourceEntry, String> {
        let p = std::path::Path::new(path);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("invalid path: {path}"))?;
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        match ext.as_str() {
            "mdx" => {
                let file = MdxFile::open(path).map_err(|e| format!("{name}: {e}"))?;
                let title = file.header().title().map(str::to_string);
                Ok(SourceEntry {
                    name,
                    title,
                    path: canonical,
                    tombstone: false,
                    source: Source::Mdx(file),
                })
            }
            "mdd" => {
                let file = MddFile::open(path).map_err(|e| format!("{name}: {e}"))?;
                Ok(SourceEntry {
                    name,
                    title: None,
                    path: canonical,
                    tombstone: false,
                    source: Source::Mdd(file),
                })
            }
            other => {
                if other.is_empty() {
                    return Err(format!("{name}: file has no extension"));
                }
                let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
                Ok(SourceEntry {
                    name,
                    title: None,
                    path: canonical,
                    tombstone: false,
                    source: Source::External {
                        path: p.to_path_buf(),
                        bytes,
                    },
                })
            }
        }
    }

    pub fn has_active_path(&self, canonical: &Path) -> bool {
        self.sources
            .iter()
            .any(|e| !e.tombstone && e.path == canonical)
    }

    /// Tombstones a source: the slot (and therefore every ResourceId that
    /// points into it) stays stable, but the source disappears from all
    /// listings, lookups and exports.
    pub fn remove(&mut self, idx: usize) -> Result<(), String> {
        let entry = self
            .sources
            .get_mut(idx)
            .ok_or_else(|| format!("source index {idx} not loaded"))?;
        entry.tombstone = true;
        Ok(())
    }

    pub fn get(&self, id: &ResourceId) -> Result<&SourceEntry, String> {
        let idx = id.source_index() as usize;
        let entry = self
            .sources
            .get(idx)
            .ok_or_else(|| format!("source index {idx} not loaded"))?;
        if entry.tombstone {
            return Err(format!("source index {idx} has been removed"));
        }
        Ok(entry)
    }

    /// Original bytes of a resource, ignoring the overlay.
    pub fn read_original(&self, id: &ResourceId) -> Result<Vec<u8>, String> {
        let entry = self.get(id)?;
        match (&entry.source, id) {
            (Source::Mdx(file), ResourceId::Mdx { ordinal, .. }) => {
                let entry = file
                    .entry_at(mdictlib::KeyOrdinal::new(*ordinal))
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("entry ordinal {ordinal} missing"))?;
                Ok(entry.text().as_bytes().to_vec())
            }
            (Source::Mdd(file), ResourceId::Mdd { ordinal, .. }) => {
                let resource = file
                    .resource_at(mdictlib::KeyOrdinal::new(*ordinal))
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("resource ordinal {ordinal} missing"))?;
                Ok(resource.bytes().to_vec())
            }
            (Source::External { bytes, .. }, ResourceId::Ext { .. }) => Ok(bytes.clone()),
            _ => Err("resource id does not match its source kind".into()),
        }
    }

    /// The storage key of a resource: entry head for MDX, resource path for
    /// MDD, file name for external files.
    pub fn key_of(&self, id: &ResourceId) -> Result<String, String> {
        let entry = self.get(id)?;
        match (&entry.source, id) {
            (Source::Mdx(file), ResourceId::Mdx { ordinal, .. }) => {
                let key = file
                    .key_at(mdictlib::KeyOrdinal::new(*ordinal))
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("entry ordinal {ordinal} missing"))?;
                Ok(key.key().to_string())
            }
            (Source::Mdd(file), ResourceId::Mdd { ordinal, .. }) => {
                let key = file
                    .key_at(mdictlib::KeyOrdinal::new(*ordinal))
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("resource ordinal {ordinal} missing"))?;
                Ok(key.key().to_string())
            }
            (Source::External { path, .. }, ResourceId::Ext { .. }) => Ok(path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "external".to_string())),
            _ => Err("resource id does not match its source kind".into()),
        }
    }
}

/// One saved edit of a resource, with its full in-memory history.
pub struct Revision {
    pub original: Vec<u8>,
    pub history: VecDeque<Vec<u8>>,
    pub current: Vec<u8>,
    pub deleted: bool,
}

/// Maximum in-memory versions per resource (oldest dropped).
const HISTORY_CAP: usize = 32;

/// A pending insertion: a new entry (MDX) or resource (MDD) materialized at
/// export time (`EditSet::insert` / `MddBuilder::add_resource`).
#[derive(Clone)]
pub struct Insertion {
    /// Source index the insertion targets (must be mdx for Entry, mdd for
    /// Resource).
    pub target: u32,
    pub kind: InsertKind,
    /// Entry head / resource path.
    pub name: String,
    /// Entry body (UTF-8 HTML) / resource bytes.
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InsertKind {
    Entry,
    Resource,
}

/// The overlay: every edit lands here; source files are never touched.
#[derive(Default)]
pub struct Overlay {
    pub revisions: HashMap<ResourceId, Revision>,
    pub insertions: Vec<Insertion>,
}

impl Overlay {
    pub fn get(&self, id: &ResourceId) -> Option<&Revision> {
        self.revisions.get(id)
    }

    /// Records `current` as a new version, pushing the previous one into
    /// history. The first edit snapshots the original from the source pool.
    pub fn write(
        &mut self,
        id: ResourceId,
        original: Vec<u8>,
        current: Vec<u8>,
    ) -> (usize, usize) {
        let rev = self.revisions.entry(id).or_insert_with(|| Revision {
            history: VecDeque::new(),
            original: original.clone(),
            current: original,
            deleted: false,
        });
        if rev.history.len() >= HISTORY_CAP {
            rev.history.pop_front();
        }
        rev.history.push_back(std::mem::take(&mut rev.current));
        rev.current = current;
        rev.deleted = false;
        (rev.history.len(), rev.original.len())
    }

    /// Pops the latest history entry back into `current`; false if empty.
    pub fn undo(&mut self, id: &ResourceId) -> bool {
        let Some(rev) = self.revisions.get_mut(id) else {
            return false;
        };
        match rev.history.pop_back() {
            Some(prev) => {
                rev.current = prev;
                true
            }
            None => false,
        }
    }

    /// Marks a resource deleted (content kept for restore).
    pub fn delete(&mut self, id: ResourceId, original: Vec<u8>) {
        let rev = self.revisions.entry(id).or_insert_with(|| Revision {
            history: VecDeque::new(),
            original: original.clone(),
            current: original,
            deleted: false,
        });
        rev.deleted = true;
    }

    /// Clears the whole edit chain; the resource returns to source content.
    pub fn revert(&mut self, id: &ResourceId) -> bool {
        self.revisions.remove(id).is_some()
    }
}

/// Overlay-aware byte read: current revision if edited, source otherwise.
pub fn current_bytes(
    pool: &SourcePool,
    overlay: &Overlay,
    id: &ResourceId,
) -> Result<Vec<u8>, String> {
    match overlay.get(id) {
        Some(rev) => Ok(rev.current.clone()),
        None => pool.read_original(id),
    }
}

/// Renames an MDX entry head. MDict has no in-place rename, so the overlay
/// records it as delete-plus-insert: the old ordinal is marked deleted and a
/// pending entry insertion carries the entry's current (overlay-aware) body
/// under the new key. Both halves materialize at export; nothing touches the
/// source file. Rejects empty keys and keys already taken by another entry
/// (case-insensitively; a case-only rename of the same entry is allowed) or
/// by a pending insertion.
pub fn rename_entry(
    pool: &SourcePool,
    overlay: &mut Overlay,
    id: &ResourceId,
    new_key: &str,
) -> Result<(), String> {
    let new_key = new_key.trim();
    if new_key.is_empty() {
        return Err("新词头不能为空".into());
    }
    let ResourceId::Mdx { source, ordinal } = *id else {
        return Err("仅 MDX 词条可重命名".into());
    };
    let entry = pool.get(id)?;
    let Source::Mdx(file) = &entry.source else {
        return Err(format!("{} 不是 MDX 源", entry.name));
    };
    if let Some(matches) = file.locate(new_key).map_err(|e| e.to_string())? {
        if matches.iter().any(|m| m.get() != ordinal) {
            return Err(format!("词头 {new_key:?} 已存在于 {}", entry.name));
        }
    }
    if overlay
        .insertions
        .iter()
        .any(|i| i.target == source && i.kind == InsertKind::Entry && i.name.to_lowercase() == new_key.to_lowercase())
    {
        return Err(format!("待插入列表中已有词头 {new_key:?}"));
    }
    let body = current_bytes(pool, overlay, id)?;
    let original = pool.read_original(id)?;
    overlay.delete(id.clone(), original);
    overlay.insertions.push(Insertion {
        target: source,
        kind: InsertKind::Entry,
        name: new_key.to_string(),
        bytes: body,
    });
    Ok(())
}

/// Global managed state. RwLock so long-running readers (export, pipeline,
/// preview) never block UI reads; writers stay mutually exclusive. Lock
/// order is always pool → overlay → registry.
#[derive(Default)]
pub struct AppState {
    pub pool: RwLock<SourcePool>,
    pub overlay: RwLock<Overlay>,
    pub registry: RwLock<Registry>,
}
