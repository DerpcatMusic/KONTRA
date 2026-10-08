//! Retained UI-loop regressions and opt-in Conflux benchmark.
use super::*;
use sampler_ui_ir as ir;

#[test]
fn loop_audit_scalar_changes_do_not_wake_idle_editor() {
    let p = SamplerParams::new();
    p.shared.ensure_parts(1);
    let atoms = p.shared.part(0).unwrap();
    let id = ir::ControlId(9);
    *atoms.controls.lock().unwrap() =
        vec![crate::plugin::ControlCell::loop_audit_new(id, 0.)].into();
    let mut watch = Watch::default();
    let meters = Meters::default();
    let computer = computer::Computer::default();
    assert!(watch.changed(&p, &meters, &computer));
    assert!(!watch.changed(&p, &meters, &computer));
    atoms.refresh_controls(|_| Some(42.));
    watch.cpu_at = None;
    assert_eq!(atoms.control_values(), [(id, 42.)]);
    assert!(
        watch.changed(&p, &meters, &computer),
        "scalar revision participates in Watch"
    );
}

#[test]
fn loop_audit_publication_resets_original() {
    let script = sampler_ksp::compile(
        "on init make_perfview declare ui_knob $k(0,100,1) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let faces: Arc<[ir::Interface]> = vec![script.ui(&|_| None).unwrap()].into();
    let p = Arc::new(SamplerParams::new());
    p.shared.ensure_parts(1);
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/missing/test.nki".into(),
            ..Default::default()
        });
    p.shared.view.lock().unwrap().parts[0].interfaces = faces.clone();
    let mut h = tests::Harness::new(&p, 1180., 780.);
    h.ui.focus("tab-rack");
    let crop = |h: &tests::Harness| {
        let r =
            h.ui.scene()
                .unwrap()
                .surface("face-original-0")
                .unwrap()
                .frame;
        let pixels = tests::pixels(&h.ui, 1180, 780);
        let mut bytes = Vec::new();
        for y in r.y.ceil() as usize..(r.y + r.size.height).floor() as usize {
            let a = (y * 1180 + r.x.ceil() as usize) * 4;
            let b = (y * 1180 + (r.x + r.size.width).floor() as usize) * 4;
            bytes.extend_from_slice(&pixels[a..b]);
        }
        blake3::hash(&bytes)
    };
    h.idle(24);
    h.press("face-vector-0");
    h.idle(24);
    let vector = crop(&h);
    h.press("face-original-0");
    h.ui.focus("tab-rack");
    h.idle(24);
    assert_ne!(crop(&h), vector, "Original is visibly selected");
    let original = crop(&h);
    p.shared.view.lock().unwrap().parts[0].interfaces = faces.to_vec().into();
    h.idle(24);
    assert_eq!(
        crop(&h),
        original,
        "even an identical new publication resets Original to Vector"
    );
}

