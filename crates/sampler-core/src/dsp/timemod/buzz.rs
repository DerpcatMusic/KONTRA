//! UVI analog tape delay `Buzz` (Workstation 4.0.9 process `0x1412ddc00` →
//! `0x14166ab40`, tempo `0x14166ba70`, prepare `0x14166a410`, Trash setter
//! `0x14166b7c0`), f32 like native:
//! - `sec = clamp(t, 0.04, 2)` (synced `59.999996/tempo·t`), target
//!   `sec·sr + 1` frames, ring `4·sr + 1`; the current delay glides on its
//!   reciprocal, `c = pow(1/3, 1/(glide·sr))`.
//! - Trash: `a = 1.14·T + 0.1`, `A = sin(a·π/2)`, `b = 0.61·T + 0.01`,
//!   `P = 10^(0.09·T)`, `q = 6.5·T + 1`; mode 2 gains `10^T` in, `1 - T/2` out.
//! - wow: `sin(n·2π/(period·sr/1000))·sr/10000` frames (periods 500/333/750
//!   ms); cubic read at `w - cur + wow`.
//! - loop: `in·gain + y·fb`, the mode's shaper (`sin(x·π/2·a)/A`;
//!   `sin(x·π/2·b)/A` then `P` soft-knee; `∓atan(|x|^q)^(1/q)`), the RBJ
//!   low-pass (`0x14163fd30`; 5500/4000/1500 Hz, Q .38/.59/.9) in f64.
//! - stereo spread `((L+R) ± (L-R)·S)/2` on the wet, out `gain·Wet·y + Dry·x`.

use super::{Planar, ProcessorState, TimeMod};
use sampler_ir::fx::timemod::UviBuzzDelay;
use std::f32::consts::FRAC_PI_2;

pub(super) fn ring(rate: f64) -> usize {
    (4.0 * rate) as usize + 1
}

pub(super) fn capacity(rate: f64) -> usize {
    ring(rate)
}

/// RBJ low-pass `0x14163fd30`: `[b0, b1, b2, a1, a2]`, or `None` above Nyquist.
fn lowpass(mode: u8, rate: f64) -> Option<[f64; 5]> {
    let (fc, q) = [(5500.0, 0.38), (4000.0, 0.59), (1500.0, 0.9)][usize::from(mode.min(2))];
    if fc > rate * 0.5 {
        return None;
    }
    let w = std::f64::consts::TAU * fc / rate;
    let alpha = w.sin() * 0.5 / q;
    let n = 1.0 / (1.0 + alpha);
    let cw = w.cos();
    Some([(1.0 - cw) * 0.5 * n, (1.0 - cw) * n, (1.0 - cw) * 0.5 * n, -2.0 * cw * n, (1.0 - alpha) * n])
}

