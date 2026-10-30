//! UVI parameter curves.

use crate::{FormulaId, Law};

/// An envelope segment's curve attribute (−1..=1) to the IR's segment shape
/// `2·ln((1 + c)/(1 − c))`, `c` clamped to ±0.9998 (setup 0x1410f1220).
pub const ENVELOPE_CURVE: Law = Law::Formula(FormulaId::LogRatio);

/// The WaveShaper's `Amount` (0..=1) to its drive `2^(8x)` (address not located).
pub const WAVESHAPER_DRIVE: Law = Law::Exp2Index {
    origin: 0.0,
    divisor: 0.125,
    offset: 0.0,
    scale: 1.0,
};

/// The DiodeClipper's `Asymmetry` in decades to a factor `10^x` (address not located).
pub const CLIPPER_ASYMMETRY: Law = Law::PowIndex {
    base: 10.0,
    origin: 0.0,
    divisor: 1.0,
    scale: 1.0,
};

/// The key/velocity fade edge (low-key branch at 0x140f28cba).
/// Finite curves are clamped to 1..=2; non-finite curves use the default of 2.
#[must_use]
pub fn fade_edge(curve: f32) -> Law {
    let curve = if curve.is_finite() {
        curve.clamp(1.0, 2.0)
    } else {
        2.0
    };
    Law::Sine { power: 2.0 / curve }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_curves_keep_the_hand_written_rounding() {
        for n in -10_000..=10_000 {
            let x = n as f32 / 10_000.0;
            let c = f64::from(x).clamp(-0.9998, 0.9998);
            assert_eq!(
                ENVELOPE_CURVE.eval_unclamped_f64(f64::from(x)) as f32,
                (2.0 * ((1.0 + c) / (1.0 - c)).ln()) as f32,
                "curve {x}"
            );
            assert_eq!(
                CLIPPER_ASYMMETRY.eval_unclamped(x),
                10f32.powf(x),
                "asymmetry {x}"
            );
            let amount = x.clamp(0.0, 1.0);
            assert_eq!(
                WAVESHAPER_DRIVE.eval(x),
                (8.0 * amount).exp2(),
                "amount {x}"
            );
        }
    }

    #[test]
    fn fade_edge_preserves_the_sampled_quarter_sine() {
        for curve in [1.0, 1.5, 2.0] {
            let law = fade_edge(curve);
            for n in 0..=127 {
                let x = n as f32 / 127.0;
                let expected = (std::f32::consts::FRAC_PI_2 * x).sin().powf(2.0 / curve);
                assert_eq!(law.eval(x), expected, "curve {curve}, input {x}");
            }
            assert_eq!(law.eval(-1.0), 0.0);
            assert_eq!(law.eval(2.0), 1.0);
        }
    }

    #[test]
    fn fade_edge_validates_curve() {
        assert_eq!(fade_edge(-1.0), fade_edge(1.0));
        assert_eq!(fade_edge(3.0), fade_edge(2.0));
        assert_eq!(fade_edge(f32::NAN), fade_edge(2.0));
        assert_eq!(fade_edge(f32::INFINITY), fade_edge(2.0));
    }
}
