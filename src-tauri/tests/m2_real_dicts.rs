//! Integration tests against the real dictionaries in C:\Dictionaries.
//! These exercise large-file indexing, real-world key shapes and reference
//! resolution on genuine dictionary content.
//!
//! Ignored by default (machine-specific paths); run explicitly:
//!   cargo test --test m2_real_dicts -- --ignored --test-threads=2

use std::time::Instant;

use mdict_visualizer_lib::category::Category;
use mdict_visualizer_lib::registry::{self, ListFilter};
use mdict_visualizer_lib::resolver::{resolve, ResolveOutcome};
use mdict_visualizer_lib::state::{AppState, ResourceId, SourcePool};

const AHD_DIR: &str = r"C:\Dictionaries\AHD美语传统双解词典（紧凑优雅版）";
const WORDNET: &str = r"C:\Dictionaries\WordNet\WordNet 3.1 1.50.mdx";
const LONGMAN: &str = r"C:\Dictionaries\LongmanDict\LongmanDictionaryOfContemporaryEnglish6thEnEn.mdx";

fn have(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

/// Simple extraction of src/href attribute values from entry HTML.
fn extract_refs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["src", "href"] {
        let needle = format!("{attr}=\"");
    let mut rest: &str = html;
        while let Some(pos) = rest.find(&needle) {
            let after = &rest[pos + needle.len()..];
            if let Some(end) = after.find('"') {
                out.push(after[..end].to_string());
                rest = &after[end..];
            } else {
                break;
            }
        }
    }
    out
}

fn open_state(paths: &[&str]) -> AppState {
    let state = AppState::default();
    {
        let mut pool = state.pool.lock().unwrap();
        for p in paths {
            pool.sources.push(SourcePool::open_path(p).expect(p));
        }
        let mut registry = state.registry.lock().unwrap();
        registry::ensure_built(&pool, &mut registry).expect("index build");
    }
    state
}

/// Locate a word in source 0 and return its HTML body.
fn locate_html(pool: &SourcePool, word: &str) -> Option<String> {
    let mdict_visualizer_lib::state::Source::Mdx(file) = &pool.sources.first()?.source else {
        return None;
    };
    let entry = file.lookup(word).ok()??;
    Some(entry.text().to_string())
}

#[test]
#[ignore]
fn real_ahd_full_combo() {
    let mdx = format!("{AHD_DIR}\\AHD双解.mdx");
    let mdd = format!("{AHD_DIR}\\AHD双解.mdd");
    let css = format!("{AHD_DIR}\\ahd3e.css");
    let js = format!("{AHD_DIR}\\ahd3e.js");
    assert!(have(&mdx) && have(&mdd), "AHD files missing");

    let t0 = Instant::now();
    let state = open_state(&[&mdx, &mdd, &css, &js]);
    let t_open = t0.elapsed();

    let (pool, registry) = (state.pool.lock().unwrap(), state.registry.lock().unwrap());
    let counts: Vec<u64> = pool.sources.iter().map(|s| s.entry_count()).collect();
    println!(
        "AHD opened+indexed in {t_open:?}: entries={} mdd={} ext×2",
        counts[0], counts[1]
    );
    assert!(counts[0] > 10_000, "AHD should have many entries");

    // Stats cover all expected categories.
    let stats = registry::stats(&pool, &state.overlay.lock().unwrap(), &registry, None).unwrap();
    let cats: Vec<Category> = stats.iter().map(|s| s.category).collect();
    assert!(cats.contains(&Category::Entry));
    assert!(cats.contains(&Category::Css) || cats.contains(&Category::Js));

    // Resolve references harvested from a real entry.
    let entry_text = locate_html(&pool, "apple").expect("lookup apple");
    let refs = extract_refs(&entry_text);
    println!("apple entry has {} refs: {refs:?}", refs.len());
    let mut resolved = 0;
    let mut external = 0;
    for r in &refs {
        match resolve(&pool, &registry, r, Some(&ResourceId::Mdx { source: 0, ordinal: 0 })).unwrap() {
            ResolveOutcome::Found { .. } => resolved += 1,
            ResolveOutcome::ExternalRef => external += 1,
            ResolveOutcome::Ambiguous { candidates, .. } => {
                resolved += 1;
                println!("  ambiguous ({}) {r}", candidates.len());
            }
            ResolveOutcome::NotFound => println!("  miss: {r}"),
        }
    }
    println!("resolved={resolved} external={external}");
    assert!(resolved + external > 0, "no refs resolved in real entry");

    // Sample real MDD key shapes (leading slash or not?) and look for keys
    // related to missed references.
    if let mdict_visualizer_lib::state::Source::Mdd(mdd) = &pool.sources[1].source {
        let mut sampled = 0;
        let mut apple_keys = Vec::new();
        for key in mdd.keys().flatten() {
            if sampled < 8 {
                println!("  mdd key sample: {:?}", key.key());
                sampled += 1;
            }
            let lower = key.key().to_lowercase();
            if lower.contains("apple") {
                apple_keys.push(key.key().to_string());
            }
        }
        println!("  keys containing 'apple': {apple_keys:?}");
    }

    // mdres serves a real MDD resource.
    drop(pool);
    drop(registry);
    let resp = mdict_visualizer_lib::handle_mdres_request(&state, "/preview/mdd-1-0/");
    assert!(resp.status() == 200 || resp.status() == 404);
    println!("mdd ordinal 0 status {}", resp.status());
}

#[test]
#[ignore]
fn real_wordnet_index_and_paging() {
    let t0 = Instant::now();
    let state = open_state(&[WORDNET]);
    let elapsed = t0.elapsed();
    let (pool, registry) = (state.pool.lock().unwrap(), state.registry.lock().unwrap());
    let count = pool.sources[0].entry_count();
    println!("WordNet ({count} entries) indexed in {elapsed:?}");
    assert!(count > 100_000);

    // Paging through the whole dictionary must be cheap per page.
    let t1 = Instant::now();
    let mut seen = 0u64;
    let mut offset = 0;
    loop {
        let page = registry::list_resources(
            &pool,
            &state.overlay.lock().unwrap(),
            &registry,
            &ListFilter { source: None, category: None, prefix: "", offset, limit: 100 },
        )
        .unwrap();
        if page.is_empty() {
            break;
        }
        seen += page.len() as u64;
        offset += 100;
        if offset > count + 100 {
            break;
        }
    }
    println!("paged {seen} rows in {:?}", t1.elapsed());
    assert_eq!(seen, count);

    // Prefix query on a real word.
    let rows = registry::list_resources(
        &pool,
        &state.overlay.lock().unwrap(),
        &registry,
        &ListFilter { source: None, category: None, prefix: "comput", offset: 0, limit: 50 },
    )
    .unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r.key.to_lowercase().starts_with("comput")));
}

#[test]
#[ignore]
fn real_longman_large_index() {
    if !have(LONGMAN) {
        return;
    }
    let t0 = Instant::now();
    let state = open_state(&[LONGMAN]);
    let elapsed = t0.elapsed();
    let pool = state.pool.lock().unwrap();
    let count = pool.sources[0].entry_count();
    println!("Longman ({count} entries, ~155MB) indexed in {elapsed:?}");
    assert!(count > 50_000);
    // Generous bound: indexing 155MB should stay in the seconds range.
    assert!(elapsed.as_secs() < 60, "indexing too slow: {elapsed:?}");
}
