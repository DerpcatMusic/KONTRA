//! Player chrome receipts: synthetic IR only, never a library's Original UI.
use super::*;
use tests::{Harness, pixels};

fn specimen() -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    let mut inst = sampler_ir::Instrument {
        name: "Extended performance strings with a deliberately long instrument title".into(),
        ..Default::default()
    };
    for n in 0..12 {
        let name = format!(
            "{n:02} Long articulation and microphone position with an extended description"
        );
        inst.groups.push(sampler_ir::Group {
            name: name.clone(),
            ..Default::default()
        });
        inst.articulations.push(sampler_ir::Articulation {
            name,
            switch_keys: vec![24 + n as u8],
            default: n == 0,
            ..Default::default()
        });
        let mut z = sampler_ir::Zone::new(sampler_ir::AssetRef(0));
        z.group = Some(sampler_ir::GroupRef(n));
        z.keys = sampler_ir::KeyRange { low: 36, high: 84 };
        inst.zones.push(z);
    }
    inst.host_volume = Some(sampler_ir::HostVolume {
        controller: 7,
        saved: 0.5,
    });
    p.selection.write().unwrap().parts = (0..3).map(|n| Part {
        path: format!("/virtual/Extremely long library folder path/Instruments/Extended performance strings {n}.nki"),
        name: inst.name.clone(), channel:n, collapsed:n>0, ..Default::default()
    }).collect();
    let mut v = p.shared.view.lock().unwrap();
    v.scanned = p.shared.libraries.wanted();
    for part in v.parts.iter_mut().take(3) {
        part.active = inst.name.clone();
        part.instrument = Some(Arc::new(inst.clone()));
        let mut report = crate::sound::report::LoadReport::default();
        report.decoded.format = "Kontakt synthetic IR: an extended format description".into();
        report.decoded.dynamics = vec![(1, 0), (11, 127)];
        report.decoded.needs_controller = true;
        part.report = Some(Arc::new(report));
    }
    drop(v);
    p
}

#[test]
fn performance_dynamics_fit_the_minimum_rack() {
    let p = specimen();
    let h = Harness::new(&p, 900., 600.);
    let scene = h.ui.scene().unwrap();
    let strip = scene.surface("perf-0").unwrap().frame;
    let last = scene.surface("dyn-0-127").unwrap().frame;
    assert!(
        last.x + last.size.width <= strip.x + strip.size.width - TIGHT + 0.5,
        "last dynamics control must fit: {last:?} in {strip:?}"
    );
}

#[test]
fn performance_keeps_numeric_volume_readable() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        let h = Harness::new(&p, width, height);
        let volume = h.ui.scene().unwrap().surface("perf-vol-0").unwrap().frame;
        assert!(
            volume.size.width >= TEXT * 9.,
            "Volume and CC7 -6.0 dB must fit: {volume:?}"
        );
    }
}

#[test]
fn performance_warning_stays_on_one_line() {
    let p = specimen();
    let h = Harness::new(&p, 1180., 900.);
    let warning = h.ui.scene().unwrap().surface("perf-needs-0").unwrap().frame;
    assert!(
        warning.size.height <= CONTROL,
        "controller warning shares the control baseline: {warning:?}"
    );
}

#[test]
#[cfg(feature = "shots")]
fn chrome_audit_shots() {
    let Some(out) = std::env::var_os("KONTRA_CHROME_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for (w, height) in [(900, 600), (1180, 900)] {
        let p = specimen();
        let mut h = Harness::new(&p, w as f64, height as f64);
        let save = |name: &str, h: &mut Harness| {
            h.idle(20);
            moose::core::screenshot::save_png(
                &out.join(format!("{name}-{w}.png")),
                &pixels(&h.ui, w, height),
                w.into(),
                height.into(),
            );
        };
        save("rack", &mut h);
        h.press("header-0");
        h.press("view-0-Info");
        save("info", &mut h);
        h.press("view-0-Sound");
        save("sound", &mut h);
        h.press("view-0-Mapping");
        save("mapping", &mut h);
        h.press("view-0-Articulations");
        save("articulations", &mut h);
        h.press("art-driver-0");
        save("trigger-menu", &mut h);
        h.press("menu-item-2");
        save("articulations-channel", &mut h);
        h.press("qwerty");
        save("keyboard", &mut h);
        h.press("more-0");
        save("part-menu", &mut h);
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.press("midi-0");
        save("midi-menu", &mut h);
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.press("output-0");
        save("output-menu", &mut h);
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.press("app-menu");
        save("app-menu", &mut h);
        h.press("menu-item-5");
        save("settings", &mut h);
        h.press("settings-close");
        h.press("app-menu");
        h.press("menu-item-8");
        save("save-multi", &mut h);
        if h.ui.scene().unwrap().surface("multi-save").is_some() {
            h.press("multi-save");
            save("save-error", &mut h);
            h.press("multi-close");
        }
        h.press("app-menu");
        h.press("menu-item-3");
        save("about", &mut h);
    }
}
