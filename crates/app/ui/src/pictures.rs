//! A tree's pictures, decoded on a worker thread: only the frames the view
//! draws, each once per file content.
//!
//! [`Pictures::frame`] asks the worker for a frame the first time the view
//! draws it and hands back what is ready: that frame, else the frame of the
//! same picture that arrived last (a knob keeps its old frame while the new
//! one decodes), else nothing. [`Pictures::poll`] takes what the worker
//! finished. No frame ever waits on a decode.

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
}

type Done = (AssetRef, u32, Option<Arc<Image>>);

/// The decoded frames of one tree's pictures, by asset and frame.
pub struct Pictures {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    ready: HashMap<(AssetRef, u32), Option<Arc<Image>>>,
    last: HashMap<AssetRef, Arc<Image>>,
    pending: usize,
}

impl Pictures {
    /// A store whose worker reads files through `read`.
    #[must_use]
    pub fn new(read: Box<Read>) -> Self {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (outbox, done) = mpsc::channel::<Done>();
        std::thread::spawn(move || {
            // The same frame of the same bytes decodes once, whatever path
            // or asset asks for it.
            let mut decoded = HashMap::<u64, Option<Arc<Image>>>::new();
            for job in inbox {
                let image = read(&job.path).and_then(|bytes| {
                    let mut h = std::hash::DefaultHasher::new();
                    bytes.hash(&mut h);
                    let axis = job.meta.axis == Orientation::Vertical;
                    (job.frame, job.meta.frames, axis).hash(&mut h);
                    decoded
                        .entry(h.finish())
                        .or_insert_with(|| decode(&bytes, job.meta, job.frame).map(Arc::new))
                        .clone()
                });
                if outbox.send((job.asset, job.frame, image)).is_err() {
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
        }
    }

    /// Frame `n` of the picture `asset` at `path`, or what stands in for it
    /// until it is decoded; asks for it the first time.
    pub fn frame(
        &mut self,
        asset: AssetRef,
        path: &str,
        meta: ImageMeta,
        n: usize,
    ) -> Option<Arc<Image>> {
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
        while let Ok((asset, n, image)) = self.done.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            if let Some(image) = &image {
                self.last.insert(asset, image.clone());
                arrived = true;
            }
            self.ready.insert((asset, n), image);
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
                p.frame(AssetRef(0), "a.png", meta, 0),
                p.frame(AssetRef(1), "b.png", meta, 0),
                p.frame(AssetRef(2), "missing.png", meta, 0),
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
        assert!(pictures.frame(AssetRef(0), "a.png", meta, 1).is_some());
        assert_eq!(pictures.pending(), 1);
    }
}
