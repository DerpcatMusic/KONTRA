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
