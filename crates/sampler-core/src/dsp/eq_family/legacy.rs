//! Kontakt legacy one-band EQ, standalone SHA 0fe6356e... (KONTAKT_DSP_LAWS).
//! Saved conversion 1409590a0, coefficients 1409591c0, scalar 140aeada0,
//! frozen-target wrapper cadence 140904770. Other native EQ families are distinct.
//!
//! This file is the LAW only: knob conversions, the coefficient generator and the 32-frame
//! control cadence. The recurrence is the one EQ kernel (`kernel.rs`): the native coefficients are
//! realised as a TPT section, which has the same transfer function as the native direct form.
use super::super::{
    ControlRamp, ControlRange, Parameter, Planar, ProcessorState, control::PreparedParameter,
};
use super::kernel;
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
        let c = coefficients(self.rate, values[0], values[1], values[2]);
        for (dst, value) in state.aux[3..8].iter_mut().zip(c) {
            *dst = f64::from(value);
        }
        // Native order b0,b1,b2,a1,a2 with the feedback signs folded in: y = b.x + a.y(past).
        let [b0, b1, b2, a1, a2] = c.map(f64::from);
        let svf = kernel::Svf::from_biquad(&kernel::Biquad { b: [b0, b1, b2], a: [-a1, -a2] })
            .unwrap_or(kernel::Svf::PASS);
        state.aux[9..15].copy_from_slice(&svf.to_array());
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
        mut block: [&mut [f64]; 2],
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
            let svf = kernel::Svf::from_array(std::array::from_fn(|i| state.aux[9 + i]));
            for (channel, history) in block.iter_mut().zip(&mut state.z) {
                svf.run(history, &mut channel[position..position + count]);
                *history = history.map(super::super::flush);
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

#[cfg(test)]
mod tests {
    use super::*;
    fn vectors() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../../../docs/architecture-v2/KONTAKT_DSP_LAWS.vectors.json"
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
    /// The unified kernel driven only by the proved knob laws (Hz, Q, dB) against the native
    /// coefficient set, over the knob range at 44.1/48/96 kHz up to 20 kHz. The native generator is
    /// the RBJ bilinear bell, so the matched design differs near Nyquist and in deep cuts; this is
    /// the error table behind keeping the native coefficients as the `Law` (they still run
    /// through the one kernel). "Musical" rows are band frequency knob <= 0.625 and |gain knob| <= 0.5.
    #[test]
    fn matched_design_versus_native_coefficients_error_table() {
        use sampler_ir::fx::eq::Kind;
        let mut rows = Vec::new();
        for rate in [44_100u32, 48_000, 96_000] {
            // [all, musical] x (max |dB|, max |deg|)
            let mut worst = [[0f64; 2]; 2];
            for fk in 0..=8 {
                for wk in 0..=4 {
                    for gk in -4..=4 {
                        let (f, w, g) = (fk as f32 / 8., wk as f32 / 4., gk as f32 / 4.);
                        let musical = f <= 0.625 && g.abs() <= 0.5;
                        let c = coefficients(rate, f, w, g).map(f64::from);
                        let native = kernel::Biquad { b: [c[0], c[1], c[2]], a: [-c[3], -c[4]] };
                        let u = (exp2_native(f * 8.882606506347656_f32) - 1.) * 0.0021335112396627665_f32;
                        let hz = f64::from((u.clamp(0., 1.) * 19980. + 20.).min(0.49 * rate as f32));
                        let b = 2f64.powf(f64::from(w * 2.6666667461395264_f32 + 1. / 3.));
                        let boost = 120. * f64::from(g) * 2f64.log10();
                        let matched = kernel::design(Kind::Peak, rate, hz, 1. / (b - 1. / b), boost).unwrap();
                        let mut probe = 20.;
                        while probe < 20_000f64.min(0.49 * f64::from(rate)) {
                            let (n, m) = (kernel::response(&native, rate, probe), kernel::response(&matched, rate, probe));
                            let db = (20. * (n.0.hypot(n.1) / m.0.hypot(m.1)).log10()).abs();
                            let deg = (n.1.atan2(n.0) - m.1.atan2(m.0)).to_degrees();
                            let deg = ((deg + 180.).rem_euclid(360.) - 180.).abs();
                            for row in 0..=usize::from(musical) {
                                worst[row][0] = worst[row][0].max(db);
                                worst[row][1] = worst[row][1].max(deg);
                            }
                            probe *= 1.05;
                        }
                    }
                }
            }
            rows.push((rate, worst));
        }
        for (rate, [all, musical]) in &rows {
            eprintln!(
                "legacy bell {rate} Hz: all knobs {:.2} dB {:.1} deg; musical knobs {:.2} dB {:.1} deg",
                all[0], all[1], musical[0], musical[1]
            );
        }
        assert!(rows.iter().all(|(_, w)| w.iter().flatten().all(|v| v.is_finite())));
    }
    /// The native direct form (feedback signs folded into the stored a1, a2) from zero state.
    fn native_direct(c: [f32; 5], x: &[f64]) -> Vec<f64> {
        let [b0, b1, b2, a1, a2] = c.map(f64::from);
        let (mut x1, mut x2, mut y1, mut y2) = (0., 0., 0., 0.);
        x.iter()
            .map(|&x0| {
                let y = a1 * y1 + a2 * y2 + b2 * x2 + b0 * x0 + b1 * x1;
                (x2, x1, y2, y1) = (x1, x0, y1, y);
                y
            })
            .collect()
    }

    /// The one kernel realises each committed native coefficient set with the native transfer
    /// function: the old direct-form recurrence and the TPT section agree to rounding, also
    /// through the 32-frame control cadence of `process_samples`.
    #[test]
    fn kernel_realises_the_committed_native_coefficients() {
        let mut checked = 0;
        for row in vectors()["eq_sample_kernel"].as_array().unwrap() {
            let c: [f32; 5] = std::array::from_fn(|i| scalar(&row["coefficients"][i]));
            let frames = row["frames"].as_u64().unwrap() as usize;
            let eq = settings(0.5, 0.5, f64::from(scalar(&row["gain"])) * 18.);
            let mut state = ProcessorState::default();
            eq.tune(&mut state, [0.5, 0.5, scalar(&row["gain"])], 7);
            assert_eq!(std::array::from_fn::<_, 5, _>(|i| state.aux[i + 3] as f32), c);
            state.aux[8] = 1.;
            state.delay_position = 32;
            let input: Vec<f64> = (0..frames).map(|i| f64::from((i as i32 % 11 - 5) as f32 * 0.03125)).collect();
            let (mut left, mut right) = (input.clone(), input.clone());
            eq.process_samples(&mut state, &[], [&mut left[..], &mut right[..]], 0);
            for (got, want) in left.iter().zip(native_direct(c, &input)) {
                assert!((got - want).abs() < 2e-6 * want.abs().max(1.), "{got} vs {want}: {row}");
            }
            assert_eq!(left, right);
            // Engine processor state is cleared on voice reuse: a silent block stays silent.
            let mut reused = ProcessorState::default();
            let mut silent = [[0.; super::super::super::BLOCK]; 2];
            eq.process(&mut reused, &[], &mut silent, 4, 129);
            assert!(silent.iter().all(|channel| channel[..4].iter().all(|x| *x == 0.)));
            checked += frames;
        }
        assert!(checked >= 129, "the long witness must run");
    }
}
