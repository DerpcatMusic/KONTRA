//! Pair definitions and the shared result types. Each pair module compares our implementation with
//! one or more candidates on the same quality metrics, and fits each one to Kontakt where a reference exists.

pub mod eq;
pub mod filter;
pub mod limiter;
pub mod oversampler;
pub mod compressor;
pub mod resampler;

use crate::band::{Band, Verdict, classify};
use serde::Serialize;

/// Absolute quality of one implementation at one rate and block size.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Quality {
    /// Aliasing under fast modulation (or nonlinear drive): dB relative to the fundamental.
    pub alias_db: f64,
    /// THD+N of a 1 kHz tone, dB relative to the fundamental.
    pub thd_n_db: f64,
    /// Output RMS for silence in, dBFS: the block's own noise floor.
    pub noise_dbfs: f64,
    /// Subnormal output samples during a decay to 1e-38.
    pub subnormal: usize,
    /// Worst block time over the decay tail divided by the median block time.
    pub denormal_spike: f64,
    /// Finite and bounded output under fast modulation.
    pub stable: bool,
    /// Peak of the output in dBFS for a full-scale-ish drive (true-peak estimate).
    pub peak_dbfs: f64,
    /// Latency in frames.
    pub latency: usize,
    /// Minimum over repetitions of processing time per frame, ns.
    pub ns_per_frame: f64,
    /// Instructions per frame (perf stat, child-minus-empty baseline); None when perf is unavailable.
    pub instr_per_frame: Option<f64>,
}

impl Quality {
    /// True when `self` beats `other` by at least `margin_db` on overshoot, aliasing or noise, or when
    /// it is stable where the other is not, or has fewer subnormal outputs.
    pub fn cleaner_than(&self, other: &Quality, margin_db: f64) -> bool {
        (other.peak_dbfs - self.peak_dbfs) >= margin_db
            || (other.alias_db - self.alias_db) >= margin_db
            || (other.noise_dbfs - self.noise_dbfs) >= margin_db
            || (self.stable && !other.stable)
            || (other.subnormal > 0 && self.subnormal == 0)
    }
}

/// Kontakt-fidelity after the best parameter mapping. `None` axes have no reference data.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Fit {
    /// Worst-case magnitude shape error, dB (max deviation after removing each case's mean offset).
    pub mag_err_db: f64,
    /// Median-case magnitude shape error, dB.
    pub mag_err_median_db: f64,
    /// Mean offset (gain) error after the mapping, dB; reported, not banded.
    pub gain_offset_db: f64,
    /// The same worst-case error with the identity mapping (our own knobs, no fit), if it applies.
    pub identity_mag_err_db: Option<f64>,
    pub phase_err_deg: Option<f64>,
    pub time_err_pct: Option<f64>,
    pub cases: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Side {
    pub label: &'static str,
    pub quality: Quality,
    pub fit: Option<Fit>,
    pub verdict: Verdict,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PairReport {
    pub id: &'static str,
    pub title: &'static str,
    pub reference: String,
    pub rate: f64,
    pub block: usize,
    pub sides: Vec<Side>,
}

/// Build the report side for one implementation from its quality, fit and the other side's quality.
pub fn side(band: &Band, label: &'static str, q: Quality, fit: Option<Fit>, other: &Quality, note: String) -> Side {
    let fits = fit.map(|f| (f.mag_err_db, f.phase_err_deg, f.time_err_pct));
    let cleaner = q.cleaner_than(other, band.cleaner_margin_db);
    Side { label, quality: q, fit, verdict: classify(band, fits, cleaner), note }
}
