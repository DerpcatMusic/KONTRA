//! Kontakt Twin Delay (0x6B; Kontakt 8.13.1 process `0x140aa73d0`, kernel
//! `0x140ab07d0`, control targets `0x140ac17b0`), f32 like native:
//! - lengths in frames `f32(rate)/f32(1/T)`: time `T = 0.001 + x²·(1/0.49999997
//!   - 0.001)` (f64), predelay `T = 2.0000001·x`; synced, the getter
//!   `0x140a74bf0` gives `ms = factor·60/tempo·max(v, 1)·1000` (clamped to
//!   25000 / 15000), mapped back to `x` (time `√(9 - (1 - ms)·8008)·
//!   0.00025 + 0.00075`, predelay `ms·0.0005`, both at most 1).
//! - a ring of `int(2·rate + 2)` frames of `[line L, line R, pre L, pre R]`;
//!   each tap reads `p = w - c` (wrapped), `i = trunc(p)`, `f = p - i`,
//!   `(x[i-1] - x[i])·f + x[i]`.
//! - per frame: write the input into all four lanes; the predelay taps (with
//!   the time taps' fractions) plus `fb·y + cross·y_other` become the lines;
//!   the four taps read the lines (time fractions for the feedback pair, the
//!   predelay fractions for the output pair); out `W/(1-W)` crossed levels.
//!
//! The control smoother (`+0x1f0`) never ramps here: the saved state is
//! static.

use super::{Planar, ProcessorState, TimeMod};
use sampler_ir::fx::timemod::TwinDelay;

/// Ring frames.
pub(super) fn ring(rate: f64) -> usize {
    (2.0 * rate as f32 + 2.0) as usize
}

/// Slab frames: two per ring frame.
pub(super) fn capacity(rate: f64) -> usize {
    2 * ring(rate)
}

fn frames(rate: f32, t: f64) -> f32 {
    rate / (1.0 / t) as f32
}

/// Time `x` → frames.
fn time_frames(rate: f32, x: f32) -> f32 {
    let x = f64::from(x);
    frames(rate, 0.001 + x * x * (1.0 / f64::from(f32::from_bits(0x3eff_ffff)) - 0.001))
}

/// Predelay `x` → frames.
fn pre_frames(rate: f32, x: f32) -> f32 {
    frames(rate, f64::from(x) * 2.000_000_1)
}

/// The synced value through getter `0x140a74bf0`, back to `x`.
fn synced(factor: f32, v: f32, tempo: f32, time: bool) -> f32 {
    let tempo = if tempo < 0.1 { 120.0 } else { tempo };
    let ms = ((factor * 60.0) / tempo) * v.max(1.0) * 1000.0;
    if time {
        let ms = ms.clamp(0.0, 25_000.0);
        ((9.0 - (1.0 - ms) * 8008.0).sqrt() * f32::from_bits(0x3982_f0e9) + f32::from_bits(0x3a44_695e)).min(1.0)
    } else {
        (ms.clamp(0.0, 15_000.0) * f32::from_bits(0x3a03_126f)).min(1.0)
    }
}

/// The control targets `c0..=c13` (`0x140ac17b0`).
pub(super) fn controls(t: &TwinDelay, rate: f32, tempo: f32) -> [f32; 14] {
    let set = |u: f32| u > f32::from_bits(0x3089_705f);
    let time = [0, 1].map(|c| time_frames(rate, if set(t.time_unit[c]) { synced(t.time_unit[c], t.time[c], tempo, true) } else { t.time[c] }));
    let pre = [0, 1].map(|c| pre_frames(rate, if set(t.predelay_unit[c]) { synced(t.predelay_unit[c], t.predelay[c], tempo, false) } else { t.predelay[c] }));
    let cross = t.cross.min(1.0 - t.feedback[0]).min(1.0 - t.feedback[1]);
    [time[0], time[1], pre[0], pre[1], t.feedback[0], t.feedback[1], t.level[0], t.level[1], cross, 1.0, t.width, 1.0 - t.width, 0.0, 1.0]
}

