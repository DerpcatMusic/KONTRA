//! Browser/chrome receipts use the existing CPU shots renderer, without sample data.
use super::*;
use tests::{Harness, pixels, center};

fn catalog() -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    let mut files: Vec<PathBuf> = (0..16).flat_map(|n| [
        format!("/virtual/Kontakt/Library {n:02}/Instruments/Keys/Piano {n:02}.nki").into(),
        format!("/virtual/Kontakt/Library {n:02}/Multis/Ensemble {n:02}.nkm").into(),
    ]).collect();
    files.extend((0..12).map(|n| PathBuf::from(format!("/virtual/UVI/Falcon Library {n:02}/Bank.ufs/Presets/Keys/Piano {n:02}.uvip"))));
    let libraries = (0..16).map(|n| crate::library::Library {
        dir: format!("/virtual/Kontakt/Library {n:02}").into(), name: format!("Library {n:02}"), instruments: 1, ..Default::default()
    }).chain((0..12).map(|n| crate::library::Library {
        dir: format!("/virtual/UVI/Falcon Library {n:02}").into(), name: format!("Falcon Library {n:02}"), instruments: 1, ..Default::default()
    })).collect();
    let mut v = p.shared.view.lock().unwrap();
    v.shelf = Arc::new(crate::library::Shelf::new(libraries));
    v.files = Arc::new(files);
    v.scanned = p.shared.libraries.wanted();
    // Own fixture artwork: its wide aspect makes cropping visible in receipts.
    for (n, library) in v.shelf.libraries.clone().iter().enumerate() {
        let rgba: Vec<u8> = (0..1024 * 256).flat_map(|i| {
            let x = i % 1024;
            [if x < 64 || x > 960 { 240 } else { 45 }, 75 + (n * 5) as u8, 105, 255]
        }).collect();
        v.artwork.insert(library.name.clone(), Arc::new(moose::mui::mui::scene::Image::rgba(1024, 256, rgba).unwrap()));
    }
    drop(v);
    p
}

fn settings_roots(p: &Arc<SamplerParams>) {
    p.shared.libraries.edit(|settings| settings.roots = vec![
        crate::library::Root { path: "/virtual/Library folders with long names/Kontakt orchestral and performance instruments".into(), single: false },
        crate::library::Root { path: "/virtual/UVI".into(), single: false },
    ]);
    let mut view = p.shared.view.lock().unwrap();
    let mut shelf = crate::library::Shelf::new(view.shelf.libraries.clone());
    shelf.per_root = vec![16, 12];
    view.shelf = Arc::new(shelf);
    view.scanned = p.shared.libraries.wanted();
}

#[test]
fn settings_keeps_the_keyboard_inside_the_minimum_window() {
    let p = catalog();
    settings_roots(&p);
    let mut h = Harness::new(&p, 900., 600.);
    h.press("library-0");
    h.press("app-menu");
    h.press("menu-item-5");
    h.idle(30);
    let keys = h.ui.scene().unwrap().surface("keys").unwrap().frame;
    assert!(keys.y + keys.size.height <= 600.5, "settings must keep the keyboard visible: {keys:?}");
    let toolbar_y = h.ui.scene().unwrap().surface("settings-close").unwrap().frame.y;
    let at = center(&h.ui, "settings-body");
    h.tick(Input { wheel: Vec2::new(0., 1000.), pointer: PointerInput { pos: Some(at), ..Default::default() }, ..Default::default() });
    h.idle(30);
    assert!(h.ui.scroll("settings-body")[1] > 0., "preferences remain reachable by scrolling");
    assert_eq!(h.ui.scene().unwrap().surface("settings-close").unwrap().frame.y, toolbar_y, "settings toolbar stays fixed");
}

