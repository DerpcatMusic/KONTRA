//! Kontakt Solid Bus Comp (SerType 0x46, `BEffectSSLGBusComp`, core
//! `NI::DSP::CSL::G_BusComp` at effect + 0x1c0): process `0x141aaa1e0`,
//! detector `0x141a95cb0`, setter `0x141ac69e0`, constructor `0x141a7fc80`,
//! prepare `0x141aa1ea0`, time coefficient `0x141acc420`, level map
//! `0x141a9d400`. f64 throughout; one detector per channel.
//!
//! Feedback detector: each channel reads its own previous output level
//! `g * x / 128`, mapped to `log2 / 32` (floor `-24`), and asks for
//! `(threshold - level) * (1 - R)` clamped to 0..=1; that request is smoothed
//! by the attack / release one-pole (auto release: a second, slower envelope
//! the request falls back to), and the gain is `2^(-32 * env)` (at most
//! -192 dB). Linked, every channel takes the smallest gain. The threshold
//! knob's spline (dB) less 20 plus a per-ratio offset is the threshold.

use super::super::eq_family::solid::Spline;
use super::{Planar, ProcessorState};
use std::sync::LazyLock;

/// Core descriptor tables 0 and 1 (threshold and makeup dB), `0x1479a7d60` /
/// `0x1479a7dc0`.
static THRESHOLD: LazyLock<Spline> = LazyLock::new(|| {
    Spline::new([-18.6, -14.88, -11.16, -7.44, -3.72, 0., 3.72, 7.44, 11.16, 14.88, 18.6])
});
static MAKEUP: LazyLock<Spline> = LazyLock::new(|| {
    Spline::new([-5.25, -2.625, 0., 2.625, 5.25, 7.875, 10.5, 13.125, 15.75, 18.375, 21.])
});
const RATIO: [f64; 6] = [1.5, 2., 3., 4., 5., 10.]; // 0x1479a7e18
const THRESHOLD_OFFSET: [f64; 6] = [-12., -9., -5., -4., -3.3, -0.2]; // 0x1479a7e48
const ATTACK_MS: [f64; 6] = [0.1, 0.3, 1., 3., 10., 30.]; // 0x1479a7e78
const RELEASE: [f64; 6] = [0.2, 0.3, 0.6, 0.8, 1.6, 1.2]; // 0x1479a7ea8, x 62.5 ms
const LN_08: f64 = f64::from_bits(0xbfcc_8ff7_c79a_9a20); // 0x1479ab6a8

#[derive(Clone, Copy)]
pub(super) struct Controls {
    /// Normalized core controls 0..=4 and 5 (link).
    pub threshold: f64,
    pub makeup: f64,
    pub attack: f64,
    pub release: f64,
    pub ratio: f64,
    pub link: bool,
}

/// `floor(5x + .5)` on a 0..5 descriptor.
fn index(x: f64) -> usize {
    (x * 5. + 0.5).floor().clamp(0., 5.) as usize
}

/// `log2(v) / 32`, with `-24` below `2^-24`.
fn level(v: f64) -> f64 {
    (if v >= (-24f64).exp2() { v.log2() } else { -24. }) * 0.03125
}

struct Coefficients {
    threshold: f64,
    slope: f64,
    makeup: f64,
    attack: f64,
    release: f64,
    auto: Option<[f64; 2]>,
}

impl Coefficients {
    fn new(c: &Controls, rate: u32) -> Self {
        let fs = f64::from(rate);
        let coefficient = |ms: f64| if ms == 0. { 0. } else { (LN_08 / (ms * 0.001 * fs)).exp().min(1.) };
        let r = index(c.ratio);
        let inverse = 1. / RATIO[r];
        let db = (THRESHOLD.eval(c.threshold) - 20.) + THRESHOLD_OFFSET[r];
        let (a, rl) = (index(c.attack), index(c.release));
        let attack_ms = ATTACK_MS[a];
        Self {
            threshold: level(10f64.powf(db * 0.05) * 0.0078125),
            slope: 1. - 1. / inverse,
            makeup: 10f64.powf(MAKEUP.eval(c.makeup) * 0.05),
            attack: coefficient(attack_ms),
            release: coefficient(RELEASE[rl] * 62.5),
            auto: (rl == 5).then(|| [coefficient(attack_ms + 90.), coefficient(150.)]),
        }
    }
}

