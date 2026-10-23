//! Head-to-head DSP bake-off. One `Block` trait, standard test signals, and three axes per pair:
//! CPU (ns and instructions per frame), absolute quality (aliasing, THD+N, noise, denormals,
//! stability, overshoot, time constants), and Kontakt fidelity (magnitude shape after a fitted
//! parameter mapping, against the Kontakt oracle where one exists). Verdict bands live in `band`.

pub mod band;
pub mod block;
pub mod fft;
pub mod fit;
pub mod measure;
pub mod pairs;
pub mod report;
pub mod signal;

pub use block::{Block, Factory, Impl};

/// Pair ids in report order.
pub const PAIRS: [&str; 3] = ["filter-lp2", "limiter", "resampler"];

/// Our and candidate implementations of a pair, as the CLI and the perf child see them.
/// Block-based pairs only; the resampler is not a `Block` and runs through `pairs::resampler`.
pub fn impls(pair: &str) -> (Impl, Option<Impl>) {
    match pair {
        "filter-lp2" => (pairs::filter::ours_impl(), pairs::filter::cand_impl()),
        "limiter" => (pairs::limiter::ours_impl(), pairs::limiter::cand_impl()),
        other => panic!("unknown pair {other}"),
    }
}

/// Default parameters for quality and CPU runs. Fidelity fits use their own search.
pub fn default_params(pair: &str, label: &str) -> Vec<(&'static str, f64)> {
    match pair {
        "filter-lp2" => pairs::filter::defaults(label),
        "limiter" if label.starts_with("sampler-core") => vec![("release_ms", 100.0)],
        "limiter" => vec![("ceiling_db", 0.0), ("lookahead_ms", 5.0), ("release_ms", 100.0)],
        other => panic!("unknown pair {other}"),
    }
}
