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
        assert!((card.size.height - (card.size.width - 2. * TIGHT) / browser::BANNER_RATIO - 2. * TIGHT).abs() < 0.5, "the standard banner and its padding fill the card: {card:?}");
        let viewport = scene.surface("browser-sources-false").unwrap().frame;
        assert!(card.y >= viewport.y - 0.5 && card.y + card.size.height <= viewport.y + viewport.size.height + 0.5,
            "the chosen library panel is fully visible at {width}: {card:?}, {viewport:?}");
        let artwork = scene.surface("library-0-art").unwrap();
        assert!(artwork.disabled, "decorative artwork leaves pointer gestures to its library card");
        let image = artwork.frame;
        assert!(image.size.width > 250.);
        assert!((image.size.width / image.size.height - browser::BANNER_RATIO).abs() < 0.01, "cover-cropping keeps the standard banner ratio");
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
fn portrait_library_art_uses_the_standard_banner_height() {
    let p = catalog();
    p.shared.view.lock().unwrap().artwork.insert("Library 00".into(), Arc::new(
        moose::mui::mui::scene::Image::rgba(256, 512, [40, 60, 80, 255].repeat(256 * 512)).unwrap()));
    let mut h = Harness::new(&p, 900., 600.);
    h.press("library-0");
    h.idle(30);
    let scene = h.ui.scene().unwrap();
    let card = scene.surface("library-0").unwrap().frame;
    let viewport = scene.surface("browser-sources-false").unwrap().frame;
    assert!(card.y >= viewport.y - 0.5 && card.y + card.size.height <= viewport.y + viewport.size.height + 0.5, "the selected portrait artwork fits its banner: {card:?}, {viewport:?}");
    let art = scene.surface("library-0-art").unwrap().frame;
    assert!((art.size.width / art.size.height - browser::BANNER_RATIO).abs() < 0.01);
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

fn redesign_catalog(real_art: bool) -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    let kontakt = PathBuf::from("/mnt/MAIN_STORAGE/Libraries/Kontakt");
    let libraries = vec![
        crate::library::Library { dir: kontakt.join("Afflatus Chapter II Brass"), name: "Afflatus Chapter II Brass".into(), instruments: 2, ..Default::default() },
        crate::library::Library { dir: kontakt.join("Areia 1.2.0 [Audio Imperia]"), name: "Areia".into(), vendor: "Audio Imperia".into(), instruments: 2, ..Default::default() },
        crate::library::Library { dir: "/virtual/UVI/Analog".into(), name: "Analog".into(), instruments: 1, ..Default::default() },
    ];
    let files = vec![
        libraries[0].dir.join("Instruments/Brass 01.nki"),
        libraries[0].dir.join("Instruments/Brass 02.nki"),
        libraries[1].dir.join("Instruments/Strings 01.nki"),
        libraries[1].dir.join("Instruments/Strings 02.nki"),
        libraries[2].dir.join("Bank.ufs/Presets/Pad.uvip"),
    ];
    let mut shelf = crate::library::Shelf::new(libraries.clone());
    shelf.snapshots.insert(files[0].clone(), crate::library::Snapshots {
        instrument: "Brass 01".into(), paths: vec![libraries[0].dir.join("Snapshots/Warm/Warm preset.nksn")]
    });
    let mut view = p.shared.view.lock().unwrap();
    view.shelf = Arc::new(shelf);
    view.files = Arc::new(files);
    view.scanned = p.shared.libraries.wanted();
    for (library, (w, h)) in libraries.iter().zip([(905, 99), (521, 98), (256, 512)]) {
        view.artwork.insert(library.name.clone(), Arc::new(moose::mui::mui::scene::Image::rgba(w, h,
            [70, 95, 130, 255].repeat((w * h) as usize)).unwrap()));
    }
    if real_art {
        let art = crate::artwork::scan(&libraries[..2]);
        assert_eq!(art.len(), 2, "both official library banners must resolve");
        view.artwork.extend(art);
    }
    drop(view);
    p
}

#[test]
fn smart_browser_has_one_header_search_and_funnel() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = redesign_catalog(false);
        let mut h = Harness::new(&p, width, height);
        let scene = h.ui.scene().unwrap();
        assert!(scene.surface("library-filter").is_none() && scene.surface("library-sort").is_none());
        let browser = scene.surface("browser").unwrap().frame;
        let search = scene.surface("search").unwrap().frame;
        let funnel = scene.surface("browser-filter").expect("shared funnel menu").frame;
        assert!((search.y - browser.y - SPACE).abs() < 0.5, "search leads the browser");
        assert!(search.x + search.size.width <= funnel.x + 0.5);
        assert!(scene.surface("browser-sort-label").unwrap().frame.size.height < search.size.height);
        h.press("browser-filter");
        h.press("menu-item-2");
        assert_eq!(p.shared.libraries.settings().sort, crate::library::Sort::Recent);
        for (item, count) in [("menu-item-7", "0"), ("menu-item-6", "5"), ("menu-item-8", "0"), ("menu-item-6", "5")] {
            h.press("browser-filter");
            h.press(item);
            assert_eq!(h.ui.scene().unwrap().surface("browser-count").unwrap().text_value.as_deref(), Some(count),
                "the funnel switches the preset source");
        }
        h.ui.focus("search");
        h.tick(Input { text: "Areia".into(), ..Default::default() });
        h.idle(3);
        for _ in 0..2 {
            h.tick(Input { keys: vec![KeyPress { key: Key::Char('f'), mods: Mods { ctrl: true, ..Default::default() } }], ..Default::default() });
            h.idle(2);
            assert_eq!(h.ui.focus_key(), Some("search"), "Ctrl+F always addresses the single query");
        }
        h.tick(Input { keys: vec![KeyPress { key: Key::Tab, mods: Mods::default() }], ..Default::default() });
        h.idle(2);
        assert_eq!(h.ui.focus_key(), Some("search-clear"), "the clear action remains keyboard accessible");
        h.tick(Input { keys: vec![KeyPress { key: Key::Tab, mods: Mods::default() }], ..Default::default() });
        h.idle(2);
        assert_eq!(h.ui.focus_key(), Some("browser-filter"), "the funnel follows the search controls in the tab order");
    }
}

