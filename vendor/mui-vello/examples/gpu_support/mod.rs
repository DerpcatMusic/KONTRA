use mui_scene::prelude::*;
use std::{sync::mpsc, time::Duration};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const W: u32 = 384;
pub const H: u32 = 256;
pub fn fixture(scale: f64) -> mui_scene::ResolvedScene {
    let plate = |id: &str, fill: Color, width: f64| {
        block(96., 64.)
            .radius(14.)
            .fill(fill)
            .stroke(Color::oklcha(1., 0., 0., 0.7))
            .stroke_width(width)
            .id(id)
    };
    let joined = row![
        plate("a", Color::oklcha(0.65, 0.2, 25., 0.8), 2.),
        plate("b", Color::oklcha(0.65, 0.2, 260., 0.8), 8.)
    ]
    .gap(20.)
    .weld(Weld::all().reach(35.).blend(80.))
    .id("join")
    .at(16., 26.);
    let root = stack![
        stack![joined]
            .size(250., 150.)
            .clip()
            .radius(18.)
            .id("viewport")
            .at(20., 30.),
        block(24., 24.)
            .fill(Color::oklcha(1., 0., 0., 1.))
            .float()
            .id("front")
            .at(100., 74.),
    ]
    .size(f64::from(W), f64::from(H))
    // The theme rounds a filled box by default; the contract reads the corners.
    .radius(0.)
    .fill(Color::oklcha(0., 0., 0., 1.));
    let spec = SceneSpec::new(root).scale(scale);
    mui_scene::resolve(&spec.weld_backend(mui_scene::WeldBackend::AnalyticGpu))
        .expect("GPU fixture resolves")
}
pub async fn device() -> Result<(wgpu::AdapterInfo, wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await?;
    let flags = mui_vello::effects::GpuTimer::required_features();
    let features = if adapter.features().contains(flags) {
        flags
    } else {
        wgpu::Features::empty()
    };
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: features,
            ..Default::default()
        })
        .await?;
    Ok((adapter.get_info(), device, queue))
}
pub fn target(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("MUI conformance target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
/// Synchronous readback is ONLY for this correctness/benchmark tooling. Neither
/// GpuRenderer nor the gallery uses it during normal presentation.
pub fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    size: [u32; 2],
) -> Result<Vec<u8>> {
    let stride = (size[0] * 4 + 255) & !255;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("conformance readback"),
        size: u64::from(stride) * u64::from(size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size[1]),
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (send, recv) = mpsc::sync_channel(1);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = send.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    recv.recv_timeout(Duration::from_secs(10))??;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let mut pixels = Vec::with_capacity(size[0] as usize * size[1] as usize * 4);
    for row in mapped.chunks(stride as usize).take(size[1] as usize) {
        pixels.extend_from_slice(&row[..size[0] as usize * 4]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}
pub fn save(path: &std::path::Path, size: [u32; 2], rgba: &[u8]) -> Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}
pub fn pixel(rgba: &[u8], size: [u32; 2], x: u32, y: u32) -> [u8; 4] {
    let at = (y as usize * size[0] as usize + x as usize) * 4;
    rgba[at..at + 4].try_into().expect("pixel")
}
