//! Pair 1: the Kontakt LP2 legacy filter. Ours is the shipping cascade (`LegacyCascade`); the
//! candidate is a single RBJ-style biquad from the `biquad` crate (feature `candidates`).
//! Fidelity: the Kontakt oracle (`cascade_kontakt_oracle.json`, class Lp2, 10 probes, 48 kHz).

use super::{Fit, PairReport, Quality, side};
use crate::band::Band;
use crate::block::{Block, Factory, Impl, param};
use crate::fit::nelder_mead;
use crate::measure::{self, PROBE_FRAMES};
use sampler_core::dspbench::{Legacy, LegacyCascade};

const ORACLE: &str = include_str!("../../../sampler-core/src/dsp/filter/cascade_kontakt_oracle.json");
/// Probe reads below this are stop-band noise on both sides; excluded (as in the engine's own test).
const FLOOR_DB: f64 = -60.0;

/// One Kontakt measurement: knobs and dB at the probes.
pub struct Case {
    pub cutoff: f64,
    pub resonance: f64,
    pub db: Vec<f64>,
}

pub struct Oracle {
    pub rate: f64,
    pub probes: Vec<f64>,
    pub cases: Vec<Case>,
}

pub fn oracle() -> Oracle {
    oracle_class("Lp2")
}

/// The Kontakt oracle cases of one legacy class (`Lp2`, `Ladder`, ...).
pub fn oracle_class(class: &str) -> Oracle {
    let v: serde_json::Value = serde_json::from_str(ORACLE).expect("oracle json");
    let num = |x: &serde_json::Value| x.as_f64().expect("number");
    Oracle {
        rate: num(&v["rate"]),
        probes: v["probes_hz"].as_array().expect("probes").iter().map(num).collect(),
        cases: v["cases"]
            .as_array()
            .expect("cases")
            .iter()
            .filter(|c| c["class"] == class)
            .map(|c| Case {
                cutoff: num(&c["cutoff"]),
                resonance: num(&c["resonance"]),
                db: c["db"].as_array().expect("db").iter().map(num).collect(),
            })
            .collect(),
    }
}

/// Our side: the shipping LP2 cascade, knobs `cutoff` and `resonance` (0..1).
struct OursLp2 {
    inner: LegacyCascade,
    rate: u32,
    cutoff: f64,
    resonance: f64,
}

impl Block for OursLp2 {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let _ = self.inner.process(input, out);
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "cutoff" => self.cutoff = value,
            "resonance" => self.resonance = value,
            _ => return false,
        }
        self.inner.set_knobs(self.cutoff, self.resonance);
        true
    }
    fn reset(&mut self) {
        self.inner = LegacyCascade::new(Legacy::Lp2, self.cutoff, self.resonance, self.rate);
    }
}

fn make_ours(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    let (cutoff, resonance) = (param(params, "cutoff", 0.6), param(params, "resonance", 0.0));
    Box::new(OursLp2 {
        inner: LegacyCascade::new(Legacy::Lp2, cutoff, resonance, rate as u32),
        rate: rate as u32,
        cutoff,
        resonance,
    })
}

#[cfg(feature = "candidates")]
pub mod candidate {
    use crate::block::Block;
    use biquad::{Biquad as _, Coefficients, DirectForm2Transposed, Hertz, Type};

    /// `biquad` crate low-pass, cutoff in Hz and Q. One section: a 2-pole Kontakt-class counterpart.
    pub struct BiquadCrate {
        rate: f64,
        hz: f64,
        q: f64,
        filter: DirectForm2Transposed<f64>,
    }

    fn coefficients(rate: f64, hz: f64, q: f64) -> Coefficients<f64> {
        Coefficients::<f64>::from_params(
            Type::LowPass,
            Hertz::<f64>::from_hz(rate).expect("rate"),
            Hertz::<f64>::from_hz(hz.clamp(5.0, rate * 0.49)).expect("hz"),
            q.max(0.05),
        )
        .expect("coefficients")
    }

    impl Block for BiquadCrate {
        fn process(&mut self, input: &[f32], out: &mut [f32]) {
            for (y, &x) in out.iter_mut().zip(input) {
                *y = self.filter.run(f64::from(x)) as f32;
            }
        }
        fn set(&mut self, key: &str, value: f64) -> bool {
            match key {
                "hz" => self.hz = value,
                "q" => self.q = value,
                _ => return false,
            }
            self.filter.update_coefficients(coefficients(self.rate, self.hz, self.q));
            true
        }
        fn reset(&mut self) {
            self.filter.reset_state();
        }
    }

