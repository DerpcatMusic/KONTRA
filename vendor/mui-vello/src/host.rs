//! The one GPU host for a window MUI paints into: the surface, a device
//! chosen for it, and the retained [`GpuRenderer`]. The gallery (winit) and
//! the plugin editor (baseview) both drive this; only the window and how a
//! surface is made from it stay theirs.
//!
//! Loss is handled here, not by a panic: a lost device (driver reset,
//! eGPU unplugged) is rebuilt on the next [`Host::present`], and a lost
//! surface is reported as [`Frame::SurfaceLost`] so the caller can hand a
//! fresh one to [`Host::replace_surface`].
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mui_scene::ResolvedScene;

use crate::Classic;
use crate::effects::{Budget, EffectStats, GpuRenderer};
use crate::kurbo::Affine;

/// A failed device rebuild waits this long before the next attempt, so a
/// GPU that keeps failing does not cost a device creation every frame.
const RETRY: Duration = Duration::from_millis(500);

// Opening every API eagerly loads GL/EGL even when Vulkan/Metal/DX12 succeeds.
// Keep the caller's enabled backends, but pay for the secondary APIs only on failure.
fn try_backends<T>(
    enabled: wgpu::Backends,
    mut open: impl FnMut(wgpu::Backends) -> Result<T, String>,
) -> Result<T, String> {
    let primary = enabled & wgpu::Backends::PRIMARY;
    let mut errors = Vec::new();
    for backends in [primary, enabled - primary]
        .into_iter()
        .filter(|b| !b.is_empty())
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open(backends)));
        match result {
            Ok(Ok(host)) => return Ok(host),
            Ok(Err(error)) => errors.push(format!("{backends:?}: {error}")),
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic payload");
                errors.push(format!(
                    "{backends:?}: GPU initialization panicked: {message}"
                ));
            }
        }
    }
    if errors.is_empty() {
        Err("no GPU backends enabled".into())
    } else {
        Err(errors.join("; "))
    }
}

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

/// Whether a window shows what is behind it where the scene is not opaque.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transparency {
    /// The surface's default: opaque wherever the platform offers that.
    #[default]
    Opaque,
    /// Composite with alpha, for a window over an OS blur (frosted glass)
    /// or with no backdrop at all. Falls back to [`Transparency::Opaque`]
    /// on a surface that cannot: see [`Host::translucent`].
    Translucent,
}

/// The alpha mode a surface offering `offered` is configured with for
/// `want`: premultiplied (what MUI paints) or else straight alpha when
/// translucent, and `Auto` -- opaque where offered -- otherwise.
pub fn alpha_mode(
    offered: &[wgpu::CompositeAlphaMode],
    want: Transparency,
) -> wgpu::CompositeAlphaMode {
    use wgpu::CompositeAlphaMode as A;
    let translucent = [A::PreMultiplied, A::PostMultiplied]
        .into_iter()
        .find(|m| offered.contains(m));
    match want {
        Transparency::Translucent => translucent.unwrap_or(A::Auto),
        Transparency::Opaque => A::Auto,
    }
}

/// Whether a surface configured `mode` composites with alpha.
fn is_translucent(mode: wgpu::CompositeAlphaMode) -> bool {
    use wgpu::CompositeAlphaMode as A;
    matches!(mode, A::PreMultiplied | A::PostMultiplied)
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

/// The initialization operation a candidate reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuStage {
    /// Surface formats, alpha modes, usages, or compute limits are unsuitable.
    Capabilities,
    /// The driver refused the requested device.
    Device,
    /// The retained renderer could not create its resources.
    Renderer,
    /// Configuring the real window surface failed.
    Surface,
    /// The complete candidate was installed.
    Ready,
}

/// One attempted adapter, including enough context to report driver failures.
#[derive(Clone, Debug)]
pub struct AdapterDiagnostic {
    pub adapter: wgpu::AdapterInfo,
    pub supported_limits: wgpu::Limits,
    pub requested_limits: wgpu::Limits,
    pub stage: GpuStage,
    pub format: Option<wgpu::TextureFormat>,
    pub alpha_mode: Option<wgpu::CompositeAlphaMode>,
    pub failure: Option<String>,
}

/// Candidates in preference order: discrete, integrated, other, virtual, then
/// CPU; native Vulkan/Metal/DX12 precede GL within each type. Equal ranks keep
/// enumeration order. Every adapter is attempted at most once, including CPU.
/// Backends are constrained by the supplied instance's configuration.
#[derive(Clone, Debug, Default)]
pub struct GpuDiagnostics {
    pub candidates: Vec<AdapterDiagnostic>,
}

impl std::fmt::Display for GpuDiagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.candidates.is_empty() {
            return f.write_str("no adapters enumerated by the configured instance");
        }
        for (i, candidate) in self.candidates.iter().enumerate() {
            if i != 0 {
                f.write_str("; ")?;
            }
            write!(
                f,
                "{} ({:?}, {:?}, vendor={:#06x}, device={:#06x}): {:?}",
                candidate.adapter.name,
                candidate.adapter.backend,
                candidate.adapter.device_type,
                candidate.adapter.vendor,
                candidate.adapter.device,
                candidate.stage
            )?;
            if let Some(error) = &candidate.failure {
                write!(f, ": {error}")?;
            }
        }
        Ok(())
    }
}

