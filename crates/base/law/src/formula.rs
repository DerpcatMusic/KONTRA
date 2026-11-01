//! Closed forms that no point list reproduces exactly.

use crate::real::Real;

/// Names a closed form. Add a variant, its arm in [`FormulaId::eval`] and a test when a law
/// needs one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaId {
    /// A bipolar attack-curve knob (−1 concave ..= 1 convex) to an exponential segment shape.
    /// Its base is rounded to f32 before the log, as the native envelope does
    /// (`docs/re/engine/k-mod-envelopes.md`).
    AttackShape,
    /// A flex-envelope curve knob (0.5 linear, 0 read as linear) to a segment shape: the log of
    /// the level ratio `(1/f − 1)²`, `f` the knob's quadratic ease-in/out; knobs below 0.001
    /// read `f = 2·10⁻⁶` and above 0.999 `f ≈ 0.999998` (Kontakt 0x140ae5a10).
    FlexShape,
    /// A feedback compressor's ratio `r` to its knee factor
    /// `3.6018/r + 0.8903 − 1.4406/r^1.5` (Kontakt ratio law 0x141a91d10).
    FeedbackKnee,
    /// A bipolar curve knob `c` to `2·ln((1 + c)/(1 − c))` with `c` clamped to ±0.9998,
    /// computed in f64 (UVI envelope setup 0x1410f1220).
    LogRatio,
}

impl FormulaId {
    /// Evaluates the closed form at `x` in its own source units.
    #[must_use]
    pub fn eval(self, x: f32) -> f32 {
        self.at(x)
    }

    pub(crate) fn at<T: Real>(self, x: T) -> T {
        match self {
            Self::AttackShape => T::of(attack_shape(x.to_f64())),
            Self::FlexShape => T::of(flex_ratio(x.to_f32()).ln()),
            Self::FeedbackKnee => T::of(feedback_knee(x.to_f32())),
            Self::LogRatio => T::of(log_ratio(x.to_f64())),
        }
    }

    /// The input this closed form maps to `y`, where an inverse is known.
    pub(crate) fn inverse(self, y: f64) -> Option<f64> {
        match self {
            Self::AttackShape => Some(attack_curve(y)),
            Self::FlexShape | Self::FeedbackKnee | Self::LogRatio => None,
        }
    }
}

/// The attack base's span and floor: `b = e^((1 − |c|)·ln ATTACK_SPAN − ln ATTACK_FLOOR)`, so
/// `b` runs from 1/20 000 at a full bend to 25 when straight
/// (`docs/re/engine/k-mod-envelopes.md`). [`attack_curve`] inverts with the same values.
const ATTACK_SPAN: f64 = 500_000.0;
const ATTACK_FLOOR: f64 = 20_000.0;

fn attack_shape(curve: f64) -> f32 {
    let c = curve.clamp(-1.0, 1.0);
    let b = f64::from(((1.0 - c.abs()) * ATTACK_SPAN.ln() - ATTACK_FLOOR.ln()).exp() as f32);
    let k = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    k as f32
}

/// The bipolar knob whose [`attack_shape`] is `shape`.
fn attack_curve(shape: f64) -> f64 {
    if shape == 0.0 {
        return 0.0;
    }
    let b = 1.0 / shape.abs().exp_m1();
    let c = (1.0 - (b.ln() + ATTACK_FLOOR.ln()) / ATTACK_SPAN.ln()).clamp(0.0, 1.0);
    -shape.signum() * c
}

fn log_ratio(c: f64) -> f32 {
    let c = c.clamp(-0.9998, 0.9998);
    (2.0 * ((1.0 + c) / (1.0 - c)).ln()) as f32
}

fn feedback_knee(r: f32) -> f32 {
    3.601_805_4 / r + 0.840_284_1 - 1.440_552_4 / (r.sqrt() * r) + 0.05
}

fn flex_ratio(c: f32) -> f32 {
    let f = if c == 0.0 {
        0.5
    } else if c < 0.001 {
        0.001 * 0.002
    } else if c > 0.999 {
        (4.0 - 1.998) * 0.999 - 1.0
    } else if c <= 0.5 {
        2.0 * c * c
    } else {
        (4.0 - 2.0 * c) * c - 1.0
    };
    (1.0 / f - 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::FormulaId;

    #[test]
    fn the_flex_knob_is_linear_at_half_and_bows_either_side() {
        let shape = |c| FormulaId::FlexShape.eval(c);
        assert!(shape(0.5).abs() < 1e-6);
        assert!(shape(0.0).abs() < 1e-6, "zero reads as linear");
        assert!(shape(0.25) > 0.0);
        assert!(shape(0.75) < 0.0);
        // f(0.25) = 0.125 and f(0.75) = 0.875: ratios 49 and 1/49.
        assert!((shape(0.25) - 49f32.ln()).abs() < 1e-5, "{}", shape(0.25));
        assert!((shape(0.75) + 49f32.ln()).abs() < 1e-5, "{}", shape(0.75));
        // The end branches: ±2·ln(1/(2·10⁻⁶)) ≈ ±26.24 (f32 rounding of 1/f − 1 near 1 shifts
        // the upper end slightly).
        assert!((shape(0.0005) - 26.24).abs() < 0.01, "{}", shape(0.0005));
        assert!((shape(0.9995) + 26.24).abs() < 0.2, "{}", shape(0.9995));
    }

    #[test]
    fn the_attack_knob_is_odd_and_bends_hardest_at_its_ends() {
        let shape = |c| FormulaId::AttackShape.eval(c);
        assert!((shape(-0.4) + shape(0.4)).abs() < 1e-6);
        assert!(shape(1.0) < shape(0.5) && shape(0.5) < 0.0);
        // b = √(5·10⁵) / 2·10⁴ at 0.5 and 1 / 2·10⁴ at 1: ln(b / (1 + b)).
        assert!((shape(0.5) + 3.377_05).abs() < 1e-4, "{}", shape(0.5));
        assert!((shape(1.0) + 9.903_54).abs() < 1e-4, "{}", shape(1.0));
    }
}
