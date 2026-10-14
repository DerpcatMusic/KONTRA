//! UVI DiffuseDelay (Workstation 4.0.9: process `0x141232700` → core
//! `0x141657890`, path `0x141658680`, prepare `0x1416573e0`), two lanes:
//! - input `x' = x·√(1-Fb)`; line input `u = Fb·R(πX)·y_prev + x'`
//!   (`R` the rotation by CrossTalk·π, sincos `0x14199de20`).
//! - a modulated line (ring `2^⌈log2(rate/6)⌉`): every `P = (int)(rate/Hz)`
//!   frames a new target `T = rand·cents/1200·P` (MT19937 seed 0, canonical
//!   float); `ds += ((k/P)(T - T') + T' - ds)(1 - e^(-2π·2Hz/rate))`; write
//!   `u`, read `(1-f)·x[w-n] + f·x[w-n-1]`.
//! - `n` damped allpasses, `g = Diffusion·0.618034005` (`0x3f1e377a`),
//!   damping `a = e^(-2π(1-Damp)·20000/rate)`: `lp += (z - lp)(1-a)`,
//!   `v = in - g·lp`, write `v`, out `g·v + lp`.
//! - stage lengths (`0x141659880`): `D = (long)(t·rate)`, `r = (2/π)^spread`,
//!   `g_n = (1-r)/(1-r^n)` (`1/n` at spread 0); `n` the last of 2..16 with
//!   `r^(n-1)·D·g_n > 64`; `L[n-1] = g_n·D`, each earlier stage `×r`;
//!   Randomness scales each by `(2u-1)·rand + 1` (MT19937 seeded
//!   `lane·0x5bd1e995`, canonical double) and renormalises to `D`; rounded
//!   half-even, at least 1.
//! - out `y·√Mix + x'·√(1-Mix)` (the unit's dry/wet).

use super::mt::Mt;
use super::{Planar, ProcessorState, TimeMod};
use sampler_ir::fx::timemod::UviDiffuseDelay;

const MAX_STAGES: usize = 16;
const MT_FRAMES: usize = 625;

fn exp2pi(x: f32) -> f32 {
    (-6.283_185_307_179_586 * f64::from(x)).exp() as f32
}

/// Modulation ring frames.
fn mod_ring(rate: f64) -> usize {
    1 << (rate * 0.166_666_664_183_139_83).log2().ceil() as u32
}

/// Slab layout: modulation ring, allpass rings, then per-stage damping
/// states, positions and lengths, then the modulation generator.
struct Layout {
    ring: usize,
    ap: usize,
    lp: usize,
    pos: usize,
    len: usize,
    mt: usize,
}

fn layout(rate: f64) -> Layout {
    let ring = mod_ring(rate);
    let ap = ring;
    let lp = ap + rate as usize + 64;
    Layout { ring, ap, lp, pos: lp + MAX_STAGES, len: lp + 2 * MAX_STAGES, mt: lp + 3 * MAX_STAGES }
}

pub(super) fn capacity(rate: f64) -> usize {
    layout(rate).mt + MT_FRAMES
}

/// Stage lengths before rounding for `n` stages.
pub(super) fn lengths(d: i64, spread: f32, n: usize, randomness: f32, lane: u32) -> Vec<f64> {
    let r = f64::from_bits(0x3fe4_5f30_6dc9_c883).powf(f64::from(spread));
    let g = |n: usize| if spread <= 0.0 { 1.0 / n as f64 } else { (1.0 - r) / (1.0 - r.powf(n as f64)) };
    let mut l = vec![0.0; n];
    l[n - 1] = g(n) * d as f64;
    for k in (0..n - 1).rev() {
        l[k] = l[k + 1] * r;
    }
    if randomness > 0.0 {
        let mut mt = Mt::new(lane.wrapping_mul(0x5bd1_e995));
        for v in &mut l {
            *v *= (2.0 * mt.unit_f64() - 1.0) * f64::from(randomness) + 1.0;
        }
        let sum: f64 = l.iter().sum();
        for v in &mut l {
            *v *= d as f64 / sum;
        }
    }
    l
}

