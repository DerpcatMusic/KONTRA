// Copyright 2022 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Take an encoded scene and create a graph to render it

use crate::recording::{BufferProxy, Command, ImageFormat, ImageProxy, Recording, ResourceProxy};
use crate::shaders::FullShaders;
use crate::{AaConfig, RenderParams};

use vello_encoding::{Encoding, Resolver, WorkgroupSize, make_mask_lut, make_mask_lut_16};

#[derive(Clone, Copy, Debug)]
enum AtlasProxyAction {
    Created,
    Reused,
    Resized,
}

// Choose an enabled shader and its matching mask together, exhaustively.
// Validate this before coarse allocation as well as at the low-level fine API.
fn fine_pipeline(aa: AaConfig, shaders: &FullShaders) -> crate::Result<(crate::ShaderId, Option<fn() -> Vec<u8>>)> {
    let (shader, mask): (_, Option<fn() -> Vec<u8>>) = match aa {
        AaConfig::Area => (shaders.fine_area, None),
        AaConfig::Msaa8 => (shaders.fine_msaa8, Some(make_mask_lut)),
        AaConfig::Msaa16 => (shaders.fine_msaa16, Some(make_mask_lut_16)),
    };
    Ok((shader.ok_or(crate::Error::UnsupportedAntialiasing(aa))?, mask))
}

// Resolve has already marked these images clean, but none of this recording's
// uploads/frees may be treated as submitted after a refusal. Dirty every upload,
// including valid images preceding/following the refused image, and retain the
// previously committed atlas (a resized atlas's FreeImage was not submitted).
fn restore_unsubmitted_coarse(
    resolver: &mut Resolver,
    persistent_image_atlas: &mut Option<ImageProxy>,
    previous_atlas: Option<ImageProxy>,
    recording: &Recording,
) {
    for command in &recording.commands {
        if let Command::WriteImage(_, _, image) = command {
            resolver.mark_image_dirty(image);
        }
    }
    *persistent_image_atlas = previous_atlas;
}

/// Restore unsubmitted image bookkeeping when coarse submission is refused.
///
/// Static engine refusals leave GPU resources untouched; their owner must also
/// restore unsubmitted resolver/atlas bookkeeping. Dynamic errors still poison
/// the engine: restoring CPU bookkeeping does not roll back queued GPU writes
/// or make an invalidated engine reusable.
pub(crate) fn submit_coarse_recording(
    resolver: &mut Resolver,
    persistent_image_atlas: &mut Option<ImageProxy>,
    previous_atlas: Option<ImageProxy>,
    recording: &Recording,
    submit: impl FnOnce() -> crate::Result<()>,
) -> crate::Result<()> {
    let result = submit();
    if result.is_err() {
        restore_unsubmitted_coarse(resolver, persistent_image_atlas, previous_atlas, recording);
    }
    result
}

/// State for a render in progress.
pub struct Render {
    fine_resources: Option<FineResources>,
    mask_buf: Option<ResourceProxy>,
    pub(crate) allocation: vello_encoding::BumpAllocators,
    pub(crate) buffer_limit: u64,
    #[cfg(feature = "debug_layers")]
    pub(crate) capture_debug: bool,

    #[cfg(feature = "debug_layers")]
    captured_buffers: Option<CapturedBuffers>,
}

#[cfg(feature = "debug_layers")]
impl Drop for Render {
    fn drop(&mut self) {
        if self.captured_buffers.is_some() {
            unreachable!("Render captured buffers without freeing them");
        }
    }
}

/// Resources produced by pipeline, needed for fine rasterization.
struct FineResources {
    aa_config: AaConfig,
    wg_count: WorkgroupSize,

    config_buf: ResourceProxy,
    bump_buf: ResourceProxy,
    tile_buf: ResourceProxy,
    segments_buf: ResourceProxy,
    ptcl_buf: ResourceProxy,
    gradient_image: ResourceProxy,
    info_bin_data_buf: ResourceProxy,
    image_atlas: ResourceProxy,
    blend_spill_buf: ResourceProxy,

