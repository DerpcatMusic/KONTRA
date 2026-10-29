//! A tree's pictures, decoded on a worker thread: only the frames the view
//! draws, each once per file content.
//!
//! [`Pictures::frame`] asks the worker for a frame the first time the view
//! draws it and hands back what is ready: that frame, else the frame of the
//! same picture that arrived last (a knob keeps its old frame while the new
//! one decodes), else nothing. [`Pictures::poll`] takes what the worker
//! finished. No frame ever waits on a decode.
//!
//! Frames decode at the scale they are drawn at on the device, in quarter
//! steps, so the painter maps them pixel for pixel; a new scale drops the
//! frames of the old one.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::sync::{Arc, mpsc};

use kontra_ui_model::{AssetRef, ImageMeta, Orientation};
use mui::prelude::Image;

/// Reads a library-relative path; `None` when there is no such file.
pub type Read = dyn Fn(&str) -> Option<Vec<u8>> + Send;

struct Job {
    asset: AssetRef,
    frame: u32,
    path: String,
    meta: ImageMeta,
    scale: f32,
}

type Done = (AssetRef, u32, f32, Option<Arc<Image>>);

/// The decoded frames of one tree's pictures, by asset and frame.
pub struct Pictures {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    ready: HashMap<(AssetRef, u32), Option<Arc<Image>>>,
    last: HashMap<AssetRef, Arc<Image>>,
    pending: usize,
    /// The scale the frames in `ready` decoded at.
    scale: f32,
}

impl Pictures {
    /// A store whose worker reads files through `read`.
    #[must_use]
    pub fn new(read: Box<Read>) -> Self {
        Self::with_look(read, |_| None)
    }

    /// A store whose worker reads files through `read` and draws each
    /// picture as `look` makes it (`None` keeps it as it is).
    #[must_use]
    pub fn with_look(read: Box<Read>, look: fn(&Image) -> Option<Image>) -> Self {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (outbox, done) = mpsc::channel::<Done>();
        std::thread::spawn(move || {
            // The same frame of the same bytes decodes once, whatever path
            // or asset asks for it.
            let mut decoded = HashMap::<u64, Option<Arc<Image>>>::new();
            let mut scale = 1.;
            for job in inbox {
                if job.scale != scale {
                    decoded.clear();
                    scale = job.scale;
                }
                let image = read(&job.path).and_then(|bytes| {
                    let mut h = std::hash::DefaultHasher::new();
                    bytes.hash(&mut h);
                    let axis = job.meta.axis == Orientation::Vertical;
                    (job.frame, job.meta.frames, axis).hash(&mut h);
                    decoded
                        .entry(h.finish())
                        .or_insert_with(|| {
                            let image = decode(&bytes, job.meta, job.frame)?;
                            let image = look(&image).unwrap_or(image);
                            at(image, job.scale).map(Arc::new)
                        })
                        .clone()
                });
                if outbox
                    .send((job.asset, job.frame, job.scale, image))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            jobs,
            done,
            ready: HashMap::new(),
            last: HashMap::new(),
            pending: 0,
            scale: 1.,
        }
    }

    /// Frame `n` of the picture `asset` at `path`, decoded for `scale` device
    /// pixels a picture pixel, or what stands in for it until it is; asks
    /// for it the first time.
    pub fn frame(
        &mut self,
        asset: AssetRef,
        path: &str,
        meta: ImageMeta,
        n: usize,
        scale: f64,
    ) -> Option<Arc<Image>> {
        let scale = quarter(scale);
        if scale != self.scale {
            // The old frames stand in (`last`) until the new ones arrive.
            self.ready.clear();
            self.scale = scale;
        }
        let n = u32::try_from(n)
            .unwrap_or(u32::MAX)
            .min(meta.frames.saturating_sub(1));
        match self.ready.get(&(asset, n)) {
            Some(Some(image)) => return Some(image.clone()),
            Some(None) => {}
            None => {
                self.ready.insert((asset, n), None);
                let job = Job {
                    asset,
                    frame: n,
                    path: path.to_owned(),
                    meta,
                    scale,
                };
                if self.jobs.send(job).is_ok() {
                    self.pending += 1;
                }
            }
        }
        self.last.get(&asset).cloned()
    }

    /// Takes what the worker finished; `true` when a frame arrived and the
    /// view should be drawn again.
    pub fn poll(&mut self) -> bool {
        let mut arrived = false;
        while let Ok((asset, n, scale, image)) = self.done.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            if let Some(image) = &image {
                self.last.insert(asset, image.clone());
                arrived = true;
            }
            if scale == self.scale {
                self.ready.insert((asset, n), image);
            }
        }
        arrived
    }

    /// Asked for and not back yet.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending
    }