#[test]
#[cfg(feature = "shots")]
fn w14_browser_chrome_shots() {
    let Some(out) = std::env::var_os("KONTRA_BROWSER_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for (w, h) in [(1180, 760), (900, 600)] {
        let p = catalog();
        let mut harness = Harness::new(&p, w as f64, h as f64);
        harness.idle(30);
        let save = |name: &str, harness: &mut Harness| {
            // The artwork worker remains off the frame thread.
            harness.settle_art();
            harness.idle(30);
            moose::core::screenshot::save_png(&out.join(format!("{name}-{w}.png")), &pixels(&harness.ui, w, h), w.into(), h.into());
        };
        save("formats", &mut harness);
        harness.press("library-0");
        save("hierarchy", &mut harness);
        harness.press("app-menu");
        save("menu", &mut harness);
        settings_roots(&p);
        harness.press("menu-item-5");
        save("settings", &mut harness);
        harness.press("settings-close");
        {
            let mut selection = p.selection.write().unwrap();
            selection.parts = (0..3).map(|n| Part { path: format!("/virtual/Kontakt/Library 00/Instruments/Piano {n}.nki"),
                name: "Extended performance instrument with a long title".into(), channel: n, ..Default::default() }).collect();
        }
        {
            let mut view = p.shared.view.lock().unwrap();
            for part in view.parts.iter_mut().take(3) { part.instrument = Some(Arc::new(sampler_ir::Instrument::default())); }
        }
        harness.idle(30);
        save("performance", &mut harness);
        harness.press("tab-rack");
        save("rack", &mut harness);
    }
}

#[test]
fn w14_format_tabs_separate_libraries() {
    let p = catalog();
    let mut h = Harness::new(&p, 1180., 760.);
    assert!(h.ui.scene().unwrap().surface("bank-kontakt").is_some(), "Kontakt must have its own tab");
    assert!(h.ui.scene().unwrap().surface("bank-uvi").is_some(), "Falcon/UVI must have its own tab");
    assert_eq!(p.shared.view.lock().unwrap().files.len(), 44);
    assert!(h.ui.scene().unwrap().surface("library-15").is_some());
    h.press("library-0");
    assert!(h.ui.scene().unwrap().surface("folder-0").is_some(), "Instruments category");
    assert!(h.ui.scene().unwrap().surface("folder-3").is_some(), "Multis category, separate from Instruments");
    h.press("bank-uvi");
    assert!(h.ui.scene().unwrap().surface("library-11").is_some());
    assert!(h.ui.scene().unwrap().surface("library-12").is_none(), "Kontakt libraries stay in their tab");
    h.press("library-0");
    assert!(h.ui.scene().unwrap().surface("folder-0").is_some(), "Presets category");
    h.press("bank-kontakt");
    assert!(h.ui.scene().unwrap().surface("folder-3").is_some(), "Kontakt selection is restored independently");
}

#[test]
fn uvi_scan_publication_populates_the_falcon_tab_after_an_empty_start() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = catalog();
        {
            let mut view = p.shared.view.lock().unwrap();
            view.shelf = Arc::new(crate::library::Shelf::new(Vec::new()));
            view.files = Arc::new(Vec::new());
        }
        let mut h = Harness::new(&p, width, height);
        h.press("bank-uvi");
        assert!(h.ui.scene().unwrap().surface("library-0").is_none());
        let bank = PathBuf::from("/virtual/UVI/Published Bank.UFS");
        let preset = bank.join("Piano.uvip");
        {
            let mut view = p.shared.view.lock().unwrap();
            view.shelf = Arc::new(crate::library::Shelf::new(vec![crate::library::Library {
                dir: bank.clone(), name: "Published Falcon bank".into(), instruments: 1,
                ..Default::default()
            }]));
            view.files = Arc::new(vec![preset.clone()]);
        }
        h.idle(5);
        assert!(h.ui.scene().unwrap().surface("library-0").is_some(),
            "a newly published bank appears without reopening the browser");
        h.press("library-0");
        assert!(h.ui.scene().unwrap().surface("instrument-0").is_some(),
            "the scan's bank member is browseable");
        h.press("bank-kontakt");
        assert!(h.ui.scene().unwrap().surface("library-0").is_none(),
            "the UVI bank belongs only to the Falcon tab");
        h.press("bank-uvi");
        assert!(h.ui.scene().unwrap().surface("instrument-0").is_some(),
            "the Falcon selection survives changing tabs");
    }
}

