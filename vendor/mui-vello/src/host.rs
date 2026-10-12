//! The one GPU host for a window MUI paints into: the surface, a device
//! chosen for it, and the retained [`GpuRenderer`]. The gallery (winit) and
//! the plugin editor (baseview) both drive this; only the window and how a
//! surface is made from it stay theirs.
//!
//! Loss is handled here, not by a panic: a lost device (driver reset,
//! eGPU unplugged) is rebuilt on the next [`Host::present`], and a lost
//! surface is reported as [`Frame::SurfaceLost`] so the caller can hand a
//! fresh one to [`Host::replace_surface`].
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mui_scene::ResolvedScene;

use crate::Classic;
use crate::effects::{Budget, EffectStats, GpuRenderer};
use crate::kurbo::Affine;

/// A failed device rebuild waits this long before the next attempt, so a
/// GPU that keeps failing does not cost a device creation every frame.
const RETRY: Duration = Duration::from_millis(500);

/// The surface size clamped to what a vello `Scene` holds (`u16`), or
/// `None` when there is nothing to draw into: a minimised window reports
/// 0x0, and configuring a zero-sized surface is invalid.
pub fn target_size(width: u32, height: u32) -> Option<(u32, u32)> {
    let max = u32::from(u16::MAX);
    (width > 0 && height > 0).then(|| (width.min(max), height.min(max)))
}

/// MUI's alpha contract wants a non-sRGB UNORM target: vello writes sRGB
/// values, and an `_Srgb` surface would encode them a second time.
pub fn surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats.iter().copied().find(|f| {
        matches!(
            f,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
        )
    })
}

/// The present mode for a window surface. Windows: `AutoNoVsync`, because
/// an embedded editor presents on the DAW's GUI thread and a backed-up
/// Fifo blocks it. Elsewhere the first of Mailbox, FifoRelaxed, Fifo.
fn present_mode(modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    use wgpu::PresentMode as P;
    if cfg!(windows) {
        return P::AutoNoVsync;
    }
    [P::Mailbox, P::FifoRelaxed]
        .into_iter()
        .find(|m| modes.contains(m))
        .unwrap_or(P::Fifo)
}

/// The swapchain extent for a `v`-pixel side. Linux steps it up to a
/// multiple of 256 so a live resize reconfigures every 256 px, not every
/// pixel; the renderer keeps the exact size and the present writes only
/// its corner. Elsewhere the exact size.
// ponytail: X11 crops the oversized buffer to the window; a Wayland
// surface takes its size from the buffer, so gate on X11 if one shows it.
fn surface_extent(v: u32, limit: u32) -> u32 {
    if cfg!(target_os = "linux") {
        v.next_multiple_of(256).min(limit)
    } else {
        v
    }
}

/// What one [`Host::present`] did.
#[derive(Debug)]
pub enum Frame {
    /// The scene is on screen.
    Presented(EffectStats),
    /// Nothing reached the screen (no size, occluded, outdated, timed out,
    /// or the device was just rebuilt): paint the same scene next frame.
    Skipped,
    /// The screen already shows this scene under this transform and no
    /// texture changed: nothing was acquired or drawn. Not a retry.
    Current,
    /// The surface is gone. Make a new one from the same window, pass it to
    /// [`Host::replace_surface`], and paint again.
    SurfaceLost,
}

/// Why a [`Host`] could not paint.
#[derive(Debug)]
pub enum HostError {
    /// No adapter can present to the surface.
    Adapter(wgpu::RequestAdapterError),
    /// The adapter refused a device.
    Device(wgpu::RequestDeviceError),
    /// The surface offers nothing MUI can paint into.
    Surface(&'static str),
    /// The renderer failed: creating it, resizing it, or a frame.
    Render(crate::effects::Error),
    /// Acquiring the surface texture failed validation. Acquiring again
    /// would fail the same way, so this is not a lost surface.
    Validation,
    /// The device was lost, and opening a new one failed with this. The
    /// next [`Host::present`] after a short wait tries again.
    DeviceLost(Box<HostError>),
}
impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adapter(e) => write!(f, "GPU adapter: {e}"),
            Self::Device(e) => write!(f, "GPU device: {e}"),
            Self::Surface(s) => write!(f, "GPU surface: {s}"),
            Self::Render(e) => write!(f, "{e}"),
            Self::Validation => f.write_str("GPU surface texture failed validation"),
            Self::DeviceLost(e) => write!(f, "rebuilding a lost device: {e}"),
        }
    }
}
impl std::error::Error for HostError {}

/// Everything that lives on one device, rebuilt whole when it is lost:
/// pipelines, atlases, weld textures and retained encodings die with it.
struct OnDevice {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The renderer's exact size; the surface may be larger (Linux steps).
    size: (u32, u32),
    renderer: GpuRenderer,
    /// Raised by wgpu's device-lost callback, on whatever thread wgpu calls it.
    lost: Arc<AtomicBool>,
}

