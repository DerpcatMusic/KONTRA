#[test]
fn probe() {
    let Ok(path) = std::env::var("PROBE_KSP") else { return };
    let source = std::fs::read_to_string(path).unwrap();
    let script = sampler_ksp::compile(&source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let face = script.ui(&|_| None).unwrap();
    for (i, w) in face.widgets.iter().enumerate() {
        let r = face.page_rect(sampler_ui_ir::WidgetRef(i));
        if r.x < 120 && r.x + r.width as i32 > 12 && r.y < 40 && r.y + r.height as i32 > 18 && !w.hidden {
            println!("W #{i} {} kind={:?} z={} rect={:?} parent={:?} hide={:?} imgs={:?}", w.name, w.kind, w.z, r, w.parent, w.hide, w.images.iter().map(|i| (i.asset.0, face.assets[i.asset.0].path.clone())).collect::<Vec<_>>());
        }
    }
}