    out_image: ImageProxy,
}

/// A collection of internal buffers that are used for debug visualization when the
/// `debug_layers` feature is enabled. The contents of these buffers remain GPU resident
/// and must be freed directly by the caller.
///
/// Some of these buffers are also scheduled for a download to allow their contents to be
/// processed for CPU-side validation. These buffers are documented as such.
#[cfg(feature = "debug_layers")]
pub struct CapturedBuffers {
    pub sizes: vello_encoding::BufferSizes,

    /// Buffers that remain GPU-only
    pub path_bboxes: BufferProxy,

    /// Buffers scheduled for download
    pub lines: BufferProxy,
}

#[cfg(feature = "debug_layers")]
impl CapturedBuffers {
    pub fn release_buffers(self, recording: &mut Recording) {
        recording.free_buffer(self.path_bboxes);
        recording.free_buffer(self.lines);
    }
}

impl Default for Render {
    fn default() -> Self {
        Self::new()
    }
}

impl Render {
    pub fn new() -> Self {
        Self {
            fine_resources: None,
            mask_buf: None,
            allocation: Default::default(),
            buffer_limit: u64::from(u32::MAX),
            #[cfg(feature = "debug_layers")]
            capture_debug: true,
            #[cfg(feature = "debug_layers")]
            captured_buffers: None,
        }
    }

