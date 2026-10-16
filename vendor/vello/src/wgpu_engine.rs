// Copyright 2023 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
// Keys are handles and small ids hashed every dispatch, never attacker input.
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, CommandEncoder, CommandEncoderDescriptor,
    ComputePass, ComputePassDescriptor, ComputePipeline, Device, PipelineCache,
    PipelineCompilationOptions, Queue, Texture, TextureAspect, TextureUsages, TextureView,
    TextureViewDimension,
};

use crate::{
    Error, Result,
    low_level::{BufferProxy, Command, ImageProxy, Recording, ResourceId, ResourceProxy, ShaderId},
    recording::BindType,
};
use vello_shaders::cpu::CpuBinding;

#[cfg(not(target_arch = "wasm32"))]
struct UninitialisedShader {
    wgsl: Cow<'static, str>,
    label: &'static str,
    entries: Vec<wgpu::BindGroupLayoutEntry>,
    shader_id: ShaderId,
}

#[derive(Default)]
pub(crate) struct WgpuEngine {
    shaders: Vec<Shader>,
    // Dynamic execution errors can invalidate persistent GPU resources. Refuse
    // further work until the renderer is recreated, rather than reuse stale IDs.
    invalidated: bool,
    pool: ResourcePool,
    bind_map: BindMap,
    bind_groups: BindGroupCache,
    downloads: HashMap<ResourceId, Buffer>,
    #[cfg(not(target_arch = "wasm32"))]
    shaders_to_initialise: Option<Vec<UninitialisedShader>>,
    pub(crate) use_cpu: bool,
    /// Overrides from a specific `Image::data`'s [`id`](peniko::Blob::id) to a wgpu `Texture`.
    ///
    /// The `Texture` should have the same size as the `Image`.
    pub(crate) image_overrides: HashMap<u64, wgpu::TexelCopyTextureInfoBase<Texture>>,
    pipeline_cache: Option<PipelineCache>,
}

enum PipelineState {
    Compute(ComputePipeline),
    #[cfg(feature = "debug_layers")]
    Render(wgpu::RenderPipeline),
}

struct WgpuShader {
    pipeline: PipelineState,
    bind_group_layout: BindGroupLayout,
}

