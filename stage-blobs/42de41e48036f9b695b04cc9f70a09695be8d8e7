//! [`TextRuns`]: the bounded cache of shaped canvas labels behind
//! [`Ui::text_run`](super::Ui::text_run), as a handle a canvas closure keeps.
use std::sync::{Arc, Mutex};

use mui_scene::Font;
use mui_text::{Axis, TextRun};
use rustc_hash::FxHashMap;

/// How many runs the cache keeps before it starts over.
const LIMIT: usize = 2048;
/// The outline tolerance, in pixels at the run's size.
const TOLERANCE: f64 = 0.1;

/// Shaped and outlined text for canvases, kept: a cheap, cloneable,
/// thread-safe handle onto one cache, so a `canvas(move |size| ..)` closure or
/// a helper holding only fonts can draw labels without a `&mut Ui`. Get one
/// from [`Ui::text_runs`](super::Ui::text_runs); every clone shares it.
///
/// A run's `path` has its baseline at `y = 0`; its top-left corner is at
/// `(0, -ascent)`, so `Draw::fill(run.path.clone(), c).at(pt(x, y + run.ascent))`
/// puts the run's top-left at `(x, y)`. The path is an `Arc`: drawing it
/// every frame copies nothing.
#[derive(Clone, Default)]
pub struct TextRuns(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    /// Tried per grapheme after the run's own font.
    fallbacks: Vec<Font>,
    /// By face, size and axes, then by text: a hit allocates nothing.
    // ponytail: a linear scan over the (face, size, axes) sets; a map if a
    // canvas ever uses more than a few dozen.
    sets: Vec<(Key, FxHashMap<String, Arc<TextRun>>)>,
    len: usize,
}

type Key = (u64, u64, Vec<(String, u32)>);

fn same(key: &Key, font: &Font, size: f64, axes: &[Axis<'_>]) -> bool {
    key.0 == font.id()
        && key.1 == size.to_bits()
        && key.2.len() == axes.len()
        && key
            .2
            .iter()
            .zip(axes)
            .all(|(a, b)| a.0 == b.0 && a.1 == b.1.to_bits())
}

impl TextRuns {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    /// `text` shaped and outlined in `font` (then the `Ui`'s fallback faces)
    /// at `size` px, with the variation `axes` set (omitted axes sit at the
    /// face's defaults), shaped once and kept. `None` if it cannot shape.
    pub fn get(
        &self,
        font: &Font,
        text: &str,
        size: f64,
        axes: &[Axis<'_>],
    ) -> Option<Arc<TextRun>> {
        let fonts = {
            let inner = self.lock();
            let hit = inner
                .sets
                .iter()
                .find(|(k, _)| same(k, font, size, axes))
                .and_then(|(_, runs)| runs.get(text));
            if let Some(run) = hit {
                return Some(run.clone());
            }
            std::iter::once(font)
                .chain(&inner.fallbacks)
                .cloned()
                .collect::<Vec<_>>()
        };
        // Shaped outside the lock: another thread's hit need not wait on it.
        let run = Arc::new(mui_text::text_run(&fonts, text, size, axes, TOLERANCE).ok()?);
        let mut inner = self.lock();
        // ponytail: flushed whole at the cap; an LRU if canvases churn past it.
        if inner.len >= LIMIT {
            inner.sets.clear();
            inner.len = 0;
        }
        let i = if let Some(i) = inner
            .sets
            .iter()
            .position(|(k, _)| same(k, font, size, axes))
        {
            i
        } else {
            let key = (
                font.id(),
                size.to_bits(),
                axes.iter()
                    .map(|(t, v)| ((*t).to_owned(), v.to_bits()))
                    .collect(),
            );
            inner.sets.push((key, FxHashMap::default()));
            inner.sets.len() - 1
        };
        if inner.sets[i]
            .1
            .insert(text.to_owned(), run.clone())
            .is_none()
        {
            inner.len += 1;
        }
        Some(run)
    }

    /// How many runs are kept.
    pub fn len(&self) -> usize {
        self.lock().len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// New fallback faces: every kept run may have picked differently.
    pub(super) fn set_fallbacks(&self, fallbacks: Vec<Font>) {
        let mut inner = self.lock();
        *inner = Inner {
            fallbacks,
            ..Inner::default()
        };
    }
}