    /// The decoded bytes held, each shared frame once.
    #[must_use]
    pub fn bytes(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        self.ready
            .values()
            .flatten()
            .filter(|i| seen.insert(Arc::as_ptr(i)))
            .map(|i| i.rgba.len())
            .sum()
    }
}

impl std::fmt::Debug for Pictures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pictures")
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

/// Above this a PNG is not a UI picture (v2's bound).
const LIMIT: usize = 32 << 20;

/// Frame `n` of a PNG strip of `meta.frames` along `meta.axis`, as RGBA8.
/// A vertical strip stops decoding at the frame's last row.
fn decode(bytes: &[u8], meta: ImageMeta, n: u32) -> Option<Image> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        return jpeg(bytes);
    }
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: LIMIT });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    let channels = match reader.output_color_type().0 {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    let (frames, n) = (meta.frames.max(1) as usize, n as usize);
    let [x, y, fw, fh] = match meta.axis {
        Orientation::Vertical => {
            let fh = h / frames;
            [0, n * fh, w, fh]
        }
        Orientation::Horizontal => {
            let fw = w / frames;
            [n * fw, 0, fw, h]
        }
    };
    if fw == 0 || fh == 0 || x + fw > w || y + fh > h {
        return None;
    }
    let mut out = Vec::with_capacity(fw * fh * 4);
    let mut take = |row: &[u8]| -> Option<()> {
        let px = row.get(x * channels..(x + fw) * channels)?;
        match channels {
            4 => out.extend_from_slice(px),
            3 => out.extend(
                px.as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|&[r, g, b]| [r, g, b, 255]),
            ),
            2 => out.extend(
                px.as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|&[l, a]| [l, l, l, a]),
            ),
            _ => out.extend(px.iter().flat_map(|&l| [l, l, l, 255])),
        }
        Some(())
    };
    if reader.info().interlaced {
        // Adam7 rows arrive out of order: the whole image, then the frame.
        let mut all = vec![0; reader.output_buffer_size()?];
        reader.next_frame(&mut all).ok()?;
        for row in all.chunks_exact(w * channels).skip(y).take(fh) {
            take(row)?;
        }
    } else {
        for sy in 0..y + fh {
            let row = reader.next_row().ok()??;
            if sy >= y {
                take(row.data())?;
            }
        }
    }
    Image::rgba(fw as u32, fh as u32, out)
}

/// `scale` up to the next quarter, between a quarter and four.
fn quarter(scale: f64) -> f32 {
    let q = (scale * 4.).ceil() / 4.;
    if q.is_finite() {
        q.clamp(0.25, 4.) as f32
    } else {
        1.
    }
}

/// `image` at `scale`, unchanged at 1.
fn at(image: Image, scale: f32) -> Option<Image> {
    if scale == 1. {
        return Some(image);
    }
    let size = |n: u32| ((f64::from(n) * f64::from(scale)).round() as u32).max(1);
    resample(&image, size(image.width), size(image.height))
}

