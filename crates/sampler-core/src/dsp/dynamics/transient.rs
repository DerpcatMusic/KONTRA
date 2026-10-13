//! Kontakt Transient Master (SerType 0x43, `BEffectTrans`, core at effect +
//! 0x1c0): setter `0x141ac87b0`, prepare `0x141aa27a0`, state `0x141a83550` /
//! `0x141aa0b80`, process dispatcher `0x141aaac90` (64 variants by flag). This
//! is variant 0 (`0x141a4a9d0`) and, with the output clip, variant 32
//! (`0x141a59d40`); the other flags (controls 4..7) are not reached by a saved
//! effect. f32 throughout.
//!
//! The channel maximum of a 10 Hz high-passed input, raised to the 0.1 power
//! (a 30001-entry table over 0.0005..=3, `powf` outside), times 0.7, feeds six
//! one-pole envelopes. The attack signal is a fast-minus-slow difference,
//! gated, high-passed (2 Hz) and peak-held; the sustain signal is a slow
//! envelope over a medium one, less the gated fast-over-medium excess. The
//! gain is `attack * A + sustain * (S - 0.33 A) + gain` dB through the native
//! exponent table, applied before a 10 Hz DC blocker (and the ±0.9999 clip).

use super::super::filter::ar_kernel::exponential;
use super::{Planar, ProcessorState};
use std::sync::OnceLock;

/// `(i / 10000)^0.1`, `0x1478fb240`.
fn root_table() -> &'static [f32; 30_002] {
    static T: OnceLock<Box<[f32; 30_002]>> = OnceLock::new();
    T.get_or_init(|| Box::new(std::array::from_fn(|i| (i as f64 / 10_000.).powf(0.1) as f32)))
}

fn tenth_root(x: f32) -> f32 {
    if !(f32::from_bits(0x3a03_126f)..=3.).contains(&x) {
        return x.powf(f32::from_bits(0x3dcc_cccd));
    }
    let t = root_table();
    let p = x * 10_000.;
    let i = p as i32 as usize;
    (t[i + 1] - t[i]) * (p - i as f32) + t[i]
}

/// Initial envelopes (`0x141aa0b80`): e1, e2, e3, e4, e5, e6.
const INIT: [u32; 6] = [0x3e86_24dd, 0x3ed9_999a, 0, 0x3ece_5604, 0x3ece_5604, 0x3f21_47ae];

#[derive(Clone, Copy)]
pub(super) struct Controls {
    /// Normalized saved controls 0..=2.
    pub attack: f32,
    pub sustain: f32,
    pub gain: f32,
    /// Control 3: clip the output to ±0.9999.
    pub clip: bool,
}

struct Coefficients {
    attack: f32,
    sustain: f32,
    gain: f32,
    hp: [f32; 3],
    sub: [f32; 3],
    k: [f32; 9],
}

impl Coefficients {
    fn new(c: &Controls, rate: u32) -> Self {
        let fs = rate as f32;
        let inv = 1. / fs;
        let hp = |w: f32| {
            let t = (inv * w).tan();
            let b = 1. / (t + 1.);
            [b, -b, (t - 1.) * b]
        };
        let per = |bits: u32| f32::from_bits(bits) / fs;
        let a = (c.attack - 0.5) * 120.;
        let s = (c.sustain - 0.5) * 115.;
        Self {
            attack: a + a,
            sustain: s + s,
            gain: (c.gain - 0.5) * if c.gain >= 0.5 { 12. } else { 48. },
            hp: hp(f32::from_bits(0x41fb_53d1)),
            sub: hp(f32::from_bits(0x4149_0fdb)),
            // e1 up/down, e2 up/down, e3 down, e4 down, e5 up/down, e6 up/down.
            k: [
                per(0x4765_7546),
                per(0x4128_091f),
                per(0x42cc_c52a),
                per(0x4128_091f),
                per(0x42ac_9e2b),
                per(0x415d_ce9a),
                per(0x44ad_4968),
                per(0x45b4_81ce),
                per(0x3ed0_c272),
            ],
        }
    }
}

