#[test]
fn w1_performance_intent_selects_authored_source_and_retains_explicit_choices() {
    let emit = |source| sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[])
        .unwrap().ui(&|_| None).unwrap();
    let auxiliary = emit("on init declare ui_label $a(1,1) declare ui_label $b(1,1) end on");
    let mut performance = emit("on init make_perfview declare ui_knob $k(0,100,1) end on");
    performance.source = sampler_ui_ir::Source::Ksp { slot: 1 };
    let p = Arc::new(SamplerParams::new());
    p.selection.write().unwrap().parts.push(crate::plugin::Part {
        path: "/audit/performance-intent.nki".into(), view: 3, ..Default::default()
    });
    p.shared.view.lock().unwrap().parts[0].interfaces = vec![auxiliary.clone(), performance.clone()].into();
    let mut h = Harness::new(&p, 1180., 900.);
    let selected = |h: &Harness, id: &str| matches!(
        h.ui.scene().unwrap().surface(id).unwrap().semantics.as_ref().unwrap().role,
        A11y::Toggle { on: true }
    );
    assert!(selected(&h, "face-0-1"), "make_perfview must beat auxiliary widget count");
    assert!(selected(&h, "face-vector-0"), "saved mode takes precedence");
    h.press("face-0-0");
    performance.widgets[0].name = "Changed".into();
    p.shared.view.lock().unwrap().parts[0].publish_interface(&performance);
    h.idle(3);
    assert!(selected(&h, "face-0-0"), "publication retains the selected source tab");
    assert!(selected(&h, "face-vector-0"));
    h.press("face-0-1");
    let promoted = sampler_ui_ir::Interface { performance: true, ..auxiliary };
    performance.performance = false;
    {
        let mut view = p.shared.view.lock().unwrap();
        assert!(view.parts[0].publish_interface(&promoted));
        assert!(view.parts[0].publish_interface(&performance));
    }
    h.idle(3);
    assert!(selected(&h, "face-0-1"), "changed default intent retains an explicit source tab");
    let reopened = Harness::new(&p, 1180., 900.);
    assert!(selected(&reopened, "face-0-0"), "reopen uses the current published intent");
    assert!(selected(&reopened, "face-vector-0"));
}

#[test]
fn w1_background_performance_intent_retains_the_selected_tab() {
    let emit = |source| sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[])
        .unwrap().ui(&|_| None).unwrap();
    let auxiliary = emit("on init declare ui_label $a(1,1) end on");
    let mut performance = emit("on init make_perfview set_ui_color(0123456H) end on");
    performance.source = sampler_ui_ir::Source::Ksp { slot: 1 };
    let p = Arc::new(SamplerParams::new());
    p.selection.write().unwrap().parts.push(crate::plugin::Part {
        path: "/audit/background-performance-intent.nki".into(), view: 1, ..Default::default()
    });
    p.shared.view.lock().unwrap().parts[0].interfaces = vec![auxiliary, performance.clone()].into();
    let mut h = Harness::new(&p, 1180., 900.);
    let selected = |h: &Harness, id: &str| matches!(
        h.ui.scene().unwrap().surface(id).unwrap().semantics.as_ref().unwrap().role,
        A11y::Toggle { on: true }
    );
    assert!(selected(&h, "face-0-1"));
    performance.performance = false;
    assert!(p.shared.view.lock().unwrap().parts[0].publish_interface(&performance));
    h.idle(3);
    assert!(selected(&h, "face-0-1"), "intent alone must not discard an empty selected tab");
    assert!(selected(&h, "face-original-0"));
    let reopened = Harness::new(&p, 1180., 900.);
    assert!(reopened.ui.scene().unwrap().surface("part-0-epoch-0-script-0-ir-view").is_some(),
        "a sole eligible source renders without a source tab bar");
}

