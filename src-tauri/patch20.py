s = open('tests/m4_export.rs', encoding='utf-8').read()

# 1) export_edited_mdx_and_reopen
old = """    assert_eq!(report.files.len(), 1, "{:?}", report.files);
    let file = &report.files[0];
    assert!(file.check_ok, "message: {}", file.message);
    assert_eq!(file.entries, 5, "6 minus 1 deleted");"""
new = """    let file = report
        .files
        .iter()
        .find(|f| f.path.ends_with("ocean.mdx"))
        .expect("mdx in edited/");
    assert!(file.check_ok, "message: {}", file.message);
    assert_eq!(file.entries, 5, "6 minus 1 deleted");"""
assert old in s
s = s.replace(old, new)

# 2) export_edited_mdd_with_externals_embedded
old = """    assert_eq!(report.files.len(), 1, "{:?}", report.files);
    let file = &report.files[0];
    assert!(file.check_ok, "message: {}", file.message);
    // 9 - 1 deleted + 2 embedded
    assert_eq!(file.entries, 10);
    assert!(report.skipped_externals.is_empty());

    // PNG resources became JPEG inside the lossy copy.
    let reopened = mdictlib::MddFile::open(&file.path).unwrap();"""
new = """    let file = report
        .files
        .iter()
        .find(|f| f.path.ends_with("assets.mdd"))
        .expect("mdd in edited/");
    assert!(file.check_ok, "message: {}", file.message);
    // 9 - 1 deleted + 2 embedded
    assert_eq!(file.entries, 10);
    assert!(report.skipped_externals.is_empty());

    // PNG resources became JPEG inside the lossy copy.
    let reopened = mdictlib::MddFile::open(&file.path).unwrap();"""
assert old in s
s = s.replace(old, new)

# 3) embed_rebuilds_even_without_edits
old = """    assert_eq!(report.files.len(), 1, "{:?}", report.files);
    assert!(report.files[0].check_ok, "{}", report.files[0].message);
    assert_eq!(report.files[0].entries, 11, "9 original + 2 embedded");
    assert!(report.files[0].path.contains("edited") && report.files[0].path.ends_with("assets.mdd"));"""
new = """    let file = report
        .files
        .iter()
        .find(|f| f.path.ends_with("assets.mdd"))
        .expect("mdd in edited/");
    assert!(file.check_ok, "{}", file.message);
    assert_eq!(file.entries, 11, "9 original + 2 embedded");"""
assert old in s
s = s.replace(old, new)

# 4) default full export + only_edited skip
old = """    let report = export_build(
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
    assert_eq!(report.files.len(), 1);
    assert!(report.files[0].check_ok, "{}", report.files[0].message);
    assert_eq!(report.files[0].entries, 6, "full rebuild without edits");"""
new = """    let report = export_build(
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
    // Complete set: mdx + mdd + both externals, all inside edited/.
    assert_eq!(report.files.len(), 4);
    for f in &report.files {
        assert!(f.path.contains("edited"), "{}", f.path);
    }
    let mdx = report
        .files
        .iter()
        .find(|f| f.path.ends_with("ocean.mdx"))
        .unwrap();
    assert!(mdx.check_ok, "{}", mdx.message);
    assert_eq!(mdx.entries, 6, "full rebuild without edits");"""
assert old in s
s = s.replace(old, new)

old = """    let out2 = tempfile::tempdir().unwrap();
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
    assert!(!report.files[0].check_ok);
    assert!(report.files[0].message.contains("no edits"));"""
new = """    let out2 = tempfile::tempdir().unwrap();
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
    // Nothing has edits — only the externals copy through.
    assert!(report
        .files
        .iter()
        .all(|f| f.message == "copied external"), "{:?}", report.files.iter().map(|f| f.message.clone()).collect::<Vec<_>>());"""
assert old in s
s = s.replace(old, new)

open('tests/m4_export.rs', 'w', encoding='utf-8', newline='\n').write(s)
print('ok')
