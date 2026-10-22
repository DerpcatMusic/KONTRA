//! Stereo dB-domain peak limiter: Kontakt 8 `NI::DSP::Limiter` (vtable `14769c220`; constructor
//! `141adc050`, rate setup `141ae28c0` (slot 3), parameter setter `141aea740` (slot 12), block
//! process `141ae3ba0` (slot 7)). Generic: no product names.
//!
//! CONFIRMED (decompile arithmetic): the five parameter laws; the detector candidates
//! `|G0 x + 1e-12|` per channel and their maximum in dB (`8.6858902 ln`); the rate setup, which
//! stores the rate at `L+0x40` and computes attack `1 - 200 / rate` (`144f38ce4`), second-state
//! rise `1 - (1/15) / rate` (`144f386a8`) and fall `1 - (1/3) / rate` (`144760108`); the release
//! `1 - 1 / (tau rate)` with `tau` from the setter; the first dB envelope (instant rise when index 4
//! is set, else the attack; release on fall); the second dB state; the gain `-max(0, env, hold)` dB;
//! the output clamp to +-2.5.
//! INFERRED: the detector's role (the peak of the driven pair). UNKNOWN: the release before the
//! first index-1 setter call (`L+0xb0` default, so `release` starts at 0), and the ramp lengths of
//! `141ae28c0` (`L+0x88`, `L+0x8c`), which are not applied (parameters apply at once). The rebuild
//! `141ae6c60` writes release and hold coefficients too, but it is reached from another class's
//! function (`141ae9e40`), not from a slot of this vtable; not ported.
//! The dB-to-linear table (`T4`) is replaced by `powf`; the difference is below 2e-5.
#![allow(dead_code)] // not wired: the Replika matrix that would host it is not implemented yet.

/// Detector floor, added before the magnitude (f32 `144f385a8` = 1e-12).
const FLOOR: f32 = 1e-12;
/// 20 / ln 10: dB per neper (f32 `1479ab570`, bit pattern 0x410af968).
const DB_PER_NEPER: f32 = f32::from_bits(0x410a_f968);
/// Output clamp (f32 `144fc40e0` = 2.5, `145494e5c` = -2.5).
const OUTPUT_LIMIT: f32 = 2.5;
/// Attack: coefficient `1 - ATTACK_HZ / rate` (f32 `144f38ce4` = 200).
const ATTACK_HZ: f32 = 200.;
/// Second-state rise: `1 - HOLD_RISE_HZ / rate` (f32 `144f386a8` = 1/15).
const HOLD_RISE_HZ: f32 = 1. / 15.;
/// Second-state fall: `1 - HOLD_FALL_HZ / rate` (f32 `144760108` = 1/3).
const HOLD_FALL_HZ: f32 = 1. / 3.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DbLimiter {
    rate: f32,
    input_gain: f32,
    output_gain: f32,
    release: f32,
    attack: f32,
    hold_rise: f32,
    hold_fall: f32,
    side: bool,
    instant: bool,
    /// First dB envelope (`L+0xc8`).
    envelope: f32,
    /// Second dB state (`L+0xbc`).
    hold: f32,
}

