//! Native UI assets decoded only by the existing preparation worker.
//! SVG paths stay retained vectors; bitmap assets retain their source pixels.
use moose::mui::mui::scene::{Image, Vector, VectorCommand};
use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
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
    /// Called only by the existing serialized asset preparation worker.
    pub fn prepare_pending_rasters(&self) -> Result<bool, String> {
        match self {
            Self::Vector(v) => v
                .raster_source()
                .map_or(Ok(false), |s| s.prepare_requested()),
            Self::Bitmap(_) => Ok(false),
        }
    }
    pub fn set_raster_waker(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        if let Self::Vector(v) = self {
            if let Some(s) = v.raster_source() {
                s.set_waker(wake);
            }
        }
    }
    /// Poll at the existing editor/watch boundary as well as on a paint miss,
    /// so a full host job queue can retry without continuous repainting.
    pub fn wake_pending_rasters(&self) -> bool {
        match self {
            Self::Vector(v) => v.raster_source().is_some_and(|s| s.wake_pending()),
            Self::Bitmap(_) => false,
        }
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

/// Bound compact <use> expansion before usvg allocates normalized nodes.
/// roxmltree supplies the XML topology; this is a resource-limit preflight,
/// never a second SVG renderer/parser. Memoization keeps doubling graphs cheap.
fn svg_expansion_budget(xml: &roxmltree::Document<'_>, source_bytes: usize) -> Result<(), String> {
    fn count<'a, 'input>(
        node: roxmltree::Node<'a, 'input>,
        ids: &HashMap<String, roxmltree::Node<'a, 'input>>,
        memo: &mut HashMap<roxmltree::NodeId, usize>,
        active: &mut HashSet<roxmltree::NodeId>,
        depth: usize,
        max: usize,
    ) -> Result<usize, String> {
        if let Some(n) = memo.get(&node.id()) {
            return Ok(*n);
        }
        if depth > 512 || !active.insert(node.id()) {
            return Err("SVG use expansion is cyclic or too deeply nested".into());
        }
        let mut n = 1usize;
        for child in node.children().filter(|n| n.is_element()) {
            n = n.saturating_add(count(child, ids, memo, active, depth + 1, max)?);
        }
        if node.tag_name().name() == "use" {
            if let Some(href) = node
                .attribute("href")
                .or_else(|| node.attribute(("http://www.w3.org/1999/xlink", "href")))
            {
                let id = href
                    .strip_prefix('#')
                    .ok_or("External SVG use references are not supported")?;
                let target = *ids.get(id).ok_or("SVG use target was not found")?;
                n = n.saturating_add(count(target, ids, memo, active, depth + 1, max)?);
            }
        }
        active.remove(&node.id());
        if n > max {
            return Err("SVG use expansion exceeds preparation memory budget".into());
        }
        memo.insert(node.id(), n);
        Ok(n)
    }
    // Original attributes/path preprocessing plus a conservative allowance for
    // each expanded normalized node/box. Retained storage is counted separately.
    let source = source_bytes.saturating_mul(64);
    let remaining = PIXEL_LIMIT
        .checked_sub(source)
        .ok_or("SVG source exceeds preparation memory budget")?;
    let max = remaining / 2048;
    let mut ids = HashMap::new();
    for n in xml.descendants().filter(|n| n.is_element()) {
        if let Some(id) = n.attribute("id") {
            ids.entry(id.to_owned()).or_insert(n);
        }
    }
    count(
        xml.root_element(),
        &ids,
        &mut HashMap::new(),
        &mut HashSet::new(),
        0,
        max,
    )?;
    Ok(())
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
                | "feImage"
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
    svg_expansion_budget(&xml, bytes.len())?;
    let filtered = xml
        .descendants()
        .any(|n| n.is_element() && n.tag_name().name() == "filter");
    if filtered
        && bytes
            .len()
            .saturating_mul(65)
            .saturating_add(FILTER_CACHE_LIMIT)
            > PIXEL_LIMIT
    {
        return Err("Filtered SVG exceeds normalized source memory budget".into());
    }
    let mut options = usvg::Options::default();
    // No side-channel filesystem/network reads from SVG image hrefs.
    options.image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: Box::new(|_, _, _| None),
        resolve_string: Box::new(|_, _| None),
    };
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|e| e.to_string())?;
    if filtered {
        let size = tree.size();
        let retained = std::mem::size_of::<usvg::Tree>()
            .saturating_add(normalized_tree_bytes(tree.root(), bytes.len()))
            .saturating_add(bytes.len())
            .saturating_add(FILTER_CACHE_LIMIT);
        if retained > PIXEL_LIMIT {
            return Err("Filtered SVG normalized tree exceeds retained memory budget".into());
        }
        return Vector::filtered(
            size.width() as f64,
            size.height() as f64,
            Arc::new(FilteredSvg {
                tree,
                source: bytes.into(),
                retained,
                revision: std::sync::atomic::AtomicU64::new(0),
                retry: std::sync::atomic::AtomicBool::new(false),
                wake: OnceLock::new(),
                state: Mutex::new(RasterState {
                    images: Vec::with_capacity(2),
                    pending: Vec::with_capacity(2),
                    errors: Vec::with_capacity(2),
                    ..RasterState::default()
                }),
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
/// Count normalized nodes, not source length: <use> may expand a compact
/// document into many retained boxes. Repeated shared paint/path buffers are
/// charged conservatively, including spare Vec capacity and reference storage.
fn normalized_tree_bytes(group: &usvg::Group, source_bytes: usize) -> usize {
    use std::mem::size_of;
    fn paint(p: &usvg::Paint) -> usize {
        fn gradient(g: &usvg::BaseGradient) -> usize {
            g.id()
                .len()
                .saturating_mul(2)
                .saturating_add(g.stops().len().saturating_mul(2 * size_of::<usvg::Stop>()))
        }
        match p {
            usvg::Paint::Color(_) => 0,
            usvg::Paint::LinearGradient(g) => {
                size_of::<usvg::LinearGradient>().saturating_add(gradient(g))
            }
            usvg::Paint::RadialGradient(g) => {
                size_of::<usvg::RadialGradient>().saturating_add(gradient(g))
            }
            usvg::Paint::Pattern(_) => usize::MAX,
        }
    }
    let mut bytes = size_of::<usvg::Group>()
        .saturating_add(group.id().len().saturating_mul(2))
        .saturating_add(
            group
                .children()
                .len()
                .saturating_mul(2)
                .max(4)
                .saturating_mul(size_of::<usvg::Node>()),
        )
        .saturating_add(
            group
                .filters()
                .len()
                .saturating_mul(2 * size_of::<Arc<usvg::filter::Filter>>()),
        );
    for filter in group.filters() {
        // Primitive structs plus conservative source-backed tables/input/result
        // strings. These remain shared references; charging every use is safe.
        bytes = bytes
            .saturating_add(size_of::<usvg::filter::Filter>())
            .saturating_add(
                filter
                    .primitives()
                    .len()
                    .saturating_mul(2 * size_of::<usvg::filter::Primitive>()),
            )
            .saturating_add(source_bytes.saturating_mul(16));
    }
    for node in group.children() {
        let owned = match node {
            usvg::Node::Group(g) => normalized_tree_bytes(g, source_bytes),
            usvg::Node::Path(p) => {
                let mut n = size_of::<usvg::Path>() + p.id().len() * 2;
                n = n
                    .saturating_add(std::mem::size_of_val(p.data().points()).saturating_mul(2))
                    .saturating_add(std::mem::size_of_val(p.data().verbs()).saturating_mul(2));
                if let Some(fill) = p.fill() {
                    n = n.saturating_add(paint(fill.paint()));
                }
                if let Some(stroke) = p.stroke() {
                    n = n.saturating_add(paint(stroke.paint())).saturating_add(
                        stroke
                            .dasharray()
                            .map_or(0, |d| d.len().saturating_mul(2 * size_of::<f32>())),
                    );
                }
                n
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) => usize::MAX,
        };
        bytes = bytes.saturating_add(owned);
        node.subroots(|root| {
            bytes = bytes.saturating_add(normalized_tree_bytes(root, source_bytes));
        });
    }
    bytes
}
// Effects require compositing. Preserve the normalized vector tree and source;
// only the exact requested device-sized intermediate is rasterized. The source
// belongs to this instrument's asset cache, with no global cache or worker.
const FILTER_CACHE_LIMIT: usize = 8 << 20;
#[derive(Default)]
struct RasterState {
    images: Vec<Image>,
    pending: Vec<(u32, u32)>,
    inflight: Option<(u32, u32)>,
    errors: Vec<((u32, u32), String)>,
}
impl std::fmt::Debug for RasterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RasterState")
            .field("images", &self.images.len())
            .field("pending", &self.pending)
            .field("inflight", &self.inflight)
            .finish_non_exhaustive()
    }
}
struct FilteredSvg {
    tree: usvg::Tree,
    source: Arc<[u8]>,
    retained: usize,
    revision: std::sync::atomic::AtomicU64,
    retry: std::sync::atomic::AtomicBool,
    wake: OnceLock<Arc<dyn Fn() + Send + Sync>>,
    state: Mutex<RasterState>,
}
impl std::fmt::Debug for FilteredSvg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilteredSvg")
            .field("size", &self.tree.size())
            .field("source_bytes", &self.source.len())
            .field("retained", &self.retained)
            .finish_non_exhaustive()
    }
}
impl FilteredSvg {
    fn charge(width: u32, height: u32) -> Result<usize, String> {
        let charge = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("SVG filter extent overflow")?;
        if charge == 0 || charge > FILTER_CACHE_LIMIT {
            return Err(
                "SVG filter requested device extent exceeds 8 MiB intermediate budget".into(),
            );
        }
        Ok(charge)
    }
    // Worker-only: do not hold the cache lock while resvg executes.
    fn render(&self, width: u32, height: u32) -> Result<Image, String> {
        Self::charge(width, height)?;
        let sx = width as f64 / self.tree.size().width() as f64;
        let sy = height as f64 / self.tree.size().height() as f64;
        fn temporary_bytes(group: &usvg::Group, scale: Affine) -> usize {
            let mut total = 0usize;
            for filter in group.filters() {
                let r = filter.rect();
                let bounds = (scale * vello_svg::util::to_affine(&group.abs_transform()))
                    .transform_rect_bbox(vello_svg::kurbo::Rect::new(
                        r.x() as f64,
                        r.y() as f64,
                        r.right() as f64,
                        r.bottom() as f64,
                    ));
                let area = bounds.width().ceil() * bounds.height().ceil();
                if !area.is_finite() || area < 0. || area > PIXEL_LIMIT as f64 {
                    return usize::MAX;
                }
                // Allow every named primitive result plus source/work pixels.
                total = total.saturating_add(
                    (area as usize)
                        .saturating_mul(4)
                        .saturating_mul(filter.primitives().len().saturating_add(2)),
                );
            }
            for node in group.children() {
                if let usvg::Node::Group(g) = node {
                    total = total.saturating_add(temporary_bytes(g, scale));
                }
            }
            total
        }
        if temporary_bytes(self.tree.root(), Affine::scale_non_uniform(sx, sy)) > PIXEL_LIMIT {
            return Err("SVG filter graph exceeds temporary pixel memory budget".into());
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
        Ok(image)
    }
}
impl moose::mui::mui::scene::RasterSource for FilteredSvg {
    fn retained_bytes(&self) -> usize {
        self.retained.max(self.source.len())
    }
    fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }
    fn prepared_image(&self, width: u32, height: u32) -> Result<Option<Image>, String> {
        Self::charge(width, height)?;
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err("SVG filter cache unavailable".into());
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                self.retry.store(true, std::sync::atomic::Ordering::Release);
                self.revision
                    .fetch_add(1, std::sync::atomic::Ordering::Release);
                if let Some(wake) = self.wake.get() {
                    wake();
                }
                return Ok(None);
            }
        };
        if let Some(image) = state
            .images
            .iter()
            .find(|i| i.width == width && i.height == height)
        {
            return Ok(Some(image.clone()));
        }
        let size = (width, height);
        if let Some((_, error)) = state.errors.iter().find(|(s, _)| *s == size) {
            return Err(error.clone());
        }
        if state.inflight != Some(size) && !state.pending.contains(&size) {
            if state.pending.len() == 2 {
                state.pending.remove(0);
            }
            state.pending.push(size);
        }
        let pending = !state.pending.is_empty();
        drop(state);
        if pending {
            if let Some(wake) = self.wake.get() {
                wake();
            }
        }
        Ok(None)
    }
    fn set_waker(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.wake.set(wake);
    }
    fn wake_pending(&self) -> bool {
        let pending = self.retry.load(std::sync::atomic::Ordering::Acquire)
            || self.state.try_lock().is_ok_and(|s| !s.pending.is_empty());
        if pending {
            if let Some(wake) = self.wake.get() {
                wake();
            }
        }
        pending
    }
    fn prepare_requested(&self) -> Result<bool, String> {
        let retry = self.retry.swap(false, std::sync::atomic::Ordering::AcqRel);
        let mut changed = retry;
        let mut failure = None;
        for _ in 0..2 {
            let size = {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| "SVG filter cache unavailable")?;
                if state.pending.is_empty() {
                    break;
                }
                let size = state.pending.remove(0);
                state.inflight = Some(size);
                size
            };
            let result = self.render(size.0, size.1);
            let mut state = self
                .state
                .lock()
                .map_err(|_| "SVG filter cache unavailable")?;
            state.inflight = None;
            match result {
                Ok(image) => {
                    let charge = image.rgba.len();
                    while !state.images.is_empty()
                        && (state.images.len() >= 2
                            || state.images.iter().map(|i| i.rgba.len()).sum::<usize>() + charge
                                > FILTER_CACHE_LIMIT)
                    {
                        state.images.remove(0);
                    }
                    state.images.push(image);
                }
                Err(error) => {
                    if state.errors.len() == 2 {
                        state.errors.remove(0);
                    }
                    state.errors.push((size, error.clone()));
                    failure = Some(error);
                }
            }
            drop(state);
            self.revision
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            changed = true;
        }
        if retry {
            self.revision
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        if let Some(error) = failure {
            Err(error)
        } else {
            Ok(changed)
        }
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
    fn native_svg_charges_normalized_use_expansion() {
        let mut svg = String::from(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><filter id="f"><feGaussianBlur stdDeviation=".5"/></filter><g id="p0"><rect width="2" height="2"/></g>"##,
        );
        for depth in 1..=12 {
            svg.push_str(&format!(
                r##"<g id="p{depth}"><use href="#p{}"/><use href="#p{}"/></g>"##,
                depth - 1,
                depth - 1
            ));
        }
        svg.push_str(r##"</defs><use href="#p12" filter="url(#f)"/></svg>"##);
        let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).unwrap();
        let normalized = normalized_tree_bytes(tree.root(), svg.len());
        assert!(
            normalized > svg.len() * 65,
            "compact use expansion must be charged by actual nodes: {normalized} vs {}",
            svg.len() * 65
        );
        let asset = Asset::Vector(Arc::new(decode_svg(svg.as_bytes()).unwrap()));
        assert!(asset.retained_bytes() >= normalized + FILTER_CACHE_LIMIT);
        let folder = std::env::temp_dir().join(format!(
            "kontra-svg-budget-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let resources = folder.join("Resources/native_ui");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("expanded.svg"), &svg).unwrap();
        let instrument = folder.join("case.nki");
        let budget = svg.len() * 65 + FILTER_CACHE_LIMIT;
        let bounded = Assets::prepare_bounded(&instrument, ["expanded.svg"], budget);
        std::fs::remove_dir_all(&folder).unwrap();
        assert!(
            bounded.entries["expanded.svg"]
                .as_ref()
                .unwrap_err()
                .contains("retained budget")
        );
        let deep = svg.replace(r#"p12" filter"#, r#"p19" filter"#);
        let extra = (13..=19)
            .map(|d| {
                format!(
                    r##"<g id="p{d}"><use href="#p{}"/><use href="#p{}"/></g>"##,
                    d - 1,
                    d - 1
                )
            })
            .collect::<String>();
        let deep = deep.replace("</defs>", &(extra + "</defs>"));
        assert!(
            decode_svg(deep.as_bytes())
                .unwrap_err()
                .contains("expansion"),
            "compact large expansion must be rejected before usvg"
        );
    }
    #[test]
    fn native_svg_ready_cache_contention_retries_retained_paint() {
        use moose::mui::mui::scene::RasterSource;
        use moose::mui::mui::scene::{self, prelude::*};
        let bytes = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><filter id="f"><feGaussianBlur stdDeviation="1"/></filter></defs><rect x="5" y="5" width="10" height="10" fill="#f00" filter="url(#f)"/></svg>"##;
        let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).unwrap();
        let filter = Arc::new(FilteredSvg {
            retained: normalized_tree_bytes(tree.root(), bytes.len())
                + bytes.len()
                + FILTER_CACHE_LIMIT,
            tree,
            source: bytes.as_slice().into(),
            revision: std::sync::atomic::AtomicU64::new(0),
            retry: std::sync::atomic::AtomicBool::new(false),
            wake: OnceLock::new(),
            state: Mutex::new(RasterState::default()),
        });
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wake_count = wakes.clone();
        filter.set_waker(Arc::new(move || {
            wake_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));
        assert!(filter.prepared_image(20, 20).unwrap().is_none());
        assert!(filter.prepare_requested().unwrap());
        let first = filter.prepared_image(20, 20).unwrap().unwrap();
        let vector = Arc::new(Vector::filtered(20., 20., filter.clone()).unwrap());
        let spec = scene::SceneSpec::new(
            block(20., 20.).fill(scene::Fill::Vector(vector, scene::Fit::Fill)),
        )
        .offered(scene::Size::new(20., 20.));
        let before = scene::resolve(&spec).unwrap();
        let guard = filter.state.lock().unwrap();
        let wake_before = wakes.load(std::sync::atomic::Ordering::Relaxed);
        // Controlled contention: a blocking read/cold raster would deadlock on
        // this thread. The pending frame captures the previous Paint revision.
        assert!(filter.prepared_image(20, 20).unwrap().is_none());
        assert!(filter.revision() > 1);
        assert!(wakes.load(std::sync::atomic::Ordering::Relaxed) > wake_before);
        drop(guard);
        assert!(
            filter.prepare_requested().unwrap(),
            "empty worker retry must publish readiness"
        );
        let ready = filter.prepared_image(20, 20).unwrap().unwrap();
        assert!(
            Arc::ptr_eq(&first.rgba, &ready.rgba),
            "retry must reuse prepared pixels without rerasterizing"
        );
        let after = scene::resolve(&spec).unwrap();
        assert_ne!(
            before.paint, after.paint,
            "retained GPU encoding must invalidate same Vector Arc after a busy miss"
        );
        assert!(!filter.wake_pending());
        assert!(
            !filter.prepare_requested().unwrap(),
            "completed retry must not make an idle worker loop"
        );
    }
    #[test]
    fn native_png_bounds_compressed_metadata_before_pixel_decode() {
        let mut bytes = Vec::new();
        let mut info = png::Info::with_size(1, 1);
        info.color_type = png::ColorType::Rgba;
        info.bit_depth = png::BitDepth::Eight;
        info.icc_profile = Some(std::borrow::Cow::Owned(vec![0; 64 << 10]));
        {
            let mut writer = png::Encoder::with_info(&mut bytes, info)
                .unwrap()
                .write_header()
                .unwrap();
            writer.write_image_data(&[255, 0, 0, 255]).unwrap();
            writer.finish().unwrap();
        }
        assert!(
            bytes.len() < 4096,
            "authored metadata must be highly compressed"
        );
        let unlimited = crate::artwork::decode_report(&bytes).unwrap();
        assert_eq!((unlimited.width, unlimited.height), (1, 1));
        let error = crate::artwork::decode_report_bounded(&bytes, 1024).unwrap_err();
        assert!(
            error.to_ascii_lowercase().contains("limit"),
            "metadata must hit decoder bound before RGBA allocation: {error}"
        );
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
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wake_count = wakes.clone();
        source.set_waker(Arc::new(move || {
            wake_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));
        assert!(
            source.prepared_image(20, 20).unwrap().is_none(),
            "cold paint queues work without rasterizing"
        );
        assert!(
            source.wake_pending(),
            "pending jobs must retry a full host queue"
        );
        assert!(wakes.load(std::sync::atomic::Ordering::Relaxed) >= 2);
        assert!(source.prepare_requested().unwrap());
        assert!(!source.wake_pending());
        let first = source.prepared_image(20, 20).unwrap().unwrap();
        let again = source.prepared_image(20, 20).unwrap().unwrap();
        assert!(
            !source.prepare_requested().unwrap(),
            "cache hit queues no work"
        );
        assert!(
            Arc::ptr_eq(&first.rgba, &again.rgba),
            "same scale must reuse texture identity"
        );
        assert!(source.prepared_image(40, 40).unwrap().is_none());
        assert!(source.prepare_requested().unwrap());
        let second = source.prepared_image(40, 40).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&first.rgba, &second.rgba));
        assert_eq!((second.width, second.height), (40, 40));
        let different = decode_svg(svg).unwrap();
        let different_source = different.raster_source().unwrap();
        assert!(different_source.prepared_image(20, 20).unwrap().is_none());
        assert!(different_source.prepare_requested().unwrap());
        let revised = different_source.prepared_image(20, 20).unwrap().unwrap();
        assert!(
            !Arc::ptr_eq(&first.rgba, &revised.rgba),
            "new asset revision must own new pixels"
        );
        assert!(
            source.prepared_image(4096, 4096).is_err(),
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
            Assets::prepare_bounded(
                std::path::Path::new("/does-not-exist.nki"),
                ["assets/a.svg"],
                0
            )
            .entries["assets/a.svg"]
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
