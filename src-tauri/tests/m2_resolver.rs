//! M2 integration tests: reference resolution and the mdres:// preview
//! request handler (called directly, no webview required).

use mdict_editor_lib::fixtures;
use mdict_editor_lib::registry::{self, Registry};
use mdict_editor_lib::resolver::{resolve, resolve_path, ResolveOutcome};
use mdict_editor_lib::state::{AppState, ResourceId, SourcePool};
use std::sync::Mutex;

struct Env {
    _dir: tempfile::TempDir,
    state: AppState,
    mdd_logo: ResourceId,
    mdd_heart: ResourceId,
    mdd_style: ResourceId,
    mdd_txt: ResourceId,
    mdx_world: ResourceId,
    mdx_hello: ResourceId,
}

fn setup() -> Env {
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
    // Sorted mdd rows: audio/hello.wav=0, css/extra.css=1, css/style.css=2,
    // data/blob.bin=3, docs/my file.txt=4, fonts/fake.ttf=5, img/heart.png=6,
    // img/logo.png=7, js/app.js=8.
    Env {
        _dir: dir,
        state,
        mdd_logo: ResourceId::Mdd { source: 1, ordinal: 7 },
        mdd_heart: ResourceId::Mdd { source: 1, ordinal: 6 },
        mdd_style: ResourceId::Mdd { source: 1, ordinal: 2 },
        mdd_txt: ResourceId::Mdd { source: 1, ordinal: 4 },
        mdx_world: ResourceId::Mdx { source: 0, ordinal: 4 },
        mdx_hello: ResourceId::Mdx { source: 0, ordinal: 2 },
    }
}

fn found_of(out: ResolveOutcome) -> (ResourceId, &'static str) {
    match out {
        ResolveOutcome::Found { target, basis } => (target, basis),
        other => panic!("expected Found, got {other:?}"),
    }
}

#[test]
fn normalize_paths() {
    use mdict_editor_lib::resolver::{dir_of, normalize_path};
    assert_eq!(normalize_path("a/b/../c.png"), "/a/c.png");
    assert_eq!(normalize_path("\\img\\logo.png"), "/img/logo.png");
    assert_eq!(normalize_path("img//x/../y.png"), "/img/y.png");
    assert_eq!(normalize_path("../up.png"), "/up.png");
    assert_eq!(dir_of("/css/style.css"), "/css/");
    assert_eq!(dir_of("/style.css"), "/");
    assert_eq!(dir_of("/"), "/");
}

#[test]
fn relative_resolution_from_css_context() {
    let env = setup();
    let out = resolve(
        &env.state.pool.lock().unwrap(),
        &env.state.registry.lock().unwrap(),
        "../img/heart.png",
        Some(&env.mdd_style),
    )
    .unwrap();
    let (target, basis) = found_of(out);
    assert_eq!(target, env.mdd_heart);
    assert_eq!(basis, "rel");
}

#[test]
fn root_and_bare_and_percent() {
    let env = setup();
    let pool = env.state.pool.lock().unwrap();
    let registry = env.state.registry.lock().unwrap();

    // Root-anchored from an MDX entry context.
    let (t, basis) = found_of(resolve(&pool, &registry, "/img/logo.png", Some(&env.mdx_hello)).unwrap());
    assert_eq!(t, env.mdd_logo);
    assert_eq!(basis, "root");

    // Percent-encoded reference resolves to a key containing a space.
    let (t, _) = found_of(resolve(&pool, &registry, "docs/my%20file.txt", Some(&env.mdx_world)).unwrap());
    assert_eq!(t, env.mdd_txt);

    // Bare path from root context.
    let (t, basis) = found_of(resolve(&pool, &registry, "css/style.css", None).unwrap());
    assert_eq!(t, env.mdd_style);
    assert_eq!(basis, "root");
}

#[test]
fn schemes() {
    let env = setup();
    let pool = env.state.pool.lock().unwrap();
    let registry = env.state.registry.lock().unwrap();

    // entry:// jumps into the MDX.
    let (t, basis) = found_of(resolve(&pool, &registry, "entry://world", Some(&env.mdx_hello)).unwrap());
    assert_eq!(t, env.mdx_world);
    assert_eq!(basis, "entry");

    // sound:// resolves as a resource path.
    let (t, _) = found_of(resolve(&pool, &registry, "sound://audio/hello.wav", None).unwrap());
    assert_eq!(t, ResourceId::Mdd { source: 1, ordinal: 0 });

    // External links are reported, not resolved.
    assert!(matches!(
        resolve(&pool, &registry, "https://example.com/x.png", None).unwrap(),
        ResolveOutcome::ExternalRef
    ));
}

