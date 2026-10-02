//! Kontakt filter types beyond the plain state-variable ones, each reduced
//! to a cascade of the same TPT sections: the linear response of the analog
//! topology, factored into 2-pole sections.
//!
//! - Ladder (AR, Daft): `n` one-pole stages in a feedback loop,
//!   `H = G^n / (1 + k G^n)` with `G = 1 / (1 + s)` (low pass) or
//!   `s / (1 + s)` (high pass). The roots of `(1 + s)^n + k` give each
//!   section's frequency and damping exactly.
//! - Phaser: four first-order all-passes summed with the input,
//!   `(1 + A(s)) / 2`, whose zeros sit at `(√2 ± 1)·fc`.
//! - Formant: three vowel formants as peaking sections, `talk` morphing
//!   A-E-I-O-U, `size` shifting them, `sharp` their bandwidth.

use super::{Proto, Response, filter_settings};
use std::f32::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Model {
    /// One-pole ladder stages: 2 or 4 poles (1 or 2 sections). `compensate`
    /// keeps the pass band at unity as resonance rises (AR); without it the
    /// pass band drops by `1 + k` as on a Moog ladder (Daft).
    Ladder { response: Response, poles: u8, compensate: bool },
    Phaser,
    Formant,
}

/// Feedback at full resonance, just short of self-oscillation (`k = 4`) so
/// the linear model stays stable.
const LADDER_K: f32 = 3.96;
/// 2-pole ladders never self-oscillate; their Q runs 0.5..=28 like the SVFs'.
const LADDER2_Q_SPAN: f32 = 56.0;
/// Formant boost over the rest of the spectrum, and the cut that keeps the
/// level near the input's.
const FORMANT_DB: f32 = 15.0;
const FORMANT_TRIM: f32 = 0.25;
/// F1..F3 (Hz) of the vowels A, E, I, O, U `talk` sweeps through.
const VOWELS: [[f32; 3]; 5] = [
    [730.0, 1090.0, 2440.0],
    [530.0, 1840.0, 2480.0],
    [270.0, 2290.0, 3010.0],
    [570.0, 840.0, 2410.0],
    [300.0, 870.0, 2240.0],
];

impl Model {
    /// 2-pole sections.
    pub(crate) fn sections(self) -> u8 {
        match self {
            Model::Ladder { poles, .. } => poles / 2,
            Model::Phaser => 2,
            Model::Formant => 3,
        }
    }

    /// Knobs it reads: cutoff and resonance, plus size for the formant.
    pub(crate) fn knobs(self) -> usize {
        if self == Model::Formant { 3 } else { 2 }
    }

    /// Section `b` from normalized knobs `[cutoff | talk, resonance | sharp, size]`.
    pub(crate) fn proto(self, key: [f32; 3], b: usize, rate: f32) -> Proto {
        let (hz, _) = filter_settings(key[0], 0.0);
        match self {
            Model::Ladder { response, poles, compensate } => ladder(response, poles, compensate, hz, key[1], b, rate),
            Model::Phaser => phaser(hz, key[1], b, rate),
            Model::Formant => formant(key, b, rate),
        }
    }
}

/// A section at `w · hz` with damping `k` (`s² + k s + 1` around its own
/// frequency) and output mix `m`.
fn section(hz: f32, w: f32, k: f32, m: [f32; 3], rate: f32) -> Proto {
    let g = (PI * (hz * w).min(0.49 * rate) / rate).tan();
    Proto { g, k, m }
}

fn ladder(response: Response, poles: u8, compensate: bool, hz: f32, resonance: f32, b: usize, rate: f32) -> Proto {
    // Low-pass prototype poles as (frequency, damping) per section, and the
    // feedback `k` (DC gain 1 / (1 + k)).
    let (w, damping, k) = if poles == 2 {
        // (1 + s)² + k: ω0 = √(1 + k), damping 2 / ω0.
        let q = 0.5 * LADDER2_Q_SPAN.powf(resonance);
        let k = 4.0 * q * q - 1.0;
        let w = (1.0 + k).sqrt();
        (w, 2.0 / w, k)
    } else {
        // (1 + s)^4 + k: poles -1 + r e^{±iπ/4}, -1 + r e^{±i3π/4}, r = k^¼.
        let k = LADDER_K * resonance;
        let r = k.powf(0.25);
        let re = if b == 0 { -1.0 + r * FRAC_1_SQRT_2 } else { -1.0 - r * FRAC_1_SQRT_2 };
        let im = r * FRAC_1_SQRT_2;
        let w = (re * re + im * im).sqrt();
        (w, -2.0 * re / w, k)
    };
    // The pass-band gain `1 / (1 + k)` goes on the first section.
    let level = if compensate || b > 0 { 1.0 } else { 1.0 / (1.0 + k) };
    match response {
        Response::High => section(hz, 1.0 / w, damping, [level, -damping * level, -level], rate),
        // Centred on the cutoff, peak at unity (the band output peaks at
        // 1 / damping).
        Response::Band => section(hz, 1.0, damping, [0.0, damping * level, 0.0], rate),
        Response::Notch => section(hz, w, damping, [level, -damping * level, 0.0], rate),
        Response::Low => section(hz, w, damping, [0.0, 0.0, level], rate),
    }
}

