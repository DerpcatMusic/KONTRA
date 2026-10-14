//! UVI TapeEcho (Workstation 4.0.9: process `0x141254780`, core process
//! `0x141663e40`, prepare `0x141663c30`), f32 like native:
//! - a mono tape loop of `⌈0.7·rate⌉ + 10` frames; the record head sits 5
//!   frames before the end, each play head `clamp(d, 0.05, 0.5)·rate` behind,
//!   the erase head at 0; every head moves `v` frames per sample.
//! - speed: `S += (Speed - S)(1 - e^(-2π·0.6666667/rate))` (0 when TapeStop);
//!   `v = S·(1 + 0.005·(0.2·n₂ + 0.5555556·(0.8·wow + flutter) - 0.1))`,
//!   wow and flutter the UVI sine table at `Speed·1.7` and `Speed·9.4` Hz.
//! - heads read linearly and mix by Mode; one-poles at 5000, 14000 and
//!   400 Hz give `D = B - C`; de-emphasis shelf → the wet sample.
//! - record: `tanh(pre-emphasis(y·Feedback + (2N·n₁ - N + m)(1 + 0.005·(0.2·
//!   flutter + 0.8·wow))))`, smoothed by `e^(-2π·min(S/2, 1/2))`, written
//!   between the old and new record positions with linear weights after the
//!   erase head clears its span. `n₁, n₂`: MT19937 seed 42, canonical float.
//! - wet EQ: bass shelf at 100 Hz and treble shelf at 5000 Hz in one biquad.
//!
//! Modes 5..=12 also feed a convolution reverb whose impulse response is
//! embedded in the UVI binary; it is not reproduced (reported by the mapper).

use super::mt::Mt;
use super::{Planar, ProcessorState, TimeMod};
use crate::blocks::lfo::uvi_sine;
use sampler_ir::fx::timemod::UviTapeEcho;

fn exp2pi(x: f32) -> f32 {
    (-6.283_185_307_179_586 * f64::from(x)).exp() as f32
}

/// First-order shelf `0x141649120` (f64): DC gain `ga`, Nyquist gain `gb`,
/// as `b0 b1 a1` for `y = b0·x + s; s = b1·x - a1·y`.
pub(super) fn shelf(f: f64, ga: f64, gb: f64) -> [f32; 3] {
    let r = ga.max(1e-20) / gb.max(1e-20);
    let t = (std::f64::consts::PI * f).tan() / r.sqrt();
    let u = t * r;
    let k = 1.0 / (t + 1.0);
    let b0 = k * (u + 1.0) * gb;
    [b0 as f32, ((u - 1.0) / (u + 1.0) * b0) as f32, ((t - 1.0) * k) as f32]
}

/// Loop frames.
pub(super) fn ring(rate: f64) -> usize {
    (0.7f32 * rate as f32).ceil() as usize + 10
}

pub(super) fn capacity(rate: f64) -> usize {
    ring(rate).max(SC + 32)
}

/// Column 1: the generator (625 frames), then scalar state from `SC`.
const SC: usize = 625;
const INIT: usize = SC;
const HEADS: usize = SC + 1;
const ERASE: usize = SC + 4;
const WRITE: usize = SC + 6;
const W: usize = SC + 8;
const SPEED: usize = SC + 10;
const LP: usize = SC + 11;
const DE: usize = SC + 14;
const PRE: usize = SC + 15;
const EQ: usize = SC + 16;
const PHASE: usize = SC + 18;
const TIMES: usize = SC + 20;

