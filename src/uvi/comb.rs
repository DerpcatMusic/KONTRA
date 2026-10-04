//! Original feedback-comb kernel, independently compared with authored native
//! UVI Workstation 4.0.9 impulses. Parameters: https://lua.uvi.net/_elements.html
//! Plus/Minus topology: official Falcon manual, effect appendix, Comb Filter.
//!
//! This kernel is deliberately not admitted by playback preflight: native
//! connected-source binding and broader matrix/control generation remain unverified.
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{Result, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "CombFilter audio, physical control-point dispatch and bounded ConstantModulation held-Value adapter are native-verified at 32/44.1/48/96 kHz; graph/property lifecycle and broader matrix/control generation remain unverified";

/// The measured audio kernel and physical control-point dispatcher. This does not implement the
/// surrounding Falcon control-variable lifecycle.
pub struct CombKernel {
    channels: usize,
    rate: f32,
    mode: u32,
    ring: Vec<f32>,
    length: usize,
    cursor: usize,
    delay: usize,
    fraction: f32,
    feedback: f32,
}
impl CombKernel {
    pub fn new(channels: usize, rate: f64, freq: f64, q: f64, mode: u32) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "Invalid CombFilter channel count"
        );
        ensure!(
            [32_000., 44_100., 48_000., 96_000.].contains(&rate),
            "Unmeasured CombFilter sample rate"
        );
        ensure!(
            freq.is_finite() && (20. ..=20_000.).contains(&freq),
            "Invalid CombFilter frequency"
        );
        ensure!(
            q.is_finite() && (0. ..=1.).contains(&q),
            "Invalid CombFilter resonance"
        );
        ensure!(mode <= 1, "Invalid CombFilter mode");
        let frequency = (freq as f32).min(rate as f32 * 0.5);
        // Period is 1/f seconds. Native scalar measurements establish the
        // half-period reciprocal's float rounding before doubling.
        let period = (1. / ((frequency + frequency) / rate as f32)) * 2.;
        let delay = period as usize;
        // Native history capacity supports later edits down to 20 Hz.
        let length = ((rate * 0.05).ceil() as usize + 513).next_power_of_two();
        Ok(Self {
            channels,
            rate: rate as f32,
            mode,
            ring: vec![0.; channels * length],
            length,
            cursor: 0,
            delay,
            fraction: period - delay as f32,
            feedback: q as f32 * if mode == 0 { 1. } else { -1. },
        })
    }
    pub fn process(&mut self, frames: &mut [Frame]) {
        let mask = self.length - 1;
        for frame in frames {
            let newer = self.cursor.wrapping_sub(self.delay) & mask;
            let older = newer.wrapping_sub(1) & mask;
            for (channel, x) in frame[..self.channels].iter_mut().enumerate() {
                let buffer = &mut self.ring[channel * self.length..(channel + 1) * self.length];
                let delayed = buffer[newer] + (buffer[older] - buffer[newer]) * self.fraction;
                *x += delayed * self.feedback;
                buffer[self.cursor] = *x;
            }
            self.cursor = (self.cursor + 1) & mask;
        }
    }
    /// Apply measured physical Freq/Q points held for each 32-frame span.
    /// This consumes the surrounding controller's output; it does not generate
    /// matrix sources or smoothing. Bypass updates controls while freezing audio.
    pub fn process_control_points(
        &mut self,
        frames: &mut [Frame],
        freq: &[f32],
        q: &[f32],
        bypass: bool,
    ) -> Result<()> {
        let count = frames.len().div_ceil(32);
        ensure!(
            freq.len() == count && q.len() == count,
            "Invalid CombFilter control-point count"
        );
        ensure!(
            freq.iter()
                .all(|v| v.is_finite() && (20. ..=20_000.).contains(v)),
            "Invalid CombFilter Freq points"
        );
        ensure!(
            q.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "Invalid CombFilter Q points"
        );
        if bypass {
            if let Some(index) = count.checked_sub(1) {
                self.configure(freq[index], q[index]);
            }
            return Ok(());
        }
        for ((chunk, freq), q) in frames.chunks_mut(32).zip(freq).zip(q) {
            self.configure(*freq, *q);
            self.process(chunk);
        }
        Ok(())
    }
    fn configure(&mut self, freq: f32, q: f32) {
        let frequency = freq.min(self.rate * 0.5);
        let period = (1. / ((frequency + frequency) / self.rate)) * 2.;
        self.delay = period as usize;
        self.fraction = period - self.delay as f32;
        self.feedback = q * if self.mode == 0 { 1. } else { -1. };
    }
    pub fn clear(&mut self) {
        self.ring.fill(0.);
        self.cursor = 0;
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.ring.capacity() * std::mem::size_of::<f32>()
    }
}