pub(crate) enum CpuShaderType {
    Present(fn(u32, &[CpuBinding<'_>])),
    Missing,
    Skipped,
}

struct CpuShader {
    shader: fn(u32, &[CpuBinding<'_>]),
}

enum ShaderKind<'a> {
    Wgpu(&'a WgpuShader),
    Cpu(&'a CpuShader),
}

struct Shader {
    label: &'static str,
    wgpu: Option<WgpuShader>,
    cpu: Option<CpuShader>,
}

impl Shader {
    fn select(&self) -> Result<ShaderKind<'_>> {
        if let Some(cpu) = self.cpu.as_ref() {
            Ok(ShaderKind::Cpu(cpu))
        } else if let Some(wgpu) = self.wgpu.as_ref() {
            Ok(ShaderKind::Wgpu(wgpu))
        } else {
            Err(Error::MissingShaderImplementation(self.label))
        }
    }
}

pub(crate) enum ExternalResource<'a> {
    #[expect(unused, reason = "No buffers are accepted as arguments currently")]
    Buffer(BufferProxy, &'a Buffer),
    Image(ImageProxy, &'a TextureView),
}

/// A buffer can exist either on the GPU or on CPU.
enum MaterializedBuffer {
    Gpu(Buffer),
    Cpu(RefCell<Vec<u8>>),
}

struct BindMapBuffer {
    buffer: MaterializedBuffer,
    label: &'static str,
}

#[derive(Default)]
struct BindMap {
    buf_map: HashMap<ResourceId, BindMapBuffer>,
    image_map: HashMap<ResourceId, (Texture, TextureView)>,
    pending_clears: HashSet<ResourceId>,
}

#[derive(Hash, PartialEq, Eq)]
struct BufferProperties {
    size: u64,
    usages: BufferUsages,
    name: &'static str,
}

#[derive(Default)]
struct ResourcePool {
    bufs: HashMap<BufferProperties, Vec<Buffer>>,
    /// MUI patch: images the last recording freed, for this one to reuse
    /// (a gradient ramp is a fresh image per frame); the rest are destroyed
    /// when it ends.
    images: Vec<(Texture, TextureView)>,
    spare_images: Vec<(Texture, TextureView)>,
}

/// MUI patch: bind groups kept across recordings, keyed by what they bind.
/// A frame like the last one draws the same pooled buffers in the same order
/// (see the free order in `run_recording`), so its bind groups are the last
/// frame's. The key holds clones of the handles, so an address it hashes
/// cannot be reused by another object while the entry lives.
#[derive(Default)]
struct BindGroupCache {
    map: HashMap<(BindGroupLayout, Vec<Bound>), (BindGroup, u64)>,
    /// Recordings run so far.
    now: u64,
}

/// An entry goes once this many recordings passed without using it.
const BIND_GROUP_RECORDINGS: u64 = 4;

#[derive(Clone, PartialEq, Eq, Hash)]
enum Bound {
    Buffer(Buffer, u64, Option<core::num::NonZeroU64>),
    View(TextureView),
}

/// The transient bind map contains short-lifetime resources.
///
/// In particular, it has resources scoped to a single call of
/// `run_recording()`, including external resources and also buffer
/// uploads.
#[derive(Default)]
struct TransientBindMap<'a> {
    bufs: HashMap<ResourceId, TransientBuf<'a>>,
    // TODO: create transient image type
    images: HashMap<ResourceId, &'a TextureView>,
}

enum TransientBuf<'a> {
    Cpu(&'a [u8]),
    Gpu(&'a Buffer),
}

impl WgpuEngine {
    pub fn new(use_cpu: bool, pipeline_cache: Option<PipelineCache>) -> Self {
        Self {
            use_cpu,
            pipeline_cache,
            ..Default::default()
        }
    }

    /// Enable creating any remaining shaders in parallel
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_parallel_initialisation(&mut self) {
        if self.shaders_to_initialise.is_some() {
            return;
        }
        self.shaders_to_initialise = Some(Vec::new());
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Initialise (in parallel) any shaders which are yet to be created
    pub fn build_shaders_if_needed(
        &mut self,
        device: &Device,
        num_threads: Option<std::num::NonZeroUsize>,
    ) {
        use std::num::NonZeroUsize;

        if let Some(mut new_shaders) = self.shaders_to_initialise.take() {
            let num_threads = num_threads
                .map(NonZeroUsize::get)
                .unwrap_or_else(|| {
                    // Fallback onto a heuristic. This tries to not to use all threads.
                    // We keep the main thread blocked and not doing much whilst this is running,
                    // so we broadly leave two cores unused at the point of maximum parallelism
                    // (This choice is arbitrary, and could be tuned, although a 'proper' threadpool
                    // should probably be used instead)
                    std::thread::available_parallelism().map_or(2, |it| it.get().max(4) - 2)
                })
                .min(new_shaders.len());
            log::info!("Initialising shader modules in parallel using {num_threads} threads");
            let remainder = new_shaders.split_off(num_threads);
            let (tx, rx) = std::sync::mpsc::channel::<(ShaderId, WgpuShader)>();

            // We expect each initialisation to take much longer than acquiring a lock, so we just
            // use a mutex for our work queue
            let work_queue = std::sync::Mutex::new(remainder.into_iter());
            let work_queue = &work_queue;
            std::thread::scope(|scope| {
                let pipeline_cache = self.pipeline_cache.as_ref();
                let tx = tx;
                new_shaders
                    .into_iter()
                    .map(|it| {
                        let tx = tx.clone();
                        std::thread::Builder::new()
                            .name("Vello shader initialisation worker thread".into())
                            .spawn_scoped(scope, move || {
                                let shader = Self::create_compute_pipeline(
                                    device,
                                    it.label,
                                    it.wgsl,
                                    it.entries,
                                    pipeline_cache,
                                );
                                // We know the rx can only be closed if all the tx references are dropped
                                tx.send((it.shader_id, shader)).unwrap();
                                while let Ok(mut guard) = work_queue.lock() {
                                    if let Some(value) = guard.next() {
                                        drop(guard);
                                        let shader = Self::create_compute_pipeline(
                                            device,
                                            value.label,
                                            value.wgsl,
                                            value.entries,
                                            pipeline_cache,
                                        );
                                        tx.send((value.shader_id, shader)).unwrap();
                                    } else {
                                        break;
                                    }
                                }
                                // Another thread panicked or we finished.
                                // If another thread panicked, we ignore that here and finish our processing
                                drop(tx);
                            })
                            .expect("failed to spawn thread");
                    })
                    .for_each(drop);
                // Drop the initial sender, to mean that there will be no more senders if and only if all other threads have finished
                drop(tx);

                while let Ok((id, value)) = rx.recv() {
                    self.shaders[id.0].wgpu = Some(value);
                }
            });
        }
    }

    /// Add a shader.
    ///
    /// This function is somewhat limited, it doesn't apply a label, only allows one bind group,
    /// doesn't support push constants, and entry point is hardcoded as "main".
    ///
    /// Maybe should do template instantiation here? But shader compilation pipeline feels maybe
    /// a bit separate.
    pub fn add_compute_shader(
        &mut self,
        device: &Device,
        label: &'static str,
        wgsl: Cow<'static, str>,
        layout: &[BindType],
        cpu_shader: CpuShaderType,
    ) -> ShaderId {
        let mut add = |shader| {
            let id = self.shaders.len();
            self.shaders.push(shader);
            ShaderId(id)
        };

        if self.use_cpu {
            match cpu_shader {
                CpuShaderType::Present(shader) => {
                    return add(Shader {
                        wgpu: None,
                        cpu: Some(CpuShader { shader }),
                        label,
                    });
                }
                // This shader is unused in CPU mode, create a dummy shader
                CpuShaderType::Skipped => {
                    return add(Shader {
                        wgpu: None,
                        cpu: None,
                        label,
                    });
                }
                // Create a GPU shader as we don't have a CPU shader
                CpuShaderType::Missing => {}
            }
        }

        let entries = Self::create_bind_group_layout_entries(
            layout.iter().map(|b| (*b, wgpu::ShaderStages::COMPUTE)),
        );
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(uninit) = self.shaders_to_initialise.as_mut() {
            let id = add(Shader {
                label,
                wgpu: None,
                cpu: None,
            });
            uninit.push(UninitialisedShader {
                wgsl,
                label,
                entries,
                shader_id: id,
            });
            return id;
        }
        let wgpu = Self::create_compute_pipeline(
            device,
            label,
            wgsl,
            entries,
            self.pipeline_cache.as_ref(),
        );
        add(Shader {
            wgpu: Some(wgpu),
            cpu: None,
            label,
        })
    }

    #[cfg(feature = "debug_layers")]
    pub fn add_render_shader(
        &mut self,
        device: &Device,
        label: &'static str,
        module: &wgpu::ShaderModule,
        vertex_main: &'static str,
        fragment_main: &'static str,
        topology: wgpu::PrimitiveTopology,
        color_attachment: wgpu::ColorTargetState,
        vertex_buffer: Option<wgpu::VertexBufferLayout<'_>>,
        bind_layout: &[(BindType, wgpu::ShaderStages)],
    ) -> ShaderId {
        let entries = Self::create_bind_group_layout_entries(bind_layout.iter().copied());
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        // No buffer is no slot, not one empty slot.
        let buffers = [vertex_buffer];
        let buffers = if buffers[0].is_some() {
            &buffers[..]
        } else {
            &[]
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some(vertex_main),
                buffers,
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fragment_main),
                targets: &[Some(color_attachment)],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: self.pipeline_cache.as_ref(),
        });
        let id = self.shaders.len();
        self.shaders.push(Shader {
            wgpu: Some(WgpuShader {
                pipeline: PipelineState::Render(pipeline),
                bind_group_layout,
            }),
            cpu: None,
            label,
        });
        ShaderId(id)
    }

    fn validate_recording(&self, recording: &Recording) -> Result<()> {
        if self.invalidated {
            return Err(Error::RendererInvalidated);
        }
        for command in &recording.commands {
            match command {
                Command::WriteImage(_, _, image) => {
                    if image.data.is_empty() && image.width != 0 && image.height != 0
                        && !self.image_overrides.contains_key(&image.data.id())
                    {
                        return Err(Error::UnavailableImage(image.data.id()));
                    }
                }
                Command::Dispatch(id, _, bindings) | Command::DispatchIndirect(id, _, _, bindings) => {
                    let shader = self.shaders.get(id.0).ok_or(Error::UnavailableShader(id.0))?;
                    match shader.select()? {
                        ShaderKind::Cpu(_) => {
                            for binding in bindings {
                                match binding {
                                    ResourceProxy::Buffer(_) => {}
                                    ResourceProxy::BufferRange { .. } => return Err(Error::UnsupportedCpuBinding("buffer range")),
                                    ResourceProxy::Image(_) => return Err(Error::UnsupportedCpuBinding("image")),
                                }
                            }
                        }
                        ShaderKind::Wgpu(gpu) => match &gpu.pipeline {
                            PipelineState::Compute(_) => {}
                            #[cfg(feature = "debug_layers")]
                            PipelineState::Render(_) => return Err(Error::InvalidShaderUse {
                                shader: shader.label, operation: "compute dispatch",
                            }),
                        },
                    }
                }
                #[cfg(feature = "debug_layers")]
                Command::Draw(params) => {
                    let shader = self.shaders.get(params.shader_id.0)
                        .ok_or(Error::UnavailableShader(params.shader_id.0))?;
                    match shader.select()? {
                        ShaderKind::Wgpu(gpu) if matches!(gpu.pipeline, PipelineState::Render(_)) => {}
                        _ => return Err(Error::InvalidShaderUse { shader: shader.label, operation: "draw" }),
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Static preflight refusals leave resident resources untouched. A dynamic
    /// execution failure drops the unsubmitted encoder, releases all resident
    /// engine resources, and poisons this engine. Its resolver/atlas IDs may be
    /// stale: the caller must recreate the renderer, never retry into a blank frame.
    pub fn run_recording(
        &mut self,
        device: &Device,
        queue: &Queue,
        recording: &Recording,
        external_resources: &[ExternalResource<'_>],
        label: &'static str,
        #[cfg(feature = "wgpu-profiler")] profiler: &mut wgpu_profiler::GpuProfiler,
    ) -> Result<()> {
        self.validate_recording(recording)?;
        let result = self.run_recording_inner(
            device, queue, recording, external_resources, label,
            #[cfg(feature = "wgpu-profiler")]
            profiler,
        );
        // Inner locals (including passes/encoder) have been dropped before
        // releasing handles here. No rollback of already queued writes is claimed.
        self.finish_recording(result)
    }

    fn finish_recording(&mut self, result: Result<()>) -> Result<()> {
        if result.is_err() {
            self.invalidated = true;
            self.bind_groups = BindGroupCache::default();
            self.bind_map = BindMap::default();
            self.pool = ResourcePool::default();
            self.downloads.clear();
            self.image_overrides.clear();
        }
        result
    }

    fn run_recording_inner(
        &mut self,
        device: &Device,
        queue: &Queue,
        recording: &Recording,
        external_resources: &[ExternalResource<'_>],
        label: &'static str,
        #[cfg(feature = "wgpu-profiler")] profiler: &mut wgpu_profiler::GpuProfiler,
    ) -> Result<()> {
        // MUI patch: in order, and returned to the pool in reverse, so the
        // next recording like this one pops the same buffers for the same
        // proxies and finds its bind groups cached.
        let mut free_bufs: Vec<ResourceId> = Vec::new();
        let mut free_images: HashSet<ResourceId> = HashSet::default();
        self.pool.spare_images = std::mem::take(&mut self.pool.images);
        let mut transient_map = TransientBindMap::new(external_resources);

        let mut encoder =
            device.create_command_encoder(&CommandEncoderDescriptor { label: Some(label) });
        // MUI patch: consecutive dispatches share one compute pass (wgpu
        // still synchronizes each dispatch); anything else recorded on the
        // encoder ends it first. A pass per dispatch cost a hal command
        // buffer and a usage scope each.
        let mut pass: Option<ComputePass<'static>> = None;
        self.bind_groups.now += 1;
        #[cfg(feature = "wgpu-profiler")]
        let query = profiler.begin_query(label, &mut encoder);
        for command in &recording.commands {
            match command {
                Command::Upload(buf_proxy, bytes) => {
                    transient_map
                        .bufs
                        .insert(buf_proxy.id, TransientBuf::Cpu(bytes));
                    // TODO: restrict VERTEX usage to "debug_layers" feature?
                    let usage = BufferUsages::COPY_SRC
                        | BufferUsages::COPY_DST
                        | BufferUsages::STORAGE
                        | BufferUsages::VERTEX;
                    let buf = self
                        .pool
                        .get_buf(buf_proxy.size, buf_proxy.name, usage, device);
                    // TODO: if buffer is newly created, might be better to make it mapped at creation
                    // and copy. However, we expect reuse will be most common.
                    queue.write_buffer(&buf, 0, bytes);
                    self.bind_map.insert_buf(buf_proxy, buf);
                }
                Command::UploadUniform(buf_proxy, bytes) => {
                    transient_map
                        .bufs
                        .insert(buf_proxy.id, TransientBuf::Cpu(bytes));
                    let usage = BufferUsages::UNIFORM | BufferUsages::COPY_DST;
                    // Same consideration as above
                    let buf = self
                        .pool
                        .get_buf(buf_proxy.size, buf_proxy.name, usage, device);
                    queue.write_buffer(&buf, 0, bytes);
                    self.bind_map.insert_buf(buf_proxy, buf);
                }
                Command::UploadImage(image_proxy, bytes) => {
                    let format = image_proxy.format.to_wgpu();
                    let block_size = format
                        .block_copy_size(None)
                        .expect("ImageFormat must have a valid block size");
                    let (texture, texture_view) = self.pool.get_image(image_proxy, device);
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                            aspect: TextureAspect::All,
                        },
                        bytes,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(image_proxy.width * block_size),
                            rows_per_image: None,
                        },
                        wgpu::Extent3d {
                            width: image_proxy.width,
                            height: image_proxy.height,
                            depth_or_array_layers: 1,
                        },
                    );
                    self.bind_map
                        .insert_image(image_proxy.id, texture, texture_view);
                }
                Command::WriteImage(proxy, [x, y], image) => {
                    let (texture, _) = self
                        .bind_map
                        .image_map
                        .entry(proxy.id)
                        .or_insert_with(|| self.pool.get_image(proxy, device));
                    let format = proxy.format.to_wgpu();
                    let block_size = format
                        .block_copy_size(None)
                        .expect("ImageFormat must have a valid block size");
                    if let Some(overrider) = self.image_overrides.get(&image.data.id()) {
                        pass = None;
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &overrider.texture,
                                mip_level: overrider.mip_level,
                                origin: overrider.origin,
                                aspect: overrider.aspect,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d { x: *x, y: *y, z: 0 },
                                aspect: TextureAspect::All,
                            },
                            wgpu::Extent3d {
                                width: image.width,
                                height: image.height,
                                depth_or_array_layers: 1,
                            },
                        );
                    } else {
                        queue.write_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d { x: *x, y: *y, z: 0 },
                                aspect: TextureAspect::All,
                            },
                            image.data.data(),
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(image.width * block_size),
                                rows_per_image: None,
                            },
                            wgpu::Extent3d {
                                width: image.width,
                                height: image.height,
                                depth_or_array_layers: 1,
                            },
                        );
                    }
                }
                Command::Dispatch(shader_id, wg_size, bindings) => {
                    let (x, y, z) = *wg_size;
                    // println!("dispatching {:?} with {} bindings", wg_size, bindings.len());
                    let shader = &self.shaders[shader_id.0];
                    match shader.select()? {
                        ShaderKind::Cpu(cpu_shader) => {
                            // The current strategy is to run the CPU shader synchronously. This
                            // works because there is currently the added constraint that data
                            // can only flow from CPU to GPU, not the other way around. If and
                            // when we implement that, we will need to defer the execution. Of
                            // course, we will also need to wire up more async synchronization
                            // mechanisms, as the CPU dispatch can't run until the preceding
                            // command buffer submission completes (and, in WebGPU, the async
                            // mapping operations on the buffers completes).
                            let resources =
                                transient_map.create_cpu_resources(&mut self.bind_map, bindings)?;
                            (cpu_shader.shader)(x, &resources);
                        }
                        ShaderKind::Wgpu(wgpu_shader) => {
                            // Workaround for https://github.com/linebender/vello/issues/637
                            if x == 0 || y == 0 || z == 0 {
                                continue;
                            }
                            let bind_group = transient_map.create_bind_group(
                                &mut self.bind_map,
                                &mut self.pool,
                                device,
                                queue,
                                &mut encoder,
                                &mut pass,
                                &mut self.bind_groups,
                                &wgpu_shader.bind_group_layout,
                                bindings,
                            );
                            let cpass = pass.get_or_insert_with(|| {
                                encoder
                                    .begin_compute_pass(&ComputePassDescriptor::default())
                                    .forget_lifetime()
                            });
                            #[cfg(feature = "wgpu-profiler")]
                            let query = profiler
                                .begin_query(shader.label, cpass)
                                .with_parent(Some(&query));
                            let pipeline = match &wgpu_shader.pipeline {
                                PipelineState::Compute(pipeline) => pipeline,
                                #[cfg(feature = "debug_layers")]
                                PipelineState::Render(_) => return Err(Error::InvalidShaderUse {
                                    shader: shader.label, operation: "compute dispatch",
                                }),
                            };
                            cpass.set_pipeline(pipeline);
                            cpass.set_bind_group(0, &bind_group, &[]);
                            cpass.dispatch_workgroups(x, y, z);
                            #[cfg(feature = "wgpu-profiler")]
                            profiler.end_query(cpass, query);
                        }
                    }
                }
                Command::DispatchIndirect(shader_id, proxy, offset, bindings) => {
                    let shader = &self.shaders[shader_id.0];
                    match shader.select()? {
                        ShaderKind::Cpu(cpu_shader) => {
                            // Same consideration as above about running the CPU shader synchronously.
                            let n_wg = transient_map.indirect_workgroups(&self.bind_map, proxy, *offset)?;
                            let resources =
                                transient_map.create_cpu_resources(&mut self.bind_map, bindings)?;
                            (cpu_shader.shader)(n_wg, &resources);
                        }
                        ShaderKind::Wgpu(wgpu_shader) => {
                            let bind_group = transient_map.create_bind_group(
                                &mut self.bind_map,
                                &mut self.pool,
                                device,
                                queue,
                                &mut encoder,
                                &mut pass,
                                &mut self.bind_groups,
                                &wgpu_shader.bind_group_layout,
                                bindings,
                            );
                            transient_map.materialize_gpu_buf_for_indirect(
                                &mut self.bind_map,
                                &mut self.pool,
                                device,
                                queue,
                                proxy,
                            );
                            let cpass = pass.get_or_insert_with(|| {
                                encoder
                                    .begin_compute_pass(&ComputePassDescriptor::default())
                                    .forget_lifetime()
                            });
                            #[cfg(feature = "wgpu-profiler")]
                            let query = profiler
                                .begin_query(shader.label, cpass)
                                .with_parent(Some(&query));
                            let pipeline = match &wgpu_shader.pipeline {
                                PipelineState::Compute(pipeline) => pipeline,
                                #[cfg(feature = "debug_layers")]
                                PipelineState::Render(_) => return Err(Error::InvalidShaderUse {
                                    shader: shader.label, operation: "compute dispatch",
                                }),
                            };
                            cpass.set_pipeline(pipeline);
                            cpass.set_bind_group(0, &bind_group, &[]);
                            let buf = self.bind_map.get_gpu_buf(proxy.id).ok_or(
                                Error::UnavailableBufferUsed(proxy.name, "indirect dispatch"),
                            )?;
                            cpass.dispatch_workgroups_indirect(buf, *offset);
                            #[cfg(feature = "wgpu-profiler")]
                            profiler.end_query(cpass, query);
                        }
                    }
                }
                #[cfg(feature = "debug_layers")]
                Command::Draw(draw_params) => {
                    let shader = &self.shaders[draw_params.shader_id.0];
                    #[cfg(feature = "wgpu-profiler")]
                    let label = shader.label;
                    let ShaderKind::Wgpu(shader) = shader.select()? else {
                        return Err(Error::InvalidShaderUse { shader: shader.label, operation: "draw" });
                    };
                    let bind_group = transient_map.create_bind_group(
                        &mut self.bind_map,
                        &mut self.pool,
                        device,
                        queue,
                        &mut encoder,
                        &mut pass,
                        &mut self.bind_groups,
                        &shader.bind_group_layout,
                        &draw_params.resources,
                    );
                    pass = None;
                    let render_target = transient_map
                        .materialize_external_image_for_render_pass(&draw_params.target)?;
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: render_target,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: match draw_params.clear_color {
                                    Some(c) => wgpu::LoadOp::Clear(wgpu::Color {
                                        r: c[0] as f64,
                                        g: c[1] as f64,
                                        b: c[2] as f64,
                                        a: c[3] as f64,
                                    }),
                                    None => wgpu::LoadOp::Load,
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                        multiview_mask: None,
                    });
                    #[cfg(feature = "wgpu-profiler")]
                    let query = profiler
                        .begin_query(label, &mut rpass)
                        .with_parent(Some(&query));
                    let PipelineState::Render(pipeline) = &shader.pipeline else {
                        return Err(Error::InvalidShaderUse {
                            shader: self.shaders[draw_params.shader_id.0].label, operation: "draw",
                        });
                    };
                    rpass.set_pipeline(pipeline);
                    if let Some(proxy) = draw_params.vertex_buffer {
                        // TODO: need a way to materialize a CPU initialized buffer. For now assume
                        // buffer exists? Also, need to materialize this buffer with vertex usage
                        let buf = self
                            .bind_map
                            .get_gpu_buf(proxy.id)
                            .ok_or(Error::UnavailableBufferUsed(proxy.name, "draw"))?;
                        rpass.set_vertex_buffer(0, buf.slice(..));
                    }
                    rpass.set_bind_group(0, &bind_group, &[]);
                    rpass.draw(0..draw_params.vertex_count, 0..draw_params.instance_count);
                    #[cfg(feature = "wgpu-profiler")]
                    profiler.end_query(&mut rpass, query);
                }
                Command::Download(proxy) => {
                    let src_buf = self
                        .bind_map
                        .get_gpu_buf(proxy.id)
                        .ok_or(Error::UnavailableBufferUsed(proxy.name, "download"))?;
                    let usage = BufferUsages::MAP_READ | BufferUsages::COPY_DST;
                    let buf = self.pool.get_buf(proxy.size, "download", usage, device);
                    pass = None;
                    encoder.copy_buffer_to_buffer(src_buf, 0, &buf, 0, proxy.size);
                    self.downloads.insert(proxy.id, buf);
                }
                Command::Clear(proxy, offset, size) => {
                    if let Some(buf) = self.bind_map.get_buf(*proxy) {
                        match &buf.buffer {
                            MaterializedBuffer::Gpu(b) => {
                                pass = None;
                                encoder.clear_buffer(b, *offset, *size);
                            }
                            MaterializedBuffer::Cpu(b) => {
                                let mut slice = &mut b.borrow_mut()[*offset as usize..];
                                if let Some(size) = size {
                                    slice = &mut slice[..*size as usize];
                                }
                                slice.fill(0);
                            }
                        }
                    } else {
                        self.bind_map.pending_clears.insert(proxy.id);
                    }
                }
                Command::FreeBuffer(proxy) => {
                    free_bufs.push(proxy.id);
                }
                Command::FreeImage(proxy) => {
                    free_images.insert(proxy.id);
                }
            }
        }
        drop(pass);
        #[cfg(feature = "wgpu-profiler")]
        profiler.end_query(&mut encoder, query);
        // TODO: This only actually needs to happen once per frame, but run_recording happens two or three times
        #[cfg(feature = "wgpu-profiler")]
        profiler.resolve_queries(&mut encoder);
        queue.submit(Some(encoder.finish()));
        for id in free_bufs.into_iter().rev() {
            if let Some(buf) = self.bind_map.buf_map.remove(&id)
                && let MaterializedBuffer::Gpu(gpu_buf) = buf.buffer
            {
                let props = BufferProperties {
                    size: gpu_buf.size(),
                    usages: gpu_buf.usage(),
                    name: buf.label,
                };
                self.pool.bufs.entry(props).or_default().push(gpu_buf);
            }
        }
        let now = self.bind_groups.now;
        self.bind_groups
            .map
            .retain(|_, (_, used)| now - *used < BIND_GROUP_RECORDINGS);
        for id in free_images {
            if let Some(image) = self.bind_map.image_map.remove(&id) {
                self.pool.images.push(image);
            }
        }
        for (texture, _) in self.pool.spare_images.drain(..) {
            texture.destroy();
        }
        Ok(())
    }

    pub fn get_download(&self, buf: BufferProxy) -> Option<&Buffer> {
        self.downloads.get(&buf.id)
    }

    pub fn free_download(&mut self, buf: BufferProxy) {
        self.downloads.remove(&buf.id);
    }

    fn create_bind_group_layout_entries(
        layout: impl Iterator<Item = (BindType, wgpu::ShaderStages)>,
    ) -> Vec<wgpu::BindGroupLayoutEntry> {
        layout
            .enumerate()
            .map(|(i, (bind_type, visibility))| match bind_type {
                BindType::Buffer | BindType::BufReadOnly => wgpu::BindGroupLayoutEntry {
                    binding: i as u32,
                    visibility,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage {
                            read_only: bind_type == BindType::BufReadOnly,
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindType::Uniform => wgpu::BindGroupLayoutEntry {
                    binding: i as u32,
                    visibility,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindType::Image(format) | BindType::ImageRead(format) => {
                    wgpu::BindGroupLayoutEntry {
                        binding: i as u32,
                        visibility,
                        ty: if bind_type == BindType::ImageRead(format) {
                            wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: TextureViewDimension::D2,
                                multisampled: false,
                            }
                        } else {
                            wgpu::BindingType::StorageTexture {
                                access: wgpu::StorageTextureAccess::WriteOnly,
                                format: format.to_wgpu(),
                                view_dimension: TextureViewDimension::D2,
                            }
                        },
                        count: None,
                    }
                }
            })
            .collect::<Vec<_>>()
    }

    fn create_compute_pipeline(
        device: &Device,
        label: &str,
        wgsl: Cow<'_, str>,
        entries: Vec<wgpu::BindGroupLayoutEntry>,
        cache: Option<&PipelineCache>,
    ) -> WgpuShader {
        // SAFETY: We only call this with trusted shaders (written by Vello developers)
        let shader_module = unsafe {
            device.create_shader_module_trusted(
                wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Wgsl(wgsl),
                },
                wgpu::ShaderRuntimeChecks::unchecked(),
            )
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&compute_pipeline_layout),
            module: &shader_module,
            entry_point: None,
            compilation_options: PipelineCompilationOptions {
                zero_initialize_workgroup_memory: false,
                ..Default::default()
            },
            cache,
        });
        WgpuShader {
            pipeline: PipelineState::Compute(pipeline),
            bind_group_layout,
        }
    }
}

fn read_indirect_arguments(proxy: &BufferProxy, offset: u64, bytes: &[u8]) -> Result<u32> {
    let invalid = || Error::InvalidIndirectDispatch { buffer: proxy.name, offset };
    if offset % 4 != 0 || offset.checked_add(12).is_none_or(|end| end > proxy.size) {
        return Err(invalid());
    }
    let start = usize::try_from(offset).map_err(|_| invalid())?;
    let end = start.checked_add(12).ok_or_else(invalid)?;
    let args = bytes.get(start..end).ok_or_else(invalid)?;
    Ok(u32::from_le_bytes([args[0], args[1], args[2], args[3]]))
}

impl BindMap {
    fn indirect_workgroups(&self, proxy: &BufferProxy, offset: u64) -> Result<u32> {
        let buffer = self.buf_map.get(&proxy.id)
            .ok_or(Error::UnavailableBufferUsed(proxy.name, "indirect CPU dispatch"))?;
        let MaterializedBuffer::Cpu(bytes) = &buffer.buffer else {
            return Err(Error::UnavailableBufferUsed(proxy.name, "indirect CPU dispatch"));
        };
        read_indirect_arguments(proxy, offset, &bytes.borrow())
    }

    fn insert_buf(&mut self, proxy: &BufferProxy, buffer: Buffer) {
        self.buf_map.insert(
            proxy.id,
            BindMapBuffer {
                buffer: MaterializedBuffer::Gpu(buffer),
                label: proxy.name,
            },
        );
    }

    /// Get a buffer, only if it's on GPU.
    fn get_gpu_buf(&self, id: ResourceId) -> Option<&Buffer> {
        self.buf_map.get(&id).and_then(|b| match &b.buffer {
            MaterializedBuffer::Gpu(b) => Some(b),
            _ => None,
        })
    }

    /// Get a CPU buffer, refusing missing or GPU-only storage.
    fn get_cpu_buf(&self, proxy: &BufferProxy) -> Result<CpuBinding<'_>> {
        match self.buf_map.get(&proxy.id).map(|buf| &buf.buffer) {
            Some(MaterializedBuffer::Cpu(b)) => Ok(CpuBinding::BufferRW(b)),
            _ => Err(Error::UnavailableBufferUsed(proxy.name, "CPU dispatch")),
        }
    }

    fn materialize_cpu_buf(&mut self, buf: &BufferProxy) {
        self.buf_map.entry(buf.id).or_insert_with(|| {
            let buffer = MaterializedBuffer::Cpu(RefCell::new(vec![0; buf.size as usize]));
            BindMapBuffer {
                buffer,
                // TODO: do we need to cfg this?
                label: buf.name,
            }
        });
    }

    fn insert_image(&mut self, id: ResourceId, image: Texture, image_view: TextureView) {
        self.image_map.insert(id, (image, image_view));
    }

    fn get_buf(&mut self, proxy: BufferProxy) -> Option<&BindMapBuffer> {
        self.buf_map.get(&proxy.id)
    }
}

const SIZE_CLASS_BITS: u32 = 1;

impl ResourcePool {
    /// Get a buffer from the pool or create one.
    fn get_buf(
        &mut self,
        size: u64,
        name: &'static str,
        usage: BufferUsages,
        device: &Device,
    ) -> Buffer {
        let rounded_size = Self::size_class(size, SIZE_CLASS_BITS);
        let props = BufferProperties {
            size: rounded_size,
            usages: usage,
            name,
        };
        if let Some(buf_vec) = self.bufs.get_mut(&props)
            && let Some(buf) = buf_vec.pop()
        {
            return buf;
        }
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(name),
            size: rounded_size,
            usage,
            mapped_at_creation: false,
        })
    }

    /// An image from the last recording's leftovers, or a new one.
    fn get_image(&mut self, proxy: &ImageProxy, device: &Device) -> (Texture, TextureView) {
        let format = proxy.format.to_wgpu();
        let fits = |(t, _): &(Texture, TextureView)| {
            (t.width(), t.height(), t.format()) == (proxy.width, proxy.height, format)
        };
        if let Some(i) = self.spare_images.iter().position(fits) {
            return self.spare_images.swap_remove(i);
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: proxy.width,
                height: proxy.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            format,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            usage: None,
            dimension: Some(TextureViewDimension::D2),
            aspect: TextureAspect::All,
            mip_level_count: None,
            base_mip_level: 0,
            base_array_layer: 0,
            array_layer_count: None,
            format: Some(format),
        });
        (texture, view)
    }

    /// Quantize a size up to the nearest size class.
    fn size_class(x: u64, bits: u32) -> u64 {
        if x > 1 << bits {
            let a = (x - 1).leading_zeros();
            let b = (x - 1) | (((u64::MAX / 2) >> bits) >> a);
            b + 1
        } else {
            1 << bits
        }
    }
}