// Adapter type/backend preference adapted from Zed's Apache-2.0
// crates/gpui_wgpu/src/wgpu_context.rs at
// a84689073d296dfd39987bc7dd478e43ef76d83a (sort_adapters).
// Copyright 2022 - 2025 Zed Industries, Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
// Adaptation: stable high-performance ranking without Zed-specific overrides;
// retain CPU fallback and MUI's full compute renderer requirements.
fn adapter_rank(info: &wgpu::AdapterInfo) -> (u8, u8) {
    let device = match info.device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::Other => 2,
        wgpu::DeviceType::VirtualGpu => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    let backend = match info.backend {
        wgpu::Backend::Vulkan | wgpu::Backend::Metal | wgpu::Backend::Dx12 => 0,
        _ => 1,
    };
    (device, backend)
}

fn validate_renderer_requirements(
    info: &wgpu::AdapterInfo,
    flags: wgpu::DownlevelFlags,
    requested: &wgpu::Limits,
    supported: &wgpu::Limits,
) -> Result<(), String> {
    // WARP's compute JIT faults outside Rust error handling; see the captured
    // stack in research/platform-validation-2026-10-08.md.
    if info.backend == wgpu::Backend::Dx12
        && info.device_type == wgpu::DeviceType::Cpu
        && info.vendor == 0x1414
    {
        return Err("WARP compute shader compilation can crash; use CPU rendering".into());
    }
    if !flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        return Err("Vello requires compute shaders; downlevel GL is unsupported".into());
    }
    let mut missing = Vec::new();
    requested.check_limits_with_fail_fn(supported, false, |name, requested, supported| {
        missing.push(format!(
            "{name}: requested {requested}, supported {supported}"
        ));
    });
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("renderer limits: {}", missing.join(", ")))
    }
}

/// Run the finite candidate list once. The same policy is exercised without a
/// driver in tests; a failed preferred device must not hide working software.
fn select_candidate<A, T>(
    candidates: impl IntoIterator<Item = (A, AdapterDiagnostic)>,
    mut attempt: impl FnMut(A, &mut AdapterDiagnostic) -> Result<T, String>,
) -> Result<(T, GpuDiagnostics), HostError> {
    let mut diagnostics = GpuDiagnostics::default();
    for (adapter, mut candidate) in candidates {
        crate::diagnostics::breadcrumb(
            "mui-vello",
            "adapter_capabilities",
            &format!("{candidate:?}"),
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            attempt(adapter, &mut candidate)
        }));
        match result {
            Ok(Ok(selected)) => {
                candidate.stage = GpuStage::Ready;
                diagnostics.candidates.push(candidate);
                crate::diagnostics::breadcrumb("mui-vello", "gpu_ready", &diagnostics.to_string());
                return Ok((selected, diagnostics));
            }
            Ok(Err(error)) => candidate.failure = Some(error),
            Err(payload) => {
                let message = crate::diagnostics::panic_message(&*payload);
                candidate.failure = Some(format!("panic: {message}"));
                crate::diagnostics::gpu_error("initialization_panic", message, &candidate.adapter);
            }
        }
        crate::diagnostics::breadcrumb("mui-vello", "candidate_failed", &format!("{candidate:?}"));
        diagnostics.candidates.push(candidate);
    }
    crate::diagnostics::error(
        "mui-vello",
        "initialization_failed",
        &diagnostics.to_string(),
    );
    Err(HostError::Initialization(diagnostics))
}

/// Device-wide callbacks outlive the host's old generation while callers
/// retain resources from it. Errors coalesce; each observer has its own cursor.
#[derive(Default)]
struct DeviceErrors {
    lost: AtomicBool,
    latest: Mutex<(u64, Option<String>)>,
}
impl DeviceErrors {
    fn record(&self, message: String) {
        let mut latest = self
            .latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        latest.0 = latest.0.wrapping_add(1);
        latest.1 = Some(message);
    }
    fn observe(&self, cursor: &mut u64) -> Option<String> {
        let latest = self
            .latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *cursor == latest.0 {
            return None;
        }
        *cursor = latest.0;
        latest.1.clone()
    }
}

/// Why a [`Host`] could not paint.
#[derive(Debug)]
pub enum HostError {
    /// Every candidate failed. Inspect the complete adapter/stage report.
    Initialization(GpuDiagnostics),
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
    /// Configuring or reconfiguring a surface failed without installing it.
    Configuration(String),
    /// The device was lost, and opening a new one failed with this. The
    /// next [`Host::present`] after a short wait tries again.
    DeviceLost(Box<HostError>),
}
impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initialization(report) => write!(f, "GPU initialization: {report}"),
            Self::Adapter(e) => write!(f, "GPU adapter: {e}"),
            Self::Device(e) => write!(f, "GPU device: {e}"),
            Self::Surface(s) => write!(f, "GPU surface: {s}"),
            Self::Render(e) => write!(f, "{e}"),
            Self::Validation => f.write_str("GPU surface texture failed validation"),
            Self::Configuration(error) => write!(f, "GPU surface configuration: {error}"),
            Self::DeviceLost(e) => write!(f, "rebuilding a lost device: {e}"),
        }
    }
}
impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Adapter(error) => Some(error),
            Self::Device(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::DeviceLost(error) => Some(&**error),
            _ => None,
        }
    }
}
impl HostError {
    /// Candidate failures, also when this error wraps a device-loss rebuild.
    pub fn diagnostics(&self) -> Option<&GpuDiagnostics> {
        match self {
            Self::Initialization(report) => Some(report),
            Self::DeviceLost(error) => error.diagnostics(),
            _ => None,
        }
    }
}

