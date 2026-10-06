//! Control-prepared, table-interpolated low-pass kernel for resident rate conversion.
use std::{f64::consts::PI, sync::OnceLock};

pub(super) const MIN_STEP: f64 = 1.0 / 256.0;
pub(super) const MAX_STEP: f64 = 16.0;
const RADIUS: usize = 48;
const RESOLUTION: usize = 1024;

pub(super) struct Kernel(Box<[f32]>);

impl Kernel {
    /// Called by Runtime construction, never lazily initialized by rendering.
    pub(super) fn shared() -> &'static Self {
        static KERNEL: OnceLock<Kernel> = OnceLock::new();
        KERNEL.get_or_init(|| {
            Self(
                (0..=RADIUS * RESOLUTION)
                    .map(|i| {
                        let x = i as f64 / RESOLUTION as f64;
                        let sinc = if i == 0 {
                            0.9
                        } else {
                            (0.9 * PI * x).sin() / (PI * x)
                        };
                        let window = 0.42
                            + 0.5 * (PI * x / RADIUS as f64).cos()
                            + 0.08 * (2.0 * PI * x / RADIUS as f64).cos();
                        (sinc * window) as f32
                    })
                    .collect(),
            )
        })
    }

    fn coefficient(&self, distance: u64) -> f64 {
        let whole = (distance >> 32) as usize;
        if whole >= self.0.len() - 1 {
            return 0.0;
        }
        let a = f64::from(self.0[whole]);
        a + (f64::from(self.0[whole + 1]) - a) * (f64::from(distance as u32) / 4294967296.0)
    }

    pub(super) fn radius(step: f64) -> i64 {
        (RADIUS as f64 * step.max(1.0)).ceil() as i64
    }

    pub(super) fn sample(
        &self,
        fraction: f64,
        step: f64,
        mut read: impl FnMut(i64) -> [f32; 2],
    ) -> [f32; 2] {
        let inverse_scale = 1.0 / step.max(1.0);
        let radius = Self::radius(step);
        // Fixed-point table traversal avoids a float-to-index conversion per tap.
        // Q32 is in table intervals, not PCM frames; rounding error over the widest
        // window is below one millionth of a table interval.
        let increment = (inverse_scale * RESOLUTION as f64 * 4294967296.0).round() as i64;
        let mut distance = ((-radius as f64 - fraction)
            * inverse_scale
            * RESOLUTION as f64
            * 4294967296.0)
            .round() as i64;
        let mut sum = [0.0; 2];
        let mut weight = 0.0;
        // At most 1,537 source reads. No traversal loop depends on a loop's length.
        for offset in -radius..=radius {
            let coefficient = self.coefficient(distance.unsigned_abs());
            distance += increment;
            let frame = read(offset);
            weight += coefficient;
            for channel in 0..2 {
                sum[channel] += f64::from(frame[channel]) * coefficient;
            }
        }
        // Normalize the finite fractional-phase kernel, including out-of-view zero
        // padding in the denominator. Boundary transients are not gain-compensated.
        [(sum[0] / weight) as f32, (sum[1] / weight) as f32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_kernel_preserves_dc_passband_and_rejects_alias_band() {
        let kernel = Kernel::shared();
        for step in [MIN_STEP, 0.5, 1.0, 48000.0 / 44100.0, 2.0, 8.0, MAX_STEP] {
            for fraction in [0.0, 0.125, 0.5, 0.999] {
                assert_eq!(kernel.sample(fraction, step, |_| [1.0, -1.0]), [1.0, -1.0]);
                for (frequency, expected) in [
                    (0.1 / step.max(1.0), true),
                    (0.39 / step.max(1.0), true),
                    (0.5 / step.max(1.0), false),
                ] {
                    let output = kernel.sample(fraction, step, |i| {
                        let angle = 2.0 * PI * frequency * i as f64;
                        [angle.cos() as f32, angle.sin() as f32]
                    });
                    let amplitude = f64::from(output[0]).hypot(f64::from(output[1]));
                    if expected {
                        assert!(
                            (amplitude - 1.0).abs() < 0.001,
                            "passband step={step} phase={fraction}: {amplitude}"
                        );
                        let angle = 2.0 * PI * frequency * fraction;
                        assert!((f64::from(output[0]) - angle.cos()).abs() < 0.001);
                        assert!((f64::from(output[1]) - angle.sin()).abs() < 0.001);
                    } else {
                        assert!(
                            amplitude < 0.0002,
                            "stopband step={step} phase={fraction}: {amplitude}"
                        );
                    }
                }
            }
        }
    }
}
