//! Persistent projects: round-trip edits, undo, renamed heads and binary data;
//! failed restores must leave the existing session untouched.
use mdict_editor_lib::{
    export::{export_build, ExportConfig},
    fixtures,
    pipeline::no_ctl,
    project,
    state::{self, AppState, InsertKind, Insertion, ResourceId, SourcePool},
};

fn setup() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let (mdx, mdd, css, js) = fixtures::write_all(dir.path());
    let state = AppState::default();
    for path in [mdx, mdd, css, js] {
        state
            .pool
            .write()
            .unwrap()
            .sources
            .push(SourcePool::open_path(&path).unwrap());
    }
    (dir, state)
}

fn save(path: &std::path::Path, state: &AppState) {
    project::save(
        path,
        &state.pool.read().unwrap(),
        &state.overlay.read().unwrap(),
    )
    .unwrap();
}

#[test]
fn project_roundtrip_preserves_edits_undo_rename_and_export() {
    let (dir, state) = setup();
    let path = dir.path().join("校对.mdict-project");
    let hello = ResourceId::Mdx {
        source: 0,
        ordinal: 2,
    };
    let banana = ResourceId::Mdx {
        source: 0,
        ordinal: 1,
    };
    let external = ResourceId::Ext { file: 2 };
    let binary = ResourceId::Mdd {
        source: 1,
        ordinal: 3,
    };
    let original_hash = project::fingerprint(&dir.path().join("ocean.mdx")).unwrap();
    {
        let pool = state.pool.read().unwrap();
        let mut overlay = state.overlay.write().unwrap();
        let original = pool.read_original(&hello).unwrap();
        overlay.write(hello.clone(), original.clone(), b"<p>first</p>".to_vec());
        overlay.write(hello.clone(), original, "<p>校对</p>".as_bytes().to_vec());
        overlay.delete(banana.clone(), pool.read_original(&banana).unwrap());
        overlay.write(
            external.clone(),
            pool.read_original(&external).unwrap(),
            b"p{color:red}".to_vec(),
        );
        overlay.write(
            binary.clone(),
            pool.read_original(&binary).unwrap(),
            vec![0, 255, 128, 1],
        );
        state::rename_entry(&pool, &mut overlay, &hello, "corrected").unwrap();
        overlay.insertions.push(Insertion {
            target: 0,
            kind: InsertKind::Entry,
            name: "alias".into(),
            bytes: b"@@@LINK=corrected".to_vec(),
        });
        overlay.insertions.push(Insertion {
            target: 1,
            kind: InsertKind::Resource,
            name: "new.bin".into(),
            bytes: vec![255, 0, 17],
        });
    }
    save(&path, &state);
    let restored = AppState::default();
    project::load(&path, &restored).unwrap();
    assert_eq!(restored.pool.read().unwrap().sources.len(), 4);
    {
        let overlay = restored.overlay.read().unwrap();
        assert_eq!(overlay.get(&hello).unwrap().history.len(), 2);
        assert!(overlay.get(&hello).unwrap().deleted);
        assert!(overlay.get(&banana).unwrap().deleted);
        assert_eq!(overlay.get(&binary).unwrap().current, [0, 255, 128, 1]);
        assert_eq!(overlay.get(&external).unwrap().current, b"p{color:red}");
        assert_eq!(overlay.insertions.len(), 3);
    }
    let out = tempfile::tempdir().unwrap();
    let report = export_build(
        &restored.pool.read().unwrap(),
        &restored.overlay.read().unwrap(),
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    assert!(report.ok);
    let mdx_path = report
        .files
        .iter()
        .find(|f| f.path.ends_with("ocean.mdx"))
        .unwrap();
    let mdx = mdictlib::MdxFile::open(&mdx_path.path).unwrap();
    assert_eq!(
        mdx.lookup("corrected").unwrap().unwrap().text(),
        "<p>校对</p>"
    );
    assert_eq!(
        mdx.lookup("alias").unwrap().unwrap().text(),
        "@@@LINK=corrected"
    );
    assert!(mdx.locate("banana").unwrap().is_none());
    let mdd_path = report
        .files
        .iter()
        .find(|f| f.path.ends_with("assets.mdd"))
        .unwrap();
    let mdd = mdictlib::MddFile::open(&mdd_path.path).unwrap();
    assert_eq!(
        mdd.lookup("new.bin").unwrap().unwrap().bytes(),
        [255, 0, 17]
    );
    assert!(restored.overlay.write().unwrap().undo(&hello));
    assert_eq!(
        restored
            .overlay
            .read()
            .unwrap()
            .get(&hello)
            .unwrap()
            .current,
        b"<p>first</p>"
    );
    assert_eq!(
        project::fingerprint(&dir.path().join("ocean.mdx")).unwrap(),
        original_hash
    );
}

#[test]
fn removed_sources_are_compacted_and_ids_remapped() {
    let (dir, state) = setup();
    let path = dir.path().join("test.mdict-project");
    state.pool.write().unwrap().remove(0).unwrap();
    let binary = ResourceId::Mdd {
        source: 1,
        ordinal: 3,
    };
    {
        let pool = state.pool.read().unwrap();
        let mut overlay = state.overlay.write().unwrap();
        overlay.write(
            binary,
            pool.read_original(&ResourceId::Mdd {
                source: 1,
                ordinal: 3,
            })
            .unwrap(),
            vec![255],
        );
        overlay.insertions.push(Insertion {
            target: 1,
            kind: InsertKind::Resource,
            name: "x.bin".into(),
            bytes: vec![1],
        });
        overlay.insertions.push(Insertion {
            target: 0,
            kind: InsertKind::Entry,
            name: "stale".into(),
            bytes: vec![],
        });
    }
    save(&path, &state);
    let restored = AppState::default();
    project::load(&path, &restored).unwrap();
    assert_eq!(restored.pool.read().unwrap().sources.len(), 3);
    let overlay = restored.overlay.read().unwrap();
    assert_eq!(
        overlay
            .get(&ResourceId::Mdd {
                source: 0,
                ordinal: 3
            })
            .unwrap()
            .current,
        [255]
    );
    assert_eq!(overlay.insertions.len(), 1);
    assert_eq!(overlay.insertions[0].target, 0);
}

#[test]
fn changed_missing_or_corrupt_projects_preserve_current_session() {
    let (dir, state) = setup();
    let path = dir.path().join("test.mdict-project");
    save(&path, &state);
    let original = std::fs::read(&path).unwrap();
    let current = AppState::default();
    project::load(&path, &current).unwrap();
    let id = ResourceId::Mdx {
        source: 0,
        ordinal: 2,
    };
    current
        .overlay
        .write()
        .unwrap()
        .write(id.clone(), vec![], b"keep me".to_vec());
    let assert_unchanged = || {
        assert_eq!(current.pool.read().unwrap().sources.len(), 4);
        assert_eq!(
            current.overlay.read().unwrap().get(&id).unwrap().current,
            b"keep me"
        );
    };
    for change in ["version", "base64", "id"] {
        let mut json: serde_json::Value = serde_json::from_slice(&original).unwrap();
        match change {
            "version" => json["version"] = 99.into(),
            "base64" => {
                json["insertions"] = serde_json::json!([{"target":0,"kind":"entry","name":"test","bytes":"!invalid"}])
            }
            _ => {
                json["revisions"] = serde_json::json!([{"id":{"t":"mdx","source":99,"ordinal":0},"original":"","current":"","history":[],"deleted":false}])
            }
        }
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(project::load(&path, &current).is_err());
        assert_unchanged();
    }
    std::fs::write(&path, &original).unwrap();
    let css = dir.path().join("style-ext.css");
    std::fs::write(&css, b"changed source").unwrap();
    assert!(project::load(&path, &current)
        .unwrap_err()
        .contains("源文件已变化"));
    assert_unchanged();
    std::fs::remove_file(css).unwrap();
    assert!(project::load(&path, &current).is_err());
    assert_unchanged();
}

#[test]
fn saving_cannot_overwrite_sources_and_can_replace_project() {
    let (dir, state) = setup();
    let source = dir.path().join("ocean.mdx");
    let bytes = std::fs::read(&source).unwrap();
    assert!(project::save(
        &source,
        &state.pool.read().unwrap(),
        &state.overlay.read().unwrap()
    )
    .is_err());
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    let path = dir.path().join("project.mdict-project");
    save(&path, &state);
    let id = ResourceId::Mdx {
        source: 0,
        ordinal: 2,
    };
    let original = state.pool.read().unwrap().read_original(&id).unwrap();
    state
        .overlay
        .write()
        .unwrap()
        .write(id.clone(), original, b"second save".to_vec());
    save(&path, &state);
    let restored = AppState::default();
    project::load(&path, &restored).unwrap();
    assert_eq!(
        restored.overlay.read().unwrap().get(&id).unwrap().current,
        b"second save"
    );
}
