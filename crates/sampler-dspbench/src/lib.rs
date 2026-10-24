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
#[cfg(feature = "candidates")]
pub mod stretch;
#[cfg(feature = "candidates")]
pub mod signalsmith;

pub use block::{Block, Factory, Impl};

/// Pair ids in report order.
pub const PAIRS: [&str; 7] = ["filter-lp2", "limiter", "resampler", "eq-bell", "oversampler", "compressor", "svf-ladder"];

/// Our and candidate implementations of a pair, as the CLI and the perf child see them.
/// Block-based pairs only; the resampler is not a `Block` and runs through `pairs::resampler`.
pub fn impls(pair: &str) -> (Impl, Option<Impl>) {
    match pair {
        "filter-lp2" => (pairs::filter::ours_impl(), pairs::filter::cand_impl()),
        "limiter" => (pairs::limiter::ours_impl(), pairs::limiter::cand_impl()),
        "eq-bell" => (pairs::eq::ours_impl(), pairs::eq::cand_impl()),
        "oversampler" => (pairs::oversampler::ours_impl(), pairs::oversampler::cand_impl()),
        "compressor" => (pairs::compressor::ours_impl(), pairs::compressor::cand_impl()),
        "svf-ladder" => (pairs::svf::ours_impl(), pairs::svf::cand_impl()),
        other => panic!("unknown pair {other}"),
    }
}

/// Default parameters for quality and CPU runs. Fidelity fits use their own search.
pub fn default_params(pair: &str, label: &str) -> Vec<(&'static str, f64)> {
    match pair {
        "filter-lp2" => pairs::filter::defaults(label),
        "limiter" if label.starts_with("sampler-core") => vec![("release_ms", 100.0)],
        "limiter" => vec![("ceiling_db", 0.0), ("lookahead_ms", 5.0), ("release_ms", 100.0)],
        "eq-bell" => vec![("shape", 0.0), ("hz", 1000.0), ("db", 6.0), ("q", 1.0)],
        "oversampler" => vec![("drive", 4.0)],
        "compressor" => pairs::compressor::defaults(),
        "svf-ladder" => pairs::svf::defaults(label),
        other => panic!("unknown pair {other}"),
    }
}
