//! Optional timings for the reader and translator on the current loader thread.

use std::cell::RefCell;
use std::time::{Duration, Instant};

/// A format reader's measurable stages.
#[derive(Clone, Copy, Debug)]
pub enum Stage {
    Read,
    Translate,
}

/// Stage durations; a missing value means that the format did not expose it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub read: Option<Duration>,
    pub translate: Option<Duration>,
}

thread_local! {
    static CURRENT: RefCell<Option<Timings>> = const { RefCell::new(None) };
}

/// Measures a stage when a caller is collecting timings.
pub fn measure<T>(stage: Stage, f: impl FnOnce() -> T) -> T {
    let active = CURRENT.with(|current| current.borrow().is_some());
    if !active {
        return f();
    }
    let start = Instant::now();
    let result = f();
    CURRENT.with(|current| {
        if let Some(timings) = current.borrow_mut().as_mut() {
            let value = match stage {
                Stage::Read => &mut timings.read,
                Stage::Translate => &mut timings.translate,
            };
            *value = Some(value.unwrap_or_default() + start.elapsed());
        }
    });
    result
}

struct Restore(Option<Timings>);

impl Drop for Restore {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = self.0.take());
    }
}

/// Runs one instrument read and collects its exposed stage timings.
pub fn collect<T>(f: impl FnOnce() -> T) -> (T, Timings) {
    let previous = CURRENT.with(|current| current.replace(Some(Timings::default())));
    let _restore = Restore(previous);
    let result = f();
    let timings = CURRENT.with(|current| current.borrow_mut().take().unwrap_or_default());
    (result, timings)
}
