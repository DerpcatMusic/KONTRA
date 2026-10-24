//! Pair 7: feed-forward compressor. Ours is the shipping generic dynamics kernel (`Law::Generic`,
//! peak detector, quadratic knee, through `dsp::dynamics`); the candidate is the Giannoulis, Massberg
//! and Reiss (JAES 2012) gain computer with one-pole ballistics on the gain reduction, written here
//! from the paper's equations (no code copied). Kontakt: the committed data has the legacy limiter's
//! static curve but no compressor static curve or ballistics, so the Kontakt axis is absent.

use super::{PairReport, Quality, side};
use crate::band::Band;
use crate::block::{Block, Factory, Impl, param};
use crate::measure;
use sampler_core::dspbench::{DynamicsLaw, DynamicsSettings, DynamicsStage, Detector, Direction, Generic, Link, Parameter, Time, Topology};

/// Keys: `threshold` (dB), `ratio`, `knee` (dB width), `attack_ms`, `release_ms`.
fn settings(p: &[(&str, f64)]) -> DynamicsSettings {
    let (thr, ratio, knee) = (param(p, "threshold", -20.0), param(p, "ratio", 4.0), param(p, "knee", 6.0));
    let (attack, release) = (param(p, "attack_ms", 10.0) / 1000.0, param(p, "release_ms", 100.0) / 1000.0);
    DynamicsSettings {
        law: DynamicsLaw::Generic(Generic {
            threshold_db: thr,
            ratio,
            knee_db: knee,
            direction: Direction::Downward,
            detector: Detector::Peak,
            topology: Topology::FeedForward,
            lookahead: Time::Milliseconds(0.0),
            sidechain_highpass_hz: 0.0,
            range_db: 0.0,
        }),
        link: Link::Max,
        makeup: 1.0,
        mix: 1.0,
        threshold: Parameter::Constant(thr),
        ratio: Parameter::Constant(ratio),
        attack_seconds: Parameter::Constant(attack),
        release_seconds: Parameter::Constant(release),
    }
}

/// Keeps its settings and rate so `reset` can rebuild the stage (the stage has no reset of its own).
struct Ours {
    stage: DynamicsStage,
    settings: DynamicsSettings,
    rate: u32,
}

impl Block for Ours {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        self.stage.process(input, out);
    }
    fn set(&mut self, _key: &str, _value: f64) -> bool {
        false
    }
    fn reset(&mut self) {
        self.stage = DynamicsStage::new(self.settings, self.rate);
    }
}

fn make_ours(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    let settings = settings(params);
    Box::new(Ours { stage: DynamicsStage::new(settings, rate as u32), settings, rate: rate as u32 })
}

/// Giannoulis, Massberg and Reiss: static gain computer in dB, smoothed gain reduction (attack when
/// the reduction grows, release when it shrinks).
pub struct Giannoulis {
    t: f64,
    r: f64,
    w: f64,
    attack: f64,
    release: f64,
    reduction_db: f64,
}

impl Giannoulis {
    pub fn new(rate: f64, p: &[(&str, f64)]) -> Self {
        let (attack_s, release_s) = (param(p, "attack_ms", 10.0) / 1000.0, param(p, "release_ms", 100.0) / 1000.0);
        Self {
            t: param(p, "threshold", -20.0),
            r: param(p, "ratio", 4.0),
            w: param(p, "knee", 6.0),
            attack: (-1.0 / (attack_s * rate)).exp(),
            release: (-1.0 / (release_s * rate)).exp(),
            reduction_db: 0.0,
        }
    }

    /// Static output level (dB) for input level `x` (dB).
    fn computer(&self, x: f64) -> f64 {
        let (t, r, w) = (self.t, self.r, self.w);
        if 2.0 * (x - t) < -w {
            x
        } else if 2.0 * (x - t).abs() <= w {
            x + (1.0 / r - 1.0) * (x - t + w / 2.0).powi(2) / (2.0 * w)
        } else {
            t + (x - t) / r
        }
    }
}

impl Block for Giannoulis {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        for (y, &v) in out.iter_mut().zip(input) {
            let x = 20.0 * f64::from(v).abs().max(1e-12).log10();
            let x_l = self.computer(x) - x;
            let coef = if x_l < self.reduction_db { self.attack } else { self.release };
            self.reduction_db = coef * self.reduction_db + (1.0 - coef) * x_l;
            *y = (f64::from(v) * 10f64.powf(self.reduction_db / 20.0)) as f32;
        }
    }
    fn set(&mut self, _key: &str, _value: f64) -> bool {
        false
    }
    fn reset(&mut self) {
        self.reduction_db = 0.0;
    }
}

