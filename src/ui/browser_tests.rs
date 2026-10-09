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