#[test]
fn unsupported_uvi_banks_stay_visible_when_no_library_opens() {
    let p = catalog();
    {
        let mut view = p.shared.view.lock().unwrap();
        let mut shelf = crate::library::Shelf::new(Vec::new());
        view.files = Arc::new(Vec::new());
        shelf.bank_issues.push(crate::library::BankIssue {
            unsupported: true, message: "protected library: not supported".into(),
            locations: (0..26).map(|n| PathBuf::from(format!("/virtual/UVI/Bank{n}.ufs"))).collect(),
        });
        view.shelf = Arc::new(shelf);
    }
    for (width, height) in [(1180., 760.), (900., 600.)] {
        let mut h = Harness::new(&p, width, height);
        h.press("bank-uvi");
        assert!(h.ui.scene().unwrap().surface("uvi-bank-problem").is_some());
        assert!(h.ui.scene().unwrap().surface("uvi-reader-locate").is_none());
        if let Some(out) = std::env::var_os("KONTRA_BROWSER_SHOTS").map(PathBuf::from) {
            std::fs::create_dir_all(&out).unwrap();
            moose::core::screenshot::save_png(&out.join(format!("uvi-unsupported-{width}.png")), &pixels(&h.ui, width as u16, height as u16), width as u32, height as u32);
        }
    }
}

#[test]
fn native_uvi_content_failures_stay_visible_in_the_empty_catalog() {
    let p = catalog();
    {
        let mut view = p.shared.view.lock().unwrap();
        let mut shelf = crate::library::Shelf::new(Vec::new());
        view.files = Arc::new(Vec::new());
        shelf.bank_issues.push(crate::library::BankIssue {
            unsupported: false, message: "invalid PNG checksum".into(),
            locations: vec![PathBuf::from("/virtual/UVI/Corrupt.ufs")],
        });
        view.shelf = Arc::new(shelf);
    }
    let mut h = Harness::new(&p, 900., 600.);
    h.press("bank-uvi");
    assert!(h.ui.scene().unwrap().surface("uvi-bank-problem").is_some());
}