    /// Prepare a recording for the coarse rasterization phase.
    ///
    /// The `robust` parameter controls whether we're preparing for readback
    /// of the atomic bump buffer, for robust dynamic memory.
    pub fn render_encoding_coarse(
        &mut self,
        encoding: &Encoding,
        resolver: &mut Resolver,
        shaders: &FullShaders,
        persistent_image_atlas: &mut Option<ImageProxy>,
        params: &RenderParams,
        robust: bool,
    ) -> crate::Result<Recording> {
        use vello_encoding::RenderConfig;
        if self.fine_resources.is_some() {
            return Err(crate::Error::FinePassPending);
        }
        fine_pipeline(params.antialiasing_method, shaders)?;
        let mut recording = Recording::default();
        let mut packed = vec![];

        let previous_atlas = *persistent_image_atlas;
        let (layout, ramps, images) = resolver.resolve(encoding, &mut packed);
        let gradient_image = if ramps.height == 0 {
            ResourceProxy::new_image(1, 1, ImageFormat::Rgba8)
        } else {
            let data: &[u8] = bytemuck::cast_slice(ramps.data);
            ResourceProxy::Image(recording.upload_image(
                ramps.width,
                ramps.height,
                ImageFormat::Rgba8,
                data,
            ))
        };
        let atlas_width = images.width.max(1);
        let atlas_height = images.height.max(1);
        let (image_atlas, atlas_proxy_action) = match persistent_image_atlas {
            // KONTAKTO patch: an encoding without late-bound resources resolves
            // no atlas (0x0). Keep the persistent one: trading it for a 1x1
            // stand-in drops every resident image, and the resolver, which
            // still counts them as uploaded, never uploads them again.
            Some(proxy)
                if images.width == 0
                    || (proxy.width == atlas_width && proxy.height == atlas_height) =>
            {
                (*proxy, AtlasProxyAction::Reused)
            }
            Some(proxy) => {
                recording.free_image(*proxy);
                let new_proxy = ImageProxy::new(atlas_width, atlas_height, ImageFormat::Rgba8);
                *persistent_image_atlas = Some(new_proxy);
                (new_proxy, AtlasProxyAction::Resized)
            }
            None => {
                let proxy = ImageProxy::new(atlas_width, atlas_height, ImageFormat::Rgba8);
                *persistent_image_atlas = Some(proxy);
                (proxy, AtlasProxyAction::Created)
            }
        };
        if !matches!(atlas_proxy_action, AtlasProxyAction::Reused)
            || !images.images.is_empty()
            || images.evicted != 0
        {
            if images.evicted == 0 {
                log::debug!(
                    "image atlas {}x{}: {:?}, {} upload(s)",
                    atlas_width,
                    atlas_height,
                    atlas_proxy_action,
                    images.images.len()
                );
            } else {
                log::debug!(
                    "image atlas {}x{}: {:?}, {} upload(s), {} eviction(s)",
                    atlas_width,
                    atlas_height,
                    atlas_proxy_action,
                    images.images.len(),
                    images.evicted
                );
            }
        }
        for image in images.images {
            recording.write_image(image_atlas, image.1, image.2, image.0.clone());
        }
        let mut cpu_config =
            RenderConfig::new(&layout, params.width, params.height, &params.base_color);
        if let Err(error) = grow_buffers(&mut cpu_config, self.allocation, self.buffer_limit) {
            // Preparation itself can refuse after resolve has cleaned uploads.
            restore_unsubmitted_coarse(resolver, persistent_image_atlas, previous_atlas, &recording);
            return Err(error);
        }
        // HACK: The coarse workgroup counts is the number of active bins.
        if (cpu_config.workgroup_counts.coarse.0
            * cpu_config.workgroup_counts.coarse.1
            * cpu_config.workgroup_counts.coarse.2)
            > 256
        {
            log::warn!(
                "Trying to paint too large image. {}x{}.\n\
                See https://github.com/linebender/vello/issues/680 for details",
                params.width,
                params.height
            );
        }
        let buffer_sizes = &cpu_config.buffer_sizes;
        let wg_counts = &cpu_config.workgroup_counts;

        if packed.is_empty() {
            // HACK: wgpu doesn't allow empty buffers, so we make sure that the scene buffer we upload
            // can contain at least one array item.
            // The values passed here should never be read, because the scene size in config
            // is zero.
            packed.resize(size_of::<u32>(), u8::MAX);
        }
        let scene_buf = ResourceProxy::Buffer(recording.upload("vello.scene", packed));
        let config_buf = ResourceProxy::Buffer(
            recording.upload_uniform("vello.config", bytemuck::bytes_of(&cpu_config.gpu)),
        );
        let info_bin_data_buf = ResourceProxy::new_buf(
            buffer_sizes.bin_data.size_in_bytes() as u64,
            "vello.info_bin_data_buf",
        );
        let tile_buf =
            ResourceProxy::new_buf(buffer_sizes.tiles.size_in_bytes().into(), "vello.tile_buf");
        let segments_buf = ResourceProxy::new_buf(
            buffer_sizes.segments.size_in_bytes().into(),
            "vello.segments_buf",
        );
        let ptcl_buf =
            ResourceProxy::new_buf(buffer_sizes.ptcl.size_in_bytes().into(), "vello.ptcl_buf");
        let reduced_buf = ResourceProxy::new_buf(
            buffer_sizes.path_reduced.size_in_bytes().into(),
            "vello.reduced_buf",
        );
        // TODO: really only need pathtag_wgs - 1
        recording.dispatch(
            shaders.pathtag_reduce,
            wg_counts.path_reduce,
            [config_buf, scene_buf, reduced_buf],
        );
        let mut pathtag_parent = reduced_buf;
        let mut large_pathtag_bufs = None;
        let use_large_path_scan = wg_counts.use_large_path_scan && !shaders.pathtag_is_cpu;
        if use_large_path_scan {
            let reduced2_buf = ResourceProxy::new_buf(
                buffer_sizes.path_reduced2.size_in_bytes().into(),
                "vello.reduced2_buf",
            );
            recording.dispatch(
                shaders.pathtag_reduce2,
                wg_counts.path_reduce2,
                [reduced_buf, reduced2_buf],
            );
            let reduced_scan_buf = ResourceProxy::new_buf(
                buffer_sizes.path_reduced_scan.size_in_bytes().into(),
                "reduced_scan_buf",
            );
            recording.dispatch(
                shaders.pathtag_scan1,
                wg_counts.path_scan1,
                [reduced_buf, reduced2_buf, reduced_scan_buf],
            );
            pathtag_parent = reduced_scan_buf;
            large_pathtag_bufs = Some((reduced2_buf, reduced_scan_buf));
        }

        let tagmonoid_buf = ResourceProxy::new_buf(
            buffer_sizes.path_monoids.size_in_bytes().into(),
            "vello.tagmonoid_buf",
        );
        let pathtag_scan = if use_large_path_scan {
            shaders.pathtag_scan_large
        } else {
            shaders.pathtag_scan
        };
        recording.dispatch(
            pathtag_scan,
            wg_counts.path_scan,
            [config_buf, scene_buf, pathtag_parent, tagmonoid_buf],
        );
        recording.free_resource(reduced_buf);
        if let Some((reduced2, reduced_scan)) = large_pathtag_bufs {
            recording.free_resource(reduced2);
            recording.free_resource(reduced_scan);
        }
        let path_bbox_buf = ResourceProxy::new_buf(
            buffer_sizes.path_bboxes.size_in_bytes().into(),
            "vello.path_bbox_buf",
        );
        recording.dispatch(
            shaders.bbox_clear,
            wg_counts.bbox_clear,
            [config_buf, path_bbox_buf],
        );
        let bump_buf = BufferProxy::new(
            buffer_sizes.bump_alloc.size_in_bytes().into(),
            "vello.bump_buf",
        );
        recording.clear_all(bump_buf);
        let bump_buf = ResourceProxy::Buffer(bump_buf);
        let lines_buf =
            ResourceProxy::new_buf(buffer_sizes.lines.size_in_bytes().into(), "vello.lines_buf");
        recording.dispatch(
            shaders.flatten,
            wg_counts.flatten,
            [
                config_buf,
                scene_buf,
                tagmonoid_buf,
                path_bbox_buf,
                bump_buf,
                lines_buf,
            ],
        );
        let draw_reduced_buf = ResourceProxy::new_buf(
            buffer_sizes.draw_reduced.size_in_bytes().into(),
            "vello.draw_reduced_buf",
        );
        recording.dispatch(
            shaders.draw_reduce,
            wg_counts.draw_reduce,
            [config_buf, scene_buf, draw_reduced_buf],
        );
        let draw_monoid_buf = ResourceProxy::new_buf(
            buffer_sizes.draw_monoids.size_in_bytes().into(),
            "vello.draw_monoid_buf",
        );
        let clip_inp_buf = ResourceProxy::new_buf(
            buffer_sizes.clip_inps.size_in_bytes().into(),
            "vello.clip_inp_buf",
        );
        recording.dispatch(
            shaders.draw_leaf,
            wg_counts.draw_leaf,
            [
                config_buf,
                scene_buf,
                draw_reduced_buf,
                path_bbox_buf,
                draw_monoid_buf,
                info_bin_data_buf,
                clip_inp_buf,
            ],
        );
        recording.free_resource(draw_reduced_buf);
        let clip_el_buf = ResourceProxy::new_buf(
            buffer_sizes.clip_els.size_in_bytes().into(),
            "vello.clip_el_buf",
        );
        let clip_bic_buf = ResourceProxy::new_buf(
            buffer_sizes.clip_bics.size_in_bytes().into(),
            "vello.clip_bic_buf",
        );
        if wg_counts.clip_reduce.0 > 0 {
            recording.dispatch(
                shaders.clip_reduce,
                wg_counts.clip_reduce,
                [clip_inp_buf, path_bbox_buf, clip_bic_buf, clip_el_buf],
            );
        }
        let clip_bbox_buf = ResourceProxy::new_buf(
            buffer_sizes.clip_bboxes.size_in_bytes().into(),
            "vello.clip_bbox_buf",
        );
        if wg_counts.clip_leaf.0 > 0 {
            recording.dispatch(
                shaders.clip_leaf,
                wg_counts.clip_leaf,
                [
                    config_buf,
                    clip_inp_buf,
                    path_bbox_buf,
                    clip_bic_buf,
                    clip_el_buf,
                    draw_monoid_buf,
                    clip_bbox_buf,
                ],
            );
        }
        recording.free_resource(clip_inp_buf);
        recording.free_resource(clip_bic_buf);
        recording.free_resource(clip_el_buf);
        let draw_bbox_buf = ResourceProxy::new_buf(
            buffer_sizes.draw_bboxes.size_in_bytes().into(),
            "vello.draw_bbox_buf",
        );
        let bin_header_buf = ResourceProxy::new_buf(
            buffer_sizes.bin_headers.size_in_bytes().into(),
            "vello.bin_header_buf",
        );
        recording.dispatch(
            shaders.binning,
            wg_counts.binning,
            [
                config_buf,
                draw_monoid_buf,
                path_bbox_buf,
                clip_bbox_buf,
                draw_bbox_buf,
                bump_buf,
                info_bin_data_buf,
                bin_header_buf,
            ],
        );
        recording.free_resource(draw_monoid_buf);
        recording.free_resource(clip_bbox_buf);
        // Note: this only needs to be rounded up because of the workaround to store the tile_offset
        // in storage rather than workgroup memory.
        let path_buf =
            ResourceProxy::new_buf(buffer_sizes.paths.size_in_bytes().into(), "vello.path_buf");
        recording.dispatch(
            shaders.tile_alloc,
            wg_counts.tile_alloc,
            [
                config_buf,
                scene_buf,
                draw_bbox_buf,
                bump_buf,
                path_buf,
                tile_buf,
            ],
        );
        recording.free_resource(draw_bbox_buf);
        recording.free_resource(tagmonoid_buf);
        let indirect_count_buf = BufferProxy::new(
            buffer_sizes.indirect_count.size_in_bytes().into(),
            "vello.indirect_count",
        );
        recording.dispatch(
            shaders.path_count_setup,
            wg_counts.path_count_setup,
            [bump_buf, indirect_count_buf.into()],
        );
        let seg_counts_buf = ResourceProxy::new_buf(
            buffer_sizes.seg_counts.size_in_bytes().into(),
            "vello.seg_counts_buf",
        );
        recording.dispatch_indirect(
            shaders.path_count,
            indirect_count_buf,
            0,
            [
                config_buf,
                bump_buf,
                lines_buf,
                path_buf,
                tile_buf,
                seg_counts_buf,
            ],
        );
        recording.dispatch(
            shaders.backdrop,
            wg_counts.backdrop,
            [config_buf, bump_buf, path_buf, tile_buf],
        );
        recording.dispatch(
            shaders.coarse,
            wg_counts.coarse,
            [
                config_buf,
                scene_buf,
                draw_monoid_buf,
                bin_header_buf,
                info_bin_data_buf,
                path_buf,
                tile_buf,
                bump_buf,
                ptcl_buf,
            ],
        );
        recording.dispatch(
            shaders.path_tiling_setup,
            wg_counts.path_tiling_setup,
            [bump_buf, indirect_count_buf.into(), ptcl_buf],
        );
        recording.dispatch_indirect(
            shaders.path_tiling,
            indirect_count_buf,
            0,
            [
                bump_buf,
                seg_counts_buf,
                lines_buf,
                path_buf,
                tile_buf,
                segments_buf,
            ],
        );
        recording.free_buffer(indirect_count_buf);
        recording.free_resource(seg_counts_buf);
        recording.free_resource(scene_buf);
        recording.free_resource(draw_monoid_buf);
        recording.free_resource(bin_header_buf);
        recording.free_resource(path_buf);
        let out_image = ImageProxy::new(params.width, params.height, ImageFormat::Rgba8);
        let blend_spill_buf = BufferProxy::new(
            buffer_sizes.blend_spill.size_in_bytes().into(),
            "vello.blend_spill",
        );
        self.fine_resources = Some(FineResources {
            wg_count: wg_counts.fine,
            aa_config: params.antialiasing_method,
            config_buf,
            bump_buf,
            tile_buf,
            segments_buf,
            ptcl_buf,
            gradient_image,
            info_bin_data_buf,
            blend_spill_buf: ResourceProxy::Buffer(blend_spill_buf),
            image_atlas: ResourceProxy::Image(image_atlas),
            out_image,
        });
        if robust {
            recording.download(*bump_buf.as_buf().unwrap());
        }
        recording.free_resource(bump_buf);

        #[cfg(feature = "debug_layers")]
        {
            if robust && self.capture_debug {
                let path_bboxes = *path_bbox_buf.as_buf().unwrap();
                let lines = *lines_buf.as_buf().unwrap();
                recording.download(lines);

                self.captured_buffers = Some(CapturedBuffers {
                    sizes: cpu_config.buffer_sizes,
                    path_bboxes,
                    lines,
                });
            } else {
                recording.free_resource(path_bbox_buf);
                recording.free_resource(lines_buf);
            }
        }
        #[cfg(not(feature = "debug_layers"))]
        {
            recording.free_resource(path_bbox_buf);
            recording.free_resource(lines_buf);
        }

        Ok(recording)
    }