/// Audit-only probe: real frontend and full editor, without decoding audio PCM.
#[test]
#[ignore = "set KONTRA_AUDIT_UI_PATCH to a locally owned NKI"]
fn audit_ui_real_frames() {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let patch = PathBuf::from(std::env::var_os("KONTRA_AUDIT_UI_PATCH").expect("patch"));
    let start = Instant::now();
    let mut source = sampler_kontakt::read(&patch).unwrap().instrument;
    let read_ms = start.elapsed().as_secs_f64() * 1000.;
    let original = Arc::new(source.clone());
    // No samples are needed for UI initialization; keep group names and saved scripts.
    source.retain_zones(|_| false);
    source.assets.clear();
    let start = Instant::now();
    let loaded = sampler_kontakt::prepare(
        source,
        vec![],
        &sampler_kontakt::Options {
            library: Some(patch.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    println!(
        "AUDIT_UI read_ms={read_ms:.3} setup_ms={:.3} scripts={} faces={} widgets={} unsupported={}",
        start.elapsed().as_secs_f64() * 1000.,
        loaded.scripts.len(),
        loaded.interfaces.len(),
        loaded
            .interfaces
            .iter()
            .map(|f| f.widgets.len())
            .sum::<usize>(),
        original.unsupported.len()
    );
    let p = Arc::new(SamplerParams::new());
    let native_face = loaded.interfaces.iter().position(|face| face.native_ui.is_some());
    println!("AUDIT_UI native_face={native_face:?}");
    if let Some(scale) = std::env::var_os("KONTRA_AUDIT_UI_SCALE") {
        let scale: f32 = scale.to_str().unwrap().parse().unwrap();
        p.shared.libraries.edit(|settings| settings.view_scale = scale);
    }
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: patch.to_string_lossy().into(),
            ..Default::default()
        });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.parts[0].active = original.name.clone();
        view.parts[0].interfaces = loaded.interfaces.into();
        view.parts[0].instrument = Some(original);
    }
    let mut ui = theme::ui();
    let mut bridge = Bridge::new(p.clone());
    let mut draw = build(
        &p,
        Arc::default(),
        Arc::default(),
        Arc::default(),
        Arc::default(),
    );
    let (width, height) = (1180, 900);
    let mut ctx = RenderContext::new(width, height);
    let mut resources = Resources::default();
    let mut cache = vello::Cache::default();
    let mut pix = Pixmap::new(width, height);
    for mode in ["Original", "Vector"] {
        p.selection.write().unwrap().parts[0].view = if mode == "Original" { 1 } else { 3 };
        for _ in 0..4 {
            let tree = draw(&mut ui, &mut bridge);
            ui.frame(
                tree,
                Some(Size::new(width as f64, height as f64)),
                Input::default(),
                1. / 60.,
            )
            .unwrap();
        }
        if let Some(index) = native_face {
            let id = format!("face-0-{index}");
            if ui.scene().unwrap().surface(&id).is_some() {
                ui.focus(&id);
                let tree = draw(&mut ui, &mut bridge);
                ui.frame(tree, Some(Size::new(width as f64, height as f64)), enter(), 1. / 60.)
                    .unwrap();
            }
        }
        if mode == "Original" && native_face.is_some() {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut stable, mut decoded) = (0, 0);
            loop {
                let tree = draw(&mut ui, &mut bridge);
                ui.frame(tree, Some(Size::new(width as f64, height as f64)), Input::default(), 1. / 60.)
                    .unwrap();
                let diagnostics = super::native_ui::gate_diagnostics();
                assert!(diagnostics.is_empty(), "NativeUI failed: {diagnostics:?}");
                let assets = super::native_ui::audit_assets();
                let painted = ui.scene().unwrap().surfaces().any(|surface| {
                    surface.key.as_str().starts_with("nui-")
                });
                stable = if painted && assets.2 == 0 && assets.1 == decoded { stable + 1 } else { 0 };
                decoded = assets.1;
                if stable == 3 { break; }
                assert!(Instant::now() < deadline,
                    "NativeUI did not reach a rendered, resource-ready frame: painted={painted}, assets={assets:?}, phases={:?}",
                    p.shared.ui_activity.phase_snapshot());
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let mut phases_before = None;
        let mut timings: [Vec<f64>; 6] = Default::default();
        for frame in 0..32 {
            if frame == 8 { phases_before = p.shared.ui_activity.phase_snapshot(); }
            let a = Instant::now();
            let tree = draw(&mut ui, &mut bridge);
            let b = Instant::now();
            ui.frame(
                tree,
                Some(Size::new(width as f64, height as f64)),
                Input::default(),
                1. / 60.,
            )
            .unwrap();
            let c = Instant::now();
            let scene = ui.scene().unwrap().clone();
            let d = Instant::now();
            ctx.reset();
            vello::paint(
                &mut vello::Cpu {
                    ctx: &mut ctx,
                    resources: &mut resources,
                    cache: &mut cache,
                },
                &scene,
                vello::kurbo::Affine::IDENTITY,
            )
            .unwrap();
            ctx.flush();
            let e = Instant::now();
            ctx.render(&mut pix, &mut resources);
            let f = Instant::now();
            if frame >= 8 {
                for (t, (start, end)) in
                    timings
                        .iter_mut()
                        .zip([(a, b), (b, c), (c, d), (d, e), (e, f), (a, f)])
                {
                    t.push((end - start).as_secs_f64() * 1000.);
                }
            }
        }
        let scene = ui.scene().unwrap();
        let canvas = if mode == "Original" && native_face.is_some() {
            scene.surfaces().filter(|surface| surface.key.as_str().starts_with("nui-"))
                .max_by(|a, b| (a.frame.size.width * a.frame.size.height)
                    .total_cmp(&(b.frame.size.width * b.frame.size.height)))
        } else {
            scene.surfaces().find(|surface| surface.key.as_str().ends_with("-ir-view"))
        }.expect("selected authored canvas").frame;
        println!("AUDIT_UI mode={mode} output={width}x{height} canvas={:.3}x{:.3}", canvas.size.width, canvas.size.height);
        if let (Some(before), Some(after)) = (phases_before, p.shared.ui_activity.phase_snapshot()) {
            for (n, (name, _)) in crate::plugin::ui_activity::PHASES.iter().enumerate() {
                println!("AUDIT_UI_PHASE mode={mode} phase={name} count={} total_ns={} cumulative_max_ns={}",
                    after[n * 3] - before[n * 3], after[n * 3 + 1] - before[n * 3 + 1], after[n * 3 + 2]);
            }
        }
        for (stage, mut t) in [
            "build",
            "layout",
            "scene_clone",
            "cpu_paint",
            "cpu_raster",
            "total",
        ]
        .into_iter()
        .zip(timings)
        {
            t.sort_by(f64::total_cmp);
            println!(
                "AUDIT_UI mode={mode} stage={stage} n={} mean_ms={:.3} median_ms={:.3} p99_ms={:.3}",
                t.len(),
                t.iter().sum::<f64>() / t.len() as f64,
                t[t.len() / 2],
                t[t.len() - 1]
            );
        }
        if let Some(dir) = std::env::var_os("KONTRA_AUDIT_UI_SHOTS") {
            let to = PathBuf::from(dir).join(format!(
                "{}-{mode}.png",
                patch.file_stem().unwrap().to_string_lossy()
            ));
            moose::core::screenshot::save_png(
                &to,
                &pixels(&ui, width, height),
                width.into(),
                height.into(),
            );
        }
    }
}

#[test]
fn w1_original_default_survives_identical_publication() {
    use sampler_ui_ir as ir;
    let mut f = ir::Interface::default();
    f.pages.push(ir::Page {
        name: "Main".into(),
        size: ir::Size {
            width: 633,
            height: 500,
        },
        ..Default::default()
    });
    f.widgets.push(ir::Widget::new(
        "Cutoff",
        ir::PageRef(0),
        ir::Rect::new(10, 10, 64, 64),
        ir::Kind::Knob {
            range: ir::Range {
                min: 0.,
                max: 127.,
                default: 64.,
                step: None,
            },
            display: Default::default(),
        },
    ));
    let p = Arc::new(SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/audit/synthetic.nki".into(),
            ..Default::default()
        });
    p.shared.view.lock().unwrap().parts[0].interfaces = vec![f].into();
    let mut h = Harness::new(&p, 1180., 900.);
    let original = |h: &Harness| match h
        .ui
        .scene()
        .unwrap()
        .surface("face-original-0")
        .unwrap()
        .semantics
        .as_ref()
        .unwrap()
        .role
    {
        A11y::Toggle { on } => on,
        _ => panic!("toggle"),
    };
    assert!(original(&h), "diagnostics must not choose Vector");
    let copy = p.shared.view.lock().unwrap().parts[0].interfaces.to_vec();
    p.shared.view.lock().unwrap().parts[0].interfaces = copy.into();
    h.idle(2);
    assert!(original(&h), "identical publication must retain Original");
}

