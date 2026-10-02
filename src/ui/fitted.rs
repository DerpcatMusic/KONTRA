//! Script pictures shrunk by area coverage once and off the UI thread.
//! Enlargements retain the source for the renderer's interpolated sampling;
//! repeating source pixels first would make smooth artwork look pixelated.
//! Until one is made the picture as it is stands in, and [`ready`] asks
//! for the frame that swaps it in.
use crate::artwork;
use moose::mui::mui::scene::Image;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};

/// A picture by address, and what was made of it: a cut `[x, y, w, h]`,
/// or a size `[w, h, 0, 0]` tagged by `cut == false`.
type Key = (usize, bool, [u32; 4]);
/// A weak source keeps its address reserved without retaining unloaded artwork.
type Made = HashMap<Key, (Weak<Image>, Option<Arc<Image>>)>;

static MADE: LazyLock<Mutex<Made>> = LazyLock::new(Mutex::default);
static READY: AtomicBool = AtomicBool::new(false);
/// Per owner (a rack slot), bumped with each picture made for it.
static GENERATION: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
// ponytail: forgets everything when full (a rack of big views makes a few
// thousand); an LRU if window resizes ever thrash it.
const MAX: usize = 1 << 16;

static WORKER: LazyLock<Mutex<Sender<(Key, Arc<Image>, usize)>>> = LazyLock::new(|| {
    let (tx, rx) = channel::<(Key, Arc<Image>, usize)>();
    let _ = std::thread::Builder::new().name("kontakto-fitted".into()).spawn(move || {
        for (key, source, owner) in rx {
            let [w, h, ..] = key.2;
            let made = artwork::resize(&source, w, h).map(Arc::new);
            made_lock().insert(key, (Arc::downgrade(&source), made));
            GENERATION[owner % 64].fetch_add(1, Ordering::Relaxed);
            READY.store(true, Ordering::Release);
        }
    });
    Mutex::new(tx)
});

fn made_lock() -> std::sync::MutexGuard<'static, Made> {
    let mut m = MADE.lock().unwrap_or_else(PoisonError::into_inner);
    if m.len() > MAX {
        m.clear();
    }
    m
}

/// Whether a picture was made since last asked: the editor redraws.
pub fn ready() -> bool {
    let ready = READY.swap(false, Ordering::AcqRel);
    if ready {
        made_lock().retain(|_, (source, _)| source.strong_count() > 0);
    }
    ready
}

/// Bumped with every picture made for `owner`: its memo reads it, and
/// other owners' memos stay as built.
pub fn generation(owner: usize) -> u64 {
    GENERATION[owner % 64].load(Ordering::Relaxed)
}

/// `image` to draw at `w` x `h` device pixels: itself at its size or larger,
/// otherwise shrunk once for `owner` as soon as that is made.
pub fn fitted(image: &Arc<Image>, w: u32, h: u32, owner: usize) -> Arc<Image> {
    if (w, h) == (image.width, image.height) || w == 0 || h == 0 || image.width == 0 || image.height == 0 {
        return image.clone();
    }
    if w > image.width || h > image.height {
        return image.clone();
    }
    let key = (Arc::as_ptr(image) as usize, false, [w, h, 0, 0]);
    let mut made = made_lock();
    match made.get(&key) {
        Some((_, Some(done))) => done.clone(),
        Some((_, None)) => image.clone(),
        None => {
            made.insert(key, (Arc::downgrade(image), None));
            drop(made);
            let _ = WORKER.lock().unwrap_or_else(PoisonError::into_inner).send((key, image.clone(), owner));
            image.clone()
        }
    }
}

/// The `w` by `h` pixels of `image` at `x`, `y`, cut once.
pub fn cut(image: &Arc<Image>, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
    let key = (Arc::as_ptr(image) as usize, true, [x, y, w, h]);
    let mut made = made_lock();
    if let Some((_, piece)) = made.get(&key) {
        return piece.clone();
    }
    let piece = artwork::crop(image, x, y, w, h);
    made.insert(key, (Arc::downgrade(image), piece.clone()));
    piece
}

/// A visible wallpaper window. Bound offsets cached for each full source atlas,
/// while retaining enough windows for all sixteen simultaneously visible parts.
pub fn window(image: &Arc<Image>, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
    let source = Arc::as_ptr(image) as usize;
    let key = (source, true, [x, y, w, h]);
    let mut made = made_lock();
    if let Some((_, piece)) = made.get(&key) { return piece.clone(); }
    if made.keys().filter(|k| k.0 == source && k.1).count() >= 16
        && let Some(old) = made.keys().find(|k| k.0 == source && k.1).copied() {
        made.remove(&old);
    }
    let piece = artwork::crop(image, x, y, w, h);
    made.insert(key, (Arc::downgrade(image), piece.clone()));
    piece
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallpaper_windows_are_cached_and_bounded_for_large_atlases() {
        let image = Arc::new(Image::rgba(2, 20_000, [80, 120, 160, 255].repeat(40_000)).unwrap());
        let first = window(&image, 0, 8_180, 2, 40).unwrap();
        assert_eq!((first.width, first.height), (2, 40), "only the viewport reaches the GPU, even across its atlas limit");
        assert!(Arc::ptr_eq(&first, &window(&image, 0, 8_180, 2, 40).unwrap()), "unchanged windows retain image identity");
        for y in 0..32 { window(&image, 0, y, 2, 40).unwrap(); }
        assert!(made_lock().keys().filter(|k| k.0 == Arc::as_ptr(&image) as usize && k.1).count() <= 16);
        assert_eq!(first.rgba.as_ref(), &[80, 120, 160, 255].repeat(80), "an evicted window still held by a scene stays alive");
    }

    #[test]
    fn pictures_are_made_to_the_pixel_once() {
        let image = Arc::new(Image::rgba(4, 2, (0..32u8).map(|v| v * 8).collect::<Vec<u8>>()).unwrap());
        assert!(Arc::ptr_eq(&fitted(&image, 4, 2, 0), &image), "one to one is itself");
        assert!(Arc::ptr_eq(&fitted(&image, 6, 3, 0), &image), "a fractional growth is left to the renderer");
        assert!(Arc::ptr_eq(&fitted(&image, 8, 4, 0), &image), "whole growth also retains source sampling");
        let first = fitted(&image, 2, 1, 0);
        assert!(Arc::ptr_eq(&first, &image), "the original stands in until it is made");
        let made = loop {
            let f = fitted(&image, 2, 1, 0);
            if !Arc::ptr_eq(&f, &image) {
                break f;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!((made.width, made.height), (2, 1));
        assert!(Arc::ptr_eq(&made, &fitted(&image, 2, 1, 0)), "shrunk once and reused");
        let edge = Image::rgba(4, 1, vec![255,0,0,255, 0,255,0,255, 0,0,255,255, 255,255,0,255]).unwrap();
        assert_eq!(artwork::resize(&edge, 2, 1).unwrap().rgba.as_ref(), &[127,127,0,255, 127,127,127,255], "resize keeps both edges instead of cropping to cover");
        let a = cut(&image, 1, 0, 2, 2).unwrap();
        assert!(Arc::ptr_eq(&a, &cut(&image, 1, 0, 2, 2).unwrap()), "cut once");
    }
}
