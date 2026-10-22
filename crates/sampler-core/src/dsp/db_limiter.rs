//! Stereo dB-domain peak limiter: Kontakt 8 `NI::DSP::Limiter` (vtable `14769c220`; constructor
//! `141adc050`, parameter setter `141aea740`, block process `141ae3ba0`). Generic: no product names.
//!
//! CONFIRMED (decompile arithmetic): the five parameter laws; the detector candidates
//! `|G0 x + 1e-12|` per channel and their maximum in dB (`8.6858902 ln`); the dB envelope
//! `env = level + (env - level) c` with `c` the release when the level falls; the gain
//! `-max(0, env)` dB; the output clamp to +-2.5.
//! INFERRED: the detector's role (the peak of the driven pair) and the release unit. The release is
//! `1 - 1 / (tau rate)`, `tau = max(6 v, 0.001)`, with the rate read from `L+0x40` (writer not decoded).
//! UNKNOWN, ponytail: the attack coefficient (`L+0xcc`) has no decoded writer, so attack is 0
//! (instant). With that, the mode (index 4) changes nothing, and the second dB state (`L+0xbc`,
//! coefficients `L+0xc0`/`L+0xc4`) stays under the envelope, so it is omitted.
//! The dB-to-linear table (`T4`, linear in 1/60 octave) is replaced by `powf`; the difference is below 2e-5.
#![allow(dead_code)] // not wired: the Replika matrix that would host it is not implemented yet.

/// Detector floor, added before the magnitude (f32 `144f385a8`).
const FLOOR: f32 = 1e-12;
/// 20 / ln 10: dB per neper (f32 `1479ab570`, bit pattern 0x410af968).
const DB_PER_NEPER: f32 = f32::from_bits(0x410a_f968);
/// Output clamp (f32 `144fc40e0` = 2.5, `145494e5c` = -2.5).
const OUTPUT_LIMIT: f32 = 2.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DbLimiter {
    rate: f32,
    input_gain: f32,
    output_gain: f32,
    release: f32,
    side: bool,
    /// DB envelope (`L+0xc8`).
    envelope: f32,
}

impl DbLimiter {
    /// Unity gains and instant release until the setter runs (the constructor's values are not decoded).
    pub fn new(rate: f32) -> Self {
        Self { rate, input_gain: 1., output_gain: 1., release: 0., side: false, envelope: 0. }
    }

    /// Parameter `index` (decompile `141aea740`), value in 0..1.
    pub fn set(&mut self, index: u32, value: f32) {
        match index {
            // G0 = 10^(40 (1 - v) / 20): 100 at v = 0, 1 at v = 1.
            0 => self.input_gain = 10f32.powf(2. * (1. - value)),
            // Release: tau = max(6 v, 0.001) with the coefficient 1 - 1 / (tau rate).
            1 => self.release = 1. - 1. / ((6. * value).max(0.001) * self.rate),
            // G2 = 10^(-40 (1 - v) / 20): 0.01 at v = 0, 1 at v = 1.
            2 => self.output_gain = 10f32.powf(-2. * (1. - value)),
            3 => self.side = value > 0.5,
            // Mode: the peak-attack switch only changes the attack law, which is not decoded.
            _ => {}
        }
    }

    /// One stereo frame (`141ae3ba0`). `sidechain` is read only when the side flag is set; the
    /// signal is driven by G0 only in that case, and the detector always sees G0.
    pub fn process(&mut self, input: [f32; 2], sidechain: [f32; 2]) -> [f32; 2] {
        let (key, drive) = if self.side { (sidechain, self.input_gain) } else { (input, 1.) };
        let g0 = self.input_gain;
        // ponytail: MIN_POSITIVE guards the log; the floored sum's magnitude can reach 0.
        let level = (g0 * key[0] + FLOOR).abs().max((g0 * key[1] + FLOOR).abs()).max(f32::MIN_POSITIVE);
        let level_db = DB_PER_NEPER * level.ln();
        // Falling level uses the release; rising uses the attack, which is 0 (see the header).
        let coefficient = if level_db <= self.envelope { self.release } else { 0. };
        self.envelope = level_db + (self.envelope - level_db) * coefficient;
        let gain = 10f32.powf(-self.envelope.max(0.) / 20.);
        input.map(|x| (drive * x * self.output_gain * gain).clamp(-OUTPUT_LIMIT, OUTPUT_LIMIT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_reduces_an_over_threshold_input_to_full_scale() {
        let mut lim = DbLimiter::new(48_000.);
        let out = lim.process([4., 0.], [0., 0.]);
        // 4 x 10^(-12.04 / 20) = 1, the 0 dB ceiling of the default unity gains.
        assert!((out[0] - 1.).abs() < 1e-4, "{out:?}");
    }

    #[test]
    fn limiter_passes_an_input_under_the_threshold_unchanged() {
        let mut lim = DbLimiter::new(48_000.);
        assert_eq!(lim.process([0.5, -0.25], [0., 0.]), [0.5, -0.25]);
    }

    #[test]
    fn limiter_output_never_exceeds_plus_minus_two_and_a_half() {
        let mut lim = DbLimiter::new(48_000.);
        lim.set(0, 0.);
        lim.set(2, 1.);
        lim.set(3, 1.);
        for i in 0..2_000 {
            let x = (i as f32 * 0.37).sin() * 1_000. * (1 + i % 3) as f32;
            let out = lim.process([x, -0.5 * x], [x, -0.5 * x]);
            assert!(out.iter().all(|v| v.is_finite() && v.abs() <= OUTPUT_LIMIT), "{i}: {out:?}");
        }
    }
}
