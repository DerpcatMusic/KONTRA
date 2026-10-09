use crate::Frame;

/// v1 voice.rs::mix_avx2 columns: unchanged f32 polynomial and 24-bit phase.
/// Safe arrays use the existing SIMD dispatcher instead of architecture intrinsics.
#[inline(always)]
#[cfg(test)]
pub(crate) fn cubic_four(windows: [&[Frame]; 4], fractions: [f64; 4]) -> [Frame; 4] {
    let mut frames = [[0.; 2]; 4];
    cubic_columns(windows, fractions, |lane, value| frames[lane / 2][lane % 2] = value);
    frames
}

/// v1's fused Hermite + mix loop; v2 keeps its existing amplitude product order.
#[inline(always)]
pub(crate) fn mix_four(windows: [&[Frame]; 4], fractions: [f64; 4], levels: [f32; 4],
    gain: f32, gains: [f32; 2], output: &mut [Frame; 4]) -> Frame {
    let mut last = [0.; 2];
    cubic_columns(windows, fractions, |lane, value| {
        let (i, c) = (lane / 2, lane % 2);
        output[i][c] += value * gain * gains[c] * levels[i];
        if i == 3 { last[c] = value; }
    });
    last
}

#[inline(always)]
fn cubic_columns(windows: [&[Frame]; 4], fractions: [f64; 4], mut emit: impl FnMut(usize, f32)) {
    let taps: [[f32; 8]; 4] = std::array::from_fn(|tap| {
        std::array::from_fn(|lane| windows[lane / 2][tap + 1][lane % 2])
    });
    let [xm1, x0, x1, x2] = taps;
    for lane in 0..8 {
        let t = super::phase(fractions[lane / 2]);
        let c1 = 0.5 * (x1[lane] - xm1[lane]);
        let c2 = xm1[lane] - 2.5 * x0[lane] + 2.0 * x1[lane] - 0.5 * x2[lane];
        let c3 = 0.5 * (x2[lane] - xm1[lane]) + 1.5 * (x0[lane] - x1[lane]);
        emit(lane, ((c3 * t + c2) * t + c1) * t + x0[lane]);
    }
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
