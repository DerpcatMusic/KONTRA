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