fn make_giannoulis(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    Box::new(Giannoulis::new(rate, params))
}

pub fn ours_impl() -> Impl {
    Impl { label: "sampler-core generic compressor kernel", make: make_ours }
}

pub fn cand_impl() -> Option<Impl> {
    Some(Impl { label: "Giannoulis/Massberg/Reiss gain computer (bench)", make: make_giannoulis })
}

pub fn defaults() -> Vec<(&'static str, f64)> {
    vec![("threshold", -20.0), ("ratio", 4.0), ("knee", 6.0), ("attack_ms", 10.0), ("release_ms", 100.0)]
}

/// Static curve difference (dB) between the two sides, over input levels -60..0 dBFS.
fn static_levels() -> Vec<f64> {
    (0..=12).map(|i| -60.0 + 5.0 * f64::from(i)).collect()
}

/// Ballistics time (ms) of the gain: step from -40 dB to -6 dB at the input (attack) and back (release).
fn ballistics(make: Factory, rate: f64, params: &[(&str, f64)], block: usize) -> (f64, f64) {
    let up = measure::time_constant_ms(make, rate, params, block, 0.01, 0.5);
    let down = measure::time_constant_ms(make, rate, params, block, 0.5, 0.01);
    (up, down)
}

fn quality(make: Factory, rate: f64, block: usize) -> (Quality, Vec<f64>, (f64, f64)) {
    let p = defaults();
    let levels = static_levels();
    let curve = measure::static_curve(make, rate, &p, block, &levels);
    let bal = ballistics(make, rate, &p, block);
    let thd = measure::thd_n_db(&measure::run(measure::build(make, rate, &p).as_mut(), &crate::signal::sine(rate, 1000.0, 0.2, rate as usize), block), rate, 1000.0);
    let (subnormal, spike) = measure::denormal_tail(make, rate, &p, block);
    (
        Quality {
            alias_db: f64::NAN,
            thd_n_db: thd,
            noise_dbfs: measure::noise_floor_dbfs(make, rate, &p, block),
            subnormal,
            denormal_spike: spike,
            stable: true,
            peak_dbfs: curve.last().copied().unwrap_or(f64::NAN),
            latency: measure::latency_frames(make, rate, &p),
            ns_per_frame: measure::cpu_ns_per_frame(make, rate, &p, block, 5),
            instr_per_frame: None,
        },
        curve,
        bal,
    )
}

pub fn run(band: &Band, ours: Impl, cand: Option<Impl>, rate: f64, block: usize) -> PairReport {
    let (qo, co, bo) = quality(ours.make, rate, block);
    let cand = cand.map(|c| (c, quality(c.make, rate, block)));
    let (divergence, reach) = match &cand {
        Some((_, (_, cc, _))) => {
            let diffs: Vec<f64> = co.iter().zip(cc).map(|(a, b)| (a - b).abs()).collect();
            (diffs.iter().fold(0.0f64, |m, &d| m.max(d)), diffs.iter().sum::<f64>() / diffs.len() as f64)
        }
        None => (f64::NAN, f64::NAN),
    };
    let curve_note = |c: &[f64]| {
        levels_note(c)
    };
    let mut sides = vec![side(band, ours.label, Quality { alias_db: f64::NAN, ..qo }, None, &cand.as_ref().map_or(qo, |(_, (q, _, _))| *q), format!("static out dB at in -60..0: {}; attack {:.1} ms, release {:.1} ms (63 %)", curve_note(&co), bo.0, bo.1))];
    if let Some((c, (qc, cc, bc))) = cand {
        sides.push(side(band, c.label, Quality { alias_db: f64::NAN, ..qc }, None, &qo, format!("static out dB at in -60..0: {}; attack {:.1} ms, release {:.1} ms; divergence from ours: max {:.2} dB, mean {:.2} dB", curve_note(&cc), bc.0, bc.1, divergence, reach)));
    }
    PairReport { id: "compressor", title: "Compressor: generic kernel (ours) vs Giannoulis/Reiss gain computer", reference: "none for compressors (the committed Kontakt data has the legacy limiter curve only)".into(), rate, block, sides }
}

fn levels_note(curve: &[f64]) -> String {
    curve.iter().step_by(2).map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(" ")
}
