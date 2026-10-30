//! Kontakt controller and modulation laws.
//!
//! The modulation compiler at 0x140873120 emits plain and `fcubic` routes;
//! the source classification is decoded by the format translator.

use std::sync::LazyLock;

use crate::{FormulaId, Law};

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

/// Note-to-frequency cutoff law; see `docs/re/engine/filt-sv.md`.
pub const FILTER_NOTE_HZ: Law = Law::PowIndex {
    base: 2.0,
    origin: 69.0,
    divisor: 12.0,
    scale: 440.0,
};

/// Fifth-power state-variable filter gain mapping; see `filt-sv.md`.
pub const UNIT_FIFTH: Law = Law::IntegerPower {
    exponent: 5,
    bipolar: false,
};

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

/// A flex-envelope point's curve knob to its segment shape (0x140ae5a10).
pub const FLEX_SHAPE: Law = Law::Formula(FormulaId::FlexShape);

/// A legacy EQ band's stored gain (dB) to its peak (dB): `A = 2^(dB/6)` doubles it
/// (generator 0x1409591c0, `docs/re/engine/eq-kontakt.md` §1.1).
pub const EQ_PEAK_DB: Law = Law::Linear {
    lo: 0.0,
    hi: 20.0 * std::f32::consts::LOG10_2 / 3.0,
};

/// The Ladder's gain knob (−1..=1) to linear gain `2^(1.993 g)`; see
/// `docs/re/engine/filt-ladder-monark.md`.
pub const LADDER_GAIN: Law = Law::ExpAffine {
    base: 2.0,
    offset: 0.0,
    factor: 1.993,
    shift: 0.0,
    scale: 1.0,
};

/// The legacy filters' resonance knob to the sharpest stage's Q gain, `e^(6.4 r)` (fitted to
/// the measured LP4 peaks; `docs/re/engine/filt-legacy.md`).
pub const LEGACY_RESONANCE_Q: Law = Law::Exp {
    lo: 1.0,
    hi: 601.845,
};

/// The legacy filters' passband loss at full resonance for 2, 4 and 6 poles (fitted, as
/// [`LEGACY_RESONANCE_Q`]).
pub const LEGACY_RESONANCE_LOSS: [Law; 3] = [
    Law::Db { lo: 0.0, hi: -5.5 },
    Law::Db { lo: 0.0, hi: -11.0 },
    Law::Db { lo: 0.0, hi: -24.0 },
];

/// Fraction of the legacy cutoff's 21 759.5 Hz span the setters' exp series reaches at the
/// knob's end, before the 0..=1 clamp: `(e^(6.15 u) − 1) · this`.
const LEGACY_CUTOFF_SLOPE: f32 = 0.002_572_831_7;

/// The legacy setters' exp series to Hz, `span · ((e^(6.15 u) − 1) · 0.0025728 + 0.002)`
/// with the series clamped to 1: one exponential segment, flat once the series passes 1
/// near u = 0.97 (`docs/re/engine/filt-legacy.md`).
#[must_use]
pub fn legacy_exp_series(span_hz: f32) -> Law {
    let curve = (1.0 / LEGACY_CUTOFF_SLOPE).ln_1p();
    let (lo, hi) = (span_hz * 0.002, span_hz * 1.002);
    Law::Breakpoints(std::sync::Arc::from([
        (0.0, lo, curve),
        (curve / 6.15, hi, 0.0),
        (1.0, hi, 0.0),
    ]))
}

/// The legacy filters' cutoff knob to Hz: [`legacy_exp_series`] over 21 759.5 Hz.
pub static LEGACY_CUTOFF_HZ: LazyLock<Law> = LazyLock::new(|| legacy_exp_series(21_759.5));

/// The legacy reverb's damping-derived position to its high cut: [`legacy_exp_series`] over
/// `22006 · 0.988793` Hz.
pub static LEGACY_REVERB_HIGH_CUT_HZ: LazyLock<Law> =
    LazyLock::new(|| legacy_exp_series(22_006.0 * 0.988_793));

