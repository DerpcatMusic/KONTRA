//! Kontakt Psyche Delay (0x63; Kontakt 8.13.1 process `0x140aa3c30`, kernel
//! `0x140aadb50`, apply `0x140abafb0`, reset `0x140ab7fb0`): a grain pitch
//! shifter, two crossfaded heads per side, f32 like native.
//! - time: `x = min(ms·0x3a03126f, 1)` round-trips the curve-3 time object
//!   (limits `f32(0x4479ffff/2000)`..`f32(0x4479ffff/5)` Hz, inverted; f64
//!   note law `ln(f/440)·12/ln2 + 69`), `D = f32(x_rt)·2000` ms.
//! - ratio `r = lut((semis + det - 0x42118155)·5 + 0x44acbc36)` on the table
//!   `0x14a6b2600` (`2^(i/60 - 20)`, built `0x1404d14e4`); head rates
//!   `1 ∓ r`; per side `s = |rate·500|`, `q = max(s/D, 0.5)`, window phase
//!   step `(4/sr)·q`, restart `(D - (s/q)·sign)·(sr·0.001)` frames.
//! - ring `2 << int(log2(sr·0.001·6000))` frames; heads read with the 6-tap
//!   cubic kernel stretched by `1/r` (linear above 88.2 kHz).
//!
//! The 8-control smoother (`+0x1c0`, 32-frame tick) never ramps here: the
//! saved state is static.

use super::{Planar, ProcessorState, TimeMod};
use sampler_ir::fx::timemod::PsycheDelay;

/// Ring frames.
pub(super) fn ring(rate: f64) -> usize {
    let frames = rate as f32 * 0.001 * 6000.0;
    2 << (f64::from(frames).ln() * std::f64::consts::LOG2_E) as u32
}

/// Ring plus the four heads' `[offset, fraction]`.
pub(super) fn capacity(rate: f64) -> usize {
    ring(rate) + 4
}

/// Table `0x14a6b2600` lookup.
fn lut(s: f32) -> f32 {
    let v = |i: usize| (2f64.powf(i as f64 / 60.0 - 20.0)) as f32;
    if s < 0.0 {
        0.0
    } else if s >= 2400.0 {
        v(2400)
    } else {
        let i = s as usize;
        (s - i as f32) * (v(i + 1) - v(i)) + v(i)
    }
}

/// Delay milliseconds after the time object's round trip.
fn delay_ms(ms: f32) -> f32 {
    let x = (ms * f32::from_bits(0x3a03_126f)).min(1.0);
    let k = f32::from_bits(0x4479_ffff);
    let (lo, hi) = (f64::from(k / 2000.0), f64::from(k / 5.0));
    let note = |f: f64| (f * (1.0 / 440.0)).ln() * (12.0 / std::f64::consts::LN_2) + 69.0;
    let (nlo, nhi) = (note(lo), note(hi));
    let n = (nhi - nlo) * (1.0 - f64::from(x)) + nlo;
    let hz = 2f64.powf((n - 69.0) * (1.0 / 12.0)) * 440.0;
    let r = (1.0 - (note(hz) - nlo) / (nhi - nlo)).clamp(0.0, 1.0);
    r as f32 * 2000.0
}

/// Apply step `0x140abafb0`.
pub(super) struct Controls {
    pub ratio: [f32; 2],
    /// c2 cross, c3 feedback; head rates c4, c5, c6 (c6 for both right heads).
    pub cross: f32,
    pub feedback: f32,
    pub rates: [f32; 3],
    pub step: [f32; 2],
    pub restart: [f32; 2],
}

