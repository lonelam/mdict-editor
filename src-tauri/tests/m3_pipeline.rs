//! M3 integration tests: processors and pipeline dry-run/apply semantics.

use mdict_visualizer_lib::fixtures;
use mdict_visualizer_lib::pipeline::{self, Scope};
use mdict_visualizer_lib::processors::Processor;
use mdict_visualizer_lib::registry::{self, Registry};
use mdict_visualizer_lib::state::{AppState, ResourceId, SourcePool};

fn setup() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let (mdx, mdd, ext_css, ext_js) = fixtures::write_all(dir.path());
    let mut state = AppState::default();
    {
        let mut pool = state.pool.lock().unwrap();
        for p in [&mdx, &mdd, &ext_css, &ext_js] {
            pool.sources.push(SourcePool::open_path(p).unwrap());
        }
        let mut registry = state.registry.lock().unwrap();
        registry::ensure_built(&pool, &mut registry).unwrap();
    }
    (dir, state)
}

#[test]
fn minify_css_strips_whitespace() {
    let input = b"/* header */\n.entry {\n  color:   #123456;\n  margin: 0 auto;\n}\n";
    let out = Processor::MinifyCss.process("a.css", input).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.len() < input.len());
    assert!(text.contains(".entry"));
    assert!(!text.contains('\n'));
}

#[test]
fn minify_js_compresses_and_mangles() {
    let input = br#"
// a comment
function addNumbers(firstValue, secondValue) {
  var resultTotal = firstValue + secondValue;
  console.log("debug: result is", resultTotal);
  return resultTotal;
}
"#;
    let out = Processor::MinifyJs.process("a.js", input).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.len() < input.len(), "should shrink: {text}");
    assert!(!text.contains("addNumbers"), "mangled: {text}");
    assert!(text.contains("console.log"), "console kept by default: {text}");
}

#[test]
fn minify_html_strips_whitespace() {
    let input = b"<div  class=\"entry\" >\n  <p>hello</p>\n</div>\n";
    let out = Processor::MinifyHtml.process("a.html", input).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.len() < input.len());
    assert!(text.contains("<p>hello</p>"));
}

