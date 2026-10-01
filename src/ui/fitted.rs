//! Script pictures at the size they show: shrunk by area averaging, or
//! grown by whole pixels, once and off the UI thread, so the renderer
//! draws them one to one instead of resampling them bilinearly every frame.
//! Until one is made the picture as it is stands in, and [`ready`] asks
//! for the frame that swaps it in.
use crate::artwork;
use moose::mui::mui::scene::Image;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// A picture by address, and what was made of it: a cut `[x, y, w, h]`,
/// or a size `[w, h, 0, 0]` tagged by `cut == false`.
type Key = (usize, bool, [u32; 4]);
/// The source is kept with what was made of it, so its address is not reused.
type Made = HashMap<Key, (Arc<Image>, Option<Arc<Image>>)>;

static MADE: LazyLock<Mutex<Made>> = LazyLock::new(Mutex::default);
static READY: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
// ponytail: forgets everything when full; an LRU if window resizes thrash it.
const MAX: usize = 4096;

static WORKER: LazyLock<Mutex<Sender<(Key, Arc<Image>)>>> = LazyLock::new(|| {
    let (tx, rx) = channel::<(Key, Arc<Image>)>();
    let _ = std::thread::Builder::new().name("kontakto-fitted".into()).spawn(move || {
        for (key, source) in rx {
            let [w, h, ..] = key.2;
            let made = resize(&source, w, h).map(Arc::new);
            made_lock().insert(key, (source, made));
            GENERATION.fetch_add(1, Ordering::Relaxed);
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
    READY.swap(false, Ordering::AcqRel)
}

/// Bumped with every picture made: a memo over pictures reads it.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// `image` to draw at `w` x `h` device pixels: itself when that is its
/// size or it would grow by a fraction, else shrunk or grown whole, as soon
/// as that is made.
pub fn fitted(image: &Arc<Image>, w: u32, h: u32) -> Arc<Image> {
    if (w, h) == (image.width, image.height) || w == 0 || h == 0 || image.width == 0 || image.height == 0 {
        return image.clone();
    }
    let whole = w % image.width == 0 && h % image.height == 0 && w / image.width == h / image.height;
    if !whole && (w > image.width || h > image.height) {
        return image.clone();
    }
    let key = (Arc::as_ptr(image) as usize, false, [w, h, 0, 0]);
    let mut made = made_lock();
    match made.get(&key) {
        Some((_, Some(done))) => done.clone(),
        Some((_, None)) => image.clone(),
        None => {
            made.insert(key, (image.clone(), None));
            drop(made);
            let _ = WORKER.lock().unwrap_or_else(PoisonError::into_inner).send((key, image.clone()));
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
    made.insert(key, (image.clone(), piece.clone()));
    piece
}

/// Shrunk by area averaging, or grown by whole pixels.
fn resize(image: &Image, w: u32, h: u32) -> Option<Image> {
    if w <= image.width && h <= image.height {
        return artwork::thumbnail(image, w, h);
    }
    let k = (w / image.width) as usize;
    let row = image.width as usize * 4;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for line in image.rgba.chunks_exact(row) {
        let wide: Vec<u8> = line.chunks_exact(4).flat_map(|p| p.repeat(k)).collect();
        for _ in 0..k {
            rgba.extend_from_slice(&wide);
        }
    }
    Image::rgba(w, h, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_made_to_the_pixel_once() {
        let image = Arc::new(Image::rgba(4, 2, (0..32u8).map(|v| v * 8).collect::<Vec<u8>>()).unwrap());
        assert!(Arc::ptr_eq(&fitted(&image, 4, 2), &image), "one to one is itself");
        assert!(Arc::ptr_eq(&fitted(&image, 6, 3), &image), "a fractional growth is left to the renderer");
        let first = fitted(&image, 8, 4);
        assert!(Arc::ptr_eq(&first, &image), "the original stands in until it is made");
        let made = loop {
            let f = fitted(&image, 8, 4);
            if !Arc::ptr_eq(&f, &image) {
                break f;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!((made.width, made.height), (8, 4));
        assert_eq!(&made.rgba[..8], &image.rgba[..4].repeat(2)[..], "grown by whole pixels");
        let small = resize(&image, 2, 1).unwrap();
        assert_eq!((small.width, small.height), (2, 1));
        let a = cut(&image, 1, 0, 2, 2).unwrap();
        assert!(Arc::ptr_eq(&a, &cut(&image, 1, 0, 2, 2).unwrap()), "cut once");
    }
}
