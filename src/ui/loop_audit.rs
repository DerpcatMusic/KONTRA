//! Retained UI-loop regressions and opt-in Conflux benchmark.
use super::*;
use sampler_ui_ir as ir;

#[test]
fn loop_audit_scalar_changes_do_not_wake_idle_editor() {
    use crate::plugin::ui_activity::{Activity, Count};
    let mut p = SamplerParams::new();
    p.shared.ui_activity = Arc::new(Activity::new(true));
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
    atoms.refresh_controls(|_| Some(42.));
    watch.cpu_at = None;
    assert!(!watch.changed(&p, &meters, &computer), "unchanged readback stays idle");
    let counts = p.shared.ui_activity.snapshot().unwrap();
    assert_eq!(counts[Count::WatchCalls as usize], 4);
    assert_eq!(counts[Count::ReadoutPolls as usize], 3);
    assert_eq!(counts[Count::ReadoutChanges as usize], 2);
    assert_eq!(counts[Count::WatchWakes as usize], 2);
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

/// Benchmark-only admission: require a live value, nonzero range and the actual
/// pointer-hit winner in the resolved scene. Never infer a callback from a name.
fn loop_drag_target(
    ui: &Ui,
    face: &ir::Interface,
    values: &ir_view::Values,
    ids: &std::collections::HashSet<u128>,
) -> (Option<(ir::WidgetRef, ir::ControlId, Point)>, std::collections::BTreeMap<&'static str, usize>) {
    let mut skipped = std::collections::BTreeMap::new();
    let mut selected = None;
    let Some(scene) = ui.scene() else {
        skipped.insert("no_resolved_scene", 1);
        return (None, skipped);
    };
    let mut hit = moose::mui::mui::input::Hit::default();
    for surface in scene.surfaces().filter(|s| (Id::is_named(&s.key) || s.pointer_states) && !s.disabled) {
        let placed = if surface.hits.is_empty() {
            hit.push_placed(surface.key.clone(), None, &surface.path, surface.offset,
                surface.clip, surface.clip_paths())
        } else {
            for (tag, path) in &surface.hits {
                if hit.push_placed(surface.key.clone(), Some(tag.clone()), path,
                    surface.offset, surface.clip, surface.clip_paths()).is_err() {
                    skipped.insert("invalid_hit_geometry", 1);
                    return (None, skipped);
                }
            }
            continue;
        };
        if placed.is_err() {
            skipped.insert("invalid_hit_geometry", 1);
            return (None, skipped);
        }
    }
    for n in face.draw_order(ir::PageRef(0)) {
        let w = &face.widgets[n.0];
        let range = match &w.kind {
            ir::Kind::Knob { range, .. } | ir::Kind::Slider { range, .. } => range,
            _ => continue,
        };
        let reason = if !face.visible(n) {
            Some("not_visible")
        } else if !w.enabled || !w.intercepts_mouse {
            Some("not_interactive")
        } else if !range.min.is_finite() || !range.max.is_finite() || range.min >= range.max {
            Some("unavailable_range")
        } else if let ir::Binding::Control(id) = w.binding {
            if !ids.contains(&id.0) {
                Some("not_runtime_bound")
            } else if !values.get(&id).is_some_and(|v| v.is_finite()) {
                Some("no_live_value")
            } else if let Some(surface) = scene.surface(&format!("ir-{}", n.0)) {
                let r = surface.frame;
                let center = Point::new(r.x + r.size.width / 2., r.y + r.size.height / 2.);
                if surface.disabled || r.size.width <= 0. || r.size.height <= 0. {
                    Some("no_interactive_geometry")
                } else if hit.at(center).is_none_or(|key| format!("ir-{}", n.0) != key) {
                    Some("clipped_or_occluded")
                } else {
                    if selected.is_none() {
                        selected = Some((n, id, center));
                    }
                    None
                }
            } else {
                Some("not_rendered")
            }
        } else {
            Some("not_control_bound")
        };
        if let Some(reason) = reason {
            *skipped.entry(reason).or_default() += 1;
        }
    }
    (selected, skipped)
}

#[test]
fn loop_drag_target_skips_unavailable_controls_and_uses_runtime_values() {
    let script = sampler_ksp::compile(
        "on init make_perfview declare ui_knob $k(0,100,1) end on",
        48000, sampler_ksp::Limits::LIBRARY, &[],
    ).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let ir::Binding::Control(id) = face.widgets[0].binding else { panic!("owned knob binding") };
    let ids = std::collections::HashSet::from([id.0]);
    let mut values = ir_view::Values::from([(id, 37.)]);
    let mut ui = theme::ui();
    let el = ir_view::view(&mut ui, &face, ir::PageRef(0), &Default::default(),
        ir::Presentation::Vector, 1., &mut values);
    ui.frame(col![el].w(1000.).h(600.), Some(Size::new(1000., 600.)), Input::default(), 1. / 60.).unwrap();
    assert_eq!(loop_drag_target(&ui, &face, &values, &ids).0.unwrap().1, id);
    assert_eq!(values[&id], 37., "building does not initialize/replace caller values");
    let (target, skipped) = loop_drag_target(&ui, &face, &Default::default(), &ids);
    assert!(target.is_none());
    assert_eq!(skipped.get("no_live_value"), Some(&1));
    let (target, skipped) = loop_drag_target(&ui, &face, &values, &Default::default());
    assert!(target.is_none());
    assert_eq!(skipped.get("not_runtime_bound"), Some(&1));
    let mut disabled = face.clone();
    disabled.widgets[0].enabled = false;
    assert!(loop_drag_target(&ui, &disabled, &values, &ids).0.is_none());
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
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 32, 0);
    let mut rt = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
    rt.set_behavior_block_fuel(sampler_core::Limits::DEFAULT_BEHAVIOR_FUEL);
    // Production receives core readback before rendering; view() itself does not
    // populate untouched controls. Preserve exact runtime values, no invented defaults.
    let initial_values: ir_view::Values = ids.iter().filter_map(|&id| {
        let value = rt.control_value(rt.active_plan(), sampler_core::ControlId(id)).ok()?;
        let value = match value {
            sampler_core::ControlValue::Integer(v) => v as f64,
            sampler_core::ControlValue::Real(v) => v,
            sampler_core::ControlValue::Toggle(v) => f64::from(u8::from(v)),
        };
        Some((ir::ControlId(id), value))
    }).collect();
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
            let mut values = initial_values.clone();
            let size = Size::new(1000., 600.);
            let mut build_ms = Vec::new();
            let mut paint_ms = Vec::new();
            for n in 0..32 {
                let t = Instant::now();
                let el = ir_view::view(
                    &mut ui,
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
            let (knob, skipped) = loop_drag_target(&ui, &face, &values, &ids);
            let mut drag_changes = 0;
            if let Some((_n, id, center)) = knob {
                let was = values[&id];
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
                        &face,
                        ir::PageRef(0),
                        &assets,
                        presentation,
                        1.,
                        &mut values,
                    );
                    ui.frame(col![el].w(1000.).h(600.), Some(size), input, 1. / 60.)
                        .unwrap();
                    drag_changes |= usize::from(values.get(&id).is_some_and(|v| *v != was));
                }
                if drag_changes != 0 {
                    sampled.insert(id.0);
                }
            }
            let zero_ranges = face.widgets.iter().filter(|w| matches!(w.kind,ir::Kind::Knob{range,..}|ir::Kind::Slider{range,..} if range.min==range.max)).count();
            results.push(serde_json::json!({"index":index,"source":format!("{:?}",face.source),"mode":format!("{presentation:?}"),"widgets":face.widgets.len(),"visible":face.draw_order(ir::PageRef(0)).iter().filter(|n|face.visible(**n)).count(),"assets":face.assets.len(),"missing":missing,"decoded_bytes":assets.bytes(),"decode_ms":decode_ms,"build_ms":build_ms,"paint_ms":paint_ms,"bound":face.widgets.iter().filter(|w| matches!(w.binding,ir::Binding::Control(id) if ids.contains(&id.0))).count(),"tested_drags":usize::from(knob.is_some()),"changed_drags":drag_changes,"skipped_unavailable_drag_controls":skipped,"selected_control":knob.map(|(_,id,_)|format!("{:032x}",id.0)),"zero_range_knobs_sliders":zero_ranges,"page_size":[face.pages[0].size.width,face.pages[0].size.height],"background_present":face.pages[0].background.image.is_some()}));
        }
    }
    // Checkpoint once per stage, not per frame. Callback assertions stay strict;
    // their failure must not discard completed layout/software-paint measurements.
    let mut record = serde_json::json!({
        "stage": "render_complete_callbacks_pending",
        "scope": "owned_ir_headless_layout_and_software_paint_not_native_ui_or_gpu_present",
        "load_ms": load_ms, "controls": controls, "interfaces": loaded.interfaces.len(),
        "publication_ms": publication, "renders": results,
        "unavailable_control_readback": controls.saturating_sub(initial_values.len()),
    });
    let checkpoint = |record: &serde_json::Value| {
        std::fs::write(cache.join("conflux.json"), serde_json::to_vec_pretty(record).unwrap()).unwrap();
    };
    checkpoint(&record);
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
        callbacks.push(serde_json::json!({"control":format!("{:032x}",id.0),"admitted":admitted.is_ok(),"admit_us":admit_us,"callback":admitted.as_ref().ok().is_some_and(|(_,b)|b.is_some()),"outcome_after_512_frames":outcome,"fault":rt.take_fault().map(|(_,e)|format!("{e:?}")),"effects":effects,"applied":applied,"ignored":ignored,"dropped_effects":rt.dropped_effects()}));
    }
    record["stage"] = serde_json::json!("callbacks_complete_assertions_pending");
    record["callbacks"] = serde_json::json!(callbacks);
    record["sampled_changed_bound_controls"] = serde_json::json!(callbacks.len());
    checkpoint(&record);
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
    record["stage"] = serde_json::json!("complete");
    record["changed_publication_ms"] = serde_json::json!(changed_publication);
    record["noop_publication_ms"] = serde_json::json!(noop_publication);
    checkpoint(&record);
    println!(
        "LOOP_CONFLUX {}",
        serde_json::json!({"load_ms":load_ms,"controls":controls,"interfaces":loaded.interfaces.len(),"publication_ms":publication,"changed_publication_ms":changed_publication,"noop_publication_ms":noop_publication})
    );
}
