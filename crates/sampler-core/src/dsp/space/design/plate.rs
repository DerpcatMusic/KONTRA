//! Kontakt Plate Reverb (0x57; Kontakt 8.13.1 apply `0x140963eb3`, core
//! setter `0x141ac7280`): decay (core 1), low shelf (9), high damp (10),
//! predelay (0), stereo (13); core 7 fixed at 1 (`0x14470bad4`), core 8 = 1.
//! - input: `0.5·(L+R)`, 20 first-order allpasses `a = 0.9`
//!   (`0x144714914`), then first-order shelves `0x141a92480` (decay: 1000 Hz,
//!   `1.6595869 → 10^(0.15(1-d))`; low: 160 Hz, `10^((1-ls)·1.71875 -
//!   1.4375) → 1`; damp: 5600 Hz, `1 → 10^(0.2(1-hd) - 0.2)`), one frame
//!   apart. Init's 7500 Hz bilinear low-pass survives `clear()` until a
//!   decay/low/damp setter rebuilds the sections; the saved state is applied
//!   through those setters, so it is off here (the native vector test runs
//!   the constructed core with it on).
//! - a ring of the input: early taps `E_j·0.8912509` at `tap_j + (int)(sr·0.005)
//!   + 1` into group `j mod 4`; line injection at `tap_j + 1` into line
//!   `4((j/4) mod 4) + j mod 4`, signed; `tap_j = (int)(f32 sr·f32 τ_j)`
//!   (tables `0x1479aa750`, `0x1479aa7d0`).
//! - 16 lines `(int)(sr·T_i)` long; each read passes its biquad (the product
//!   of shelves `(g_lo → g_mid, f1)` and `(1 → g_hi/g_mid, f2)`, `g = fexp2(
//!   1000·ln(0.001)/sr/RT·L·1.44269)` in f32, `0x141abcff0`/`0x141abec10`/
//!   `0x141a972c0`), `v = (y + s·inj)/4`, unnormalised 16-point Walsh–Hadamard.
//! - out: groups `S_m = Σ_g r_{4g+m} + ER_m`, panned `cos/sin θ_m` (polynomials
//!   `0x149e745e0`/`0x149e74600`), `θ = [(1-2x)π/4, (1-x)π/4, (1+x)π/4,
//!   (1+2x)π/4]` clamped to `[0, π/2]`; wet predelay `max(int(0.25·p·sr), 1)`
//!   frames; level `0.04216965` (`0x3d2cba15`).
//!
//! Everything is linear and time-invariant, so the wet predelay and the
//! sections' three frames of pipeline run at the input. Parameter ramps
//! (`int(sr·0.02/bs + 0.5)` blocks) are not reproduced.

use super::super::network::{Allpass, Feed, LineSpec, Matrix, Ring, Spec, TapFrom};
use std::f64::consts::PI;

const E: [f32; 32] = [
    -1.0283, 1.0507, -1.0418, 0.9833, -0.8775, -1.3643, -1.2729, -1.2166, 1.3947, 1.2706, 1.2378, -1.4102, 1.3066, 1.4135, -1.3504, 1.0404, -1.0575, -0.8443, -1.1791, -1.0901, -1.0313, -1.1631, -1.2617, -1.2342, 0.9911, 0.9796, -0.7477, -1.256, -1.111, 0.9026, -1.3962, -1.0208,
];
/// Tap times in frames at 48 kHz.
const TAP48: [u16; 32] = [1496, 1395, 1435, 1706, 2241, 168, 494, 706, 64, 502, 625, 12, 371, 1, 216, 1441, 1364, 2422, 853, 1222, 1482, 917, 535, 639, 1669, 1724, 2993, 556, 1132, 2108, 59, 1530];
const LINE48: [u16; 16] = [1015, 1027, 1133, 1200, 1266, 1395, 1483, 1577, 1655, 1817, 1922, 2037, 2181, 2321, 2447, 2643];
const SIGN: [f32; 16] = [1., -1., 1., 1., -1., 1., -1., -1., -1., 1., -1., -1., 1., 1., -1., 1.];

/// `f32(n/48000)` scaled to `sr`, truncated.
fn frames(sr: f32, n: u16) -> usize {
    (sr * (f64::from(n) / 48_000.0) as f32) as usize
}

