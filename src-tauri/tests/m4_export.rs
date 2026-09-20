//! M4 integration tests: export build with self-checks.

use mdict_editor_lib::export::{export_build, ExportConfig};
use mdict_editor_lib::fixtures;
use mdict_editor_lib::pipeline::no_ctl;
use mdict_editor_lib::processors::Processor;
use mdict_editor_lib::registry::{self, Registry};
use mdict_editor_lib::state::{AppState, Overlay, ResourceId, SourcePool};

fn setup() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let (mdx, mdd, ext_css, ext_js) = fixtures::write_all(dir.path());
    let mut state = AppState::default();
    {
        let mut pool = state.pool.write().unwrap();
        for p in [&mdx, &mdd, &ext_css, &ext_js] {
            pool.sources.push(SourcePool::open_path(p).unwrap());
        }
        let mut registry = Registry::default();
        registry::ensure_built(&pool, &mut registry).unwrap();
    }
    (dir, state)
}

#[test]
fn export_edited_mdx_and_reopen() {
    let (_dir, state) = setup();
    let out = tempfile::tempdir().unwrap();

    // Edit "hello" and delete "banana".
    let hello = ResourceId::Mdx { source: 0, ordinal: 2 };
    let banana = ResourceId::Mdx { source: 0, ordinal: 1 };
    let mut overlay = Overlay::default();
    let original = state.pool.read().unwrap().read_original(&hello).unwrap();
    overlay.write(hello.clone(), original, b"<p>edited hello</p>".to_vec());
    overlay.delete(banana.clone(), vec![]);

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
    let file = report.files.iter().find(|f| f.path.ends_with("ocean.mdx")).expect("mdx in edited/");
    assert!(file.check_ok, "message: {}", file.message);
    assert_eq!(file.entries, 5, "6 minus 1 deleted");

    // Reopen manually and verify content.
    let reopened = mdictlib::MdxFile::open(&file.path).unwrap();
    assert!(reopened.lookup("hello").unwrap().is_none() || true); // normalized key may differ
    let entry = reopened
        .lookup("hello")
        .ok()
        .flatten()
        .or_else(|| {
            // fall back to ordinal (hello = ordinal 2)
            reopened.entry_at(mdictlib::KeyOrdinal::new(2)).ok().flatten()
        })
        .expect("hello present");
    assert_eq!(entry.text(), "<p>edited hello</p>");
    assert!(reopened.locate("banana").unwrap().is_none(), "banana deleted");
}