pub(super) fn controls(p: &PsycheDelay, rate: f32, tempo: f32) -> Controls {
    let ms = if p.unit > f32::from_bits(0x3089_705f) {
        // Getter 0x140a74bf0; the sub-object's min/max are unknown, the
        // time object caps at 2000 ms.
        let tempo = if tempo < 0.1 { 120.0 } else { tempo };
        ((p.unit * 60.0) / tempo) * p.time.max(1.0) * 1000.0
    } else {
        p.time
    };
    let d = delay_ms(ms.max(0.0));
    let semis = (p.pitch * 24.0 - 12.0).round_ties_even();
    let det = p.detune - 0.5;
    let at = |s: f32| lut((s - f32::from_bits(0x4211_8155)) * 5.0 + f32::from_bits(0x44ac_bc36));
    let r1 = at(semis + det);
    let r2 = if p.detune_stereo { at(semis) } else { r1 };
    let c4 = 1.0 + if p.reverse { r1 } else { -r1 };
    let c6 = 1.0 + if p.reverse == p.reverse_stereo { -r2 } else { r2 };
    let side = |c: f32, d: f32| {
        let s = (c * 500.0).abs();
        let w = if c * 500.0 < 0.0 { -1.0 } else { 1.0 };
        let q = (s / d).max(0.5);
        ((4.0 / rate) * q, (d - (s / q) * w) * (rate * 0.001))
    };
    let (sl, rl) = side(c4, d);
    let (sr, rr) = side(c6, d * (1.0 - p.offset * 0.5));
    Controls { ratio: [r1, r2], cross: p.cross, feedback: p.feedback * f32::from_bits(0x3f7a_e148), rates: [c4, c4, c6], step: [sl, sr], restart: [rl, rr] }
}

/// The cubic kernel.
fn kernel(t: f32) -> f32 {
    let b = f32::from_bits;
    if t < 1.0 {
        t * t * (t * b(0x3faa_aaab) - b(0x4015_5555)) + 1.0
    } else if t < 2.0 {
        ((3.0 - t * b(0x3f15_5555)) * t - b(0x409d_5555)) * t + b(0x4020_0000)
    } else {
        ((t * b(0x3daa_aaab) - b(0x3f2a_aaab)) * t + b(0x3fe0_0000)) * t - b(0x3fc0_0000)
    }
}

