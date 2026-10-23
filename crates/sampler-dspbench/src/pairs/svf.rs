//! Pair 6: the Kontakt Ladder (legacy class 17). Ours is the shipping `LegacyCascade` with `Legacy::Ladder`.
//! Candidate: a Zavalishin TPT one-pole ladder (four zero-delay integrators, feedback `k = 4 r`, the same
//! resonance law as the shipping ladder), written here from the TPT equations. Both sides take cutoff and
//! resonance and are fitted per oracle case. Fidelity: Kontakt oracle class `Ladder` (7 cases x 10 probes, 48 kHz).

use super::filter::{fit_case, oracle_class, quality};
use super::{Fit, PairReport, side};
use crate::band::Band;
use crate::block::{Block, Impl, param};
use sampler_core::dspbench::{Legacy, LegacyCascade};

/// Ours: the shipping Ladder cascade, knobs `cutoff` and `resonance` (0..1).
struct OursLadder {
    inner: LegacyCascade,
    rate: u32,
    cutoff: f64,
    resonance: f64,
}

impl Block for OursLadder {
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
        self.inner = LegacyCascade::new(Legacy::Ladder, self.cutoff, self.resonance, self.rate);
    }
}

fn make_ours(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    let (cutoff, resonance) = (param(params, "cutoff", 0.6), param(params, "resonance", 0.0));
    Box::new(OursLadder {
        inner: LegacyCascade::new(Legacy::Ladder, cutoff, resonance, rate as u32),
        rate: rate as u32,
        cutoff,
        resonance,
    })
}

/// Zavalishin TPT ladder. Each stage is a zero-delay one-pole: `v = (u - s) G`, `y = v + s`, `s = y + v`,
/// with `G = g / (1 + g)`, `g = tan(pi fc / fs)`. Feedback is taken from the previous output sample (the
/// zero-delay solve is not implemented), so the resonance peak is approximate at high `r`.
/// Ceiling: a one-sample feedback delay; upgrade path is the closed-form zero-delay solve.
pub struct TptLadder {
    rate: f64,
    hz: f64,
    r: f64,
    g: f64,
    s: [f64; 4],
    y4: f64,
}

impl TptLadder {
    fn coefficients(&mut self) {
        let g = (std::f64::consts::PI * self.hz.clamp(5.0, self.rate * 0.49) / self.rate).tan();
        self.g = g / (1.0 + g);
    }
}

impl Block for TptLadder {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let k = 4.0 * self.r;
        for (y, &x) in out.iter_mut().zip(input) {
            let mut u = f64::from(x) - k * self.y4;
            for s in &mut self.s {
                let v = (u - *s) * self.g;
                let stage = v + *s;
                *s = stage + v;
                u = stage;
            }
            self.y4 = u;
            *y = u as f32;
        }
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "hz" => self.hz = value,
            "resonance" => self.r = value.clamp(0.0, 1.0),
            _ => return false,
        }
        self.coefficients();
        true
    }
    fn reset(&mut self) {
        self.s = [0.0; 4];
        self.y4 = 0.0;
    }
}

fn make_tpt(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    let mut b = TptLadder {
        rate,
        hz: param(params, "hz", 1000.0),
        r: param(params, "resonance", 0.0).clamp(0.0, 1.0),
        g: 0.0,
        s: [0.0; 4],
        y4: 0.0,
    };
    b.coefficients();
    Box::new(b)
}

pub fn ours_impl() -> Impl {
    Impl { label: "sampler-core Ladder (legacy class 17)", make: make_ours }
}

pub fn cand_impl() -> Option<Impl> {
    Some(Impl { label: "Zavalishin TPT ladder, one-sample feedback (bench)", make: make_tpt })
}

pub fn defaults(label: &str) -> Vec<(&'static str, f64)> {
    if label.starts_with("sampler-core") { vec![("cutoff", 0.6), ("resonance", 0.0)] } else { vec![("hz", 1000.0), ("resonance", 0.0)] }
}

fn summarise(shapes: &mut [f64], offsets: &[f64]) -> Fit {
    shapes.sort_by(f64::total_cmp);
    Fit {
        mag_err_db: shapes.last().copied().unwrap_or(f64::NAN),
        mag_err_median_db: shapes.get(shapes.len() / 2).copied().unwrap_or(f64::NAN),
        gain_offset_db: offsets.iter().map(|o| o.abs()).sum::<f64>() / offsets.len().max(1) as f64,
        identity_mag_err_db: None,
        phase_err_deg: None,
        time_err_pct: None,
        cases: shapes.len(),
    }
}

pub fn run(band: &Band, ours: Impl, cand: Option<Impl>, rate: f64, block: usize, fit: bool) -> PairReport {
    let o = oracle_class("Ladder");
    let reference = format!("Kontakt 8 Ladder oracle, {} cases x {} probes at {} Hz (cascade_kontakt_oracle.json, class Ladder)", o.cases.len(), o.probes.len(), o.rate);
    let on_oracle_rate = (rate - o.rate).abs() < 0.5;
    let quality_ours = quality(ours.make, rate, block, &defaults(ours.label), "cutoff", 0.2, 0.9);
    let cand_pair = cand.map(|c| (c, quality(c.make, rate, block, &defaults(c.label), "hz", 200.0, 8000.0)));
    let other_for_ours = cand_pair.map_or(quality_ours, |(_, q)| q);

    let mut sides = Vec::new();
    let ours_fit = (fit && on_oracle_rate).then(|| {
        let mut shapes = Vec::new();
        let mut offsets = Vec::new();
        for c in &o.cases {
            let to_params = |x: &[f64]| vec![("cutoff", x[0].clamp(0.0, 1.0)), ("resonance", x[1].clamp(0.0, 1.0))];
            let (_, sh, mean) = fit_case(ours.make, rate, &o.probes, c, &[vec![c.cutoff, c.resonance]], &to_params, &[0.05, 0.05]);
            shapes.push(sh);
            offsets.push(mean);
        }
        summarise(&mut shapes, &offsets)
    });
    sides.push(side(band, ours.label, quality_ours, ours_fit, &other_for_ours, "cutoff and resonance fitted per case".into()));

    if let (Some(c), Some((_, qc))) = (cand, cand_pair) {
        let cand_fit = (fit && on_oracle_rate).then(|| {
            let mut shapes = Vec::new();
            let mut offsets = Vec::new();
            for case in &o.cases {
                let to_params = |x: &[f64]| vec![("hz", x[0].exp()), ("resonance", x[1].clamp(0.0, 1.0))];
                // Start: cutoff mapped log-wise across 20 Hz..20 kHz; resonance from the case.
                let hz0 = 20.0 * 1000f64.powf(case.cutoff);
                let (_, sh, mean) = fit_case(c.make, rate, &o.probes, case, &[vec![hz0.ln(), case.resonance]], &to_params, &[0.5, 0.05]);
                shapes.push(sh);
                offsets.push(mean);
            }
            summarise(&mut shapes, &offsets)
        });
        sides.push(side(band, c.label, qc, cand_fit, &quality_ours, "hz and resonance fitted per case".into()));
    }
    PairReport { id: "svf-ladder", title: "Ladder: Kontakt legacy class 17 (ours) vs Zavalishin TPT ladder", reference, rate, block, sides }
}