/// The feedback compressor's ratio to its knee start `max(r, 2)^0.2 · 0.70394 − 0.67251`
/// (ratio law 0x141a91d10); evaluate at the ratio, at least 2.
pub const FEEDBACK_THRESHOLD: Law = Law::Power {
    lo: -0.672_507_3,
    hi: 0.703_942_9 - 0.672_507_3,
    exponent: 0.2,
};

/// The feedback compressor's ratio to its knee width factor (0x141a91d10).
pub const FEEDBACK_KNEE: Law = Law::Formula(FormulaId::FeedbackKnee);

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
                original.to_bits(),
                "note {note}"
            );
        }
    }

    #[test]
    fn eq_peak_doubles_the_stored_db() {
        for i in -1800..=1800 {
            let db = i as f32 / 100.0;
            let original = (2f32.powf(db / 3.0)).log10() * 20.0;
            let got = EQ_PEAK_DB.eval_unclamped(db);
            assert!((got - original).abs() <= 2e-5, "{db}: {got} vs {original}");
        }
    }

    #[test]
    fn ladder_gain_preserves_original_rounding() {
        for i in -10_000..=10_000 {
            let g = i as f32 / 10_000.0;
            assert_eq!(
                LADDER_GAIN.eval_unclamped(g).to_bits(),
                2f32.powf(1.993 * g).to_bits(),
                "gain {g}"
            );
        }
    }

    #[test]
    fn legacy_resonance_matches_the_fitted_exponentials() {
        for n in 0..=10_000 {
            let r = n as f32 / 10_000.0;
            let q = (6.4 * r).exp();
            assert!(
                (LEGACY_RESONANCE_Q.eval(r) / q - 1.0).abs() < 1e-5,
                "q at {r}"
            );
            for (law, drop) in LEGACY_RESONANCE_LOSS.iter().zip([5.5f32, 11.0, 24.0]) {
                assert_eq!(
                    law.eval(r).to_bits(),
                    10f32.powf(-drop * r / 20.0).to_bits(),
                    "loss {drop} dB at {r}"
                );
            }
        }
    }

    #[test]
    fn feedback_compressor_laws_match_the_fitted_forms() {
        for n in 0..=19_000 {
            let r = 1.0 + n as f32 / 1000.0;
            let threshold = r.max(2.0).powf(0.2) * 0.703_942_9 - 0.672_507_3;
            let got = FEEDBACK_THRESHOLD.eval_unclamped(r.max(2.0));
            assert!((got - threshold).abs() < 1e-6, "{r}: {got} vs {threshold}");
            let knee = 3.601_805_4 / r + 0.840_284_1 - 1.440_552_4 / (r.sqrt() * r) + 0.05;
            assert_eq!(FEEDBACK_KNEE.eval_unclamped(r).to_bits(), knee.to_bits());
        }
    }

    #[test]
    fn legacy_reverb_high_cut_matches_the_exp_series() {
        for n in 0..=10_000 {
            let x = n as f32 / 10_000.0;
            let corner = ((6.15 * x).exp() - 1.0) * LEGACY_CUTOFF_SLOPE;
            let original = (corner.clamp(0.0, 1.0) + 0.002) * 22_006.0 * 0.988_793;
            let got = LEGACY_REVERB_HIGH_CUT_HZ.eval(x);
            assert!(
                (got / original - 1.0).abs() < 2e-5,
                "{x}: {got} vs {original}"
            );
        }
    }

    #[test]
    fn legacy_cutoff_matches_the_exp_series() {
        for n in 0..=10_000 {
            let u = n as f32 / 10_000.0;
            let v = ((6.15 * u).exp_m1() * LEGACY_CUTOFF_SLOPE).clamp(0.0, 1.0);
            let original = 21_759.5 * (v + 0.002);
            let got = LEGACY_CUTOFF_HZ.eval(u);
            assert!(
                (got / original - 1.0).abs() < 2e-5,
                "{u}: {got} vs {original}"
            );
        }
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
