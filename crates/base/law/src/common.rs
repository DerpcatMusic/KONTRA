//! Vendor-neutral laws shared by every format and the engine. Each lives here once; a
//! vendor module names only what is vendor-specific.

use crate::{FormulaId, Law};

/// A 0..=1 setting as −1..=1 (Kontakt 0x14071a840 on 0..=1).
pub const BIPOLAR: Law = Law::Linear { lo: -1.0, hi: 1.0 };

/// A setting's cube: volume, sustain, send levels and state-variable resonance (Kontakt's
/// sustain `(x·x)·x` at 0x140ae48d0; SV resonance `1 − (1 − r)³` at 0x140b05f26).
pub const CUBE: Law = Law::IntegerPower {
    exponent: 3,
    bipolar: false,
};

/// A setting squared: a route's intensity (Kontakt probes/mod_par_laws.lua).
pub const SQUARE: Law = Law::IntegerPower {
    exponent: 2,
    bipolar: false,
};

/// A bipolar route depth `(2x − 1)³` (Kontakt probes/mod_par_laws.lua).
pub const SIGNED_CUBE: Law = Law::IntegerPower {
    exponent: 3,
    bipolar: true,
};

/// An attack-curve knob (−1 concave ..= 1 convex) to an exponential segment shape (Kontakt
/// AHDSR setup 0x140ae4958: `b = e^((1 − |c|)·ln 5·10⁵ − ln 2·10⁴)` rounded to f32, then
/// `ln(b/(1 + b))` or `ln((1 + b)/b)`).
pub const ATTACK_CURVE: Law = Law::Formula(FormulaId::AttackShape);

/// A stage time `2·(base^x − 1)` ms: `exp(ln 2 + (ln(top + 2) − ln 2)·x) − 2`, the
/// shifted exponential with `base = (top + 2) / 2`.
const fn stage(base: f32) -> Law {
    Law::ExpAffine {
        base,
        offset: 0.0,
        factor: 1.0,
        shift: -1.0,
        scale: 2.0,
    }
}

/// Attack and hold: 0..=15 s on the stage law (Kontakt 0x1407b85a0, `C = 8.9228`).
pub const STAGE_15S: Law = stage(7_501.0);
/// Decay and release: 0..=25 s on the stage law (Kontakt 0x1407b85e0, `C = 9.4336`).
pub const STAGE_25S: Law = stage(12_501.0);
/// An LFO's fade-in: 0..=10 s on the stage law (address not located; Kontakt probe
/// probes/mod_par_laws.lua: 500_000 saves 139.45 ms, 1_000_000 10 002 ms).
pub const STAGE_10S: Law = stage(5_001.0);

/// An LFO's fade-in over its fade time: Kontakt (0x140b06c90) runs a state from 0.3 to 1.3
/// by a constant ratio and scales by `state − 0.3`, so `0.3·((13/3)^x − 1)` rises to 1.
pub const LFO_FADE_IN: Law = Law::ExpAffine {
    base: 13.0 / 3.0,
    offset: 0.0,
    factor: 1.0,
    shift: -1.0,
    scale: 0.3,
};

/// The square root: an equal-power mix (UVI's delay and reverb `Mix`) and UVI's upper
/// key follow.
pub const SQUARE_ROOT: Law = Law::Power {
    lo: 0.0,
    hi: 1.0,
    exponent: 0.5,
};

/// A level in dB as linear gain, `10^(dB/20)`.
pub const DB_GAIN: Law = Law::PowIndex {
    base: 10.0,
    origin: 0.0,
    divisor: 20.0,
    scale: 1.0,
};

/// A level in millibels (1/1000 dB, KSP's volume unit) as linear gain.
pub const MILLIBEL_GAIN: Law = Law::PowIndex {
    base: 10.0,
    origin: 0.0,
    divisor: 20_000.0,
    scale: 1.0,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bipolar_maps_normalized_endpoints_and_midpoint() {
        for (x, expected) in [(0.0, -1.0), (0.5, 0.0), (1.0, 1.0)] {
            assert_eq!(BIPOLAR.eval(x), expected, "input {x}");
        }
    }

    #[test]
    fn cube_preserves_original_f32_rounding() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            assert_eq!(CUBE.eval(x), x.powi(3), "cubic at {x}");
        }
        assert_eq!(CUBE.eval(-1.0), 0.0);
    }

    /// `powf(0.5)` and `sqrt` may differ in the last bit; never by more.
    #[test]
    fn square_root_matches_sqrt_to_an_ulp() {
        for n in 0..=100_000 {
            let x = n as f32 / 100_000.0;
            let (got, want) = (SQUARE_ROOT.eval(x), x.sqrt());
            assert!(
                got.to_bits().abs_diff(want.to_bits()) <= 1,
                "{x}: {got} vs {want}"
            );
            let x = f64::from(x);
            let (got, want) = (SQUARE_ROOT.eval_f64(x), x.sqrt());
            assert!(
                got.to_bits().abs_diff(want.to_bits()) <= 1,
                "{x}: {got} vs {want}"
            );
        }
    }

    #[test]
    fn log_levels_keep_the_hand_written_rounding() {
        for mdb in (-200_000..=24_000).step_by(3) {
            let db = mdb as f32 / 1000.0;
            assert_eq!(
                DB_GAIN.eval_unclamped(db).to_bits(),
                10f32.powf(db / 20.0).to_bits()
            );
            let mdb = mdb as f32;
            assert_eq!(
                MILLIBEL_GAIN.eval_unclamped(mdb).to_bits(),
                10f32.powf(mdb / 20_000.0).to_bits()
            );
        }
    }
}
