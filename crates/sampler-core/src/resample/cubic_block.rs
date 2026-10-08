use crate::Frame;

/// v1 voice.rs::mix_avx2 columns, adapted to v2's full f64 phase and coefficients.
#[inline(always)]
pub(crate) fn cubic_four(windows: [&[Frame]; 4], fractions: [f64; 4]) -> [Frame; 4] {
    let taps: [[f64; 8]; 4] = std::array::from_fn(|tap| {
        std::array::from_fn(|lane| f64::from(windows[lane / 2][tap + 1][lane % 2]))
    });
    let [xm1, x0, x1, x2] = taps;
    let mut result = [0.0; 8];
    for lane in 0..8 {
        let t = fractions[lane / 2];
        let c1 = 0.5 * (x1[lane] - xm1[lane]);
        let c2 = xm1[lane] - 2.5 * x0[lane] + 2.0 * x1[lane] - 0.5 * x2[lane];
        let c3 = 0.5 * (x2[lane] - xm1[lane]) + 1.5 * (x0[lane] - x1[lane]);
        result[lane] = ((c3 * t + c2) * t + c1) * t + x0[lane];
    }
    let result = std::hint::black_box(result);
    std::array::from_fn(|i| [result[2 * i] as f32, result[2 * i + 1] as f32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_match_scalar_cubic_bit_for_bit() {
        let waves: [[Frame; 5]; 4] = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                let x = ((i * 5 + j) as f32 * 1.2345).sin();
                [x, x * -0.31415]
            })
        });
        for windows in [
            waves,
            [[[-0.0, 0.0]; 5]; 4],
            std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    let sign = if (i + j) % 2 == 0 { 1.0 } else { -1.0 };
                    [sign * f32::MAX, sign * f32::MIN_POSITIVE]
                })
            }),
        ] {
            for phases in [
                [0.0; 4],
                [0.125, 0.5, 0.99, 1.0 - f64::EPSILON],
                [0.123456789; 4],
            ] {
                let actual = sampler_simd::dispatch(
                    #[inline(always)]
                    || cubic_four(windows.each_ref().map(|w| w.as_slice()), phases),
                );
                for i in 0..4 {
                    let expected =
                        super::super::cubic(phases[i], |offset| windows[i][(offset + 2) as usize]);
                    assert_eq!(actual[i].map(f32::to_bits), expected.map(f32::to_bits));
                }
            }
        }
    }
}
