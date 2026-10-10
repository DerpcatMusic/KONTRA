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
fn info_starts_with_the_file_and_keeps_the_name_in_the_header_tooltip() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        let mut h = Harness::new(&p, width, height);
        h.press("view-0-Info");
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        let inside = scene.surface("inside-0").unwrap().frame;
        let file = scene.surface("info-0-File").unwrap();
        assert!(
            (file.frame.y - inside.y - INSET).abs() < 0.5,
            "the file is the first fact, without a repeated name row: {:?} in {inside:?}",
            file.frame
        );
        assert!(scene.surface("info-0-Instrument").is_none());
        let part = &p.selection.read().unwrap().parts[0];
        assert_eq!(file.tip.as_deref(), Some(part.path.as_str()));
        assert!(
            scene
                .surface("name-0")
                .unwrap()
                .tip
                .as_deref()
                .unwrap()
                .contains(&part.name)
        );
        let last = scene.surface("info-0-Keyswitches").unwrap().frame;
        assert!(
            last.y + last.size.height <= inside.y + inside.size.height - INSET + 0.5,
            "the last fact stays inside the panel inset"
        );
    }
}

#[test]
#[cfg(feature = "shots")]
fn info_distill_shots() {
    let Some(out) = std::env::var_os("KONTRA_INFO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 900)] {
        let p = specimen();
        let mut h = Harness::new(&p, width as f64, height as f64);
        h.press("view-0-Info");
        h.idle(20);
        moose::core::screenshot::save_png(
            &out.join(format!("info-{width}.png")),
            &pixels(&h.ui, width, height),
            width.into(),
            height.into(),
        );
    }
}

#[test]
fn articulation_count_starts_at_the_panel_inset_without_a_second_title() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        let mut h = Harness::new(&p, width, height);
        h.press("view-0-Articulations");
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        let inside = scene.surface("inside-0").unwrap().frame;
        let count = scene
            .surface("art-count-0")
            .expect("the count remains visible");
        assert!(
            (count.frame.x - inside.x - INSET).abs() < 0.5,
            "the repeated Articulations heading should not displace the count"
        );
        assert_eq!(count.tip.as_deref(), Some("12 articulations"));
        let mode = scene.surface("art-driver-0").unwrap().frame;
        assert!(
            count.frame.x + count.frame.size.width + SPACE <= mode.x,
            "count and trigger mode keep their gap"
        );
        assert!(
            scene.surface("arts-more-0").unwrap().frame.x + CONTROL
                <= inside.x + inside.size.width - INSET + 0.5
        );
        hover_text(&mut h, "12");
    }
}

