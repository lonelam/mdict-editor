//! M1 integration tests: source pool, registry index/listing/stats, overlay
//! revision semantics. Uses the shared fixture builder on a temp dir.

use std::path::Path;

use mdict_editor_lib::category::Category;
use mdict_editor_lib::fixtures;
use mdict_editor_lib::registry::{self, ListFilter, Registry};
use mdict_editor_lib::state::{Overlay, ResourceId, SourcePool};

struct Env {
    _dir: tempfile::TempDir,
    pool: SourcePool,
    registry: Registry,
}

fn setup() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let (mdx, mdd, ext_css, ext_js) = fixtures::write_all(dir.path());
    let mut pool = SourcePool::default();
    for p in [&mdx, &mdd, &ext_css, &ext_js] {
        pool.sources.push(SourcePool::open_path(p).unwrap());
    }
    let mut registry = Registry::default();
    registry::ensure_built(&pool, &mut registry).unwrap();
    Env {
        _dir: dir,
        pool,
        registry,
    }
}

fn id_mdx(source: u32, ordinal: u64) -> ResourceId {
    ResourceId::Mdx { source, ordinal }
}
fn id_mdd(source: u32, ordinal: u64) -> ResourceId {
    ResourceId::Mdd { source, ordinal }
}

#[test]
fn open_and_count_sources() {
    let env = setup();
    assert_eq!(env.pool.sources.len(), 4);
    assert_eq!(env.pool.sources[0].kind(), "mdx");
    assert_eq!(env.pool.sources[0].title.as_deref(), Some("Ocean Dictionary"));
    assert_eq!(env.pool.sources[0].entry_count(), 6);
    assert_eq!(env.pool.sources[1].kind(), "mdd");
    assert_eq!(env.pool.sources[1].entry_count(), 9);
    assert_eq!(env.pool.sources[2].kind(), "ext");
    assert_eq!(env.pool.sources[3].kind(), "ext");
}

#[test]
fn list_all_and_by_category() {
    let env = setup();
    let overlay = Overlay::default();

    let all = registry::list_resources(
        &env.pool,
        &overlay,
        &env.registry,
        &ListFilter {
            source: None,
            category: None,
            prefix: "",
            offset: 0,
            limit: 1000,
        },
    )
    .unwrap();
    // 6 entries + 9 mdd resources + 2 external
    assert_eq!(all.len(), 17);
    // Keys are sorted case-insensitively across sources (mdx first: apple..苹果).
    let keys: Vec<&str> = all.iter().map(|m| m.key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "apple", "banana", "hello", "pear", "world", "苹果", // mdx
            "audio/hello.wav",
            "css/extra.css",
            "css/style.css",
            "data/blob.bin",
            "docs/my file.txt",
            "fonts/fake.ttf",
            "img/heart.png",
            "img/logo.png",
            "js/app.js", // mdd
            "style-ext.css",
            "script-ext.js", // external
        ]
    );
    // Wait: that's 17 — the mdd has 9 resources (css×2, js, wav, txt, ttf,
    // bin, png×2). Verified by the count assertion above.
    assert_eq!(env.pool.sources[1].entry_count(), 9);

    let css = registry::list_resources(
        &env.pool,
        &overlay,
        &env.registry,
        &ListFilter {
            source: Some(1),
            category: Some(Category::Css),
            prefix: "",
            offset: 0,
            limit: 1000,
        },
    )
    .unwrap();
    assert_eq!(
        css.iter().map(|m| m.key.as_str()).collect::<Vec<_>>(),
        vec!["css/extra.css", "css/style.css"]
    );
    assert_eq!(css[0].mime, "text/css");
    assert_eq!(css[0].source_name, "assets.mdd");

    let entries = registry::list_resources(
        &env.pool,
        &overlay,
        &env.registry,
        &ListFilter {
            source: None,
            category: Some(Category::Entry),
            prefix: "",
            offset: 0,
            limit: 1000,
        },
    )
    .unwrap();
    assert_eq!(entries.len(), 6);
    assert_eq!(entries[0].mime, "text/html");
}

#[test]
fn list_prefix_filter() {
    let env = setup();
    let overlay = Overlay::default();
    for (prefix, expected) in [
        ("ap", vec!["apple"]),
        ("WO", vec!["world"]), // case-insensitive
        ("img/", vec!["img/heart.png", "img/logo.png"]),
        ("zzz", Vec::<&str>::new()),
    ] {
        let rows = registry::list_resources(
            &env.pool,
            &overlay,
            &env.registry,
            &ListFilter {
                source: None,
                category: None,
                prefix,
                offset: 0,
                limit: 100,
            },
        )
        .unwrap();
        let keys: Vec<String> = rows.into_iter().map(|m| m.key).collect();
        assert_eq!(keys, expected, "prefix {prefix:?}");
    }
}

#[test]
fn list_pagination() {
    let env = setup();
    let overlay = Overlay::default();
    let page = |offset: u64| {
        registry::list_resources(
            &env.pool,
            &overlay,
            &env.registry,
            &ListFilter {
                source: None,
                category: None,
                prefix: "",
                offset,
                limit: 5,
            },
        )
        .unwrap()
    };
    let p0 = page(0);
    let p1 = page(5);
    assert_eq!(p0.len(), 5);
    assert_eq!(p1.len(), 5);
    assert_ne!(p0[0].key, p1[0].key);
    assert_eq!(page(15).len(), 2);
}