/// `aux[0..2]` envelope, `aux[2..4]` fed-back level, `aux[4..6]` the auto
/// release envelope, per channel.
pub(super) fn process(c: &Controls, mix: f64, rate: u32, state: &mut ProcessorState, block: &mut Planar, len: usize) {
    let k = Coefficients::new(c, rate);
    let (wet, dry) = (mix.clamp(0., 1.), 1. - mix.clamp(0., 1.));
    for i in 0..len {
        let x = [block[0][i] as f32 as f64, block[1][i] as f32 as f64];
        let mut gain = [1.; 2];
        for ch in 0..2 {
            let d = ((k.threshold - level(state.aux[2 + ch].abs().min(1.))) * k.slope).clamp(0., 1.);
            let env = state.aux[ch];
            let next = match k.auto {
                None => {
                    let c = if d <= env { k.release } else { k.attack };
                    (1. - c) * d + env * c
                }
                Some([up, down]) => {
                    let slow = state.aux[4 + ch];
                    let c = if d <= slow { down } else { up };
                    let slow = (1. - c) * d + c * slow;
                    state.aux[4 + ch] = slow;
                    if d > env {
                        (1. - k.attack) * d + env * k.attack
                    } else {
                        (1. - k.release) * d.max(slow) + env * k.release
                    }
                }
            };
            state.aux[ch] = next;
            gain[ch] = (next * -32.).clamp(-32., 0.).exp2();
            state.aux[2 + ch] = gain[ch] * x[ch] * 0.0078125;
        }
        let low = gain[0].min(gain[1]);
        for ch in 0..2 {
            let w = x[ch] * if c.link { low } else { gain[ch] } * k.makeup;
            block[ch][i] = f64::from((x[ch] * dry + w * wet) as f32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = super::super::super::BLOCK;

    fn peak_after(c: Controls, amp: f64) -> f64 {
        let mut state = ProcessorState::default();
        let mut peak = 0f64;
        for b in 0..1500 {
            let mut block = [[0.; BLOCK]; 2];
            for i in 0..BLOCK {
                let v = amp * ((b * BLOCK + i) as f64 * 1000. * std::f64::consts::TAU / 48_000.).sin();
                (block[0][i], block[1][i]) = (v, v);
            }
            process(&c, 1., 48_000, &mut state, &mut block, BLOCK);
            if b > 1200 {
                peak = block[0].iter().fold(peak, |p, v| p.max(v.abs()));
            }
        }
        peak
    }

    #[test]
    fn tables_and_coefficients_match_the_native_values() {
        // KONTAKT_DSP_LAWS: table 0 at .05 is -17.32959641360375; 48 kHz
        // attack 3 ms .998451592027297, release 50 ms .9999070278424647.
        assert!((THRESHOLD.eval(0.05) + 17.32959641360375).abs() < 1e-12);
        let c = Controls { threshold: 0.5, makeup: 0.2, attack: 0.5, release: 0.5, ratio: 0.5, link: true };
        let k = Coefficients::new(&c, 48_000);
        assert_eq!(k.attack, 0.998451592027297);
        assert_eq!(k.release, 0.9999070278424647);
        assert!((k.makeup - 1.).abs() < 1e-8);
    }

    #[test]
    fn feedback_detector_reaches_the_ratio() {
        // Ratio 10 (x = 1), threshold well below the signal: 20 dB more input
        // gives about 2 dB more output.
        let c = Controls { threshold: 0., makeup: 0.2, attack: 0., release: 0., ratio: 1., link: true };
        let (loud, quiet) = (peak_after(c, 1.), peak_after(c, 0.1));
        let db = 20. * (loud / quiet).log10();
        assert!((db - 2.).abs() < 0.5, "{db}");
    }
}
