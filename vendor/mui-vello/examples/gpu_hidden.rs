//! Transparent subtrees must not exhaust the GPU's scene buffers.
use mui_scene::prelude::*;
use mui_vello::{effects::{Budget, GpuRenderer}, kurbo::Affine};
mod gpu_support;
fn main() -> gpu_support::Result<()> { pollster::block_on(run()) }
async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = gpu_support::device().await?;
    println!("ADAPTER {} {:?}", info.name, info.backend);
    let size = [1180, 760];

    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("receipt directory"));
    std::fs::create_dir_all(&dir)?;
    let mut reference = None;
    let mut failures = 0;
    for count in [0, 1, 56] {
        let target = gpu_support::target(&device, size);
        let view = target.create_view(&Default::default());
        let mut renderer = GpuRenderer::new(&device,&queue,wgpu::TextureFormat::Rgba8Unorm,size,Budget::default()).await?;
        let root = stack((0..count).map(|_| {
            let mut node = block(1180.,760.).fill(Color::srgb(1.,0.,0.));
            for _ in 0..22 {node=stack([node]).size(1180.,760.).opacity(0.);}
            node
        }))
            .size(1180.,760.).radius(0.).fill(Color::srgb(0.2,0.3,0.4));
        let scene = mui_scene::resolve(&SceneSpec::new(root))?;
        renderer.invalidate();
        renderer.render(&scene, Affine::IDENTITY,&view)?;
        let rgba = gpu_support::readback(&device,&queue,&target,size)?;
        let transparent = rgba.chunks_exact(4).filter(|p| p[3]==0).count();
        if let Some(expected)=&reference { if expected != &rgba { failures+=1; } } else {reference=Some(rgba.clone());}
        gpu_support::save(&dir.join(format!("hidden-{count}.png")),size,&rgba)?;
        println!("FRAME hidden={count} paint_nodes={} transparent_pixels={transparent}",scene.paint.len());
    }
    assert_eq!(failures,0,"invisible subtrees changed the visible frame");
    Ok(())
}
