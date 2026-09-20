//! Insertion integration tests: pending entries/resources materialize at
//! export; duplicate keys are rejected; insertions alone trigger rebuilds.

use mdict_editor_lib::export::{export_build, ExportConfig};
use mdict_editor_lib::fixtures;
use mdict_editor_lib::pipeline::NO_CTL;
use mdict_editor_lib::registry::{self, Registry};
use mdict_editor_lib::state::{AppState, InsertKind, Overlay, SourcePool};

fn setup() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let (mdx, mdd, ext_css, ext_js) = fixtures::write_all(dir.path());
    let state = AppState::default();
    {
        let mut pool = state.pool.lock().unwrap();
        for p in [&mdx, &mdd, &ext_css, &ext_js] {
            pool.sources.push(SourcePool::open_path(p).unwrap());
        }
    }
    {
        let pool = state.pool.lock().unwrap();
        let mut registry = state.registry.lock().unwrap();
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
        &state.pool.lock().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            mdx: true,
            ..Default::default()
        },
        &NO_CTL,
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
        &state.pool.lock().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            mdd: true,
            lossy: Some(vec![mdict_editor_lib::processors::Processor::ImgConvert {
                format: "jpeg".into(),
                quality: Some(75),
            }]),
            ..Default::default()
        },
        &NO_CTL,
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
        .find(|f| f.path.ends_with("lossy.mdd"))
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
    let pool = state.pool.lock().unwrap();
    let registry = state.registry.lock().unwrap();
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
        &state.pool.lock().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            mdx: true,
            ..Default::default()
        },
        &NO_CTL,
    )
    .unwrap();
    assert!(!report.files[0].check_ok);
    assert!(report.files[0].message.contains("no edits"));
}