    /// Run fine rasterization assuming the coarse phase succeeded.
    ///
    /// A missing coarse pass or unsupported AA mode is refused before consuming
    /// any resources or appending commands. The caller can retry or discard.
    pub fn record_fine(&mut self, shaders: &FullShaders, recording: &mut Recording) -> crate::Result<()> {
        let fine = self.fine_resources.as_ref().ok_or(crate::Error::MissingFinePass)?;
        let (shader, mask_lut) = fine_pipeline(fine.aa_config, shaders)?;
        let fine = self.fine_resources.take().ok_or(crate::Error::MissingFinePass)?;
        let mut resources = Vec::with_capacity(if mask_lut.is_some() { 9 } else { 8 });
        resources.extend([
            fine.config_buf,
            fine.segments_buf,
            fine.ptcl_buf,
            fine.info_bin_data_buf,
            fine.blend_spill_buf,
            ResourceProxy::Image(fine.out_image),
            fine.gradient_image,
            fine.image_atlas,
        ]);
        if let Some(make_lut) = mask_lut {
            let mask = self.mask_buf.get_or_insert_with(|| {
                recording.upload("vello.mask_lut", make_lut()).into()
            });
            resources.push(*mask);
        }
        recording.dispatch(shader, fine.wg_count, resources);
        self.free_fine(fine, recording);
        Ok(())
    }

