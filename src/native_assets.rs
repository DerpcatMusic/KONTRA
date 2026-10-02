//! Native UI assets decoded only by the existing preparation worker.
//! SVG paths stay retained vectors; bitmap assets retain their source pixels.
use moose::mui::mui::scene::{Image, Vector, VectorCommand};
use std::{
    collections::HashMap,
    io::Cursor,
    path::Path,
    sync::{Arc, Mutex},
};
use vello_svg::{
    kurbo::{Affine, Shape},
    peniko::{BlendMode, Brush, Fill},
    usvg,
};

const INPUT_LIMIT: usize = 32 << 20;
const PIXEL_LIMIT: usize = 128 << 20;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AssetRef {
    pub resource: String,
    pub resizable: bool,
}
#[derive(Clone, Debug)]
pub(crate) enum Asset {
    Vector(Arc<Vector>),
    Bitmap(Arc<Image>),
}
impl Asset {
    /// Conservative retained storage charge; shared buffers are charged per asset
    /// so cache budgets never depend on deduplicating allocator ownership.
    pub fn retained_bytes(&self) -> usize {
        use std::mem::size_of;
        let vector_bytes = |v: &Vector| {
            let mut bytes = size_of::<Vector>() + v.commands().len() * size_of::<VectorCommand>();
            for c in v.commands() {
                let (path, brush, stroke) = match c {
                    VectorCommand::Fill { path, brush, .. } => (path, Some(brush), None),
                    VectorCommand::Stroke {
                        path,
                        brush,
                        stroke,
                        ..
                    } => (path, Some(brush), Some(stroke)),
                    VectorCommand::PushLayer { path, .. } => (path, None, None),
                    VectorCommand::PopLayer => continue,
                };
                // Decoder paths grow geometrically; allow their spare capacity.
                bytes = bytes.saturating_add(
                    path.elements()
                        .len()
                        .max(8)
                        .saturating_mul(2 * size_of::<vello_svg::kurbo::PathEl>()),
                );
                if let Some(Brush::Gradient(g)) = brush {
                    bytes = bytes.saturating_add(
                        g.stops.capacity() * size_of::<vello_svg::peniko::ColorStop>(),
                    );
                }
                if let Some(s) = stroke {
                    bytes = bytes.saturating_add(s.dash_pattern.capacity() * size_of::<f64>());
                }
            }
            if let Some(source) = v.raster_source() {
                bytes = bytes.saturating_add(source.retained_bytes());
            }
            bytes
        };
        size_of::<Self>().saturating_add(match self {
            Self::Vector(v) => vector_bytes(v),
            Self::Bitmap(i) => size_of::<Image>().saturating_add(i.rgba.len()),
        })
    }
    pub fn size(&self) -> (f64, f64) {
        match self {
            Self::Vector(v) => (v.width, v.height),
            Self::Bitmap(i) => (i.width as f64, i.height as f64),
        }
    }
}

/// Scoped to one prepared instrument. Both successes and failures are retained;
/// no global cache or file work is performed by the scene snapshot/paint path.
#[derive(Clone, Debug, Default)]
pub(crate) struct Assets {
    pub entries: HashMap<String, Result<Arc<Asset>, String>>,
}
impl Assets {
    /// Call on the preparation worker with evaluated Image.file references.
    pub fn prepare(instrument: &Path, names: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Self::prepare_bounded(instrument, names, PIXEL_LIMIT)
    }
    /// Remaining instrument-wide retained budget, supplied by the scene owner.
    /// Decode remains off-thread; a rejected result is dropped before insertion.
    pub fn prepare_bounded(
        instrument: &Path,
        names: impl IntoIterator<Item = impl AsRef<str>>,
        mut remaining: usize,
    ) -> Self {
        let mut out = Self::default();
        let mut source = crate::resources::Resources::of(instrument, "native_ui");
        for name in names {
            let literal = name.as_ref();
            if out.entries.contains_key(literal) {
                continue;
            }
            let decoded = resource_name(literal).and_then(|name| {
                if remaining == 0 {
                    return Err("Native UI retained asset budget exhausted".into());
                }
                let bytes = source
                    .read_limited(&name, INPUT_LIMIT)?
                    .ok_or_else(|| format!("Native UI asset {name:?} was not found"))?;
                let asset = decode(&name, &bytes)?;
                let charge = asset.retained_bytes();
                if charge > remaining {
                    return Err("Native UI asset exceeds remaining retained budget".into());
                }
                remaining -= charge;
                Ok(Arc::new(asset))
            });
            out.entries.insert(literal.to_owned(), decoded);
        }
        out
    }
}

