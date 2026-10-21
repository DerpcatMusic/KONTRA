//! 3x2 Versatile and Pro-53: configurations of the explicit (Chamberlin) stage in
//! [`super::linear`]. The stage laws (cutoff, damping, morph weights, wet, gain normalisation)
//! live here; the recurrence and its nonlinear feedback are shared with every explicit section.
//! Arithmetic order is the native order; the recorded stage vectors in `cascade.rs` pin it.

use super::cascade::{clamp01, note_hertz};
use super::linear::Explicit;
use super::shaper::Soft;

/// Up to three explicit stages in series, a soft-clipped feedback of the last low pass subtracted
/// from the input, and an output gain smoothed toward `gain` by `smooth` per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Chamberlin {
    pub stages: [Explicit; 3],
    pub count: usize,
    pub feedback: Soft,
    pub gain: f32,
    pub smooth: f32,
}

/// Pro-53 (`FUN_14095d940` setter, `FUN_1409099a0` process): two stages, the second fed the
/// first's low pass scaled by its damping, `NL(l2)` fed back.
pub(super) fn pro52(cutoff: f32, resonance: f32, rate: f32) -> Chamberlin {
    let r = clamp01(resonance);
    let k = if r >= 0.5 { r * 0.48 + 0.46 } else { r * 1.16 + 0.12 };
    let limit = (r * 0.293) * (r + 1.3075) + 0.7352;
    let g = (note_hertz(cutoff) * (f32::from_bits(0x40c90e56) / rate)).clamp(0.0, limit);
    let k2 = k * 0.8;
    let d2 = 2.0 - (k2 + k2);
    let low = |d, k, eps| Explicit { g, d, k, eps, weights: [1.0, 0.0, 0.0], wet: 1.0, ..Explicit::default() };
    Chamberlin {
        stages: [low(2.0 - (k + k), 1.0, 1e-7), low(d2, d2, 0.0), Explicit::default()],
        count: 2,
        feedback: Soft { a: 0.133_35, c: 0.016_668_75 },
        gain: 1.0,
        smooth: 1.0,
    }
}

/// 3x2 Versatile (`FUN_14095db50` cutoff, `FUN_140966230`.. resonances, assignment
/// `0x14096490b`, normaliser `FUN_14096f840`, process `FUN_140aef6e0`): three stages with the
/// band-state shaper, morphing LP -> BP -> HP, each blended by its wet amount.
pub(super) fn versatile(v: &sampler_ir::fx::filter::Versatile, cutoff: f32, resonance: f32, rate: f32) -> Chamberlin {
    let k = (1.0 / f64::from(rate) * f64::from_bits(0x401921cac083126f)) as f32;
    let note = clamp01(cutoff) * 111.0 + 20.0;
    let shift = v.shift.map(|x| clamp01(x) * 80.0);
    let n1 = shift[0] + note;
    let notes = [note, n1, n1 + shift[1]];
    let res = [clamp01(resonance), clamp01(v.resonance[0]), clamp01(v.resonance[1])];
    let wet = v.wet.map(clamp01);
    let mut stages = [Explicit::default(); 3];
    let mut comp = [0f32; 3];
    for i in 0..3 {
        let r = res[i];
        let q = if r > 0.5 { (f64::from(r - 0.5) * 0.66 + 0.65) as f32 } else { (f64::from(r) * 1.3) as f32 };
        let limit = ((f64::from(q) + 1.3075) * f64::from(q) * 0.293 + 0.7352) as f32;
        let arg = (notes[i] - 69.0) * 0.083_333_336;
        let g = ((f64::from(arg).exp2() * 440.0) as f32 * k).min(limit);
        let t = clamp01(v.types[i]);
        let weights = if t < 0.5 {
            let a = 0.5 - t;
            [a + a, t + t, 0.0]
        } else {
            let a = 1.0 - t;
            [0.0, a + a, ((t - 0.5) + t) - 0.5]
        };
        comp[i] = (f64::from(wet[i]) / (f64::from(1.0 - r) * 1.4 + 0.05) + f64::from(1.0 - wet[i])) as f32;
        stages[i] = Explicit { g, d: 2.0 - (q + q), k: 1.0, eps: 0.0, shaper: Soft { a: 0.06, c: 0.0075 }, weights, wet: wet[i] };
    }
    let [fa, fb, fc] = stages.map(|s| s.g);
    let [a1, b1, c1] = comp.map(|c| c - 1.0);
    let inv = 1.0 / fa;
    let t1 = (a1 * b1) / ((fb * inv) * 6.0 - 5.0);
    let t2 = (c1 * b1) / ((fc / fb) * 6.0 - 5.0);
    let t3 = (c1 * a1) / ((fc * inv) * 6.0 - 5.0);
    let norm = (3.0 / f64::from((((t1 + t2) + t3) + comp[1]) + (comp[2] + comp[0]))) as f32;
    let gain = 10f64.powf(f64::from((clamp01(v.gain) - 0.5) * 3.0)) as f32;
    Chamberlin { stages, count: 3, feedback: Soft::default(), gain: gain * norm, smooth: (1.0 / f64::from(rate) * 20.0) as f32 }
}

impl Chamberlin {
    /// One frame. `state = [b, l] x 3, feedback, smoothed gain`.
    #[inline(always)]
    pub(super) fn tick(&self, state: &mut [f32; 8], input: f32) -> f32 {
        let mut y = input - state[6];
        let mut low = 0.0;
        for (i, st) in self.stages[..self.count].iter().enumerate() {
            let (out, l) = st.tick(&mut state[2 * i..2 * i + 2], y);
            y = out;
            low = l;
        }
        state[6] = self.feedback.apply(low);
        state[7] += (self.gain - state[7]) * self.smooth;
        state[7] * y
    }
}