impl BindMapBuffer {
    // Upload a buffer from CPU to GPU if needed.
    //
    // Note data flow is one way only, from CPU to GPU. Once this method is
    // called, the buffer is no longer materialized on CPU, and cannot be
    // accessed from a CPU shader.
    fn upload_if_needed(
        &mut self,
        proxy: &BufferProxy,
        device: &Device,
        queue: &Queue,
        pool: &mut ResourcePool,
    ) {
        if let MaterializedBuffer::Cpu(cpu_buf) = &self.buffer {
            let usage = BufferUsages::COPY_SRC
                | BufferUsages::COPY_DST
                | BufferUsages::STORAGE
                | BufferUsages::INDIRECT;
            let buf = pool.get_buf(proxy.size, proxy.name, usage, device);
            queue.write_buffer(&buf, 0, &cpu_buf.borrow());
            self.buffer = MaterializedBuffer::Gpu(buf);
        }
    }
}

impl<'a> TransientBindMap<'a> {
    fn indirect_workgroups(&self, bind_map: &BindMap, proxy: &BufferProxy, offset: u64) -> Result<u32> {
        match self.bufs.get(&proxy.id) {
            // Upload commands earlier in this recording have their newest bytes
            // here, even if an older resident copy is materialized on the GPU.
            Some(TransientBuf::Cpu(bytes)) => read_indirect_arguments(proxy, offset, bytes),
            Some(TransientBuf::Gpu(_)) => Err(Error::UnavailableBufferUsed(proxy.name, "indirect CPU dispatch")),
            None => bind_map.indirect_workgroups(proxy, offset),
        }
    }

