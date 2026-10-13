//! Kontakt Compressor, Classic (mode 0): core `0x140ab2250`, coefficients from
//! the setter `0x140ab9220` (mode 0 branch), state cleared by `0x14094b710`.
//!
//! Not a level-proportional compressor. Per channel a "hold" `a` is set to 1
//! whenever the detector exceeds the threshold and decays by a fixed factor
//! otherwise; an attack-limited follower `b` chases it from below (falling to
//! it at once); the gain is `1 - (1 - g) * b` with the fixed gain
//! `g = thr^(1 - ratio)`. A correction keeps `a` within 1.5 % of `1 + b`.
//! The core runs in f32 audio with an f64 recurrence.

use super::{Planar, ProcessorState, Shared};
use sampler_ir::fx::dynamics::Link;

/// Constants at `0x144fc3fe0`, `0x144fc3fd8`, `0x144fc3df8`, `0x144fc4000`.
const HOLD_EDGE: f64 = 0.985;
const CORRECT_BELOW: f64 = 0.97;
const CORRECT_SCALE: f64 = 1.015_228_426_395_939;
const CORRECT_OFFSET: f64 = 0.015_228_426_395_939_021;

pub(super) struct Coefficients {
    /// `2^(dB/6)`, f32.
    pub threshold: f32,
    /// Gain at full follower `thr^(1-ratio)`, f32.
    pub gain: f32,
    /// Follower step `0.625^(1/n)` and hold decay `(1/3)^(1/n)`.
    pub attack: f64,
    pub release: f64,
}

impl Coefficients {
    /// `knobs` = [threshold dB, saved ratio, attack s, release s].
    pub fn new([db, ratio, attack, release]: [f64; 4], rate: u32) -> Self {
        let threshold = (db as f32 * f32::from_bits(0x3e2a_aaab)).exp2();
        // Setter: `pow(2, (L*c1 - L*c1*ratio) * c2)` with L = ln(threshold),
        // c1 = -8.65617 (`0x144fc4d28`), c2 = -1/6 (`0x144fc4c58`); f32 up to the pow.
        let scaled = threshold.ln() * -8.656_169_89_f32;
        let exponent = (scaled - scaled * ratio as f32) * f32::from_bits(0xbe2a_aaab);
        let gain = 2_f64.powf(f64::from(exponent)) as f32;
        // trunc(ms * 0.001 * rate + 0.5) in f32, at least one frame.
        let frames = |seconds: f64| {
            let n = (seconds as f32 * 1000. * 0.001_f32 * rate as f32 + 0.5) as u64;
            n.max(1) as f64
        };
        Self {
            threshold,
            gain,
            attack: 0.625_f64.powf(1. / frames(attack)),
            release: (1. / 3_f64).powf(1. / frames(release)),
        }
    }
}

