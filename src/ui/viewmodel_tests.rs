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
    source.zones.clear();
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
        let id = format!("face-{}-0", mode.to_lowercase());
        if ui.scene().unwrap().surface(&id).is_some() {
            ui.focus(&id);
            let tree = draw(&mut ui, &mut bridge);
            ui.frame(
                tree,
                Some(Size::new(width as f64, height as f64)),
                enter(),
                1. / 60.,
            )
            .unwrap();
        }
        let mut timings: [Vec<f64>; 6] = Default::default();
        for frame in 0..32 {
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
        let canvas = ui.scene().unwrap().surface("ir-view").or_else(|| ui.scene().unwrap().surface("part-0-epoch-0-script-0-ir-view")).unwrap().frame;
        println!("AUDIT_UI mode={mode} output={width}x{height} canvas={:.3}x{:.3}", canvas.size.width, canvas.size.height);
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
