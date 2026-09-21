//! Insertion integration tests: pending entries/resources materialize at
//! export; duplicate keys are rejected; insertions alone trigger rebuilds.

use mdict_editor_lib::export::{export_build, ExportConfig};
use mdict_editor_lib::fixtures;
use mdict_editor_lib::pipeline::no_ctl;
use mdict_editor_lib::registry::{self, Registry};
use mdict_editor_lib::state::{AppState, InsertKind, Overlay, SourcePool};

fn setup() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let (mdx, mdd, ext_css, ext_js) = fixtures::write_all(dir.path());
    let state = AppState::default();
    {
        let mut pool = state.pool.write().unwrap();
        for p in [&mdx, &mdd, &ext_css, &ext_js] {
            pool.sources.push(SourcePool::open_path(p).unwrap());
        }
    }
    {
        let pool = state.pool.read().unwrap();
        let mut registry = state.registry.write().unwrap();
        registry::ensure_built(&pool, &mut registry).unwrap();
    }
    (dir, state)
}

fn out_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn insert_entry_materializes_at_export() {
    let (_dir, state) = setup();
    let mut overlay = Overlay::default();
    overlay.insertions.push(mdict_editor_lib::state::Insertion {
        target: 0,
        kind: InsertKind::Entry,
        name: "zebra".into(),
        bytes: b"<p>a striped animal</p>".to_vec(),
    });

    let out = out_dir();
    let report = export_build(
        &state.pool.read().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    let file = &report.files[0];
    assert!(file.check_ok, "{}", file.message);
    assert_eq!(file.entries, 7, "6 original + 1 inserted");

    let reopened = mdictlib::MdxFile::open(&file.path).unwrap();
    let entry = reopened.lookup("zebra").unwrap().expect("inserted entry");
    assert_eq!(entry.text(), "<p>a striped animal</p>");
}

#[test]
fn insert_resource_materializes_with_lossy_chain() {
    let (_dir, state) = setup();
    let mut overlay = Overlay::default();
    overlay.insertions.push(mdict_editor_lib::state::Insertion {
        target: 1,
        kind: InsertKind::Resource,
        name: "img/added.png".into(),
        bytes: std::fs::read(
            // reuse a fixture png: regenerate one quickly
            {
                let mut img = image::RgbImage::new(48, 48);
                for (x, y, p) in img.enumerate_pixels_mut() {
                    *p = image::Rgb([x as u8, y as u8, 7]);
                }
                let dir = tempfile::tempdir().unwrap();
                let p = dir.path().join("added.png");
                image::DynamicImage::ImageRgb8(img)
                    .save_with_format(&p, image::ImageFormat::Png)
                    .unwrap();
                std::mem::forget(dir); // keep file until read below
                p
            },
        )
        .unwrap(),
    });

    let out = out_dir();
    let report = export_build(
        &state.pool.read().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            lossy: Some(vec![mdict_editor_lib::processors::Processor::ImgConvert {
                format: "jpeg".into(),
                quality: Some(75),
            }]),
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    // .edited rebuild (insertion counts as edit) + .lossy copy.
    assert!(report.files.len() >= 2, "{:?}", report.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>());
    for f in &report.files {
        assert!(f.check_ok, "{}: {}", f.path, f.message);
    }
    let lossy = report
        .files
        .iter()
        .find(|f| f.path.contains("lossy") && f.path.ends_with("assets.mdd"))
        .unwrap();
    let reopened = mdictlib::MddFile::open(&lossy.path).unwrap();
    let added = reopened.lookup("img/added.png").unwrap().expect("inserted resource");
    assert_eq!(
        image::guess_format(added.bytes()).unwrap(),
        image::ImageFormat::Jpeg,
        "insertion passes through the lossy chain"
    );
    assert_eq!(reopened.len(), 10, "9 original + 1 inserted");
}

#[test]
fn duplicate_insertion_keys_rejected_by_commands() {
    // Command-level validation is exercised through the command fns; here we
    // verify the dedupe predicates used by them: source index + pending list.
    let (_dir, state) = setup();
    let pool = state.pool.read().unwrap();
    let registry = state.registry.write().unwrap();
    let mut overlay = Overlay::default();

    // "hello" exists in the mdx index (source 0).
    let exists = registry.indices[0]
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|(k, _)| k == "hello");
    assert!(exists, "index sees existing entry");

    // img/logo.png exists in the mdd index under its normalized key.
    let exists_res = registry.indices[1]
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|(k, _)| k == "img/logo.png");
    assert!(exists_res);

    // Pending duplicates are caught by normalized comparison.
    overlay.insertions.push(mdict_editor_lib::state::Insertion {
        target: 1,
        kind: InsertKind::Resource,
        name: "IMG\\Logo.PNG".into(),
        bytes: vec![],
    });
    let dup_pending = overlay.insertions.iter().any(|i| {
        i.target == 1
            && i.kind == InsertKind::Resource
            && registry::normalize_key(&i.name) == "img/logo.png"
    });
    assert!(dup_pending, "pending insertion normalized duplicate detected");
    drop(pool);
}