#[test]
fn suffix_fallback_and_ambiguity() {
    // Extra source with a colliding key, to exercise exact-match ambiguity.
    let dir = tempfile::tempdir().unwrap();
    let mut second = mdictlib::MddBuilder::new();
    second
        .add_resource("css/extra.css", b"duplicate key".as_slice())
        .unwrap();
    let mut dup_bytes = Vec::new();
    second.finish(&mut dup_bytes).unwrap();
    let dup_path = dir.path().join("dup.mdd");
    std::fs::write(&dup_path, &dup_bytes).unwrap();

    let env = setup();
    env.state
        .pool
        .lock()
        .unwrap()
        .sources
        .push(SourcePool::open_path(dup_path.to_str().unwrap()).unwrap());
    {
        let pool = env.state.pool.lock().unwrap();
        let mut registry = env.state.registry.lock().unwrap();
        registry::ensure_built(&pool, &mut registry).unwrap();
    }

    let pool = env.state.pool.lock().unwrap();
    let registry = env.state.registry.lock().unwrap();

    // Key is img/logo.png but authors write logo.png → unique suffix hit.
    let (t, basis) = found_of(resolve(&pool, &registry, "logo.png", Some(&env.mdx_hello)).unwrap());
    assert_eq!(t, env.mdd_logo);
    assert_eq!(basis, "suffix");

    // css/extra.css exists in two MDDs → exact-match ambiguity.
    let out = resolve(&pool, &registry, "css/extra.css", None).unwrap();
    match out {
        ResolveOutcome::Ambiguous { candidates, keys } => {
            assert_eq!(candidates.len(), 2);
            assert_eq!(keys.len(), 2);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }

    // Total miss.
    assert!(matches!(
        resolve(&pool, &registry, "nope/missing.png", None).unwrap(),
        ResolveOutcome::NotFound
    ));
}

#[test]
fn case_insensitive_keys() {
    let env = setup();
    let pool = env.state.pool.lock().unwrap();
    let registry = env.state.registry.lock().unwrap();
    let (t, _) = found_of(resolve(&pool, &registry, "/IMG/LOGO.PNG", None).unwrap());
    assert_eq!(t, env.mdd_logo);
}

#[test]
fn mdres_handler_serves_resolved_bytes() {
    let env = setup();
    // Preview the "world" entry itself.
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdx-0-4/");
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["Content-Type"].to_str().unwrap().starts_with("text/html"));
    let body = String::from_utf8_lossy(resp.body());
    assert!(body.contains("css/style.css"));

    // Relative request inside that context resolves to real css bytes.
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdx-0-4/css/style.css");
    assert_eq!(resp.status(), 200);
    let body = String::from_utf8_lossy(resp.body());
    assert!(body.contains("@import"));

    // Image via suffix-less root path, percent-encoded dir with space.
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdx-0-4/img/heart.png");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["Content-Type"], "image/png");
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdx-0-4/docs/my%20file.txt");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.body(), b"spaces in the middle");

    // Misses → 404.
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdx-0-4/nope.png");
    assert_eq!(resp.status(), 404);
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/bogus");
    assert_eq!(resp.status(), 404);

    // Preview an MDD css resource directly (context = the css itself).
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdd-1-2/");
    assert_eq!(resp.status(), 200);
    assert!(String::from_utf8_lossy(resp.body()).contains(".logo"));
    // Its @import resolves relative to /css/.
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdd-1-2/extra.css");
    assert_eq!(resp.status(), 200);
}

#[test]
fn mdres_uses_overlay_bytes() {
    let env = setup();
    // Edit css/style.css in the overlay; the preview must reflect it.
    let edited = b"/* edited */\n.entry { color: red; }".to_vec();
    env.state
        .overlay
        .lock()
        .unwrap()
        .write(env.mdd_style.clone(), vec![], edited);
    let resp = mdict_editor_lib::handle_mdres_request(&env.state, "/preview/mdd-1-2/");
    assert!(String::from_utf8_lossy(resp.body()).contains("/* edited */"));
}

#[test]
fn resolve_path_public_api_sound() {
    let env = setup();
    let pool = env.state.pool.lock().unwrap();
    let registry = env.state.registry.lock().unwrap();
    let (t, _) = found_of(resolve_path(&pool, &registry, "/audio/hello.wav", None).unwrap());
    assert_eq!(t, ResourceId::Mdd { source: 1, ordinal: 0 });
}
