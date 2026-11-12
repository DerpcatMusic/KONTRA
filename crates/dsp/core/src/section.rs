//! An analog [`Section`] at a rate: the bilinear transform prewarped at its
//! corner, as an f64 [`Biquad`] or a lean f32 [`Biquad32`] for oversampled
//! stages well above the bass, where f32 state is clean.

use kontra_law::Section;

use crate::Biquad;

/// `b0, b1, b2, a0, a1, a2` of `s` at `rate`; the corner is held within
/// 1 Hz..=0.49 of the rate. A first-order section stays first order (no
/// pole at Nyquist cancelled by a zero there).
#[must_use]
pub fn bilinear(s: Section, rate: u32) -> [f64; 6] {
    let r = f64::from(rate.max(1));
    let c = 1.0 / (std::f64::consts::PI * f64::from(s.hz).clamp(1.0, 0.49 * r) / r).tan();
    let [n0, n1, n2] = s.num.map(f64::from);
    let [d0, d1, d2] = s.den.map(f64::from);
    if n2 == 0.0 && d2 == 0.0 {
        return [n0 + n1 * c, n0 - n1 * c, 0.0, d0 + d1 * c, d0 - d1 * c, 0.0];
    }
    let c2 = c * c;
    let z = |a: f64, b: f64, e: f64| [a + b * c + e * c2, 2.0 * (a - e * c2), a - b * c + e * c2];
    let ([b0, b1, b2], [a0, a1, a2]) = (z(n0, n1, n2), z(d0, d1, d2));
    [b0, b1, b2, a0, a1, a2]
}

impl Biquad {
    /// `s` at `rate`.
    #[must_use]
    pub fn analog(s: Section, rate: u32) -> Self {
        Self::new(bilinear(s, rate))
    }
}

/// One stereo biquad section, transposed direct form II, f32: for corners
/// well above the bass at an oversampled rate, one frame at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Biquad32 {
    /// b0, b1, b2, a1, a2 with a0 = 1.
    c: [f32; 5],
    state: [[f32; 2]; 2],
}

impl Biquad32 {
    /// `s` at `rate`.
    #[must_use]
    pub fn analog(s: Section, rate: u32) -> Self {
        let [b0, b1, b2, a0, a1, a2] = bilinear(s, rate);
        Self {
            c: [b0, b1, b2, a1, a2].map(|v| (v / a0) as f32),
            state: [[0.0; 2]; 2],
        }
    }

    /// One stereo frame.
    #[must_use]
    pub fn tick(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let [b0, b1, b2, a1, a2] = self.c;
        let mut out = frame;
        for (x, s) in out.iter_mut().zip(&mut self.state) {
            let input = *x;
            let y = b0 * input + s[0];
            s[0] = b1 * input - a1 * y + s[1];
            s[1] = b2 * input - a2 * y;
            *x = y;
        }
        out
    }

    /// Takes `from`'s coefficients and keeps this section's state.
    pub fn retune(&mut self, from: &Self) {
        self.c = from.c;
    }

    /// Zeroes state that has decayed into the denormal range.
    pub fn settle(&mut self) {
        for v in self.state.as_flattened_mut() {
            if v.abs() < 1e-20 {
                *v = 0.0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 2]; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain(c: [f64; 6], hz: f64, rate: f64) -> f64 {
        use std::f64::consts::TAU;
        let z = |k: f64| (-(TAU * hz / rate) * k).sin_cos();
        let eval = |a: f64, b: f64, e: f64| {
            let ((s1, c1), (s2, c2)) = (z(1.0), z(2.0));
            let (re, im) = (a + b * c1 + e * c2, b * s1 + e * s2);
            (re * re + im * im).sqrt()
        };
        eval(c[0], c[1], c[2]) / eval(c[3], c[4], c[5])
    }

    /// A Butterworth low-pass sits at −3 dB on its corner, at any rate.
    #[test]
    fn a_prewarped_corner_lands_on_its_frequency() {
        let lp = Section {
            hz: 15_000.0,
            num: [1.0, 0.0, 0.0],
            den: [1.0, std::f32::consts::SQRT_2, 1.0],
        };
        for rate in [44_100u32, 96_000] {
            let db = 20.0 * gain(bilinear(lp, rate), 15_000.0, f64::from(rate)).log10();
            assert!((db + 3.0103).abs() < 1e-3, "{rate}: {db}");
        }
        let one = Section {
            hz: 80.0,
            num: [2.0, 1.0, 0.0],
            den: [1.0, 1.0, 0.0],
        };
        let c = bilinear(one, 48_000);
        assert_eq!(c[2], 0.0);
        assert!((gain(c, 0.5, 48_000.0) - 2.0).abs() < 1e-4);
    }
}
