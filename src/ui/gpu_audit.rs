//! Opt-in real full-editor GPU parity witness; screenshots stay in RAM.
use super::*;
use mui_vello::{
    effects::{Budget, GpuRenderer, GpuTimer},
    kurbo::Affine,
};
#[path = "../../vendor/mui-vello/examples/gpu_support/mod.rs"]
mod gpu_support;
pub(super) fn frames(h: &mut super::tests::Harness, output: &Path) {
    pollster::block_on(run(h, output)).expect("full-editor GPU audit");
}
async fn run(h: &mut super::tests::Harness, output: &Path) -> gpu_support::Result<()> {
    let size = [1180, 760];
    let (info, device, queue) = gpu_support::device().await?;
    let target = gpu_support::target(&device, size);
    let view = target.create_view(&Default::default());
    let reference = gpu_support::target(&device, size);
    let reference_view = reference.create_view(&Default::default());
    let mut renderer = GpuRenderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        size,
        Budget::default(),
    )
    .await?;
    let mut fresh = GpuRenderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        size,
        Budget::default(),
    )
    .await?;
    let mut timer = GpuTimer::new(&device, &queue)?;
    let mut rows = Vec::new();
    let ram = std::path::PathBuf::from("/dev/shm/kontra-w3-gpu-root");
    std::fs::create_dir_all(&ram)?;
    for frame in 0..8 {
        let at = Instant::now();
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(Point::new(320. + f64::from(frame % 8) * 80., 52.)),
                ..Default::default()
            },
            ..Default::default()
        });
        let build_ms = at.elapsed().as_secs_f64() * 1000.;
        let scene = h.ui.scene_snapshot().expect("full editor scene");
        if frame == 0 {
            let cpu = super::tests::pixels(&h.ui, size[0] as u16, size[1] as u16);
            gpu_support::save(&ram.join("cpu-0.png"), size, &cpu)?;
            let details = scene.paint.iter().enumerate().map(|(i,p)| serde_json::json!({"index":i,"layer":format!("{:?}",p.layer),"commands":p.path.commands.len(),"offset":[p.offset.x,p.offset.y],"transform":p.transform.as_coeffs(),"solid":if let mui_scene::Paint::Solid(c)=&p.paint {Some(c.to_srgb().components)}else{None}})).collect::<Vec<_>>();
            std::fs::write(output.with_file_name("SCENE_NUMERIC.json"),serde_json::to_vec_pretty(&details)?)?;
        }
        let native_surfaces = scene
            .surfaces()
            .filter(|s| s.key.as_str().starts_with("nui-"))
            .count();
        let mut start = device.create_command_encoder(&Default::default());
        let ticket = timer.begin(&mut start).expect("timing slot");
        queue.submit([start.finish()]);
        let at = Instant::now();
        let stats = renderer.render(&scene, Affine::IDENTITY, &view)?;
        let submit_ms = at.elapsed().as_secs_f64() * 1000.;
        let mut end = device.create_command_encoder(&Default::default());
        timer.finish(&mut end, ticket);
        queue.submit([end.finish()]);
        timer.submitted(ticket);
        fresh.invalidate();
        fresh.render(&scene, Affine::IDENTITY, &reference_view)?;
        let actual = gpu_support::readback(&device, &queue, &target, size)?;
        let expected = gpu_support::readback(&device, &queue, &reference, size)?;
        if frame == 0 {
            let mut diagnostic = Vec::new();
            for kind in ["visible", "no_text", "solid_only", "ground_only"] {
                let mut altered = (*scene).clone();
                let mut depth = 0usize;
                let mut hidden = |p: &mui_scene::Painted| {
                    if depth > 0 {
                        match p.layer {mui_scene::Layer::Blend{..}=>depth+=1,mui_scene::Layer::Unblend=>depth-=1,_=>{}}
                        return false;
                    }
                    if matches!(p.layer,mui_scene::Layer::Blend{opacity:0.,..}) {depth=1;return false;}
                    true
                };
                match kind {
                    "visible"=>altered.paint.retain(&mut hidden),
                    "no_text"=>altered.paint.retain(|p|p.layer!=mui_scene::Layer::Text),
                    "solid_only"=>altered.paint.retain(|p| matches!(p.paint,mui_scene::Paint::Solid(_))),
                    "ground_only"=>altered.paint.truncate(1),
                    _=>unreachable!(),
                }
                fresh.invalidate();
                fresh.render(&altered,Affine::IDENTITY,&reference_view)?;
                let rgba = gpu_support::readback(&device,&queue,&reference,size)?;
                let transparent = rgba.chunks_exact(4).filter(|p|p[3]==0).count();
                gpu_support::save(&ram.join(format!("diagnostic-{kind}.png")),size,&rgba)?;
                diagnostic.push(serde_json::json!({"kind":kind,"paint_nodes":altered.paint.len(),"transparent_pixels":transparent}));
            }
            std::fs::write(output.with_file_name("GPU_ENCODING_DIAGNOSTIC.json"),serde_json::to_vec_pretty(&diagnostic)?)?;
        }

        let different = actual
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        let pure_black = actual
            .chunks_exact(4)
            .filter(|p| p[..3] == [0, 0, 0])
            .count();
        let mut times = Vec::new();
        timer.collect(&device, &mut times);
        rows.push(serde_json::json!({"frame":frame,"paint_nodes":scene.paint.len(),"native_surfaces":native_surfaces,"different_pixels":different,"pure_black_pixels":pure_black,"build_ms":build_ms,"gpu_submit_ms":submit_ms,"gpu_ms":times.first().map(|t|t.milliseconds),"encoded_scenes":stats.encoded_scenes,"renders":stats.renders,"rendered_pixels":stats.rendered_pixels}));
        if frame == 0 || different > 0 {
            gpu_support::save(&ram.join(format!("partial-{frame}.png")), size, &actual)?;
            gpu_support::save(&ram.join(format!("fresh-{frame}.png")), size, &expected)?;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    std::fs::write(
        output,
        serde_json::to_vec_pretty(
            &serde_json::json!({"adapter":info.name,"backend":format!("{:?}",info.backend),"clock_claim":"UNKNOWN: contended; GPU bracket includes possible queue wait; reference/readback outside bracket","frames":rows,"screenshots_ram":ram}),
        )?,
    )?;
    Ok(())
}
