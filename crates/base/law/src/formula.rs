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
    /// the level ratio `(1/f − 1)²`, `f` the knob's quadratic ease-in/out clamped to
    /// 0.001..=0.999 (Kontakt 0x140ae5a10).
    FlexShape,
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
        }
    }

    /// The input this closed form maps to `y`, where an inverse is known.
    pub(crate) fn inverse(self, y: f64) -> Option<f64> {
        match self {
            Self::AttackShape => Some(attack_curve(y)),
            Self::FlexShape => None,
        }
    }
}

fn attack_shape(curve: f64) -> f32 {
    let c = curve.clamp(-1.0, 1.0);
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
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
    let c = (1.0 - (b.ln() + 20_000f64.ln()) / 500_000f64.ln()).clamp(0.0, 1.0);
    -shape.signum() * c
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
    }

    #[test]
    fn the_attack_knob_is_odd_and_bends_hardest_at_its_ends() {
        let shape = |c| FormulaId::AttackShape.eval(c);
        assert!((shape(-0.4) + shape(0.4)).abs() < 1e-6);
        assert!(shape(1.0) < shape(0.5) && shape(0.5) < 0.0);
    }
}