/// Everything that lives on one device, rebuilt whole when it is lost:
/// pipelines, atlases, weld textures and retained encodings die with it.
struct OnDevice {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The renderer's exact physical size, matching the native surface.
    size: (u32, u32),
    renderer: GpuRenderer,
    /// Raised by wgpu's device-lost callback, on whatever thread wgpu calls it.
    errors: Arc<DeviceErrors>,
    diagnostics: GpuDiagnostics,
}

impl OnDevice {
    /// A device able to present to `surface` (any, headless), configured
    /// for it at `size`, and a renderer on it.
    fn open(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        size: (u32, u32),
        transparency: Transparency,
    ) -> Result<Self, HostError> {
        // Enumeration remains constrained by the instance's enabled backends
        // (including WGPU_BACKEND when its descriptor was built from env).
        let operation = crate::diagnostics::operation(
            "mui-vello",
            "enumerate_adapters",
            &format!(
                "size={size:?}; MUI {}; os={}; arch={}; panic_unwind={}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
                cfg!(panic = "unwind")
            ),
            None,
        );
        let mut adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        drop(operation);
        adapters.sort_by_key(|adapter| adapter_rank(&adapter.get_info()));
        let candidates = adapters.into_iter().map(|adapter| {
            let supported_limits = adapter.limits();
            // Do not use GPUI's downlevel/GL limits: Vello needs compute,
            // storage buffers, and the full default workgroup limits.
            let requested_limits = wgpu::Limits::default()
                .using_resolution(supported_limits.clone())
                .using_alignment(supported_limits.clone());
            let candidate = AdapterDiagnostic {
                adapter: adapter.get_info(),
                supported_limits,
                requested_limits,
                stage: GpuStage::Capabilities,
                format: None,
                alpha_mode: None,
                failure: None,
            };
            (adapter, candidate)
        });
        let (mut gpu, diagnostics) = select_candidate(candidates, |adapter, candidate| {
            Self::try_adapter(&adapter, surface, size, transparency, candidate)
        })?;
        gpu.diagnostics = diagnostics;
        Ok(gpu)
    }

