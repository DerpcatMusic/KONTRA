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

#[test]
fn inside_uses_the_shared_panel_inset() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        let mut h = Harness::new(&p, width, height);
        h.press("view-0-Mapping");
        let scene = h.ui.scene().unwrap();
        let inside = scene.surface("inside-0").unwrap().frame;
        let map = scene.surface("map-0").unwrap().frame;
        let right = inside.x + inside.size.width - INSET;
        assert!(
            (map.x + map.size.width - right).abs() < 0.5,
            "inside content uses the shared panel inset: {map:?} in {inside:?}"
        );
    }
}

#[test]
fn mapping_keeps_full_group_names_in_tooltips() {
    let p = specimen();
    let mut h = Harness::new(&p, 900., 600.);
    h.press("view-0-Mapping");
    let group = h.ui.scene().unwrap().surface("map-group-0-0").unwrap();
    assert!(
        group
            .tip
            .as_deref()
            .is_some_and(|tip| tip.ends_with("extended description")),
        "truncated group names remain inspectable"
    );
}

#[test]
fn keyboard_toolbar_and_keybed_share_the_right_inset() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        let h = Harness::new(&p, width, height);
        let scene = h.ui.scene().unwrap();
        let toggle = scene.surface("keyboard-toggle").unwrap().frame;
        let keys = scene.surface("keys").unwrap().frame;
        assert!(
            (toggle.x + toggle.size.width - keys.x - keys.size.width).abs() < 0.5,
            "keyboard controls and keys share their inset: {toggle:?}, {keys:?}"
        );
    }
}

fn trigger_conflict(h: &mut Harness, p: &Arc<SamplerParams>) {
    let inst = p.shared.view.lock().unwrap().parts[0]
        .instrument
        .clone()
        .unwrap();
    let ids = crate::sound::articulation::identities(&inst.articulations);
    h.type_into(&format!("{}-trigger", inside::row_id(0, &ids[0])), "25");
}

fn generated_specimen(p: &Arc<SamplerParams>) {
    use sampler_ui_ir as ir;
    let mut face = ir::Interface {
        pages: vec![ir::Page {
            name: "Extended performance editor page".into(),
            size: ir::Size {
                width: 700,
                height: 240,
            },
            ..Default::default()
        }],
        ..Default::default()
    };
    for n in 0..8 {
        let mut knob = ir::Widget::new(
            format!("$parameter_{n}"),
            ir::PageRef(0),
            ir::Rect::new(n * 68, 32, 60, 64),
            ir::Kind::Knob {
                range: ir::Range::default(),
                display: ir::Display::default(),
            },
        );
        knob.text = format!("Extended parameter caption {n}");
        face.widgets.push(knob);
    }
    p.shared.view.lock().unwrap().parts[0].interfaces = Arc::from([face]);
    p.selection.write().unwrap().parts[0].view = 2;
}

#[test]
#[cfg(feature = "shots")]
fn chrome_extended_shots() {
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
        trigger_conflict(&mut h, &p);
        save("trigger-conflict", &mut h);
        h.press("art-cancel-0");
        let ids = crate::sound::articulation::identities(
            &p.shared.view.lock().unwrap().parts[0]
                .instrument
                .as_ref()
                .unwrap()
                .articulations,
        );
        h.type_into(
            &format!("{}-trigger", inside::row_id(0, &ids[0])),
            "invalid",
        );
        save("trigger-error", &mut h);
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.press("app-menu");
        h.press("menu-item-5");
        let at = tests::center(&h.ui, "settings-body");
        h.tick(Input {
            wheel: Vec2::new(0., 1000.),
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            ..Default::default()
        });
        h.idle(30);
        save("settings-controls", &mut h);
        h.press("settings-close");
        generated_specimen(&p);
        h.press("view-0-Interface");
        save("generated-editor", &mut h);
    }
}

#[test]
fn trigger_conflicts_keep_fixed_control_labels_readable() {
    let widths = |width, height| {
        let p = specimen();
        let mut h = Harness::new(&p, width, height);
        trigger_conflict(&mut h, &p);
        let scene = h.ui.scene().unwrap();
        ["art-swap-0", "art-driver-0"].map(|id| scene.surface(id).unwrap().frame.size.width)
    };
    assert_eq!(
        widths(900., 600.),
        widths(1180., 900.),
        "Swap and trigger mode retain their text width during long-name conflicts"
    );
}