#[test]
fn w1_picture_menu_has_three_persisted_modes_and_defaults_ignore_diagnostics() {
    use sampler_ui_ir as ir;
    let script = sampler_ksp::compile(
        "on init make_perfview declare ui_knob $k(0,100,1) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let authored = script.ui(&|_| None).unwrap();
    let p = Arc::new(SamplerParams::new());
    let path = format!("/audit/default-gates-{}.nki", std::process::id());
    p.shared.libraries.edit(|s| {
        s.view_mode = crate::library::ViewMode::Original;
        s.instrument_views.remove(&path);
    });
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: path.clone(),
            ..Default::default()
        });
    p.shared.view.lock().unwrap().parts[0].interfaces = vec![authored.clone()].into();
    let mut h = Harness::new(&p, 1180., 900.);
    let selected = |h: &Harness, id: &str| match h
        .ui
        .scene()
        .unwrap()
        .surface(id)
        .unwrap()
        .semantics
        .as_ref()
        .unwrap()
        .role
    {
        A11y::Toggle { on } => on,
        _ => panic!("toggle"),
    };
    for count in [0, 1, 5, 0] {
        let mut current = authored.clone();
        current.unsupported = (0..count)
            .map(|_| ir::Unsupported {
                widget: None,
                feature: "synthetic".into(),
                value: "unavailable".into(),
            })
            .collect();
        p.shared.view.lock().unwrap().parts[0].publish_interface(&current);
        h.idle(3);
        assert!(selected(&h, "face-original-0"));
    }
    for (item, code, id) in [
        (1, 3, "face-vector-0"),
        (2, 2, "face-kontra-0"),
        (0, 1, "face-original-0"),
    ] {
        h.press("view-0");
        assert!(h.ui.scene().unwrap().surface("menu-item-2").is_some());
        h.press(&format!("menu-item-{item}"));
        h.idle(3);
        assert_eq!(p.selection.read().unwrap().parts[0].view, code);
        assert!(selected(&h, id));
        let mut reopened = Harness::new(&p, 1180., 900.);
        reopened.idle(3);
        assert!(
            selected(&reopened, id),
            "editor reopen retains the selected mode"
        );
    }
    let settings = p.shared.libraries.settings();
    assert_eq!(
        settings.instrument_views.get(&path),
        Some(&crate::library::ViewMode::Original)
    );
    p.shared.libraries.edit(|s| {
        s.instrument_views.remove(&path);
    });
}