/// Preserve nested native_ui namespaces; never append a PNG suffix or search
/// by basename, which can silently select a different asset.
fn resource_name(name: &str) -> Result<String, String> {
    let name = name.replace('\\', "/");
    let name = name.strip_prefix("Resources/native_ui/").unwrap_or(&name);
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(':')
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err("Native UI asset must be a relative resource path".into());
    }
    Ok(name.into())
}

fn decode(name: &str, bytes: &[u8]) -> Result<Asset, String> {
    if bytes.len() > INPUT_LIMIT {
        return Err("Native UI asset exceeds 32 MiB".into());
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "svg" => decode_svg(bytes).map(|v| Asset::Vector(Arc::new(v))),
        "webp" => decode_webp(bytes).map(|i| Asset::Bitmap(Arc::new(i))),
        "png" | "jpg" | "jpeg" => crate::artwork::decode_report_bounded(bytes, PIXEL_LIMIT)
            .map(|i| Asset::Bitmap(Arc::new(i))),
        _ => Err(format!("Unsupported native UI asset format {ext:?}")),
    }
}
fn decode_webp(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder =
        image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    decoder.set_memory_limit(PIXEL_LIMIT);
    if decoder.is_animated() {
        return Err("Animated WebP is not supported by the native UI asset adapter".into());
    }
    let (w, h) = decoder.dimensions();
    let pixels = (w as usize)
        .checked_mul(h as usize)
        .ok_or("WebP dimensions overflow")?;
    if pixels == 0 || pixels.checked_mul(4).is_none_or(|n| n > PIXEL_LIMIT) {
        return Err("WebP exceeds 128 MiB of decoded pixels".into());
    }
    let mut raw = vec![
        0;
        decoder
            .output_buffer_size()
            .ok_or("WebP dimensions overflow")?
    ];
    decoder.read_image(&mut raw).map_err(|e| e.to_string())?;
    let rgba = if decoder.has_alpha() {
        raw
    } else {
        raw.chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    };
    Image::rgba(w, h, rgba).ok_or_else(|| "Invalid WebP pixel buffer".into())
}