impl TimeMod {
    pub(super) fn psyche_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, p: PsycheDelay) -> bool {
        let n = ring(self.rate);
        let mask = n - 1;
        let c = controls(&p, self.rate as f32, tempo as f32);
        let restart = [c.restart[0], c.restart[0], c.restart[1], c.restart[1]];
        let rates = [c.rates[0], c.rates[1], c.rates[2], c.rates[2]];
        let stretch = |r: f32| if r > 1.0 { Some(1.0 / r) } else { None };
        let sc_l = stretch(c.ratio[0]).or(stretch(c.ratio[1])).unwrap_or(1.0);
        let sc = [sc_l, sc_l, stretch(c.ratio[1]).unwrap_or(1.0), stretch(c.ratio[1]).unwrap_or(1.0)];
        let linear = self.rate as f32 > 88_200.0;
        // aux: 0-1 window phases, 2-3 signs, 4-5 feedback, 6 write, 7 reset,
        // 8-11 heads active; slab[n + k] = [offset, fraction].
        let a = &mut state.aux;
        if a[7] != 1.0 {
            // Reset 0x140ab7fb0.
            a[..12].copy_from_slice(&[0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
            for (k, r) in restart.iter().enumerate() {
                slab[n + k] = [f64::from(*r as i32), f64::from(r - (*r as i32) as f32)];
            }
        }
        let mut ph = [a[0] as f32, a[1] as f32];
        let mut sg = [a[2] as f32, a[3] as f32];
        let (mut fb_l, mut fb_r) = (a[4] as f32, a[5] as f32);
        let mut w = a[6] as usize;
        let mut act = [a[8] != 0.0, a[9] != 0.0, a[10] != 0.0, a[11] != 0.0];
        let mut off: [i64; 4] = std::array::from_fn(|k| slab[n + k][0] as i64);
        let mut frac: [f32; 4] = std::array::from_fn(|k| slab[n + k][1] as f32);
        let half = f32::from_bits(0x3eff_f2e5);
        let mut finite = true;
        let [left, right] = block;
        for f in 0..len {
            for j in 0..2 {
                ph[j] += c.step[j];
                if ph[j] > 1.0 {
                    ph[j] -= 2.0;
                    sg[j] = -sg[j];
                }
            }
            let (va, vb) = ((ph[0] * ph[0] - 1.0) * sg[0], (ph[1] * ph[1] - 1.0) * sg[1]);
            let win = [va + half, -va + half, vb + half, -vb + half];
            for k in 0..4 {
                if win[k] <= 0.0 {
                    act[k] = false;
                } else if !act[k] {
                    let r = restart[k];
                    off[k] = i64::from(r as i32);
                    frac[k] = r - (r as i32) as f32;
                    act[k] = true;
                }
            }
            let (x_l, x_r) = (left[f] as f32, right[f] as f32);
            let wl = ((fb_r - fb_l) * c.cross + fb_l) * c.feedback + x_l;
            let wr = ((fb_l - fb_r) * c.cross + fb_r) * c.feedback + x_r;
            for k in 0..4 {
                let a = rates[k] + frac[k];
                let t = a as i32;
                let m = i32::from(a < t as f32);
                frac[k] = a - (t as f32 - m as f32);
                off[k] += i64::from(t - m);
            }
            let old = w as i64;
            w = (w + 1) & mask;
            slab[w] = [f64::from(wl), f64::from(wr)];
            let at = |i: i64, ch: usize| slab[(i as usize) & mask][ch] as f32;
            let mut h = [0f32; 4];
            for k in 0..4 {
                let ch = k >> 1;
                let rd = old - off[k];
                h[k] = if linear {
                    let s0 = at(rd, ch);
                    (at(rd - 1, ch) - s0) * frac[k] + s0
                } else {
                    let s = sc[k];
                    let mut acc = 0f32;
                    let (mut t, mut i) = (s * frac[k], rd);
                    while t < 3.0 {
                        i += 1;
                        acc += kernel(t) * at(i, ch);
                        t += s;
                    }
                    let (mut t, mut i) = ((1.0 - frac[k]) * s, rd);
                    while t < 3.0 {
                        acc += kernel(t) * at(i, ch);
                        i -= 1;
                        t += s;
                    }
                    s * acc
                };
            }
            let y_l = win[1].max(0.0) * h[1] + win[0].max(0.0) * h[0];
            let y_r = win[3].max(0.0) * h[3] + win[2].max(0.0) * h[2];
            (fb_l, fb_r) = (y_l, y_r);
            left[f] = f64::from(y_l) * self.cfg.wet + self.cfg.dry * f64::from(x_l);
            right[f] = f64::from(y_r) * self.cfg.wet + self.cfg.dry * f64::from(x_r);
            finite &= y_l.is_finite() && y_r.is_finite();
        }
        let a = &mut state.aux;
        a[..7].copy_from_slice(&[f64::from(ph[0]), f64::from(ph[1]), f64::from(sg[0]), f64::from(sg[1]), f64::from(fb_l), f64::from(fb_r), w as f64]);
        for k in 0..4 {
            a[8 + k] = f64::from(u8::from(act[k]));
            slab[n + k] = [off[k] as f64, f64::from(frac[k])];
        }
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};

    fn vector() -> PsycheDelay {
        PsycheDelay { time: 20.0, unit: -1.0, feedback: 0.5, detune: 0.7, pitch: 0.75, offset: 0.6, cross: 0.25, reverse_stereo: false, detune_stereo: true, reverse: false }
    }

    /// Apply `0x140abafb0` at 48 kHz.
    #[test]
    fn controls_match_native_bits() {
        let c = controls(&vector(), 48_000.0, 120.0);
        let b = f32::to_bits;
        assert_eq!(b(delay_ms(20.0)), 0x41a00001);
        assert_eq!(c.ratio.map(b), [0x3fb71f79, 0x3fb50504]);
        assert_eq!([b(c.rates[0]), b(c.rates[2]), b(c.feedback)], [0xbedc7de4, 0xbed41410, 0x3efae148]);
        assert_eq!(c.step.map(b), [0x3a6b30f1, 0x3aa19561]);
        assert_eq!(c.restart.map(b), [0x44f00003, 0x44a80002]);
    }

    /// Kernel `0x140aadb50` (48 kHz kernel path): impulse 1 on L at frame 0.
    #[test]
    fn impulse_matches_native_frames() {
        let mut line = Line::single(DelayTime::Ms(0.), 2000.);
        line.native = Native::PsycheDelay(vector());
        let cfg = Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry: 0., wet: 1. };
        let mut x = vec![[0.0; 2]; 1910];
        x[0][0] = 1.0;
        let (y, _) = super::super::tests::run(cfg, 48_000, 120., &x, 512);
        let bits = |k: usize| [y[k][0] as f32, y[k][1] as f32].map(f32::to_bits);
        assert_eq!([bits(1339), bits(1340), bits(1341), bits(1346)], [[0x3a660741, 0], [0xbc1d71df, 0], [0x3d646c87, 0], [0x3ed3dfee, 0]]);
        assert_eq!([bits(1898), bits(1899), bits(1900)], [[0, 0x344b1e10], [0, 0xb72cd1b4], [0, 0x39166434]]);
    }
}