    fn try_adapter(
        adapter: &wgpu::Adapter,
        surface: Option<&wgpu::Surface<'_>>,
        size: (u32, u32),
        transparency: Transparency,
        candidate: &mut AdapterDiagnostic,
    ) -> Result<Self, String> {
        validate_renderer_requirements(
            &candidate.adapter,
            adapter.get_downlevel_capabilities().flags,
            &candidate.requested_limits,
            &candidate.supported_limits,
        )?;
        let limit = candidate.requested_limits.max_texture_dimension_2d;
        let (width, height) = target_size(size.0.min(limit), size.1.min(limit)).unwrap_or((1, 1));
        let config = if let Some(surface) = surface {
            let caps = surface.get_capabilities(adapter);
            if caps.alpha_modes.is_empty()
                || caps.present_modes.is_empty()
                || !caps.usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
            {
                return Err(
                    "surface has no usable alpha/present modes or render attachment usage".into(),
                );
            }
            let format = surface_format(&caps.formats).ok_or("no non-sRGB UNORM surface format")?;
            wgpu::SurfaceConfiguration {
                format,
                present_mode: present_mode(&caps.present_modes),
                alpha_mode: alpha_mode(&caps.alpha_modes, transparency),
                desired_maximum_frame_latency: 1,
                ..surface
                    .get_default_config(adapter, width, height)
                    .ok_or("no default surface configuration")?
            }
        } else {
            wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8Unorm,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: Vec::new(),
                color_space: wgpu::SurfaceColorSpace::Auto,
            }
        };
        candidate.format = Some(config.format);
        candidate.alpha_mode = Some(config.alpha_mode);
        candidate.stage = GpuStage::Device;
        let operation = crate::diagnostics::operation(
            "mui-vello",
            "request_device",
            &format!("{candidate:?}"),
            Some(&candidate.adapter),
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("MUI retained renderer"),
            required_limits: candidate.requested_limits.clone(),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        drop(operation);
        let errors = Arc::new(DeviceErrors::default());
        let state = Arc::clone(&errors);
        let info = candidate.adapter.clone();
        device.set_device_lost_callback(move |reason, message| {
            // Destroyed is also delivered during orderly window teardown.
            if reason != wgpu::DeviceLostReason::Destroyed {
                crate::diagnostics::gpu_error(
                    "device_lost",
                    &format!("{reason:?}: {message}"),
                    &info,
                );
            }
            state.record(format!("device lost ({reason:?}): {message}"));
            state.lost.store(true, Ordering::Release);
        });
        let state = Arc::clone(&errors);
        let info = candidate.adapter.clone();
        device.on_uncaptured_error(Arc::new(move |error| {
            eprintln!("mui-vello: uncaptured GPU error: {error}");
            crate::diagnostics::gpu_error("uncaptured_error", &error.to_string(), &info);
            state.record(error.to_string());
        }));
        candidate.stage = GpuStage::Renderer;
        let operation = crate::diagnostics::operation(
            "mui-vello",
            "create_renderer",
            &format!("format={:?}; size=({width},{height})", config.format),
            Some(&candidate.adapter),
        );
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let renderer = pollster::block_on(GpuRenderer::new(
            &device,
            &queue,
            config.format,
            [width, height],
            Budget::default(),
        ));
        let scoped = [internal, memory, validation]
            .into_iter()
            .filter_map(|scope| pollster::block_on(scope.pop()))
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        drop(operation);
        if !scoped.is_empty() {
            return Err(scoped.join("; "));
        }
        let mut renderer = renderer.map_err(|e| e.to_string())?;
        renderer.set_straight_alpha(config.alpha_mode == wgpu::CompositeAlphaMode::PostMultiplied);
        // Surface configuration is the last fallible initialization operation.
        // Failed candidates drop all pipelines/caches before trying the next.
        candidate.stage = GpuStage::Surface;
        if let Some(surface) = surface {
            configure(surface, &device, &config)?;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        if let Some(error) = errors.observe(&mut 0) {
            return Err(format!("device error during initialization: {error}"));
        }
        Ok(Self {
            device,
            queue,
            config,
            size: (width, height),
            renderer,
            errors,
            diagnostics: GpuDiagnostics::default(),
        })
    }

    fn lost(&self) -> bool {
        self.errors.lost.load(Ordering::Acquire)
    }

    /// [`OnDevice::lost`] after polling the device, which is where wgpu
    /// runs the lost callback: an idle window submits nothing that would.
    fn poll_lost(&self) -> bool {
        // A lost device errors here; the flag is what answers.
        let _ = self.device.poll(wgpu::PollType::Poll);
        self.lost()
    }
}

/// Validation failures belong to this attempt, not the process-wide callback.
fn configure(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> Result<(), String> {
    let _operation = crate::diagnostics::operation(
        "mui-vello",
        "configure_surface",
        &format!("{config:?}"),
        None,
    );
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let configured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        surface.configure(device, config);
    }));
    let errors = [internal, memory, validation]
        .into_iter()
        .filter_map(|scope| pollster::block_on(scope.pop()))
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    if let Err(payload) = configured {
        let message = crate::diagnostics::panic_message(&*payload);
        return Err(format!("panic configuring surface: {message}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// A window's surface, the device painting it and the renderer.
pub struct Host {
    instance: wgpu::Instance,
    // None after a failed replacement; acquisition must wait for a new surface.
    surface: Option<wgpu::Surface<'static>>,
    gpu: OnDevice,
    /// The size last asked for, in physical pixels: what a rebuilt device
    /// configures. `None` while there is nothing to draw into.
    wanted: Option<(u32, u32)>,
    retry_at: Option<Instant>,
    /// Bumped on every device rebuild: what lives on the old device is gone.
    generation: u64,
    /// What the caller asked for; a rebuilt device asks again.
    transparency: Transparency,
    /// Persist the first draw only; never do file I/O on steady animation frames.
    first_frame: bool,
}

impl Host {
    /// Open native APIs first, then secondary APIs if needed. `WGPU_BACKEND`
    /// remains an allowlist. The factory must drop each failed surface before
    /// creating another and keep its native window alive for the returned host.
    pub fn open_native(
        create: impl FnMut(wgpu::Backends) -> Result<(wgpu::Instance, wgpu::Surface<'static>), String>,
        size: (u32, u32),
    ) -> Result<Self, String> {
        Self::open_native_with_transparency(create, size, Transparency::Opaque)
    }

    /// [`Self::open_native`] preserving a native window's transparency policy.
    pub fn open_native_with_transparency(
        mut create: impl FnMut(
            wgpu::Backends,
        ) -> Result<(wgpu::Instance, wgpu::Surface<'static>), String>,
        size: (u32, u32),
        transparency: Transparency,
    ) -> Result<Self, String> {
        let enabled = wgpu::InstanceDescriptor::new_without_display_handle_from_env().backends;
        try_backends(enabled, |backends| {
            let (instance, surface) = create(backends)?;
            Self::with_transparency(instance, surface, size, transparency)
                .map_err(|e| e.to_string())
        })
    }

    /// A device for `surface` and a renderer at `size` physical pixels.
    /// `surface` must come from `instance`.
    pub fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        size: (u32, u32),
    ) -> Result<Self, HostError> {
        Self::with_transparency(instance, surface, size, Transparency::Opaque)
    }

    /// [`Host::new`] for a window that is `transparency`: a translucent
    /// one shows what the scene leaves uncovered or part-covered. The
    /// window itself must be able to (an ARGB visual on X11, a non-opaque
    /// layer on macOS, DirectComposition on Windows' DX12).
    pub fn with_transparency(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        size: (u32, u32),
        transparency: Transparency,
    ) -> Result<Self, HostError> {
        let gpu = OnDevice::open(&instance, Some(&surface), size, transparency)?;
        Ok(Self {
            instance,
            surface: Some(surface),
            gpu,
            wanted: target_size(size.0, size.1),
            retry_at: None,
            generation: 0,
            transparency,
            first_frame: true,
        })
    }

    /// The surface composites with alpha: [`Transparency::Translucent`]
    /// was asked for and the surface offered a way to.
    pub fn translucent(&self) -> bool {
        is_translucent(self.gpu.config.alpha_mode)
    }

    /// The instance the surface came from, for making a replacement.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// The rendered size in physical pixels, matching the native surface.
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

    /// Adapter attempts for the currently installed device generation.
    pub fn diagnostics(&self) -> &GpuDiagnostics {
        &self.gpu.diagnostics
    }

    /// Observe the latest uncaptured error or loss once per observer. Reset
    /// `cursor` to zero after [`Host::generation`] changes. Several errors
    /// between observations coalesce; callbacks never invoke application code.
    pub fn observe_gpu_error(&self, cursor: &mut u64) -> Option<String> {
        self.gpu.errors.observe(cursor)
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

    /// Force a complete repaint after a native expose, restore, or reparent.
    pub fn invalidate(&mut self) {
        self.gpu.renderer.invalidate();
        self.first_frame = true;
    }

    /// Resize to `width` x `height` physical pixels, clamped to what the
    /// device and a vello scene hold. Zero hides: presents skip until a
    /// real size comes back.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), HostError> {
        let was_hidden = self.wanted.is_none();
        self.wanted = target_size(width, height);
        if was_hidden && self.wanted.is_some() {
            self.invalidate();
        }
        let Some((width, height)) = self.wanted else {
            return Ok(());
        };
        let limit = self.gpu.device.limits().max_texture_dimension_2d;
        let (width, height) = (width.min(limit), height.min(limit));
        if (width, height) == self.size() {
            return Ok(());
        }
        // Some backends accept surface configuration on a destroyed device.
        // Its renderer resources are still invalid; let the host detach this
        // GPU before any resize or presentation uses them.
        if self.gpu.poll_lost() {
            let error = "device lost before resize".to_owned();
            crate::diagnostics::error("mui-vello", "resize_failed", &error);
            return Err(HostError::Configuration(error));
        }
        let OnDevice {
            device,
            config,
            size,
            renderer,
            ..
        } = &mut self.gpu;
        // Commit dimensions only after configuration succeeds. A failed resize
        // must retry, rather than comparing equal to a size never installed.
        let mut next = config.clone();
        (next.width, next.height) = (width, height);
        if let Some(surface) = &self.surface {
            configure(surface, device, &next).map_err(|error| {
                crate::diagnostics::error("mui-vello", "resize_failed", &error);
                HostError::Configuration(error)
            })?;
        }
        renderer
            .resize([width, height])
            .map_err(HostError::Render)?;
        *size = (width, height);
        // The drawable and renderer must have the same physical extent. An
        // oversized GL framebuffer shifts its top-left image upward, while
        // Wayland surfaces take their dimensions from the submitted buffer.
        *config = next;
        self.first_frame = true;
        Ok(())
    }

    /// Swap in a new surface for the same window after [`Frame::SurfaceLost`].
    /// It must come from [`Host::instance`]. Revalidates the adapter and changes
    /// [`Host::generation`] on success; re-upload external textures. Failures
    /// are logged and observable through [`Host::observe_gpu_error`]. Use
    /// [`Host::try_replace_surface`] to receive the full failure report.
    pub fn replace_surface(&mut self, surface: wgpu::Surface<'static>) {
        if let Err(error) = self.try_replace_surface(surface) {
            self.gpu.errors.record(error.to_string());
            eprintln!("mui-vello: replacing GPU surface failed: {error}");
        }
    }

    /// Release the previous surface, then validate a replacement against every candidate.
    /// Success changes the device generation; re-upload external textures.
    /// Failure preserves the device and generation, but leaves no surface;
    /// presents return [`Frame::SurfaceLost`] until replacement succeeds.
    pub fn try_replace_surface(
        &mut self,
        surface: wgpu::Surface<'static>,
    ) -> Result<(), HostError> {
        // EGL permits one configured window surface per native window. Wgpu
        // creates it during configure, so release the old swapchain first.
        self.surface = None;
        let gpu = OnDevice::open(
            &self.instance,
            Some(&surface),
            self.wanted.unwrap_or((1, 1)),
            self.transparency,
        )?;
        self.surface = Some(surface);
        self.gpu = gpu;
        self.generation = self.generation.wrapping_add(1);
        self.retry_at = None;
        self.first_frame = true;
        Ok(())
    }

    /// Paint `scene` under `xf` and present it. `Err` is a render or
    /// device-rebuild failure, not a lost surface; painting the same scene
    /// again would fail the same way.
    pub fn present(&mut self, scene: &ResolvedScene, xf: Affine) -> Result<Frame, HostError> {
        self.present_inner::<fn(&mut Classic<'_>)>(scene, xf, None)
            .inspect_err(|error| {
                crate::diagnostics::error("mui-vello", "present_failed", &error.to_string());
            })
    }

    /// [`Host::present`] with `overlay` drawn on top, never retained.
    pub fn present_with_overlay<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        xf: Affine,
        overlay: F,
    ) -> Result<Frame, HostError> {
        self.present_inner(scene, xf, Some(overlay))
            .inspect_err(|error| {
                crate::diagnostics::error("mui-vello", "present_failed", &error.to_string());
            })
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
        let Some(surface) = &self.surface else {
            return Ok(Frame::SurfaceLost);
        };
        if self.gpu.poll_lost() {
            let now = Instant::now();
            if self.retry_at.is_some_and(|at| now < at) {
                return Ok(Frame::Skipped);
            }
            match OnDevice::open(&self.instance, Some(surface), size, self.transparency) {
                Ok(gpu) => {
                    self.gpu = gpu;
                    self.generation = self.generation.wrapping_add(1);
                    self.first_frame = true;
                    self.retry_at = None;
                    return Ok(Frame::Skipped);
                }
                Err(e) => {
                    self.retry_at = Some(now + RETRY);
                    return Err(HostError::DeviceLost(Box::new(e)));
                }
            }
        }
        // Callers may have logged a failed resize. Retry it before acquiring a
        // surface configured to a different extent than the retained renderer.
        self.resize(size.0, size.1)?;
        let Some(surface) = &self.surface else {
            return Ok(Frame::SurfaceLost);
        };
        let operation = self.first_frame.then(|| {
            crate::diagnostics::operation(
                "mui-vello",
                "first_frame",
                "first GPU frame completion was not confirmed",
                self.gpu
                    .diagnostics
                    .candidates
                    .iter()
                    .find(|c| c.stage == GpuStage::Ready)
                    .map(|c| &c.adapter),
            )
        });
        let OnDevice {
            device,
            queue,
            config,
            renderer,
            ..
        } = &mut self.gpu;
        if self.first_frame {
            renderer.invalidate();
        }
        if overlay.is_none() && renderer.is_current(scene, xf) {
            return Ok(Frame::Current);
        }
        let frame = match surface.get_current_texture() {
            Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
            Acquired::Outdated => {
                configure(surface, device, config).map_err(HostError::Configuration)?;
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
        // Driver compilation can fault after submit returns, before work completes.
        if let Some(operation) = operation {
            queue.on_submitted_work_done(move || drop(operation));
        }
        if self.first_frame {
            crate::diagnostics::breadcrumb(
                "mui-vello",
                "frame_presented",
                "first frame submitted to the window surface",
            );
            self.first_frame = false;
        }
        Ok(Frame::Presented(stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn native_backends_are_lazy_and_preserve_the_allowlist() {
        use wgpu::Backends as B;
        let mut tried = Vec::new();
        assert_eq!(
            try_backends(B::VULKAN | B::GL, |b| {
                tried.push(b);
                Ok(42)
            }),
            Ok(42)
        );
        assert_eq!(tried, [B::VULKAN]);
        tried.clear();
        assert_eq!(
            try_backends(B::VULKAN | B::GL, |b| {
                tried.push(b);
                if b == B::GL {
                    Ok(42)
                } else {
                    Err("no surface".into())
                }
            }),
            Ok(42)
        );
        assert_eq!(tried, [B::VULKAN, B::GL]);
        let error = try_backends::<()>(B::VULKAN, |_| Err("no surface".into())).unwrap_err();
        assert!(error.contains("no surface"));
        assert!(try_backends::<()>(B::empty(), |_| panic!("must not run")).is_err());
        assert!(
            try_backends::<()>(B::VULKAN, |_| panic!("shader diagnostic"))
                .unwrap_err()
                .contains("shader diagnostic")
        );
        assert_eq!(
            try_backends(B::VULKAN | B::GL, |b| {
                if b == B::VULKAN {
                    panic!("bad driver")
                }
                Ok(42)
            }),
            Ok(42)
        );
    }

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

    /// Translucent takes premultiplied, else straight alpha, else stays
    /// opaque; opaque is wgpu's `Auto` as it always was, even where a
    /// translucent mode is on offer.
    #[test]
    fn the_alpha_mode_follows_what_the_surface_offers() {
        use Transparency::*;
        use wgpu::CompositeAlphaMode as A;
        let vulkan_x11 = [A::PreMultiplied, A::Inherit];
        let metal = [A::Opaque, A::PostMultiplied];
        let dx12_hwnd = [A::Opaque];
        assert_eq!(alpha_mode(&vulkan_x11, Translucent), A::PreMultiplied);
        assert_eq!(alpha_mode(&metal, Translucent), A::PostMultiplied);
        assert_eq!(alpha_mode(&dx12_hwnd, Translucent), A::Auto);
        for offered in [&vulkan_x11[..], &metal, &dx12_hwnd] {
            assert_eq!(alpha_mode(offered, Opaque), A::Auto, "{offered:?}");
        }
        assert!(is_translucent(A::PreMultiplied) && is_translucent(A::PostMultiplied));
        assert!(!is_translucent(A::Auto) && !is_translucent(A::Inherit));
        assert_eq!(Transparency::default(), Opaque);
    }

    #[test]
    fn present_mode_follows_the_platform() {
        use wgpu::PresentMode as P;
        let mode = present_mode(&[P::Fifo, P::FifoRelaxed]);
        if cfg!(windows) {
            assert_eq!(mode, P::AutoNoVsync);
        } else {
            assert_eq!(mode, P::FifoRelaxed);
            assert_eq!(present_mode(&[P::Fifo]), P::Fifo);
        }
    }

    fn adapter_info(
        name: &str,
        device_type: wgpu::DeviceType,
        backend: wgpu::Backend,
    ) -> wgpu::AdapterInfo {
        let mut info = wgpu::AdapterInfo::new(device_type, backend);
        info.name = name.into();
        info.vendor = 0x1002;
        info.device = 1;
        info
    }

    #[test]
    fn downlevel_gl_is_rejected_without_weakening_vello_requirements() {
        let info = adapter_info("GL", wgpu::DeviceType::DiscreteGpu, wgpu::Backend::Gl);
        let requested = wgpu::Limits::default();
        let downlevel = wgpu::Limits::downlevel_webgl2_defaults();
        assert!(
            validate_renderer_requirements(
                &info,
                wgpu::DownlevelFlags::empty(),
                &requested,
                &downlevel
            )
            .unwrap_err()
            .contains("compute shaders")
        );
        assert!(
            validate_renderer_requirements(
                &info,
                wgpu::DownlevelFlags::COMPUTE_SHADERS,
                &requested,
                &downlevel
            )
            .unwrap_err()
            .contains("renderer limits")
        );
        assert!(
            validate_renderer_requirements(
                &info,
                wgpu::DownlevelFlags::COMPUTE_SHADERS,
                &requested,
                &requested
            )
            .is_ok()
        );
    }

    #[test]
    fn warp_is_rejected_before_device_creation_without_rejecting_hardware_or_mesa() {
        let limits = wgpu::Limits::default();
        let validate = |info: &wgpu::AdapterInfo| {
            validate_renderer_requirements(
                info,
                wgpu::DownlevelFlags::COMPUTE_SHADERS,
                &limits,
                &limits,
            )
        };
        let mut info = adapter_info(
            "Microsoft Basic Render Driver",
            wgpu::DeviceType::Cpu,
            wgpu::Backend::Dx12,
        );
        info.vendor = 0x1414;
        info.device = 0x008c;
        info.driver = "10.0.26100.33438".into();
        assert!(validate(&info).unwrap_err().contains("WARP"));
        info.backend = wgpu::Backend::Vulkan;
        assert!(validate(&info).is_ok());
        info.backend = wgpu::Backend::Dx12;
        info.device_type = wgpu::DeviceType::DiscreteGpu;
        for vendor in [0x10de, 0x8086, 0x1002, 0x1414] {
            info.vendor = vendor;
            assert!(validate(&info).is_ok());
        }
        info.device_type = wgpu::DeviceType::IntegratedGpu;
        info.vendor = 0x8086;
        assert!(validate(&info).is_ok());
    }

    #[test]
    fn stable_candidate_priority_keeps_software_fallback_and_prefers_native_backends() {
        use wgpu::{Backend as B, DeviceType as D};
        let mut candidates = [
            adapter_info("software", D::Cpu, B::Vulkan),
            adapter_info("integrated", D::IntegratedGpu, B::Vulkan),
            adapter_info("discrete GL", D::DiscreteGpu, B::Gl),
            adapter_info("first discrete", D::DiscreteGpu, B::Vulkan),
            adapter_info("second discrete", D::DiscreteGpu, B::Vulkan),
        ];
        candidates.sort_by_key(adapter_rank);
        assert_eq!(
            candidates.map(|info| info.name),
            [
                "first discrete",
                "second discrete",
                "discrete GL",
                "integrated",
                "software"
            ]
        );
    }

    #[test]
    fn failures_and_driver_panics_try_each_candidate_once_until_software_works() {
        let candidates = [
            wgpu::DeviceType::DiscreteGpu,
            wgpu::DeviceType::IntegratedGpu,
            wgpu::DeviceType::Cpu,
        ];
        let candidates = candidates.into_iter().enumerate().map(|(i, device_type)| {
            (
                i,
                AdapterDiagnostic {
                    adapter: adapter_info("candidate", device_type, wgpu::Backend::Vulkan),
                    supported_limits: wgpu::Limits::default(),
                    requested_limits: wgpu::Limits::default(),
                    stage: GpuStage::Capabilities,
                    format: None,
                    alpha_mode: None,
                    failure: None,
                },
            )
        });
        let mut attempts = Vec::new();
        let (chosen, report) = select_candidate(candidates, |i, candidate| {
            attempts.push(i);
            candidate.stage = GpuStage::Device;
            match i {
                0 => Err("preferred device refused".into()),
                1 => panic!("driver failed"),
                _ => Ok(i),
            }
        })
        .unwrap();
        assert_eq!(chosen, 2);
        assert_eq!(attempts, [0, 1, 2]);
        assert_eq!(
            report.candidates[0].failure.as_deref(),
            Some("preferred device refused")
        );
        assert_eq!(
            report.candidates[1].failure.as_deref(),
            Some("panic: driver failed")
        );
        assert_eq!(report.candidates[2].stage, GpuStage::Ready);
        assert!(report.candidates[2].failure.is_none());
    }

    #[test]
    fn initialization_reports_keep_failed_adapter_stages_through_device_loss() {
        let report = GpuDiagnostics {
            candidates: vec![AdapterDiagnostic {
                adapter: adapter_info(
                    "preferred",
                    wgpu::DeviceType::DiscreteGpu,
                    wgpu::Backend::Vulkan,
                ),
                supported_limits: wgpu::Limits::default(),
                requested_limits: wgpu::Limits::default(),
                stage: GpuStage::Device,
                format: Some(wgpu::TextureFormat::Bgra8Unorm),
                alpha_mode: Some(wgpu::CompositeAlphaMode::Auto),
                failure: Some("driver refused device".into()),
            }],
        };
        let error = HostError::DeviceLost(Box::new(HostError::Initialization(report)));
        let message = error.to_string();
        assert!(
            message.contains("preferred")
                && message.contains("Vulkan")
                && message.contains("driver refused device")
        );
        assert_eq!(
            error.diagnostics().unwrap().candidates[0].stage,
            GpuStage::Device
        );
        assert!(
            HostError::Initialization(GpuDiagnostics::default())
                .to_string()
                .contains("no adapters")
        );
    }

    #[test]
    fn callback_errors_are_observed_independently_and_coalesce_without_consuming() {
        let errors = DeviceErrors::default();
        let (mut a, mut b) = (0, 0);
        assert_eq!(errors.observe(&mut a), None);
        errors.record("first".into());
        assert_eq!(errors.observe(&mut a).as_deref(), Some("first"));
        assert_eq!(errors.observe(&mut a), None);
        errors.record("second".into());
        errors.record("last".into());
        assert_eq!(errors.observe(&mut a).as_deref(), Some("last"));
        assert_eq!(errors.observe(&mut b).as_deref(), Some("last"));
    }

    /// A real loss, not a flag flipped by hand: `Device::destroy` fires the
    /// lost callback, and a device opened again renders.
    #[test]
    #[ignore = "requires a native adapter with Vello compute support"]
    fn a_destroyed_device_is_seen_and_a_new_one_renders() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let gpu = OnDevice::open(&instance, None, (16, 16), Transparency::Opaque)
            .expect("native adapter");
        eprintln!("{}", gpu.diagnostics);
        assert_eq!(
            gpu.diagnostics.candidates.last().unwrap().stage,
            GpuStage::Ready
        );
        assert!(!gpu.lost());
        gpu.device.destroy();
        assert!(gpu.poll_lost(), "loss unseen");
        assert!(gpu.errors.observe(&mut 0).unwrap().contains("device lost"));
        let mut gpu2 = OnDevice::open(&instance, None, (16, 16), Transparency::Opaque).unwrap();
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

    #[test]
    #[ignore = "requires a native adapter with Vello compute support"]
    fn a_failed_replacement_state_requires_a_surface_before_presenting() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let gpu = OnDevice::open(&instance, None, (16, 16), Transparency::Opaque)
            .expect("native adapter");
        // A replacement failure retains the existing device and generation,
        // but has released the previous surface. Do not acquire or rebuild a
        // device until the caller supplies a new surface.
        let mut host = Host {
            instance,
            surface: None,
            gpu,
            wanted: Some((16, 16)),
            retry_at: None,
            generation: 4,
            transparency: Transparency::Opaque,
            first_frame: true,
        };
        let root = block(16., 16.).fill(Role::Primary);
        let scene = resolve(&SceneSpec::new(root).offered(Size::new(16., 16.))).unwrap();
        assert!(matches!(
            host.present(&scene, Affine::IDENTITY),
            Ok(Frame::SurfaceLost)
        ));
        host.gpu.device.destroy();
        assert!(matches!(
            host.present(&scene, Affine::IDENTITY),
            Ok(Frame::SurfaceLost)
        ));
        assert_eq!(
            host.generation(),
            4,
            "missing surface cannot trigger a device rebuild"
        );
    }
    #[test]
    #[ignore = "requires a native adapter with Vello compute support"]
    fn expose_and_same_size_restore_require_full_repaint() {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let gpu = OnDevice::open(&instance, None, (16, 16), Transparency::Opaque).unwrap();
        let mut host = Host {
            instance,
            surface: None,
            gpu,
            wanted: Some((16, 16)),
            retry_at: None,
            generation: 0,
            transparency: Transparency::Opaque,
            first_frame: true,
        };
        let scene = resolve(&SceneSpec::new(block(16., 16.).fill(Role::Primary))).unwrap();
        let target = host.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("first-frame regression"),
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
        let view = target.create_view(&Default::default());
        let first = host
            .gpu
            .renderer
            .render(&scene, Affine::IDENTITY, &view)
            .unwrap();
        assert_eq!(first.encoded_scenes, 1);
        assert!(host.gpu.renderer.is_current(&scene, Affine::IDENTITY));
        host.first_frame = false;
        host.invalidate();
        assert!(host.first_frame);
        assert!(!host.gpu.renderer.is_current(&scene, Affine::IDENTITY));
        assert_eq!(
            host.gpu
                .renderer
                .render(&scene, Affine::IDENTITY, &view)
                .unwrap()
                .encoded_scenes,
            1
        );
        host.first_frame = false;
        host.resize(0, 0).unwrap();
        host.resize(16, 16).unwrap();
        assert!(
            host.first_frame,
            "restore at the same size must force first present"
        );
        assert!(!host.gpu.renderer.is_current(&scene, Affine::IDENTITY));
        host.gpu
            .renderer
            .render(&scene, Affine::IDENTITY, &view)
            .unwrap();
        host.first_frame = false;
        host.resize(32, 32).unwrap();
        assert!(host.first_frame);
        assert!(!host.gpu.renderer.is_current(&scene, Affine::IDENTITY));
    }
}