/// Shelf design `0x141a92480`: `[b0, b1, a1]` of `(b0 + b1·z⁻¹)/(1 + a1·z⁻¹)`,
/// DC gain `g0`, Nyquist gain `gp`; `k` from `sin`.
pub(super) fn shelf(f: f64, g0: f64, gp: f64, rate: f64, sin: fn(f64) -> f64) -> [f64; 3] {
    if g0 == gp {
        return [g0, 0.0, 0.0];
    }
    let q = (gp * gp - g0 * g0) / (g0 * g0 + gp * gp - 2.0 * gp * g0);
    let p = q - q.signum() * (q * q - 1.0).sqrt();
    let b0 = 0.5 * ((g0 - gp) * p + g0 + gp);
    let b1 = 0.5 * ((g0 + gp) * p + g0 - gp);
    let th = PI * (f / rate).min(1.0) / 2.0;
    let k = sin(th - PI / 4.0) / sin(th + PI / 4.0);
    let n = 1.0 / (k * p + 1.0);
    [(k * b1 + b0) * n, (k * b0 + b1) * n, (k + p) * n]
}

/// Polynomial sine `0x149e745e0`, no range reduction.
fn psin(t: f64) -> f64 {
    let t = t as f32;
    let c = [0xb2cd4ca8u32, 0x3638b94f, 0xb9500c03, 0x3c088885, 0xbe2aaac1, 0x3f800000].map(f32::from_bits);
    let t2 = t * t;
    f64::from(t * c.iter().fold(0.0, |acc, &k| acc * t2 + k))
}

/// Polynomial cosine `0x149e74600`.
fn pcos(t: f64) -> f64 {
    let t = t as f32;
    let c = [0xb48bdad9u32, 0x37cfb5a1, 0xbab609bd, 0x3d2aaa99, 0xbefff5e8, 0x3f800000].map(f32::from_bits);
    let t2 = t * t;
    f64::from(c.iter().fold(0.0, |acc, &k| acc * t2 + k))
}

/// `fexp2` of `0x141a972c0` (f32; `cvtps2dq` rounds to nearest even).
fn fexp2(y: f32) -> f32 {
    let i = y.round_ties_even() as i32;
    let f = y - i as f32;
    let p = ((f32::from_bits(0x3d65_6042) * f + f32::from_bits(0x3e77_ced9)) * f + f32::from_bits(0x3f31_6873)) * f + 1.0;
    p * f32::from_bits((i.wrapping_add(127) as u32) << 23)
}

/// Line `i`'s biquad `[b0, b1, b2, a1, a2]` (before the `1/4`).
pub(super) fn line_biquad(d: f32, hd: f32, len: f32, rate: f64) -> [f64; 5] {
    let sr = rate as f32;
    let x = 1000.0 * (0.001f32).ln() * (1.0 / sr);
    let g = |rt: f32| f64::from(fexp2(x / rt * len * f32::from_bits(0x3fb8_aa11)));
    let (lo, hi, mid) = (g(6400.0 * d + 556.0), g(1070.0 * d + 173.0), g(2993.0 * d + 1007.0));
    // The line shelves take the corner over the Nyquist frequency.
    let f1 = 2.0 * f64::from(757.0 * (1.0 - d) + 968.0);
    let f2 = 2.0 * f64::from(4851.0 * (1.0 - hd) + 6000.0);
    let [a0, a1, p1] = shelf(f1, lo, mid, rate, f64::sin);
    let [c0, c1, p2] = shelf(f2, 1.0, hi / mid, rate, psin);
    [a0 * c0, a0 * c1 + a1 * c0, a1 * c1, p1 + p2, p1 * p2]
}

pub(in super::super) fn kontakt_plate(c: &[f32; 5], rate: f64) -> Spec {
    build(c, rate, false)
}