// ponytail: resonance (the phaser's feedback) is not modelled; the four
// all-pass stages alone set the notches. Factor the feedback loop's
// quartic if a preset leans on it.
fn phaser(hz: f32, _resonance: f32, b: usize, rate: f32) -> Proto {
    // Each section: double pole at fc, zeros at ωz = √2 ∓ 1 (numerator s² + ωz²).
    let wz = if b == 0 { SQRT_2 - 1.0 } else { SQRT_2 + 1.0 };
    section(hz, 1.0, 2.0, [1.0, -2.0, wz * wz - 1.0], rate)
}

fn formant([talk, sharp, size]: [f32; 3], b: usize, rate: f32) -> Proto {
    let x = talk.clamp(0.0, 1.0) * (VOWELS.len() - 1) as f32;
    let (i, t) = ((x as usize).min(VOWELS.len() - 2), x.fract());
    let t = if x >= (VOWELS.len() - 1) as f32 { 1.0 } else { t };
    let hz = VOWELS[i][b] + (VOWELS[i + 1][b] - VOWELS[i][b]) * t;
    let hz = hz * (2.0 * (size - 0.5)).exp2();
    let q = 2.0 + 18.0 * sharp.clamp(0.0, 1.0);
    let bw = 2.0 / std::f32::consts::LN_2 * (1.0 / (2.0 * q)).asinh();
    let mut p = Proto::bell(hz, bw, FORMANT_DB, rate);
    if b == 0 {
        p.m = p.m.map(|m| m * FORMANT_TRIM);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Chain magnitude in dB at `hz`.
    fn db(model: Model, key: [f32; 3], hz: f32) -> f32 {
        let gain: f32 = (0..model.sections() as usize).map(|b| model.proto(key, b, RATE).gain(hz, RATE)).product();
        20.0 * gain.log10()
    }

    fn cutoff_of(hz: f32) -> f32 {
        (hz / super::super::CUTOFF_MIN_HZ).log2() / super::super::CUTOFF_OCTAVES
    }

    #[test]
    fn ladders_have_their_slopes_and_resonance() {
        let c = cutoff_of(1000.0);
        let lp4 = Model::Ladder { response: Response::Low, poles: 4, compensate: true };
        // Four stages at fc: -12 dB there, 24 dB/octave far above.
        assert!((db(lp4, [c, 0.0, 0.0], 1000.0) + 12.0).abs() < 0.5);
        assert!(db(lp4, [c, 0.0, 0.0], 50.0).abs() < 0.2);
        let slope = db(lp4, [c, 0.0, 0.0], 8000.0) - db(lp4, [c, 0.0, 0.0], 16000.0);
        assert!(slope > 20.0, "{slope}");
        // Resonance peaks near fc and, uncompensated, drops the bass.
        assert!(db(lp4, [c, 0.9, 0.0], 1000.0) > 10.0);
        let uncompensated = Model::Ladder { response: Response::Low, poles: 2, compensate: false };
        assert!(db(uncompensated, [c, 1.0, 0.0], 50.0) < -12.0);
        let hp2 = Model::Ladder { response: Response::High, poles: 2, compensate: true };
        assert!(db(hp2, [c, 0.0, 0.0], 100.0) < -30.0 && db(hp2, [c, 0.0, 0.0], 15000.0).abs() < 0.5);
        let bp2 = Model::Ladder { response: Response::Band, poles: 2, compensate: true };
        assert!(db(bp2, [c, 0.5, 0.0], 1000.0).abs() < 0.5 && db(bp2, [c, 0.5, 0.0], 100.0) < -20.0);
    }

    #[test]
    fn phaser_notches_follow_the_cutoff() {
        for hz in [500.0, 2000.0] {
            let key = [cutoff_of(hz), 0.3, 0.0];
            let notch = db(Model::Phaser, key, hz * (SQRT_2 - 1.0)).min(db(Model::Phaser, key, hz * (SQRT_2 + 1.0)));
            assert!(notch < -40.0, "{hz}: {notch}");
            assert!(db(Model::Phaser, key, hz).abs() < 6.0);
            assert!(db(Model::Phaser, key, 20.0).abs() < 0.5);
        }
    }

    #[test]
    fn formants_move_with_talk_and_size() {
        // A's F1 (730 Hz) stands out against the gap between F1 and F2.
        assert!(db(Model::Formant, [0.0, 0.5, 0.5], 730.0) - db(Model::Formant, [0.0, 0.5, 0.5], 1600.0) > 8.0);
        // I has F1 at 270 Hz; size up an octave moves it to 540.
        let i = 0.5;
        assert!(db(Model::Formant, [i, 0.5, 0.5], 270.0) > db(Model::Formant, [i, 0.5, 1.0], 270.0) + 6.0);
        assert!(db(Model::Formant, [i, 0.5, 1.0], 540.0) > db(Model::Formant, [i, 0.5, 0.5], 540.0) + 6.0);
    }
}
