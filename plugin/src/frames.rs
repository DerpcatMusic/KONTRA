//! Per-frame stage timings, on with `KONTRA_FRAME_TIMES=1`: every
//! [`EVERY`] samples of a stage, its p50, p99 and max go to stderr.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const EVERY: usize = 20;

static ON: LazyLock<bool> = LazyLock::new(|| std::env::var_os("KONTRA_FRAME_TIMES").is_some());

thread_local! {
    static SEEN: RefCell<HashMap<&'static str, Vec<Duration>>> = RefCell::new(HashMap::new());
}

/// `f`, timed as `stage` when timings are on.
pub fn time<T>(stage: &'static str, f: impl FnOnce() -> T) -> T {
    if !*ON {
        return f();
    }
    let t = Instant::now();
    let out = f();
    note(stage, t.elapsed());
    out
}

/// One sample of `stage`.
pub fn note(stage: &'static str, took: Duration) {
    if !*ON {
        return;
    }
    SEEN.with_borrow_mut(|seen| {
        let all = seen.entry(stage).or_default();
        all.push(took);
        if all.len() >= EVERY {
            all.sort();
            let at = |q: f64| {
                all.get(((all.len() - 1) as f64 * q) as usize)
                    .copied()
                    .unwrap_or_default()
            };
            let ms = |d: Duration| d.as_secs_f64() * 1e3;
            eprintln!(
                "frame-times {stage}: p50 {:.2} ms, p99 {:.2} ms, max {:.2} ms",
                ms(at(0.5)),
                ms(at(0.99)),
                ms(at(1.)),
            );
            all.clear();
        }
    });
}