    pub(crate) fn discard(&mut self, recording: &mut Recording) {
        if let Some(fine) = self.fine_resources.take() {
            self.free_fine(fine, recording);
        }
        #[cfg(feature = "debug_layers")]
        if let Some(captured) = self.captured_buffers.take() {
            captured.release_buffers(recording);
        }
    }

    fn free_fine(&mut self, fine: FineResources, recording: &mut Recording) {
        recording.free_resource(fine.config_buf);
        recording.free_resource(fine.tile_buf);
        recording.free_resource(fine.segments_buf);
        recording.free_resource(fine.ptcl_buf);
        recording.free_resource(fine.gradient_image);
        recording.free_resource(fine.info_bin_data_buf);
        recording.free_resource(fine.blend_spill_buf);
        // TODO: make mask buf persistent
        if let Some(mask_buf) = self.mask_buf.take() {
            recording.free_resource(mask_buf);
        }
    }

    /// Get the output image.
    ///
    /// This is going away, as the caller will add the output image to the bind
    /// map.
    pub fn out_image(&self) -> ImageProxy {
        self.fine_resources.as_ref().unwrap().out_image
    }

    pub fn bump_buf(&self) -> BufferProxy {
        *self
            .fine_resources
            .as_ref()
            .unwrap()
            .bump_buf
            .as_buf()
            .unwrap()
    }

