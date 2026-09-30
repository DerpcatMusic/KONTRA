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

type Job = (String, Arc<Image>);

#[derive(Default)]
pub struct Art {
    /// By library: the artwork they were made from (its address) and the
    /// looks, `None` while they are being made.
    made: Mutex<HashMap<String, (usize, Option<Arc<Looks>>)>>,
    work: Mutex<Option<mpsc::Sender<Job>>>,
    /// Jobs sent and not yet done.
    pending: Arc<AtomicUsize>,
    /// Looks arrived since the editor last asked.
    ready: Arc<AtomicBool>,
}

impl Art {
    /// `library`'s looks made from `image`; `None` until they are, and the
    /// first ask sends them to be made.
    pub fn get(self: &Arc<Self>, library: &str, image: &Arc<Image>) -> Option<Arc<Looks>> {
        let from = Arc::as_ptr(image) as usize;
        let mut made = super::lock(&self.made);
        if let Some((_, looks)) = made.get(library).filter(|(at, _)| *at == from) {
            return looks.clone();
        }
        made.insert(library.to_owned(), (from, None));
        drop(made);
        self.send((library.to_owned(), image.clone()));
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
                    for (library, image) in rx {
                        let looks = Arc::new(make(&image));
                        if let Some(art) = weak.upgrade() {
                            let mut made = super::lock(&art.made);
                            let from = Arc::as_ptr(&image) as usize;
                            if made.get(&library).is_some_and(|(at, _)| *at == from) {
                                made.insert(library, (from, Some(looks)));
                            }
                        }
                        pending.fetch_sub(1, Ordering::AcqRel);
                        ready.store(true, Ordering::Release);
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

/// Every look of `image`, at twice the size each is drawn for crispness.
fn make(image: &Image) -> Looks {
    let px = |(w, h): (f64, f64)| ((w * 2.).round() as u32, (h * 2.).round() as u32);
    let (tw, th) = px(super::browser::THUMB);
    let (bw, bh) = px(super::rack::BANNER);
    let banner = |blurred| artwork::banner(image, bw, bh, blurred).map(Arc::new);
    let backdrop = |blurred| artwork::backdrop(image, blurred).map(Arc::new);
    Looks {
        tint: artwork::tint(image),
        thumb: artwork::thumbnail(image, tw, th).map(Arc::new),
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
        assert!(art.get("Red", &image).is_none(), "not made yet: the frame goes on without it");
        assert!(art.get("Red", &image).is_none(), "and is not asked for twice");
        while art.busy() {
            std::thread::yield_now();
        }
        assert!(art.ready(), "its arrival wakes the editor");
        let looks = art.get("Red", &image).expect("made");
        assert!(looks.thumb.is_some() && looks.banner.iter().all(Option::is_some));
        assert!(looks.backdrop.iter().all(Option::is_some) && looks.tint.is_some());
        assert!(!art.ready());
        let other = Arc::new(Image::rgba(60, 20, vec![90; 60 * 20 * 4]).unwrap());
        assert!(art.get("Red", &other).is_none(), "new artwork for a library is made again");
    }
}