#[test]
fn format_tabs_retain_independent_preset_scroll_positions() {
    let p = catalog();
    {
        let mut view = p.shared.view.lock().unwrap();
        let mut files = (*view.files).clone();
        files.extend((0..100).flat_map(|n| [
            PathBuf::from(format!("/virtual/Kontakt/Library 00/Instruments/Keys/Patch {n:03}.nki")),
            PathBuf::from(format!("/virtual/UVI/Falcon Library 00/Bank.ufs/Presets/Keys/Patch {n:03}.uvip")),
        ]));
        view.files = Arc::new(files);
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("library-0");
    let scroll = |h: &mut Harness, id, y| {
        h.tick(Input { wheel: Vec2::new(0., y), pointer: PointerInput { pos: Some(center(&h.ui, id)), ..Default::default() }, ..Default::default() });
        h.idle(30);
    };
    scroll(&mut h, "browser-list", 1000.);
    let kontakt_y = h.ui.scene().unwrap().surface("instrument-40").unwrap().frame.y;
    h.press("bank-uvi");
    h.press("library-0");
    scroll(&mut h, "browser-list-uvi", 1800.);
    let uvi_y = h.ui.scene().unwrap().surface("instrument-70").unwrap().frame.y;
    h.press("bank-kontakt");
    assert!((h.ui.scene().unwrap().surface("instrument-40").unwrap().frame.y - kontakt_y).abs() < 1.);
    h.press("bank-uvi");
    assert!((h.ui.scene().unwrap().surface("instrument-70").unwrap().frame.y - uvi_y).abs() < 1.);
}

#[test]
fn w14_library_artwork_fills_the_browser_width() {
    for (width, height) in [(1180., 760.), (900., 600.)] {
        let p = catalog();
        let mut h = Harness::new(&p, width, height);
        h.settle_art();
        h.press("library-0");
        h.idle(30);
        let scene = h.ui.scene().unwrap();
        let card = scene.surface("library-0").unwrap().frame;
        assert!(card.size.height > 80., "full-width artwork and its label must fit: {card:?}");
        let viewport = scene.surface("browser-sources-false").unwrap().frame;
        assert!(card.y >= viewport.y - 0.5 && card.y + card.size.height <= viewport.y + viewport.size.height + 0.5,
            "the chosen library panel is fully visible at {width}: {card:?}, {viewport:?}");
        let artwork = scene.surface("library-0-art").unwrap();
        assert!(artwork.disabled, "decorative artwork leaves pointer gestures to its library card");
        let image = artwork.frame;
        assert!(image.size.width > 250.);
        assert!((image.size.width / image.size.height - 4.).abs() < 0.01, "the full artwork keeps its aspect ratio");
    }
}

/// One provider per heavy shard; artwork only, never instrument/sample loading.
#[test]
#[ignore]
#[cfg(feature = "shots")]
fn w14_real_library_art_shots() {
    let Some(provider) = std::env::var("KONTRA_ART_PROVIDER").ok().filter(|p| p == "Kontakt" || p == "UVI") else { return };
    let root = PathBuf::from(format!("/mnt/MAIN_STORAGE/Libraries/{provider}"));
    if !root.is_dir() { return; }
    let p = Arc::new(SamplerParams::new());
    p.shared.libraries.add_root(&root, false);
    let deadline = Instant::now() + Duration::from_secs(220);
    let (generation, scanned) = loop {
        if let Some((generation, Some(scanned))) = p.shared.libraries.poll(0) { break (generation, scanned) }
        assert!(Instant::now() < deadline, "artwork shard exceeded its bound");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(!scanned.shelf.libraries.is_empty());
    println!("ART provider={provider} libraries={} own_panels={}", scanned.shelf.libraries.len(), scanned.artwork.len());
    for (index, library) in scanned.shelf.libraries.iter().enumerate() {
        if scanned.artwork.contains_key(&library.name) { continue; }
        let mut resources = std::collections::BTreeMap::<String, usize>::new();
        // Search the entire library, including sample folders, before any absence verdict.
        for entry in walkdir::WalkDir::new(&library.dir).follow_links(false).into_iter().flatten().filter(|e| e.file_type().is_file()) {
            let extension = entry.path().extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
            if ["png", "jpg", "jpeg", "webp", "bmp", "svg", "nicnt", "nkr", "ufs"].contains(&extension.as_str()) {
                *resources.entry(extension).or_default() += 1;
            }
        }
        println!("ART_SEARCH library_index={index} resources={resources:?}");
    }
    {
        let mut view = p.shared.view.lock().unwrap();
        view.shelf = scanned.shelf;
        view.files = scanned.files;
        view.artwork = scanned.artwork;
        view.scanned = generation;
    }
    let mut harness = Harness::new(&p, 1180., 760.);
    if provider == "UVI" { harness.press("bank-uvi"); }
    harness.settle_art();
    harness.idle(30);
    let out = PathBuf::from(std::env::var_os("KONTRA_BROWSER_SHOTS").expect("receipt directory"));
    std::fs::create_dir_all(&out).unwrap();
    moose::core::screenshot::save_png(&out.join(format!("real-{provider}.png")), &pixels(&harness.ui, 1180, 760), 1180, 760);
    harness.press("library-0");
    harness.idle(30);
    moose::core::screenshot::save_png(&out.join(format!("real-{provider}-selected.png")), &pixels(&harness.ui, 1180, 760), 1180, 760);
}

#[test]
fn oversized_library_panel_reveals_its_top() {
    let p = catalog();
    p.shared.view.lock().unwrap().artwork.insert("Library 00".into(), Arc::new(
        moose::mui::mui::scene::Image::rgba(256, 512, [40, 60, 80, 255].repeat(256 * 512)).unwrap()));
    let mut h = Harness::new(&p, 900., 600.);
    h.press("library-0");
    h.idle(30);
    let scene = h.ui.scene().unwrap();
    let card = scene.surface("library-0").unwrap().frame;
    let viewport = scene.surface("browser-sources-false").unwrap().frame;
    assert!((card.y - viewport.y).abs() < 0.5, "an oversized panel must reveal its top: {card:?}, {viewport:?}");
}

#[test]
fn w10_uvi_bank_file_libraries_appear_in_the_uvi_browser() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut view = p.shared.view.lock().unwrap();
        view.shelf = Arc::new(crate::library::Shelf::new(["Alpha.ufs", "Beta.UFS"].into_iter().map(|bank| {
            crate::library::Library { dir: PathBuf::from("/virtual/UVISoundBanks").join(bank),
                name: bank.into(), instruments: 1, ..Default::default() }
        }).collect()));
        view.files = Arc::new(["Alpha.ufs", "Beta.UFS"].into_iter().map(|bank| {
            PathBuf::from("/virtual/UVISoundBanks").join(bank).join("Presets/Owned.uvip")
        }).collect());
        view.scanned = p.shared.libraries.wanted();
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("bank-uvi");
    assert!(h.ui.scene().unwrap().surface("library-0").is_some());
    assert!(h.ui.scene().unwrap().surface("library-1").is_some());
    h.press("library-0");
    assert!(h.ui.scene().unwrap().surface("instrument-0").is_some());
}

#[test]
fn w10_uvi_unreadable_banks_show_the_root_count_and_cause() {
    let p = Arc::new(SamplerParams::new());
    p.shared.libraries.edit(|s| s.roots = vec![crate::library::Root { path: "/virtual/Owned".into(), single: false }]);
    {
        let mut view = p.shared.view.lock().unwrap();
        let mut shelf = crate::library::Shelf::new(Vec::new());
        shelf.per_root = vec![0];
        shelf.bank_issues.push(crate::library::BankIssue { unsupported: false, message: "UFS header is truncated".into(),
            locations: vec!["/virtual/Owned/A.ufs".into(), "/virtual/Owned/B.ufs".into()] });
        view.shelf = Arc::new(shelf);
        view.scanned = p.shared.libraries.wanted();
    }
    let mut h = Harness::new(&p, 900., 600.);
    h.press("bank-uvi");
    assert!(h.ui.scene().unwrap().surface("uvi-bank-root-0").is_some(), "browser must identify the failed root");
    h.press("app-menu");
    h.press("menu-item-5");
    assert!(h.ui.scene().unwrap().surface("root-bank-problem-0").is_some(), "settings must show the count and cause");
}

#[test]
#[cfg(unix)]
fn w10_uvi_dangling_library_paths_survive_scan_and_show_root_reasons() {
    use crate::library::{Root, Progress};
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("Good/Instruments")).unwrap();
    std::fs::write(root.join("Good/Instruments/Owned.uvip"), b"<UVI4><Program/></UVI4>").unwrap();
    for n in 0..25 {
        symlink(root.join(format!("absent-{n}")), root.join(format!("Stale-{n}"))).unwrap();
    }
    symlink(root.join("absent-bank"), root.join("Dangling.ufs")).unwrap();
    symlink(root.join("Cycle"), root.join("Cycle")).unwrap();
    std::fs::write(root.join("NotFolder"), b"not a library directory").unwrap();
    let roots = vec![
        Root { path: root.to_string_lossy().into_owned(), single: false },
        Root { path: root.join("MissingRoot").to_string_lossy().into_owned(), single: true },
        Root { path: root.join("Cycle").to_string_lossy().into_owned(), single: false },
        Root { path: root.join("NotFolder").to_string_lossy().into_owned(), single: false },
    ];
    let (shelf, files) = crate::library::scan(&roots, &Progress::default()).unwrap();
    assert_eq!(shelf.per_root, [1, 0, 0, 0]);
    assert_eq!(files.len(), 1, "broken paths must not discard the healthy library");
    assert_eq!(shelf.path_issues.len(), 29, "each unavailable path must be retained once");
    assert!(shelf.root_problem(root).unwrap().contains("29 library paths skipped"));
    assert!(shelf.path_issues[&root.join("Dangling.ufs")].contains("Symbolic link cannot be resolved"));
    assert_eq!(shelf.root_problem(&root.join("NotFolder")).as_deref(),
        Some("1 library path skipped: Not a library directory or UFS bank"));
    let p = Arc::new(SamplerParams::new());
    p.shared.libraries.edit(|s| s.roots = roots);
    {
        let mut view = p.shared.view.lock().unwrap();
        view.shelf = Arc::new(shelf);
        view.files = Arc::new(files);
        view.scanned = p.shared.libraries.wanted();
    }
    let mut h = Harness::new(&p, 900., 600.);
    h.press("bank-uvi");
    assert!(h.ui.scene().unwrap().surface("uvi-bank-root-0").is_some(), "dangling entries must have a visible per-root reason");
    h.press("app-menu");
    h.press("menu-item-5");
    for n in 0..4 {
        assert!(h.ui.scene().unwrap().surface(&format!("root-bank-problem-{n}")).is_some(), "root {n} must report its skipped paths");
    }
}