    #[cfg(feature = "debug_layers")]
    pub fn take_captured_buffers(&mut self) -> Option<CapturedBuffers> {
        self.captured_buffers.take()
    }
}

fn grow_buffers(
    config: &mut vello_encoding::RenderConfig,
    need: vello_encoding::BumpAllocators,
    limit: u64,
) -> crate::Result<()> {
    use vello_encoding::BufferSize;
    fn grow<T: Copy>(
        buffer: &mut BufferSize<T>,
        need: u64,
        limit: u64,
        name: &'static str,
    ) -> crate::Result<()> {
        let mut count = u64::from(buffer.len()).max(need);
        if need > u64::from(buffer.len()) {
            // Failed stages can expose additional work on the next pass.
            count = count
                .next_power_of_two()
                .min(limit.min(u64::from(u32::MAX)) / size_of::<T>() as u64)
                .max(need);
        }
        let bytes = count.checked_mul(size_of::<T>() as u64).unwrap_or(u64::MAX);
        if bytes > limit.min(u64::from(u32::MAX)) {
            return Err(crate::Error::AllocationLimit(name, bytes, limit));
        }
        *buffer = BufferSize::new(count as u32);
        Ok(())
    }
    let sizes = &mut config.buffer_sizes;
    grow(
        &mut sizes.bin_data,
        u64::from(need.binning) + u64::from(config.gpu.layout.bin_data_start),
        limit,
        "binning",
    )?;
    grow(&mut sizes.lines, need.lines.into(), limit, "lines")?;
    grow(&mut sizes.tiles, need.tile.into(), limit, "tiles")?;
    grow(
        &mut sizes.seg_counts,
        need.seg_counts.into(),
        limit,
        "segment counts",
    )?;
    grow(&mut sizes.segments, need.segments.into(), limit, "segments")?;
    grow(
        &mut sizes.blend_spill,
        need.blend.into(),
        limit,
        "blend spill",
    )?;
    grow(&mut sizes.ptcl, need.ptcl.into(), limit, "commands")?;
    config.gpu.binning_size = sizes.bin_data.len() - config.gpu.layout.bin_data_start;
    config.gpu.lines_size = sizes.lines.len();
    config.gpu.tiles_size = sizes.tiles.len();
    config.gpu.seg_counts_size = sizes.seg_counts.len();
    config.gpu.segments_size = sizes.segments.len();
    config.gpu.blend_size = sizes.blend_spill.len();
    config.gpu.ptcl_size = sizes.ptcl.len();
    Ok(())
}

