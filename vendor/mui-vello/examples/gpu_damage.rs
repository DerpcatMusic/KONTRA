//! Partial GPU repaint must match a fresh full frame, including vector artwork.
use mui_scene::prelude::*;
use mui_vello::{
    effects::{Budget, GpuRenderer, GpuTimer},
    kurbo::{Affine, Rect, Shape},
};
use std::{sync::Arc, time::Instant};
mod gpu_support;
fn scene(color: u8, vector: bool) -> mui_scene::ResolvedScene {
    let art = if vector {
        Fill::Vector(
            Arc::new(
                mui_scene::Vector::new(
                    32.,
                    24.,
                    vec![mui_scene::VectorCommand::Fill {
                        path: Rect::new(0., 0., 32., 24.).to_path(0.01),
                        transform: Affine::IDENTITY,
                        brush: mui_vello::peniko::Brush::Solid(
                            mui_vello::peniko::Color::from_rgba8(color, 210, 70, 255),
                        ),
                        brush_transform: Affine::IDENTITY,
                        rule: mui_vello::peniko::Fill::NonZero,
                    }],
                )
                .unwrap(),
            ),
            Fit::Fill,
        )
    } else {
        Fill::Color(Color::srgb(f32::from(color) / 255., 0.8, 0.3))
    };
    mui_scene::resolve(&SceneSpec::new(
        stack![
            block(330., 20.)
                .at(12., 10.)
                .fill(Color::srgb(0.5, 0.1, 0.2))
                .id("chrome"),
            block(240., 160.)
                .at(60., 60.)
                .fill(Color::srgb(0.2, 0.3, 0.6))
                .id("instrument"),
            block(32., 24.).at(132., 116.).fill(art).id("changed"),
        ]
        .size(384., 256.)
        .radius(0.)
        .fill(Color::srgb(0.08, 0.08, 0.08)),
    ))
    .unwrap()
}
fn main() -> gpu_support::Result<()> {
    pollster::block_on(run())
}
async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = gpu_support::device().await?;
    println!("ADAPTER {info:?}");
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("receipt directory"));
    std::fs::create_dir_all(&dir)?;
    let mut mismatches = 0;
    for vector in [false, true] {
        for scale in [1., 1.5, 2.] {
            let size = [(384. * scale) as u32, (256. * scale) as u32];
            let texture = gpu_support::target(&device, size);
            let view = texture.create_view(&Default::default());
            let fresh_texture = gpu_support::target(&device, size);
            let fresh_view = fresh_texture.create_view(&Default::default());
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
            renderer.render(&scene(30, vector), Affine::scale(scale), &view)?;
            let mut timer = GpuTimer::new(&device, &queue)?;
            for (frame, color) in [160, 30, 220, 160].into_iter().enumerate() {
                let next = scene(color, vector);
                let mut start = device.create_command_encoder(&Default::default());
                let ticket = timer.begin(&mut start).expect("timing slot");
                queue.submit([start.finish()]);
                let at = Instant::now();
                let stats = renderer.render(&next, Affine::scale(scale), &view)?;
                let cpu_ms = at.elapsed().as_secs_f64() * 1000.;
                let mut end = device.create_command_encoder(&Default::default());
                timer.finish(&mut end, ticket);
                queue.submit([end.finish()]);
                timer.submitted(ticket);
                assert!(
                    stats.rendered_pixels > 0
                        && stats.rendered_pixels < u64::from(size[0]) * u64::from(size[1]),
                    "must exercise partial repaint: {stats:?}"
                );
                fresh.invalidate();
                fresh.render(&next, Affine::scale(scale), &fresh_view)?;
                let actual = gpu_support::readback(&device, &queue, &texture, size)?;
                let expected = gpu_support::readback(&device, &queue, &fresh_texture, size)?;
                let different = actual
                    .chunks_exact(4)
                    .zip(expected.chunks_exact(4))
                    .filter(|(a, b)| a != b)
                    .count();
                if different > 0 {
                    mismatches += 1;
                }
                if frame == 0 {
                    gpu_support::save(
                        &dir.join(format!("partial-vector{vector}-scale{scale}.png")),
                        size,
                        &actual,
                    )?;
                    gpu_support::save(
                        &dir.join(format!("full-vector{vector}-scale{scale}.png")),
                        size,
                        &expected,
                    )?;
                }
                let mut times = Vec::new();
                timer.collect(&device, &mut times);
                println!(
                    "FRAME vector={vector} scale={scale} frame={frame} different_pixels={different} cpu_submit_ms={cpu_ms} stats={stats:?} gpu={times:?}"
                );
            }
        }
    }
    assert_eq!(
        mismatches, 0,
        "partial rendering differs from fresh full rendering"
    );
    Ok(())
}