fn build(c: &[f32; 5], rate: f64, init_lp: bool) -> Spec {
    let [d, ls, hd, pre, st] = *c;
    let sr = rate as f32;
    let lines = LINE48
        .iter()
        .map(|&n| {
            // The decay law takes the untruncated length (`obj+0x520`).
            let span = sr * (f64::from(n) / 48_000.0) as f32;
            let len = span as usize;
            let mut l = LineSpec::new(len + 2, len as f64);
            l.bq = line_biquad(d, hd, span, rate).map(|v| v as f32);
            for b in &mut l.bq[..3] {
                *b *= 0.25;
            }
            l
        })
        .collect();
    let mut s = Spec::new(lines, Matrix::Hadamard);
    s.mix = 1.0;
    s.read_biquad = true;
    s.tap_from = TapFrom::Read;
    for ch in &mut s.diffuser {
        *ch = (0..20).map(|_| Allpass::new(1, -0.9)).collect();
    }
    let sections = [shelf(1000.0, 1.659_586_9, 10f64.powf(0.15 * f64::from(1.0 - d)), rate, f64::sin), shelf(160.0, 10f64.powf(f64::from(1.0 - ls) * 1.71875 - 1.4375), 1.0, rate, f64::sin), shelf(5600.0, 1.0, 10f64.powf(0.2 * f64::from(1.0 - hd) - 0.2), rate, f64::sin)];
    s.input_bq = sections.iter().map(|&[b0, b1, a1]| [b0 as f32, b1 as f32, 0.0, a1 as f32, 0.0]).collect();
    if init_lp {
        let k = (PI * 7500.0 / rate).tan();
        let b0 = (k / (1.0 + k)) as f32;
        s.input_bq.push([b0, b0, 0.0, ((k - 1.0) / (k + 1.0)) as f32, 0.0]);
    }
    // The wet predelay and the three section frames; the engine reads a
    // line before writing it, one frame later than the native's taps.
    let wet_pre = ((250.0 * pre * 0.001 * sr) as i32).max(1) as usize;
    s.predelay = [(wet_pre + 3 - 1) as f32; 2];
    s.pre_cap = wet_pre + 4;
    let x = f64::from(st);
    let q = PI / 4.0;
    let theta = [((1.0 - 2.0 * x) * q).max(0.0), (1.0 - x) * q, (1.0 + x) * q, ((1.0 + 2.0 * x) * q).min(PI / 2.0)];
    let pan = theta.map(|t| [pcos(t) as f32, psin(t) as f32]);
    let er = (0.005 * sr) as usize;
    let taps: Vec<usize> = TAP48.iter().map(|&n| frames(sr, n)).collect();
    let feed = Feed {
        ring: Ring::new(taps.iter().max().unwrap() + er + 4),
        inject: taps.iter().enumerate().map(|(j, &t)| {
            let line = 4 * ((j / 4) % 4) + j % 4;
            (t + 1, line as u8, 0.25 * SIGN[line])
        }).collect(),
        early: taps.iter().enumerate().map(|(j, &t)| {
            let c = E[j] * 0.891_250_9;
            (t + er + 1 + 1, pan[j % 4].map(|p| p * c))
        }).collect(),
    };
    s.feed = Some(feed);
    s.taps = [0, 1].map(|o| (0..16).map(|i| (i as u8, pan[i % 4][o])).collect());
    s.width = Some(1.0);
    s.wet = f32::from_bits(0x3d2c_ba15);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Line 0's biquad at 44.1 kHz, d = hd = 0.5 (`0x141abcff0`).
    #[test]
    fn line_biquad_matches_native() {
        let len = 44_100.0 * (1015.0 / 48_000.0f64) as f32;
        let b = line_biquad(0.5, 0.5, len, 44_100.0);
        let want = [0.86555755, -0.8433339, 0.107859075, -1.0487405, 0.18397981];
        assert!(b.iter().zip(want).all(|(g, w)| (g - w).abs() < 5e-7), "{b:?}");
    }

    /// The constructed core at 44.1 kHz (ctor d = ls = hd = 0.5, st = 1,
    /// p0 = 0, init's 7500 Hz section on): unit impulse on L, wet L.
    #[test]
    fn impulse_matches_native() {
        let mut net = super::super::super::network::Network::new(build(&[0.5, 0.5, 0.5, 0.0, 1.0], 44_100.0, true));
        let (mut l, mut r) = (vec![0f32; 240], vec![0f32; 240]);
        l[0] = 1.0;
        net.process(&mut l, &mut r);
        assert!(l[..225].iter().all(|v| *v == 0.0), "{:?}", &l[215..226]);
        let want = [0.00118216, -0.00342246, -0.000560938, 0.00655865];
        assert!(l[225..229].iter().zip(want).all(|(g, w)| (g - w).abs() < 2e-8), "{:?}", &l[225..229]);
    }

    /// Line lengths at 48 kHz: `f32(1817/48000)` truncates to 1816 frames.
    #[test]
    fn line_nine_truncates() {
        assert_eq!(frames(48_000.0, LINE48[9]), 1816);
        assert_eq!(frames(48_000.0, TAP48[3]), 1705);
    }
}