impl OnDevice {
    /// A device able to present to `surface` (any, headless), configured
    /// for it at `size`, and a renderer on it.
    fn open(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        size: (u32, u32),
    ) -> Result<Self, HostError> {
        // A desktop with an iGPU enumerates it first; paint on the card.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surface,
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(HostError::Adapter)?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(HostError::Device)?;
        let lost = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&lost);
        device.set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
        // wgpu's default panics, which in a plugin is the host's crash. An
        // error no scope caught is logged; a loss is still seen above.
        device.on_uncaptured_error(Arc::new(|e| {
            eprintln!("mui-vello: uncaptured GPU error: {e}");
        }));
        let limit = device.limits().max_texture_dimension_2d;
        let (width, height) = target_size(size.0.min(limit), size.1.min(limit)).unwrap_or((1, 1));
        let config = match surface {
            Some(surface) => {
                let caps = surface.get_capabilities(&adapter);
                let format = surface_format(&caps.formats)
                    .ok_or(HostError::Surface("no non-sRGB UNORM surface format"))?;
                let (sw, sh) = (surface_extent(width, limit), surface_extent(height, limit));
                let config = wgpu::SurfaceConfiguration {
                    format,
                    present_mode: present_mode(&caps.present_modes),
                    desired_maximum_frame_latency: 1,
                    ..surface
                        .get_default_config(&adapter, sw, sh)
                        .ok_or(HostError::Surface("no default configuration"))?
                };
                surface.configure(&device, &config);
                config
            }
            // Headless: a stand-in nothing is configured with.
            None => wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8Unorm,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: Vec::new(),
                color_space: wgpu::SurfaceColorSpace::Auto,
            },
        };
        let renderer = pollster::block_on(GpuRenderer::new(
            &device,
            &queue,
            config.format,
            [width, height],
            Budget::default(),
        ))
        .map_err(HostError::Render)?;
        Ok(Self {
            device,
            queue,
            config,
            size: (width, height),
            renderer,
            lost,
        })
    }

    fn lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// [`OnDevice::lost`] after polling the device, which is where wgpu
    /// runs the lost callback: an idle window submits nothing that would.
    fn poll_lost(&self) -> bool {
        // A lost device errors here; the flag is what answers.
        let _ = self.device.poll(wgpu::PollType::Poll);
        self.lost()
    }
}

/// A window's surface, the device painting it and the renderer.
pub struct Host {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    gpu: OnDevice,
    /// The size last asked for, in physical pixels: what a rebuilt device
    /// configures. `None` while there is nothing to draw into.
    wanted: Option<(u32, u32)>,
    retry_at: Option<Instant>,
    /// Bumped on every device rebuild: what lives on the old device is gone.
    generation: u64,
}