#[test]
fn w1_two_parts_keep_same_widget_ordinal_focus_wheel_and_drag_separate() {
    let script=sampler_ksp::compile("on init make_perfview declare ui_knob $k(0,1000,1) $k := 500 end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face=script.ui(&|_|None).unwrap();
    let id=sampler_ui_ir::ControlId(script.controls()[0].definition.id.0);
    let p=Arc::new(SamplerParams::new());
    p.shared.ensure_parts(2);
    let mut owners=Vec::new();
    for slot in 0..2 {
        p.selection.write().unwrap().parts.push(crate::plugin::Part{path:format!("/missing/part-{slot}.nki"),..Default::default()});
        let compiled=sampler_ksp::compile("on init make_perfview declare ui_knob $k(0,1000,1) $k := 500 end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
        let plan=compiled.bind(sampler_core::Prepared::new(48000,vec![],vec![],1).unwrap()).unwrap();
        let limits=sampler_core::Limits::for_plan(&plan,8,0);
        let mut owner=crate::sound::v2::Part::new(sampler_core::Runtime::new(plan,limits).unwrap(),crate::sound::tree::MixTree::instrument("synthetic")).unwrap();
        let part=p.shared.part(slot).unwrap();
        part.generation.store(7,Ordering::Release);
        *part.controls.lock().unwrap()=vec![crate::plugin::ControlCell::loop_audit_new(id,500.)].into();
        part.loop_audit_install_ingress(owner.ui_controls.take());
        let mut view=p.shared.view.lock().unwrap();
        view.parts[slot].generation=7;
        view.parts[slot].interfaces=vec![face.clone()].into();
        owners.push(owner);
    }
    let mut h=Harness::new(&p,1180.,1200.);
    let target=|slot|format!("part-{slot}-epoch-7-script-0-ir-0");
    h.idle(8);
    let value=|slot|p.shared.part(slot).unwrap().display_values().into_iter().find(|(control,_)|*control==id).unwrap().1;
    h.ui.focus(&target(1));
    h.tick(Input{keys:vec![KeyPress{key:Key::Up,mods:Mods::default()}],..Default::default()});
    h.idle(3);
    assert_eq!((value(0),value(1)),(500.,501.));
    let at=center(&h.ui,&target(0));
    h.tick(Input{pointer:PointerInput{pos:Some(at),..Default::default()},wheel:Vec2::new(0.,-1.),..Default::default()});
    h.idle(3);
    assert!(value(0)>500.);
    assert_eq!(value(1),501.);
    let before=value(0);
    for (point,down) in [(at,false),(at,true),(Point::new(at.x,at.y-30.),true),(Point::new(at.x,at.y-30.),false)] {
        for _ in 0..2 {h.tick(pointer(point,down));}
    }
    h.idle(3);
    assert!(value(0)>before);
    assert_eq!(value(1),501.);
    let mut current=face.clone();current.widgets[0].text="Published label".into();
    p.shared.view.lock().unwrap().parts[0].publish_interface(&current);
    h.idle(3);
    assert_eq!(value(1),501.);
    assert!(h.ui.scene().unwrap().surface(&target(0)).is_some());
}

#[test]
fn w1_os_file_drop_uses_epoch_admission_and_preserves_native_enter_leave() {
    use crate::sound::Core;
    let script=sampler_ksp::compile("on init make_perfview declare ui_mouse_area $area set_control_par(get_ui_id($area),$CONTROL_PAR_DND_ACCEPT_AUDIO,$NI_DND_ACCEPT_MULTIPLE) set_control_par(get_ui_id($area),$CONTROL_PAR_RECEIVE_DRAG_EVENTS,1) declare ui_knob $calls(0,1000,1) declare ui_knob $inside(0,1,1) end on on ui_control($area) inc($calls) $inside := $NI_MOUSE_OVER_CONTROL end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let mut face=script.ui(&|_|None).unwrap();
    face.widgets[0].rect=sampler_ui_ir::Rect::new(0,0,100,100);
    for widget in face.widgets.iter_mut().skip(1) {widget.rect.x=150;}
    let id=|name:&str|sampler_ui_ir::ControlId(script.controls().iter().find(|c|c.variable.ends_with(name)).unwrap().definition.id.0);
    let (calls,inside)=(id("$calls"),id("$inside"));
    let prepared=script.bind(sampler_core::Prepared::new(48000,vec![],vec![],1).unwrap()).unwrap();
    let limits=sampler_core::Limits::for_plan(&prepared,8,0);
    let mut part=crate::sound::v2::Part::new(sampler_core::Runtime::new(prepared,limits).unwrap(),crate::sound::tree::MixTree::instrument("drop")).unwrap();
    let p=Arc::new(SamplerParams::new());p.shared.ensure_parts(1);
    let atoms=p.shared.part(0).unwrap();atoms.generation.store(1,Ordering::Release);
    atoms.loop_audit_install_ingress(part.ui_controls.take());
    let mut core=crate::sound::v2::V2Core::with_parts(1,48000.);core.install(0,Some(Box::new(part)));
    {let mut view=p.shared.view.lock().unwrap();view.parts[0].generation=1;view.parts[0].interfaces=vec![face.clone()].into();}
    let mut ui=theme::ui();let mut values=ir_view::Values::default();let mut input=ir_view::InputState::default();
    let face=ir_view::resolved(&face);
    let el=ir_view::view_state(&mut ui,"part-0-epoch-1-script-0",&face,sampler_ui_ir::PageRef(0),&Default::default(),sampler_ui_ir::Presentation::Vector,1.,&mut values,&mut input);
    ui.frame(el,Some(Size::new(400.,200.)),Input::default(),1./60.).unwrap();
    let at=center(&ui,"part-0-epoch-1-script-0-ir-0");let paths=vec![PathBuf::from("/tmp/owned.wav")];
    let drag=std::sync::Mutex::new(None);let picker=picker::Picker::default();
    assert!(native_files(&p,&picker,&drag,&ui,at,&paths,false));core.render(16);
    assert_eq!((core.control_value(0,calls),core.control_value(0,inside)),(Some(1.),Some(1.)));
    assert!(!native_files(&p,&picker,&drag,&ui,Point::new(-1.,-1.),&[],false));core.render(16);
    assert_eq!((core.control_value(0,calls),core.control_value(0,inside)),(Some(2.),Some(0.)));
    assert!(native_files(&p,&picker,&drag,&ui,at,&paths,true));core.render(16);
    assert_eq!(core.control_value(0,calls),Some(3.));
    assert!(!native_files(&p,&picker,&drag,&ui,at,&vec![PathBuf::from("/tmp/owned.wav");33],true));core.render(16);
    assert_eq!(core.control_value(0,calls),Some(3.));
    atoms.generation.store(2,Ordering::Release);
    assert!(!native_files(&p,&picker,&drag,&ui,at,&paths,true));core.render(16);
    assert_eq!(core.control_value(0,calls),Some(3.));
}