#[test]
fn smart_browser_query_matches_library_vendor_preset_folder_and_nksn() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        for (query, expected, path) in [
            ("Areia Strings 02", "library-1", "Strings 02.nki"),
            ("Audio Imperia 01", "library-1", "Strings 01.nki"),
            ("Afflatus Brass 01", "library-0", "Brass 01.nki"),
            ("Warm preset", "library-0", "Warm preset.nksn"),
        ] {
            let p = redesign_catalog(false);
            let mut h = Harness::new(&p, width, height);
            h.ui.focus("search");
            h.tick(Input { text: query.into(), ..Default::default() });
            h.idle(3);
            h.idle(20);
            let scene = h.ui.scene().unwrap();
            assert!(scene.surface(expected).is_some(), "matching library stays visible for {query}");
            let other = if expected == "library-0" { "library-1" } else { "library-0" };
            assert!(scene.surface(other).is_none(), "unrelated library is filtered by {query}");
            let item = scene.surface("instrument-0").expect("matching preset");
            assert!(item.tip.as_deref().is_some_and(|tip| tip.contains(path)), "smart search preserves source identity for {query}: {:?}", item.tip);
        }
    }
}

#[test]
fn library_cards_use_one_banner_ratio_with_the_name_and_count_inside() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = redesign_catalog(false);
        let mut h = Harness::new(&p, width, height);
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        let mut heights = Vec::new();
        assert_eq!(scene.surface("browser-count").unwrap().text_value.as_deref(), Some("5"), "the provider count includes native presets");
        assert_eq!(scene.surface("library-0-count").unwrap().text_value.as_deref(), Some("3"));
        for (n, label) in [(0, "Afflatus Chapter II Brass"), (1, "Areia")] {
            let card = scene.surface(&format!("library-{n}")).unwrap().frame;
            let art = scene.surface(&format!("library-{n}-art")).unwrap();
            let name = scene.surface(&format!("library-{n}-name")).expect("literal banner label");
            let count = scene.surface(&format!("library-{n}-count")).unwrap().frame;
            assert_eq!(name.text_value.as_deref(), Some(label));
            assert!((art.frame.size.width / art.frame.size.height - 905. / 99.).abs() < 0.01);
            assert!(art.disabled, "visual artwork does not consume card pointer gestures");
            for frame in [name.frame, count] {
                assert!(frame.y >= art.frame.y - 0.5 && frame.y + frame.size.height <= art.frame.y + art.frame.size.height + 0.5,
                    "labels live inside the banner");
            }
            assert!((card.size.height - art.frame.size.height - TIGHT * 2.).abs() < 0.5, "no separate name row");
            heights.push(card.size.height);
        }
        assert_eq!(heights[0], heights[1], "different official artwork dimensions use equal cards");
    }
}

#[test]
fn empty_browser_uses_only_the_plus_action_and_presets_are_named_presets() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = Arc::new(SamplerParams::new());
        let h = Harness::new(&p, width, height);
        assert!(h.ui.scene().unwrap().surface("empty-add-many").is_none());
        assert!(h.ui.scene().unwrap().surface("empty-add-one").is_none());
        assert!(h.ui.scene().unwrap().surface("libraries-add").is_some());
        let p = redesign_catalog(false);
        let mut h = Harness::new(&p, width, height);
        h.press("library-0");
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        assert!(scene.surfaces().any(|s| s.text_value.as_deref() == Some("Presets")));
        assert!(!scene.surfaces().any(|s| s.text_value.as_deref() == Some("Snapshots")));
        h.press("folder-4");
        assert_eq!(h.ui.scene().unwrap().surface("crumb-0").unwrap().tip.as_deref(), Some("Back to Presets"));
        h.press("crumb-0");
        assert!(h.ui.scene().unwrap().surface("crumb-0").is_none(), "the renamed crumb returns to the actual preset category");
    }
}

#[test]
#[cfg(feature = "shots")]
fn browser_redesign_shots() {
    let Some(out) = std::env::var_os("KONTRA_REDESIGN_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 900)] {
        let real_art = std::env::var_os("KONTRA_REDESIGN_REAL_ART").is_some();
        let p = redesign_catalog(real_art);
        let mut h = Harness::new(&p, width as f64, height as f64);
        let save = |name: &str, h: &mut Harness| {
            h.idle(30);
            moose::core::screenshot::save_png(&out.join(format!("{name}-{width}.png")), &pixels(&h.ui, width, height), width.into(), height.into());
        };
        save("libraries", &mut h);
        h.ui.focus("search");
        h.tick(Input { text: "Areia Strings 02".into(), ..Default::default() });
        h.idle(3);
        save("search", &mut h);
        h.ui.focus("search");
        h.tick(Input { keys: vec![KeyPress { key: Key::Escape, mods: Mods::default() }], ..Default::default() });
        h.idle(3);
        h.press("library-0");
        save("presets", &mut h);
        let sort = if h.ui.scene().unwrap().surface("library-sort").is_some() { "library-sort" } else { "browser-filter" };
        h.press(sort);
        save("sort", &mut h);
        let p = Arc::new(SamplerParams::new());
        let mut h = Harness::new(&p, width as f64, height as f64);
        save("empty", &mut h);
    }
}
