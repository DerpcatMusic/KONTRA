//! Real-device public Renderer regression for refused coarse image uploads.
//! Requires gpu-effects and an absolute KONTRA_GPU_RECEIPT_DIR. No adapter,
//! device loss, timeout, or pixel mismatch is an accepted skip. Software GPUs
//! are allowed and their adapter identity is printed in the receipt.
#![cfg(feature = "gpu-effects")]

#[path = "../examples/gpu_support/mod.rs"]
pub mod gpu_support;

use std::{
    future::Future,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
use vello::kurbo::Affine;
use vello::peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageQuality};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

const SIZE: [u32; 2] = [64, 32];
const SENTINEL: [u8; 4] = [17, 29, 43, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const TEST_BUDGET: Duration = Duration::from_secs(180);

type Result<T> = gpu_support::Result<T>;

#[derive(Default, Debug)]
struct Receipt {
    refused: usize,
    rendered: usize,
    pixel_frames: usize,
}

enum Event {
    Stage(String),
    Done(std::result::Result<Receipt, String>),
}

// This oracle is specified independently of image data, scene encoding, atlas
// coordinates, Renderer results, or a second renderer's output. Primary opaque
// colors and integer-aligned nearest-sampled geometry require exact RGBA bytes.
#[derive(Clone, Copy)]
enum Expected {
    Sentinel,
    Left([u8; 4]),
    Resized,
}

impl Expected {
    fn pixel(self, x: u32, y: u32) -> [u8; 4] {
        match self {
            Self::Sentinel => SENTINEL,
            Self::Left(color) if (8..24).contains(&x) && (8..24).contains(&y) => color,
            Self::Resized if y == 0 => YELLOW,
            Self::Resized if (8..24).contains(&x) && (8..24).contains(&y) => BLUE,
            Self::Resized if (40..56).contains(&x) && (8..24).contains(&y) => CYAN,
            Self::Left(_) | Self::Resized => [0, 0, 0, 255],
        }
    }
}

fn failure(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn check_deadline(deadline: Instant, stage: &str) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(format!(
            "GPU_TIMEOUT at {stage}; no partial result is accepted"
        )));
    }
    Ok(())
}

fn stage(send: &mpsc::Sender<Event>, label: &str, deadline: Instant) -> Result<()> {
    check_deadline(deadline, label)?;
    send.send(Event::Stage(label.to_owned()))
        .map_err(|error| failure(format!("GPU test supervisor closed: {error}")))
}