    /// Create new transient bind map, seeded from external resources
    fn new(external_resources: &'a [ExternalResource<'_>]) -> Self {
        let mut bufs = HashMap::default();
        let mut images = HashMap::default();
        for resource in external_resources {
            match resource {
                ExternalResource::Buffer(proxy, gpu_buf) => {
                    bufs.insert(proxy.id, TransientBuf::Gpu(gpu_buf));
                }
                ExternalResource::Image(proxy, gpu_image) => {
                    images.insert(proxy.id, *gpu_image);
                }
            }
        }
        TransientBindMap { bufs, images }
    }

    fn materialize_gpu_buf_for_indirect(
        &mut self,
        bind_map: &mut BindMap,
        pool: &mut ResourcePool,
        device: &Device,
        queue: &Queue,
        buf: &BufferProxy,
    ) {
        if !self.bufs.contains_key(&buf.id)
            && let Some(b) = bind_map.buf_map.get_mut(&buf.id)
        {
            b.upload_if_needed(buf, device, queue, pool);
        }
    }

    #[cfg(feature = "debug_layers")]
    fn materialize_external_image_for_render_pass(&mut self, proxy: &ImageProxy) -> Result<&TextureView> {
        // TODO: Maybe this should support instantiating a transient texture. Right now all render
        // passes target a `SurfaceTexture`, so supporting external textures is sufficient.
        self.images
            .get(&proxy.id)
            .copied()
            .ok_or(Error::UnavailableRenderTarget(proxy.id.0.get()))
    }

    fn create_bind_group(
        &mut self,
        bind_map: &mut BindMap,
        pool: &mut ResourcePool,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        pass: &mut Option<ComputePass<'static>>,
        cache: &mut BindGroupCache,
        layout: &BindGroupLayout,
        bindings: &[ResourceProxy],
    ) -> BindGroup {
        for proxy in bindings {
            match proxy {
                ResourceProxy::Buffer(proxy)
                | ResourceProxy::BufferRange {
                    proxy,
                    offset: _,
                    size: _,
                } => {
                    if self.bufs.contains_key(&proxy.id) {
                        continue;
                    }
                    match bind_map.buf_map.entry(proxy.id) {
                        Entry::Vacant(v) => {
                            // TODO: only some buffers will need indirect & vertex, but does it hurt?
                            let usage = BufferUsages::COPY_SRC
                                | BufferUsages::COPY_DST
                                | BufferUsages::STORAGE
                                | BufferUsages::INDIRECT
                                | BufferUsages::VERTEX;
                            let buf = pool.get_buf(proxy.size, proxy.name, usage, device);
                            if bind_map.pending_clears.remove(&proxy.id) {
                                // The encoder is locked while a pass is open.
                                *pass = None;
                                encoder.clear_buffer(&buf, 0, None);
                            }
                            v.insert(BindMapBuffer {
                                buffer: MaterializedBuffer::Gpu(buf),
                                label: proxy.name,
                            });
                        }
                        Entry::Occupied(mut o) => {
                            o.get_mut().upload_if_needed(proxy, device, queue, pool);
                        }
                    }
                }
                ResourceProxy::Image(proxy) => {
                    if self.images.contains_key(&proxy.id) {
                        continue;
                    }
                    if let Entry::Vacant(v) = bind_map.image_map.entry(proxy.id) {
                        v.insert(pool.get_image(proxy, device));
                    }
                }
            }
        }
        let bound = bindings
            .iter()
            .map(|proxy| match proxy {
                ResourceProxy::Buffer(proxy) => {
                    let buf = match self.bufs.get(&proxy.id) {
                        Some(TransientBuf::Gpu(b)) => b,
                        _ => bind_map.get_gpu_buf(proxy.id).unwrap(),
                    };
                    Bound::Buffer(buf.clone(), 0, None)
                }
                ResourceProxy::BufferRange {
                    proxy,
                    offset,
                    size,
                } => {
                    let buf = match self.bufs.get(&proxy.id) {
                        Some(TransientBuf::Gpu(b)) => b,
                        _ => bind_map.get_gpu_buf(proxy.id).unwrap(),
                    };
                    Bound::Buffer(buf.clone(), *offset, core::num::NonZeroU64::new(*size))
                }
                ResourceProxy::Image(proxy) => {
                    let view = self
                        .images
                        .get(&proxy.id)
                        .copied()
                        .or_else(|| bind_map.image_map.get(&proxy.id).map(|v| &v.1))
                        .unwrap();
                    Bound::View(view.clone())
                }
            })
            .collect::<Vec<_>>();
        let now = cache.now;
        let (group, used) =
            cache
                .map
                .entry((layout.clone(), bound))
                .or_insert_with_key(|(_, bound)| {
                    let entries = bound
                        .iter()
                        .enumerate()
                        .map(|(i, b)| wgpu::BindGroupEntry {
                            binding: i as u32,
                            resource: match b {
                                Bound::Buffer(buffer, offset, size) => {
                                    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                        buffer,
                                        offset: *offset,
                                        size: *size,
                                    })
                                }
                                Bound::View(view) => wgpu::BindingResource::TextureView(view),
                            },
                        })
                        .collect::<Vec<_>>();
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout,
                        entries: &entries,
                    });
                    (group, now)
                });
        *used = now;
        group.clone()
    }

    fn create_cpu_resources(
        &self,
        bind_map: &'a mut BindMap,
        bindings: &[ResourceProxy],
    ) -> Result<Vec<CpuBinding<'_>>> {
        // First pass is mutable; create buffers as needed. Unsupported resource
        // kinds and GPU-only resources are explicit refusals, never empty output.
        for resource in bindings {
            match resource {
                ResourceProxy::Buffer(proxy) => match self.bufs.get(&proxy.id) {
                    Some(TransientBuf::Cpu(_)) => (),
                    Some(TransientBuf::Gpu(_)) => return Err(Error::UnavailableBufferUsed(proxy.name, "CPU dispatch")),
                    None => bind_map.materialize_cpu_buf(proxy),
                },
                ResourceProxy::BufferRange { .. } => return Err(Error::UnsupportedCpuBinding("buffer range")),
                ResourceProxy::Image(_) => return Err(Error::UnsupportedCpuBinding("image")),
            }
        }
        // Second pass takes immutable references after materialization.
        bindings.iter().map(|resource| match resource {
            ResourceProxy::Buffer(buf) => match self.bufs.get(&buf.id) {
                Some(TransientBuf::Cpu(b)) => Ok(CpuBinding::Buffer(b)),
                _ => bind_map.get_cpu_buf(buf),
            },
            ResourceProxy::BufferRange { .. } => Err(Error::UnsupportedCpuBinding("buffer range")),
            ResourceProxy::Image(_) => Err(Error::UnsupportedCpuBinding("image")),
        }).collect()
    }
}

