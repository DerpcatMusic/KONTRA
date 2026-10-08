//! Control-prepared, table-interpolated low-pass kernel for rate conversion.
use std::{f64::consts::PI, sync::OnceLock};

pub(super) const MIN_STEP: f64 = 1.0 / 256.0;
pub(super) const MAX_STEP: f64 = 16.0;
/// Octave levels that cover steps up to MAX_STEP.
pub(super) const OCTAVES: usize = 4;
const RADIUS: usize = 48;
const SHORT_RADIUS: usize = 12;

/// Source frames any quality reads on each side of a voice's position at
/// `step`: what a resident range around a start needs beyond its frames.
pub fn read_radius(step: f64) -> usize {
    Kernel::radius(step) as usize
}
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

/// Support of one decimation step, in frames of the finer level.
pub(super) const DECIMATION_REACH: i64 = 2 * RADIUS as i64;

/// Octave levels 1..=log2(MAX_STEP), each the previous low-passed by the long
/// kernel (0.9 of the new Nyquist) and decimated by two, sample j centered on
/// frame 2j of the finer level. Control-side only.
pub(super) fn octaves(frames: &[[f32; 2]], depth: usize) -> Box<[Box<[[f32; 2]]>]> {
    let table = Kernel::new(ResampleQuality::High).long;
    let mut levels: Vec<Box<[[f32; 2]]>> = Vec::new();
    for _ in 0..depth.min(OCTAVES) {
        let finer = levels.last().map_or(frames, |level| level);
        if finer.len() < 2 {
            break;
        }
        let coarser = (0..finer.len().div_ceil(2) as i64)
            .map(|j| {
                table.sample(0.0, 2.0, |offset| {
                    usize::try_from(2 * j + offset)
                        .ok()
                        .and_then(|i| finer.get(i).copied())
                        .unwrap_or([0.0; 2])
                })
            })
            .collect();
        levels.push(coarser);
    }
    levels.into_boxed_slice()
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

/// Stretches of the polyphase bank: eight per octave above unity.
const STRETCHES: usize = 8;
/// Phases per input frame; coefficients interpolate linearly between rows.
const PHASES: usize = 64;

/// The short sinc pre-evaluated for one stretch: `PHASES + 1` rows of
/// `2 * radius + 1` f32 taps, row p for fraction p / PHASES, each row
/// normalized to unit sum (so any interpolated row is too).
pub(super) struct Polyphase {
    stretch: f64,
    radius: usize,
    // Row length in f32: each tap twice (left, right), rounded up to a chunk.
    stride: usize,
    rows: Box<[f32]>,
}

/// f32 lanes per accumulation chunk: four stereo frames.
const CHUNK: usize = 8;

impl Polyphase {
    fn new(table: &Table, stretch: f64) -> Self {
        let radius = table.radius(stretch) as usize;
        let taps = 2 * radius + 1;
        let stride = (2 * taps).div_ceil(CHUNK) * CHUNK;
        let mut rows = vec![0.0_f32; (PHASES + 1) * stride];
        for (phase, row) in rows.chunks_exact_mut(stride).enumerate() {
            let fraction = phase as f64 / PHASES as f64;
            let mut coefficients = [0.0_f64; MAX_TAPS];
            // Evaluate the reference impulse row once, rather than once per tap.
            let inverse = 1.0 / stretch.max(1.0);
            let increment = (inverse * RESOLUTION as f64 * 4294967296.0).round() as i64;
            let mut distance = ((-(radius as f64) - fraction)
                * inverse
                * RESOLUTION as f64
                * 4294967296.0)
                .round() as i64;
            let mut weight = 0.;
            for coefficient in &mut coefficients[..taps] {
                *coefficient = table.coefficient(distance.unsigned_abs());
                weight += *coefficient;
                distance += increment;
            }
            for coefficient in &mut coefficients[..taps] {
                *coefficient = f64::from(((0. + *coefficient) / weight) as f32);
            }
            let sum: f64 = coefficients[..taps].iter().sum();
            for (pair, c) in row
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(&coefficients[..taps])
            {
                pair.fill((c / sum) as f32);
            }
        }
        Self {
            stretch,
            radius,
            stride,
            rows: rows.into_boxed_slice(),
        }
    }

    /// Interpolate at `fraction` from `window`, the 2 * radius + 1 frames
    /// centered on tap 0, in traversal order.
    #[inline]
    /// Frames a padded window spans: taps rounded up to whole chunks.
    pub(super) fn width(&self) -> usize {
        self.stride / 2
    }

    /// Interpolate at `fraction` from `window`, frames from tap -radius in
    /// traversal order: `width()` frames read directly (frames past the taps
    /// meet zero coefficients and must be finite), shorter ones are padded.
    #[inline]
    pub(super) fn sample(&self, fraction: f64, window: &[[f32; 2]]) -> [f32; 2] {
        // The same arithmetic as the vector runs use on this CPU, so a frame
        // does not depend on whether it was rendered alone or in a run.
        sampler_simd::dispatch_fused(
            (self, fraction, window),
            #[inline(always)]
            |(this, f, w)| this.sample_with::<false>(f, w),
            #[inline(always)]
            |(this, f, w)| this.sample_with::<true>(f, w),
        )
    }

    #[inline(always)]
    fn sample_with<const FUSED: bool>(&self, fraction: f64, window: &[[f32; 2]]) -> [f32; 2] {
        if window.len() < self.width() {
            let mut padded = [[0.0; 2]; MAX_WIDTH];
            let taps = 2 * self.radius + 1;
            padded[..taps].copy_from_slice(&window[..taps]);
            return self.dot::<FUSED>(fraction, &padded[..self.width()]);
        }
        self.dot::<FUSED>(fraction, window)
    }

    /// [`Self::sample`] for a window of at least `width()` frames; `FUSED`
    /// uses fused multiply-adds, which only a wide-target caller may.
    #[inline(always)]
    pub(super) fn dot<const FUSED: bool>(&self, fraction: f64, window: &[[f32; 2]]) -> [f32; 2] {
        let position = fraction * PHASES as f64;
        // `position` is in [0, PHASES]: the u32 conversion is exact and
        // cheaper than a saturating usize one.
        let phase = (position as u32 as usize).min(PHASES - 1);
        let t = (position - phase as f64) as f32;
        let stride = self.stride;
        let (a, b) = self.rows[phase * stride..][..2 * stride].split_at(stride);
        let window = &window.as_flattened()[..stride];
        // Independent lanes, interleaved left/right: no serial f32 reduction
        // and no deinterleave, so the loop vectorizes as written.
        let mut sum = [[0.0_f32; 4]; 2];
        let (a, b, x) = (
            a.as_chunks::<CHUNK>().0,
            b.as_chunks::<CHUNK>().0,
            window.as_chunks::<CHUNK>().0,
        );
        for ((a, b), x) in a.iter().zip(b).zip(x) {
            accumulate::<FUSED>(&mut sum, t, a, b, x);
        }
        // Fold in register order (lanes k and k + 4), then the stereo pairs.
        // The lanes leave through memory: otherwise SLP vectorization permutes
        // them to suit this stereo fold and turns every load into scalar loads.
        let sum = std::hint::black_box(sum);
        let half: [f32; 4] = std::array::from_fn(|k| sum[0][k] + sum[1][k]);
        [half[0] + half[2], half[1] + half[3]]
    }
}

#[inline(always)]
fn accumulate<const FUSED: bool>(
    sum: &mut [[f32; 4]; 2],
    t: f32,
    a: &[f32; CHUNK],
    b: &[f32; CHUNK],
    x: &[f32; CHUNK],
) {
    for (half, sum) in sum.iter_mut().enumerate() {
        let (a, b, x) = (&a[4 * half..], &b[4 * half..], &x[4 * half..]);
        for k in 0..4 {
            if FUSED {
                // One rounding per fused step, on the wide path only.
                sum[k] = t.mul_add(b[k] - a[k], a[k]).mul_add(x[k], sum[k]);
            } else {
                sum[k] += (a[k] + t * (b[k] - a[k])) * x[k];
            }
        }
    }
}

/// Widest polyphase window: radius 12 at stretch 2.
const MAX_TAPS: usize = 2 * 2 * SHORT_RADIUS + 1;
/// Its padded width in frames.
const MAX_WIDTH: usize = (2 * MAX_TAPS).div_ceil(CHUNK) * CHUNK / 2;

/// The runtime's rate converter: immutable tables shared by every runtime,
/// prepared by Runtime construction and never lazily initialized by rendering.
#[derive(Clone, Copy)]
pub(super) struct Kernel {
    quality: ResampleQuality,
    long: &'static Table,
    short: &'static Table,
    bank: &'static [Polyphase; STRETCHES],
}

impl Kernel {
    pub(super) fn new(quality: ResampleQuality) -> Self {
        static LONG: OnceLock<Table> = OnceLock::new();
        static SHORT: OnceLock<Table> = OnceLock::new();
        static BANK: OnceLock<[Polyphase; STRETCHES]> = OnceLock::new();
        let short = SHORT.get_or_init(|| Table::new(SHORT_RADIUS, 0.8));
        Self {
            quality,
            long: LONG.get_or_init(|| Table::new(RADIUS, 0.9)),
            short,
            bank: BANK
                .get_or_init(|| std::array::from_fn(|i| Polyphase::new(short, Self::stretch(i)))),
        }
    }

    fn stretch(index: usize) -> f64 {
        ((index + 1) as f64 / STRETCHES as f64).exp2()
    }

    /// The bank entry for 1 < step <= 2: the narrowest stretch at or above
    /// step, so the band edge sits at most an eighth of an octave low.
    pub(super) fn polyphase(&self, step: f64) -> Option<&Polyphase> {
        if self.quality != ResampleQuality::Realtime || step <= 1.0 || step > 2.0 {
            return None;
        }
        self.bank.iter().find(|entry| entry.stretch >= step)
    }

    /// The widest window any quality reads at `step`: demand prediction uses it.
    pub(super) fn radius(step: f64) -> i64 {
        (RADIUS as f64 * step.max(1.0)).ceil() as i64
    }

    /// Frames read on each side of the current position at `step`.
    pub(super) fn window(&self, step: f64) -> i64 {
        if let Some(bank) = self.polyphase(step) {
            return bank.radius as i64;
        }
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
        mut read: impl FnMut(i64) -> [f32; 2],
    ) -> [f32; 2] {
        if let Some(bank) = self.polyphase(step) {
            let radius = bank.radius as i64;
            let mut window = [[0.0; 2]; MAX_WIDTH];
            for (frame, offset) in window.iter_mut().zip(-radius..=radius) {
                *frame = read(offset);
            }
            return bank.sample(fraction, &window);
        }
        match self.quality {
            ResampleQuality::High => self.long.sample(fraction, step, read),
            ResampleQuality::Realtime if step <= 1.0 => cubic(fraction, read),
            ResampleQuality::Realtime => self.short.sample(fraction, step, read),
        }
    }

    /// `sample` over a contiguous forward window of 2 * window(step) + 1 frames.
    #[inline]
    pub(super) fn sample_window(&self, fraction: f64, step: f64, window: &[[f32; 2]]) -> [f32; 2] {
        if let Some(bank) = self.polyphase(step) {
            return bank.sample(fraction, window);
        }
        let radius = self.window(step);
        self.sample(fraction, step, |offset| window[(offset + radius) as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_rows_match_reference_impulses_without_changing_narrow_bank_rounding() {
        let kernel = Kernel::new(ResampleQuality::Realtime);
        for bank in kernel.bank {
            let taps = 2 * bank.radius + 1;
            for phase in 0..=PHASES {
                let row = &bank.rows[phase * bank.stride..][..bank.stride];
                let mut expected = vec![0f64; taps];
                for (k, coefficient) in expected.iter_mut().enumerate() {
                    let offset = k as i64 - bank.radius as i64;
                    *coefficient = f64::from(
                        kernel
                            .short
                            .sample(phase as f64 / PHASES as f64, bank.stretch, |at| {
                                if at == offset { [1.; 2] } else { [0.; 2] }
                            })[0],
                    );
                }
                let sum: f64 = expected.iter().sum();
                for (k, c) in expected.iter().enumerate() {
                    let bits = (c / sum) as f32;
                    assert_eq!(row[2 * k].to_bits(), bits.to_bits());
                    assert_eq!(row[2 * k + 1].to_bits(), bits.to_bits());
                }
                assert!(row[2 * taps..].iter().all(|&x| x == 0.));
            }
        }
    }

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
        for step in [
            MIN_STEP,
            0.5,
            44100.0 / 48000.0,
            1.0,
            1.5,
            2.0,
            8.0,
            MAX_STEP,
        ] {
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
                assert!(
                    (tone(pass) - 1.0).abs() < tolerance,
                    "step={step} phase={fraction}"
                );
                if step > 1.0 {
                    // At least 50 dB below the passband at the output Nyquist.
                    assert!(tone(0.5 / scale) < 0.0032, "step={step} phase={fraction}");
                }
            }
        }
    }
}