impl TimeMod {
    pub(super) fn twin_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, t: TwinDelay) -> bool {
        let n = ring(self.rate);
        let nf = n as f32;
        let c = controls(&t, self.rate as f32, tempo as f32);
        // aux: 0-1 previous feedback taps, 2 write position.
        let (mut y0, mut y1, mut w) = (state.aux[0] as f32, state.aux[1] as f32, state.aux[2] as usize);
        let lane = |slab: &[[f64; 2]], k: usize, l: usize| slab[2 * k + l / 2][l % 2] as f32;
        let mut finite = true;
        let [left, right] = block;
        for f in 0..len {
            if w == n {
                w = 0;
            }
            let tap = |k: usize| {
                let mut p = w as f32 - c[k];
                if nf <= p {
                    p -= nf;
                }
                if p < 0.0 {
                    p = (p + nf).clamp(0.0, nf - 1.0);
                }
                let i = p as usize;
                (i, if i == 0 { n - 1 } else { i - 1 }, p - i as f32)
            };
            let taps = [tap(0), tap(1), tap(2), tap(3)];
            let read = |slab: &[[f64; 2]], (i, j, _): (usize, usize, f32), l: usize, f: f32| {
                let (a, b) = (lane(slab, i, l), lane(slab, j, l));
                (b - a) * f + a
            };
            let x = [left[f] as f32, right[f] as f32];
            let inp = [c[13] * x[0] * c[9], c[13] * x[1] * c[9]];
            slab[2 * w] = [f64::from(inp[0]), f64::from(inp[1])];
            slab[2 * w + 1] = [f64::from(inp[0]), f64::from(inp[1])];
            let pre_l = read(slab, taps[2], 2, taps[0].2);
            let pre_r = read(slab, taps[3], 3, taps[1].2);
            slab[2 * w][0] = f64::from(pre_l + c[4] * y0 + c[8] * y1);
            slab[2 * w][1] = f64::from(pre_r + c[5] * y1 + c[8] * y0);
            y0 = read(slab, taps[0], 0, taps[0].2);
            y1 = read(slab, taps[1], 1, taps[1].2);
            let y2 = read(slab, taps[0], 0, taps[2].2);
            let y3 = read(slab, taps[1], 1, taps[3].2);
            w += 1;
            let ol = (c[11] * c[7] * y3 + c[10] * c[6] * y2) * c[9] + c[12] * x[0];
            let or = (c[10] * c[7] * y3 + c[11] * c[6] * y2) * c[9] + c[12] * x[1];
            left[f] = f64::from(ol) * self.cfg.wet + self.cfg.dry * f64::from(x[0]);
            right[f] = f64::from(or) * self.cfg.wet + self.cfg.dry * f64::from(x[1]);
            finite &= ol.is_finite() && or.is_finite();
        }
        (state.aux[0], state.aux[1], state.aux[2]) = (f64::from(y0), f64::from(y1), w as f64);
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};

    fn vector() -> TwinDelay {
        TwinDelay {
            time: [0.0001, 0.00021],
            predelay: [0.001, 0.0013],
            time_unit: [-1.0; 2],
            predelay_unit: [-1.0; 2],
            feedback: [0.6, 0.5],
            level: [0.9, 0.7],
            width: 0.8,
            cross: 0.3,
        }
    }

    /// `0x140ac17b0` targets at 48 kHz.
    #[test]
    fn controls_match_native_bits() {
        let c = controls(&vector(), 48_000.0, 120.0);
        assert_eq!([c[0], c[1], c[2], c[3], c[8], c[11]].map(f32::to_bits), [0x424000fc, 0x42400455, 0x42c00002, 0x42f9999b, 0x3e99999a, 0x3e4ccccc]);
    }

    /// Kernel `0x140ab07d0`: impulse L 1 at frame 0, R 0.5 at frame 3.
    #[test]
    fn impulse_matches_native_frames() {
        let mut line = Line::single(DelayTime::Ms(0.), 2000.);
        line.native = Native::TwinDelay(vector());
        let cfg = Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry: 0., wet: 1. };
        let mut x = vec![[0.0; 2]; 200];
        x[0][0] = 1.0;
        x[3][1] = 0.5;
        let (y, _) = super::super::tests::run(cfg, 48_000, 120., &x, 64);
        let bits = |k: usize| [y[k][0] as f32, y[k][1] as f32].map(f32::to_bits);
        assert_eq!(bits(146), [0x323570a3, 0x313570a2]);
        assert_eq!(bits(148), [0x3f3823d7, 0x3e3823d6]);
    }
}