/// Measured desired-state input for unipolar ConstantModulation -> edition-mode Freq.
/// This owns source and Freq RC stages. The upstream Value-property RC is excluded.
/// Calls are bounded to the measured 4096-frame host block; graph binding is gated.
/// Only Ratio=1, Offset=0, no inversion/mapper and the measured rates are covered.
pub struct ConstantFrequencyControl {
    alpha_sample: f32,
    alpha32: f32,
    source: f32,
    normalized_freq: f32,
    target_hz: f32,
    cold: bool,
}
impl ConstantFrequencyControl {
    pub fn new(rate: u32, source: f32, initial_hz: f32) -> Result<Self> {
        let (alpha_sample, alpha32) = control_coefficients(rate)?;
        ensure!(
            source.is_finite() && (0. ..=1.).contains(&source),
            "Invalid ConstantModulation Value"
        );
        ensure!(
            initial_hz.is_finite() && (20. ..=20_000.).contains(&initial_hz),
            "Invalid initial CombFilter Freq"
        );
        ensure!(
            initial_hz.to_bits() == Self::physical(source).to_bits(),
            "Unmeasured inconsistent ConstantModulation/CombFilter initialization"
        );
        Ok(Self {
            alpha_sample,
            alpha32,
            source,
            normalized_freq: Self::normalized(initial_hz),
            target_hz: initial_hz,
            cold: true,
        })
    }
    /// Convenience study API; realtime callers can supply reusable storage below.
    pub fn next_points(&mut self, value: f32, frames: usize) -> Result<Vec<f32>> {
        self.validate(value, frames)?;
        let mut points = vec![0.; frames.div_ceil(32)];
        self.next_points_into(value, frames, &mut points)?;
        Ok(points)
    }
    /// Fill caller-owned physical Freq points without allocating.
    pub fn next_points_into(
        &mut self,
        value: f32,
        frames: usize,
        points: &mut [f32],
    ) -> Result<()> {
        self.validate(value, frames)?;
        ensure!(
            points.len() == frames.div_ceil(32),
            "Invalid Comb control-point output length"
        );
        if frames == 0 {
            return Ok(());
        }
        if self.cold {
            self.source = value;
            self.cold = false;
        }
        if self.source != value {
            let lookahead = if ((value - self.source) * self.alpha32).abs() < 1e-7 {
                self.source = value;
                value
            } else {
                let (current, future) = self.ramp(self.source, value, frames, false, &mut []);
                self.source = current;
                future
            };
            self.target_hz = Self::physical(lookahead);
        }
        self.write_freq_points(frames, points);
        Ok(())
    }
    /// Consume held physical Value points from an upstream property manager.
    /// The upstream manager owns its RC state; this leaf owns source and Freq RC.
    /// Dynamic Value dispatch invokes the native source law at every 32-frame span.
    pub fn next_value_points_into(
        &mut self,
        values: &[f32],
        frames: usize,
        value_is_static: bool,
        points: &mut [f32],
    ) -> Result<()> {
        ensure!(
            frames <= 4096,
            "Unmeasured ConstantModulation control-block length"
        );
        ensure!(
            values.len() == frames.div_ceil(32) && points.len() == values.len(),
            "Invalid held Value/Freq point lengths"
        );
        if frames == 0 {
            return Ok(());
        }
        for &value in values {
            ensure!(
                value.is_finite() && (0. ..=1.).contains(&value),
                "Invalid held ConstantModulation Value"
            );
        }
        self.validate(values[0], frames)?;
        if value_is_static {
            ensure!(
                values.iter().all(|&v| v == values[0]),
                "Inconsistent static Value points"
            );
            return self.next_points_into(values[0], frames, points);
        }
        self.cold = false;
        let mut future = self.source;
        for (&value, start) in values.iter().zip((0..frames).step_by(32)) {
            if ((value - self.source) * self.alpha32).abs() < 1e-7 {
                self.source = value;
                future = value;
            } else {
                let (current, next) =
                    self.ramp(self.source, value, (frames - start).min(32), false, &mut []);
                self.source = current;
                future = next;
            }
        }
        self.target_hz = Self::physical(future);
        self.write_freq_points(frames, points);
        Ok(())
    }
    fn write_freq_points(&mut self, frames: usize, points: &mut [f32]) {
        let target = Self::normalized(self.target_hz);
        if self.normalized_freq == target {
            points.fill(self.target_hz);
            return;
        }
        let (current, _) = self.ramp(self.normalized_freq, target, frames, true, points);
        self.normalized_freq = current;
        for point in points {
            *point = Self::physical(*point);
        }
    }
    fn validate(&self, value: f32, frames: usize) -> Result<()> {
        ensure!(
            frames <= 4096,
            "Unmeasured ConstantModulation control-block length"
        );
        ensure!(
            value.is_finite() && (0. ..=1.).contains(&value),
            "Invalid ConstantModulation Value"
        );
        if self.cold && frames != 0 {
            ensure!(
                value == self.source,
                "Unmeasured cold ConstantModulation Value edit"
            );
        }
        Ok(())
    }
    fn ramp(
        &self,
        current: f32,
        target: f32,
        frames: usize,
        snap: bool,
        points: &mut [f32],
    ) -> (f32, f32) {
        control_ramp(
            (self.alpha_sample, self.alpha32),
            current,
            target,
            frames,
            snap,
            points,
        )
    }
    fn physical(value: f32) -> f32 {
        (f64::from(value) * (20_000_f64.ln() - 20_f64.ln()) + 20_f64.ln()).exp() as f32
    }
    fn normalized(value: f32) -> f32 {
        ((f64::from(value).ln() - 20_f64.ln()) / (20_000_f64.ln() - 20_f64.ln())) as f32
    }
}

