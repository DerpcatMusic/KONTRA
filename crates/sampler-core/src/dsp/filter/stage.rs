//! Composable filter stages: the nonlinear transfer functions a core places inside its loop, and
//! the 2x oversampling that wraps a loop. A linear section comes from `cascade` or `svf`; a core
//! names its stage and its placement, so a product's saturator is a setting, not a new core.
//!
//! Placement is the core's choice and part of its law: `daft` applies [`soft`] to its band state
//! before the feedback, `ladder` applies its own per-stage curve. Other cores should reuse these
//! helpers before adding a copy.

/// Odd transfer with unity slope at the origin: `x (27 + x^2) / (27 + 9 x^2)` inside |x| < 3, and
/// the sign of `x` beyond it. A stand-in for Kontakt's Daft saturator, whose curve is not yet
/// recovered (docs/re/engine/filt-ar-daft.md, gap "Saturation transfer").
#[inline(always)]
pub(crate) fn soft(x: f64) -> f64 {
    if x.abs() >= 3. {
        x.signum()
    } else {
        x * (27. + x * x) / (27. + 9. * x * x)
    }
}

/// Upsample by two with linear interpolation: the midpoint from the previous frame, then the frame.
#[inline(always)]
pub(crate) fn upsample2(previous: f64, x: f64) -> [f64; 2] {
    [0.5 * (previous + x), x]
}

/// Downsample by two with the two-tap average of the processed half-rate frames.
#[inline(always)]
pub(crate) fn decimate2(first: f64, second: f64) -> f64 {
    0.5 * (first + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_is_odd_unity_at_origin_and_bounded() {
        assert_eq!(soft(0.), 0.);
        assert!((soft(1e-6) / 1e-6 - 1.).abs() < 1e-9);
        assert_eq!(soft(-5.), -1.);
        assert_eq!(soft(3.), 1.);
        assert!((soft(-2.) + soft(2.)).abs() < 1e-15);
    }
}