impl DbLimiter {
    /// Unity gains and zero release until the setter runs (the constructor's values are not decoded).
    pub fn new(rate: f32) -> Self {
        Self {
            rate,
            input_gain: 1.,
            output_gain: 1.,
            release: 0.,
            attack: 1. - ATTACK_HZ / rate,
            hold_rise: 1. - HOLD_RISE_HZ / rate,
            hold_fall: 1. - HOLD_FALL_HZ / rate,
            side: false,
            instant: false,
            envelope: 0.,
            hold: 0.,
        }
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
            // Mode (`L+0xad` = v >= 0.5): the first envelope rises at once, with no attack coefficient.
            4 => self.instant = value >= 0.5,
            _ => {}
        }
    }

    /// One stereo frame (`141ae3ba0`). `sidechain` is read only when the side flag is set; the
    /// drive is G0 only in that case, and the detector always sees G0.
    pub fn process(&mut self, input: [f32; 2], sidechain: [f32; 2]) -> [f32; 2] {
        let (key, drive) = if self.side {
            (sidechain, self.input_gain)
        } else {
            (input, 1.)
        };
        let g0 = self.input_gain;
        // ponytail: MIN_POSITIVE guards the log; the floored sum's magnitude can reach 0.
        let level = (g0 * key[0] + FLOOR)
            .abs()
            .max((g0 * key[1] + FLOOR).abs())
            .max(f32::MIN_POSITIVE);
        let level_db = DB_PER_NEPER * level.ln();
        // First envelope: falling uses the release; rising uses the attack, or jumps to the level in
        // the instant mode.
        self.envelope = if level_db <= self.envelope {
            level_db + (self.envelope - level_db) * self.release
        } else if self.instant {
            level_db
        } else {
            level_db + (self.envelope - level_db) * self.attack
        };
        // Second state: the same level, with the rise or fall coefficient.
        let hold_coefficient = if level_db <= self.hold {
            self.hold_fall
        } else {
            self.hold_rise
        };
        self.hold = level_db + (self.hold - level_db) * hold_coefficient;
        let gain = 10f32.powf(-self.envelope.max(self.hold).max(0.) / 20.);
        input.map(|x| (drive * x * self.output_gain * gain).clamp(-OUTPUT_LIMIT, OUTPUT_LIMIT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.;

    /// Attack and second-state coefficients from the rate setup (`141ae28c0`): `1 - K / rate` with
    /// K = 200, 1/15 and 1/3 (constants 144f38ce4, 144f386a8, 144760108).
    #[test]
    fn rate_setup_coefficients_match_the_decoded_constants() {
        let lim = DbLimiter::new(FS);
        assert!((lim.attack - (1. - 200. / FS)).abs() < 1e-6);
        assert!((lim.hold_rise - (1. - (1. / 15.) / FS)).abs() < 1e-6);
        assert!((lim.hold_fall - (1. - (1. / 3.) / FS)).abs() < 1e-6);
    }

    /// Mode 0: the first envelope rises by the attack coefficient, so the first frame sees
    /// `level dB x (200 / rate)` (`141ae3ba0`, `1 - 200 / rate`).
    #[test]
    fn attack_coefficient_sets_the_first_frame_gain() {
        let mut lim = DbLimiter::new(FS);
        let out = lim.process([1.5, 0.], [0., 0.]);
        let level_db = 20. * 1.5f32.log10();
        let want = 1.5 * 10f32.powf(-level_db * (200. / FS) / 20.);
        assert!((out[0] - want).abs() < 1e-5, "{out:?} vs {want}");
    }

    /// Instant mode (index 4 set): 4 x 10^(-12.04 / 20) = 1, the 0 dB ceiling of the unity gains.
    #[test]
    fn instant_mode_reaches_the_peak_on_the_first_frame() {
        let mut lim = DbLimiter::new(FS);
        lim.set(4, 1.);
        let out = lim.process([4., 0.], [0., 0.]);
        assert!((out[0] - 1.).abs() < 1e-4, "{out:?}");
    }

    /// Second state (`L+0xbc`): above the envelope it sets the gain, so the output carries
    /// `-hold` dB (`141ae3ba0` takes the gain from max(env, hold)).
    #[test]
    fn second_db_state_sets_the_gain_when_above_the_envelope() {
        let mut lim = DbLimiter::new(FS);
        lim.hold = 6.;
        let out = lim.process([0.5, 0.], [0., 0.]);
        let want = 0.5 * 10f32.powf(-6. / 20.);
        assert!((out[0] - want).abs() < 1e-4, "{out:?} vs {want}");
    }

    #[test]
    fn limiter_passes_an_input_under_the_threshold_unchanged() {
        let mut lim = DbLimiter::new(FS);
        assert_eq!(lim.process([0.5, -0.25], [0., 0.]), [0.5, -0.25]);
    }

    #[test]
    fn limiter_output_never_exceeds_plus_minus_two_and_a_half() {
        let mut lim = DbLimiter::new(FS);
        lim.set(0, 0.);
        lim.set(2, 1.);
        lim.set(3, 1.);
        for i in 0..2_000 {
            let x = (i as f32 * 0.37).sin() * 1_000. * (1 + i % 3) as f32;
            let out = lim.process([x, -0.5 * x], [x, -0.5 * x]);
            assert!(
                out.iter().all(|v| v.is_finite() && v.abs() <= OUTPUT_LIMIT),
                "{i}: {out:?}"
            );
        }
    }
}
