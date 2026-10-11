//! Scoped wall-clock totals for the UI hot path, compiled in only with the
//! `shots` feature; otherwise `span` is a no-op the optimizer removes.
#[cfg(feature = "shots")]
mod on {
    use std::{cell::RefCell, collections::BTreeMap, time::Instant};
    thread_local!(static TOTALS: RefCell<BTreeMap<&'static str, (f64, u64)>> = Default::default());
    pub struct Span(&'static str, Instant);
    pub fn span(name: &'static str) -> Span {
        Span(name, Instant::now())
    }
    impl Drop for Span {
        fn drop(&mut self) {
            let ms = self.1.elapsed().as_secs_f64() * 1000.;
            TOTALS.with(|t| {
                let mut t = t.borrow_mut();
                let e = t.entry(self.0).or_default();
                e.0 += ms;
                e.1 += 1;
            });
        }
    }
    /// Drain the totals as `name=ms/calls` pairs.
    pub fn take() -> Vec<(&'static str, f64, u64)> {
        TOTALS.with(|t| std::mem::take(&mut *t.borrow_mut()).into_iter().map(|(k, (ms, n))| (k, ms, n)).collect())
    }
}
#[cfg(feature = "shots")]
pub use on::*;
#[cfg(not(feature = "shots"))]
pub fn span(_: &'static str) {}