fn poll_until<T>(
    future: impl Future<Output = T>,
    deadline: Instant,
    mut progress: impl FnMut() -> Result<()>,
) -> Result<T> {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        check_deadline(deadline, "future acquisition/completion")?;
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => {
                check_deadline(deadline, "future completion")?;
                return Ok(result);
            }
            Poll::Pending => {
                progress()?;
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

fn cpu_image(width: u32, height: u32, color: [u8; 4]) -> ImageData {
    ImageData {
        data: Blob::new(Arc::new(color.repeat(width as usize * height as usize))),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    }
}

fn image_scene(images: &[(&ImageData, f64, f64)]) -> Scene {
    let mut scene = Scene::new();
    for (image, x, y) in images {
        let brush = ImageBrush::new((*image).clone()).with_quality(ImageQuality::Low);
        scene.draw_image(brush.as_ref(), Affine::translate((*x, *y)));
    }
    scene
}

fn new_renderer(device: &wgpu::Device) -> Result<Renderer> {
    Ok(Renderer::new(
        device,
        RendererOptions {
            use_cpu: false,
            antialiasing_support: AaSupport::area_only(),
            num_init_threads: NonZeroUsize::new(1),
            pipeline_cache: None,
        },
    )?)
}

fn write_texture(queue: &wgpu::Queue, texture: &wgpu::Texture, size: [u32; 2], color: [u8; 4]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &color.repeat(size[0] as usize * size[1] as usize),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

fn foreign_image(
    foreign_owner: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: [u32; 2],
) -> ImageData {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("foreign renderer registered fixture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_texture(queue, &texture, size, YELLOW);
    // The Blob is created by the real public API and the owning Renderer stays
    // alive. This is foreign ownership, not synthetic malformed pixel data.
    foreign_owner.register_texture(texture)
}

struct Case<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    texture: &'a wgpu::Texture,
    view: &'a wgpu::TextureView,
    directory: &'a Path,
    send: &'a mpsc::Sender<Event>,
    deadline: Instant,
    receipt: Receipt,
}

impl Case<'_> {
    fn capture(&mut self, label: &str, expected: Expected) -> Result<()> {
        stage(self.send, label, self.deadline)?;
        let pixels = gpu_support::readback(self.device, self.queue, self.texture, SIZE)?;
        check_deadline(self.deadline, label)?;
        gpu_support::save(&self.directory.join(format!("{label}.png")), SIZE, &pixels)?;
        let mut mismatches = 0;
        let mut first = None;
        for y in 0..32 {
            for x in 0..64 {
                let actual = gpu_support::pixel(&pixels, SIZE, x, y);
                let wanted = expected.pixel(x, y);
                if actual != wanted {
                    mismatches += 1;
                    if first.is_none() {
                        first = Some((x, y, actual, wanted));
                    }
                }
            }
        }
        if mismatches != 0 {
            return Err(failure(format!(
                "PIXEL_MISMATCH {label}: {mismatches}/2048 pixels, first={first:?}"
            )));
        }
        self.receipt.pixel_frames += 1;
        println!("PIXELS_PASS {label} compared=2048 exact_rgba=true");
        Ok(())
    }

    fn render(
        &self,
        renderer: &mut Renderer,
        scene: &Scene,
        label: &str,
    ) -> std::result::Result<(), vello::Error> {
        println!("PUBLIC_RENDER_CALL {label}");
        renderer.render_to_texture(
            self.device,
            self.queue,
            scene,
            self.view,
            &RenderParams {
                base_color: vello::peniko::Color::BLACK,
                width: SIZE[0],
                height: SIZE[1],
                antialiasing_method: AaConfig::Area,
            },
        )
    }

    fn refuse(
        &mut self,
        renderer: &mut Renderer,
        scene: &Scene,
        foreign: &ImageData,
        label: &str,
        unchanged: Expected,
    ) -> Result<()> {
        stage(self.send, label, self.deadline)?;
        let result = self.render(renderer, scene, label);
        check_deadline(self.deadline, label)?;
        if !matches!(&result, Err(vello::Error::UnavailableImage(id)) if *id == foreign.data.id()) {
            return Err(failure(format!(
                "OWNERSHIP_REFUSAL_FAILURE {label}: expected UnavailableImage({}), got {result:?}",
                foreign.data.id()
            )));
        }
        self.receipt.refused += 1;
        self.capture(label, unchanged)
    }

    fn accept(
        &mut self,
        renderer: &mut Renderer,
        scene: &Scene,
        label: &str,
        expected: Expected,
    ) -> Result<()> {
        stage(self.send, label, self.deadline)?;
        self.render(renderer, scene, label)?;
        check_deadline(self.deadline, label)?;
        self.receipt.rendered += 1;
        self.capture(label, expected)
    }
}

fn scenarios(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    directory: &Path,
    send: &mpsc::Sender<Event>,
    deadline: Instant,
) -> Result<Receipt> {
    stage(send, "create-public-renderers", deadline)?;
    let mut renderer = new_renderer(device)?;
    let mut foreign_owner = new_renderer(device)?;
    check_deadline(deadline, "shader initialization")?;
    // Preserve the existing helper target usages for its other examples. This
    // direct Vello target specifically requires STORAGE_BINDING and COPY_DST.
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("public Vello retry pixel target"),
        size: wgpu::Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    write_texture(queue, &texture, SIZE, SENTINEL);
    let foreign = foreign_image(&mut foreign_owner, device, queue, [16, 16]);
    let mut case = Case {
        device,
        queue,
        texture: &texture,
        view: &view,
        directory,
        send,
        deadline,
        receipt: Receipt::default(),
    };
    case.capture("initial-sentinel", Expected::Sentinel)?;
    let foreign_only = image_scene(&[(&foreign, 40.0, 8.0)]);
    for attempt in 1..=2 {
        case.refuse(
            &mut renderer,
            &foreign_only,
            &foreign,
            &format!("foreign-only-refusal-{attempt}"),
            Expected::Sentinel,
        )?;
    }
    let mut resident = cpu_image(16, 16, RED);
    case.accept(
        &mut renderer,
        &image_scene(&[(&resident, 8.0, 8.0)]),
        "foreign-only-valid-recovery",
        Expected::Left(RED),
    )?;
    let mut previous = RED;
    for (foreign_first, color, order) in
        [(false, GREEN, "valid-first"), (true, BLUE, "foreign-first")]
    {
        let valid = cpu_image(16, 16, color);
        let mixed = if foreign_first {
            image_scene(&[(&foreign, 40.0, 8.0), (&valid, 8.0, 8.0)])
        } else {
            image_scene(&[(&valid, 8.0, 8.0), (&foreign, 40.0, 8.0)])
        };
        for attempt in 1..=2 {
            case.refuse(
                &mut renderer,
                &mixed,
                &foreign,
                &format!("mixed-{order}-refusal-{attempt}"),
                Expected::Left(previous),
            )?;
        }
        case.accept(
            &mut renderer,
            &image_scene(&[(&valid, 8.0, 8.0)]),
            &format!("mixed-{order}-valid-recovery"),
            Expected::Left(color),
        )?;
        resident = valid;
        previous = color;
    }
    // The pinned image cache starts at 1024. Every preceding image is 16x16
    // (fewer than six distinct images), so a 2048-wide image forces growth.
    // Keep the real foreign owner alive across both refused resize attempts.
    stage(send, "prepare-forced-atlas-resize", deadline)?;
    let large_foreign = foreign_image(&mut foreign_owner, device, queue, [2048, 1]);
    let cyan = cpu_image(16, 16, CYAN);
    let mixed_resize = image_scene(&[
        (&resident, 8.0, 8.0),
        (&cyan, 40.0, 8.0),
        (&large_foreign, 0.0, 0.0),
    ]);
    for attempt in 1..=2 {
        case.refuse(
            &mut renderer,
            &mixed_resize,
            &large_foreign,
            &format!("resize-refusal-{attempt}"),
            Expected::Left(BLUE),
        )?;
    }
    // A real valid wide upload on recovery independently forces a GPU atlas
    // large enough for 2048 pixels, even without inspecting private cache state.
    let wide_valid = cpu_image(2048, 1, YELLOW);
    let recovered = image_scene(&[
        (&resident, 8.0, 8.0),
        (&cyan, 40.0, 8.0),
        (&wide_valid, 0.0, 0.0),
    ]);
    case.accept(
        &mut renderer,
        &recovered,
        "resize-valid-recovery",
        Expected::Resized,
    )?;
    case.accept(
        &mut renderer,
        &recovered,
        "resize-valid-retained-reuse",
        Expected::Resized,
    )?;
    Ok(case.receipt)
}

fn run(directory: PathBuf, send: &mpsc::Sender<Event>, deadline: Instant) -> Result<Receipt> {
    stage(send, "request-adapter-device", deadline)?;
    let device_deadline = deadline.min(Instant::now() + Duration::from_secs(20));
    let (info, device, queue) = poll_until(gpu_support::device(), device_deadline, || Ok(()))?
        .map_err(|error| {
            failure(format!(
                "DEVICE_UNAVAILABLE: {error}; GPU gate FAILED, not skipped"
            ))
        })?;
    let identity = format!(
        "ADAPTER {info:?} software_adapter={}\n",
        matches!(info.device_type, wgpu::DeviceType::Cpu)
    );
    println!("{identity}");
    std::fs::write(directory.join("adapter.txt"), identity)?;
    if device.limits().max_texture_dimension_2d < 2048 {
        return Err(failure(
            "DEVICE_UNAVAILABLE: requires max_texture_dimension_2d >= 2048; not a skip",
        ));
    }
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = scenarios(&device, &queue, &directory, send, deadline);
    stage(send, "finish-validation-scope", deadline)?;
    let scope_deadline = deadline.min(Instant::now() + Duration::from_secs(10));
    if let Some(error) = poll_until(scope.pop(), scope_deadline, || {
        device.poll(wgpu::PollType::Poll)?;
        Ok(())
    })? {
        return Err(failure(format!("WGPU_VALIDATION_FAILURE: {error}")));
    }
    result
}

#[test]
fn public_renderer_owner_retry_pixels() {
    let started = Instant::now();
    let deadline = started + TEST_BUDGET;
    let directory = PathBuf::from(
        std::env::var_os("KONTRA_GPU_RECEIPT_DIR")
            .expect("KONTRA_GPU_RECEIPT_DIR must name an absolute retained artifact directory"),
    );
    assert!(
        directory.is_absolute(),
        "receipt directory must be absolute"
    );
    std::fs::create_dir_all(&directory).expect("create GPU receipt directory");
    let (send, recv) = mpsc::channel();
    // Renderer::new and the public synchronous render API enter native code.
    // Bound the entire real call chain from a supervisor thread, without lint
    // waivers or the deprecated async API. Root also supplies a process timeout.
    let worker = std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(directory, &send, deadline)
        }));
        let result = match outcome {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_else(|| "non-string panic".to_owned());
                Err(format!("GPU_WORKER_PANIC: {message}"))
            }
        };
        let _ = send.send(Event::Done(result));
    });
    let mut last_stage = "worker-start".to_owned();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match recv.recv_timeout(remaining) {
            Ok(Event::Stage(stage)) => {
                last_stage = stage;
                eprintln!(
                    "GPU_STAGE elapsed_ms={} {last_stage}",
                    started.elapsed().as_millis()
                );
            }
            Ok(Event::Done(result)) => {
                assert!(Instant::now() < deadline, "GPU_TIMEOUT after {last_stage}");
                while !worker.is_finished() {
                    assert!(
                        Instant::now() < deadline,
                        "GPU_TIMEOUT finishing worker after {last_stage}"
                    );
                    std::thread::sleep(Duration::from_millis(2));
                }
                worker.join().expect("GPU worker completion");
                let receipt = result
                    .unwrap_or_else(|error| panic!("GPU_GATE_FAILED after {last_stage}: {error}"));
                assert_eq!(
                    (receipt.refused, receipt.rendered, receipt.pixel_frames),
                    (8, 5, 14)
                );
                println!(
                    "GPU_GATE_PASS refusals=8 recoveries_and_reuse=5 pixel_frames=14 pixels_per_frame=2048 exact_rgba=true"
                );
                break;
            }
            Err(error) => panic!(
                "GPU_GATE_FAILED timeout/disconnection after {last_stage}: {error}; device absence or partial output is not a skip"
            ),
        }
    }
}
