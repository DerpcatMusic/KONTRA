use super::Error;
use mui_weld::analytic::{AnalyticWeld, PARAM_BYTES, dirty_ranges};
use mui_weld::boundary::BOUNDARY_BYTES;
use std::{collections::BTreeMap, num::NonZeroU64, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub max_surfaces: usize,
    /// Logical texel storage, not a claim about driver allocation/VRAM overhead.
    pub max_texture_bytes: u64,
    pub max_surface_pixels: u64,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            max_surfaces: 128,
            max_texture_bytes: 64 * 1024 * 1024,
            max_surface_pixels: 4 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct EffectStats {
    pub boundary_upload_bytes: u64,
    pub uniform_upload_bytes: u64,
    pub uniform_writes: u64,
    pub texture_allocations: u64,
    pub effect_draws: u64,
    pub effect_pixels: u64,
    pub encoded_scenes: u64,
    /// Vello render passes recorded: 0 on a frame that left the target as
    /// the previous one did.
    pub renders: u64,
    /// The pixels those renders drew of the frame: all of it, or the box
    /// around what changed.
    pub rendered_pixels: u64,
    pub resident_texture_bytes: u64,
    pub peak_texture_bytes: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ContentState {
    pub uploaded: Option<[u8; PARAM_BYTES]>,
    pub rendered: Option<[u8; PARAM_BYTES]>,
    pub pending: Option<[u8; PARAM_BYTES]>,
}
impl ContentState {
    pub fn needs_render(&self, desired: &[u8; PARAM_BYTES]) -> bool {
        self.rendered.as_ref() != Some(desired)
    }
    pub fn submitted(&mut self) {
        if let Some(p) = self.pending.take() {
            self.rendered = Some(p);
        }
    }
    pub fn aborted(&mut self) {
        self.pending = None;
    }
}
struct Slot {
    view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    boundary: wgpu::Buffer,
    boundary_uploaded: Option<Arc<[u8; BOUNDARY_BYTES]>>,
    boundary_rendered: Option<Arc<[u8; BOUNDARY_BYTES]>>,
    boundary_pending: Option<Arc<[u8; BOUNDARY_BYTES]>>,
    bind: wgpu::BindGroup,
    capacity: [u32; 2],
    used: [u32; 2],
    texture: wgpu::Texture,
    /// Minted per allocation: a renderer that registered the old texture
    /// sees the id change and registers the new one.
    id: u64,
    state: ContentState,
    seen: u64,
    /// The last frame whose scene carried this weld at all, visible or not.
    present: u64,
    encoded_epoch: u64,
}
impl Slot {
    fn bytes(&self) -> u64 {
        u64::from(self.capacity[0]) * u64::from(self.capacity[1]) * 4
    }
}

/// The pool does not own a frame loop or submit behind the caller's back.
/// Call `commit_submitted()` ONLY after the encoder containing these draws has
/// been submitted. `abort()` leaves texture content dirty for a later attempt.
/// The high-level `GpuRenderer` performs this transaction itself.
pub struct WeldTextures {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    slots: BTreeMap<Arc<str>, Slot>,
    budget: Budget,
    epoch: u64,
    next_id: u64,
    mapping_revision: u64,
    peak: u64,
    in_frame: bool,
}
impl WeldTextures {
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        budget: Budget,
    ) -> Result<Self, Error> {
        if budget.max_surfaces == 0
            || budget.max_texture_bytes == 0
            || budget.max_surface_pixels == 0
        {
            return Err(Error::Budget("zero limit"));
        }
        // Scope catches WGSL/type/layout errors instead of calling this a working
        // pipeline because the source string exists. Native smoke tests await it.
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("MUI weld uniforms"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(PARAM_BYTES as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(BOUNDARY_BYTES as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("MUI weld layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("MUI analytic weld"),
            source: wgpu::ShaderSource::Wgsl(super::WELD_SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("MUI analytic weld"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_hybrid"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(e) = error_scope.pop().await {
            return Err(Error::Device(e.to_string()));
        }
        Ok(Self {
            device: device.clone(),
            queue: queue.clone(),
            pipeline,
            layout,
            slots: BTreeMap::new(),
            budget,
            epoch: 0,
            next_id: 1,
            mapping_revision: 0,
            peak: 0,
            in_frame: false,
        })
    }
    pub fn mapping_revision(&self) -> u64 {
        self.mapping_revision
    }
    /// The weld's texture: its allocation id, the texture, and the corner
    /// of it the material fills. Premultiplied, sRGB-encoded RGBA.
    pub fn texture(&self, key: &str) -> Option<(u64, &wgpu::Texture, [u32; 2])> {
        self.slots.get(key).map(|s| (s.id, &s.texture, s.used))
    }
    fn resident_bytes(&self) -> u64 {
        self.slots.values().map(Slot::bytes).sum()
    }
    /// Preflight ALL of this frame's `wanted` (visible) storage before
    /// allocating, and start a transaction. Textures not wanted this frame stay
    /// resident while the budget allows -- scrolling back must not re-render --
    /// and the least recently wanted go first when it does not. Dropping wgpu
    /// handles permits deferred release; explicit texture.destroy() is
    /// intentionally not used on resources an earlier submission may still
    /// reference.
    pub fn begin<'a>(
        &mut self,
        wanted: impl Clone + Iterator<Item = (&'a str, &'a AnalyticWeld)>,
    ) -> Result<EffectStats, Error> {
        if self.in_frame {
            return Err(Error::Unsupported(
                "finish or abort the previous effect frame first",
            ));
        }
        let limit = self.device.limits().max_texture_dimension_2d.min(65535);
        let mut count = 0usize;
        let mut bytes = 0u64;
        // A key wanted twice is caught by `encode`, which refuses a second
        // encode per submission; here it only counts twice against the budget.
        for (key, m) in wanted.clone() {
            let size = m.pixels();
            if key.is_empty() || size.contains(&0) || size.iter().any(|v| *v > limit) {
                return Err(Error::Budget("effect key or texture dimensions"));
            }
            let capacity = self.slots.get(key).map_or_else(
                || bucket(size, limit),
                |s| {
                    if fits(size, s.capacity) {
                        s.capacity
                    } else {
                        bucket(size, limit)
                    }
                },
            );
            let pixels = u64::from(capacity[0]) * u64::from(capacity[1]);
            if pixels > self.budget.max_surface_pixels {
                return Err(Error::Budget("per-surface pixel limit"));
            }
            bytes = bytes
                .checked_add(pixels * 4)
                .ok_or(Error::Budget("byte overflow"))?;
            count = count
                .checked_add(1)
                .ok_or(Error::Budget("surface overflow"))?;
        }
        if count > self.budget.max_surfaces || bytes > self.budget.max_texture_bytes {
            return Err(Error::Budget(
                "visible surfaces exceed budget; no resolution was silently reduced",
            ));
        }
        if self.epoch == u64::MAX {
            for s in self.slots.values_mut() {
                s.seen = 0;
                s.present = 0;
                s.encoded_epoch = 0;
            }
            self.epoch = 0;
        }
        self.epoch += 1;
        for (key, _) in wanted {
            if let Some(s) = self.slots.get_mut(key) {
                s.seen = self.epoch;
            }
        }
        let epoch = self.epoch;
        let (mut count, mut bytes) = self
            .slots
            .values()
            .filter(|s| s.seen != epoch)
            .fold((count, bytes), |(n, b), s| {
                (n + 1, b.saturating_add(s.bytes()))
            });
        if count > self.budget.max_surfaces || bytes > self.budget.max_texture_bytes {
            // ponytail: collects and sorts the idle slots, but only on a frame
            // that is over budget.
            let mut idle: Vec<_> = self
                .slots
                .iter()
                .filter(|(_, s)| s.seen != epoch)
                .map(|(k, s)| (s.seen, k.clone()))
                .collect();
            idle.sort_unstable();
            for (_, key) in idle {
                if count <= self.budget.max_surfaces && bytes <= self.budget.max_texture_bytes {
                    break;
                }
                if let Some(s) = self.slots.remove(&key) {
                    self.mapping_revision = self.mapping_revision.wrapping_add(1);
                    count -= 1;
                    bytes -= s.bytes();
                }
            }
        }
        self.in_frame = true;
        Ok(EffectStats::default())
    }
    /// Free the texture of every weld the scene has not carried for
    /// [`ABSENT_FRAMES`] frames. One scrolled offscreen is still in the
    /// scene and keeps its texture; one removed goes without waiting for
    /// budget pressure. Call after [`WeldTextures::begin`].
    pub fn forget_absent(&mut self, in_scene: impl Fn(&str) -> bool) {
        let epoch = self.epoch;
        let before = self.slots.len();
        self.slots.retain(|key, s| {
            if in_scene(key) {
                s.present = epoch;
            }
            epoch - s.present <= ABSENT_FRAMES
        });
        if self.slots.len() != before {
            self.mapping_revision = self.mapping_revision.wrapping_add(1);
        }
    }
    pub fn encode(
        &mut self,
        key: &str,
        material: &AnalyticWeld,
        encoder: &mut wgpu::CommandEncoder,
        stats: &mut EffectStats,
    ) -> Result<(), Error> {
        if !self.in_frame {
            return Err(Error::Unsupported("begin the effect frame before encoding"));
        }
        let size = material.pixels();
        let limit = self.device.limits().max_texture_dimension_2d.min(65535);
        let must_allocate = self.slots.get(key).is_none_or(|s| !fits(size, s.capacity));
        if must_allocate {
            let capacity = bucket(size, limit);
            let bytes = u64::from(capacity[0]) * u64::from(capacity[1]) * 4;
            let old = self.slots.get(key).map_or(0, Slot::bytes);
            if size.contains(&0)
                || size.iter().any(|s| *s > limit)
                || bytes / 4 > self.budget.max_surface_pixels
                || self
                    .resident_bytes()
                    .saturating_sub(old)
                    .saturating_add(bytes)
                    > self.budget.max_texture_bytes
                || (!self.slots.contains_key(key) && self.slots.len() >= self.budget.max_surfaces)
            {
                return Err(Error::Budget("allocation exceeds preflight/budget"));
            }
            let id = self.next_id;
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or(Error::Budget("texture ID exhausted"))?;
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("MUI retained weld"),
                size: wgpu::Extent3d {
                    width: capacity[0],
                    height: capacity[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("MUI weld parameters"),
                size: PARAM_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let boundary = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("MUI crisp boundary"),
                size: BOUNDARY_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("MUI weld binding"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: boundary.as_entire_binding(),
                    },
                ],
            });
            self.slots.insert(
                Arc::from(key),
                Slot {
                    view,
                    uniform,
                    boundary,
                    boundary_uploaded: None,
                    boundary_rendered: None,
                    boundary_pending: None,
                    bind,
                    capacity,
                    used: size,
                    texture,
                    id,
                    state: ContentState::default(),
                    seen: self.epoch,
                    present: self.epoch,
                    encoded_epoch: 0,
                },
            );
            self.mapping_revision = self.mapping_revision.wrapping_add(1);
            stats.texture_allocations += 1;
        }
        let slot = self
            .slots
            .get_mut(key)
            .ok_or_else(|| Error::Missing(key.into()))?;
        if slot.encoded_epoch == self.epoch {
            return Err(Error::Unsupported(
                "encode each effect only once per submission; repeated UBO writes would change earlier draws",
            ));
        }
        slot.encoded_epoch = self.epoch;
        if slot.seen != self.epoch {
            slot.seen = self.epoch;
        }
        if slot.used != size {
            slot.used = size;
            slot.state.rendered = None;
            self.mapping_revision = self.mapping_revision.wrapping_add(1);
        }
        let bytes = material.uniform_bytes();
        for range in dirty_ranges(slot.state.uploaded.as_ref(), &bytes) {
            self.queue
                .write_buffer(&slot.uniform, range.start as u64, &bytes[range.clone()]);
            stats.uniform_upload_bytes += range.len() as u64;
            stats.uniform_writes += 1;
        }
        slot.state.uploaded = Some(bytes);
        let geometry = material.boundary_bytes();
        let equal = |a: &Option<Arc<[u8; BOUNDARY_BYTES]>>| {
            a.as_ref()
                .is_some_and(|a| Arc::ptr_eq(a, geometry) || a.as_ref() == geometry.as_ref())
        };
        if !equal(&slot.boundary_uploaded) {
            self.queue
                .write_buffer(&slot.boundary, 0, geometry.as_ref());
            stats.boundary_upload_bytes += BOUNDARY_BYTES as u64;
            slot.boundary_uploaded = Some(geometry.clone());
        }
        if slot.state.needs_render(&bytes) || !equal(&slot.boundary_rendered) {
            let attachment = Some(wgpu::RenderPassColorAttachment {
                view: &slot.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("MUI weld material"),
                color_attachments: &[attachment],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &slot.bind, &[]);
            pass.set_viewport(0., 0., size[0] as f32, size[1] as f32, 0., 1.);
            pass.set_scissor_rect(0, 0, size[0], size[1]);
            pass.draw(0..6, 0..1);
            slot.state.pending = Some(bytes);
            slot.boundary_pending = Some(geometry.clone());
            stats.effect_draws += 1;
            stats.effect_pixels += u64::from(size[0]) * u64::from(size[1]);
        }
        Ok(())
    }
    pub fn commit_submitted(&mut self, stats: &mut EffectStats) {
        for slot in self.slots.values_mut() {
            slot.state.submitted();
            if let Some(b) = slot.boundary_pending.take() {
                slot.boundary_rendered = Some(b);
            }
        }
        self.in_frame = false;
        self.peak = self.peak.max(self.resident_bytes());
        stats.resident_texture_bytes = self.resident_bytes();
        stats.peak_texture_bytes = self.peak;
    }
    pub fn abort(&mut self) {
        for slot in self.slots.values_mut() {
            slot.state.aborted();
            slot.boundary_pending = None;
        }
        self.in_frame = false;
    }
}
/// How many frames a weld may leave the scene and come back to its texture:
/// a hover that flickers one off for a frame should not re-render it.
pub const ABSENT_FRAMES: u64 = 3;
fn fits(want: [u32; 2], capacity: [u32; 2]) -> bool {
    want[0] <= capacity[0] && want[1] <= capacity[1]
}
pub(crate) fn bucket(size: [u32; 2], limit: u32) -> [u32; 2] {
    size.map(|v| v.saturating_add(63) / 64 * 64)
        .map(|v| v.min(limit))
}
