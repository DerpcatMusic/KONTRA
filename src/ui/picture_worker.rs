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
    identity: String,
    frame: usize,
    target: [u32; 2],
    window: Option<[u32; 4]>,
}
#[derive(Clone)]
struct Request {
    key: Key,
    asset: ir::Asset,
    epoch: u64,
}
struct Result {
    key: Key,
    picture: Option<Arc<Picture>>,
    font: Option<Font>,
    epoch: u64,
    bytes: usize,
    #[cfg(feature = "shots")]
    scan: super::pictures::Scan,
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
    jobs: SyncSender<Vec<Request>>,
    results: Receiver<Result>,
    epoch: Arc<AtomicU64>,
    cache: HashMap<Key, Cached>,
    pending: HashSet<Key>,
    wanted: Vec<Key>,
    assets: Vec<ir::Asset>,
    presentation: ir::Presentation,
    tick: u64,
    bytes: usize,
    #[cfg(feature = "shots")]
    pub scan: super::pictures::Scan,
    #[cfg(feature = "shots")]
    decoded_bytes: HashMap<Key, usize>,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::Release);
    }
}
impl Preparation {
    pub fn new(path: &Path) -> Self {
        let (jobs, requests) = mpsc::sync_channel::<Vec<Request>>(1);
        let (done, results) = mpsc::sync_channel(1);
        let epoch = Arc::new(AtomicU64::new(0));
        let worker_epoch = epoch.clone();
        let worker_path = path.to_path_buf();
        let _ = thread::Builder::new()
            .name("authored-art".into())
            .spawn(move || {
                // Discovery and all encoded reads happen here, never in Face::new.
                let mut source = Source::of(&worker_path);
                while let Ok(batch) = requests.recv() {
                    for request in batch {
                        let canceled = || worker_epoch.load(Ordering::Acquire) != request.epoch;
                        if canceled() {
                            continue;
                        }
                        let _permit = DECODE.get_or_init(|| Mutex::new(())).lock_unpoisoned();
                        let (picture, font) = if request.asset.kind == ir::AssetKind::TrueTypeFont {
                            (None, source.font(&request.asset))
                        } else {
                            (
                                source.load_frame(
                                    &request.asset,
                                    request.key.frame,
                                    request.key.target,
                                    request.key.window,
                                    canceled,
                                ),
                                None,
                            )
                        };
                        drop(_permit);
                        if canceled() {
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
            jobs,
            results,
            epoch,
            cache: HashMap::new(),
            pending: HashSet::new(),
            wanted: Vec::new(),
            assets: Vec::new(),
            presentation: Default::default(),
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            decoded_bytes: HashMap::new(),
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn pending(&self) -> usize {
        self.pending.len()
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
    ) -> Vec<(usize, Option<Arc<Picture>>, Option<Font>)> {
        if self.assets != face.assets || self.presentation != presentation {
            self.epoch.fetch_add(1, Ordering::Release);
            self.pending.clear();
            let needed = face.needed_assets(presentation);
            self.cache.retain(|key, _| {
                needed.get(key.asset).copied().unwrap_or(false)
                    && face
                        .assets
                        .get(key.asset)
                        .is_some_and(|a| key.identity == format!("{}:{:?}", a.path, a.kind))
            });
            #[cfg(feature = "shots")]
            self.decoded_bytes
                .retain(|key, _| self.cache.contains_key(key));
            self.bytes = self.cache.values().map(|e| e.bytes).sum();
            // Identity keys keep unchanged assets reusable across a sparse publication.
            self.assets = face.assets.clone();
            self.presentation = presentation;
        }
        let epoch = self.epoch.load(Ordering::Acquire);
        self.tick = self.tick.wrapping_add(1);
        let mut wanted = Vec::new();
        let mut add =
            |asset: ir::AssetRef, frame: usize, target: [u32; 2], window: Option<[u32; 4]>| {
                if let Some(a) = face.assets.get(asset.0) {
                    wanted.push(Key {
                        asset: asset.0,
                        identity: format!("{}:{:?}", a.path, a.kind),
                        frame,
                        target,
                        window,
                    });
                }
            };
        if let Some(p) = face.pages.get(page.0)
            && let Some(a) = p.background.image
        {
            let y = p
                .background
                .origin_y
                .saturating_add(p.background.offset_y.max(0) as u32);
            let size = match face.assets[a.0].kind {
                ir::AssetKind::Image(m) => m.size,
                _ => None,
            };
            let w = size.map_or(p.size.width, |s| s.width.min(p.size.width));
            let h = size.map_or(p.size.height, |s| {
                (s.height - f64::from(y)).max(0.).min(p.size.height)
            });
            if w > 0. && h > 0. {
                add(
                    a,
                    p.background.frame as usize,
                    [w, h].map(|n| (n as f64 * scale.clamp(0.01, 1.)).ceil().max(1.) as u32),
                    Some([0, y, w.ceil() as u32, h.ceil() as u32]),
                );
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
                            let t = if range.max == range.min {
                                0.
                            } else {
                                ((value - range.min) / (range.max - range.min)).clamp(0., 1.)
                            };
                            (t * count.saturating_sub(1) as f64).round() as usize
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
                add(image.asset, frame, target, None);
            }
            for s in w
                .style
                .into_iter()
                .chain(w.state_styles.into_iter().flatten())
                .filter_map(|s| face.styles.get(s.0))
            {
                match s.font {
                    ir::Font::Bitmap(a) if presentation == ir::Presentation::Bitmap => {
                        add(a, 0, [u32::MAX; 2], None)
                    }
                    ir::Font::File(a) => add(a, 0, [0; 2], None),
                    _ => {}
                }
            }
        }
        wanted.sort_by(|a, b| {
            (&a.identity, a.frame, a.target, a.window).cmp(&(
                &b.identity,
                b.frame,
                b.target,
                b.window,
            ))
        });
        wanted.dedup();
        if self.wanted != wanted {
            self.cache.retain(|_, entry| !entry.limited);
        }
        self.wanted = wanted;
        while let Ok(mut result) = self.results.try_recv() {
            self.pending.remove(&result.key);
            if result.epoch != epoch {
                continue;
            }
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
            })
            .collect::<Vec<_>>();
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
        self.wanted
            .iter()
            .filter_map(|key| {
                self.cache.get_mut(key).map(|entry| {
                    entry.tick = self.tick;
                    (key.asset, entry.picture.clone(), entry.font.clone())
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            jobs,
            results,
            epoch: Arc::new(AtomicU64::new(0)),
            cache: HashMap::new(),
            pending: HashSet::new(),
            wanted: Vec::new(),
            assets: Vec::new(),
            presentation: Default::default(),
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: Default::default(),
            #[cfg(feature = "shots")]
            decoded_bytes: HashMap::new(),
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
            if let Some((_, Some(picture), _)) = prepared.first() {
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
}