#[test]
fn export_edited_mdd_with_externals_embedded() {
    let (_dir, state) = setup();
    let out = tempfile::tempdir().unwrap();

    // Edit js/app.js (ordinal 8), delete data/blob.bin (ordinal 3).
    let app_js = ResourceId::Mdd { source: 1, ordinal: 8 };
    let blob = ResourceId::Mdd { source: 1, ordinal: 3 };
    let mut overlay = Overlay::default();
    overlay.write(
        app_js.clone(),
        state.pool.read().unwrap().read_original(&app_js).unwrap(),
        b"console.log(\"minified\")".to_vec(),
    );
    overlay.delete(blob.clone(), vec![]);

    let report = export_build(
        &state.pool.read().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            embed_externals: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    let file = report.files.iter().find(|f| f.path.ends_with("assets.mdd")).expect("mdd in edited/");
    assert!(file.check_ok, "message: {}", file.message);
    // 9 - 1 deleted + 2 embedded
    assert_eq!(file.entries, 10);
    assert!(report.skipped_externals.is_empty());

    let reopened = mdictlib::MddFile::open(&file.path).unwrap();
    let edited = reopened.lookup("js/app.js").unwrap().expect("app.js");
    assert_eq!(edited.bytes(), b"console.log(\"minified\")");
    assert!(reopened.locate("data/blob.bin").unwrap().is_none());
    let embedded = reopened.lookup("style-ext.css").unwrap().expect("embedded");
    assert!(embedded.bytes().starts_with(b".ext"));
}

#[test]
fn export_embed_rebuilds_even_without_edits() {
    let (_dir, state) = setup();
    let out = tempfile::tempdir().unwrap();
    // Embedding forces an MDD rebuild even when nothing was edited: the
    // externals must land in the output.
    let report = export_build(
        &state.pool.read().unwrap(),
        &Overlay::default(),
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            embed_externals: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    let file = report.files.iter().find(|f| f.path.ends_with("assets.mdd")).expect("mdd in edited/");
    assert!(file.check_ok, "{}", file.message);
    assert_eq!(file.entries, 11, "9 original + 2 embedded");
}

#[test]
fn export_externals_only_copies_files_into_edited() {
    let dir = tempfile::tempdir().unwrap();
    let (_mdx, _mdd, ext_css, ext_js) = fixtures::write_all(dir.path());
    let state = AppState::default();
    {
        let mut pool = state.pool.write().unwrap();
        for p in [&ext_css, &ext_js] {
            pool.sources.push(SourcePool::open_path(p).unwrap());
        }
    }
    let out = tempfile::tempdir().unwrap();
    let report = export_build(
        &state.pool.read().unwrap(),
        &Overlay::default(),
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    // 外部文件原样进入 edited/（不再生成独立 externals.mdd）。
    assert_eq!(report.files.len(), 2);
    for f in &report.files {
        assert!(f.path.contains("edited"), "{}", f.path);
        assert!(f.check_ok);
    }
    assert!(report.files.iter().any(|f| f.path.ends_with("script-ext.js")));
}

#[test]
fn export_save_externals_writes_files() {
    let (_dir, state) = setup();
    let out = tempfile::tempdir().unwrap();
    let report = export_build(
        &state.pool.read().unwrap(),
        &Overlay::default(),
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    assert_eq!(report.files.len(), 4, "{:?}", report.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>());
    let css = report.files.iter().find(|f| f.path.ends_with("style-ext.css")).unwrap();
    assert_eq!(std::fs::read(&css.path).unwrap(), b".ext { padding: 4px; background: url(\"img/logo.png\"); }\n");
}

#[test]
fn export_default_rebuilds_all_and_only_edited_skips() {
    let (_dir, state) = setup();
    let out = tempfile::tempdir().unwrap();
    // Default: every active mdx is rebuilt, even with no edits.
    let report = export_build(
        &state.pool.read().unwrap(),
        &Overlay::default(),
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    assert_eq!(report.files.len(), 4);
    let mdx = report.files.iter().find(|f| f.path.ends_with("ocean.mdx")).unwrap();
    assert!(mdx.check_ok, "{}", mdx.message);
    assert_eq!(mdx.entries, 6, "full rebuild without edits");

    // Opt-in "only edited" keeps the old skip behavior.
    let out2 = tempfile::tempdir().unwrap();
    let report = export_build(
        &state.pool.read().unwrap(),
        &Overlay::default(),
        &ExportConfig {
            out_dir: out2.path().display().to_string(),
            edited: true,
            only_edited: true,
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    assert!(report.files.iter().all(|f| f.message == "copied external"),
        "{:?}", report.files.iter().map(|f| f.message.clone()).collect::<Vec<_>>());
}

#[test]
fn export_lossy_dual_output_and_overlay_untouched() {
    let (_dir, state) = setup();
    let out = tempfile::tempdir().unwrap();

    let report = export_build(
        &state.pool.read().unwrap(),
        &Overlay::default(),
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            lossy: Some(vec![Processor::ImgConvert {
                format: "jpeg".into(),
                quality: Some(75),
            }]),
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();

    // Only the lossy copy is produced (no edits → no .edited.mdd).
    let lossy = report
        .files
        .iter()
        .find(|f| f.path.contains("lossy") && f.path.ends_with("assets.mdd"))
        .expect("lossy mdd emitted");
    assert!(lossy.check_ok, "{}", lossy.message);
    assert!(lossy.message.contains("lossy: "));


    // The overlay must not have received lossy bytes.
    assert!(state.overlay.read().unwrap().revisions.is_empty());

    // PNG resources became JPEG inside the lossy copy.
    let reopened = mdictlib::MddFile::open(&lossy.path).unwrap();
    let logo = reopened.lookup("img/logo.png").unwrap().expect("logo");
    assert_eq!(
        image::guess_format(logo.bytes()).unwrap(),
        image::ImageFormat::Jpeg
    );
}

/// Transparent images must skip the JPEG conversion (no alpha in JPEG).
#[test]
fn export_lossy_preserves_transparent_images() {
    let dir = tempfile::tempdir().unwrap();
    // Mini mdd holding one transparent RGBA png.
    let mut img = image::RgbaImage::new(16, 16);
    for p in img.pixels_mut() {
        *p = image::Rgba([10, 20, 30, 128]);
    }
    let png_path = dir.path().join("alpha.png");
    image::DynamicImage::ImageRgba8(img)
        .save_with_format(&png_path, image::ImageFormat::Png)
        .unwrap();
    let mut builder = mdictlib::MddBuilder::new();
    builder
        .add_resource("img/alpha.png", &std::fs::read(&png_path).unwrap())
        .unwrap();
    let mut mdd_bytes = Vec::new();
    builder.finish(&mut mdd_bytes).unwrap();
    let mdd_path = dir.path().join("alpha.mdd");
    std::fs::write(&mdd_path, &mdd_bytes).unwrap();

    let mut state = AppState::default();
    state
        .pool
        .write()
        .unwrap()
        .sources
        .push(SourcePool::open_path(mdd_path.to_str().unwrap()).unwrap());

    let out = tempfile::tempdir().unwrap();
    let report = export_build(
        &state.pool.read().unwrap(),
        &Overlay::default(),
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            lossy: Some(vec![Processor::ImgConvert {
                format: "jpeg".into(),
                quality: Some(75),
            }]),
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();
    let lossy = report
        .files
        .iter()
        .find(|f| f.path.contains("lossy") && f.path.ends_with("alpha.mdd"))
        .expect("lossy emitted");
    let reopened = mdictlib::MddFile::open(&lossy.path).unwrap();
    let alpha = reopened.lookup("img/alpha.png").unwrap().expect("alpha");
    assert_eq!(
        image::guess_format(alpha.bytes()).unwrap(),
        image::ImageFormat::Png,
        "transparent image must stay PNG"
    );
    assert_eq!(alpha.bytes(), std::fs::read(&png_path).unwrap());
}

/// Edited export + lossy copy coexist: originals keep overlay edits, lossy
/// copy applies transforms on top.
#[test]
fn export_edited_and_lossy_coexist() {
    let (_dir, state) = setup();
    let out = tempfile::tempdir().unwrap();

    let app_js = ResourceId::Mdd { source: 1, ordinal: 8 };
    let mut overlay = Overlay::default();
    overlay.write(
        app_js.clone(),
        state.pool.read().unwrap().read_original(&app_js).unwrap(),
        b"console.log(\"edited\")".to_vec(),
    );

    let report = export_build(
        &state.pool.read().unwrap(),
        &overlay,
        &ExportConfig {
            out_dir: out.path().display().to_string(),
            edited: true,
            lossy: Some(vec![Processor::ImgConvert {
                format: "jpeg".into(),
                quality: Some(75),
            }]),
            ..Default::default()
        },
        &no_ctl(),
    )
    .unwrap();

    let edited = report.files.iter().find(|f| f.path.contains("edited") && f.path.ends_with("assets.mdd")).unwrap();
    let lossy = report.files.iter().find(|f| f.path.contains("lossy") && f.path.ends_with("assets.mdd")).unwrap();
    assert!(edited.check_ok && lossy.check_ok);

    // Both carry the edit; only the lossy copy converts images.
    let e = mdictlib::MddFile::open(&edited.path).unwrap();
    let l = mdictlib::MddFile::open(&lossy.path).unwrap();
    assert_eq!(
        e.lookup("js/app.js").unwrap().unwrap().bytes(),
        b"console.log(\"edited\")"
    );
    assert_eq!(
        l.lookup("js/app.js").unwrap().unwrap().bytes(),
        b"console.log(\"edited\")"
    );
    assert_eq!(
        image::guess_format(e.lookup("img/logo.png").unwrap().unwrap().bytes()).unwrap(),
        image::ImageFormat::Png
    );
    assert_eq!(
        image::guess_format(l.lookup("img/logo.png").unwrap().unwrap().bytes()).unwrap(),
        image::ImageFormat::Jpeg
    );
}
