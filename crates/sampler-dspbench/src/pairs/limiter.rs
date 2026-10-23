//! Pair 2: the dB-domain limiter. Ours is `DbLimiter` (Kontakt `NI::DSP::Limiter` port, via
//! `sampler_core::dspbench`). Candidate: a plain look-ahead peak limiter written for this bench.
//! No Kontakt reference is captured for the limiter yet, so the fidelity axis is reported as absent.

use super::{PairReport, Quality, side};
use crate::band::Band;
use crate::block::{Block, Factory, Impl, param};
use crate::measure;
use sampler_core::dspbench::DbLimiter;
use std::collections::VecDeque;

/// Ours, mono (both channels fed the same input, left out). Knobs follow the engine's law:
/// `input` and `output` at 1.0 are unity gain; `release` value v gives tau = 6 v seconds.
pub struct OursLimiter {
    inner: DbLimiter,
    rate: f32,
}

impl OursLimiter {
    fn new(rate: f64, release_ms: f64) -> Self {
        let mut inner = DbLimiter::new(rate as f32);
        inner.set(0, 1.0);
        inner.set(1, (release_ms / 6000.0) as f32);
        inner.set(2, 1.0);
        inner.set(4, 0.0);
        Self { inner, rate: rate as f32 }
    }
}

impl Block for OursLimiter {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        for (y, &x) in out.iter_mut().zip(input) {
            *y = self.inner.process([x, x], [0.0, 0.0])[0];
        }
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "release_ms" => self.inner.set(1, (value / 6000.0) as f32),
            "instant" => self.inner.set(4, if value >= 0.5 { 1.0 } else { 0.0 }),
            _ => return false,
        }
        true
    }
    fn reset(&mut self) {
        *self = Self::new(f64::from(self.rate), 100.0);
    }
}

fn make_ours(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    Box::new(OursLimiter::new(rate, param(params, "release_ms", 100.0)))
}

/// Reference look-ahead limiter: the gain needed for each sample is held over a `lookahead_ms`
/// window (sliding minimum), applied to the delayed signal, so the ceiling is never exceeded. The
/// gain falls back with a one-pole release.
pub struct LookAhead {
    rate: f64,
    ceiling: f32,
    look: usize,
    release: f32,
    ring: Vec<f32>,
    pos: usize,
    window: VecDeque<(usize, f32)>,
    gain: f32,
    t: usize,
}

impl LookAhead {
    pub fn new(rate: f64, ceiling_db: f64, lookahead_ms: f64, release_ms: f64) -> Self {
        let look = ((lookahead_ms * rate / 1000.0).round() as usize).max(1);
        Self {
            rate,
            ceiling: 10f32.powf(ceiling_db as f32 / 20.0),
            look,
            release: (-1.0 / (release_ms * 0.001 * rate)).exp() as f32,
            ring: vec![0.0; look + 1],
            pos: 0,
            window: VecDeque::new(),
            gain: 1.0,
            t: 0,
        }
    }
}

impl Block for LookAhead {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        for (y, &x) in out.iter_mut().zip(input) {
            let need = if x.abs() > self.ceiling { self.ceiling / x.abs() } else { 1.0 };
            while self.window.back().is_some_and(|&(_, v)| v >= need) {
                self.window.pop_back();
            }
            self.window.push_back((self.t, need));
            while self.window.front().is_some_and(|&(i, _)| i + self.look < self.t) {
                self.window.pop_front();
            }
            let target = self.window.front().map_or(1.0, |&(_, v)| v);
            self.gain = if target < self.gain { target } else { target + (self.gain - target) * self.release };
            let delayed = self.ring[self.pos];
            self.ring[self.pos] = x;
            self.pos = (self.pos + 1) % self.ring.len();
            *y = delayed * self.gain;
            self.t += 1;
        }
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "ceiling_db" => self.ceiling = 10f32.powf(value as f32 / 20.0),
            _ => return false,
        }
        true
    }
    fn reset(&mut self) {
        *self = Self::new(self.rate, 0.0, self.look as f64 * 1000.0 / self.rate, 100.0);
    }
    fn latency(&self) -> usize {
        self.look
    }
}

fn make_lookahead(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    Box::new(LookAhead::new(
        rate,
        param(params, "ceiling_db", 0.0),
        param(params, "lookahead_ms", 5.0),
        param(params, "release_ms", 100.0),
    ))
}

/// The look-ahead reference, as a `Factory` for the pair runner.
pub fn lookahead_factory() -> Factory {
    make_lookahead
}

pub fn ours_factory() -> Factory {
    make_ours
}

/// Quality of one limiter: the static curve, true-peak overshoot, alias, THD+N, noise, denormal
/// tail, and attack and release times.
pub fn quality(make: Factory, rate: f64, block: usize, params: &[(&str, f64)]) -> Quality {
    // Drive at +6 dBFS so the ceiling is actually active: below it both limiters are transparent.
    let drive = measure::run(measure::build(make, rate, params).as_mut(), &crate::signal::sine(rate, 1000.0, 2.0, rate as usize), block);
    let peak = measure::true_peak_db(&drive[drive.len() / 2..]);
    let alias_out = measure::run(measure::build(make, rate, params).as_mut(), &crate::signal::sine(rate, 5000.0, 2.0, rate as usize), block);
    let thd = measure::thd_n_db(&measure::run(measure::build(make, rate, params).as_mut(), &crate::signal::sine(rate, 1000.0, 2.0, rate as usize), block), rate, 1000.0);
    let (subnormal, spike) = measure::denormal_tail(make, rate, params, block);
    let mod_out = measure::modulated_run(make, rate, params, "release_ms", 20.0, 400.0, block);
    let (stable, _) = measure::is_stable(&mod_out);
    Quality {
        alias_db: measure::alias_db(&alias_out, rate, 5000.0),
        thd_n_db: thd,
        noise_dbfs: measure::noise_floor_dbfs(make, rate, params, block),
        subnormal,
        denormal_spike: spike,
        stable,
        peak_dbfs: peak,
        latency: measure::latency_frames(make, rate, params),
        ns_per_frame: measure::cpu_ns_per_frame(make, rate, params, block, 5),
        instr_per_frame: None,
    }
}

pub fn run(band: &Band, ours: Impl, cand: Impl, rate: f64, block: usize) -> PairReport {
    let qo = quality(ours.make, rate, block, &[("release_ms", 100.0)]);
    let qc = quality(cand.make, rate, block, &[("ceiling_db", 0.0), ("lookahead_ms", 5.0), ("release_ms", 100.0)]);
    let sides = vec![
        side(band, ours.label, qo, None, &qc, "no Kontakt limiter capture; absolute quality only".into()),
        side(band, cand.label, qc, None, &qo, "no Kontakt limiter capture; absolute quality only".into()),
    ];
    PairReport { id: "limiter", title: "Limiter: DbLimiter (ours) vs look-ahead reference", reference: "none (no Kontakt limiter capture)".into(), rate, block, sides }
}

pub fn ours_impl() -> Impl {
    Impl { label: "sampler-core DbLimiter", make: make_ours }
}

pub fn cand_impl() -> Option<Impl> {
    Some(Impl { label: "look-ahead reference (bench)", make: make_lookahead })
}