#[test]
#[cfg(feature = "library-access")]
fn w10_uvi_catalogued_protected_programs_keep_rows_and_show_bank_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let bank = tmp.path().join("Owned.ufs");
    crate::library::tests::clear_bank(&bank);
    let mut bytes = std::fs::read(&bank).unwrap();
    let member = bytes.windows(4).position(|x| x == 0x675850e4u32.to_le_bytes()).unwrap();
    bytes[member + 276] = 2;
    std::fs::write(&bank, bytes).unwrap();
    assert!(sampler_uvi::Bank::open(&bank).is_err(), "authored bank is catalogued but cannot load");
    let roots = vec![crate::library::Root { path: tmp.path().to_string_lossy().into_owned(), single: false }];
    let (shelf, files) = crate::library::scan(&roots, &crate::library::Progress::default()).unwrap();
    assert_eq!((shelf.libraries.len(), files.len()), (1, 1));
    assert_eq!(shelf.bank_status.get(&bank).map(String::as_str), Some("1 of 1 presets need content access before they can load."));
    let p = Arc::new(SamplerParams::new());
    p.shared.libraries.edit(|s| s.roots = roots);
    {
        let mut view = p.shared.view.lock().unwrap();
        view.shelf = Arc::new(shelf);
        view.files = Arc::new(files);
        view.scanned = p.shared.libraries.wanted();
    }
    for (width, height) in [(900., 600.), (1180., 760.)] {
        let mut h = Harness::new(&p, width, height);
        h.press("bank-uvi");
        h.press("library-0");
        h.idle(30);
        assert!(h.ui.scene().unwrap().surface("instrument-0").is_some(), "catalogued preset stays visible");
        let status = h.ui.scene().unwrap().surface("uvi-bank-status-0");
        assert!(status.is_some(), "selected bank must show the access reason beside its presets");
        let status = status.unwrap().frame;
        let badge = h.ui.scene().unwrap().surface("library-0-status").unwrap().frame;
        let viewport = h.ui.scene().unwrap().surface("browser-sources-true").unwrap().frame;
        assert!(badge.y + badge.size.height <= viewport.y + viewport.size.height + 0.5, "library status must stay visible: {badge:?} {viewport:?}");
        assert!(status.size.height > 0. && status.y + status.size.height < height, "status must fit: {status:?}");
        #[cfg(feature = "shots")]
        if let Some(out) = std::env::var_os("KONTRA_UVI_STATUS_SHOTS").map(PathBuf::from) {
            std::fs::create_dir_all(&out).unwrap();
            h.settle_art();
            h.idle(30);
            moose::core::screenshot::save_png(&out.join(format!("bank-status-{width:.0}.png")),
                &pixels(&h.ui, width as u16, height as u16), width as u32, height as u32);
        }
    }
}