#[test]
#[ignore = "local Conflux witness; KONTRA_LOOP_PATCH and KONTRA_LOOP_CACHE required"]
fn loop_audit_conflux() {
    use std::time::Instant;
    let patch = PathBuf::from(std::env::var_os("KONTRA_LOOP_PATCH").unwrap());
    let cache = PathBuf::from(std::env::var_os("KONTRA_LOOP_CACHE").unwrap());
    std::fs::create_dir_all(&cache).unwrap();
    let t = Instant::now();
    let loaded = sampler_kontakt::load(
        &patch,
        &sampler_kontakt::Options {
            keys: 0..=0,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let load_ms = t.elapsed().as_secs_f64() * 1000.;
    let mut scripts = crate::sound::ScriptUi {
        views: loaded.scripts,
        resources: loaded.resources,
        ..Default::default()
    };
    let mut publication = Vec::new();
    for _ in 0..8 {
        let t = Instant::now();
        let interfaces = scripts.interfaces();
        std::hint::black_box(interfaces);
        publication.push(t.elapsed().as_secs_f64() * 1000.);
    }
    let controls = loaded.plan.controls().len();
    let ids: std::collections::HashSet<_> = loaded.plan.controls().iter().map(|c| c.id.0).collect();
    let mut sampled = std::collections::BTreeSet::new();
    let mut results = Vec::new();
    for (index, face) in loaded
        .interfaces
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.widgets.is_empty())
    {
        let face = ir_view::resolved(face);
        let mut source = pictures::Source::of(&patch);
        let mut assets = ir_view::Assets::default();
        for presentation in [ir::Presentation::Bitmap, ir::Presentation::Vector] {
            let t = Instant::now();
            assets.sync(&face, presentation, |a| source.load(a));
            let decode_ms = t.elapsed().as_secs_f64() * 1000.;
            let missing = face
                .needed_assets(presentation)
                .iter()
                .enumerate()
                .filter(|(n, need)| **need && assets.get(ir::AssetRef(*n)).is_none())
                .count();
            let mut ui = theme::ui();
            let mut values = ir_view::Values::default();
            let size = Size::new(1000., 600.);
            let mut build_ms = Vec::new();
            let mut paint_ms = Vec::new();
            for n in 0..32 {
                let t = Instant::now();
                let el = ir_view::view(
                    &mut ui,
                    "",
                    &face,
                    ir::PageRef(0),
                    &assets,
                    presentation,
                    1.,
                    &mut values,
                );
                ui.frame(
                    col![el].w(1000.).h(600.),
                    Some(size),
                    Input::default(),
                    1. / 60.,
                )
                .unwrap();
                if n >= 8 {
                    build_ms.push(t.elapsed().as_secs_f64() * 1000.);
                }
                let t = Instant::now();
                let rgba = tests::pixels(&ui, 1000, 600);
                if n >= 8 {
                    paint_ms.push(t.elapsed().as_secs_f64() * 1000.);
                }
                if n == 31 {
                    moose::core::screenshot::save_png(
                        &cache.join(format!("conflux-{index}-{presentation:?}.png")),
                        &rgba,
                        1000,
                        600,
                    );
                }
            }
            let knob = face.draw_order(ir::PageRef(0)).into_iter().find(|&n| {
                face.visible(n)
                    && matches!(
                        face.widgets[n.0].kind,
                        ir::Kind::Knob { .. } | ir::Kind::Slider { .. }
                    )
                    && matches!(face.widgets[n.0].binding, ir::Binding::Control(_))
            });
            let mut drag_changes = 0;
            if let Some(n) = knob {
                let ir::Binding::Control(id) = face.widgets[n.0].binding else {
                    unreachable!()
                };
                sampled.insert(id.0);
                let was = values[&id];
                let r = ui
                    .scene()
                    .unwrap()
                    .surface(&format!("ir-{}", n.0))
                    .unwrap()
                    .frame;
                let center = Point::new(r.x + r.size.width / 2., r.y + r.size.height / 2.);
                for (offset, down) in [
                    (0., true),
                    (-64., true),
                    (-64., true),
                    (-64., false),
                    (-64., false),
                    (0., false),
                    (0., true),
                    (64., true),
                    (64., true),
                    (64., false),
                    (64., false),
                ] {
                    let input = Input {
                        pointer: PointerInput {
                            pos: Some(Point::new(center.x, center.y + offset)),
                            buttons: if down {
                                Buttons::PRIMARY
                            } else {
                                Buttons::default()
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let el = ir_view::view(
                        &mut ui,
                        "",
                        &face,
                        ir::PageRef(0),
                        &assets,
                        presentation,
                        1.,
                        &mut values,
                    );
                    ui.frame(col![el].w(1000.).h(600.), Some(size), input, 1. / 60.)
                        .unwrap();
                    drag_changes |= usize::from(values[&id] != was);
                }
            }
            let zero_ranges = face.widgets.iter().filter(|w| matches!(w.kind,ir::Kind::Knob{range,..}|ir::Kind::Slider{range,..} if range.min==range.max)).count();
            results.push(serde_json::json!({"index":index,"source":format!("{:?}",face.source),"mode":format!("{presentation:?}"),"widgets":face.widgets.len(),"visible":face.draw_order(ir::PageRef(0)).iter().filter(|n|face.visible(**n)).count(),"assets":face.assets.len(),"missing":missing,"decoded_bytes":assets.bytes(),"decode_ms":decode_ms,"build_ms":build_ms,"paint_ms":paint_ms,"bound":face.widgets.iter().filter(|w| matches!(w.binding,ir::Binding::Control(id) if ids.contains(&id.0))).count(),"tested_drags":usize::from(knob.is_some()),"changed_drags":drag_changes,"zero_range_knobs_sliders":zero_ranges,"page_size":[face.pages[0].size.width,face.pages[0].size.height],"background_present":face.pages[0].background.image.is_some()}));
        }
    }
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 32, 0);
    let mut rt = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
    rt.set_behavior_block_fuel(sampler_core::Limits::DEFAULT_BEHAVIOR_FUEL);
    let mut callbacks = Vec::new();
    let mut label_effect = None;
    for id in sampled {
        use sampler_core::{ControlDomain, ControlValue};
        let id = sampler_core::ControlId(id);
        let d = rt.control_definition(rt.active_plan(), id).unwrap();
        let value = match d.domain {
            ControlDomain::Integer { min, max } => ControlValue::Integer(min + (max - min) / 2),
            ControlDomain::Real { min, max } => ControlValue::Real(min + (max - min) * 0.5),
            ControlDomain::Toggle => ControlValue::Toggle(true),
        };
        let context = sampler_core::ControlContext {
            performance: rt.performance(0).unwrap(),
            origin: sampler_core::ChannelAddress {
                protocol: sampler_core::Protocol::Midi1,
                port: 0,
                group: 0,
                channel: 0,
            },
            channels: 1,
        };
        let t = Instant::now();
        let admitted = rt.invoke_control(
            context,
            rt.active_plan(),
            None,
            sampler_core::ControlWrite { id, value },
        );
        let admit_us = t.elapsed().as_secs_f64() * 1e6;
        for _ in 0..8 {
            rt.render(&mut [[0.; 2]; 64]).unwrap();
        }
        let (mut effects, mut applied) = (0, 0);
        let mut ignored = std::collections::BTreeMap::<String, usize>::new();
        rt.drain_effects(|e| {
            effects += 1;
            if let Some(instance) = e.instance {
                let instance = usize::from(instance.0);
                let service = scripts.views[instance]
                    .service(e.service)
                    .unwrap_or("unknown")
                    .to_owned();
                if service == "set_knob_label" {
                    label_effect = Some((instance, *e));
                }
                if scripts.apply(instance, e) {
                    applied += 1;
                } else {
                    *ignored.entry(service).or_default() += 1;
                }
            }
            true
        });
        let outcome = admitted
            .as_ref()
            .ok()
            .and_then(|(_, b)| *b)
            .and_then(|b| rt.behavior_outcome(b).ok().flatten())
            .map(|o| format!("{o:?}"));
        callbacks.push(serde_json::json!({"admitted":admitted.is_ok(),"admit_us":admit_us,"callback":admitted.as_ref().ok().is_some_and(|(_,b)|b.is_some()),"outcome_after_512_frames":outcome,"fault":rt.take_fault().map(|(_,e)|format!("{e:?}")),"effects":effects,"applied":applied,"ignored":ignored,"dropped_effects":rt.dropped_effects()}));
    }
    let (instance, mut effect) = label_effect.expect("Conflux callback emits its knob label");
    assert!(
        callbacks
            .iter()
            .any(|c| c["effects"] == 3 && c["applied"] == 3),
        "all three callback UI effects reach their views: {callbacks:?}"
    );
    let mut published = crate::plugin::PartView {
        interfaces: loaded.interfaces.clone().into(),
        ..Default::default()
    };
    let base = published.interfaces.clone();
    let (mut changed_publication, mut noop_publication) = (Vec::new(), Vec::new());
    for n in 0..8 {
        effect.text = Some(sampler_core::Text::new(&format!("probe {n}")));
        let t = Instant::now();
        assert!(scripts.apply(instance, &effect));
        let interface = scripts.interface(instance).unwrap();
        assert!(published.publish_interface(&interface));
        changed_publication.push(t.elapsed().as_secs_f64() * 1000.);
        let t = Instant::now();
        assert!(!scripts.apply(instance, &effect));
        noop_publication.push(t.elapsed().as_secs_f64() * 1000.);
    }
    assert!(Arc::ptr_eq(&base, &published.interfaces));
    let record = serde_json::json!({"load_ms":load_ms,"controls":controls,"interfaces":loaded.interfaces.len(),"publication_ms":publication,"changed_publication_ms":changed_publication,"noop_publication_ms":noop_publication,"renders":results,"callbacks":callbacks});
    std::fs::write(
        cache.join("conflux.json"),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    println!(
        "LOOP_CONFLUX {}",
        serde_json::json!({"load_ms":load_ms,"controls":controls,"interfaces":loaded.interfaces.len(),"publication_ms":publication,"changed_publication_ms":changed_publication,"noop_publication_ms":noop_publication})
    );
}
