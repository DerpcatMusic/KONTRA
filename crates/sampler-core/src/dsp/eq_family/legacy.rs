//! Kontakt legacy one-band EQ, standalone SHA 0fe6356e... (KONTAKT_DSP_LAWS).
//! Saved conversion 1409590a0, coefficients 1409591c0, scalar 140aeada0,
//! frozen-target wrapper cadence 140904770. Other native EQ families are distinct.
use super::{
    ControlRamp, ControlRange, Parameter, Planar, ProcessorState, control::PreparedParameter,
};
use crate::Error;

#[derive(Clone, Copy, Debug)]
pub struct KontaktLegacyEqSettings {
    pub frequency: Parameter,
    pub bandwidth: Parameter,
    /// Displayed dB; retained for existing script/automation controls.
    pub gain_db: Parameter,
}
impl KontaktLegacyEqSettings {
    pub fn frequency_knob(hz: f32) -> f32 {
        let u = ((hz - 20.) * 0.000050050050049321726_f32).clamp(0., 1.);
        let t = u * 468.7109375 + 1.;
        let z = t.to_bits() as i32 as f32 * (1. / 8388608.) - 127.;
        let q = z - z.floor();
        (z + (q - q * q) * 0.346607_f32) * 0.11257956176996231_f32
    }
    pub fn bandwidth_knob(octaves: f32) -> f32 {
        ((octaves - 1_f32 / 3.) * 0.375).clamp(0., 1.)
    }
    pub fn gain_knob(db: f32) -> f32 {
        ((db - -18.) * f32::from_bits(0x3d638e39) - 1.).clamp(-1., 1.)
    }
    pub fn frequency_hz(normalized: f32, rate: u32) -> f32 {
        let u = (exp2_native(normalized.clamp(0., 1.) * 8.882606506347656_f32) - 1.)
            * 0.0021335112396627665_f32;
        (u.clamp(0., 1.) * 19980. + 20.).min(0.49 * rate as f32)
    }
    pub(super) fn valid(self) -> bool {
        [self.frequency, self.bandwidth].iter().all(|p| {
            p.valid()
                && !matches!(p, Parameter::Expression { .. })
                && p.bounds().iter().all(|v| (0. ..=1.).contains(v))
        }) && self.gain_db.valid()
            && !matches!(self.gain_db, Parameter::Expression { .. })
            && self
                .gain_db
                .bounds()
                .iter()
                .all(|v| (-18. ..=18.).contains(v))
    }
    pub(super) fn compile(
        self,
        rate: u32,
        bindings: &mut Vec<ControlRange>,
    ) -> Result<NativeEq, Error> {
        if rate == 0 || !self.valid() {
            return Err(Error::InvalidInput);
        }
        Ok(NativeEq {
            rate,
            frequency: self.frequency.compile(bindings),
            bandwidth: self.bandwidth.compile(bindings),
            gain: self.gain_db.compile(bindings),
        })
    }
}

fn exp2_native(z: f32) -> f32 {
    let q = z - z.floor();
    f32::from_bits(((z + 127. - (q - q * q) * 0.33971_f32) * 8388608.) as u32)
}

/// Stored native order b0,b1,b2,a1,a2 (feedback signs already included).
fn coefficients(rate: u32, frequency: f32, width: f32, gain: f32) -> [f32; 5] {
    let u = (exp2_native(frequency * 8.882606506347656_f32) - 1.) * 0.0021335112396627665_f32;
    let hz = u.clamp(0., 1.) * 19980. + 20.;
    let omega = (f64::from((hz / rate as f32).min(0.49_f32)) * std::f64::consts::TAU) as f32;
    let (sine, cosine) = (f64::from(omega).sin() as f32, f64::from(omega).cos() as f32);
    let a = f64::from(exp2_native(((gain * 0.5 + 0.5) * 6.) - 3.));
    let b = 2_f64.powf(f64::from(width * 2.6666667461395264_f32 + 1_f32 / 3.));
    let alpha = f64::from(sine) * (0.5 / (1. / (b - 1. / b)));
    let (p, q) = (alpha / a, alpha * a);
    let d = 1. / (1. + p);
    [
        (q + 1.) as f64 * d,
        -2. * f64::from(cosine) * d,
        (1. - q) * d,
        2. * f64::from(cosine) * d,
        (p - 1.) * d,
    ]
    .map(|v| v as f32)
}