fn decode_svg(bytes: &[u8]) -> Result<Vector, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    // usvg may discard unsupported/unresolved nodes during preprocessing. State
    // these gaps before parsing, rather than showing an apparently complete UI.
    let xml = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    for node in xml.descendants().filter(|n| n.is_element()) {
        let name = node.tag_name().name();
        if matches!(
            name,
            "text"
                | "image"
                | "mask"
                | "pattern"
                | "script"
                | "animate"
                | "animateTransform"
                | "animateMotion"
                | "set"
                | "foreignObject"
        ) {
            return Err(format!("Native SVG feature {name:?} is not supported"));
        }
    }
    let filtered = xml
        .descendants()
        .any(|n| n.is_element() && n.tag_name().name() == "filter");
    let mut options = usvg::Options::default();
    // No side-channel filesystem/network reads from SVG image hrefs.
    options.image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: Box::new(|_, _, _| None),
        resolve_string: Box::new(|_, _| None),
    };
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|e| e.to_string())?;
    if filtered {
        let size = tree.size();
        return Vector::filtered(
            size.width() as f64,
            size.height() as f64,
            Arc::new(FilteredSvg {
                tree,
                source: bytes.into(),
                images: Mutex::new(Vec::new()),
            }),
        )
        .ok_or_else(|| "Invalid filtered SVG extent".into());
    }
    let mut sink = Commands::default();
    sink.group(tree.root(), Affine::IDENTITY, false)?;
    let size = tree.size();
    Vector::new(size.width() as f64, size.height() as f64, sink.0)
        .ok_or_else(|| "Invalid native SVG geometry or layer stack".into())
}
// Effects require compositing. Preserve the normalized vector tree and source;
// only the exact requested device-sized intermediate is rasterized. The source
// belongs to this instrument's asset cache, with no global cache or worker.
const FILTER_CACHE_LIMIT: usize = 8 << 20;
#[derive(Debug)]
struct FilteredSvg {
    tree: usvg::Tree,
    source: Arc<[u8]>,
    images: Mutex<Vec<Image>>,
}
impl moose::mui::mui::scene::RasterSource for FilteredSvg {
    fn retained_bytes(&self) -> usize {
        // usvg's normalized tree expands source records/paths. Conservatively
        // charge the source, an expansion allowance, and the full raster cache.
        self.source
            .len()
            .saturating_mul(65)
            .saturating_add(FILTER_CACHE_LIMIT)
    }
    fn rasterize(&self, width: u32, height: u32) -> Result<Image, String> {
        let charge = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("SVG filter extent overflow")?;
        if charge == 0 || charge > FILTER_CACHE_LIMIT {
            return Err(
                "SVG filter requested device extent exceeds 8 MiB intermediate budget".into(),
            );
        }
        let mut images = self
            .images
            .lock()
            .map_err(|_| "SVG filter cache unavailable")?;
        if let Some(image) = images
            .iter()
            .find(|i| i.width == width && i.height == height)
        {
            return Ok(image.clone());
        }
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
            .ok_or("Cannot allocate SVG filter intermediate")?;
        let size = self.tree.size();
        resvg::render(
            &self.tree,
            resvg::tiny_skia::Transform::from_scale(
                width as f32 / size.width(),
                height as f32 / size.height(),
            ),
            &mut pixmap.as_mut(),
        );
        // resvg produces premultiplied sRGB; MUI's shared CPU/GPU image seam
        // accepts straight RGBA and premultiplies once during upload.
        let mut rgba = pixmap.take();
        for p in rgba.chunks_exact_mut(4) {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = if a == 0 {
                    0
                } else {
                    ((*c as u32 * 255 + a / 2) / a).min(255) as u8
                };
            }
        }
        let image = Image::rgba(width, height, rgba).ok_or("Invalid SVG filter pixels")?;
        while !images.is_empty()
            && (images.len() >= 2
                || images.iter().map(|i| i.rgba.len()).sum::<usize>() + charge > FILTER_CACHE_LIMIT)
        {
            images.remove(0);
        }
        images.push(image.clone());
        Ok(image)
    }
}
#[derive(Default)]
struct Commands(Vec<VectorCommand>);
impl Commands {
    fn layer(&mut self, group: &usvg::Group, base: Affine, blend: BlendMode, alpha: f32) {
        let b = group.layer_bounding_box();
        self.0.push(VectorCommand::PushLayer {
            path: vello_svg::kurbo::Rect::new(
                b.x() as f64,
                b.y() as f64,
                b.right() as f64,
                b.bottom() as f64,
            )
            .to_path(0.01),
            transform: base * vello_svg::util::to_affine(&group.abs_transform()),
            blend,
            alpha,
        });
    }
    // usvg supplies normalized geometry and absolute transforms. Layer masks
    // union multiple clip children at the current device scale; nested masks
    // intersect them. No flattening, source-pixel bitmap or asset-name rule.
    fn group(&mut self, group: &usvg::Group, base: Affine, clip: bool) -> Result<(), String> {
        use vello_svg::peniko::{Compose, Mix};
        self.layer(
            group,
            base,
            if clip {
                BlendMode::default()
            } else {
                blend(group.blend_mode()).into()
            },
            if clip { 1. } else { group.opacity().get() },
        );
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => self.group(g, base, clip)?,
                usvg::Node::Path(path) if path.is_visible() => {
                    let shape = vello_svg::util::to_bez_path(path);
                    let transform = base * vello_svg::util::to_affine(&path.abs_transform());
                    if clip {
                        self.0.push(VectorCommand::Fill {
                            path: shape,
                            transform,
                            brush: Brush::Solid(vello_svg::peniko::Color::WHITE),
                            brush_transform: Affine::IDENTITY,
                            rule: path.fill().map_or(Fill::NonZero, |f| match f.rule() {
                                usvg::FillRule::NonZero => Fill::NonZero,
                                usvg::FillRule::EvenOdd => Fill::EvenOdd,
                            }),
                        });
                    } else {
                        let fill = || -> Result<Option<VectorCommand>, String> {
                            let Some(fill) = path.fill() else {
                                return Ok(None);
                            };
                            let (brush, brush_transform) =
                                vello_svg::util::to_brush(fill.paint(), fill.opacity())
                                    .ok_or("Unsupported native SVG paint")?;
                            Ok(Some(VectorCommand::Fill {
                                path: shape.clone(),
                                transform,
                                brush,
                                brush_transform,
                                rule: match fill.rule() {
                                    usvg::FillRule::NonZero => Fill::NonZero,
                                    usvg::FillRule::EvenOdd => Fill::EvenOdd,
                                },
                            }))
                        };
                        let stroke = || -> Result<Option<VectorCommand>, String> {
                            let Some(stroke) = path.stroke() else {
                                return Ok(None);
                            };
                            let (brush, brush_transform) =
                                vello_svg::util::to_brush(stroke.paint(), stroke.opacity())
                                    .ok_or("Unsupported native SVG paint")?;
                            Ok(Some(VectorCommand::Stroke {
                                path: shape.clone(),
                                transform,
                                brush,
                                brush_transform,
                                stroke: vello_svg::util::to_stroke(stroke),
                            }))
                        };
                        let pair = match path.paint_order() {
                            usvg::PaintOrder::FillAndStroke => [fill()?, stroke()?],
                            usvg::PaintOrder::StrokeAndFill => [stroke()?, fill()?],
                        };
                        self.0.extend(pair.into_iter().flatten());
                    }
                }
                usvg::Node::Path(_) => {}
                _ => return Err("Unsupported native SVG image or text node".into()),
            }
        }
        if let Some(path) = group.clip_path() {
            self.layer(
                group,
                base,
                BlendMode::new(Mix::Normal, Compose::DestIn),
                1.,
            );
            self.clip(
                path,
                base * vello_svg::util::to_affine(&group.abs_transform()),
            )?;
            self.0.push(VectorCommand::PopLayer);
        }
        self.0.push(VectorCommand::PopLayer);
        Ok(())
    }
    fn clip(&mut self, path: &usvg::ClipPath, base: Affine) -> Result<(), String> {
        use vello_svg::peniko::{Compose, Mix};
        let at = base * vello_svg::util::to_affine(&path.transform());
        self.group(path.root(), at, true)?;
        if let Some(nested) = path.clip_path() {
            // ClipPath's own clip-path intersects its accumulated child union.
            // Keep that union in a layer while applying the nested alpha mask.
            // A enclosing normal layer was emitted by group(); insert its close
            // after the nested mask rather than closing before it.
            let close = self.0.pop().ok_or("Invalid native SVG clip layer")?;
            self.layer(
                path.root(),
                at,
                BlendMode::new(Mix::Normal, Compose::DestIn),
                1.,
            );
            self.clip(nested, at)?;
            self.0.push(VectorCommand::PopLayer);
            self.0.push(close);
        }
        Ok(())
    }
}
fn blend(mode: usvg::BlendMode) -> vello_svg::peniko::Mix {
    use vello_svg::peniko::Mix;
    match mode {
        usvg::BlendMode::Normal => Mix::Normal,
        usvg::BlendMode::Multiply => Mix::Multiply,
        usvg::BlendMode::Screen => Mix::Screen,
        usvg::BlendMode::Overlay => Mix::Overlay,
        usvg::BlendMode::Darken => Mix::Darken,
        usvg::BlendMode::Lighten => Mix::Lighten,
        usvg::BlendMode::ColorDodge => Mix::ColorDodge,
        usvg::BlendMode::ColorBurn => Mix::ColorBurn,
        usvg::BlendMode::HardLight => Mix::HardLight,
        usvg::BlendMode::SoftLight => Mix::SoftLight,
        usvg::BlendMode::Difference => Mix::Difference,
        usvg::BlendMode::Exclusion => Mix::Exclusion,
        usvg::BlendMode::Hue => Mix::Hue,
        usvg::BlendMode::Saturation => Mix::Saturation,
        usvg::BlendMode::Color => Mix::Color,
        usvg::BlendMode::Luminosity => Mix::Luminosity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_assets_keep_namespace_and_svg_geometry() {
        assert_eq!(
            resource_name(r"assets\pics\title.svg").unwrap(),
            "assets/pics/title.svg"
        );
        assert!(resource_name("assets/../other.svg").is_err());
        assert!(resource_name("C:\\outside.svg").is_err());
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16"><defs><linearGradient id="g" gradientTransform="rotate(20)"><stop stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient><clipPath id="c"><path d="M0 0H24V16H0Z"/></clipPath></defs><g opacity=".7" clip-path="url(#c)" transform="translate(1 2)"><path fill="url(#g)" fill-rule="evenodd" stroke="#fff" stroke-width=".5" d="M0 0H20V12H0ZM4 4H8V8H4Z"/></g></svg>"##;
        let Asset::Vector(vector) = decode("assets/pics/test.svg", svg).unwrap() else {
            panic!("SVG was rasterized")
        };
        assert_eq!((vector.width, vector.height), (24., 16.));
        assert!(vector.commands().iter().any(|c| matches!(
            c,
            VectorCommand::Fill {
                rule: Fill::EvenOdd,
                brush: Brush::Gradient(_),
                ..
            }
        )));
        assert!(
            vector
                .commands()
                .iter()
                .any(|c| matches!(c, VectorCommand::Stroke { .. }))
        );
        assert!(
            vector
                .commands()
                .iter()
                .any(|c| matches!(c, VectorCommand::PushLayer { alpha, .. } if *alpha == 0.7))
        );
        assert!(decode("bad.svg", br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16"><mask id="m"/></svg>"#).unwrap_err().contains("mask"));
    }
    #[test]
    fn native_svg_filter_glow_preserves_pixels_and_device_cache_identity() {
        use moose::mui::mui::{
            scene::{self, prelude::*},
            vello::{
                self,
                vello_cpu::{Pixmap, RenderContext, Resources},
            },
        };
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><filter id="glow" x="0" y="0" width="20" height="20" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feFlood flood-opacity="0" result="background"/><feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/><feOffset/><feGaussianBlur stdDeviation="2"/><feComposite in2="hardAlpha" operator="out"/><feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 .5 0"/><feBlend in2="background" mode="normal" result="effect"/><feBlend in="SourceGraphic" in2="effect" mode="normal"/></filter></defs><rect x="7" y="7" width="6" height="6" fill="#ff0000" filter="url(#glow)"/></svg>"##;
        let vector = Arc::new(decode_svg(svg).unwrap());
        let source = vector.raster_source().unwrap();
        let first = source.rasterize(20, 20).unwrap();
        let again = source.rasterize(20, 20).unwrap();
        assert!(
            Arc::ptr_eq(&first.rgba, &again.rgba),
            "same scale must reuse texture identity"
        );
        let second = source.rasterize(40, 40).unwrap();
        assert!(!Arc::ptr_eq(&first.rgba, &second.rgba));
        assert_eq!((second.width, second.height), (40, 40));
        let different = decode_svg(svg).unwrap();
        let revised = different
            .raster_source()
            .unwrap()
            .rasterize(20, 20)
            .unwrap();
        assert!(
            !Arc::ptr_eq(&first.rgba, &revised.rgba),
            "new asset revision must own new pixels"
        );
        assert!(
            source.rasterize(4096, 4096).is_err(),
            "bounded cache cannot allocate arbitrary device extents"
        );
        let root = block(20., 20.).fill(scene::Fill::Vector(vector.clone(), scene::Fit::Fill));
        let scene =
            scene::resolve(&scene::SceneSpec::new(root).offered(scene::Size::new(20., 20.)))
                .unwrap();
        let mut cache = vello::Cache::default();
        for scale in [1u16, 2] {
            let w = 20 * scale;
            let mut ctx = RenderContext::new(w, w);
            let mut resources = Resources::default();
            vello::paint(
                &mut vello::Cpu {
                    ctx: &mut ctx,
                    resources: &mut resources,
                    cache: &mut cache,
                },
                &scene,
                Affine::scale(scale as f64),
            )
            .unwrap();
            ctx.flush();
            let mut pixels = Pixmap::new(w, w);
            ctx.render(&mut pixels, &mut resources);
            let center = pixels.data()[10 * scale as usize * w as usize + 10 * scale as usize];
            let glow = pixels.data()[9 * scale as usize * w as usize + 5 * scale as usize];
            assert!(
                center.r > 245 && center.g < 5 && center.a > 245,
                "source must survive graph at {scale}x: {center:?}"
            );
            assert!(
                glow.a > 0
                    && glow.a < 128
                    && glow.r.abs_diff(glow.g) <= 1
                    && glow.g.abs_diff(glow.b) <= 1,
                "faithful white translucent outer glow at {scale}x: {glow:?}"
            );
        }
        assert!(Asset::Vector(vector).retained_bytes() >= FILTER_CACHE_LIMIT);
        assert!(
            Assets::prepare_bounded(Path::new("/does-not-exist.nki"), ["assets/a.svg"], 0).entries
                ["assets/a.svg"]
                .as_ref()
                .unwrap_err()
                .contains("budget")
        );
    }
    #[test]
    fn native_svg_compound_and_nested_clips_intersect_at_device_scale() {
        use moose::mui::mui::{
            scene::{self, prelude::*},
            vello::{
                self,
                vello_cpu::{Pixmap, RenderContext, Resources},
            },
        };
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="12"><defs><clipPath id="columns"><rect x="2" y="1" width="6" height="10"/><rect x="12" y="1" width="6" height="10"/></clipPath><clipPath id="top"><rect width="20" height="6"/></clipPath></defs><g transform="translate(1 0)" clip-path="url(#columns)"><g clip-path="url(#top)"><rect width="20" height="12" fill="#ff0000"/></g></g></svg>"##;
        let vector = Arc::new(decode_svg(svg).unwrap());
        let root = block(20., 12.).fill(scene::Fill::Vector(vector, scene::Fit::Fill));
        let scene =
            scene::resolve(&scene::SceneSpec::new(root).offered(scene::Size::new(20., 12.)))
                .unwrap();
        for scale in [1u16, 2] {
            let (w, h) = (20 * scale, 12 * scale);
            let mut ctx = RenderContext::new(w, h);
            let mut resources = Resources::default();
            vello::paint(
                &mut vello::Cpu {
                    ctx: &mut ctx,
                    resources: &mut resources,
                    cache: &mut vello::Cache::default(),
                },
                &scene,
                Affine::scale(scale as f64),
            )
            .unwrap();
            ctx.flush();
            let mut pixels = Pixmap::new(w, h);
            ctx.render(&mut pixels, &mut resources);
            let at = |x: usize, y: usize| {
                pixels.data()[y * scale as usize * w as usize + x * scale as usize]
            };
            for (x, y, expected) in [(4, 3, 255), (16, 3, 255), (10, 3, 0), (4, 9, 0), (0, 3, 0)] {
                assert_eq!(
                    at(x, y).a,
                    expected,
                    "clip union/intersection/transform at ({x},{y}) {scale}x"
                );
            }
        }
    }
}
