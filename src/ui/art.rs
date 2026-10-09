//! Library artwork made into what the editor shows: the browser's
//! thumbnail, the header banner, the backdrop and the library's color. Made
//! on a thread of its own, both blurred and sharp, once per library: a
//! frame never waits on a picture being scaled. Until one is ready the
//! editor draws without it.

use crate::artwork;
use moose::mui::mui::scene::Image;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

/// Everything shown for one library's artwork.
#[derive(Default)]
pub struct Looks {
    /// The artwork's identity hue (OKLCH degrees), if it has one.
    pub tint: Option<f32>,
    pub thumb: Option<Arc<Image>>,
    /// The header banner and the backdrop, `[sharp, blurred]`.
    pub banner: [Option<Arc<Image>>; 2],
    pub backdrop: [Option<Arc<Image>>; 2],
}

/// What a library's looks are made from: its own artwork, a picture the
/// player chose, or its generated cover ([`super::cover`]).
#[derive(Clone)]
pub enum Source {
    Image(Arc<Image>),
    /// A PNG or JPEG the player chose, and when it was chosen.
    File(std::path::PathBuf, u64),
    /// The generated cover, its color from `artwork` when there is some.
    Cover(super::cover::Spec, Option<Arc<Image>>),
}

impl Source {
    /// A number that changes whenever what the looks show would.
    fn identity(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        match self {
            Self::Image(image) => (0u8, Arc::as_ptr(image) as usize).hash(&mut h),
            Self::File(path, stamp) => (1u8, path, stamp).hash(&mut h),
            Self::Cover(spec, artwork) => {
                (2u8, spec.key(0, 0, true), artwork.as_ref().map(|a| Arc::as_ptr(a) as usize)).hash(&mut h)
            }
        }
        h.finish()
    }
}

type Job = (String, Source);
/// What looks were made from ([`Source::identity`]), and the looks once made.
type Made = (u64, Option<Arc<Looks>>);

#[derive(Default)]
pub struct Art {
    /// By library: the artwork they were made from (its address) and the
    /// looks, `None` while they are being made.
    made: Mutex<HashMap<String, Made>>,
    work: Mutex<Option<mpsc::Sender<Job>>>,
    /// Jobs sent and not yet done.
    pending: Arc<AtomicUsize>,
    /// Looks arrived since the editor last asked.
    ready: Arc<AtomicBool>,
}

impl Art {
    /// `library`'s looks made from `source`; `None` until they are, and the
    /// first ask sends them to be made.
    pub fn get(self: &Arc<Self>, library: &str, source: Source) -> Option<Arc<Looks>> {
        let from = source.identity();
        let mut made = super::lock(&self.made);
        if let Some((_, looks)) = made.get(library).filter(|(at, _)| *at == from) {
            return looks.clone();
        }
        made.insert(library.to_owned(), (from, None));
        drop(made);
        self.send((library.to_owned(), source));
        None
    }

    /// Whether looks arrived since the last call.
    pub fn ready(&self) -> bool {
        self.ready.swap(false, Ordering::AcqRel)
    }

    /// Whether any are still being made.
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.pending.load(Ordering::Acquire) > 0
    }

    fn send(self: &Arc<Self>, job: Job) {
        let mut work = super::lock(&self.work);
        let tx = work.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            // Holds the results, not the `Art`: the thread ends once the
            // editor is gone and its sender with it.
            let weak = Arc::downgrade(self);
            let (pending, ready) = (self.pending.clone(), self.ready.clone());
            let spawned = std::thread::Builder::new()
                .name("kontakto-artwork".into())
                .spawn(move || {
                    for (library, source) in rx {
                        let from = source.identity();
                        let looks = Arc::new(made_from(source));
                        if let Some(art) = weak.upgrade() {
                            let mut made = super::lock(&art.made);
                            if made.get(&library).is_some_and(|(at, _)| *at == from) {
                                made.insert(library, (from, Some(looks)));
                            }
                        }
                        ready.store(true, Ordering::Release);
                        pending.fetch_sub(1, Ordering::AcqRel);
                    }
                });
            if spawned.is_err() {
                // No thread to be had: the editor goes without artwork.
                return mpsc::channel().0;
            }
            tx
        });
        self.pending.fetch_add(1, Ordering::AcqRel);
        if tx.send(job).is_err() {
            self.pending.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

fn made_from(source: Source) -> Looks {
    match source {
        Source::Image(image) => make(image),
        Source::File(path, _) => artwork::decode_file(&path).map(|i| make(Arc::new(i))).unwrap_or_default(),
        Source::Cover(mut spec, artwork) => {
            if let Some(hue) = artwork.as_deref().and_then(artwork::tint) {
                spec = super::cover::Spec::new(&spec.name, &spec.vendor, Some(hue));
            }
            cover(&spec)
        }
    }
}

/// A generated cover's looks: the thumbnail with the name on it; the
/// banner and backdrop in its plain color, as the header names the part.
fn cover(spec: &super::cover::Spec) -> Looks {
    let (tw, th) = px((super::theme::SIDEBAR_MAX, super::theme::SIDEBAR_MAX / 4.));
    let plain = super::cover::cached(spec, 64, 16, false);
    let mut looks = plain.map(|i| make(Arc::new(i))).unwrap_or_default();
    looks.thumb = super::cover::cached(spec, tw, th, true).map(Arc::new);
    looks.tint = Some(spec.hue);
    looks
}

/// A size in points as pixels at twice that, for crispness.
fn px((w, h): (f64, f64)) -> (u32, u32) {
    ((w * 2.).round() as u32, (h * 2.).round() as u32)
}

/// Every look of `image`, at twice the size each is drawn for crispness.
fn make(image: Arc<Image>) -> Looks {
    let (bw, bh) = px(super::rack::BANNER);
    let banner = |blurred| artwork::banner(&image, bw, bh, blurred).map(Arc::new);
    let backdrop = |blurred| artwork::backdrop(&image, blurred).map(Arc::new);
    Looks {
        tint: artwork::tint(&image),
        thumb: Some(image.clone()),
        banner: [banner(false), banner(true)],
        backdrop: [backdrop(false), backdrop(true)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_are_made_off_the_frame_once() {
        let art = Arc::new(Art::default());
        let image = Arc::new(Image::rgba(60, 20, [200u8, 40, 40, 255].repeat(60 * 20)).unwrap());
        let red = || Source::Image(image.clone());
        assert!(art.get("Red", red()).is_none(), "not made yet: the frame goes on without it");
        assert!(art.get("Red", red()).is_none(), "and is not asked for twice");
        while art.busy() {
            std::thread::yield_now();
        }
        assert!(art.ready(), "its arrival wakes the editor");
        let looks = art.get("Red", red()).expect("made");
        assert!(looks.thumb.is_some() && looks.banner.iter().all(Option::is_some));
        assert!(looks.backdrop.iter().all(Option::is_some) && looks.tint.is_some());
        assert!(!art.ready());
        let other = Arc::new(Image::rgba(60, 20, vec![90; 60 * 20 * 4]).unwrap());
        assert!(art.get("Red", Source::Image(other)).is_none(), "new artwork for a library is made again");
        let spec = super::super::cover::Spec::new("Red", "", None);
        assert!(art.get("Red", Source::Cover(spec.clone(), None)).is_none(), "so is a generated cover");
        while art.busy() {
            std::thread::yield_now();
        }
        let looks = art.get("Red", Source::Cover(spec, None)).expect("made");
        assert!(looks.thumb.is_some() && looks.banner.iter().all(Option::is_some) && looks.tint.is_some());
    }
}