/// `img` at `w` × `h` through a Catmull-Rom filter, widened when it
/// shrinks, in premultiplied alpha so an edge keeps its colour.
pub(crate) fn resample(img: &Image, w: u32, h: u32) -> Option<Image> {
    let (sw, sh) = (img.width as usize, img.height as usize);
    let (w, h) = (w as usize, h as usize);
    let src: Vec<[f32; 4]> = img
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| {
            let k = f32::from(a) / 255.;
            [
                f32::from(r) * k,
                f32::from(g) * k,
                f32::from(b) * k,
                f32::from(a),
            ]
        })
        .collect();
    if src.len() != sw * sh {
        return None;
    }
    let mut across = Vec::with_capacity(w * sh);
    let taps = weights(sw, w);
    for row in src.chunks_exact(sw.max(1)) {
        for (lo, ws) in &taps {
            across.push(mix(row.iter().skip(*lo), ws));
        }
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for (lo, ws) in weights(sh, h) {
        let rows: Vec<&[[f32; 4]]> = across
            .chunks_exact(w.max(1))
            .skip(lo)
            .take(ws.len())
            .collect();
        for x in 0..w {
            let px = mix(rows.iter().filter_map(|r| r.get(x)), &ws);
            let k = if px[3] > 0. { 255. / px[3] } else { 0. };
            let [r, g, b, a] = px;
            out.extend([r * k, g * k, b * k, a].map(|c| c.round().clamp(0., 255.) as u8));
        }
    }
    Image::rgba(w as u32, h as u32, out)
}

/// The weighted sum of `px`.
fn mix<'a>(px: impl Iterator<Item = &'a [f32; 4]>, ws: &[f32]) -> [f32; 4] {
    let mut sum = [0.; 4];
    for (p, &w) in px.zip(ws) {
        for (s, c) in sum.iter_mut().zip(p) {
            *s += c * w;
        }
    }
    sum
}

/// For each of `to` samples across `from`: its first source sample and the
/// weights from there.
fn weights(from: usize, to: usize) -> Vec<(usize, Vec<f32>)> {
    let ratio = to as f64 / from.max(1) as f64;
    let stretch = (1. / ratio).max(1.);
    let last = from.saturating_sub(1) as f64;
    (0..to)
        .map(|i| {
            let c = (i as f64 + 0.5) / ratio - 0.5;
            let lo = (c - 2. * stretch).ceil().clamp(0., last);
            let hi = (c + 2. * stretch).floor().clamp(lo, last);
            let mut ws: Vec<f32> = (lo as usize..=hi as usize)
                .map(|s| catmull_rom((s as f64 - c) / stretch) as f32)
                .collect();
            let sum: f32 = ws.iter().sum();
            if sum != 0. {
                ws.iter_mut().for_each(|w| *w /= sum);
            }
            (lo as usize, ws)
        })
        .collect()
}

fn catmull_rom(x: f64) -> f64 {
    let x = x.abs();
    if x < 1. {
        (1.5 * x - 2.5) * x * x + 1.
    } else if x < 2. {
        ((-0.5 * x + 2.5) * x - 4.) * x + 2.
    } else {
        0.
    }
}