    pub fn make(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
        let (hz, q) = (crate::block::param(params, "hz", 1000.0), crate::block::param(params, "q", 0.707));
        Box::new(BiquadCrate { rate, hz, q, filter: DirectForm2Transposed::<f64>::new(coefficients(rate, hz, q)) })
    }
}

/// Fit a model's parameters to one oracle case: minimise the mean squared dB error over the probes
/// above the floor. Returns the best parameter vector and its shape error (dB).
pub(crate) fn fit_case(make: Factory, rate: f64, probes: &[f64], case: &Case, x0: &[Vec<f64>], to_params: &dyn Fn(&[f64]) -> Vec<(&'static str, f64)>, steps: &[f64]) -> (Vec<f64>, f64, f64) {
    let objective = |x: &[f64]| {
        let h = measure::impulse_response(make, rate, &to_params(x), 64, PROBE_FRAMES);
        let db = measure::probe_db(&h, rate, probes);
        probes
            .iter()
            .enumerate()
            .filter(|(i, _)| case.db[*i] > FLOOR_DB)
            .map(|(i, _)| (db[i] - case.db[i]).powi(2))
            .sum::<f64>()
    };
    let best = x0
        .iter()
        .map(|s| nelder_mead(&objective, s, steps, 120))
        .min_by(|a, b| objective(a).total_cmp(&objective(b)))
        .expect("start");
    let h = measure::impulse_response(make, rate, &to_params(&best), 64, PROBE_FRAMES);
    let db = measure::probe_db(&h, rate, probes);
    let err: Vec<f64> = probes
        .iter()
        .enumerate()
        .filter(|(i, _)| case.db[*i] > FLOOR_DB)
        .map(|(i, _)| db[i] - case.db[i])
        .collect();
    let mean = err.iter().sum::<f64>() / err.len().max(1) as f64;
    let shape = err.iter().map(|e| (e - mean).abs()).fold(0.0, f64::max);
    (best, shape, mean)
}

/// Fit summary across cases: worst and median shape error, mean offset, optional identity error.
fn summarise(shapes: &mut [f64], offsets: &[f64], identity: Option<f64>) -> Fit {
    shapes.sort_by(f64::total_cmp);
    Fit {
        mag_err_db: shapes.last().copied().unwrap_or(f64::NAN),
        mag_err_median_db: shapes.get(shapes.len() / 2).copied().unwrap_or(f64::NAN),
        gain_offset_db: offsets.iter().map(|o| o.abs()).sum::<f64>() / offsets.len().max(1) as f64,
        identity_mag_err_db: identity,
        phase_err_deg: None,
        time_err_pct: None,
        cases: shapes.len(),
    }
}

pub fn run(band: &Band, ours: Impl, cand: Option<Impl>, rate: f64, block: usize, fit: bool) -> PairReport {
    let o = oracle();
    let reference = format!("Kontakt 8 LP2 oracle, {} cases x {} probes at {} Hz (cascade_kontakt_oracle.json)", o.cases.len(), o.probes.len(), o.rate);
    let ours_defaults = defaults(ours.label);
    let cand_defaults = cand.map(|c| defaults(c.label));
    let quality_ours = quality(ours.make, rate, block, &ours_defaults, "cutoff", 0.2, 0.9);
    let quality_cand = cand.zip(cand_defaults).map(|(c, d)| quality(c.make, rate, block, &d, "hz", 200.0, 8000.0));
    let other_for_ours = quality_cand.unwrap_or(quality_ours);

    let mut sides = Vec::new();
    // Fidelity exists only at the oracle's own rate, so fits are skipped elsewhere.
    let ours_fit = (fit && (rate - o.rate).abs() < 0.5).then(|| {
        let mut shapes = Vec::new();
        let mut offsets = Vec::new();
        let mut identity = Vec::new();
        for c in &o.cases {
            let h = measure::impulse_response(ours.make, rate, &[("cutoff", c.cutoff), ("resonance", c.resonance)], 64, PROBE_FRAMES);
            let e = errors(&measure::probe_db(&h, rate, &o.probes), &c.db);
            identity.push(shape(&e));
            let to_params = |x: &[f64]| vec![("cutoff", x[0].clamp(0.0, 1.0)), ("resonance", x[1].clamp(0.0, 1.0))];
            let (_, sh, mean) = fit_case(ours.make, rate, &o.probes, c, &[vec![c.cutoff, c.resonance]], &to_params, &[0.05, 0.05]);
            shapes.push(sh);
            offsets.push(mean);
        }
        let worst_identity = identity.iter().copied().fold(0.0, f64::max);
        summarise(&mut shapes, &offsets, Some(worst_identity))
    });
    let ours_note = ours_fit.map_or("no fit (not oracle rate)".into(), |f| format!("identity worst {:.2} dB", f.identity_mag_err_db.unwrap_or(f64::NAN)));
    sides.push(side(band, ours.label, quality_ours, ours_fit, &other_for_ours, ours_note));

    if let (Some(c), Some(qc)) = (cand, quality_cand) {
        let cand_fit = fit.then_some(()).filter(|_| (rate - o.rate).abs() < 0.5).map(|()| {
            let mut shapes = Vec::new();
            let mut offsets = Vec::new();
            for case in &o.cases {
                let to_params = |x: &[f64]| vec![("hz", x[0].exp()), ("q", x[1].max(0.05))];
                // Starts: cutoff mapped log-wise across 20 Hz..20 kHz, Q from resonance.
                let hz0 = 20.0 * 1000f64.powf(case.cutoff);
                let q0 = 0.707 + 3.0 * case.resonance;
                let x0 = vec![vec![hz0.ln(), q0], vec![(hz0 * 2.0).ln(), q0]];
                let (_, sh, mean) = fit_case(c.make, rate, &o.probes, case, &x0, &to_params, &[0.5, 0.3]);
                shapes.push(sh);
                offsets.push(mean);
            }
            summarise(&mut shapes, &offsets, None)
        });
        sides.push(side(band, c.label, qc, cand_fit, &quality_ours, "best mapping: hz, q fitted per case".into()));
    }
    PairReport { id: "filter-lp2", title: "Filter: Kontakt LP2 legacy (ours) vs biquad crate", reference, rate, block, sides }
}

fn errors(db: &[f64], target: &[f64]) -> Vec<f64> {
    db.iter().zip(target).filter(|(_, t)| **t > FLOOR_DB).map(|(d, t)| d - t).collect()
}

fn shape(e: &[f64]) -> f64 {
    let mean = e.iter().sum::<f64>() / e.len().max(1) as f64;
    e.iter().map(|v| (v - mean).abs()).fold(0.0, f64::max)
}

/// Parameters for the quality tests: our LP2 at a mid knob; the candidate at 1 kHz, Q 0.707.
pub fn defaults(label: &str) -> Vec<(&'static str, f64)> {
    if label.starts_with("sampler-core") { vec![("cutoff", 0.6), ("resonance", 0.0)] } else { vec![("hz", 1000.0), ("q", 0.707)] }
}

/// Quality of one implementation with its own default setting, plus fast-modulation alias and stability.
pub(crate) fn quality(make: Factory, rate: f64, block: usize, defaults: &[(&str, f64)], key: &str, lo: f64, hi: f64) -> Quality {
    let thd = {
        let y = measure::run(measure::build(make, rate, defaults).as_mut(), &crate::signal::sine(rate, 1000.0, 0.5, rate as usize), block);
        measure::thd_n_db(&y, rate, 1000.0)
    };
    let mod_out = measure::modulated_run(make, rate, defaults, key, lo, hi, block);
    let alias = measure::alias_db(&mod_out, rate, 2000.0);
    let (stable, peak) = measure::is_stable(&mod_out);
    let (subnormal, spike) = measure::denormal_tail(make, rate, defaults, block);
    Quality {
        alias_db: alias,
        thd_n_db: thd,
        noise_dbfs: measure::noise_floor_dbfs(make, rate, defaults, block),
        subnormal,
        denormal_spike: spike,
        stable,
        peak_dbfs: peak,
        latency: measure::latency_frames(make, rate, defaults),
        ns_per_frame: measure::cpu_ns_per_frame(make, rate, defaults, block, 5),
        instr_per_frame: None,
    }
}

pub fn ours_impl() -> Impl {
    Impl { label: "sampler-core LP2 cascade", make: make_ours }
}

/// The candidate exists only with the `candidates` feature; the shipping build never pulls `biquad`.
#[cfg(feature = "candidates")]
pub fn cand_impl() -> Option<Impl> {
    Some(Impl { label: "biquad crate LP (RBJ-style)", make: candidate::make })
}

#[cfg(not(feature = "candidates"))]
pub fn cand_impl() -> Option<Impl> {
    None
}