/// Native Value-property RC for this measured Constant-to-Comb edition route.
/// Descriptor Smooth=1, physical points held for 32 frames. Feed these points
/// and the returned static flag into `next_value_points_into` for all three RCs.
/// Host serial/coverage, property binding and voice ownership remain above this
/// gated leaf. Process each source owner's host block once, at most 4096 frames.
pub struct CombEditionValueProperty {
    alpha_sample: f32,
    alpha32: f32,
    current: f32,
    static_point: f32,
}
impl CombEditionValueProperty {
    pub fn new(rate: u32, initial_value: f32) -> Result<Self> {
        let (alpha_sample, alpha32) = control_coefficients(rate)?;
        ensure!(
            initial_value.is_finite() && (0. ..=1.).contains(&initial_value),
            "Invalid initial ConstantModulation Value property"
        );
        Ok(Self {
            alpha_sample,
            alpha32,
            current: initial_value,
            static_point: initial_value,
        })
    }
    /// Fill reusable physical Value points; false means this block is dynamic,
    /// including the block that snaps its final current state to the target.
    pub fn next_points_into(
        &mut self,
        value: f32,
        frames: usize,
        points: &mut [f32],
    ) -> Result<bool> {
        ensure!(
            frames <= 4096,
            "Unmeasured Value-property control-block length"
        );
        ensure!(
            points.len() == frames.div_ceil(32),
            "Invalid Value-property point length"
        );
        ensure!(
            value.is_finite() && (0. ..=1.).contains(&value),
            "Invalid ConstantModulation Value property"
        );
        let is_static = self.current == value;
        if frames == 0 {
            return Ok(is_static);
        }
        if is_static {
            points.fill(self.static_point);
        } else {
            self.current = control_ramp(
                (self.alpha_sample, self.alpha32),
                self.current,
                value,
                frames,
                true,
                points,
            )
            .0;
            // Computed physical points canonicalize zero; authored static initial
            // values retain their bits until the property first becomes dynamic.
            for point in points {
                if *point == 0. {
                    *point = 0.;
                }
            }
            self.static_point = self.current + 0.;
        }
        Ok(is_static)
    }
}
fn control_coefficients(rate: u32) -> Result<(f32, f32)> {
    match rate {
        32_000 => Ok((0.003458559513092041_f32, 0.10494154691696167_f32)),
        44_100 => Ok((0.00251084566116333_f32, 0.07729637622833252_f32)),
        48_000 => Ok((0.0023070573806762695_f32, 0.07124549150466919_f32)),
        96_000 => Ok((0.001154184341430664_f32, 0.036280930042266846_f32)),
        _ => anyhow::bail!("Unmeasured ConstantModulation control rate"),
    }
}
fn control_ramp(
    coefficients: (f32, f32),
    mut current: f32,
    target: f32,
    frames: usize,
    snap: bool,
    points: &mut [f32],
) -> (f32, f32) {
    let (alpha_sample, alpha32) = coefficients;
    for (index, start) in (0..frames).step_by(32).enumerate() {
        if let Some(point) = points.get_mut(index) {
            *point = current;
        }
        let count = (frames - start).min(32);
        if count == 32 {
            current += (target - current) * alpha32;
        } else {
            for _ in 0..count {
                current += (target - current) * alpha_sample;
            }
        }
    }
    if snap && (target - current).abs() * alpha32 < 1e-6 {
        current = target;
    }
    let mut lookahead = current;
    for _ in 0..frames.wrapping_neg() & 31 {
        lookahead += (target - lookahead) * alpha_sample;
    }
    (current, lookahead)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_native_scalar_impulses_and_fragmented_channels() {
        // Authored outputs from native execution, not vendor audio or code.
        for &(rate, freq, q, mode, taps) in FIXTURES {
            let mut kernel = CombKernel::new(12, rate, freq, q, mode).unwrap();
            let mut frames = vec![[0.; MAX_CHANNELS]; 4096];
            for c in 0..12 {
                frames[c][c] = 1.;
            }
            for range in [0..1, 1..32, 32..49, 49..305, 305..4096] {
                kernel.process(&mut frames[range]);
            }
            for &(index, bits) in taps {
                for c in 0..12 {
                    assert_eq!(
                        frames[index + c][c].to_bits(),
                        bits,
                        "rate={rate},freq={freq},q={q},mode={mode},index={index},channel={c}"
                    );
                }
            }
            kernel.clear();
            let mut silence = [[0.; MAX_CHANNELS]; 32];
            kernel.process(&mut silence);
            assert_eq!(silence, [[0.; MAX_CHANNELS]; 32]);
        }
        assert!(CombKernel::new(1, 88_200., 1000., 0.5, 0).is_err());
        assert!(CombKernel::new(0, 48_000., 1000., 0.5, 0).is_err());
        assert!(CombKernel::new(1, 48_000., f64::NAN, 0.5, 0).is_err());
        assert!(CombKernel::new(1, 48_000., 1000., 1.1, 0).is_err());
        assert!(CombKernel::new(1, 48_000., 1000., 0.5, 2).is_err());
    }
    #[test]
    fn native_delay_growth_after_bypassed_edit_preserves_history() {
        for &(rate, mode, taps) in DYNAMIC_FIXTURES {
            let mut kernel = CombKernel::new(1, rate, 1008.4754, 0.93642187, mode).unwrap();
            let mut warm = vec![[0.; MAX_CHANNELS]; 128];
            warm[0][0] = 1.;
            kernel
                .process_control_points(&mut warm, &[1008.4754; 4], &[0.93642187; 4], false)
                .unwrap();
            let mut dry = [[0.125; MAX_CHANNELS]; 64];
            kernel
                .process_control_points(&mut dry, &[20.; 2], &[0.93642187; 2], true)
                .unwrap();
            assert_eq!(dry, [[0.125; MAX_CHANNELS]; 64]);
            let mut resumed = vec![[0.; MAX_CHANNELS]; 5000];
            let count = resumed.len().div_ceil(32);
            kernel
                .process_control_points(
                    &mut resumed,
                    &vec![20.; count],
                    &vec![0.93642187; count],
                    false,
                )
                .unwrap();
            for &(index, bits) in taps {
                assert_eq!(
                    resumed[index][0].to_bits(),
                    bits,
                    "rate={rate},mode={mode},index={index}"
                );
            }
            let before = kernel.memory_bytes();
            assert!(
                kernel
                    .process_control_points(&mut resumed[..32], &[f32::NAN], &[0.5], false)
                    .is_err()
            );
            assert_eq!(kernel.memory_bytes(), before);
        }
    }
    #[test]
    fn native_constant_edition_frequency_double_rc_and_partial_spans() {
        for &(rate, expected) in CONSTANT_FIXTURES {
            let mut control = ConstantFrequencyControl::new(rate, 0.1, 39.905247).unwrap();
            assert_eq!(control.next_points(0.1, 32).unwrap(), vec![39.905247]);
            let mut got = Vec::new();
            for count in [256, 17, 65, 33] {
                got.extend(
                    control
                        .next_points(0.5, count)
                        .unwrap()
                        .into_iter()
                        .map(f32::to_bits),
                );
            }
            assert_eq!(got, expected, "rate={rate}");
            assert!(control.next_points(f32::NAN, 32).is_err());
            assert!(control.next_points(1.1, 32).is_err());
        }
        assert!(ConstantFrequencyControl::new(88_200, 0.1, 39.905247).is_err());
        assert!(ConstantFrequencyControl::new(48_000, 0.1, 1000.).is_err());
        let mut cold = ConstantFrequencyControl::new(48_000, 0.1, 39.905247).unwrap();
        assert!(cold.next_points(0.5, 32).is_err());
        assert!(cold.next_points(0.5, usize::MAX).is_err());
        let mut output = [0.; 1];
        assert!(cold.next_points_into(0.1, 32, &mut []).is_err());
        assert!(cold.next_points_into(0.5, usize::MAX, &mut output).is_err());
        cold.next_points_into(0.1, 32, &mut output).unwrap();
        assert_eq!(output, [39.905247]);
    }
    #[test]
    fn native_value_property_held_points_feed_source_and_frequency_rc() {
        for &(rate, blocks) in VALUE_FIXTURES {
            let mut control = ConstantFrequencyControl::new(rate, 0.1, 39.905247).unwrap();
            let mut output = [0.; 128];
            control
                .next_value_points_into(&[0.1; 96], 3072, true, &mut output[..96])
                .unwrap();
            for &(frames, values, expected) in blocks {
                let held: Vec<_> = values.iter().map(|&v| f32::from_bits(v)).collect();
                control
                    .next_value_points_into(&held, frames, false, &mut output[..expected.len()])
                    .unwrap();
                for (&got, &bits) in output.iter().zip(expected) {
                    assert_eq!(got.to_bits(), bits, "rate={rate},frames={frames}");
                }
            }
        }
        let mut cold = ConstantFrequencyControl::new(48_000, 0.1, 39.905247).unwrap();
        let mut point = [123.];
        assert!(
            cold.next_value_points_into(&[0.1, f32::NAN], 64, false, &mut [0.; 2])
                .is_err()
        );
        assert!(
            cold.next_value_points_into(&[0.1], usize::MAX, false, &mut point)
                .is_err()
        );
        assert!(
            cold.next_value_points_into(&[0.1, 0.2], 64, true, &mut [0.; 2])
                .is_err()
        );
        assert_eq!(point, [123.]);
        cold.next_value_points_into(&[0.1], 32, true, &mut point)
            .unwrap();
        assert_eq!(point, [39.905247]);
    }
    #[test]
    fn native_value_property_generator_is_consumed_by_edition_controller() {
        for &(rate, blocks) in VALUE_FIXTURES {
            let mut property = CombEditionValueProperty::new(rate, 0.1).unwrap();
            let mut edition = ConstantFrequencyControl::new(rate, 0.1, 39.905247).unwrap();
            let mut values = [0.; 128];
            let mut frequencies = [0.; 128];
            assert!(
                property
                    .next_points_into(0.1, 3072, &mut values[..96])
                    .unwrap()
            );
            edition
                .next_value_points_into(&values[..96], 3072, true, &mut frequencies[..96])
                .unwrap();
            for &(frames, expected_values, expected_freq) in blocks {
                let count = frames.div_ceil(32);
                let is_static = property
                    .next_points_into(0.5, frames, &mut values[..count])
                    .unwrap();
                assert!(!is_static);
                for (&value, &bits) in values.iter().zip(expected_values) {
                    assert_eq!(value.to_bits(), bits, "rate={rate},frames={frames}");
                }
                edition
                    .next_value_points_into(
                        &values[..count],
                        frames,
                        is_static,
                        &mut frequencies[..count],
                    )
                    .unwrap();
                for (&freq, &bits) in frequencies.iter().zip(expected_freq) {
                    assert_eq!(freq.to_bits(), bits, "rate={rate},frames={frames}");
                }
            }
        }
        assert!(CombEditionValueProperty::new(88_200, 0.1).is_err());
        assert!(CombEditionValueProperty::new(48_000, f32::NAN).is_err());
        let mut property = CombEditionValueProperty::new(48_000, 0.1).unwrap();
        let mut output = [123.];
        assert!(
            property
                .next_points_into(f32::NAN, 32, &mut output)
                .is_err()
        );
        assert!(
            property
                .next_points_into(0.5, usize::MAX, &mut output)
                .is_err()
        );
        assert!(property.next_points_into(0.5, 32, &mut []).is_err());
        assert_eq!(property.current, 0.1);
        assert_eq!(output, [123.]);
        assert!(!property.next_points_into(0.5, 0, &mut []).unwrap());
        assert_eq!(property.current, 0.1);
        // Native snap publishes the old held point, then becomes static next block.
        assert!(
            !property
                .next_points_into(0.10000002, 32, &mut output)
                .unwrap()
        );
        assert_eq!(output, [0.1]);
        assert!(
            property
                .next_points_into(0.10000002, 32, &mut output)
                .unwrap()
        );
        assert_eq!(output, [0.10000002]);
        let mut zero = CombEditionValueProperty::new(48_000, 0.).unwrap();
        assert!(zero.next_points_into(-0., 32, &mut output).unwrap());
        assert_eq!(output[0].to_bits(), 0);
        let mut authored_negative_zero = CombEditionValueProperty::new(48_000, -0.).unwrap();
        assert!(
            authored_negative_zero
                .next_points_into(0., 32, &mut output)
                .unwrap()
        );
        assert_eq!(output[0].to_bits(), (-0_f32).to_bits());
    }
    type ValueBlock = (usize, &'static [u32], &'static [u32]);
    #[rustfmt::skip]
    const VALUE_FIXTURES: &[(u32, &[ValueBlock])] = &[
        (32000, &[(17, &[0x3dcccccd], &[0x421f9ef9]), (65, &[0x3dfba7c9, 0x3e265a6d, 0x3e4aa03d], &[0x421f9ef9, 0x4221b97f, 0x4223a185]), (33, &[0x3e4bb228, 0x3e6c0cc2], &[0x4223b001, 0x4226decd])]),
        (44100, &[(17, &[0x3dcccccd], &[0x421f9ef9]), (65, &[0x3def12ca, 0x3e15df69, 0x3e31dd24], &[0x421f9ef9, 0x42207c07, 0x4221490f]), (33, &[0x3e32b3ea, 0x3e4c7728], &[0x42214f38, 0x4222ab13])]),
        (48000, &[(17, &[0x3dcccccd], &[0x421f9ef9]), (65, &[0x3dec57c0, 0x3e123adb, 0x3e2c4a14], &[0x421f9ef9, 0x42204d20, 0x4220ef89]), (33, &[0x3e2d12b7, 0x3e45385a], &[0x4220f46e, 0x42220913])]),
        (96000, &[(17, &[0x3dcccccd], &[0x421f9ef9]), (65, &[0x3ddcb9df, 0x3df9de9d, 0x3e0afa56], &[0x421f9ef9, 0x421fb6d1, 0x421fcdce]), (33, &[0x3e0b688e, 0x3e18ed28], &[0x421fce83, 0x421ff6b1])]),
    ];
    #[rustfmt::skip]
    const CONSTANT_FIXTURES: &[(u32, &[u32])] = &[
        (32000, &[0x421f9ef9, 0x423d4c67, 0x425c8318, 0x427cca82, 0x428ed5c8, 0x429f5ad8, 0x42afc14c, 0x42bfdc54, 0x42cf861a, 0x42d919cd, 0x42f0f186, 0x43043f8c, 0x43049cd2, 0x43116111]),
        (44100, &[0x421f9ef9, 0x4230a61e, 0x4241f7b5, 0x42537339, 0x4264f9cc, 0x42766ebf, 0x4283dbf4, 0x428c5eef, 0x4294b60c, 0x4299c7d1, 0x42a65324, 0x42b2ce21, 0x42b33181, 0x42c0d341]),
        (48000, &[0x421f9ef9, 0x422e484b, 0x423d1a55, 0x424bfe77, 0x425adf61, 0x4269a954, 0x42784a4f, 0x42835915, 0x428a6955, 0x428eb246, 0x4299464f, 0x42a3cce6, 0x42a420bf, 0x42af9ff2]),
        (96000, &[0x421f9ef9, 0x4223c502, 0x4227dea0, 0x422beb04, 0x422fe973, 0x4233d942, 0x4237b9d6, 0x423b8aa6, 0x423f4b36, 0x42418a8d, 0x4246fee7, 0x424c65d1, 0x424c90ce, 0x42526c48]),
    ];
    type DynamicFixture = (f64, u32, &'static [(usize, u32)]);
    #[rustfmt::skip]
    const DYNAMIC_FIXTURES: &[DynamicFixture] = &[
        (32000.0, 0, &[(1472, 0x3f6fb958), (1503, 0x3e717c05), (1504, 0x3f241c97), (1534, 0x3d734200), (1535, 0x3ea5511d), (1536, 0x3ee0b275), (1565, 0x3c750b53), (1566, 0x3df9cbdd)]),
        (32000.0, 1, &[(1472, 0xbf6fb958), (1503, 0x3e717c05), (1504, 0x3f241c97), (1534, 0xbd734200), (1535, 0xbea5511d), (1536, 0xbee0b275), (1565, 0x3c750b53), (1566, 0x3df9cbdd)]),
        (44100.0, 0, &[(2077, 0x3f6fb958), (2120, 0x3e7300eb), (2121, 0x3f23bb5e), (2163, 0x3d7653fb), (2164, 0x3ea5f8cd), (2165, 0x3edfa889), (4282, 0x3f607b99), (4325, 0x3e638dcb)]),
        (44100.0, 1, &[(2077, 0xbf6fb958), (2120, 0x3e7300eb), (2121, 0x3f23bb5e), (2163, 0xbd7653fb), (2164, 0xbea5f8cd), (2165, 0xbedfa889), (4282, 0x3f607b99), (4325, 0xbe638dcb)]),
        (48000.0, 0, &[(2272, 0x3f6fb958), (2319, 0x3eb51c78), (2320, 0x3f05ed5d), (2366, 0x3e08d44d), (2367, 0x3eca5d1b), (2368, 0x3e95a493), (4672, 0x3f607b99), (4719, 0x3ea998b4)]),
        (48000.0, 1, &[(2272, 0xbf6fb958), (2319, 0x3eb51c78), (2320, 0x3f05ed5d), (2366, 0xbe08d44d), (2367, 0xbeca5d1b), (2368, 0xbe95a493), (4672, 0x3f607b99), (4719, 0xbea998b4)]),
        (96000.0, 0, &[(4672, 0x3f6fb958), (4767, 0x3f351c78), (4768, 0x3e2d7c85)]),
        (96000.0, 1, &[(4672, 0xbf6fb958), (4767, 0x3f351c78), (4768, 0x3e2d7c85)]),
    ];
    type Fixture = (f64, f64, f64, u32, &'static [(usize, u32)]);
    #[rustfmt::skip]
    const FIXTURES: &[Fixture] = &[
        (32000.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (801, 0x3dc08d3f), (802, 0x3f57a7b0), (1602, 0x3c10d429)]),
        (32000.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (801, 0xbdc08d3f), (802, 0xbf57a7b0), (1602, 0x3c10d429)]),
        (32000.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (31, 0x3e80f0a3), (32, 0x3f2f4106), (62, 0x3d81e30a)]),
        (32000.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (31, 0xbe80f0a3), (32, 0xbf2f4106), (62, 0x3d81e30a)]),
        (32000.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (2, 0x3f6fb958), (4, 0x3f607b99), (6, 0x3f5235ea)]),
        (32000.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (2, 0xbf6fb958), (4, 0x3f607b99), (6, 0xbf5235ea)]),
        (44100.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (1105, 0x3f537c2b), (1106, 0x3de1e96a), (2210, 0x3f2eb5ea)]),
        (44100.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (1105, 0xbf537c2b), (1106, 0xbde1e96a), (2210, 0x3f2eb5ea)]),
        (44100.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (43, 0x3e81c04a), (44, 0x3f2ed933), (86, 0x3d8386b6)]),
        (44100.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (43, 0xbe81c04a), (44, 0xbf2ed933), (86, 0x3d8386b6)]),
        (44100.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (2, 0x3f3e949e), (3, 0x3e4492e7), (4, 0x3f0de0f1)]),
        (44100.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (2, 0xbf3e949e), (3, 0xbe4492e7), (4, 0x3f0de0f1)]),
        (48000.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (1202, 0x3e10716d), (1203, 0x3f4b9cfd), (2404, 0x3ca2ff9a)]),
        (48000.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (1202, 0xbe10716d), (1203, 0xbf4b9cfd), (2404, 0x3ca2ff9a)]),
        (48000.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (47, 0x3ec1685f), (48, 0x3f0f0529), (94, 0x3e121e8a)]),
        (48000.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (47, 0xbec1685f), (48, 0xbf0f0529), (94, 0x3e121e8a)]),
        (48000.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (2, 0x3f0fd59a), (3, 0x3ebfc77d), (4, 0x3ea1a0a8)]),
        (48000.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (2, 0xbf0fd59a), (3, 0xbebfc77d), (4, 0x3ea1a0a8)]),
        (96000.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (2405, 0x3e90716d), (2406, 0x3f2780a1)]),
        (96000.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (2405, 0xbe90716d), (2406, 0xbf2780a1)]),
        (96000.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (95, 0x3f41685f), (96, 0x3e3943e5), (190, 0x3f121e8a)]),
        (96000.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (95, 0xbf41685f), (96, 0xbe3943e5), (190, 0x3f121e8a)]),
        (96000.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (4, 0x3e3fc76e), (5, 0x3f3fc77d), (8, 0x3d0fab33)]),
        (96000.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (4, 0xbe3fc76e), (5, 0xbf3fc77d), (8, 0x3d0fab33)]),
    ];
}