/// A JPEG (a library's cover), whole, as RGBA8: JPEGs are stills.
fn jpeg(bytes: &[u8]) -> Option<Image> {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    let (w, h) = (u32::from(info.width), u32::from(info.height));
    if usize::from(info.width) * usize::from(info.height) * 4 > LIMIT {
        return None;
    }
    Image::rgba(w, h, decoder.decode().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jpeg_decodes_whole_as_a_still() {
        let bytes = include_bytes!("../tests/fixtures/cover.jpg");
        let image = decode(bytes, ImageMeta::default(), 0);
        let image = image
            .as_ref()
            .map(|i| (i.width, i.height, i.rgba.get(..4).map(<[u8]>::to_vec)));
        let Some((8, 4, Some(px))) = image else {
            panic!("{image:?}");
        };
        // The red it was saved with, give or take the JPEG's rounding.
        assert!(
            px[0] > 180 && px[1] < 70 && px[2] < 50 && px[3] == 255,
            "{px:?}"
        );
    }

    pub(crate) fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = e.write_header() {
            assert!(w.write_image_data(rgba).is_ok());
        }
        out
    }

    #[test]
    fn a_frame_cuts_from_its_strip_on_either_axis() {
        // 2x4, rows red, green, blue, white.
        let px: Vec<u8> = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255; 4],
        ]
        .iter()
        .flat_map(|c| [*c, *c])
        .flatten()
        .collect();
        let bytes = png(2, 4, &px);
        let meta = ImageMeta {
            frames: 2,
            ..ImageMeta::default()
        };
        let down = decode(&bytes, meta, 1);
        assert_eq!(down.as_ref().map(|i| (i.width, i.height)), Some((2, 2)));
        assert_eq!(
            down.and_then(|i| i.rgba.get(..4).map(<[u8]>::to_vec)),
            Some(vec![0, 0, 255, 255])
        );
        let across = ImageMeta {
            axis: Orientation::Horizontal,
            ..meta
        };
        assert_eq!(
            decode(&bytes, across, 1).map(|i| (i.width, i.height)),
            Some((1, 4))
        );
    }

    #[test]
    fn frames_arrive_off_the_ui_thread_once_and_a_missing_file_is_not_pending() {
        let red = png(1, 2, &[255, 0, 0, 255, 0, 0, 255, 255]);
        let mut pictures =
            Pictures::new(Box::new(move |p| (p != "missing.png").then(|| red.clone())));
        let meta = ImageMeta {
            frames: 2,
            ..ImageMeta::default()
        };
        let ask = |p: &mut Pictures| {
            [
                p.frame(AssetRef(0), "a.png", meta, 0, 1.),
                p.frame(AssetRef(1), "b.png", meta, 0, 1.),
                p.frame(AssetRef(2), "missing.png", meta, 0, 1.),
            ]
        };
        assert!(
            ask(&mut pictures).iter().all(Option::is_none),
            "nothing waits"
        );
        ask(&mut pictures);
        assert_eq!(pictures.pending(), 3, "each frame is asked for once");
        for _ in 0..1000 {
            pictures.poll();
            if pictures.pending() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let [a, b, missing] = ask(&mut pictures);
        assert!(a.is_some() && missing.is_none());
        // a.png and b.png hold the same bytes: one decode, shared.
        assert!(a.zip(b).is_some_and(|(a, b)| Arc::ptr_eq(&a, &b)));
        assert_eq!(pictures.bytes(), 4);
        // The other frame is asked for; the one shown stands in meanwhile.
        assert!(pictures.frame(AssetRef(0), "a.png", meta, 1, 1.).is_some());
        assert_eq!(pictures.pending(), 1);
    }

    #[test]
    fn a_frame_decodes_at_its_device_scale_and_keeps_its_edges() {
        // A white pixel beside a clear one.
        let img = Image::rgba(2, 1, vec![255, 255, 255, 255, 0, 0, 0, 0]);
        let big = img.and_then(|i| at(i, quarter(1.9)));
        assert_eq!(big.as_ref().map(|i| (i.width, i.height)), Some((4, 2)));
        // Premultiplied: what is not clear stays white, never grey.
        let px: Vec<[u8; 4]> = big
            .map(|i| i.rgba.as_chunks::<4>().0.to_vec())
            .unwrap_or_default();
        assert_eq!(px.first(), Some(&[255, 255, 255, 255]));
        assert!(
            px.iter().all(|p| p[3] == 0 || p[..3] == [255, 255, 255]),
            "{px:?}"
        );
        assert_eq!(quarter(1.23), 1.25);
        assert_eq!(quarter(1.), 1.);
        // Shrunk by half, a flat picture stays flat.
        let grey = Image::rgba(4, 4, [90u8, 90, 90, 255].repeat(16));
        let small = grey.and_then(|i| at(i, 0.5));
        assert_eq!(
            small.map(|i| i.rgba.to_vec()),
            Some([90u8, 90, 90, 255].repeat(4))
        );
    }
}