#[test]
fn png_optimize_and_resize_and_convert() {
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("t.png");
    let mut img = image::RgbImage::new(200, 100);
    for y in 0..100u32 {
        for x in 0..200u32 {
            img.put_pixel(x, y, image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
        }
    }
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(&png_path, image::ImageFormat::Png)
        .unwrap();
    let bytes = std::fs::read(&png_path).unwrap();

    // Resize to 64px wide.
    let resized = Processor::ImgResize {
        width: Some(64),
        height: None,
    }
    .process("t.png", &bytes)
    .unwrap();
    let dim = image::load_from_memory(&resized).unwrap();
    assert_eq!((dim.width(), dim.height()), (64, 32), "aspect ratio kept");

    // Convert to jpeg with quality.
    let jpg = Processor::ImgConvert {
        format: "jpeg".into(),
        quality: Some(60),
    }
    .process("t.png", &bytes)
    .unwrap();
    assert_eq!(
        image::guess_format(&jpg).unwrap(),
        image::ImageFormat::Jpeg
    );

    // PNG optimization: gradient png may not shrink; require success and
    // valid png output.
    let opt = Processor::PngOptimize { level: Some(2) }
        .process("t.png", &bytes)
        .unwrap();
    assert_eq!(image::guess_format(&opt).unwrap(), image::ImageFormat::Png);

    // Errors propagate per-resource.
    assert!(Processor::ImgResize {
        width: None,
        height: None
    }
    .process("t.png", &bytes)
    .is_err());
}

#[test]
fn pipeline_dry_run_reports_without_writing() {
    let (_dir, state) = setup();
    let pool = state.pool.lock().unwrap();
    let overlay = state.overlay.lock().unwrap();
    let registry = state.registry.lock().unwrap();

    // Category-scoped css+js minify.
    let reports = pipeline::dry_run(
        &pool,
        &overlay,
        &registry,
        &[Processor::MinifyCss, Processor::MinifyJs],
        &Scope {
            all: true,
            ..Default::default()
        },
    )
    .unwrap();

    let ok_rows: Vec<_> = reports.iter().filter(|r| r.status == "ok").collect();
    let skip_rows: Vec<_> = reports.iter().filter(|r| r.status == "skip").collect();
    assert_eq!(ok_rows.len(), 5, "2 mdd css + 1 mdd js + 2 ext: {ok_rows:?}");
    assert!(skip_rows.len() >= 10, "entries/images skipped");
    assert!(ok_rows.iter().all(|r| r.delta > 0), "css/js must shrink");
    // Dry run must not write.
    assert!(overlay.get(&ResourceId::Mdd { source: 1, ordinal: 8 }).is_none());
}

#[test]
fn pipeline_apply_writes_overlay_and_reverts() {
    let (_dir, state) = setup();
    let scope_all = Scope {
        all: true,
        ..Default::default()
    };
    {
        let pool = state.pool.lock().unwrap();
        let mut overlay = state.overlay.lock().unwrap();
        let mut registry = state.registry.lock().unwrap();
        let reports = pipeline::apply(
            &pool,
            &mut overlay,
            &mut registry,
            &[Processor::MinifyCss, Processor::MinifyJs],
            &scope_all,
        )
        .unwrap();
        let applied: Vec<_> = reports.iter().filter(|r| r.status == "ok").collect();
        assert_eq!(applied.len(), 5);
    }
    let app_js = ResourceId::Mdd { source: 1, ordinal: 8 };
    let (rev_original_len, rev_current_len, minified_ok) = {
        let overlay = state.overlay.lock().unwrap();
        let rev = overlay.get(&app_js).expect("applied");
        (
            rev.original.len(),
            rev.current.len(),
            !String::from_utf8_lossy(&rev.current).contains("dictionary helper"),
        )
    };
    assert!(rev_current_len < rev_original_len);
    assert!(minified_ok, "minified js no longer has comments");

    // Revert everything back.
    let mut overlay = state.overlay.lock().unwrap();
    let ids: Vec<ResourceId> = overlay.revisions.keys().cloned().collect();
    for id in ids {
        overlay.revert(&id);
    }
    assert!(overlay.revisions.is_empty());
}

#[test]
fn pipeline_per_item_errors_do_not_break_batch() {
    let (_dir, state) = setup();
    let pool = state.pool.lock().unwrap();
    let overlay = state.overlay.lock().unwrap();
    let registry = state.registry.lock().unwrap();

    // png-optimize only applies to png; text files get "skip".
    let reports = pipeline::dry_run(
        &pool,
        &overlay,
        &registry,
        &[Processor::PngOptimize { level: Some(2) }, Processor::MinifyJs],
        &Scope {
            ids: Some(vec![
                ResourceId::Mdd { source: 1, ordinal: 7 }, // img/logo.png
                ResourceId::Mdd { source: 1, ordinal: 4 }, // docs/my file.txt
                ResourceId::Mdd { source: 1, ordinal: 8 }, // js/app.js
            ]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reports.len(), 3);
    assert!(reports[0].status == "ok" || reports[0].status == "skip");
    assert_eq!(reports[1].status, "skip", "txt not applicable: {:?}", reports[1]);
    assert_eq!(reports[2].status, "ok");
}

#[test]
fn pipeline_empty_processor_set_skips_everything() {
    let (_dir, state) = setup();
    let pool = state.pool.lock().unwrap();
    let overlay = state.overlay.lock().unwrap();
    let registry = state.registry.lock().unwrap();
    let reports = pipeline::dry_run(
        &pool,
        &overlay,
        &registry,
        &[],
        &Scope {
            ids: Some(vec![ResourceId::Mdd { source: 1, ordinal: 8 }]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reports[0].status, "skip");
}