#[cfg(test)]
mod doctor_tests {
    use super::*;
    use crate::recording::ImageFormat;
    use peniko::{Blob, ImageAlphaType, ImageData};
    use std::sync::Arc;

    fn cpu_shader(_: u32, _: &[CpuBinding<'_>]) {}

    #[test]
    fn recording_refuses_missing_shader_and_implementation_without_gpu() {
        let mut engine = WgpuEngine::default();
        let mut recording = Recording::default();
        recording.dispatch(ShaderId(0), (1, 1, 1), std::iter::empty::<ResourceProxy>());
        assert!(matches!(engine.validate_recording(&recording), Err(Error::UnavailableShader(0))));
        engine.shaders.push(Shader { label: "missing", wgpu: None, cpu: None });
        assert!(matches!(engine.validate_recording(&recording), Err(Error::MissingShaderImplementation("missing"))));
        engine.shaders[0].cpu = Some(CpuShader { shader: cpu_shader });
        assert!(engine.validate_recording(&recording).is_ok());
    }

    #[test]
    fn recording_refuses_unowned_empty_image_and_accepts_cpu_pixels() {
        let engine = WgpuEngine::default();
        let proxy = ImageProxy::new(1, 1, ImageFormat::Rgba8);
        let image = |data: Vec<u8>| ImageData {
            data: Blob::new(Arc::new(data)), format: peniko::ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha, width: 1, height: 1,
        };
        let mut recording = Recording::default();
        let empty = image(Vec::new());
        let id = empty.data.id();
        recording.push(Command::WriteImage(proxy, [0, 0], empty));
        assert!(matches!(engine.validate_recording(&recording), Err(Error::UnavailableImage(got)) if got == id));
        assert!(engine.bind_map.image_map.is_empty());
        assert!(engine.pool.images.is_empty());
        recording.commands.clear();
        recording.push(Command::WriteImage(proxy, [0, 0], image(vec![255; 4])));
        assert!(engine.validate_recording(&recording).is_ok());
    }

    #[test]
    fn indirect_cpu_arguments_respect_offset_and_refuse_missing_or_invalid_storage() {
        let mut map = BindMap::default();
        let proxy = BufferProxy::new(24, "arguments");
        assert!(matches!(map.indirect_workgroups(&proxy, 0), Err(Error::UnavailableBufferUsed("arguments", _))));
        let bytes: Vec<u8> = [7u32, 1, 1, 11, 1, 1].into_iter().flat_map(u32::to_le_bytes).collect();
        map.buf_map.insert(proxy.id, BindMapBuffer {
            buffer: MaterializedBuffer::Cpu(RefCell::new(bytes)), label: "arguments",
        });
        assert_eq!(map.indirect_workgroups(&proxy, 0).unwrap(), 7);
        assert_eq!(map.indirect_workgroups(&proxy, 12).unwrap(), 11);
        for offset in [1, 16, 24, u64::MAX] {
            assert!(matches!(map.indirect_workgroups(&proxy, offset), Err(Error::InvalidIndirectDispatch { .. })));
        }
        map.buf_map.insert(proxy.id, BindMapBuffer {
            buffer: MaterializedBuffer::Cpu(RefCell::new(vec![0; 4])), label: "arguments",
        });
        assert!(matches!(map.indirect_workgroups(&proxy, 0), Err(Error::InvalidIndirectDispatch { .. })));
    }
}

#[cfg(test)]
mod cpu_resource_refusal_tests {
    use super::*;
    use crate::recording::ImageFormat;

    #[test]
    fn cpu_binding_kinds_are_refused_or_materialized_explicitly() {
        let transient = TransientBindMap::default();
        let image = ResourceProxy::new_image(1, 1, ImageFormat::Rgba8);
        let buffer = BufferProxy::new(16, "cpu");
        let range = ResourceProxy::BufferRange { proxy: buffer, offset: 0, size: 4 };
        assert!(matches!(transient.create_cpu_resources(&mut BindMap::default(), &[image]), Err(Error::UnsupportedCpuBinding("image"))));
        assert!(matches!(transient.create_cpu_resources(&mut BindMap::default(), &[range]), Err(Error::UnsupportedCpuBinding("buffer range"))));
        let mut map = BindMap::default();
        assert!(matches!(map.get_cpu_buf(&buffer), Err(Error::UnavailableBufferUsed("cpu", _))));
        let bindings = [ResourceProxy::Buffer(buffer)];
        let resources = transient.create_cpu_resources(&mut map, &bindings).unwrap();
        assert!(matches!(resources.as_slice(), [CpuBinding::BufferRW(_)]));
    }

    #[cfg(feature = "debug_layers")]
    #[test]
    fn missing_debug_target_is_a_typed_refusal() {
        let mut transient = TransientBindMap::default();
        let image = ImageProxy::new(1, 1, ImageFormat::Rgba8);
        assert!(matches!(transient.materialize_external_image_for_render_pass(&image), Err(Error::UnavailableRenderTarget(_))));
    }
}

#[cfg(test)]
mod recording_owner_tests {
    use super::*;
    use crate::recording::ImageFormat;

    fn shader(_: u32, _: &[CpuBinding<'_>]) {}

    #[test]
    fn static_cpu_binding_refusal_preserves_existing_resources_and_allows_retry() {
        let mut engine = WgpuEngine::default();
        engine.shaders.push(Shader { label: "cpu", cpu: Some(CpuShader { shader }), wgpu: None });
        let buffer = BufferProxy::new(16, "resident");
        engine.bind_map.buf_map.insert(buffer.id, BindMapBuffer {
            buffer: MaterializedBuffer::Cpu(RefCell::new(vec![7; 16])), label: "resident",
        });
        let mut bad = Recording::default();
        bad.dispatch(ShaderId(0), (1, 1, 1), [ResourceProxy::new_image(1, 1, ImageFormat::Rgba8)]);
        assert!(matches!(engine.validate_recording(&bad), Err(Error::UnsupportedCpuBinding("image"))));
        assert_eq!(engine.bind_map.buf_map.len(), 1);
        assert!(!engine.invalidated);
        let mut good = Recording::default();
        good.dispatch(ShaderId(0), (1, 1, 1), [ResourceProxy::Buffer(buffer)]);
        assert!(engine.validate_recording(&good).is_ok());
    }

    #[test]
    fn dynamic_failure_releases_resources_and_refuses_further_recordings_until_recreated() {
        let mut engine = WgpuEngine::default();
        for _ in 0..32 {
            let buffer = BufferProxy::new(16, "owned");
            engine.bind_map.buf_map.insert(buffer.id, BindMapBuffer {
                buffer: MaterializedBuffer::Cpu(RefCell::new(vec![0; 16])), label: "owned",
            });
        }
        assert!(engine.finish_recording(Err(Error::InvalidIndirectDispatch { buffer: "owned", offset: 1 })).is_err());
        assert!(engine.bind_map.buf_map.is_empty());
        assert!(engine.bind_map.image_map.is_empty());
        assert!(engine.bind_groups.map.is_empty());
        assert!(engine.pool.bufs.is_empty());
        assert!(engine.pool.images.is_empty());
        assert!(engine.pool.spare_images.is_empty());
        assert!(engine.downloads.is_empty());
        for _ in 0..128 {
            assert!(matches!(engine.validate_recording(&Recording::default()), Err(Error::RendererInvalidated)));
            assert!(engine.bind_map.buf_map.is_empty());
        }
        assert!(WgpuEngine::default().validate_recording(&Recording::default()).is_ok());
    }

    #[test]
    fn indirect_cpu_dispatch_uses_latest_uploaded_bytes_before_resident_data() {
        let proxy = BufferProxy::new(24, "arguments");
        let mut resident = BindMap::default();
        resident.buf_map.insert(proxy.id, BindMapBuffer {
            buffer: MaterializedBuffer::Cpu(RefCell::new(vec![0; 24])), label: "arguments",
        });
        let bytes: Vec<u8> = [7u32, 1, 1, 11, 1, 1].into_iter().flat_map(u32::to_le_bytes).collect();
        let mut transient = TransientBindMap::default();
        transient.bufs.insert(proxy.id, TransientBuf::Cpu(&bytes));
        assert_eq!(transient.indirect_workgroups(&resident, &proxy, 0).unwrap(), 7);
        assert_eq!(transient.indirect_workgroups(&resident, &proxy, 12).unwrap(), 11);
        assert!(matches!(transient.indirect_workgroups(&resident, &proxy, 1), Err(Error::InvalidIndirectDispatch { .. })));
        assert_eq!(resident.indirect_workgroups(&proxy, 0).unwrap(), 0);
    }
}

#[cfg(test)]
mod coarse_owner_retry_tests {
    use super::*;
    use crate::{AaConfig, Error, RenderParams, Scene, low_level::FullShaders, render::Render};
    use peniko::{Blob, ImageAlphaType, ImageData, kurbo::Affine};
    use std::{collections::BTreeSet, sync::Arc};
    use vello_encoding::Resolver;

    // Only real static preflight is called. No shader executes, no device is
    // opened, and an Ok callback is not evidence of submitted/pixel output.
    fn cpu_shader(_: u32, _: &[CpuBinding<'_>]) {}

    fn engine() -> WgpuEngine {
        let mut engine = WgpuEngine::default();
        engine.shaders.push(Shader {
            label: "coarse-preflight", cpu: Some(CpuShader { shader: cpu_shader }), wgpu: None,
        });
        engine
    }

    fn shaders() -> FullShaders {
        FullShaders {
            pathtag_reduce: ShaderId(0), pathtag_reduce2: ShaderId(0),
            pathtag_scan1: ShaderId(0), pathtag_scan: ShaderId(0), pathtag_scan_large: ShaderId(0),
            bbox_clear: ShaderId(0), flatten: ShaderId(0), draw_reduce: ShaderId(0),
            draw_leaf: ShaderId(0), clip_reduce: ShaderId(0), clip_leaf: ShaderId(0),
            binning: ShaderId(0), tile_alloc: ShaderId(0), backdrop: ShaderId(0),
            path_count_setup: ShaderId(0), path_count: ShaderId(0), coarse: ShaderId(0),
            path_tiling_setup: ShaderId(0), path_tiling: ShaderId(0),
            fine_area: Some(ShaderId(0)), fine_msaa8: None, fine_msaa16: None,
            pathtag_is_cpu: true,
        }
    }

    fn params() -> RenderParams {
        RenderParams {
            base_color: peniko::Color::BLACK, width: 8, height: 8,
            antialiasing_method: AaConfig::Area,
        }
    }

    fn image(width: u32, height: u32, foreign: bool) -> ImageData {
        ImageData {
            data: Blob::new(Arc::new(if foreign { Vec::new() } else { vec![255; (width * height * 4) as usize] })),
            format: peniko::ImageFormat::Rgba8, alpha_type: ImageAlphaType::Alpha,
            width, height,
        }
    }

    fn scene(images: &[&ImageData]) -> Scene {
        let mut scene = Scene::new();
        for image in images { scene.draw_image(*image, Affine::IDENTITY); }
        scene
    }

    struct Attempt {
        result: Result<()>,
        uploads: BTreeSet<u64>,
        write_targets: BTreeSet<u64>,
        frees: Vec<u64>,
        prepared_atlas: ImageProxy,
        committed_atlas: Option<ImageProxy>,
    }

    fn attempt(
        resolver: &mut Resolver,
        atlas: &mut Option<ImageProxy>,
        engine: &WgpuEngine,
        scene: &Scene,
    ) -> Attempt {
        let previous_atlas = *atlas;
        let mut render = Render::new();
        #[cfg(feature = "debug_layers")]
        { render.capture_debug = false; }
        let recording = render.render_encoding_coarse(
            scene.encoding(), resolver, &shaders(), atlas, &params(), true,
        ).unwrap();
        let prepared_atlas = atlas.expect("image scene prepares an atlas proxy");
        let uploads = recording.commands.iter().filter_map(|command| match command {
            Command::WriteImage(_, _, image) => Some(image.data.id()), _ => None,
        }).collect();
        let write_targets = recording.commands.iter().filter_map(|command| match command {
            Command::WriteImage(proxy, _, _) => Some(proxy.id.0.get()), _ => None,
        }).collect();
        let frees = recording.commands.iter().filter_map(|command| match command {
            Command::FreeImage(proxy) => Some(proxy.id.0.get()), _ => None,
        }).collect();
        // This is the same owner completion helper used by public Renderer.
        // The callback is its actual engine static preflight, without execution.
        let result = crate::render::submit_coarse_recording(
            resolver, atlas, previous_atlas, &recording,
            || engine.validate_recording(&recording),
        );
        Attempt { result, uploads, write_targets, frees, prepared_atlas, committed_atlas: *atlas }
    }

    fn atlas_id(atlas: Option<ImageProxy>) -> Option<u64> {
        atlas.map(|proxy| proxy.id.0.get())
    }

    fn unavailable(attempt: &Attempt, image: &ImageData) {
        assert!(matches!(&attempt.result, Err(Error::UnavailableImage(id)) if *id == image.data.id()),
            "expected repeated ownership refusal, got {:?}", attempt.result);
    }

    fn static_engine_untouched(engine: &WgpuEngine) {
        assert!(!engine.invalidated);
        assert!(engine.bind_map.buf_map.is_empty());
        assert!(engine.bind_map.image_map.is_empty());
        assert!(engine.pool.bufs.is_empty());
        assert!(engine.pool.images.is_empty());
        assert!(engine.pool.spare_images.is_empty());
        assert!(engine.bind_groups.map.is_empty());
        assert!(engine.downloads.is_empty());
    }

    #[test]
    fn foreign_image_refuses_both_attempts_through_one_persistent_resolver() {
        let mut resolver = Resolver::new();
        let mut atlas = None;
        let engine = engine();
        let foreign = image(4, 4, true);
        let scene = scene(&[&foreign]);
        // Generate BOTH attempts before assertions: the old owner behavior
        // yields no WriteImage and apparent success on the second attempt.
        let attempts: Vec<_> = (0..2).map(|_| attempt(&mut resolver, &mut atlas, &engine, &scene)).collect();
        for result in &attempts { unavailable(result, &foreign); }
        for result in &attempts {
            assert_eq!(result.uploads, BTreeSet::from([foreign.data.id()]));
            assert_eq!(result.write_targets, BTreeSet::from([result.prepared_atlas.id.0.get()]));
            assert!(result.frees.is_empty());
            assert!(result.committed_atlas.is_none());
        }
        assert_ne!(attempts[0].prepared_atlas.id.0.get(), attempts[1].prepared_atlas.id.0.get());
        assert!(atlas.is_none());
        static_engine_untouched(&engine);
    }

    #[test]
    fn mixed_refusal_redirties_valid_uploads_on_both_sides_of_foreign_image() {
        for foreign_first in [false, true] {
            let mut resolver = Resolver::new();
            let mut atlas = None;
            let engine = engine();
            let valid = image(2, 2, false);
            let foreign = image(4, 4, true);
            let mixed = if foreign_first { scene(&[&foreign, &valid]) } else { scene(&[&valid, &foreign]) };
            let rejected = attempt(&mut resolver, &mut atlas, &engine, &mixed);
            let valid_only = scene(&[&valid]);
            let retry = attempt(&mut resolver, &mut atlas, &engine, &valid_only);
            unavailable(&rejected, &foreign);
            assert_eq!(rejected.uploads, BTreeSet::from([valid.data.id(), foreign.data.id()]));
            assert!(retry.result.is_ok());
            assert_eq!(retry.uploads, BTreeSet::from([valid.data.id()]));
            assert!(rejected.committed_atlas.is_none());
            assert_ne!(retry.prepared_atlas.id.0.get(), rejected.prepared_atlas.id.0.get());
            assert_eq!(retry.write_targets, BTreeSet::from([retry.prepared_atlas.id.0.get()]));
            assert!(retry.frees.is_empty());
            let accepted_id = atlas_id(atlas);
            let clean = attempt(&mut resolver, &mut atlas, &engine, &valid_only);
            assert!(clean.result.is_ok());
            assert!(clean.uploads.is_empty());
            assert_eq!(atlas_id(atlas), accepted_id);
            static_engine_untouched(&engine);
        }
    }

    #[test]
    fn refused_resize_restores_old_atlas_and_retries_its_free_and_all_surviving_uploads() {
        let mut resolver = Resolver::new();
        let mut atlas = None;
        let engine = engine();
        let resident = image(2, 2, false);
        assert!(attempt(&mut resolver, &mut atlas, &engine, &scene(&[&resident])).result.is_ok());
        let old = atlas.unwrap();
        let valid = image(3, 3, false);
        // The cached allocator must grow. The foreign Blob stays empty, so
        // constructing this fixture needs no large image-pixel allocation.
        let foreign = image(old.width + 1, 1, true);
        let mixed = scene(&[&resident, &valid, &foreign]);
        let attempts: Vec<_> = (0..2).map(|_| attempt(&mut resolver, &mut atlas, &engine, &mixed)).collect();
        for rejected in &attempts { unavailable(rejected, &foreign); }
        for rejected in &attempts {
            assert!(rejected.prepared_atlas.width > old.width);
            assert_eq!(atlas_id(rejected.committed_atlas), Some(old.id.0.get()));
            assert_eq!(rejected.frees, [old.id.0.get()]);
            assert_eq!(rejected.uploads, BTreeSet::from([resident.data.id(), valid.data.id(), foreign.data.id()]));
        }
        assert_eq!(atlas_id(atlas), Some(old.id.0.get()));
        let retry = attempt(&mut resolver, &mut atlas, &engine, &scene(&[&resident, &valid]));
        assert!(retry.result.is_ok());
        assert!(retry.prepared_atlas.width > old.width);
        assert_eq!(retry.frees, [old.id.0.get()]);
        assert_eq!(retry.uploads, BTreeSet::from([resident.data.id(), valid.data.id()]));
        assert_eq!(retry.write_targets, BTreeSet::from([retry.prepared_atlas.id.0.get()]));
        for rejected in &attempts { assert_ne!(retry.prepared_atlas.id.0.get(), rejected.prepared_atlas.id.0.get()); }
        assert_eq!(atlas_id(atlas), Some(retry.prepared_atlas.id.0.get()));
        static_engine_untouched(&engine);
    }

    #[test]
    fn refused_reused_atlas_preserves_resident_atlas_and_requeues_new_uploads() {
        let mut resolver = Resolver::new();
        let mut atlas = None;
        let engine = engine();
        let resident = image(2, 2, false);
        assert!(attempt(&mut resolver, &mut atlas, &engine, &scene(&[&resident])).result.is_ok());
        let old = atlas.unwrap();
        let valid = image(3, 3, false);
        let foreign = image(4, 4, true);
        let rejected = attempt(&mut resolver, &mut atlas, &engine, &scene(&[&resident, &valid, &foreign]));
        unavailable(&rejected, &foreign);
        assert_eq!(rejected.prepared_atlas.id.0.get(), old.id.0.get());
        assert!(rejected.frees.is_empty());
        assert_eq!(rejected.uploads, BTreeSet::from([valid.data.id(), foreign.data.id()]));
        let retry = attempt(&mut resolver, &mut atlas, &engine, &scene(&[&resident, &valid]));
        assert!(retry.result.is_ok());
        assert_eq!(retry.prepared_atlas.id.0.get(), old.id.0.get());
        assert!(retry.frees.is_empty());
        assert_eq!(retry.uploads, BTreeSet::from([valid.data.id()]));
        static_engine_untouched(&engine);
    }

    #[test]
    fn allocation_refusal_after_resolve_restores_unsubmitted_uploads_before_retry() {
        let mut resolver = Resolver::new();
        let mut atlas = None;
        let valid = image(2, 2, false);
        let scene = scene(&[&valid]);
        let mut render = Render::new();
        render.buffer_limit = 0;
        let result = render.render_encoding_coarse(
            scene.encoding(), &mut resolver, &shaders(), &mut atlas, &params(), true,
        );
        assert!(matches!(result, Err(Error::AllocationLimit(_, _, _))));
        let restored = atlas.is_none();
        let engine = engine();
        let retry = attempt(&mut resolver, &mut atlas, &engine, &scene);
        assert!(retry.result.is_ok());
        assert_eq!(retry.uploads, BTreeSet::from([valid.data.id()]));
        assert!(restored);
        assert!(retry.frees.is_empty());
        static_engine_untouched(&engine);
    }
}