impl Host {
    /// A device for `surface` and a renderer at `size` physical pixels.
    /// `surface` must come from `instance`.
    pub fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        size: (u32, u32),
    ) -> Result<Self, HostError> {
        let gpu = OnDevice::open(&instance, Some(&surface), size)?;
        Ok(Self {
            instance,
            surface,
            gpu,
            wanted: target_size(size.0, size.1),
            retry_at: None,
            generation: 0,
        })
    }

    /// The instance the surface came from, for making a replacement.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// The rendered size, physical pixels. The configured surface can be
    /// larger (Linux steps it to 256 px).
    pub fn size(&self) -> (u32, u32) {
        self.gpu.size
    }

    /// The device and queue painting the surface, for textures a caller
    /// draws itself. Rebuilt on loss: check [`Host::generation`].
    pub fn device(&self) -> (&wgpu::Device, &wgpu::Queue) {
        (&self.gpu.device, &self.gpu.queue)
    }

    /// Bumped each time a lost device is rebuilt; anything made on the old
    /// [`Host::device`] must be made again.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Paint `texture` where the scene has an image texture `key`; see
    /// [`GpuRenderer::set_texture`]. The next present paints even if the
    /// scene did not change.
    pub fn set_texture(&mut self, key: u64, texture: &wgpu::Texture) {
        self.gpu.renderer.set_texture(key, texture);
    }

    /// The device was lost; the next [`Host::present`] rebuilds it. An idle
    /// window polls this so it does not wait for an event to find out: it
    /// polls the device, which is where wgpu runs its lost callback.
    pub fn device_lost(&self) -> bool {
        self.gpu.poll_lost()
    }

    /// Resize to `width` x `height` physical pixels, clamped to what the
    /// device and a vello scene hold. Zero hides: presents skip until a
    /// real size comes back.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), HostError> {
        self.wanted = target_size(width, height);
        let Some((width, height)) = self.wanted else {
            return Ok(());
        };
        let limit = self.gpu.device.limits().max_texture_dimension_2d;
        let (width, height) = (width.min(limit), height.min(limit));
        if (width, height) == self.size() {
            return Ok(());
        }
        let OnDevice {
            device,
            config,
            size,
            renderer,
            ..
        } = &mut self.gpu;
        renderer
            .resize([width, height])
            .map_err(HostError::Render)?;
        *size = (width, height);
        let extent = (surface_extent(width, limit), surface_extent(height, limit));
        if extent != (config.width, config.height) {
            (config.width, config.height) = extent;
            self.surface.configure(device, config);
        }
        Ok(())
    }

    /// Swap in a new surface for the same window after
    /// [`Frame::SurfaceLost`]. It must come from [`Host::instance`].
    pub fn replace_surface(&mut self, surface: wgpu::Surface<'static>) {
        self.surface = surface;
        self.surface.configure(&self.gpu.device, &self.gpu.config);
        self.gpu.renderer.invalidate();
    }

    /// Paint `scene` under `xf` and present it. `Err` is a render or
    /// device-rebuild failure, not a lost surface; painting the same scene
    /// again would fail the same way.
    pub fn present(&mut self, scene: &ResolvedScene, xf: Affine) -> Result<Frame, HostError> {
        self.present_inner::<fn(&mut Classic<'_>)>(scene, xf, None)
    }

    /// [`Host::present`] with `overlay` drawn on top, never retained.
    pub fn present_with_overlay<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        xf: Affine,
        overlay: F,
    ) -> Result<Frame, HostError> {
        self.present_inner(scene, xf, Some(overlay))
    }

    fn present_inner<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        xf: Affine,
        overlay: Option<F>,
    ) -> Result<Frame, HostError> {
        use wgpu::CurrentSurfaceTexture as Acquired;
        let Some(size) = self.wanted else {
            return Ok(Frame::Skipped);
        };
        if self.gpu.lost() {
            let now = Instant::now();
            if self.retry_at.is_some_and(|at| now < at) {
                return Ok(Frame::Skipped);
            }
            match OnDevice::open(&self.instance, Some(&self.surface), size) {
                Ok(gpu) => {
                    self.gpu = gpu;
                    self.generation += 1;
                    self.retry_at = None;
                    return Ok(Frame::Skipped);
                }
                Err(e) => {
                    self.retry_at = Some(now + RETRY);
                    return Err(HostError::DeviceLost(Box::new(e)));
                }
            }
        }
        let OnDevice {
            device,
            queue,
            config,
            renderer,
            ..
        } = &mut self.gpu;
        if overlay.is_none() && renderer.is_current(scene, xf) {
            return Ok(Frame::Current);
        }
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
            Acquired::Outdated => {
                self.surface.configure(device, config);
                renderer.invalidate();
                return Ok(Frame::Skipped);
            }
            Acquired::Occluded | Acquired::Timeout => return Ok(Frame::Skipped),
            Acquired::Lost => return Ok(Frame::SurfaceLost),
            Acquired::Validation => return Err(HostError::Validation),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let stats = match overlay {
            Some(draw) => renderer.render_with_overlay(scene, xf, &view, draw),
            None => renderer.render(scene, xf, &view),
        }
        .map_err(HostError::Render)?;
        queue.present(frame);
        Ok(Frame::Presented(stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn sizes_clamp_and_zero_is_nothing_to_draw_into() {
        assert_eq!(target_size(0, 600), None);
        assert_eq!(target_size(800, 0), None);
        assert_eq!(target_size(70_000, 600), Some((65_535, 600)));
    }

    #[test]
    fn the_surface_format_is_never_srgb() {
        use wgpu::TextureFormat as F;
        assert_eq!(
            surface_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm]),
            Some(F::Bgra8Unorm)
        );
        assert_eq!(surface_format(&[F::Rgba8UnormSrgb]), None);
    }

    #[test]
    fn present_mode_and_swapchain_steps_follow_the_platform() {
        use wgpu::PresentMode as P;
        let mode = present_mode(&[P::Fifo, P::FifoRelaxed]);
        if cfg!(windows) {
            assert_eq!(mode, P::AutoNoVsync);
            assert_eq!(surface_extent(300, 8192), 300);
        } else {
            assert_eq!(mode, P::FifoRelaxed);
            assert_eq!(present_mode(&[P::Fifo]), P::Fifo);
        }
        if cfg!(target_os = "linux") {
            assert_eq!(surface_extent(300, 8192), 512);
            assert_eq!(surface_extent(8000, 8192), 8192);
        }
    }

    /// A real loss, not a flag flipped by hand: `Device::destroy` fires the
    /// lost callback, and a device opened again renders.
    #[test]
    fn a_destroyed_device_is_seen_and_a_new_one_renders() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let gpu = match OnDevice::open(&instance, None, (16, 16)) {
            Ok(gpu) => gpu,
            Err(e) => return eprintln!("SKIPPED: no wgpu device ({e})"),
        };
        assert!(!gpu.lost());
        gpu.device.destroy();
        assert!(gpu.poll_lost(), "loss unseen");
        let mut gpu2 = OnDevice::open(&instance, None, (16, 16)).unwrap();
        assert_ne!(gpu2.device, gpu.device);
        let target = gpu2.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let scope = gpu2.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let root = block(16., 16.).fill(Role::Primary);
        let scene = resolve(&SceneSpec::new(root).offered(Size::new(16., 16.))).unwrap();
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        gpu2.renderer
            .render(&scene, Affine::IDENTITY, &view)
            .unwrap();
        assert!(pollster::block_on(scope.pop()).is_none());
        assert!(!gpu2.lost());
    }
}