/// State per channel: `z[c] = [hold a, follower b]`.
pub(super) fn process(
    shared: &Shared,
    knobs: [f64; 4],
    rate: u32,
    state: &mut ProcessorState,
    block: &mut Planar,
    len: usize,
) {
    let c = Coefficients::new(knobs, rate);
    let linked = shared.link != Link::Off;
    let reach = f64::from(1. - c.gain);
    let [left, right] = block;
    for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
        let input = [*l as f32, *r as f32];
        // Linked: the signed mean, built as in the core (each scaled by 1/n).
        let mean = input[0] * 0.5 + input[1] * 0.5;
        let mut out = [0f32; 2];
        for ch in 0..2 {
            let detected = if linked { mean } else { input[ch] }.abs();
            let [mut a, mut b] = state.z[ch];
            if detected <= c.threshold || a >= HOLD_EDGE {
                a *= c.release;
            } else {
                a = 1.;
            }
            let held = a;
            if held <= b {
                b = held;
            } else {
                b = 1. - (1. - b) * c.attack;
                if held <= b {
                    b = held;
                }
            }
            if held < CORRECT_BELOW && (b + 1.) / (held + 1.) < HOLD_EDGE {
                a = b * CORRECT_SCALE + CORRECT_OFFSET;
            }
            state.z[ch] = [a, b];
            out[ch] = (1. - reach * b) as f32 * input[ch];
        }
        (*l, *r) = (
            f64::from(out[0]) * shared.factor(1.),
            f64::from(out[1]) * shared.factor(1.),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(knobs: [f64; 4], link: Link, input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let shared = Shared { link, makeup: 1., mix: 1. };
        let mut state = ProcessorState::default();
        let mut out = vec![];
        for chunk in input.chunks(super::super::super::BLOCK) {
            let mut block = [[0.; super::super::super::BLOCK]; 2];
            for (i, f) in chunk.iter().enumerate() {
                (block[0][i], block[1][i]) = (f[0], f[1]);
            }
            process(&shared, knobs, 48_000, &mut state, &mut block, chunk.len());
            out.extend((0..chunk.len()).map(|i| [block[0][i], block[1][i]]));
        }
        out
    }

    #[test]
    fn coefficients_follow_the_setter() {
        // Threshold -12 dB -> 0.25; ratio 0.5 -> g = 0.25^0.5.
        let c = Coefficients::new([-12., 0.5, 0.01, 0.1], 48_000);
        assert!((f64::from(c.threshold) - 0.25).abs() < 1e-6);
        assert!((f64::from(c.gain) - 0.5).abs() < 1e-5);
        // 10 ms at 48 kHz = 480 frames; 100 ms = 4800.
        assert!((c.attack - 0.625_f64.powf(1. / 480.)).abs() < 1e-12);
        assert!((c.release - (1. / 3_f64).powf(1. / 4800.)).abs() < 1e-12);
        // Zero time still moves one frame at a time.
        assert!((Coefficients::new([0., 0., 0., 0.], 48_000).attack - 0.625).abs() < 1e-12);
    }

    #[test]
    fn below_the_threshold_the_signal_is_untouched() {
        let input = vec![[0.1, -0.05]; 256];
        let out = run([-12., 0.5, 0.01, 0.1], Link::Mean, &input);
        for (o, i) in out.iter().zip(&input) {
            assert_eq!(*o, [f64::from(i[0] as f32), f64::from(i[1] as f32)]);
        }
    }

    #[test]
    fn a_loud_signal_settles_to_the_fixed_gain() {
        // Zero attack: the follower jumps to 1 on the first loud sample.
        let input = vec![[1., 1.]; 64];
        let out = run([-12., 0.5, 0., 0.1], Link::Mean, &input);
        // First sample: the follower has taken one 0.625 step toward the hold.
        assert!((out[0][0] - (1. - 0.5 * 0.375)).abs() < 1e-6, "{}", out[0][0]);
        assert!((out[63][0] - 0.5).abs() < 0.01, "{}", out[63][0]);
    }

    #[test]
    fn the_signed_mean_cancels_opposite_channels_only_when_linked() {
        let input = vec![[1., -1.]; 64];
        let linked = run([-12., 0.5, 0., 0.1], Link::Mean, &input);
        let free = run([-12., 0.5, 0., 0.1], Link::Off, &input);
        assert_eq!(linked[63], [1., -1.]);
        assert!((free[63][0] - 0.5).abs() < 0.01 && (free[63][1] + 0.5).abs() < 0.01);
    }

    #[test]
    fn release_returns_to_unity_without_overshoot() {
        let mut input = vec![[1., 1.]; 32];
        input.extend(vec![[0.01, 0.01]; 4800]);
        let out = run([-12., 0.5, 0., 0.02], Link::Mean, &input);
        // The follower may still be catching up with the hold for a few frames.
        let tail: Vec<f64> = out[132..].iter().map(|f| f[0] / 0.01).collect();
        let bad: Vec<_> = tail.windows(2).enumerate().filter(|(_, w)| w[1] < w[0] - 1e-6).take(3).collect();
        assert!(bad.is_empty(), "monotone release {bad:?}");
        assert!(tail.last().unwrap() > &0.99);
        assert!(tail.iter().all(|g| *g <= 1.0000001));
    }
}
