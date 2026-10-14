//! UVI legacy delays (Workstation 4.0.9), f32 like native:
//! - delay `d = rate·t` (synced: `59.9999962/tempo·beats`, `0x141e29f48`,
//!   capped at 10 s, `0x141cfab90`); ring `N = ⌊d⌋ + 2` frames per line,
//!   cleared with the loop filter whenever `N` changes.
//! - SimpleDelay (`0x14124f060`): one line per channel read at `w - d` by
//!   Catmull-Rom (2.5, 1.5, 0.5), write `F(fb·y) + x`, out `y·m + x·(1-m)`.
//! - StereoDelay (`0x1412512c0`, line `0x141251e70`): left and right lines
//!   with their own times, read at `w - N`, i.e. exactly `N` frames.
//! - PingPongDelay (`0x14124c290`, core `0x14124d730`): one `N` from the
//!   time, read at `w - N`; input `s = L + R`, `k = 0.5 - 0.5·Spread`, line 0
//!   writes `F(fb·y1) + s·k`, line 1 `F(fb·y0) + s·(1-k)`.
//! - F: the legacy cutoff biquad (`0x14156b1e0` → `0x141243ec0`), DF-I.

use super::{Planar, ProcessorState, TimeMod};
use crate::dsp::kernels::uvi_legacy_cutoff;
use sampler_ir::fx::timemod::{UviLegacyDelay, UviLegacyKind};

/// Ring frames for `d`: `⌊d⌋ + 2`.
pub(super) fn ring(d: f32) -> usize {
    d.max(0.).floor() as usize + 2
}

/// The longest ring the controls can ask for at `rate`.
pub(super) fn capacity(d: &UviLegacyDelay, rate: f64) -> usize {
    let t = if d.synced { 10. } else { d.time[0].max(d.time[1]).min(10.) };
    ring(rate as f32 * t)
}

fn biquad(c: Option<[f32; 5]>, s: &mut [f64], x: f32) -> f32 {
    let Some([b0, b1, b2, a1, a2]) = c else { return x };
    let [x1, x2, y1, y2] = [s[0], s[1], s[2], s[3]].map(|v| v as f32);
    let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
    s[..4].copy_from_slice(&[x, x1, y, y1].map(f64::from));
    y
}

/// Catmull-Rom at native ring position `pos` (`0 ≤ pos < n`).
fn catmull(ring: &[[f64; 2]], n: usize, c: usize, pos: f32) -> f32 {
    let i = pos as usize;
    let f = pos - i as f32;
    let at = |k: usize| ring[k % n][c] as f32;
    let (p0, p1, p2, p3) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
    ((((p0 - p1 * 2.5) + p2 + p2 - p3 * 0.5) + ((p1 - p2) * 1.5 + (p3 - p0) * 0.5) * f) * f + (p2 - p0) * 0.5) * f + p1
}

impl TimeMod {
    pub(super) fn uvi_legacy_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, d: UviLegacyDelay) -> bool {
        let rate = self.rate as f32;
        let tempo = if tempo < 0.1 { 120. } else { tempo as f32 };
        let seconds = d.time.map(|t| if d.synced { (f32::from_bits(0x426f_ffff) / tempo * t).min(10.) } else { t });
        let shared = d.kind != UviLegacyKind::Stereo;
        let delay = [0, 1].map(|c| rate * seconds[if shared { 0 } else { c }]);
        let n = delay.map(|v| ring(v).min(self.cap));
        let filter = uvi_legacy_cutoff(d.cutoff, rate);
        // aux: 0-1 write positions, 2-3 ring lengths, 4-7 and 8-11 DF-I states.
        for c in 0..2 {
            if state.aux[2 + c] != n[c] as f64 {
                state.aux[2 + c] = n[c] as f64;
                state.aux[c] = 0.;
                state.aux[4 + 4 * c..8 + 4 * c].fill(0.);
                slab[..self.cap].iter_mut().for_each(|f| f[c] = 0.);
            }
        }
        let (dry, wet) = (self.cfg.dry as f32, self.cfg.wet as f32);
        let fb = d.feedback;
        let k = 0.5 - 0.5 * d.spread;
        let mut w = [state.aux[0] as usize, state.aux[1] as usize];
        let mut finite = true;
        let [left, right] = block;
        for f in 0..len {
            let x = [left[f] as f32, right[f] as f32];
            let y: [f32; 2] = std::array::from_fn(|c| match d.kind {
                UviLegacyKind::Simple => {
                    let mut pos = w[c] as f32 - delay[c];
                    if pos < 0. {
                        pos += n[c] as f32;
                    }
                    catmull(slab, n[c], c, pos)
                }
                _ => slab[w[c]][c] as f32,
            });
            let (into, from) = match d.kind {
                UviLegacyKind::PingPong => {
                    let s = x[0] + x[1];
                    ([s * k, s * (1. - k)], [y[1], y[0]])
                }
                _ => (x, y),
            };
            for c in 0..2 {
                let v = biquad(filter, &mut state.aux[4 + 4 * c..8 + 4 * c], fb * from[c]) + into[c];
                slab[w[c]][c] = f64::from(v);
                w[c] = if w[c] + 1 >= n[c] { 0 } else { w[c] + 1 };
            }
            left[f] = f64::from(y[0] * wet + x[0] * dry);
            right[f] = f64::from(y[1] * wet + x[1] * dry);
            finite &= y[0].is_finite() && y[1].is_finite();
        }
        (state.aux[0], state.aux[1]) = (w[0] as f64, w[1] as f64);
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{impulse, run};
    use super::*;
    use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};

    fn unit(kind: UviLegacyKind, time: [f32; 2], feedback: f32, spread: f32) -> Config {
        let mut line = Line::single(DelayTime::Ms(0.), 10_000.);
        line.native = Native::UviLegacyDelay(UviLegacyDelay { kind, time, synced: false, feedback, cutoff: 0., spread });
        Config { address: None, line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry: 0., wet: 1. }
    }

    /// `0x14124f060` Catmull-Rom vector: p = [0, 1, 0, 0] at t = 0.25 reads
    /// 0.8671875.
    #[test]
    fn catmull_matches_the_native_vector() {
        let ring = [[0.; 2], [1.; 2], [0.; 2], [0.; 2], [0.; 2]];
        assert_eq!(catmull(&ring, 5, 0, 1.25), 0.867_187_5);
    }

    /// Simple: echoes every `rate·t` frames, `fb` per pass. Stereo and
    /// PingPong: every `⌊d⌋ + 2`.
    #[test]
    fn echo_positions_follow_the_native_rings() {
        let at = |kind, time, spread| run(unit(kind, time, 0.5, spread), 1000, 120., &impulse(60, [1., 1.]), 64).0;
        let y = at(UviLegacyKind::Simple, [0.010, 0.010], 0.);
        assert_eq!((y[10], y[20], y[9], y[11]), ([1.; 2], [0.5; 2], [0.; 2], [0.; 2]));
        let y = at(UviLegacyKind::Stereo, [0.010, 0.020], 0.);
        assert_eq!((y[12][0], y[22][1], y[24][0], y[44][1]), (1., 1., 0.5, 0.5));
        // Spread -1: L + R all into line 0; the echoes alternate sides.
        let y = at(UviLegacyKind::PingPong, [0.010, 0.], -1.);
        assert_eq!((y[12], y[24], y[36]), ([2., 0.], [0., 1.], [0.5, 0.]));
    }
}
