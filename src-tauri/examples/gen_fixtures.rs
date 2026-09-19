//! Generates test fixtures into `fixtures/` for GUI testing.
//!
//! Run: cargo run --example gen_fixtures

fn main() {
    let root = std::path::Path::new("fixtures");
    let (mdx, mdd, css, js) = mdict_visualizer_lib::fixtures::write_all(root);
    let size = |p: &str| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    println!(
        "fixtures written: {} ({}B), {} ({}B), {}, {}",
        mdx,
        size(&mdx),
        mdd,
        size(&mdd),
        css,
        js
    );
}
