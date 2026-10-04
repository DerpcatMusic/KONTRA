//! The paint walk onto a classic `vello::Scene`: the GPU canvas.
//!
//! Classic Vello takes the transform and brush per call where the
//! sparse-strip scenes hold them as state, so [`Classic`] is that state.
//! Encoding is serialisation only -- flattening, binning and rasterising all
//! run in compute shaders -- which is why a frame that changed costs the CPU
//! a fraction of a millisecond however much of the window it touches.
use crate::kurbo::{Affine, BezPath, Rect, Stroke};
use crate::{Cache, Canvas, Stored};
use std::collections::HashMap;
use std::sync::Arc;
use vello::peniko::{
    self, BlendMode, Blob, Brush, Compose, Fill, ImageAlphaType, ImageBrush, ImageData,
    ImageFormat, Mix,
};
use vello_common::paint::{ImageId, ImageSource, PaintType};

/// A GPU texture registered with the renderer, as the image Vello samples.
pub(crate) type Textures = HashMap<u64, ImageData>;

/// Everything a clip, layer or fill covers when it has nothing tighter:
/// one tile past the target, so no edge is anti-aliased against the void.
pub(crate) fn everything(size: [u32; 2]) -> Rect {
    Rect::new(
        -16.,
        -16.,
        f64::from(size[0]) + 16.,
        f64::from(size[1]) + 16.,
    )
}

pub struct Classic<'a> {
    pub(crate) scene: &'a mut vello::Scene,
    pub(crate) cache: &'a mut Cache,
    pub(crate) textures: &'a Textures,
    /// The images this walk handed out, indexed by the opaque id that
    /// stands for them in a [`PaintType`].
    pub(crate) images: Vec<ImageData>,
    /// Device pixels, for the layers that have no shape of their own.
    pub(crate) size: [u32; 2],
    pub(crate) transform: Affine,
    pub(crate) brush: Brush,
    pub(crate) brush_transform: Option<Affine>,
    pub(crate) stroke: Stroke,
}

impl<'a> Classic<'a> {
    pub(crate) fn new(
        scene: &'a mut vello::Scene,
        cache: &'a mut Cache,
        textures: &'a Textures,
        size: [u32; 2],
    ) -> Self {
        Self {
            scene,
            cache,
            textures,
            images: Vec::new(),
            size,
            transform: Affine::IDENTITY,
            brush: Brush::Solid(peniko::Color::TRANSPARENT),
            brush_transform: None,
            stroke: Stroke::new(1.0),
        }
    }

    fn color(&self) -> peniko::Color {
        match self.brush {
            Brush::Solid(c) => c,
            _ => peniko::Color::BLACK,
        }
    }

    /// A layer over the whole target, in device space.
    fn push_everything(&mut self, blend: BlendMode, alpha: f32) {
        let clip = everything(self.size);
        self.scene
            .push_layer(Fill::NonZero, blend, alpha, Affine::IDENTITY, &clip);
    }

    pub(crate) fn hand_out(&mut self, image: ImageData) -> PaintType {
        let id = ImageId::new(self.images.len() as u32);
        self.images.push(image);
        vello_common::paint::Image {
            image: ImageSource::OpaqueId {
                id,
                may_have_transparency: true,
            },
            sampler: vello::peniko::ImageSampler::default(),
        }
        .into()
    }
}

/// An app's pixels as Vello's image. MUI's
/// buffers are straight alpha, which Vello premultiplies as it samples.
fn image_data(img: &mui_scene::Image) -> ImageData {
    ImageData {
        // A copy, once per buffer: sharing it would keep the app's buffer
        // alive for as long as Vello's encoding and atlas remember it.
        data: Blob::new(Arc::new(img.rgba.to_vec())),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: img.width,
        height: img.height,
    }
}