impl TimeMod {
    pub(super) fn uvi_tape_echo(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, t: UviTapeEcho) -> bool {
        let _ = state;
        let rate = self.rate as f32;
        let n = ring(self.rate);
        let nf = n as f32;
        let tempo = if tempo < 0.1 { 120. } else { tempo as f32 };
        let d = t.delays.map(|d| if t.synced { f32::from_bits(0x426f_ffff) / tempo * d } else { d });
        let times = d.iter().map(|&v| f64::from(v)).sum::<f64>();
        let st = |slab: &[[f64; 2]], i: usize| slab[i][1] as f32;
        if slab[INIT][1] != 1.0 || slab[TIMES][1] != times {
            // Prepare 0x141663c30 / a delay change: clear the loop, place the heads.
            slab[..n].iter_mut().for_each(|f| f[0] = 0.0);
            let mt = Mt::new(42);
            for (k, v) in mt.s.iter().enumerate() {
                slab[k][1] = f64::from(*v);
            }
            slab[624][1] = 624.0;
            let w = nf - 5.0;
            for (h, d) in d.iter().enumerate() {
                slab[HEADS + h][1] = f64::from(w - d.clamp(0.05, 0.5) * rate);
            }
            slab[ERASE..TIMES].iter_mut().for_each(|f| f[1] = 0.0);
            slab[WRITE][1] = f64::from(w);
            slab[WRITE + 1][1] = f64::from(w.floor());
            slab[SPEED][1] = f64::from(t.speed);
            (slab[INIT][1], slab[TIMES][1]) = (1.0, times);
        }
        let mut mt = Mt { s: std::array::from_fn(|k| slab[k][1] as u32), i: slab[624][1] as usize };
        let pre = shelf(2500.0 / self.rate, 1.0, 3.162_277_698_516_845_7);
        let de = shelf(2500.0 / self.rate, 1.0, 0.316_227_763_891_220_1);
        let a = [exp2pi(5000.0 / rate), exp2pi(14_000.0 / rate), exp2pi(400.0 / rate)];
        let a_speed = exp2pi(f32::from_bits(0x3f2a_aaab) / rate);
        let inc = [1.7f32, 9.4].map(|k| ((t.speed * k * 256.0 / rate) * 16_777_216.0) as i32 as u32);
        let mut s = st(slab, SPEED);
        let c = exp2pi((0.5 * s).min(0.5));
        // EQ (0x141649510): the two shelves' product, formed in f32.
        let [b0, b1, a1] = shelf(100.0 / self.rate, 10f64.powf(f64::from(t.bass_db) * 0.05), 1.0);
        let [c0, c1, e1] = shelf(5000.0 / self.rate, 1.0, 10f64.powf(f64::from(t.treble_db) * 0.05));
        let eq = [b0 * c0, b0 * c1 + b1 * c0, b1 * c1, a1 + e1, a1 * e1].map(f64::from);
        let third = f32::from_bits(0x3eaa_aaab);
        let g = match t.mode {
            1 | 5 => [1.0, 0.0, 0.0],
            2 | 6 => [0.0, 1.0, 0.0],
            3 | 7 => [0.0, 0.0, 1.0],
            4 | 9 => [0.0, 0.5, 0.5],
            8 => [0.5, 0.5, 0.0],
            10 => [0.5, 0.0, 0.5],
            11 => [third; 3],
            _ => [0.0; 3],
        };
        let noise = t.noise.max(1e-10);
        let target = if t.tape_stop { 0.0 } else { t.speed };
        let mut heads = [0, 1, 2].map(|h| st(slab, HEADS + h));
        let (mut erase, mut erased) = (st(slab, ERASE), slab[ERASE + 1][1] as usize);
        let (mut wpos, mut written) = (st(slab, WRITE), slab[WRITE + 1][1] as usize);
        let (mut w, mut w_prev) = (st(slab, W), st(slab, W + 1));
        let mut lp = [0, 1, 2].map(|i| st(slab, LP + i));
        let (mut sd, mut sp) = (st(slab, DE), st(slab, PRE));
        let (mut q1, mut q2) = (slab[EQ][1], slab[EQ + 1][1]);
        let mut phase = [slab[PHASE][1] as u32, slab[PHASE + 1][1] as u32];
        let (dry, wet) = (self.cfg.dry as f32, self.cfg.wet as f32);
        let inv_sqrt2 = 1.0 / 2f32.sqrt();
        let mut finite = true;
        let [left, right] = block;
        for f in 0..len {
            let m = (left[f] as f32 + right[f] as f32) * inv_sqrt2;
            let n1 = mt.unit_f32();
            let wow = uvi_sine(phase[0]);
            phase[0] = phase[0].wrapping_add(inc[0]);
            let flutter = uvi_sine(phase[1]);
            phase[1] = phase[1].wrapping_add(inc[1]);
            let wow8 = wow * 0.8;
            let n2 = mt.unit_f32();
            let k = (n2 * 0.2 + (wow8 + flutter) * f32::from_bits(0x3f0e_38e4)) - 0.1;
            let amp = (flutter * 0.2 + wow8) * f32::from_bits(0x3ba3_d70a);
            s = (target - s) * (1.0 - a_speed) + s;
            let v = (k * f32::from_bits(0x3ba3_d70a) + 1.0) * s;
            let read = |p: f32| {
                let i = p.floor();
                let (a, b) = (slab[i as usize][0] as f32, slab[(i as usize + 1) % n][0] as f32);
                (b - a) * (p - i) + a
            };
            let h = heads.map(read);
            for p in &mut heads {
                *p += v;
                if nf <= *p {
                    *p -= nf;
                }
            }
            let end = (erase.floor() as usize + 1) % n;
            let mut j = (erased + 1) % n;
            while j != end {
                slab[j][0] = 0.0;
                erased = j;
                j = (j + 1) % n;
            }
            erase += v;
            if nf <= erase {
                erase -= nf;
            }
            let mix = (h[1] * g[1] + h[0] * g[0]) + h[2] * g[2];
            lp[0] = (mix - lp[0]) * (1.0 - a[0]) + lp[0];
            lp[1] = (lp[0] - lp[1]) * (1.0 - a[1]) + lp[1];
            lp[2] = (lp[1] - lp[2]) * (1.0 - a[2]) + lp[2];
            let dd = lp[1] - lp[2];
            let y = de[0] * dd + sd;
            sd = de[1] * dd - y * de[2];
            let x = ((n1 * (noise + noise) - noise) + m) * (amp + 1.0);
            let sum = y * t.feedback + x;
            let pt = pre[0] * sum + sp;
            sp = pre[1] * sum - pt * pre[2];
            w = (pt.tanh() - w) * (1.0 - c) + w;
            let end = (wpos.floor() as usize + 1) % n;
            let mut j = (written + 1) % n;
            while j != end {
                let mut dist = wpos - j as f32;
                if dist < 0.0 {
                    dist += nf;
                }
                let al = 1.0 - dist / v;
                let al = if 1.0 <= al { 1.0 } else { al };
                let al = if 0.0 < al { al } else { 0.0 };
                slab[j][0] = f64::from((1.0 - al) * w_prev + al * w + slab[j][0] as f32);
                written = j;
                j = (j + 1) % n;
            }
            w_prev = w;
            wpos += v;
            if nf <= wpos {
                wpos -= nf;
            }
            // Wet EQ, transposed direct form II in f64.
            let yf = f64::from(y);
            let e = eq[0] * yf + q1;
            q1 = eq[1] * yf + q2 - eq[3] * e;
            q2 = eq[2] * yf - eq[4] * e;
            let e = e as f32;
            left[f] = f64::from(left[f] as f32 * dry + e * wet);
            right[f] = f64::from(right[f] as f32 * dry + e * wet);
            finite &= e.is_finite();
        }
        for (k, v) in mt.s.iter().enumerate() {
            slab[k][1] = f64::from(*v);
        }
        slab[624][1] = mt.i as f64;
        let put = |slab: &mut [[f64; 2]], i: usize, v: f32| slab[i][1] = f64::from(v);
        for (h, v) in heads.into_iter().enumerate() {
            put(slab, HEADS + h, v);
        }
        put(slab, ERASE, erase);
        slab[ERASE + 1][1] = erased as f64;
        put(slab, WRITE, wpos);
        slab[WRITE + 1][1] = written as f64;
        put(slab, W, w);
        put(slab, W + 1, w_prev);
        put(slab, SPEED, s);
        for (i, v) in lp.into_iter().enumerate() {
            put(slab, LP + i, v);
        }
        put(slab, DE, sd);
        put(slab, PRE, sp);
        (slab[EQ][1], slab[EQ + 1][1]) = (q1, q2);
        (slab[PHASE][1], slab[PHASE + 1][1]) = (f64::from(phase[0]), f64::from(phase[1]));
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x141649120` at 48 kHz: the pre- and de-emphasis shelves.
    #[test]
    fn emphasis_shelves_match_native_bits() {
        assert_eq!(shelf(2500.0 / 48_000.0, 1.0, 3.162_277_698_516_845_7).map(f32::to_bits), [0x402afa48, 0xc00ded3c, 0xbf0bcbd2]);
        assert_eq!(shelf(2500.0 / 48_000.0, 1.0, 0.316_227_763_891_220_1).map(f32::to_bits), [0x3ebfa699, 0xbe515017, 0xbf5480b9]);
    }

    /// tanhf vectors (`0x1419f245c`), within one ulp of the CRT.
    #[test]
    fn tanh_matches_the_crt() {
        for (x, bits) in [(0.3f32, 0x3e9526eeu32), (1.0, 0x3f42f7d6), (2.5, 0x3f7c92c1)] {
            assert!(x.tanh().to_bits().abs_diff(bits) <= 1, "{x}");
        }
    }

    /// Mode 1, no feedback, no noise, Speed 2: the echo of an impulse
    /// arrives after Delay1 at the mean speed (0.165 s at 2 frames/sample is
    /// half that), within the wow/flutter swing.
    #[test]
    fn first_echo_arrives_after_the_head_gap() {
        use super::super::tests::{impulse, run};
        use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};
        let t = UviTapeEcho { speed: 2.0, feedback: 0.0, mode: 1, bass_db: 0.0, treble_db: 0.0, delays: [0.165, 0.324, 0.487], synced: false, noise: 0.0, tape_stop: false };
        let mut line = Line::single(DelayTime::Ms(0.), 1000.);
        line.native = Native::UviTapeEcho(t);
        let cfg = Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry: 0., wet: 1. };
        let (y, fault) = run(cfg, 48_000, 120., &impulse(12_000, [1., 1.]), 64);
        assert!(!fault);
        let peak = y.iter().enumerate().max_by(|a, b| a.1[0].abs().total_cmp(&b.1[0].abs())).unwrap().0;
        let expect = 0.165 * 48_000.0 / 2.0;
        assert!((peak as f64 - expect).abs() < 120.0, "{peak} vs {expect}");
    }
}