#[derive(Clone, Copy)]
pub(crate) struct NativeEq {
    rate: u32,
    frequency: PreparedParameter,
    bandwidth: PreparedParameter,
    gain: PreparedParameter,
}
impl NativeEq {
    pub(crate) fn trace_parameters(self) -> [(&'static str, PreparedParameter); 3] {
        [
            ("frequency_knob", self.frequency),
            ("bandwidth_knob", self.bandwidth),
            ("gain_db", self.gain),
        ]
    }
    fn targets(self, parameters: &[ControlRamp], at: u64) -> [f32; 3] {
        [
            self.frequency.value(parameters, at, None) as f32,
            self.bandwidth.value(parameters, at, None) as f32,
            KontaktLegacyEqSettings::gain_knob(self.gain.value(parameters, at, None) as f32),
        ]
    }
    fn tune(self, state: &mut ProcessorState, incoming: [f32; 3], mask: u8) {
        if mask == 0 {
            return;
        }
        let mut values: [f32; 3] = std::array::from_fn(|i| state.aux[i] as f32);
        for i in 0..3 {
            if mask & (1 << i) != 0 {
                values[i] = incoming[i].clamp(if i == 2 { -1. } else { 0. }, 1.);
            }
        }
        for (dst, value) in state.aux[0..3].iter_mut().zip(values) {
            *dst = f64::from(value);
        }
        for (dst, value) in state.aux[3..8]
            .iter_mut()
            .zip(coefficients(self.rate, values[0], values[1], values[2]))
        {
            *dst = f64::from(value);
        }
    }
    pub(super) fn process(
        self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
    ) {
        let [left, right] = block;
        self.process_samples(state, parameters, [&mut left[..len], &mut right[..len]], at);
    }
    fn process_samples(
        self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: [&mut [f64]; 2],
        at: u64,
    ) {
        let len = block[0].len();
        if len == 0 {
            return;
        }
        if state.aux[8] == 0. {
            // Prepared saved state starts settled. Native initial dirty/reset and
            // modulation-buffer cadence remain unverified; do not invent a ramp from zero.
            self.tune(state, self.targets(parameters, at), 7);
            state.aux[8] = 1.;
            state.delay_position = 32;
        }
        let mut position = 0;
        while position < len {
            let count = (state.delay_position as usize).min(len - position);
            let c = std::array::from_fn(|i| state.aux[3 + i] as f32);
            for channel in 0..2 {
                let base = 9 + channel * 2;
                let mut h = [
                    state.aux[base] as f32,
                    state.aux[base + 1] as f32,
                    state.z[channel][0] as f32,
                    state.z[channel][1] as f32,
                ];
                for sample in &mut block[channel][position..position + count] {
                    *sample = f64::from(sample_native(c, &mut h, *sample as f32));
                }
                state.aux[base] = f64::from(h[0]);
                state.aux[base + 1] = f64::from(h[1]);
                state.z[channel] = [f64::from(h[2]), f64::from(h[3])];
            }
            position += count;
            state.delay_position -= count as u32;
            if state.delay_position == 0 && position < len {
                state.delay_position = 32;
            }
            // The native wrapper updates after partial chunks too; inherited
            // zero countdown produces an update before samples in the next call.
            let target = self.targets(parameters, at + position.saturating_sub(1) as u64);
            let frequency = state.aux[0] as f32;
            let gain = state.aux[2] as f32;
            let mask = 5 | if target[1] != state.aux[1] as f32 {
                2
            } else {
                0
            };
            self.tune(
                state,
                [
                    frequency + (target[0] - frequency) * 0.01_f32,
                    target[1],
                    gain + (target[2] - gain) * 0.01_f32,
                ],
                mask,
            );
        }
    }
}

#[inline(always)]
fn sample_native([b0, b1, b2, a1, a2]: [f32; 5], h: &mut [f32; 4], x: f32) -> f32 {
    let [x1, x2, y1, y2] = *h;
    // addss 140aeafcb groups both feedforward terms before adding them to
    // the feedback/history sum. Left association loses bits on long tails.
    let y = (((a1 * y1 + a2 * y2) + b2 * x2) + (b0 * x + b1 * x1)) + 0.;
    *h = [x, x1, y, y1];
    y
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vectors() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../../docs/architecture-v2/KONTAKT_DSP_LAWS.vectors.json"
        ))
        .unwrap()
    }
    fn scalar(v: &serde_json::Value) -> f32 {
        v.as_f64().unwrap() as f32
    }
    fn settings(frequency: f64, bandwidth: f64, gain_db: f64) -> NativeEq {
        KontaktLegacyEqSettings {
            frequency: Parameter::Constant(frequency),
            bandwidth: Parameter::Constant(bandwidth),
            gain_db: Parameter::Constant(gain_db),
        }
        .compile(48000, &mut Vec::new())
        .unwrap()
    }
    #[test]
    fn saved_conversion_matches_original_instruction_vectors() {
        for row in vectors()["eq_saved_conversion"].as_array().unwrap() {
            let expected = &row["normalized"];
            assert_eq!(
                KontaktLegacyEqSettings::frequency_knob(scalar(&row["hz"])),
                scalar(&expected[0])
            );
            assert_eq!(
                KontaktLegacyEqSettings::bandwidth_knob(scalar(&row["octaves"])),
                scalar(&expected[1])
            );
            assert_eq!(
                KontaktLegacyEqSettings::gain_knob(scalar(&row["db"])),
                scalar(&expected[2])
            );
        }
        assert_eq!(
            KontaktLegacyEqSettings::frequency_hz(0.5, 48000),
            903.2728881835938_f32
        );
    }
    #[test]
    fn coefficients_match_original_instruction_vectors_at_all_rates() {
        for row in vectors()["eq_coefficients"].as_array().unwrap() {
            let actual = coefficients(
                row["rate"].as_u64().unwrap() as u32,
                scalar(&row["cutoff"]),
                scalar(&row["width"]),
                scalar(&row["gain"]),
            );
            let expected: [f32; 5] = std::array::from_fn(|i| scalar(&row["coefficients"][i]));
            assert_eq!(actual, expected, "{row}");
        }
    }
    #[test]
    fn frozen_target_wrapper_cadence_matches_native_block_partitions() {
        for row in vectors()["eq_control_cadence"].as_array().unwrap() {
            let eq = settings(0.5, 0.5, 9.);
            let mut state = ProcessorState::default();
            eq.tune(&mut state, [0.1, 0.5, 0.1], 7);
            state.aux[8] = 1.;
            state.delay_position = 32;
            let mut at = 0;
            for observation in row["observations"].as_array().unwrap() {
                let frames = observation["frames"].as_u64().unwrap() as usize;
                let (mut left, mut right) = ([0.; 97], [0.; 97]);
                eq.process_samples(
                    &mut state,
                    &[],
                    [&mut left[..frames], &mut right[..frames]],
                    at,
                );
                assert_eq!(
                    state.delay_position,
                    observation["countdown"].as_u64().unwrap() as u32
                );
                assert_eq!(state.aux[0] as f32, scalar(&observation["frequency"]));
                assert_eq!(state.aux[2] as f32, scalar(&observation["gain"]));
                at += frames as u64;
            }
        }
    }
    #[test]
    fn nonzero_scalar_recurrence_preserves_native_history_and_order() {
        // Nonzero coefficients are independently instruction-checked; these
        // samples are computed recurrence vectors, not new host recordings.
        let c = coefficients(48000, 0.5, 0.5, 0.5);
        let mut history = [0.125, -0.25, 0.375, -0.5];
        let actual = [1., 0., -0.25, 0.5].map(|x| sample_native(c, &mut history, x));
        assert_eq!(
            actual,
            [
                2.1833057403564453_f32,
                1.946739912033081_f32,
                1.855787992477417_f32,
                2.916332244873047_f32
            ]
        );
        assert_eq!(history, [0.5, -0.25, actual[3], actual[2]]);
    }
    #[test]
    fn parameter_masks_clamp_only_updated_fields_and_retain_coefficients() {
        let eq = settings(0.25, 0.5, 4.5);
        for row in vectors()["eq_update_masks"].as_array().unwrap() {
            let mut state = ProcessorState::default();
            eq.tune(&mut state, [0.25, 0.5, 0.25], 7);
            eq.tune(
                &mut state,
                std::array::from_fn(|i| scalar(&row["incoming"][i])),
                row["mask"].as_u64().unwrap() as u8,
            );
            for i in 0..3 {
                assert_eq!(state.aux[i] as f32, scalar(&row["stored"][i]));
            }
            for i in 0..5 {
                assert_eq!(state.aux[i + 3] as f32, scalar(&row["coefficients"][i]));
            }
        }
    }
    #[test]
    fn nonzero_scalar_pcm_and_histories_match_original_instruction_vectors() {
        let mut long_nonzero = false;
        for row in vectors()["eq_sample_kernel"].as_array().unwrap() {
            let c: [f32; 5] = std::array::from_fn(|i| scalar(&row["coefficients"][i]));
            assert!(
                c.iter().any(|x| *x != 0.),
                "coefficient setter must execute in native harness"
            );
            let frames = row["frames"].as_u64().unwrap() as usize;
            assert_eq!(row["reset_checked"], true);
            assert_eq!(row["vector_dispatch_disabled"], true);
            let eq = settings(0.5, 0.5, f64::from(scalar(&row["gain"])) * 18.);
            let mut state = ProcessorState::default();
            eq.tune(&mut state, [0.5, 0.5, scalar(&row["gain"])], 7);
            assert_eq!(
                std::array::from_fn::<_, 5, _>(|i| state.aux[i + 3] as f32),
                c
            );
            state.aux[8] = 1.;
            state.delay_position = 32;
            let (mut left, mut right) = ([0.; 129], [0.; 129]);
            for i in 0..frames {
                let input = f64::from((i as i32 % 11 - 5) as f32 * 0.03125);
                left[i] = input;
                right[i] = input;
            }
            for channel in 0..row["channels"].as_u64().unwrap() as usize {
                state.aux[9 + channel * 2] = 0.125;
                state.aux[10 + channel * 2] = -0.25;
                state.z[channel] = [0.375, -0.5];
                let mut history = [0.125, -0.25, 0.375, -0.5];
                for i in 0..frames {
                    let input = (i as i32 % 11 - 5) as f32 * 0.03125;
                    assert_eq!(
                        sample_native(c, &mut history, input),
                        scalar(&row["samples"][channel][i]),
                        "frame {i}: {row}"
                    );
                }
                assert_eq!(
                    history,
                    std::array::from_fn(|i| scalar(&row["final_histories"][channel][i]))
                );
            }
            eq.process_samples(
                &mut state,
                &[],
                [&mut left[..frames], &mut right[..frames]],
                0,
            );
            for channel in 0..row["channels"].as_u64().unwrap() as usize {
                for (i, sample) in [left, right][channel][..frames].iter().enumerate() {
                    assert_eq!(*sample as f32, scalar(&row["samples"][channel][i]));
                }
                let actual = [
                    state.aux[9 + channel * 2] as f32,
                    state.aux[10 + channel * 2] as f32,
                    state.z[channel][0] as f32,
                    state.z[channel][1] as f32,
                ];
                assert_eq!(
                    actual,
                    std::array::from_fn(|i| scalar(&row["final_histories"][channel][i]))
                );
            }
            // Engine processor state is cleared on voice reuse. Its next saved
            // initialization snaps controls and cannot retain the old tail.
            let mut reused = ProcessorState::default();
            let mut silent = [[0.; super::super::BLOCK]; 2];
            eq.process(&mut reused, &[], &mut silent, 4, 129);
            assert!(
                silent
                    .iter()
                    .all(|channel| channel[..4].iter().all(|x| *x == 0.))
            );
            long_nonzero |= frames == 129;
        }
        assert!(
            long_nonzero,
            "long recurrence witness must cover the old sample-63 rounding regression"
        );
    }
}
