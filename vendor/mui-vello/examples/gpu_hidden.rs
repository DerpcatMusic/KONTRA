//! First-open, changed, full-redraw and resized frames must survive GPU overflow.
use mui_scene::prelude::*;
use mui_vello::{
    effects::{Budget, GpuRenderer},
    kurbo::Affine,
};
mod gpu_support;
fn scene(hidden: usize, alpha: f32, color: f32) -> mui_scene::ResolvedScene {
    let root = stack((0..hidden).map(|_| {
        let mut node = block(1180., 760.).radius(0.).fill(Color::srgb(1., 0., 0.));
        for _ in 0..22 {
            node = stack([node]).size(1180., 760.).opacity(alpha);
        }
        node
    }))
    .size(1180., 760.)
    .radius(0.)
    .fill(Color::srgb(color, 0.3, 0.4));
    mui_scene::resolve(&SceneSpec::new(root)).unwrap()
}
fn main() -> gpu_support::Result<()> {
    pollster::block_on(run())
}
async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = gpu_support::device().await?;
    println!("ADAPTER {} {:?}", info.name, info.backend);
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("receipt directory"));
    std::fs::create_dir_all(&dir)?;
    for alpha in [0., 0.9] {
        let size = [1180, 760];
        let target = gpu_support::target(&device, size);
        let view = target.create_view(&Default::default());
        // A fresh renderer AND target prevent a stale cached frame from passing.
        let mut renderer = GpuRenderer::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        let initial = scene(1, alpha, 0.2);
        let first = renderer.render(&initial, Affine::IDENTITY, &view)?;
        assert_eq!(first.encoded_scenes, 1);
        assert_eq!(
            first.rendered_pixels,
            u64::from(size[0]) * u64::from(size[1])
        );
        let before = gpu_support::readback(&device, &queue, &target, size)?;
        assert!(
            before.chunks_exact(4).all(|p| p[3] == 255),
            "first frame must be opaque"
        );
        assert!(
            before.chunks_exact(4).all(|p| p[..3] != [0, 0, 0]),
            "first frame must contain visible color"
        );
        gpu_support::save(&dir.join(format!("first-alpha{alpha}.png")), size, &before)?;
        let next = scene(1, alpha, 0.7);
        renderer.render(&next, Affine::IDENTITY, &view)?;
        let changed = gpu_support::readback(&device, &queue, &target, size)?;
        assert_ne!(
            before, changed,
            "a failed render must not reuse the previous frame"
        );
        renderer.invalidate();
        let full = renderer.render(&next, Affine::IDENTITY, &view)?;
        assert_eq!(
            full.encoded_scenes, 1,
            "native expose must force full encoding"
        );
        assert_eq!(
            full.rendered_pixels,
            u64::from(size[0]) * u64::from(size[1])
        );
        assert_eq!(
            changed,
            gpu_support::readback(&device, &queue, &target, size)?
        );
        let resized = [590, 380];
        renderer.resize(resized)?;
        let target = gpu_support::target(&device, resized);
        let resized_view = target.create_view(&Default::default());
        let frame = renderer.render(&next, Affine::scale(0.5), &resized_view)?;
        assert_eq!(frame.encoded_scenes, 1);
        assert_eq!(
            frame.rendered_pixels,
            u64::from(resized[0]) * u64::from(resized[1])
        );
        let pixels = gpu_support::readback(&device, &queue, &target, resized)?;
        assert!(
            pixels.chunks_exact(4).all(|p| p[3] == 255),
            "resized first frame must be opaque"
        );
        gpu_support::save(
            &dir.join(format!("resize-alpha{alpha}.png")),
            resized,
            &pixels,
        )?;
        println!(
            "PASS alpha={alpha} first_opaque={} changed=true full_redraw=true resized_opaque={}",
            size[0] * size[1],
            resized[0] * resized[1]
        );
    }
    Ok(())
}