#[test]
#[cfg(feature = "shots")]
fn articulation_distill_shots() {
    let Some(out) = std::env::var_os("KONTRA_ARTICULATION_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 900)] {
        let p = specimen();
        let mut h = Harness::new(&p, width as f64, height as f64);
        h.press("view-0-Articulations");
        h.idle(20);
        moose::core::screenshot::save_png(
            &out.join(format!("articulations-{width}.png")),
            &pixels(&h.ui, width, height),
            width.into(),
            height.into(),
        );
    }
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
                width: 700.0,
                height: 240.0,
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

fn envelope_specimen(p: &Arc<SamplerParams>) {
    use sampler_ir as ir;
    let mut view = p.shared.view.lock().unwrap();
    let mut inst = (**view.parts[0].instrument.as_ref().unwrap()).clone();
    inst.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            attack: ir::Time::Seconds(9.999),
            decay: ir::Time::Seconds(9.999),
            release: ir::Time::Seconds(9.999),
            sustain: 0.57,
            ..Default::default()
        }),
    });
    for zone in &mut inst.zones {
        zone.amplitude = Some(ir::ModulatorRef(0));
    }
    let groups = inst.groups.len();
    view.parts[0].instrument = Some(Arc::new(inst));
    drop(view);
    // Numeric readouts exist only for admitted DSP lanes, as in a real loaded part.
    let plan = sampler_core::Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_groups(groups as u32, vec![])
        .unwrap()
        .with_group_envelope_parameters(
            0,
            0,
            2,
            sampler_core::Envelope::new(479952, 0, 479952, 0.57, 479952).unwrap(),
        )
        .unwrap();
    let atoms = p.shared.part(0).unwrap();
    *atoms.engine_bindings.lock().unwrap() = plan.engine_parameter_bindings().into();
    *atoms.controls.lock().unwrap() = plan
        .controls()
        .iter()
        .map(|control| {
            let sampler_core::ControlValue::Real(value) = control.default else {
                panic!("real envelope lane")
            };
            crate::plugin::ControlCell::new(sampler_ui_ir::ControlId(control.id.0), value)
        })
        .collect();
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
        envelope_specimen(&p);
        h.press("view-0-Sound");
        save("sound-envelope", &mut h);
        h.press("output-0");
        let at = tests::center(&h.ui, "menu-item-2");
        for _ in 0..60 {
            h.tick(Input {
                pointer: PointerInput {
                    pos: Some(at),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        moose::core::screenshot::save_png(
            &out.join(format!("output-tooltip-{w}.png")),
            &pixels(&h.ui, w, height),
            w.into(),
            height.into(),
        );
        h.tick(Input {
            keys: vec![KeyPress {
                key: Key::Escape,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        h.press("app-menu");
        h.press("menu-item-3");
        save("about", &mut h);
        h.press("logs-close-about");
        h.press("tab-rack");
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

#[test]
fn output_menu_keeps_the_complete_bus_label_inspectable() {
    let p = specimen();
    let mut h = Harness::new(&p, 900., 600.);
    h.press("output-0");
    let item = h.ui.scene().unwrap().surface("menu-item-2").unwrap();
    assert!(
        item.tip
            .as_deref()
            .is_some_and(|tip| tip.ends_with("deliberately long instrument title")),
        "truncated menu labels retain their complete text"
    );
}

#[test]
fn about_text_uses_the_panel_inset() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        let mut h = Harness::new(&p, width, height);
        h.press("app-menu");
        h.press("menu-item-3");
        let scene = h.ui.scene().unwrap();
        let panel = scene.surface("logs-panel").unwrap().frame;
        let text = scene
            .surfaces()
            .find(|surface| surface.text_value.as_deref() == Some(crate::build_info::SUMMARY))
            .unwrap()
            .frame;
        assert!(
            (text.x - panel.x - INSET).abs() < 0.5,
            "About body aligns with its section heading: {text:?} in {panel:?}"
        );
        assert!(
            text.x + text.size.width <= panel.x + panel.size.width - INSET + 0.5,
            "About text stays inside its panel"
        );
    }
}

#[test]
fn generated_controls_wrap_inside_the_minimum_face() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        generated_specimen(&p);
        let mut h = Harness::new(&p, width, height);
        h.press("view-0-Interface");
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        let face = scene.surface("face-0").unwrap().frame;
        for n in 0..8 {
            let control = scene
                .surface(&format!("part-0-epoch-0-script-0-ir-{n}"))
                .unwrap()
                .frame;
            assert!(
                control.x >= face.x + INSET - 0.5
                    && control.x + control.size.width <= face.x + face.size.width - INSET + 0.5,
                "generated control {n} fits its face: {control:?} in {face:?}"
            );
        }
    }
}

#[test]
fn sound_readouts_keep_the_full_envelope_description() {
    let p = specimen();
    envelope_specimen(&p);
    let mut h = Harness::new(&p, 900., 600.);
    h.press("view-0-Sound");
    let scene = h.ui.scene().unwrap();
    let readout = scene.surface("edit-envelope-value-Attack").unwrap();
    assert!(
        readout
            .tip
            .as_deref()
            .is_some_and(|tip| tip.contains("Attack") && tip.contains("type")),
        "the current live Sound editor exposes the full parameter name and typing action"
    );
}

#[test]
fn info_values_stay_inside_the_shared_panel_inset() {
    let p = specimen();
    let path = p.selection.read().unwrap().parts[0].path.clone();
    let mut h = Harness::new(&p, 900., 600.);
    h.press("view-0-Info");
    let scene = h.ui.scene().unwrap();
    let panel = scene.surface("inside-0").unwrap().frame;
    let text = scene
        .surfaces()
        .find(|surface| surface.text_value.as_deref() == Some(path.as_str()))
        .unwrap()
        .frame;
    assert!(
        text.x + text.size.width <= panel.x + panel.size.width - INSET + 0.5,
        "Info values stay inside the shared panel inset: {text:?} in {panel:?}"
    );
}

fn hover_text(h: &mut Harness, prefix: &str) {
    let frame =
        h.ui.scene()
            .unwrap()
            .surfaces()
            .find(|surface| {
                surface
                    .text_value
                    .as_deref()
                    .is_some_and(|text| text.starts_with(prefix))
                    && surface.tip.is_some()
            })
            .unwrap()
            .frame;
    let at = Point::new(
        frame.x + frame.size.width / 2.,
        frame.y + frame.size.height / 2.,
    );
    for _ in 0..60 {
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    let tip =
        h.ui.scene()
            .unwrap()
            .surface("/tip")
            .expect("full text tooltip must appear after a real hover")
            .frame;
    assert!(
        tip.x >= -0.5 && tip.x + tip.size.width <= 900.5,
        "hover tooltip fits the minimum window: {tip:?}"
    );
}

#[test]
fn sound_hover_displays_the_full_readout() {
    let p = specimen();
    envelope_specimen(&p);
    let mut h = Harness::new(&p, 900., 600.);
    h.press("view-0-Sound");
    let readout =
        h.ui.scene()
            .unwrap()
            .surface("edit-envelope-value-Attack")
            .unwrap()
            .text_value
            .clone()
            .unwrap();
    hover_text(&mut h, &readout);
}

#[test]
fn info_hover_displays_the_complete_path() {
    let p = specimen();
    let mut h = Harness::new(&p, 900., 600.);
    h.press("view-0-Info");
    hover_text(&mut h, "/virtual/");
}

#[test]
fn generated_caption_hover_displays_the_full_name() {
    let p = specimen();
    generated_specimen(&p);
    let mut h = Harness::new(&p, 900., 600.);
    h.press("view-0-Interface");
    h.idle(20);
    hover_text(&mut h, "Extended parameter caption 0");
}

#[test]
fn trigger_conflict_hover_displays_the_full_name() {
    let p = specimen();
    let mut h = Harness::new(&p, 900., 600.);
    trigger_conflict(&mut h, &p);
    hover_text(&mut h, "Used by ");
}

#[test]
fn long_inside_rows_stop_painting_at_the_next_slot() {
    for (w, h) in [(900., 600.), (1180., 900.)] {
        let p = specimen();
        let mut h = Harness::new(&p, w, h);
        trigger_conflict(&mut h, &p);
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        let next = scene.surface("header-1").unwrap().frame.y;
        let row = scene
            .surfaces()
            .find(|surface| {
                surface
                    .text_value
                    .as_deref()
                    .is_some_and(|text| text.starts_with("11 Long articulation"))
            })
            .unwrap();
        let bottom = (row.frame.y + row.frame.size.height)
            .min(row.clip.map_or(f64::INFINITY, |clip| clip.y1));
        assert!(
            bottom <= next + 0.5,
            "inside rows must clip before the next slot: visible bottom {bottom}, next {next}; row clip {:?}, viewport {:?}, scroller {:?}",
            row.clip,
            scene.surface("rack-viewport").map(|s| (s.frame, s.clip)),
            scene.surface("rack-view").map(|s| (s.frame, s.clip))
        );
    }
}

#[test]
fn sticky_header_clipping_tracks_window_resize_without_moving_the_scroll_body() {
    let p = specimen();
    let mut h = Harness::new(&p, 900., 600.);
    trigger_conflict(&mut h, &p);
    for size in [
        Size::new(900., 600.),
        Size::new(1180., 900.),
        Size::new(900., 600.),
    ] {
        h.resize(size);
        h.idle(20);
        let scene = h.ui.scene().unwrap();
        let viewport = scene.surface("rack-viewport").unwrap().frame;
        let scroller = scene.surface("rack-view").unwrap().frame;
        assert!(
            (viewport.x - scroller.x).abs() < 0.5 && (viewport.y - scroller.y).abs() < 0.5,
            "clipping does not shift the body's coordinate system"
        );
        assert!(
            (viewport.size.height - scroller.size.height).abs() < 0.5,
            "window resize changes the full scroll viewport, not only its clip"
        );
        let row = scene
            .surfaces()
            .find(|surface| {
                surface
                    .text_value
                    .as_deref()
                    .is_some_and(|text| text.starts_with("11 Long articulation"))
            })
            .unwrap();
        let bottom = (row.frame.y + row.frame.size.height)
            .min(row.clip.map_or(f64::INFINITY, |clip| clip.y1));
        assert!(
            bottom <= scene.surface("header-1").unwrap().frame.y + 0.5,
            "the resized clip follows the visible header"
        );
    }
}

#[test]
#[cfg(feature = "shots")]
fn slot_clip_shots() {
    let Some(out) = std::env::var_os("KONTRA_SLOT_CLIP_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for (w, h) in [(900, 600), (1180, 900)] {
        let p = specimen();
        let mut fixture = Harness::new(&p, w as f64, h as f64);
        trigger_conflict(&mut fixture, &p);
        fixture.idle(20);
        moose::core::screenshot::save_png(
            &out.join(format!("slot-clip-{w}.png")),
            &pixels(&fixture.ui, w, h),
            w.into(),
            h.into(),
        );
    }
}

#[test]
fn report_is_one_tab_and_settings_uses_the_workspace() {
    let p = specimen();
    let mut h = Harness::new(&p, 900., 600.);
    h.press("tab-report");
    assert!(
        h.ui.scene().unwrap().surface("tab-logs").is_none(),
        "Report replaces both old tabs"
    );
    h.press("app-menu");
    h.press("menu-item-5");
    h.idle(20);
    let scene = h.ui.scene().unwrap();
    let settings = scene.surface("settings-body").unwrap().frame;
    let center = scene.surface("center").unwrap().frame;
    assert!(
        settings.size.height > 250.,
        "Settings is a workspace panel, not a two-row strip: {settings:?}"
    );
    assert!(
        settings.y >= center.y
            && settings.y + settings.size.height <= center.y + center.size.height + 0.5
    );
    for id in [
        "settings-libraries",
        "settings-interface",
        "settings-midi",
        "settings-performance",
    ] {
        assert!(
            scene.surface(id).is_some(),
            "Settings has a labelled section: {id}"
        );
    }
    h.press("settings-close");
    assert!(h.ui.scene().unwrap().surface("settings-body").is_none());
}

#[test]
#[cfg(feature = "shots")]
fn w13_report_settings_feedback_shots() {
    let Some(out) = std::env::var_os("KONTRA_REPORT_SETTINGS_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 780)] {
        let p = specimen();
        {
            let mut view = p.shared.view.lock().unwrap();
            let report = Arc::make_mut(view.parts[0].report.as_mut().unwrap());
            report.missing.push(crate::sound::report::Missing {
                location: "Main".into(),
                feature: "script".into(),
                value: format!(
                    "42:9: {}",
                    "Synthetic long error detail for wrapping and scrolling. ".repeat(30)
                ),
                reason: crate::sound::report::MissingReason::NotModeled,
            });
        }
        let mut h = Harness::new(&p, width as f64, height as f64);
        let save = |name: &str, h: &mut Harness| {
            h.idle(20);
            moose::core::screenshot::save_png(
                &out.join(format!("{name}-{width}.png")),
                &pixels(&h.ui, width, height),
                width.into(),
                height.into(),
            );
        };
        h.press("tab-report");
        save("report-closed", &mut h);
        h.press("report-entry-0");
        save("report-expanded", &mut h);
        let at = tests::center(&h.ui, "report-detail-0");
        let outer = h.ui.scroll("logs-list");
        h.tick(Input {
            wheel: Vec2::new(0., 1000.),
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            ..Default::default()
        });
        h.idle(30);
        assert!(
            h.ui.scroll("report-detail-0")[1] > 0.,
            "instrument details scroll within their expansion"
        );
        assert_eq!(
            h.ui.scroll("logs-list"),
            outer,
            "detail wheel keeps the report list stationary"
        );
        save("report-scrolled", &mut h);
        h.press("logs-about-button");
        save("report-about", &mut h);
        h.press("logs-close-about");
        h.press("app-menu");
        h.press("menu-item-5");
        save("settings-top", &mut h);
        let at = tests::center(&h.ui, "settings-body");
        h.tick(Input {
            wheel: Vec2::new(0., 1000.),
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            ..Default::default()
        });
        save("settings-lower", &mut h);
    }
}
