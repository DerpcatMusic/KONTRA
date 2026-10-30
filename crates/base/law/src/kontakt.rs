//! Kontakt controller and modulation laws.
//!
//! The modulation compiler at 0x140873120 emits plain and `fcubic` routes;
//! the source classification is decoded by the format translator.

use crate::Law;

/// Normalized controller or script input used by a volume route.
pub const CONTROLLER_VOLUME_POWER: u8 = 1;
/// Normalized voice source used by a volume route.
pub const VOICE_VOLUME_POWER: u8 = 3;

/// Controller source taper in a volume route.
pub const CONTROLLER_VOLUME: Law = Law::Power {
    lo: 0.0,
    hi: 1.0,
    exponent: CONTROLLER_VOLUME_POWER as f32,
};
/// Voice source taper in a volume route.
pub const VOICE_VOLUME: Law = Law::Power {
    lo: 0.0,
    hi: 1.0,
    exponent: VOICE_VOLUME_POWER as f32,
};

/// Maps normalized input to a bipolar value (0x14071a840 on 0..=1).
pub const BIPOLAR: Law = Law::Linear { lo: -1.0, hi: 1.0 };

/// Note-to-frequency cutoff law; see `docs/re/engine/filt-sv.md`.
pub const FILTER_NOTE_HZ: Law = Law::PowIndex {
    base: 2.0,
    origin: 69.0,
    divisor: 12.0,
    scale: 440.0,
};

/// Cubic sustain and state-variable filter resonance mappings; see
/// `docs/re/engine/k-mod-envelopes.md` and `filt-sv.md`.
pub const UNIT_CUBIC: Law = Law::IntegerPower { exponent: 3 };

/// Fifth-power state-variable filter gain mapping; see `filt-sv.md`.
pub const UNIT_FIFTH: Law = Law::IntegerPower { exponent: 5 };

/// Replika Modern low-cut curve; measured by the native oracle, see
/// `docs/re/engine/k-replika-binding.md`. Zero is bypassed by the translator.
pub const REPLIKA_LOW_CUT: Law = Law::ExpAffine {
    base: 100.0,
    offset: 0.0,
    factor: 1.0,
    shift: 0.0,
    scale: 10.0,
};

/// Versatile filter gain; see `docs/re/engine/filt-multi-versatile.md`.
pub const VERSATILE_GAIN: Law = Law::ExpAffine {
    base: 10.0,
    offset: -0.5,
    factor: 3.0,
    shift: 0.0,
    scale: 1.0,
};

/// Convolution predelay's sampled exponential; oracle evidence is documented
/// beside the translator in `format/kontakt/src/impulse.rs`.
pub const CONVOLUTION_PREDELAY: Law = Law::ExpAffine {
    base: 151.0,
    offset: 0.0,
    factor: 1.0,
    shift: -1.0,
    scale: 2.0,
};

/// Saved modulation-shaper points as an evaluator law.
///
/// Kontakt 0x14087c7f0 scales curvature by 16 and reverses it on a falling
/// segment. The evaluator uses the exponential curve without native's fast
/// exponential approximation.
#[must_use]
pub fn modulation_shaper(points: &[(f32, f32, f32)]) -> Law {
    Law::Breakpoints(
        points
            .iter()
            .enumerate()
            .map(|(i, &(x, y, curve))| {
                let direction = points
                    .get(i + 1)
                    .map_or(1.0, |next| if next.1 < y { -1.0 } else { 1.0 });
                (x, y, 16.0 * curve * direction)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_note_law_preserves_the_original_float_calculation() {
        for i in 0..=16000 {
            let note = i as f32 / 100.0;
            let original = 440.0 * 2f32.powf((note - 69.0) / 12.0);
            assert_eq!(
                FILTER_NOTE_HZ.eval_unclamped(note).to_bits(),
                original.to_bits()
            );
        }
    }

    /// The normalized bipolar mapping agrees with native endpoints and midpoint.
    #[test]
    fn bipolar_curve_maps_normalized_endpoints_and_midpoint() {
        for (x, expected) in [(0.0, -1.0), (0.5, 0.0), (1.0, 1.0)] {
            assert_eq!(BIPOLAR.eval(x), expected, "input {x}");
        }
    }

    #[test]
    fn cubic_preserves_original_f32_rounding() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            assert_eq!(UNIT_CUBIC.eval(x), x.powi(3), "cubic at {x}");
        }
        assert_eq!(UNIT_CUBIC.eval(-1.0), 0.0);
    }

    #[test]
    fn fifth_preserves_original_f32_rounding() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            assert_eq!(UNIT_FIFTH.eval(x), x.powi(5), "fifth at {x}");
        }
        assert_eq!(UNIT_FIFTH.eval(2.0), 1.0);
    }

    #[test]
    fn low_cut_preserves_original_rounding() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            assert_eq!(
                REPLIKA_LOW_CUT.eval(x),
                10.0 * 100f32.powf(x),
                "low cut {x}"
            );
        }
    }

    #[test]
    fn versatile_gain_preserves_original_rounding() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            assert_eq!(
                VERSATILE_GAIN.eval(x),
                10f32.powf((x - 0.5) * 3.0),
                "gain {x}"
            );
        }
    }

    #[test]
    fn convolution_predelay_preserves_original_rounding() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            assert_eq!(
                CONVOLUTION_PREDELAY.eval(x),
                2.0 * (151f32.powf(x) - 1.0),
                "predelay {x}"
            );
        }
    }
}
