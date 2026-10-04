//! Native GPU conformance. Failure to get an adapter is a FAILURE, not a skipped
//! passing test. Software adapters are allowed for correctness, and named.
use mui_vello::{
    effects::{Budget, GpuRenderer},
    kurbo::Affine,
};
mod gpu_support;
fn main() -> gpu_support::Result<()> {
    pollster::block_on(run())
}
async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = gpu_support::device().await?;
    println!("ADAPTER {info:?}");
    let dir = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "gpu-contract".into()),
    );
    std::fs::create_dir_all(&dir)?;
    for scale in [1., 1.5, 2.] {
        let size = [
            (f64::from(gpu_support::W) * scale) as u32,
            (f64::from(gpu_support::H) * scale) as u32,
        ];
        let mut scene = gpu_support::fixture(scale);
        let layout = scene.layout.clone();
        let paint = scene.paint.clone();
        let texture = gpu_support::target(&device, size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = GpuRenderer::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let first = renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(first.effect_draws, 1);
        assert_eq!(first.texture_allocations, 1);
        assert_eq!(first.encoded_scenes, 1);
        let a = gpu_support::readback(&device, &queue, &texture, size)?;
        let second = renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(second.effect_draws, 0);
        assert_eq!(second.uniform_upload_bytes, 0);
        assert_eq!(second.texture_allocations, 0);
        assert_eq!(second.encoded_scenes, 0);
        assert_eq!(
            a,
            gpu_support::readback(&device, &queue, &texture, size)?,
            "retained frame changed pixels"
        );
        scene.set_weld_morph("join", 0.)?;
        assert_eq!(scene.layout, layout);
        assert_eq!(scene.paint, paint);
        let changed = renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(changed.uniform_upload_bytes, 16);
        assert_eq!(changed.effect_draws, 1);
        assert_eq!(changed.texture_allocations, 0);
        assert_eq!(changed.encoded_scenes, 0);
        let b = gpu_support::readback(&device, &queue, &texture, size)?;
        assert_ne!(a, b, "morph did not update pixels");
        let f = scene.surface("front").expect("front").frame;
        let white = gpu_support::pixel(
            &b,
            size,
            ((f.x + 12.) * scale) as u32,
            ((f.y + 12.) * scale) as u32,
        );
        assert!(
            white.iter().all(|v| *v >= 254),
            "GPU content covered the later floating control: {white:?}"
        );
        assert_eq!(
            gpu_support::pixel(&b, size, size[0] - 4, 4),
            [0, 0, 0, 255],
            "outside the clip/background changed"
        );
        if let Some(error) = error_scope.pop().await {
            return Err(error.to_string().into());
        }
        gpu_support::save(&dir.join(format!("merged-{scale}.png")), size, &a)?;
        gpu_support::save(&dir.join(format!("separate-{scale}.png")), size, &b)?;
        println!("PASS scale={scale} static-reuse morph-16B no-relayout paint-order pixels");
    }
    Ok(())
}
