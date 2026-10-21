//! The 2x oversampling wrapper around a feedback loop: linear upsampling of the input and a
//! two-tap average on the way back down. Cores that saturate inside the loop (Daft) run it at
//! twice the block rate; the nonlinear curve itself stays in the core or in [`super::shaper`].

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
    fn half_rate_pairs_average_back_to_the_frame() {
        assert_eq!(upsample2(0., 1.), [0.5, 1.]);
        assert_eq!(decimate2(0.5, 1.), 0.75);
    }
}