#[test]
fn w10_uvi_search_and_favorites_keep_the_bank_member_identity() {
    let p = Arc::new(SamplerParams::new());
    let tmp = tempfile::tempdir().unwrap();
    for (bank, color) in [("Alpha", [45, 110, 190, 255]), ("Beta", [160, 90, 60, 255])] {
        let path = tmp.path().join(format!("{bank}.ufs"));
        std::fs::write(&path, b"artwork uses only the named cover").unwrap();
        let mut encoder = png::Encoder::new(std::fs::File::create(path.with_extension("png")).unwrap(), 512, 128);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&color.repeat(512 * 128)).unwrap();
    }
    let alpha = tmp.path().join("Alpha.ufs/preset.uvip");
    let beta = tmp.path().join("Beta.ufs/preset.uvip");
    {
        let mut view = p.shared.view.lock().unwrap();
        view.shelf = Arc::new(crate::library::Shelf::new(["Alpha", "Beta"].into_iter().map(|name| {
            crate::library::Library { dir: tmp.path().join(format!("{name}.ufs")), name: name.into(), instruments: 1, ..Default::default() }
        }).collect()));
        view.artwork = crate::artwork::scan(&view.shelf.libraries);
        assert_eq!(view.artwork.len(), 2);
        view.files = Arc::new(vec![alpha.clone(), beta.clone()]);
        view.scanned = p.shared.libraries.wanted();
    }
    let mut h = Harness::new(&p, 900., 600.);
    h.press("bank-uvi");
    h.press("library-0");
    h.press("star-0");
    assert_eq!(p.selection.read().unwrap().favorites, [alpha.to_string_lossy()]);
    h.press("library-1");
    h.press("star-0");
    assert_eq!(p.selection.read().unwrap().favorites.len(), 2, "same-named presets in different banks stay distinct");
    h.press("source-favorites");
    #[cfg(feature = "shots")]
    if let Some(out) = std::env::var_os("KONTRA_UVI_FIXTURE_SHOTS").map(PathBuf::from) {
        std::fs::create_dir_all(&out).unwrap();
        h.idle(30);
        moose::core::screenshot::save_png(&out.join("uvi-favorites-900.png"), &pixels(&h.ui, 900, 600), 900, 600);
    }
    assert!(h.ui.scene().unwrap().surface("instrument-1").is_some());
    h.ui.focus("search");
    h.tick(Input { text: "UVI preset".into(), ..Default::default() });
    h.idle(3);
    assert!(h.ui.scene().unwrap().surface("instrument-0").is_some());
    assert!(h.ui.scene().unwrap().surface("instrument-1").is_some(), "v1 searches the UVI provider name as well as preset/folder names");
    #[cfg(feature = "shots")]
    if let Some(out) = std::env::var_os("KONTRA_UVI_FIXTURE_SHOTS").map(PathBuf::from) {
        moose::core::screenshot::save_png(&out.join("uvi-search-900.png"), &pixels(&h.ui, 900, 600), 900, 600);
    }
    h.press("star-1");
    assert_eq!(p.selection.read().unwrap().favorites, [alpha.to_string_lossy()], "search must unstar the matched full bank/member path");
    use moose::core::custom_state::State;
    let selection = p.selection.read().unwrap();
    let restored = crate::plugin::Selection::deserialize(&selection.serialize()).unwrap();
    assert_eq!(restored.favorites, selection.favorites, "favourites retain bank/member identity after state reload");
}