/// The kernel; `filter` is the loop low-pass. `st`: 0 write, 1 wow
/// counter, 2 current delay, 3 started, 4-7 the filter states.
#[allow(clippy::too_many_arguments)]
fn process(b: &UviBuzzDelay, rate: f64, tempo: f64, wet: f32, dry: f32, st: &mut [f64], slab: &mut [[f64; 2]], block: &mut Planar, len: usize, filter: Option<[f64; 5]>) -> bool {
    let sr = rate as f32;
    let n = ring(rate);
    let tempo = if tempo < 0.1 { 120.0 } else { tempo as f32 };
    let sec = if b.synced { f32::from_bits(0x426f_ffff) / tempo * b.time } else { b.time }.clamp(0.04, 2.0);
    let target = sec * sr + 1.0;
    let c = 0.333_333_333_333_333_37f64.powf(1.0 / f64::from(b.glide * sr)) as f32;
    let t = b.trash;
    let a = t * 1.14 + 0.1;
    let big_a = (a * FRAC_PI_2).sin();
    let bb = t * 0.61 + 0.01;
    let p = 10f32.powf(t * 0.09);
    let q = t * 6.5 + 1.0;
    let (gain_in, gain_out) = if b.mode == 2 { (10f32.powf(t), 1.0 - 0.5 * t) } else { (1.0, 1.0) };
    let period = [500.0f32, 333.0, 750.0][usize::from(b.mode.min(2))];
    let inc = std::f32::consts::TAU / (period * 0.001 * sr);
    let depth = (0.1f32).min(sec * 1000.0) * 0.001 * sr;
    if st[3] != 1.0 {
        (st[0], st[1], st[2], st[3]) = (f64::from(target as u64 as u32), 0.0, f64::from(target), 1.0);
    }
    let (mut w, mut k, mut cur) = (st[0] as usize, st[1] as u64, st[2] as f32);
    let shape = |x: f32| match b.mode {
        0 => (x * FRAC_PI_2 * a).sin() / big_a,
        1 => {
            let s = (x * FRAC_PI_2 * bb).sin() / big_a;
            if p == 1.0 {
                s
            } else {
                let u = p * s;
                if u > 0.0 { 1.0 - 1.0 / (u + 1.0) } else { 1.0 / (1.0 - u) - 1.0 }
            }
        }
        _ => (if x >= 0.0 { -1.0 } else { 1.0 }) * x.abs().powf(q).atan().powf(1.0 / q),
    };
    let mut finite = true;
    let [left, right] = block;
    for f in 0..len {
        let inv = 1.0 / cur;
        cur = 1.0 / (inv + (1.0 / target - inv) * (1.0 - c));
        let mut pos = w as f32 - cur + (k as f32 * inc).sin() * depth;
        if pos < 0.0 {
            pos = (pos + n as f32).max(0.0);
        }
        let i = pos as usize;
        let fr = pos - i as f32;
        let x = [left[f] as f32, right[f] as f32];
        let mut y = [0f32; 2];
        for ch in 0..2 {
            let r = |o: usize| slab[(i + o + n - 1) % n][ch] as f32;
            let (aa, bv, cv, dv) = (r(3), r(2), r(1), r(0));
            let kk = ((aa - bv) - dv) + cv;
            y[ch] = (kk * fr + ((dv - cv) - kk)) * (fr * fr) + (bv - dv) * fr + cv;
            let mut v = f64::from(shape(x[ch] * gain_in + y[ch] * b.feedback));
            if let Some([b0, b1, b2, a1, a2]) = filter {
                let out = b0 * v + st[4 + 2 * ch];
                st[4 + 2 * ch] = b1 * v - a1 * out + st[5 + 2 * ch];
                st[5 + 2 * ch] = b2 * v - a2 * out;
                v = out;
            }
            slab[w][ch] = f64::from(v as f32);
        }
        w = (w + 1) % n;
        k += 1;
        let (s, d) = (y[0] + y[1], y[0] - y[1]);
        let wy = [(s + d * b.spread) * 0.5, (s - d * b.spread) * 0.5];
        let g = gain_out * wet;
        let ol = g * wy[0] + dry * x[0];
        let or = g * wy[1] + dry * x[1];
        left[f] = f64::from(ol);
        right[f] = f64::from(or);
        finite &= ol.is_finite() && or.is_finite();
    }
    (st[0], st[1], st[2]) = (w as f64, k as f64, f64::from(cur));
    !finite
}

impl TimeMod {
    pub(super) fn uvi_buzz_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, b: UviBuzzDelay) -> bool {
        process(&b, self.rate, tempo, self.cfg.wet as f32, self.cfg.dry as f32, &mut state.aux[..8], slab, block, len, lowpass(b.mode, self.rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Design `0x14163fd30`, mode 0 at 48 kHz.
    #[test]
    fn lowpass_matches_native() {
        let [b0, b1, b2, a1, a2] = lowpass(0, 48_000.0).unwrap();
        let near = |x: f64, y: f64| (x - y).abs() < 1e-15;
        assert!(near(b0, 0.0664396739362211) && near(b1, 0.1328793478724422) && near(b2, b0));
        assert!(near(a1, -0.8051572035582284) && near(a2, 0.07091589930311286));
    }

    /// Defaults (200 ms, Fb .2, Trash .25, mode 0, Spread 1, Wet/Dry .5),
    /// impulse 0.5 on L, the loop filter stubbed as in the native run.
    #[test]
    fn impulse_matches_native_bits() {
        let b = UviBuzzDelay { time: 0.2, synced: false, feedback: 0.2, trash: 0.25, spread: 1.0, glide: 0.1, mode: 0 };
        let mut slab = vec![[0.0; 2]; capacity(48_000.0)];
        let mut st = [0.0; 8];
        let mut out = Vec::new();
        for blk in 0..160 {
            let mut planar: Planar = [[0.0; super::super::super::BLOCK]; 2];
            if blk == 0 {
                planar[0][0] = 0.5;
            }
            process(&b, 48_000.0, 120.0, 0.5, 0.5, &mut st, &mut slab, &mut planar, 64, None);
            out.extend((0..64).map(|i| planar[0][i] as f32));
        }
        let bits = |k: usize| out[k].to_bits();
        assert_eq!([bits(0), bits(9597), bits(9598), bits(9599), bits(9600)], [0x3e800000, 0xbd00178e, 0x3e7ce3fe, 0x3d5a8318, 0xbbe11680]);
    }
}
