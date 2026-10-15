//! One cancelable preparation worker per Face; byte-bounded, sparse frame LRU.
use super::{
    ir_view::{Picture, Values},
    pictures::Source,
};
use crate::support::MutexExt;
use moose::mui::mui::prelude::Font;
use sampler_ui_ir as ir;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

#[cfg(all(test, feature = "shots"))]
#[path = "picture_worker/retention_tests.rs"]
mod retention_tests;

const BUDGET: usize = 64 << 20;
static REVISION: AtomicU64 = AtomicU64::new(0);
// Bound aggregate transient decoder memory across rack instances.
static DECODE: OnceLock<Mutex<()>> = OnceLock::new();
pub(super) fn completed() {
    REVISION.fetch_add(1, Ordering::Release);
}
pub(super) fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct Key {
    asset: usize,
    identity: Arc<str>,
    frame: usize,
    target: [u32; 2],
    window: Option<[u32; 4]>,
    plane: Plane,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Plane {
    Frame,
    Wallpaper,
}

/// Wallpaper skin offsets address the full source canvas. Control pictures
/// keep their ordinary per-frame geometry, including when they share this file.
fn wallpaper_geometry(meta: ir::ImageMeta, frame: usize) -> Option<(Option<[u32; 2]>, [u32; 2])> {
    let Some(size) = meta.size else {
        return (frame == 0).then_some((None, [0, 0]));
    };
    let pixels = |value: f64| {
        (value.is_finite() && value > 0. && value.ceil() <= f64::from(u32::MAX))
            .then(|| value.ceil() as u32)
    };
    let mut extent = [pixels(size.width)?, pixels(size.height)?];
    let axis = usize::from(meta.axis == ir::Orientation::Vertical);
    let frames = meta.frames.max(1);
    let mut origin = [0, 0];
    origin[axis] = extent[axis].checked_mul(frame.min(frames as usize - 1) as u32)?;
    extent[axis] = extent[axis].checked_mul(frames)?;
    Some((Some(extent), origin))
}
#[derive(Clone)]
struct Request {
    key: Key,
    asset: ir::Asset,
    epoch: u64,
    #[cfg(feature = "shots")]
    queued: std::time::Instant,
}

impl Request {
    fn decode_spec(&self) -> Option<(ir::Asset, usize, Option<[u32; 4]>)> {
        if self.key.plane == Plane::Frame {
            return Some((self.asset.clone(), self.key.frame, self.key.window));
        }
        let ir::AssetKind::Image(mut meta) = self.asset.kind else {
            return None;
        };
        let (extent, origin) = wallpaper_geometry(meta, self.key.frame)?;
        let [x, y, w, h] = self.key.window?;
        let window = [x.checked_add(origin[0])?, y.checked_add(origin[1])?, w, h];
        if w == 0
            || h == 0
            || extent.is_some_and(|[width, height]| {
                window[0].checked_add(w).is_none_or(|right| right > width)
                    || window[1]
                        .checked_add(h)
                        .is_none_or(|bottom| bottom > height)
            })
        {
            return None;
        }
        // Decode only the viewport, never a complete wallpaper atlas. This
        // private copy does not change the face's authored sprite metadata.
        meta.frames = 1;
        meta.size = extent.map(|[width, height]| ir::Size {
            width: f64::from(width),
            height: f64::from(height),
        });
        let mut asset = self.asset.clone();
        asset.kind = ir::AssetKind::Image(meta);
        Some((asset, 0, Some(window)))
    }
}
struct Result {
    key: Key,
    picture: Option<Arc<Picture>>,
    font: Option<Font>,
    epoch: u64,
    bytes: usize,
    #[cfg(feature = "shots")]
    scan: super::pictures::Scan,
    #[cfg(feature = "shots")]
    probes: Vec<super::pictures::AssetProbe>,
}
struct Cached {
    picture: Option<Arc<Picture>>,
    font: Option<Font>,
    bytes: usize,
    tick: u64,
    limited: bool,
}
pub(super) struct Preparation {
    path: PathBuf,
    program: Option<u32>,
    jobs: SyncSender<Vec<Request>>,
    results: Receiver<Result>,
    epoch: Arc<AtomicU64>,
    cache: HashMap<Key, Cached>,
    pending: HashSet<Key>,
    wanted: Vec<Key>,
    assets: Vec<ir::Asset>,
    /// `path:kind` per asset, built once per asset list, not per frame.
    identities: Vec<Arc<str>>,
    presentation: ir::Presentation,
    /// The last `prepare` found nothing new: same wanted set, no new result.
    unchanged: bool,
    tick: u64,
    bytes: usize,
    #[cfg(feature = "shots")]
    pub scan: super::pictures::Scan,
    #[cfg(feature = "shots")]
    decoded_bytes: HashMap<Key, usize>,
    #[cfg(feature = "shots")]
    probes: Vec<super::pictures::AssetProbe>,
    #[cfg(feature = "shots")]
    probe_dropped: usize,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::Release);
    }
}
impl Preparation {
    #[cfg(test)]
    pub(super) fn worker_identity(&self) -> usize {
        Arc::as_ptr(&self.epoch) as usize
    }
    pub fn new(path: &Path) -> Self {
        Self::new_owned(path, None)
    }
    pub fn new_for_program(path: &Path, program: u32) -> Self {
        Self::new_owned(path, Some(program))
    }
    fn new_owned(path: &Path, program: Option<u32>) -> Self {
        let (jobs, requests) = mpsc::sync_channel::<Vec<Request>>(1);
        let (done, results) = mpsc::sync_channel(1);
        let epoch = Arc::new(AtomicU64::new(0));
        let worker_epoch = epoch.clone();
        let worker_path = path.to_path_buf();
        let _ = thread::Builder::new()
            .name("authored-art".into())
            .spawn(move || {
                // Discovery and all encoded reads happen here, never in Face::new.
                let mut source = program.map_or_else(
                    || Source::of(&worker_path),
                    |program| {
                        Source::for_program(&worker_path, program).unwrap_or_else(Source::failed)
                    },
                );
                while let Ok(batch) = requests.recv() {
                    for request in batch {
                        let canceled = || worker_epoch.load(Ordering::Acquire) != request.epoch;
                        if canceled() {
                            continue;
                        }
                        #[cfg(feature = "shots")]
                        let queue_ms = request.queued.elapsed().as_secs_f64() * 1000.;
                        #[cfg(feature = "shots")]
                        let permit_start = std::time::Instant::now();
                        let _permit = DECODE.get_or_init(|| Mutex::new(())).lock_unpoisoned();
                        #[cfg(feature = "shots")]
                        let permit_wait_ms = permit_start.elapsed().as_secs_f64() * 1000.;
                        let (picture, font) = if request.asset.kind == ir::AssetKind::TrueTypeFont {
                            (None, source.font(&request.asset))
                        } else {
                            let picture =
                                request.decode_spec().and_then(|(asset, frame, window)| {
                                    source.load_frame(
                                        &asset,
                                        frame,
                                        request.key.target,
                                        window,
                                        canceled,
                                    )
                                });
                            let picture = picture.map(|picture| {
                                if request.key.plane == Plane::Wallpaper {
                                    Arc::new(picture.with_window(request.key.window))
                                } else {
                                    picture
                                }
                            });
                            (picture, None)
                        };
                        drop(_permit);
                        if canceled() {
                            #[cfg(feature = "shots")]
                            {
                                source.take_probes();
                            }
                            continue;
                        }
                        let bytes = font.as_ref().map_or(0, |f| f.as_ref().len())
                            + picture
                                .as_ref()
                                .map_or(0, |p| p.frames.iter().map(|i| i.rgba.len()).sum());
                        if done
                            .send(Result {
                                key: request.key,
                                picture,
                                font,
                                epoch: request.epoch,
                                bytes,
                                #[cfg(feature = "shots")]
                                scan: source.scan,
                                #[cfg(feature = "shots")]
                                probes: {
                                    let mut probes = source.take_probes();
                                    for probe in &mut probes {
                                        probe.queue_ms = Some(queue_ms);
                                        probe.permit_wait_ms = Some(permit_wait_ms);
                                    }
                                    probes
                                },
                            })
                            .is_err()
                        {
                            return;
                        }
                        REVISION.fetch_add(1, Ordering::Release);
                    }
                }
            });
        Self {
            path: path.into(),
            program,
            jobs,
            results,
            epoch,
            cache: HashMap::new(),
            pending: HashSet::new(),
            wanted: Vec::new(),
            assets: Vec::new(),
            identities: Vec::new(),
            presentation: Default::default(),
            unchanged: false,
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            decoded_bytes: HashMap::new(),
            #[cfg(feature = "shots")]
            probes: Vec::new(),
            #[cfg(feature = "shots")]
            probe_dropped: 0,
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn program(&self) -> Option<u32> {
        self.program
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
    #[cfg(feature = "shots")]
    pub fn probes(&self) -> &[super::pictures::AssetProbe] {
        &self.probes
    }
    #[cfg(feature = "shots")]
    pub fn probe_dropped(&self) -> usize {
        self.probe_dropped
    }
    /// Whether the last `prepare` found nothing new (it then returned nothing,
    /// and what the caller took from the one before still stands).
    pub fn unchanged(&self) -> bool {
        self.unchanged
    }
    #[cfg(feature = "shots")]
    pub fn failures(&self) -> Vec<String> {
        self.wanted
            .iter()
            .filter(|key| {
                self.cache
                    .get(*key)
                    .is_some_and(|v| v.picture.is_none() && v.font.is_none())
            })
            .map(|key| blake3::hash(key.identity.as_bytes()).to_hex().to_string())
            .collect()
    }
    #[cfg(feature = "shots")]
    pub fn completed_key_bytes(&self) -> Vec<(String, usize)> {
        self.wanted
            .iter()
            .filter_map(|key| {
                self.decoded_bytes.get(key).map(|bytes| {
                    (
                        blake3::hash(format!("{key:?}").as_bytes())
                            .to_hex()
                            .to_string(),
                        *bytes,
                    )
                })
            })
            .collect()
    }
    pub fn prepare(
        &mut self,
        face: &ir::Interface,
        page: ir::PageRef,
        presentation: ir::Presentation,
        scale: f64,
        values: &Values,
    ) -> Vec<(usize, Option<Arc<Picture>>, Option<Font>, Plane)> {
        // The first call must publish even an empty owner. Later calls are
        // driven by actual input/result changes, not the previous return flag.
        let mut changed = self.tick == 0;
        let metadata_changed = self.assets != face.assets || self.presentation != presentation;
        if metadata_changed {
            let needed = face.needed_assets(presentation);
            self.cache.retain(|key, _| {
                needed.get(key.asset).copied().unwrap_or(false)
                    && face
                        .assets
                        .get(key.asset)
                        .is_some_and(|a| *key.identity == *format!("{}:{:?}", a.path, a.kind))
            });
            #[cfg(feature = "shots")]
            self.decoded_bytes
                .retain(|key, _| self.cache.contains_key(key));
            self.bytes = self.cache.values().map(|e| e.bytes).sum();
            // Identity keys keep unchanged assets reusable across a sparse publication.
            self.assets = face.assets.clone();
            self.identities = face
                .assets
                .iter()
                .map(|a| Arc::from(format!("{}:{:?}", a.path, a.kind)))
                .collect();
            self.presentation = presentation;
            changed = true;
        }
        self.tick = self.tick.wrapping_add(1);
        let mut wanted = Vec::new();
        let identities = &self.identities;
        let mut add = |asset: ir::AssetRef,
                       frame: usize,
                       target: [u32; 2],
                       window: Option<[u32; 4]>,
                       plane| {
            if let Some(a) = face.assets.get(asset.0) {
                wanted.push(Key {
                    asset: asset.0,
                    identity: identities[asset.0].clone(),
                    frame,
                    target,
                    window,
                    plane,
                });
            }
        };
        if let Some(p) = face.pages.get(page.0)
            && let Some(a) = p.background.image
        {
            let geometry = match face.assets[a.0].kind {
                ir::AssetKind::Image(meta) => wallpaper_geometry(meta, p.background.frame as usize),
                _ => None,
            };
            if let Some((extent, origin)) = geometry
                && let Some(y) = p
                    .background
                    .origin_y
                    .checked_add(p.background.offset_y.max(0) as u32)
                && let Some(source_y) = origin[1].checked_add(y)
            {
                let w = extent.map_or(p.size.width, |size| {
                    f64::from(size[0].saturating_sub(origin[0])).min(p.size.width)
                });
                let h = extent.map_or(p.size.height, |size| {
                    f64::from(size[1].saturating_sub(source_y)).min(p.size.height)
                });
                if w.is_finite()
                    && h.is_finite()
                    && w > 0.
                    && h > 0.
                    && w.ceil() <= f64::from(u32::MAX)
                    && h.ceil() <= f64::from(u32::MAX)
                {
                    add(
                        a,
                        p.background.frame as usize,
                        [w, h].map(|n| (n as f64 * scale.clamp(0.01, 1.)).ceil().max(1.) as u32),
                        Some([0, y, w.ceil() as u32, h.ceil() as u32]),
                        Plane::Wallpaper,
                    );
                }
            }
        }
        for n in face
            .draw_order(page)
            .into_iter()
            .filter(|&n| face.visible(n))
        {
            let w = &face.widgets[n.0];
            let value = match w.binding {
                ir::Binding::Control(c) => values.get(&c).copied().unwrap_or(w.initial_value),
                _ => w.initial_value,
            };
            for image in &w.images {
                if presentation == ir::Presentation::Vector
                    && image.role.replaced_by_vector()
                    && !w.label_in_image()
                {
                    continue;
                }
                let meta = match face.assets[image.asset.0].kind {
                    ir::AssetKind::Image(m) => m,
                    _ => continue,
                };
                let count = meta.frames.max(1) as usize;
                let frame = image
                    .frame
                    .map(|n| n as usize)
                    .unwrap_or_else(|| match &w.kind {
                        ir::Kind::Knob { range, .. } | ir::Kind::Slider { range, .. }
                            if image.role == ir::Role::Strip =>
                        {
                            let t = super::ir_view::mapped(range, w.mapper.as_deref(), value, true);
                            super::ir_view::frame(t, 0., 1., count)
                        }
                        ir::Kind::Button { .. } | ir::Kind::Switch
                            if image.role == ir::Role::Strip =>
                        {
                            usize::from(value > 0.5).min(count - 1)
                        }
                        _ => 0,
                    });
                // Keep authored corner pixels until the device-scale sliced painter lands.
                let size = meta
                    .size
                    .map_or([w.rect.width.max(1.), w.rect.height.max(1.)], |s| {
                        [s.width, s.height]
                    });
                let factor = if scale.is_finite() {
                    scale.clamp(0.01, 1.)
                } else {
                    1.
                };
                let target = size.map(|n| (n as f64 * factor).ceil().max(1.) as u32);
                if image.frame.is_none()
                    && image.role == ir::Role::Strip
                    && count >= 6
                    && matches!(w.kind, ir::Kind::Button { .. } | ir::Kind::Switch)
                    && matches!(
                        face.source,
                        ir::Source::Ksp { .. } | ir::Source::PerformanceView
                    )
                {
                    // The painter selects normal, held and hovered on/off
                    // pairs. These transient UI states are absent from Values;
                    // prepare the six authored frames rather than show a stale
                    // normal frame while hovered or pressed.
                    for (held, hovered) in [(false, false), (true, false), (false, true)] {
                        for on in [false, true] {
                            add(
                                image.asset,
                                ir::kontakt_picture_frame(on, held, hovered, count),
                                target,
                                None,
                                Plane::Frame,
                            );
                        }
                    }
                } else {
                    add(image.asset, frame, target, None, Plane::Frame);
                }
            }
            for s in w
                .style
                .into_iter()
                .chain(w.state_styles.into_iter().flatten())
                .filter_map(|s| face.styles.get(s.0))
            {
                match s.font {
                    ir::Font::Bitmap(a) if presentation == ir::Presentation::Bitmap => {
                        add(a, 0, [u32::MAX; 2], None, Plane::Frame)
                    }
                    ir::Font::File(a) => add(a, 0, [0; 2], None, Plane::Frame),
                    _ => {}
                }
            }
        }
        wanted.sort_by(|a, b| {
            (&a.identity, a.frame, a.target, a.window, a.plane, a.asset).cmp(&(
                &b.identity,
                b.frame,
                b.target,
                b.window,
                b.plane,
                b.asset,
            ))
        });
        wanted.dedup();
        if metadata_changed {
            // Keep a whole pending batch only while every exact request is
            // still required under this worker's resource authority. Otherwise
            // retain the conservative epoch fence, including when empty.
            let retain_pending = !self.pending.is_empty() && {
                let required: HashSet<_> = wanted.iter().collect();
                self.pending.iter().all(|key| required.contains(&key))
            };
            if !retain_pending {
                self.epoch.fetch_add(1, Ordering::Release);
                self.pending.clear();
            }
        }
        let epoch = self.epoch.load(Ordering::Acquire);
        if self.wanted != wanted {
            self.cache.retain(|_, entry| !entry.limited);
            changed = true;
        }
        self.wanted = wanted;
        while let Ok(mut result) = self.results.try_recv() {
            if result.epoch != epoch {
                continue;
            }
            changed = true;
            self.pending.remove(&result.key);
            #[cfg(feature = "shots")]
            {
                result.scan.preparation_completed = self.scan.preparation_completed + 1;
                result.scan.preparation_completed_bytes = self
                    .scan
                    .preparation_completed_bytes
                    .saturating_add(result.bytes);
                result.scan.preparation_max_key_bytes =
                    self.scan.preparation_max_key_bytes.max(result.bytes);
                result.scan.preparation_wanted_peak_bytes = self.scan.preparation_wanted_peak_bytes;
                result.scan.preparation_oversized = self.scan.preparation_oversized;
                result.scan.preparation_key_budget = self.scan.preparation_key_budget;
                result.scan.preparation_evicted = self.scan.preparation_evicted;
                result.scan.preparation_requeued = self.scan.preparation_requeued;
                self.scan = result.scan;
                self.decoded_bytes.insert(result.key.clone(), result.bytes);
            }
            if let Some(old) = self.cache.remove(&result.key) {
                self.bytes -= old.bytes;
            }
            let mut limited = result.bytes > BUDGET;
            if limited {
                #[cfg(feature = "shots")]
                {
                    self.scan.preparation_oversized += 1;
                }
            }
            while !limited && self.bytes.saturating_add(result.bytes) > BUDGET {
                let Some(key) = self
                    .cache
                    .iter()
                    .filter(|(key, value)| value.bytes > 0 && !self.wanted.contains(key))
                    .min_by_key(|(_, v)| v.tick)
                    .map(|(k, _)| k.clone())
                else {
                    limited = true;
                    #[cfg(feature = "shots")]
                    {
                        self.scan.preparation_key_budget += 1;
                    }
                    break;
                };
                if let Some(old) = self.cache.remove(&key) {
                    self.bytes -= old.bytes;
                    #[cfg(feature = "shots")]
                    {
                        self.scan.preparation_evicted += 1;
                    }
                }
            }
            // Failed admissions are terminal for this wanted set, not decode retries.
            if limited {
                result.picture = None;
                result.font = None;
                result.bytes = 0;
            }
            #[cfg(feature = "shots")]
            for mut probe in result.probes {
                probe.index = Some(result.key.asset);
                if limited {
                    probe.status = "preparation_limit";
                }
                if self.probes.len() < super::pictures::PROBE_LIMIT {
                    self.probes.push(probe);
                } else {
                    self.probe_dropped += 1;
                }
            }
            self.bytes += result.bytes;
            self.cache.insert(
                result.key,
                Cached {
                    picture: result.picture,
                    font: result.font,
                    bytes: result.bytes,
                    tick: self.tick,
                    limited,
                },
            );
        }
        #[cfg(feature = "shots")]
        {
            self.scan.preparation_wanted_peak_bytes = self.scan.preparation_wanted_peak_bytes.max(
                self.wanted
                    .iter()
                    .filter_map(|key| self.decoded_bytes.get(key))
                    .copied()
                    .sum(),
            );
        }
        let batch = self
            .wanted
            .iter()
            .filter(|k| !self.cache.contains_key(*k) && !self.pending.contains(*k))
            .map(|key| Request {
                key: key.clone(),
                asset: face.assets[key.asset].clone(),
                epoch,
                #[cfg(feature = "shots")]
                queued: std::time::Instant::now(),
            })
            .collect::<Vec<_>>();
        changed |= !batch.is_empty();
        self.unchanged = !changed;
        if !changed {
            return Vec::new();
        }
        if !batch.is_empty() && self.jobs.try_send(batch.clone()).is_ok() {
            #[cfg(feature = "shots")]
            {
                self.scan.preparation_requeued += batch
                    .iter()
                    .filter(|r| self.decoded_bytes.contains_key(&r.key))
                    .count();
            }
            self.pending.extend(batch.into_iter().map(|r| r.key));
        }
        // A frame still decoding shows the nearest decoded one of its strip, so
        // a turning knob never drops its picture (or lags a whole drag behind).
        let mut shown = Vec::with_capacity(self.wanted.len());
        for key in &self.wanted {
            let hit = if self.cache.contains_key(key) {
                Some(key.clone())
            } else {
                self.cache
                    .iter()
                    .filter(|(k, v)| {
                        v.picture.is_some()
                            && (k.asset, &k.identity, k.target, k.window, k.plane)
                                == (key.asset, &key.identity, key.target, key.window, key.plane)
                    })
                    .min_by_key(|(k, _)| k.frame.abs_diff(key.frame))
                    .map(|(k, _)| k.clone())
            };
            if let Some(entry) = hit.and_then(|k| self.cache.get_mut(&k)) {
                entry.tick = self.tick;
                shown.push((
                    key.asset,
                    entry.picture.clone(),
                    entry.font.clone(),
                    key.plane,
                ));
            }
        }
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wallpaper_fixture(directory: &Path, name: &str, blue: u8) {
        let mut bytes = Vec::new();
        let pixels: Vec<_> = (0..15u8)
            .flat_map(|row| [row * 10, 30, blue, 255].repeat(4))
            .collect();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 4, 15);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
        std::fs::write(directory.join(name), bytes).unwrap();
    }

    fn wallpaper_face() -> ir::Interface {
        ir::Interface {
            source: ir::Source::Ksp { slot: 0 },
            assets: vec![ir::Asset {
                path: "Resources/pictures/canvas.png".into(),
                kind: ir::AssetKind::Image(ir::ImageMeta {
                    frames: 3,
                    size: Some(ir::Size {
                        width: 4.,
                        height: 5.,
                    }),
                    ..Default::default()
                }),
            }],
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 4.,
                    height: 3.,
                },
                background: ir::Background {
                    image: Some(ir::AssetRef(0)),
                    origin_y: 2,
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn ready_wallpaper_assets(
        assets: &mut super::super::ir_view::Assets,
        path: &Path,
        face: &ir::Interface,
    ) {
        let start = std::time::Instant::now();
        loop {
            assets.prepare(
                path,
                face,
                ir::PageRef(0),
                ir::Presentation::Bitmap,
                1.,
                &Values::default(),
            );
            if assets.pending() == 0 {
                break;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(5),
                "authored wallpaper preparation timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn wallpaper_skin_offsets_crop_the_full_canvas_and_keep_paint_origin() {
        use moose::mui::mui::prelude::*;
        let directory = tempfile::tempdir().unwrap();
        let pictures = directory.path().join("Resources/pictures");
        std::fs::create_dir_all(&pictures).unwrap();
        wallpaper_fixture(&pictures, "canvas.png", 10);
        let path = directory.path().join("authored.nki");
        let mut face = wallpaper_face();
        let authored_assets = face.assets.clone();
        let mut assets = super::super::ir_view::Assets::default();
        for (offset, source_row, height) in
            [(0, 2, 3), (5, 7, 3), (10, 12, 3), (12, 14, 1), (-1, 2, 3)]
        {
            face.pages[0].background.offset_y = offset;
            ready_wallpaper_assets(&mut assets, &path, &face);
            let picture = assets.wallpaper(ir::AssetRef(0)).unwrap();
            assert_eq!(
                picture.window,
                Some([0, offset.max(0) as u32 + 2, 4, height])
            );
            assert_eq!(
                [picture.frames[0].width, picture.frames[0].height],
                [4, height]
            );
            let expected: Vec<_> = (source_row..source_row + height as u8)
                .flat_map(|row| [row * 10, 30, 10, 255].repeat(4))
                .collect();
            assert_eq!(&*picture.frames[0].rgba, &expected);
        }
        face.pages[0].background.offset_y = 5;
        ready_wallpaper_assets(&mut assets, &path, &face);
        let mut ui = super::super::theme::ui();
        for _ in 0..2 {
            let view = super::super::ir_view::view(
                &mut ui,
                &face,
                ir::PageRef(0),
                &assets,
                ir::Presentation::Bitmap,
                1.,
                &mut Values::default(),
            );
            ui.frame(view, Some(Size::new(4., 3.)), Input::default(), 1. / 60.)
                .unwrap();
        }
        let expected: Vec<_> = (7..10u8)
            .flat_map(|row| [row * 10, 30, 10, 255].repeat(4))
            .collect();
        assert_eq!(super::super::tests::pixels(&ui, 4, 3), expected);

        face.pages[0].background.offset_y = 0;
        face.pages[0].background.frame = 1;
        ready_wallpaper_assets(&mut assets, &path, &face);
        let picture = assets.wallpaper(ir::AssetRef(0)).unwrap();
        assert_eq!(picture.window, Some([0, 2, 4, 3]));
        assert_eq!(&picture.frames[0].rgba[..4], &[70, 30, 10, 255]);
        face.pages[0].background.frame = 0;
        face.pages[0].background.offset_y = 13;
        ready_wallpaper_assets(&mut assets, &path, &face);
        assert!(
            assets.wallpaper(ir::AssetRef(0)).is_none(),
            "outside the full canvas, no stale wallpaper is substituted"
        );
        assert_eq!(face.assets, authored_assets);
    }

    #[test]
    fn wallpaper_and_control_roles_share_file_without_colliding_and_reuse_each_window() {
        for wallpaper_first in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let pictures = directory.path().join("Resources/pictures");
            std::fs::create_dir_all(&pictures).unwrap();
            wallpaper_fixture(&pictures, "canvas.png", 10);
            wallpaper_fixture(&pictures, "revised.png", 90);
            let path = directory.path().join("authored.nki");
            let mut face = wallpaper_face();
            let mut control = ir::Widget::new(
                "$value",
                ir::PageRef(0),
                ir::Rect::new(0, 0, 4, 5),
                ir::Kind::Knob {
                    range: ir::Range {
                        min: 0.,
                        max: 2.,
                        default: 1.,
                        step: Some(1.),
                    },
                    display: Default::default(),
                },
            );
            control.initial_value = 1.;
            control
                .images
                .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
            if !wallpaper_first {
                face.pages[0].background.image = None;
                face.widgets.push(control.clone());
            }
            let mut assets = super::super::ir_view::Assets::default();
            ready_wallpaper_assets(&mut assets, &path, &face);
            let first = if wallpaper_first {
                assets.wallpaper(ir::AssetRef(0))
            } else {
                assets.get(ir::AssetRef(0))
            }
            .unwrap()
            .clone();
            face.pages[0].background.image = Some(ir::AssetRef(0));
            if wallpaper_first {
                face.widgets.push(control);
            }
            ready_wallpaper_assets(&mut assets, &path, &face);
            let wallpaper = assets.wallpaper(ir::AssetRef(0)).unwrap().clone();
            let control = assets.get(ir::AssetRef(0)).unwrap().clone();
            assert!(Arc::ptr_eq(
                &first,
                if wallpaper_first {
                    &wallpaper
                } else {
                    &control
                }
            ));
            assert_eq!(&wallpaper.frames[0].rgba[..4], &[20, 30, 10, 255]);
            assert_eq!(&control.frames[0].rgba[..4], &[50, 30, 10, 255]);
            assert_eq!(
                assets.bytes(),
                (4 * 3 + 4 * 5) * 4,
                "both prepared roles count against the same finite worker budget"
            );
            face.pages[0].background.offset_y = 5;
            ready_wallpaper_assets(&mut assets, &path, &face);
            assert!(Arc::ptr_eq(&control, assets.get(ir::AssetRef(0)).unwrap()));
            assert_eq!(
                &assets.wallpaper(ir::AssetRef(0)).unwrap().frames[0].rgba[..4],
                &[70, 30, 10, 255]
            );
            face.pages[0].background.offset_y = 0;
            assets.epoch += 1; // same resource owner transferred to a fresh UI generation
            ready_wallpaper_assets(&mut assets, &path, &face);
            assert!(Arc::ptr_eq(
                &wallpaper,
                assets.wallpaper(ir::AssetRef(0)).unwrap()
            ));
            assert!(Arc::ptr_eq(&control, assets.get(ir::AssetRef(0)).unwrap()));
            #[cfg(feature = "shots")]
            assert_eq!(
                assets.scan().preparation_completed,
                3,
                "returning to the warm viewport decodes nothing again"
            );
            face.assets[0].path = "Resources/pictures/revised.png".into();
            ready_wallpaper_assets(&mut assets, &path, &face);
            assert_eq!(
                &assets.wallpaper(ir::AssetRef(0)).unwrap().frames[0].rgba[..4],
                &[20, 30, 90, 255]
            );
            assert_eq!(
                &assets.get(ir::AssetRef(0)).unwrap().frames[0].rgba[..4],
                &[50, 30, 90, 255]
            );
            assert!(!Arc::ptr_eq(
                &wallpaper,
                assets.wallpaper(ir::AssetRef(0)).unwrap()
            ));
            let identity = assets.worker_identity();
            ready_wallpaper_assets(
                &mut assets,
                &directory.path().join("different-owner.nki"),
                &face,
            );
            assert_ne!(
                identity,
                assets.worker_identity(),
                "different resource owner starts a separate preparation"
            );
        }
    }

    #[test]
    fn wallpaper_geometry_rejects_overflow_and_outside_source_viewports() {
        let asset = wallpaper_face().assets.remove(0);
        let request = |window| Request {
            key: Key {
                asset: 0,
                identity: Arc::from("synthetic"),
                frame: 0,
                target: [4, 3],
                window: Some(window),
                plane: Plane::Wallpaper,
            },
            asset: asset.clone(),
            epoch: 0,
            #[cfg(feature = "shots")]
            queued: std::time::Instant::now(),
        };
        assert!(request([0, 13, 4, 3]).decode_spec().is_none());
        assert!(request([u32::MAX, 2, 4, 3]).decode_spec().is_none());
        let meta = ir::ImageMeta {
            frames: 2,
            size: Some(ir::Size {
                width: 1.,
                height: f64::from(u32::MAX),
            }),
            ..Default::default()
        };
        assert!(wallpaper_geometry(meta, 0).is_none());
        let meta = ir::ImageMeta {
            size: Some(ir::Size {
                width: f64::NAN,
                height: 5.,
            }),
            ..Default::default()
        };
        assert!(wallpaper_geometry(meta, 0).is_none());
    }
    #[test]
    fn sparse_strip_requests_follow_authored_mappers_and_fixed_frame_overrides() {
        let (jobs, requests) = mpsc::sync_channel(1);
        let (_done, results) = mpsc::channel();
        let mut worker = Preparation {
            path: "mapper-fixture.uvip".into(),
            program: None,
            jobs,
            results,
            epoch: Arc::new(AtomicU64::new(0)),
            cache: HashMap::new(),
            pending: HashSet::new(),
            wanted: Vec::new(),
            assets: Vec::new(),
            identities: Vec::new(),
            presentation: Default::default(),
            unchanged: false,
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            decoded_bytes: HashMap::new(),
            #[cfg(feature = "shots")]
            probes: Vec::new(),
            #[cfg(feature = "shots")]
            probe_dropped: 0,
        };
        let range = ir::Range {
            min: 1.,
            max: 10000.,
            default: 100.,
            step: None,
        };
        let mut widget = ir::Widget::new(
            "Frequency",
            ir::PageRef(0),
            ir::Rect::new(0, 0, 16, 16),
            ir::Kind::Knob {
                range,
                display: Default::default(),
            },
        );
        widget.binding = ir::Binding::Control(ir::ControlId(1));
        widget.mapper = Some("Exponential".into());
        widget
            .images
            .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
        let mut face = ir::Interface {
            assets: vec![ir::Asset {
                path: "frequency.png".into(),
                kind: ir::AssetKind::Image(ir::ImageMeta {
                    frames: 101,
                    ..Default::default()
                }),
            }],
            widgets: vec![widget],
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 16.,
                    height: 16.,
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut values = Values::default();
        values.insert(ir::ControlId(1), 100.);
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        assert_eq!(
            requests.try_recv().unwrap()[0].key.frame,
            50,
            "100 Hz is the center of an authored 1..10000 exponential strip"
        );
        if let ir::Kind::Knob { range, .. } = &mut face.widgets[0].kind {
            *range = ir::Range {
                min: 0.,
                max: 1.,
                default: 0.25,
                step: None,
            };
        }
        face.widgets[0].mapper = Some("Quadratic".into());
        values.insert(ir::ControlId(1), 0.25);
        worker.pending.clear();
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        assert_eq!(requests.try_recv().unwrap()[0].key.frame, 50);
        face.widgets[0].images[0].frame = Some(7);
        worker.pending.clear();
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        assert_eq!(
            requests.try_recv().unwrap()[0].key.frame,
            7,
            "fixed authored frames take precedence over value mapping"
        );
        face.source = ir::Source::PerformanceView;
        face.widgets[0].kind = ir::Kind::Switch;
        face.widgets[0].images[0].frame = None;
        face.assets[0].kind = ir::AssetKind::Image(ir::ImageMeta {
            frames: 6,
            ..Default::default()
        });
        worker.pending.clear();
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        let frames: Vec<_> = requests
            .try_recv()
            .unwrap()
            .iter()
            .map(|request| request.key.frame)
            .collect();
        assert_eq!(
            frames,
            [0, 1, 2, 3, 4, 5],
            "Kontakt's held/hovered on/off pairs must all be ready for the sparse painter"
        );
    }
    #[test]
    fn selected_program_failure_is_terminal_even_when_generic_art_exists() {
        let dir = tempfile::tempdir().unwrap();
        let pictures = dir.path().join("Resources/pictures");
        std::fs::create_dir_all(&pictures).unwrap();
        let path = dir.path().join("invalid-owner.nki");
        std::fs::write(&path, b"not a serialized instrument").unwrap();
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[1, 2, 3, 255])
                .unwrap();
        }
        std::fs::write(pictures.join("generic.png"), bytes).unwrap();
        let asset = ir::Asset {
            path: "Resources/pictures/generic.png".into(),
            kind: ir::AssetKind::Image(Default::default()),
        };
        assert!(
            Source::of(&path).load(&asset).is_some(),
            "generic art is physically present"
        );
        let face = ir::Interface {
            assets: vec![asset],
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 1.,
                    height: 1.,
                },
                background: ir::Background {
                    image: Some(ir::AssetRef(0)),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut worker = Preparation::new_for_program(&path, 0);
        let start = std::time::Instant::now();
        let values = Values::default();
        loop {
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
            if worker.pending() == 0 {
                break;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(5),
                "owner failure must complete, not strand the request"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(worker.bytes(), 0);
        assert_eq!(worker.cache.len(), 1);
        assert!(worker.cache.values().all(|cached| cached.picture.is_none()));
        #[cfg(feature = "shots")]
        {
            assert_eq!(worker.probes().len(), 1);
            assert!(worker.probes()[0].status.starts_with("lookup-"));
            assert_eq!(worker.scan.lookup_ok, 0);
            assert_eq!(
                worker.scan.lookup_missing, 0,
                "invalid owner is not an absent picture"
            );
        }
        for _ in 0..3 {
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        }
        assert_eq!(worker.pending(), 0);
    }
    #[test]
    fn stale_completion_preserves_same_key_pending_in_new_epoch() {
        let (jobs, requests) = mpsc::sync_channel(1);
        let (done, results) = mpsc::channel();
        let mut worker = Preparation {
            path: "synthetic.nki".into(),
            program: None,
            jobs,
            results,
            epoch: Arc::new(AtomicU64::new(0)),
            cache: HashMap::new(),
            pending: HashSet::new(),
            wanted: Vec::new(),
            assets: Vec::new(),
            identities: Vec::new(),
            presentation: Default::default(),
            unchanged: false,
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            decoded_bytes: HashMap::new(),
            #[cfg(feature = "shots")]
            probes: Vec::new(),
            #[cfg(feature = "shots")]
            probe_dropped: 0,
        };
        let asset = ir::Asset {
            path: "synthetic.png".into(),
            kind: ir::AssetKind::Image(ir::ImageMeta {
                size: Some(ir::Size {
                    width: 1.0,
                    height: 1.0,
                }),
                ..Default::default()
            }),
        };
        let mut face = ir::Interface {
            assets: vec![
                asset.clone(),
                ir::Asset {
                    path: "obsolete.png".into(),
                    ..asset.clone()
                },
            ],
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 1.0,
                    height: 1.0,
                },
                background: ir::Background {
                    image: Some(ir::AssetRef(0)),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut obsolete_widget = ir::Widget::new(
            "$obsolete",
            ir::PageRef(0),
            ir::Rect::new(0, 0, 1, 1),
            ir::Kind::Label,
        );
        obsolete_widget.images.push(ir::ImageUse::new(
            ir::AssetRef(1),
            ir::Role::Background,
        ));
        face.widgets.push(obsolete_widget);
        let values = Values::default();
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        let mut original = requests.try_recv().unwrap();
        assert_eq!(original.len(), 2);
        let at = original
            .iter()
            .position(|request| request.key.asset == 0)
            .unwrap();
        let stale = original.remove(at);
        let obsolete = original.pop().unwrap();
        // Metadata publication also removes a genuinely obsolete pending key.
        // The conservative whole-batch cancellation must still fence the old
        // completion of the unchanged key while its newer request is pending.
        face.widgets[0].hidden = true;
        face.assets.push(ir::Asset {
            path: "unused.png".into(),
            ..asset
        });
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        let current = requests.try_recv().unwrap().pop().unwrap();
        assert_eq!(stale.key, current.key);
        assert_ne!(stale.epoch, current.epoch);
        let picture = Arc::new(Picture::new(vec![Arc::new(
            moose::mui::mui::scene::Image::rgba(1, 1, vec![1, 2, 3, 255]).unwrap(),
        )]));
        let complete = |request: Request| {
            done.send(Result {
                key: request.key,
                epoch: request.epoch,
                picture: Some(picture.clone()),
                font: None,
                bytes: 4,
                #[cfg(feature = "shots")]
                scan: Default::default(),
                #[cfg(feature = "shots")]
                probes: Vec::new(),
            })
            .unwrap();
        };
        complete(stale);
        assert!(
            worker
                .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
                .is_empty()
        );
        assert_eq!(
            worker.pending(),
            1,
            "old completion cannot cancel the current request"
        );
        assert_eq!(worker.bytes(), 0, "old pixels are never admitted");
        assert!(
            requests.try_recv().is_err(),
            "current request must not decode twice"
        );
        complete(obsolete);
        assert!(
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values).is_empty()
        );
        assert_eq!(
            (worker.pending(), worker.bytes()),
            (1, 0),
            "obsolete batch completion is also fenced"
        );
        complete(current);
        let prepared = worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        assert!(Arc::ptr_eq(prepared[0].1.as_ref().unwrap(), &picture));
        assert_eq!(worker.pending(), 0);
        assert_eq!(worker.bytes(), 4);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn poisoned_decode_permit_does_not_strand_later_original_art() {
        const CHILD: &str = "KONTRA_DECODE_POISON_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "ui::picture_worker::tests::poisoned_decode_permit_does_not_strand_later_original_art", "--nocapture"])
                .env(CHILD, "1").env("KONTRA_DISABLE_NETWORK", "1").output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let permit = DECODE.get_or_init(|| Mutex::new(()));
        assert!(
            std::panic::catch_unwind(|| {
                let _guard = permit.lock().unwrap();
                panic!("synthetic decoder permit fault");
            })
            .is_err()
        );
        assert!(permit.is_poisoned());
        prepares_one_frame_off_thread_and_releases_vector_strips();
        assert!(!permit.is_poisoned());
    }
    #[test]
    fn uncacheable_oversized_result_settles_without_requeueing() {
        uncacheable_results_settle(vec![BUDGET + 1]);
    }
    #[test]
    fn uncacheable_working_set_settles_without_requeueing() {
        uncacheable_results_settle(vec![BUDGET / 3 + 1; 3]);
    }
    fn uncacheable_results_settle(sizes: Vec<usize>) {
        let (jobs, requests) = mpsc::sync_channel(1);
        let (done, results) = mpsc::channel();
        let mut worker = Preparation {
            path: "synthetic.nki".into(),
            program: None,
            jobs,
            results,
            epoch: Arc::new(AtomicU64::new(0)),
            cache: HashMap::new(),
            pending: HashSet::new(),
            wanted: Vec::new(),
            assets: Vec::new(),
            identities: Vec::new(),
            presentation: Default::default(),
            unchanged: false,
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            decoded_bytes: HashMap::new(),
            #[cfg(feature = "shots")]
            probes: Vec::new(),
            #[cfg(feature = "shots")]
            probe_dropped: 0,
        };
        let mut face = ir::Interface {
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 1.0,
                    height: 1.0,
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        for asset in 0..sizes.len() {
            face.assets.push(ir::Asset {
                path: format!("synthetic-{asset}.png"),
                kind: ir::AssetKind::Image(ir::ImageMeta {
                    size: Some(ir::Size {
                        width: 1.0,
                        height: 1.0,
                    }),
                    ..Default::default()
                }),
            });
            let mut widget = ir::Widget::new(
                format!("synthetic-{asset}"),
                ir::PageRef(0),
                ir::Rect::new(0, 0, 1, 1),
                ir::Kind::Image,
            );
            widget
                .images
                .push(ir::ImageUse::new(ir::AssetRef(asset), ir::Role::Background));
            face.widgets.push(widget);
        }
        face.validate().unwrap();
        let values = Values::default();
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        for request in requests.try_recv().unwrap() {
            let asset_index = request.key.asset;
            done.send(Result {
                bytes: sizes[request.key.asset],
                key: request.key,
                epoch: request.epoch,
                picture: Some(Arc::new(Picture::new(vec![Arc::new(
                    moose::mui::mui::scene::Image::rgba(1, 1, vec![0; 4]).unwrap(),
                )]))),
                font: None,
                #[cfg(feature = "shots")]
                scan: super::super::pictures::Scan {
                    lookups: asset_index + 1,
                    lookup_ok: asset_index + 1,
                    decodes: asset_index + 1,
                    decode_ok: asset_index + 1,
                    ..Default::default()
                },
                #[cfg(feature = "shots")]
                probes: Vec::new(),
            })
            .unwrap();
        }
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        assert_eq!(
            worker.pending(),
            0,
            "an uncacheable result must settle as a terminal limit"
        );
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        assert!(
            requests.try_recv().is_err(),
            "the unchanged face must not decode again"
        );
        assert!(worker.bytes() <= BUDGET);
        assert_eq!(
            worker.cache.values().filter(|entry| entry.limited).count(),
            1
        );
        #[cfg(feature = "shots")]
        {
            assert_eq!(worker.scan.preparation_completed, sizes.len());
            assert_eq!(worker.scan.preparation_requeued, 0);
            assert_eq!(
                worker.scan.preparation_oversized,
                usize::from(sizes.len() == 1)
            );
            assert_eq!(
                worker.scan.preparation_key_budget,
                usize::from(sizes.len() > 1)
            );
            assert_eq!(worker.scan.lookups, sizes.len());
            assert_eq!(worker.completed_key_bytes().len(), sizes.len());
        }
        #[cfg(feature = "shots")]
        println!(
            "PICTURE_QUEUE completed={} bytes={} max_key_bytes={} wanted_peak_bytes={} oversized={} key_budget={} evicted={} requeued={}",
            worker.scan.preparation_completed,
            worker.scan.preparation_completed_bytes,
            worker.scan.preparation_max_key_bytes,
            worker.scan.preparation_wanted_peak_bytes,
            worker.scan.preparation_oversized,
            worker.scan.preparation_key_budget,
            worker.scan.preparation_evicted,
            worker.scan.preparation_requeued
        );
        if sizes.len() > 1 {
            face.widgets.drain(..sizes.len() - 1);
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
            let request = requests.try_recv().unwrap().pop().unwrap();
            assert_eq!(request.key.asset, sizes.len() - 1);
            done.send(Result {
                bytes: sizes[sizes.len() - 1],
                key: request.key,
                epoch: request.epoch,
                picture: None,
                font: None,
                #[cfg(feature = "shots")]
                scan: Default::default(),
                #[cfg(feature = "shots")]
                probes: Vec::new(),
            })
            .unwrap();
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
            assert_eq!(worker.pending(), 0);
            assert!(
                !worker.cache.values().any(|entry| entry.limited),
                "a smaller wanted set may retry the formerly rejected key"
            );
            assert!(worker.bytes() <= BUDGET);
            #[cfg(feature = "shots")]
            assert_eq!(
                (
                    worker.scan.preparation_evicted,
                    worker.scan.preparation_requeued
                ),
                (1, 1)
            );
        }
    }

    #[test]
    fn prepares_one_frame_off_thread_and_releases_vector_strips() {
        let dir = std::env::temp_dir().join(format!("authored-worker-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Samples")).unwrap();
        std::fs::create_dir_all(dir.join("Resources/pictures")).unwrap();
        let path = dir.join("test.nki");
        std::fs::write(&path, b"synthetic").unwrap();
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 32, 128);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            let pixels = (0..4)
                .flat_map(|n| [n * 60, 30, 90, 255].repeat(32 * 32))
                .collect::<Vec<_>>();
            w.write_image_data(&pixels).unwrap();
        }
        std::fs::write(dir.join("Resources/pictures/strip.png"), bytes).unwrap();
        let asset = ir::Asset {
            path: "Resources/pictures/strip.png".into(),
            kind: ir::AssetKind::Image(ir::ImageMeta {
                frames: 4,
                size: Some(ir::Size {
                    width: 32.0,
                    height: 32.0,
                }),
                ..Default::default()
            }),
        };
        let mut widget = ir::Widget::new(
            "$knob",
            ir::PageRef(0),
            ir::Rect::new(0, 0, 32, 32),
            ir::Kind::Knob {
                range: ir::Range {
                    min: 0.,
                    max: 3.,
                    ..Default::default()
                },
                display: Default::default(),
            },
        );
        widget.binding = ir::Binding::Control(ir::ControlId(1));
        widget
            .images
            .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
        let face = ir::Interface {
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 32.0,
                    height: 32.0,
                },
                ..Default::default()
            }],
            assets: vec![asset],
            widgets: vec![widget],
            ..Default::default()
        };
        let mut values = Values::default();
        values.insert(ir::ControlId(1), 2.);
        let mut worker = Preparation::new(&path);
        let start = std::time::Instant::now();
        loop {
            let prepared =
                worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
            if let Some((_, Some(picture), _, _)) = prepared.first() {
                assert_eq!(picture.frames[0].rgba.len(), 32 * 32 * 4);
                assert_eq!(&picture.frames[0].rgba[..4], &[120, 30, 90, 255]);
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(3));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(worker.bytes(), 32 * 32 * 4);
        assert!(
            worker
                .prepare(&face, ir::PageRef(0), ir::Presentation::Vector, 1., &values)
                .is_empty()
        );
        assert_eq!(worker.bytes(), 0);
        drop(worker);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_turning_strip_keeps_its_nearest_decoded_frame_while_the_next_decodes() {
        let (jobs, requests) = mpsc::sync_channel(1);
        let (done, results) = mpsc::sync_channel(1);
        let mut worker = Preparation {
            path: "synthetic.nki".into(),
            program: None,
            jobs,
            results,
            epoch: Arc::new(AtomicU64::new(0)),
            cache: HashMap::new(),
            pending: HashSet::new(),
            wanted: Vec::new(),
            assets: Vec::new(),
            identities: Vec::new(),
            presentation: Default::default(),
            unchanged: false,
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            decoded_bytes: HashMap::new(),
            #[cfg(feature = "shots")]
            probes: Vec::new(),
            #[cfg(feature = "shots")]
            probe_dropped: 0,
        };
        let mut widget = ir::Widget::new(
            "$knob",
            ir::PageRef(0),
            ir::Rect::new(0, 0, 50, 50),
            ir::Kind::Slider {
                range: ir::Range {
                    min: 0.,
                    max: 127.,
                    step: Some(1.),
                    ..Default::default()
                },
                orientation: ir::Orientation::Vertical,
            },
        );
        widget.binding = ir::Binding::Control(ir::ControlId(1));
        widget
            .images
            .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
        let face = ir::Interface {
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 50.0,
                    height: 50.0,
                },
                ..Default::default()
            }],
            assets: vec![ir::Asset {
                path: "strip.png".into(),
                kind: ir::AssetKind::Image(ir::ImageMeta {
                    frames: 128,
                    ..Default::default()
                }),
            }],
            widgets: vec![widget],
            ..Default::default()
        };
        let mut values = Values::default();
        let mut show = |worker: &mut Preparation, value: f64| {
            values.insert(ir::ControlId(1), value);
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
        };
        assert!(show(&mut worker, 0.).is_empty());
        let first = requests.try_recv().unwrap().pop().unwrap();
        let picture = Arc::new(Picture::prepared(
            Arc::new(moose::mui::mui::scene::Image::rgba(1, 1, vec![1, 2, 3, 255]).unwrap()),
            first.key.frame,
            128,
            None,
        ));
        done.send(Result {
            key: first.key,
            epoch: first.epoch,
            picture: Some(picture),
            font: None,
            bytes: 4,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            probes: Vec::new(),
        })
        .unwrap();
        assert_eq!(show(&mut worker, 0.).len(), 1);
        // The knob turns: its next frame is requested but not decoded yet.
        let turned = show(&mut worker, 60.);
        assert_eq!(requests.try_recv().unwrap()[0].key.frame, 60);
        assert_eq!(
            turned.len(),
            1,
            "the picture vanished while its next frame decoded"
        );
    }
}
