//! Stage timing benchmark against real dictionaries in C:\\Dictionaries.
//! Run: cargo run --release --example bench_stages -- <mdx> <mdd>
//!
//! Times: index build, full entry walk (mdx), full resource walk (mdd),
//! webp/quantize processing of every image, plain rebuild (no lossy), and
//! lossy rebuild — the stages a real export consists of.

use std::time::Instant;

use mdict_editor_lib::processors::Processor;
use mdict_editor_lib::state::SourcePool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mdx = args.get(1).map(|s| s.as_str()).unwrap_or(
        r"C:\Dictionaries\AHD美语传统双解词典（紧凑优雅版）\AHD双解.mdx",
    );
    let mdd = args.get(2).map(|s| s.as_str()).unwrap_or(
        r"C:\Dictionaries\AHD美语传统双解词典（紧凑优雅版）\AHD双解.mdd",
    );

    let t = Instant::now();
    let mut pool = SourcePool::default();
    let entry = SourcePool::open_path(mdx).expect("open mdx");
    let n_entries = entry.entry_count();
    pool.sources.push(entry);
    let entry = SourcePool::open_path(mdd).expect("open mdd");
    let n_res = entry.entry_count();
    pool.sources.push(entry);
    println!("open                : {:>8.2?}  ({} entries, {} resources)", t.elapsed(), n_entries, n_res);

    let t = Instant::now();
    let mut registry = mdict_editor_lib::registry::Registry::default();
    mdict_editor_lib::registry::ensure_built(&pool, &mut registry).unwrap();
    println!("index build         : {:>8.2?}", t.elapsed());

    // mdx full walk (decompress every entry)
    let t = Instant::now();
    let mut html_bytes = 0u64;
    {
        let mdict_editor_lib::state::Source::Mdx(file) = &pool.sources[0].source else {
            unreachable!()
        };
        for e in file.entries() {
            if let Ok(e) = e {
                html_bytes += e.text().len() as u64;
            }
        }
    }
    println!("mdx full walk       : {:>8.2?}  ({:.1} MB html)", t.elapsed(), html_bytes as f64 / 1048576.0);

    // mdd full walk + count images
    let t = Instant::now();
    let mut images: Vec<(String, Vec<u8>)> = Vec::new();
    {
        let mdict_editor_lib::state::Source::Mdd(file) = &pool.sources[1].source else {
            unreachable!()
        };
        for r in file.resources() {
            if let Ok(r) = r {
                let lower = r.key().to_lowercase();
                if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".gif") || lower.ends_with(".bmp") {
                    images.push((r.key().to_string(), r.bytes().to_vec()));
                }
            }
        }
    }
    let img_total: u64 = images.iter().map(|(_, b)| b.len() as u64).sum();
    println!(
        "mdd full walk       : {:>8.2?}  ({} images, {:.1} MB)",
        t.elapsed(),
        images.len(),
        img_total as f64 / 1048576.0
    );

    // webp q75 on every image (serial)
    let t = Instant::now();
    let mut webp_out = 0u64;
    let mut ok = 0u64;
    for (key, bytes) in &images {
        let webp_step = Processor::ImgWebp { quality: Some(75) };
        if let Ok(out) = webp_step.process(key, bytes, &Default::default()) {
            webp_out += out.len() as u64;
            ok += 1;
        }
    }
    println!(
        "webp q75 (serial)   : {:>8.2?}  ({}/{} ok, {:.1} MB -> {:.1} MB)",
        t.elapsed(),
        ok,
        images.len(),
        img_total as f64 / 1048576.0,
        webp_out as f64 / 1048576.0
    );

    // quantize on every png (serial)
    let t = Instant::now();
    let mut q_out = 0u64;
    let mut qok = 0u64;
    for (key, bytes) in &images {
        if key.to_lowercase().ends_with(".png") {
            let quant_step = Processor::PngQuantize { colors: Some(256) };
            if let Ok(out) = quant_step.process(key, bytes, &Default::default()) {
                q_out += out.len() as u64;
                qok += 1;
            }
        }
    }
    println!(
        "quantize (serial)   : {:>8.2?}  ({}/{} ok, -> {:.1} MB)",
        t.elapsed(),
        qok,
        images.len(),
        q_out as f64 / 1048576.0
    );

    // plain mdd rebuild (no lossy)
    let t = Instant::now();
    let out_dir = std::env::temp_dir().join("mdict-bench-plain");
    let _ = std::fs::create_dir_all(&out_dir);
    let overlay = mdict_editor_lib::state::Overlay::default();
    let config = mdict_editor_lib::export::ExportConfig {
        out_dir: out_dir.display().to_string(),
        mdd: true,
        only_edited: false,
        ..Default::default()
    };
    let report = mdict_editor_lib::export::export_build(&pool, &overlay, &config, &no_ctl()).unwrap();
    println!("plain rebuild        : {:>8.2?}  {:?}", t.elapsed(), report.files.iter().map(|f| f.bytes).collect::<Vec<_>>());

    // lossy rebuild (webp+quantize+minify)
    let t = Instant::now();
    let out_dir2 = std::env::temp_dir().join("mdict-bench-lossy");
    let _ = std::fs::create_dir_all(&out_dir2);
    let config = mdict_editor_lib::export::ExportConfig {
        out_dir: out_dir2.display().to_string(),
        mdd: true,
        only_edited: false,
        lossy: Some(vec![
            Processor::ImgWebp { quality: Some(75) },
            Processor::PngQuantize { colors: Some(256) },
            Processor::MinifyCss,
            Processor::MinifyJs,
        ]),
        ..Default::default()
    };
    let report = mdict_editor_lib::export::export_build(&pool, &overlay, &config, &no_ctl()).unwrap();
    println!("lossy rebuild        : {:>8.2?}  {:?}", t.elapsed(), report.files.iter().map(|f| f.bytes).collect::<Vec<_>>());
    println!("cpus                 : {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0));
}

use mdict_editor_lib::pipeline::no_ctl;