#[test]
fn stats_per_category() {
    let env = setup();
    let mut overlay = Overlay::default();
    let stats = |overlay: &Overlay| {
        let rows = registry::stats(&env.pool, overlay, &env.registry, None).unwrap();
        rows.into_iter()
            .map(|s| (s.category, s.count, s.edited))
            .collect::<Vec<_>>()
    };
    let s = stats(&overlay);
    assert!(s.contains(&(Category::Entry, 6, 0)));
    assert!(s.contains(&(Category::Css, 3, 0))); // 2 mdd + 1 ext
    assert!(s.contains(&(Category::Js, 2, 0))); // 1 mdd + 1 ext
    assert!(s.contains(&(Category::Image, 2, 0)));
    assert!(s.contains(&(Category::Audio, 1, 0)));
    assert!(s.contains(&(Category::Font, 1, 0)));
    assert!(s.contains(&(Category::Text, 1, 0)));
    assert!(s.contains(&(Category::Other, 1, 0)));

    // Edit one entry → edited count moves to Entry.
    overlay.write(
        id_mdx(0, 2),
        b"<p>old</p>".to_vec(),
        b"<p>new</p>".to_vec(),
    );
    let s = stats(&overlay);
    assert!(s.contains(&(Category::Entry, 6, 1)));
}

#[test]
fn overlay_write_undo_revert_delete() {
    let env = setup();
    let mut overlay = Overlay::default();
    let hello = id_mdx(0, 2);

    // Source read is unaffected before any edit.
    let original = env.pool.read_original(&hello).unwrap();
    assert!(String::from_utf8_lossy(&original).contains("Hello!"));

    // Write v1, then v2.
    overlay.write(hello.clone(), original.clone(), b"v1".to_vec());
    overlay.write(hello.clone(), original.clone(), b"v2".to_vec());
    let rev = overlay.get(&hello).unwrap();
    assert_eq!(rev.current, b"v2");
    assert_eq!(rev.history.len(), 2);
    assert_eq!(rev.original, original);

    // undo → v1, undo → original content, undo again → no-op.
    assert!(overlay.undo(&hello));
    assert_eq!(overlay.get(&hello).unwrap().current, b"v1");
    assert!(overlay.undo(&hello));
    assert_eq!(overlay.get(&hello).unwrap().current, original);
    assert!(!overlay.undo(&hello));

    // revert removes the chain entirely.
    assert!(overlay.revert(&hello));
    assert!(overlay.get(&hello).is_none());
    assert_eq!(env.pool.read_original(&hello).unwrap(), original);

    // delete marks without touching the source.
    overlay.delete(hello.clone(), original.clone());
    let rev = overlay.get(&hello).unwrap();
    assert!(rev.deleted);
    assert_eq!(rev.current, original);
}

#[test]
fn listing_reflects_overlay_state() {
    let env = setup();
    let mut overlay = Overlay::default();
    let world = id_mdx(0, 4);
    let logo = id_mdd(1, 7); // img/logo.png

    overlay.write(world.clone(), vec![], b"<p>edited</p>".to_vec());
    overlay.delete(logo.clone(), vec![]);

    let rows = registry::list_resources(
        &env.pool,
        &overlay,
        &env.registry,
        &ListFilter {
            source: None,
            category: None,
            prefix: "",
            offset: 0,
            limit: 1000,
        },
    )
    .unwrap();
    let world_meta = rows.iter().find(|m| m.key == "world").unwrap();
    assert!(world_meta.edited && !world_meta.deleted);
    assert_eq!(world_meta.size_current, Some(13));
    let logo_meta = rows.iter().find(|m| m.key == "img/logo.png").unwrap();
    assert!(logo_meta.edited && logo_meta.deleted);
    // Unedited rows carry no size.
    let apple_meta = rows.iter().find(|m| m.key == "apple").unwrap();
    assert!(!apple_meta.edited && apple_meta.size_current.is_none());
}

#[test]
fn read_original_via_pool() {
    let env = setup();
    // css/style.css is mdd ordinal 1 (sorted rows: audio, css/extra, css/style, ...).
    let style = id_mdd(1, 2);
    let bytes = env.pool.read_original(&style).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains(".entry") && text.contains("@import"));

    let ext = ResourceId::Ext { file: 2 };
    let bytes = env.pool.read_original(&ext).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains(".ext"));

    assert!(env.pool.key_of(&style).unwrap() == "css/style.css");
}

#[test]
fn fixtures_exist_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let (mdx, mdd, _, _) = fixtures::write_all(dir.path());
    assert!(Path::new(&mdx).exists());
    assert!(Path::new(&mdd).exists());
}

/// Regression: open_sources pushes `None` placeholders before the first
/// browse; ensure_built must fill them instead of short-circuiting on len.
#[test]
fn ensure_built_fills_placeholder_slots() {
    let env = setup();
    let mut registry = Registry::default();
    // Simulate open_sources: placeholders first, no build.
    while registry.indices.len() < env.pool.sources.len() {
        registry.indices.push(None);
    }
    registry::ensure_built(&env.pool, &mut registry).unwrap();
    let overlay = Overlay::default();
    let rows = registry::list_resources(
        &env.pool,
        &overlay,
        &registry,
        &ListFilter {
            source: None,
            category: None,
            prefix: "",
            offset: 0,
            limit: 1000,
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 17, "all rows must be listed after building");
    let stats = registry::stats(&env.pool, &overlay, &registry, None).unwrap();
    assert!(!stats.is_empty());
}
