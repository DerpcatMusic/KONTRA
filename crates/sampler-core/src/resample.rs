//! Control-prepared, table-interpolated low-pass kernel for resident rate conversion.
use std::{f64::consts::PI, sync::OnceLock};

pub(super) const MIN_STEP: f64 = 1.0 / 256.0;
pub(super) const MAX_STEP: f64 = 16.0;
const RADIUS: usize = 48;
const SHORT_RADIUS: usize = 12;
const RESOLUTION: usize = 1024;

/// Rate-conversion quality. Realtime interpolates upsampled and unity-rate
/// voices with a four-point cubic and downsampled (pitched-up) voices with a
/// short windowed sinc; High uses the long sinc for every non-unity ratio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResampleQuality {
    #[default]
    Realtime,
    High,
}

/// A tabulated Blackman-windowed sinc low-pass, `radius` zero crossings each side.
pub(super) struct Table {
    radius: usize,
    taps: Box<[f32]>,
}

impl Table {
    fn new(radius: usize, cutoff: f64) -> Self {
        Self {
            radius,
            taps: (0..=radius * RESOLUTION)
                .map(|i| {
                    let x = i as f64 / RESOLUTION as f64;
                    let sinc = if i == 0 {
                        cutoff
                    } else {
                        (cutoff * PI * x).sin() / (PI * x)
                    };
                    let window = 0.42
                        + 0.5 * (PI * x / radius as f64).cos()
                        + 0.08 * (2.0 * PI * x / radius as f64).cos();
                    (sinc * window) as f32
                })
                .collect(),
        }
    }

    fn coefficient(&self, distance: u64) -> f64 {
        let whole = (distance >> 32) as usize;
        if whole >= self.taps.len() - 1 {
            return 0.0;
        }
        let a = f64::from(self.taps[whole]);
        a + (f64::from(self.taps[whole + 1]) - a) * (f64::from(distance as u32) / 4294967296.0)
    }

    fn radius(&self, step: f64) -> i64 {
        (self.radius as f64 * step.max(1.0)).ceil() as i64
    }

    fn sample(&self, fraction: f64, step: f64, mut read: impl FnMut(i64) -> [f32; 2]) -> [f32; 2] {
        let inverse_scale = 1.0 / step.max(1.0);
        let radius = self.radius(step);
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

/// Four-point Catmull-Rom cubic between taps 0 and 1 at `fraction`.
fn cubic(fraction: f64, mut read: impl FnMut(i64) -> [f32; 2]) -> [f32; 2] {
    let taps = [read(-1), read(0), read(1), read(2)];
    let t = fraction;
    std::array::from_fn(|c| {
        let [xm1, x0, x1, x2] = taps.map(|frame| f64::from(frame[c]));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        (((c3 * t + c2) * t + c1) * t + x0) as f32
    })
}

/// The runtime's rate converter: immutable tables shared by every runtime,
/// prepared by Runtime construction and never lazily initialized by rendering.
#[derive(Clone, Copy)]
pub(super) struct Kernel {
    quality: ResampleQuality,
    long: &'static Table,
    short: &'static Table,
}

impl Kernel {
    pub(super) fn new(quality: ResampleQuality) -> Self {
        static LONG: OnceLock<Table> = OnceLock::new();
        static SHORT: OnceLock<Table> = OnceLock::new();
        Self {
            quality,
            long: LONG.get_or_init(|| Table::new(RADIUS, 0.9)),
            short: SHORT.get_or_init(|| Table::new(SHORT_RADIUS, 0.8)),
        }
    }

    /// The widest window any quality reads at `step`: demand prediction uses it.
    pub(super) fn radius(step: f64) -> i64 {
        (RADIUS as f64 * step.max(1.0)).ceil() as i64
    }

    /// Frames read on each side of the current position at `step`.
    pub(super) fn window(&self, step: f64) -> i64 {
        match self.quality {
            ResampleQuality::High => self.long.radius(step),
            ResampleQuality::Realtime if step <= 1.0 => 2,
            ResampleQuality::Realtime => self.short.radius(step),
        }
    }

    /// Interpolate at `fraction` past tap 0; `read` takes offsets within `window`.
    pub(super) fn sample(
        &self,
        fraction: f64,
        step: f64,
        read: impl FnMut(i64) -> [f32; 2],
    ) -> [f32; 2] {
        match self.quality {
            ResampleQuality::High => self.long.sample(fraction, step, read),
            ResampleQuality::Realtime if step <= 1.0 => cubic(fraction, read),
            ResampleQuality::Realtime => self.short.sample(fraction, step, read),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_kernel_preserves_dc_passband_and_rejects_alias_band() {
        let kernel = Kernel::new(ResampleQuality::High);
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

    #[test]
    fn realtime_ladder_keeps_dc_passband_and_bounds_alias_band() {
        let kernel = Kernel::new(ResampleQuality::Realtime);
        for step in [MIN_STEP, 0.5, 44100.0 / 48000.0, 1.0, 1.5, 2.0, 8.0, MAX_STEP] {
            for fraction in [0.0, 0.125, 0.5, 0.999] {
                let dc = kernel.sample(fraction, step, |_| [1.0, -1.0]);
                assert!((dc[0] - 1.0).abs() < 1e-6 && (dc[1] + 1.0).abs() < 1e-6);
                let tone = |frequency: f64| {
                    let output = kernel.sample(fraction, step, |i| {
                        let angle = 2.0 * PI * frequency * i as f64;
                        [angle.cos() as f32, angle.sin() as f32]
                    });
                    f64::from(output[0]).hypot(f64::from(output[1]))
                };
                let scale = step.max(1.0);
                // Cubic loses under 0.2 dB at a tenth of the source rate; the
                // short sinc keeps 0.3 of the output band within 0.1 dB.
                let pass = if step <= 1.0 { 0.1 } else { 0.3 / scale };
                let tolerance = if step <= 1.0 { 0.025 } else { 0.012 };
                assert!((tone(pass) - 1.0).abs() < tolerance, "step={step} phase={fraction}");
                if step > 1.0 {
                    // At least 50 dB below the passband at the output Nyquist.
                    assert!(tone(0.5 / scale) < 0.0032, "step={step} phase={fraction}");
                }
            }
        }
    }
}