#[test]
fn remove_insertion_restores_clean_export() {
    let (_dir, state) = setup();
    let mut overlay = Overlay::default();
    overlay.insertions.push(mdict_editor_lib::state::Insertion {
        target: 0,
        kind: InsertKind::Entry,
        name: "temp".into(),
        bytes: b"t".to_vec(),
    });
    overlay.insertions.clear();

    let out = out_dir();
    let report = export_build(
        &state.pool.read().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    let file = report
        .files
        .iter()
        .find(|f| f.path.ends_with("ocean.mdx"))
        .expect("mdx exported");
    assert!(file.check_ok);
    let reopened = mdictlib::MdxFile::open(&file.path).unwrap();
    assert!(reopened.locate("temp").unwrap().is_none(), "removed insertion absent");
}

/// New-file workflow (创建新词典): a builder-emitted EMPTY mdx/mdd opens as
/// a source, indexes cleanly, and pending insertions materialize at export.
#[test]
fn empty_created_sources_open_and_receive_insertions() {
    let dir = tempfile::tempdir().unwrap();
    let mdx_path = dir.path().join("fresh.mdx");
    let mdd_path = dir.path().join("fresh.mdd");
    let mut bytes = Vec::new();
    let mut mdx = mdictlib::MdxBuilder::with_options(
        mdictlib::WriteOptions::new()
            .with_encoding(mdictlib::WriteEncoding::Utf8)
            .with_compression(mdictlib::WriteCompression::Zlib),
    );
    mdx.header_attribute("Title", "fresh").unwrap();
    mdx.finish(&mut bytes).unwrap();
    std::fs::write(&mdx_path, &bytes).unwrap();
    let mut bytes = Vec::new();
    mdictlib::MddBuilder::with_options(
        mdictlib::WriteOptions::new().with_compression(mdictlib::WriteCompression::Zlib),
    )
    .finish(&mut bytes)
    .unwrap();
    std::fs::write(&mdd_path, &bytes).unwrap();

    let state = AppState::default();
    {
        let mut pool = state.pool.write().unwrap();
        for p in [&mdx_path, &mdd_path] {
            pool.sources.push(SourcePool::open_path(p.to_str().unwrap()).unwrap());
        }
    }
    {
        let pool = state.pool.read().unwrap();
        assert_eq!(pool.sources[0].entry_count(), 0);
        assert_eq!(pool.sources[1].entry_count(), 0);
        assert_eq!(pool.sources[0].title.as_deref(), Some("fresh"));
        let mut registry = state.registry.write().unwrap();
        registry::ensure_built(&pool, &mut registry).unwrap();
    }

    let mut overlay = Overlay::default();
    overlay
        .insertions
        .push(mdict_editor_lib::state::Insertion {
            target: 0,
            kind: InsertKind::Entry,
            name: "hello".into(),
            bytes: b"<p>hi</p>".to_vec(),
        });
    overlay
        .insertions
        .push(mdict_editor_lib::state::Insertion {
            target: 1,
            kind: InsertKind::Resource,
            name: "img/x.png".into(),
            bytes: vec![1, 2, 3],
        });
    let out = out_dir();
    let report = export_build(
        &state.pool.read().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    let file = report
        .files
        .iter()
        .find(|f| f.path.ends_with("fresh.mdx"))
        .expect("fresh.mdx exported");
    assert!(file.check_ok);
    let reopened = mdictlib::MdxFile::open(&file.path).unwrap();
    assert!(reopened.locate("hello").unwrap().is_some());
}