/// The stage count for `D` frames.
pub(super) fn stages(d: i64, spread: f32) -> usize {
    let r = f64::from_bits(0x3fe4_5f30_6dc9_c883).powf(f64::from(spread));
    let mut count = 1;
    for n in 2..=MAX_STAGES {
        let g = if spread <= 0.0 { 1.0 / n as f64 } else { (1.0 - r) / (1.0 - r.powf(n as f64)) };
        if 64.0 < r.powf((n - 1) as f64) * d as f64 * g {
            count = n;
        }
    }
    count
}

impl TimeMod {
    pub(super) fn uvi_diffuse_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, d: UviDiffuseDelay) -> bool {
        let rate = self.rate as f32;
        let lay = layout(self.rate);
        let tempo = if tempo < 0.1 { 120. } else { tempo as f32 };
        let t = if d.synced { (f32::from_bits(0x426f_ffff) / tempo * d.time).clamp(0.01, 1.0) } else { d.time };
        let dl = ((t * rate) as i64).clamp(1, rate as i64);
        // aux: 0 write position, 1 counter, 2 period, 3-4 target and previous,
        // 5 smoothed delay, 6-7 previous outputs, 8 D, 9 stage count. The slab
        // flag marks built lengths (a reset zeroes the slab).
        let built = slab[lay.len + MAX_STAGES - 1][1] == 1.0;
        if !built || state.aux[8] != dl as f64 {
            let n = stages(dl, d.spread);
            for lane in 0..2 {
                let ls = lengths(dl, d.spread, n, d.randomness, lane as u32);
                for (k, v) in ls.iter().enumerate() {
                    slab[lay.len + k][lane] = v.round_ties_even().max(1.0);
                    slab[lay.pos + k][lane] = 0.0;
                    slab[lay.lp + k][lane] = 0.0;
                }
            }
            slab[lay.ap..lay.lp].fill([0.0; 2]);
            if !built {
                let mt = Mt::new(0);
                for (k, v) in mt.s.iter().enumerate() {
                    slab[lay.mt + k][0] = f64::from(*v);
                }
                slab[lay.mt + 624][0] = 624.0;
                slab[..lay.ap].fill([0.0; 2]);
                state.aux[..8].fill(0.0);
            }
            (state.aux[8], state.aux[9]) = (dl as f64, n as f64);
            slab[lay.len + MAX_STAGES - 1][1] = 1.0;
        }
        let n = state.aux[9] as usize;
        let len_of = |slab: &[[f64; 2]], k: usize, c: usize| slab[lay.len + k][c] as usize;
        let fb = d.feedback;
        let pre = (1.0 - fb).sqrt();
        let g = d.diffusion * f32::from_bits(0x3f1e_377a);
        let a = exp2pi((1.0 - d.damp) * 20_000.0 / rate);
        let (s, c) = (d.crosstalk * std::f32::consts::PI).sin_cos();
        let smooth = exp2pi((d.mod_rate + d.mod_rate) / rate);
        let (dry, wet) = (self.cfg.dry as f32, self.cfg.wet as f32);
        let mask = lay.ring - 1;
        let [mut w, mut cnt, mut period, mut target, mut prev, mut ds, y0, y1] = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| state.aux[i]);
        let mut y = [y0 as f32, y1 as f32];
        let mut finite = true;
        let [left, right] = block;
        for f in 0..len {
            let x = [left[f] as f32 * pre, right[f] as f32 * pre];
            let u = [fb * (c * y[0] + s * y[1]) + x[0], fb * (-s * y[0] + c * y[1]) + x[1]];
            // Modulation: one generator, the same draw for both lanes.
            if cnt >= period {
                let p = (rate / d.mod_rate) as i64;
                let mut mt = Mt { s: std::array::from_fn(|k| slab[lay.mt + k][0] as u32), i: slab[lay.mt + 624][0] as usize };
                let r = mt.unit_f32();
                for (k, v) in mt.s.iter().enumerate() {
                    slab[lay.mt + k][0] = f64::from(*v);
                }
                slab[lay.mt + 624][0] = mt.i as f64;
                (cnt, period, prev) = (0.0, p as f64, target);
                target = f64::from(r * d.mod_depth * f32::from_bits(0x3a5a_740e) * p as f32);
            }
            let glide = (cnt as f32 / period as f32) * (target as f32 - prev as f32) + prev as f32;
            cnt += 1.0;
            let dsf = (glide - ds as f32) * (1.0 - smooth) + ds as f32;
            ds = f64::from(dsf);
            let wi = w as usize;
            let whole = dsf.floor();
            let (nn, fr) = (whole as usize, dsf - whole);
            for lane in 0..2 {
                slab[wi][lane] = f64::from(u[lane]);
                let at = |k: usize| slab[wi.wrapping_sub(k) & mask][lane] as f32;
                let mut v = (1.0 - fr) * at(nn) + fr * at(nn + 1);
                let mut start = lay.ap;
                for k in 0..n {
                    let l = len_of(slab, k, lane);
                    let p = slab[lay.pos + k][lane] as usize;
                    let z = slab[start + p][lane] as f32;
                    let lp = slab[lay.lp + k][lane] as f32;
                    let lp = lp + (z - lp) * (1.0 - a);
                    let o = v - g * lp;
                    slab[start + p][lane] = f64::from(o);
                    v = g * o + lp;
                    slab[lay.lp + k][lane] = f64::from(lp);
                    slab[lay.pos + k][lane] = if p + 1 == l { 0.0 } else { (p + 1) as f64 };
                    start += l;
                }
                y[lane] = v;
            }
            w = ((wi + 1) & mask) as f64;
            left[f] = f64::from(y[0] * wet + x[0] * dry);
            right[f] = f64::from(y[1] * wet + x[1] * dry);
            finite &= y[0].is_finite() && y[1].is_finite();
        }
        state.aux[..8].copy_from_slice(&[w, cnt, period, target, prev, ds, f64::from(y[0]), f64::from(y[1])]);
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x141659880` vectors: D 6000, spread 1, seven stages.
    #[test]
    fn stage_lengths_match_native_vectors() {
        let l = lengths(6000, 1.0, 7, 0.0, 0);
        assert!((l[0] - 151.565_399_651_902_62).abs() < 1e-9 && (l[1] - 238.078_373_042_409_1).abs() < 1e-9 && (l[6] - 2_276.770_896_019_708).abs() < 1e-9, "{l:?}");
        let r0 = lengths(6000, 1.0, 7, 0.5, 0);
        let r1 = lengths(6000, 1.0, 7, 0.5, 1);
        assert!((r0[0] - 167.894).abs() < 1e-3 && (r0[6] - 1840.543).abs() < 1e-3, "{r0:?}");
        assert!((r1[0] - 161.416).abs() < 1e-3 && (r1[6] - 1767.850).abs() < 1e-3, "{r1:?}");
    }

    /// Feedback 0, Diffusion 0, ModDepth 0: the dry path is `x·√(1-Mix)` and
    /// the wet one the line at zero delay through gain-0 allpasses, i.e. the
    /// input delayed by the sum of the stage lengths.
    #[test]
    fn plain_path_delays_by_the_stage_lengths() {
        use super::super::tests::{impulse, run};
        use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};
        let d = UviDiffuseDelay { time: 0.125, synced: false, spread: 1.0, diffusion: 0.0, feedback: 0.0, damp: 0.0, crosstalk: 0.0, randomness: 0.0, mod_rate: 1.0, mod_depth: 0.0 };
        let mut line = Line::single(DelayTime::Ms(0.), 1000.);
        line.native = Native::UviDiffuseDelay(d);
        let cfg = Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry: 0., wet: 1. };
        let (y, _) = run(cfg, 48_000, 120., &impulse(7000, [1., 1.]), 64);
        let n = stages(6000, 1.0);
        let total: f64 = lengths(6000, 1.0, n, 0.0, 0).iter().map(|v| v.round_ties_even()).sum();
        assert_eq!(y.iter().position(|v| v[0] != 0.0), Some(total as usize), "{n}");
    }
}