/// `aux[0..4]` detector high-pass (x1, y1 per channel), `aux[4..10]` the
/// envelopes less their initial values, `aux[10..12]` the attack high-pass,
/// `aux[12..16]` the output DC blocker.
pub(super) fn process(controls: &Controls, rate: u32, state: &mut ProcessorState, block: &mut Planar, len: usize) {
    let c = Coefficients::new(controls, rate);
    let mut f: [f32; 16] = std::array::from_fn(|i| state.aux[i] as f32);
    let mut e: [f32; 6] =
        std::array::from_fn(|i| (f64::from(f32::from_bits(INIT[i])) + state.aux[4 + i]) as f32);
    let [k1u, k1d, k2u, k2d, k3d, k4d, k5u, k6u, k6d] = c.k;
    let k5d = k4d;
    let step = |env: f32, target: f32, up: f32, down: f32| {
        let d = target - env;
        (if d > 0. { up } else { down }) * d + env
    };
    for i in 0..len {
        let x = [block[0][i] as f32, block[1][i] as f32];
        let mut det = 0f32;
        for ch in 0..2 {
            let y = (x[ch] * c.hp[0] + c.hp[1] * f[2 * ch]) - c.hp[2] * f[2 * ch + 1];
            (f[2 * ch], f[2 * ch + 1]) = (x[ch], y);
            det = det.max(y.abs());
        }
        let l = tenth_root(det) * f32::from_bits(0x3f33_3333);
        e[0] = step(e[0], l, k1u, k1d);
        e[1] = step(e[1], l, k2u, k2d);
        let mut v = e[0] - e[1];
        if v > f32::from_bits(0x3d98_5f07) {
            v *= f32::from_bits(0x3f92_233a);
        }
        let w = (v - f32::from_bits(0x3d4e_703b)).max(0.).min(f32::from_bits(0x3df5_c28f));
        let y = (c.sub[1] * f[10] + w * c.sub[0]) - c.sub[2] * f[11];
        (f[10], f[11]) = (w, y);
        let d = y.abs() - e[2];
        e[2] = if d <= 0. { d * k3d + e[2] } else { y.abs() };
        let d = l - e[3];
        e[3] = if d <= 0. { d * k4d + e[3] } else { l };
        e[4] = step(e[4], l, k5u, k5d);
        let s = ((e[3] - e[4]) - f32::from_bits(0x3d23_d70a)).max(0.).min(10_000.);
        e[5] = step(e[5], l, k6u, k6d);
        let sustain = ((e[5] - e[4]) - s).max(0.).min(10_000.);
        let a = e[2];
        let db = (sustain * c.sustain - c.sustain * f32::from_bits(0x3ea8_f5c3) * a) + a * c.attack + c.gain;
        let gain = exponential(db * f32::from_bits(0x411f_73da) + 1200.);
        for ch in 0..2 {
            let u = gain * x[ch];
            let y = (u * c.hp[0] + c.hp[1] * f[12 + 2 * ch]) - f[13 + 2 * ch] * c.hp[2];
            (f[12 + 2 * ch], f[13 + 2 * ch]) = (u, y);
            let out = if controls.clip { y.clamp(-0.9999, 0.9999) } else { y };
            block[ch][i] = f64::from(out);
        }
    }
    for (i, v) in f.iter().enumerate() {
        state.aux[i] = f64::from(*v);
    }
    for (i, v) in e.iter().enumerate() {
        state.aux[4 + i] = f64::from(*v) - f64::from(f32::from_bits(INIT[i]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = super::super::super::BLOCK;

    fn run(controls: Controls, input: &[f64]) -> Vec<f64> {
        let mut state = ProcessorState::default();
        let mut out = vec![];
        for chunk in input.chunks(BLOCK) {
            let mut block = [[0.; BLOCK]; 2];
            for (i, v) in chunk.iter().enumerate() {
                (block[0][i], block[1][i]) = (*v, *v);
            }
            process(&controls, 48_000, &mut state, &mut block, chunk.len());
            out.extend(block[0][..chunk.len()].iter());
        }
        out
    }

    fn bursts() -> Vec<f64> {
        (0..48_000)
            .map(|i| {
                // Decaying plucks (100 ms time constant), one every 250 ms.
                let t = (i % 12_000) as f64 / 48_000.;
                0.5 * (-t / 0.1).exp() * (i as f64 * 440. * std::f64::consts::TAU / 48_000.).sin()
            })
            .collect()
    }

    fn rms(v: &[f64]) -> f64 {
        (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt()
    }

    #[test]
    fn neutral_controls_pass_audio_at_unity() {
        let out = run(Controls { attack: 0.5, sustain: 0.5, gain: 0.5, clip: false }, &bursts());
        let input = bursts();
        let (o, i) = (rms(&out[24_000..36_000]), rms(&input[24_000..36_000]));
        assert!((o / i - 1.).abs() < 0.01, "{o} {i}");
    }

    #[test]
    fn attack_boosts_onsets_and_sustain_shapes_tails() {
        let input = bursts();
        let onset = |c| rms(&run(c, &input)[24_000..24_480]);
        let tail = |c| rms(&run(c, &input)[30_000..34_000]);
        let neutral = Controls { attack: 0.5, sustain: 0.5, gain: 0.5, clip: false };
        assert!(onset(Controls { attack: 1., ..neutral }) > onset(neutral) * 1.2);
        assert!(tail(Controls { sustain: 0., ..neutral }) < tail(neutral) * 0.9);
    }

    #[test]
    fn table_matches_powf_inside_its_range() {
        for x in [0.001f32, 0.25, 1., 2.9] {
            assert!((tenth_root(x) - x.powf(0.1)).abs() < 1e-4, "{x}");
        }
    }
}
