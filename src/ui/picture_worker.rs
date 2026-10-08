//! One cancelable preparation worker per Face; byte-bounded, sparse frame LRU.
use super::{
    ir_view::{Picture, Values},
    pictures::Source,
};
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
                        // Bound aggregate transient decoder memory across rack instances.
                        static DECODE: OnceLock<Mutex<()>> = OnceLock::new();
                        let Ok(_permit) = DECODE.get_or_init(|| Mutex::new(())).lock() else {
                            return;
                        };
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
                        let bytes = picture
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
            self.bytes = self.cache.values().map(|e| e.bytes).sum();
            // Identity keys keep unchanged assets reusable across a sparse publication.
            self.assets = face.assets.clone();
            self.presentation = presentation;
        }
        let epoch = self.epoch.load(Ordering::Acquire);
        while let Ok(result) = self.results.try_recv() {
            #[cfg(feature = "shots")]
            {
                self.scan = result.scan;
            }
            self.pending.remove(&result.key);
            if result.epoch != epoch {
                continue;
            }
            if result.bytes > BUDGET {
                continue;
            }
            while self.bytes + result.bytes > BUDGET {
                let Some(key) = self
                    .cache
                    .iter()
                    .min_by_key(|(_, v)| v.tick)
                    .map(|(k, _)| k.clone())
                else {
                    break;
                };
                if let Some(old) = self.cache.remove(&key) {
                    self.bytes -= old.bytes;
                }
            }
            if let Some(old) = self.cache.remove(&result.key) {
                self.bytes -= old.bytes;
            }
            self.bytes += result.bytes;
            self.cache.insert(
                result.key,
                Cached {
                    picture: result.picture,
                    font: result.font,
                    bytes: result.bytes,
                    tick: self.tick,
                },
            );
        }
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
            let y = p.background.origin_y.saturating_add(p.background.offset_y.max(0) as u32);
            let size = match face.assets[a.0].kind {
                ir::AssetKind::Image(m) => m.size,
                _ => None,
            };
            let w = size.map_or(p.size.width, |s| s.width.min(p.size.width));
            let h = size.map_or(p.size.height, |s| {
                s.height.saturating_sub(y).min(p.size.height)
            });
            if w > 0 && h > 0 {
                add(
                    a,
                    p.background.frame as usize,
                    [w, h].map(|n| (n as f64 * scale.clamp(0.01, 1.)).ceil().max(1.) as u32),
                    Some([0, y, w, h]),
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
                    .map_or([w.rect.width.max(1), w.rect.height.max(1)], |s| {
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
            for s in w.style.into_iter().chain(w.state_styles.into_iter().flatten()).filter_map(|s| face.styles.get(s.0)) {
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
        self.wanted = wanted;
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
                    width: 32,
                    height: 32,
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
                    width: 32,
                    height: 32,
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