#[cfg(test)]
mod allocation_tests {
    use super::*;
    #[test]
    fn grow_and_limit_gpu_allocations() {
        let mut config = vello_encoding::RenderConfig::new(
            &Default::default(),
            1180,
            760,
            &peniko::Color::BLACK,
        );
        let need = vello_encoding::BumpAllocators {
            blend: 5_908_480,
            binning: 300_000,
            ..Default::default()
        };
        grow_buffers(&mut config, need, 128 << 20).unwrap();
        assert!(config.gpu.blend_size >= need.blend);
        assert_eq!(config.buffer_sizes.blend_spill.len(), config.gpu.blend_size);
        assert!(config.gpu.binning_size >= need.binning);
        assert!(
            grow_buffers(
                &mut config,
                vello_encoding::BumpAllocators {
                    blend: u32::MAX,
                    ..need
                },
                128 << 20
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod doctor_tests {
    use super::*;
    use crate::{Error, ShaderId, recording::Command};

    fn shaders() -> FullShaders {
        FullShaders {
            pathtag_reduce: ShaderId(0), pathtag_reduce2: ShaderId(0),
            pathtag_scan1: ShaderId(0), pathtag_scan: ShaderId(0), pathtag_scan_large: ShaderId(0),
            bbox_clear: ShaderId(0), flatten: ShaderId(0), draw_reduce: ShaderId(0),
            draw_leaf: ShaderId(0), clip_reduce: ShaderId(0), clip_leaf: ShaderId(0),
            binning: ShaderId(0), tile_alloc: ShaderId(0), backdrop: ShaderId(0),
            path_count_setup: ShaderId(0), path_count: ShaderId(0), coarse: ShaderId(0),
            path_tiling_setup: ShaderId(0), path_tiling: ShaderId(0),
            fine_area: Some(ShaderId(10)), fine_msaa8: Some(ShaderId(11)),
            fine_msaa16: Some(ShaderId(12)), pathtag_is_cpu: false,
        }
    }

    fn pending(aa_config: AaConfig) -> Render {
        let mut render = Render::new();
        let buffer = || ResourceProxy::new_buf(16, "test");
        render.fine_resources = Some(FineResources {
            aa_config, wg_count: (2, 3, 1), config_buf: buffer(), bump_buf: buffer(),
            tile_buf: buffer(), segments_buf: buffer(), ptcl_buf: buffer(),
            gradient_image: ResourceProxy::new_image(1, 1, ImageFormat::Rgba8),
            info_bin_data_buf: buffer(), image_atlas: ResourceProxy::new_image(1, 1, ImageFormat::Rgba8),
            blend_spill_buf: buffer(), out_image: ImageProxy::new(8, 8, ImageFormat::Rgba8),
        });
        render
    }

    #[test]
    fn fine_modes_keep_shader_bindings_masks_and_release_commands() {
        for (aa, shader, bindings) in [(AaConfig::Area, 10, 8), (AaConfig::Msaa8, 11, 9), (AaConfig::Msaa16, 12, 9)] {
            let mut render = pending(aa);
            let mut recording = Recording::default();
            render.record_fine(&shaders(), &mut recording).unwrap();
            assert!(render.fine_resources.is_none());
            assert!(render.mask_buf.is_none());
            let dispatches: Vec<_> = recording.commands.iter().filter_map(|command| match command {
                Command::Dispatch(id, wg, resources) => Some((id.0, *wg, resources.len())),
                _ => None,
            }).collect();
            assert_eq!(dispatches, [(shader, (2, 3, 1), bindings)]);
            let uploads: Vec<_> = recording.commands.iter().filter_map(|command| match command {
                Command::Upload(_, data) => Some(data), _ => None,
            }).collect();
            match aa {
                AaConfig::Area => assert!(uploads.is_empty()),
                AaConfig::Msaa8 => assert_eq!(uploads, [&make_mask_lut()]),
                AaConfig::Msaa16 => assert_eq!(uploads, [&make_mask_lut_16()]),
            }
            // Seven transient fine resources, plus the MSAA mask when used.
            let frees = recording.commands.iter().filter(|command|
                matches!(command, Command::FreeBuffer(_) | Command::FreeImage(_))).count();
            assert_eq!(frees, if aa == AaConfig::Area { 7 } else { 8 });
            let count = recording.commands.len();
            assert!(matches!(render.record_fine(&shaders(), &mut recording), Err(Error::MissingFinePass)));
            assert_eq!(recording.commands.len(), count);
        }
    }

    #[test]
    fn unsupported_fine_shader_preserves_pending_resources_for_retry_or_discard() {
        for aa in [AaConfig::Area, AaConfig::Msaa8, AaConfig::Msaa16] {
            let mut render = pending(aa);
            let mut recording = Recording::default();
            let mut missing = shaders();
            missing.fine_area = None; missing.fine_msaa8 = None; missing.fine_msaa16 = None;
            assert!(matches!(render.record_fine(&missing, &mut recording), Err(Error::UnsupportedAntialiasing(mode)) if mode == aa));
            assert!(recording.commands.is_empty());
            assert!(render.fine_resources.is_some());
            render.record_fine(&shaders(), &mut recording).unwrap();
            assert!(render.fine_resources.is_none());
            let mut render = pending(aa);
            assert!(render.record_fine(&missing, &mut Recording::default()).is_err());
            let mut release = Recording::default();
            render.discard(&mut release);
            assert!(render.fine_resources.is_none());
            assert_eq!(release.commands.len(), 7);
        }
    }
}