impl Canvas for Classic<'_> {
    fn begin_frame(&mut self) {
        self.cache.tick();
    }
    fn set_transform(&mut self, t: Affine) {
        self.transform = t;
    }
    fn set_paint(&mut self, p: PaintType) {
        self.brush = match p {
            PaintType::Solid(c) => Brush::Solid(c),
            PaintType::Gradient(g) => Brush::Gradient(g),
            PaintType::Image(i) => match i.image {
                ImageSource::OpaqueId { id, .. } => match self.images.get(id.as_u32() as usize) {
                    Some(image) => Brush::Image(ImageBrush {
                        image: image.clone(),
                        sampler: i.sampler,
                    }),
                    None => Brush::Solid(peniko::Color::TRANSPARENT),
                },
                // `image` below only ever hands out opaque ids.
                ImageSource::Pixmap(_) => Brush::Solid(peniko::Color::TRANSPARENT),
            },
        };
    }
    fn set_paint_transform(&mut self, t: Affine) {
        self.brush_transform = Some(t);
    }
    fn reset_paint_transform(&mut self) {
        self.brush_transform = None;
    }
    fn image(&mut self, img: &mui_scene::Image) -> Option<PaintType> {
        if let Some(key) = img.texture {
            let image = self.textures.get(&key)?.clone();
            return Some(self.hand_out(image));
        }
        self.cache.sweep();
        let image = if let Some(Stored::Image(i)) = self.cache.find(&img.rgba) {
            i.clone()
        } else {
            let expected = (img.width as usize)
                .checked_mul(img.height as usize)?
                .checked_mul(4)?;
            if expected == 0 || img.rgba.len() != expected {
                return None;
            }
            let i = image_data(img);
            self.cache.remember(&img.rgba, Stored::Image(i.clone()));
            i
        };
        Some(self.hand_out(image))
    }
    fn set_stroke(&mut self, s: Stroke) {
        self.stroke = s;
    }
    fn fill_path(&mut self, p: &BezPath) {
        self.scene.fill(
            Fill::NonZero,
            self.transform,
            &self.brush,
            self.brush_transform,
            p,
        );
    }
    fn fill_path_with_rule(&mut self, p: &BezPath, rule: Fill) -> bool {
        self.scene
            .fill(rule, self.transform, &self.brush, self.brush_transform, p);
        true
    }
    fn stroke_path(&mut self, p: &BezPath) {
        self.scene.stroke(
            &self.stroke,
            self.transform,
            &self.brush,
            self.brush_transform,
            p,
        );
    }
    fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32, invert: bool) {
        let c = self.color();
        let (radius, std_dev) = (f64::from(radius), f64::from(std_dev));
        if !invert {
            self.scene
                .draw_blurred_rounded_rect(self.transform, *r, c, radius, std_dev);
            return;
        }
        // An inset shadow is the colour minus the blurred rect's coverage.
        // The walk has clipped it to the outline already; this box only
        // has to reach past everything that outline can hold.
        let reach = r.inflate(
            4. * std_dev + r.width().max(r.height()),
            4. * std_dev + r.width().max(r.height()),
        );
        self.scene
            .push_layer(Fill::NonZero, Mix::Normal, 1.0, self.transform, &reach);
        self.scene
            .fill(Fill::NonZero, self.transform, c, None, &reach);
        self.scene.push_layer(
            Fill::NonZero,
            BlendMode::new(Mix::Normal, Compose::DestOut),
            1.0,
            self.transform,
            &reach,
        );
        self.scene.draw_blurred_rounded_rect(
            self.transform,
            *r,
            peniko::Color::BLACK,
            radius,
            std_dev,
        );
        self.scene.pop_layer();
        self.scene.pop_layer();
    }
    fn push_clip(&mut self, p: &BezPath) {
        self.scene.push_clip_layer(Fill::NonZero, self.transform, p);
    }
    fn pop_clip(&mut self) {
        self.scene.pop_layer();
    }
    fn push_layer(&mut self, blend: BlendMode, opacity: f32) {
        self.push_everything(blend, opacity);
    }
    fn pop_layer(&mut self) {
        self.scene.pop_layer();
    }
    fn glyphs(&mut self, text: &mui_scene::Text) {
        let mut start = 0;
        while start < text.glyphs.len() {
            let font_index = text.glyphs[start].font;
            let end = text.glyphs[start + 1..]
                .iter()
                .position(|glyph| glyph.font != font_index)
                .map_or(text.glyphs.len(), |offset| start + 1 + offset);
            // A hand-built run can name a face it does not carry.
            let Some(face) = text.fonts.get(font_index) else {
                start = end;
                continue;
            };
            let (font, _) = self.cache.font(face);
            let coords = text.font_coords.get(font_index).map_or(&[][..], |c| &c[..]);
            let (ox, oy) = (text.origin.x as f32, text.origin.y as f32);
            self.scene
                .draw_glyphs(&font)
                .transform(self.transform)
                .brush(&self.brush)
                .font_size(text.size)
                // Off for the frame after an axis moved: see Text::hint.
                .hint(text.hint)
                // The run was measured at this instance; drawing the default
                // one under its advances is how a bold readout goes ragged.
                .normalized_coords(coords)
                .draw(
                    Fill::NonZero,
                    text.glyphs[start..end].iter().map(|g| vello::Glyph {
                        id: g.id,
                        x: ox + g.x,
                        y: oy + g.y,
                    }),
                );
            start = end;
        }
    }
}
