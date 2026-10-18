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
    pub(super) fn constant(self) -> Option<ConstantEq> {
        match [self.frequency, self.bandwidth, self.gain] {
            [PreparedParameter::Constant(frequency), PreparedParameter::Constant(bandwidth),
                PreparedParameter::Constant(gain)] => Some(ConstantEq { rate: self.rate, frequency, bandwidth, gain }),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(super) fn batches(self) -> bool {
        [self.frequency, self.bandwidth, self.gain].iter()
            .all(|p| matches!(p, PreparedParameter::Constant(_)))
    }



    pub(crate) fn trace_parameters(self) -> [(&'static str, PreparedParameter); 3] {
        [
            ("frequency_knob", self.frequency),
            ("bandwidth_knob", self.bandwidth),
            ("gain_db", self.gain),
        ]
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

}

/// The native recurrence receives an actual control owner. Constants retain
/// their f64 values until the original target conversion; neither owner is
/// downcast or reconstructed while processing.
trait NativeLaw: Copy {
    fn rate(self) -> u32;
    fn targets(self, parameters: &[ControlRamp], at: u64) -> [f32; 3];
    fn modulated(self, parameters: &[ControlRamp]) -> [bool; 3];
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
        let c = coefficients(self.rate(), values[0], values[1], values[2]);
        for (dst, value) in state.aux[3..8].iter_mut().zip(c) {
            *dst = f64::from(value);
        }
        // Native order b0,b1,b2,a1,a2 with the feedback signs folded in: y = b.x + a.y(past).
        let [b0, b1, b2, a1, a2] = c.map(f64::from);
        let svf = kernel::Svf::from_biquad(&kernel::Biquad { b: [b0, b1, b2], a: [-a1, -a2] })
            .unwrap_or(kernel::Svf::PASS);
        state.aux[9..15].copy_from_slice(&svf.to_array());
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
            // Native dirty path (wrapper 0x140918420, flag +0x1a1, also set by local reset
            // 0x14094a790): current cutoff/gain snap to their targets, coefficients once.
            self.tune(state, self.targets(parameters, at), 7);
            state.aux[15] = state.aux[2];
            state.aux[8] = 1.;
            state.delay_position = 32;
        }
        // A lane with a modulation buffer (+0x308/+0x310/+0x318) is latched only when the
        // 32-frame countdown reloads: cutoff snaps (no smoothing), bandwidth applies, gain
        // becomes the smoothing target. Unmodulated lanes follow their setters at once.
        let modulated = self.modulated(parameters);
        let mut position = 0;
        while position < len {
            let live = self.targets(parameters, at + position as u64);
            if !modulated[1] && live[1] != state.aux[1] as f32 {
                self.tune(state, live, 2);
            }
            let targets = |state: &ProcessorState| {
                [
                    if modulated[0] { state.aux[0] as f32 } else { live[0] },
                    if modulated[2] { state.aux[15] as f32 } else { live[2] },
                ]
            };
            // Native settle test: |target - current| as f32 against 1e-4 (cutoff), 1e-5 (gain).
            let unsettled = |state: &ProcessorState, [cutoff, gain]: [f32; 2]| {
                f64::from((cutoff - state.aux[0] as f32).abs()) > 1e-4
                    || f64::from((gain - state.aux[2] as f32).abs()) > 1e-5
            };
            let active = modulated.contains(&true) || unsettled(state, targets(state));
            // Settled and unmodulated: the rest of the call runs on the held coefficients and
            // the countdown stays frozen.
            let count = if active {
                (state.delay_position as usize).min(len - position)
            } else {
                len - position
            };
            let svf = kernel::Svf::from_array(std::array::from_fn(|i| state.aux[9 + i]));
            let [left, right] = &mut block;
            svf.run_stereo(&mut state.z, [&mut left[position..position + count], &mut right[position..position + count]]);
            state.z = state.z.map(|history| history.map(super::super::flush));
            position += count;
            if !active {
                continue;
            }
            state.delay_position -= count as u32;
            let mut mask = 5;
            if state.delay_position == 0 && position < len {
                state.delay_position = 32;
                let latched = self.targets(parameters, at + position as u64);
                if modulated[0] {
                    self.tune(state, latched, 1);
                }
                if modulated[1] && latched[1] != state.aux[1] as f32 {
                    mask |= 2;
                    self.tune(state, latched, 2);
                }
                if modulated[2] {
                    state.aux[15] = f64::from(latched[2]);
                }
            }
            // The native wrapper updates after partial chunks too; inherited
            // zero countdown produces an update before samples in the next call.
            let target = targets(state);
            let (frequency, gain) = (state.aux[0] as f32, state.aux[2] as f32);
            let step = unsettled(state, target);
            self.tune(
                state,
                [
                    if step { frequency + (target[0] - frequency) * 0.01_f32 } else { frequency },
                    state.aux[1] as f32,
                    if step { gain + (target[1] - gain) * 0.01_f32 } else { gain },
                ],
                mask,
            );
        }
    }
}
impl NativeLaw for NativeEq {
    fn rate(self) -> u32 { self.rate }
    fn modulated(self, parameters: &[ControlRamp]) -> [bool; 3] {
        [self.frequency, self.bandwidth, self.gain].map(|p| p.projected(parameters))
    }
    fn targets(self, parameters: &[ControlRamp], at: u64) -> [f32; 3] {
        [
            self.frequency.value(parameters, at, None) as f32,
            self.bandwidth.value(parameters, at, None) as f32,
            KontaktLegacyEqSettings::gain_knob(self.gain.value(parameters, at, None) as f32),
        ]
    }
}

#[derive(Clone, Copy)]
pub(super) struct ConstantEq {
    rate: u32,
    frequency: f64,
    bandwidth: f64,
    gain: f64,
}
impl NativeLaw for ConstantEq {
    fn rate(self) -> u32 { self.rate }
    fn modulated(self, _parameters: &[ControlRamp]) -> [bool; 3] { [false; 3] }
    fn targets(self, _parameters: &[ControlRamp], _at: u64) -> [f32; 3] {
        [self.frequency as f32, self.bandwidth as f32,
            KontaktLegacyEqSettings::gain_knob(self.gain as f32)]
    }
}
impl ConstantEq {
    pub(super) fn into_native(self) -> NativeEq {
        NativeEq { rate: self.rate, frequency: PreparedParameter::Constant(self.frequency),
            bandwidth: PreparedParameter::Constant(self.bandwidth), gain: PreparedParameter::Constant(self.gain) }
    }
    pub(super) fn trace_parameters(self) -> [(&'static str, PreparedParameter); 3] {
        [("frequency_knob", PreparedParameter::Constant(self.frequency)),
         ("bandwidth_knob", PreparedParameter::Constant(self.bandwidth)),
         ("gain_db", PreparedParameter::Constant(self.gain))]
    }
    pub(super) fn process(self, state: &mut ProcessorState, parameters: &[ControlRamp],
        block: &mut Planar, len: usize, at: u64) {
        let [left, right] = block;
        self.process_samples(state, parameters, [&mut left[..len], &mut right[..len]], at);
    }
    pub(super) fn process_lanes(
        self,
        cells: &mut super::super::lanes::Cells<'_>,
        cell: usize,
        batch: &super::super::lanes::Batch,
        block: &mut super::super::lanes::LaneBlock,
        parameters: &[ControlRamp],
        at: u64,
    ) {
        use super::super::lanes::LANES;
        let targets = self.targets(parameters, at);
        let mut held = true;
        for v in 0..batch.count {
            if batch.ends[2 * v] == 0 { continue; }
            let state = &mut cells[v].as_mut().expect("batch voice")[cell];
            if state.aux[8] == 0. {
                self.tune(state, targets, 7);
                state.aux[15] = state.aux[2];
                state.aux[8] = 1.;
                state.delay_position = 32;
            }
            // Stronger than the native settle tolerance: inherited moving
            // coefficients continue through their scalar cadence until exact.
            held &= (0..3).all(|i| state.aux[i] as f32 == targets[i]);
        }
        if !held {
            for v in 0..batch.count {
                let len = batch.ends[2 * v];
                let mut planar = std::array::from_fn(|c| std::array::from_fn(|i| block[i][2*v+c]));
                self.process(&mut cells[v].as_mut().expect("batch voice")[cell], parameters, &mut planar, len, at);
                for i in 0..len { for c in 0..2 { block[i][2*v+c] = planar[c][i]; } }
            }
            return;
        }
        let mut coefficients = [kernel::Svf::PASS; LANES];
        let mut states = [[0.; LANES]; 2];
        for v in 0..batch.count {
            let state = &cells[v].as_ref().expect("batch voice")[cell];
            let coefficient = kernel::Svf::from_array(std::array::from_fn(|i| state.aux[9+i]));
            for c in 0..2 {
                coefficients[2*v+c] = coefficient;
                states[0][2*v+c] = state.z[c][0];
                states[1][2*v+c] = state.z[c][1];
            }
        }
        if batch.ends.iter().all(|&end| end == batch.len) {
            kernel::Svf::run_lanes_dispatched::<LANES, false>(&coefficients, &mut states, &mut block[..batch.len], &batch.ends);
        } else {
            kernel::Svf::run_lanes_dispatched::<LANES, true>(&coefficients, &mut states, &mut block[..batch.len], &batch.ends);
        }
        for v in 0..batch.count {
            if batch.ends[2*v] == 0 { continue; }
            cells[v].as_mut().expect("batch voice")[cell].z = std::array::from_fn(|c| [states[0][2*v+c], states[1][2*v+c]].map(super::super::flush));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constant_voice_lanes_preserve_scalar_pcm_histories_masks_and_inherited_cadence() {
        use super::super::super::lanes::{Batch, Cells, LANES, VOICES};
        use sampler_pool::Slab;
        let eq = settings(0.5, 0.5, 9.);
        assert!(eq.batches());
        for count in [1, 3, VOICES] {
            for unequal in [false, true] {
                for inherited in [false, true] {
                    let mut scalar = [ProcessorState::default(); VOICES];
                    for (v, state) in scalar.iter_mut().enumerate() {
                        state.z = [[(v as f64 * 0.2).sin(), -0.0], [f64::from_bits(1), (v as f64 * 0.3).cos()]];
                        if inherited {
                            eq.tune(state, [0.1 + v as f32 * 0.01, 0.3, -0.2], 7);
                            state.aux[8] = 1.;
                            state.aux[15] = state.aux[2];
                            state.delay_position = [0, 1, 3, 32][v % 4];
                        } else if v % 2 == 0 {
                            eq.tune(state, eq.targets(&[], 0), 7);
                            state.aux[8] = 1.;
                            state.aux[15] = state.aux[2];
                            state.delay_position = 32;
                            // Same held targets can carry distinct retained
                            // coefficients: use each voice's actual values.
                            if v == 2 { state.aux[9..15].copy_from_slice(&kernel::Svf::PASS.to_array()); }
                        }
                    }
                    let slab = Slab::new(scalar.to_vec().into_boxed_slice(), 1);
                    let mut cells: Cells<'_> = std::array::from_fn(|v| (v < count).then(|| slab.claim(v)));
                    for round in 0..80 {
                        let len = [1, 3, 31, 32, 33, 63, 64][round % 7];
                        let ends: [usize; LANES] = std::array::from_fn(|lane| {
                            if lane / 2 >= count { len } else if unequal {
                                [0, 1, 3, 31, 32, 33, 63, 64][lane / 2].min(len)
                            } else { len }
                        });
                        let batch = Batch { count, expressions: [None; VOICES], ends, len };
                        let mut block = std::array::from_fn(|i| std::array::from_fn(|lane| {
                            match (i + lane + round) % 17 {
                                0 => -0.0,
                                1 => f64::from_bits(1),
                                _ => ((i + lane*7 + round*31) as f64 * 0.137).sin(),
                            }
                        }));
                        let mut expected = block;
                        for v in 0..count {
                            let mut planar = std::array::from_fn(|c| std::array::from_fn(|i| block[i][2*v+c]));
                            eq.process(&mut scalar[v], &[], &mut planar, ends[2*v], round as u64 * 64);
                            for i in 0..ends[2*v] { for c in 0..2 { expected[i][2*v+c] = planar[c][i]; } }
                        }
                        eq.constant().unwrap().process_lanes(&mut cells, 0, &batch, &mut block, &[], round as u64 * 64);
                        for v in 0..count {
                            let state = &cells[v].as_ref().unwrap()[0];
                            assert_eq!(state.z.map(|s| s.map(f64::to_bits)), scalar[v].z.map(|s| s.map(f64::to_bits)));
                            assert_eq!(state.aux.map(f64::to_bits), scalar[v].aux.map(f64::to_bits));
                            assert_eq!((state.delay_position, state.delay_filled), (scalar[v].delay_position, scalar[v].delay_filled));
                            for i in 0..64 { for c in 0..2 {
                                assert_eq!(block[i][2*v+c].to_bits(), expected[i][2*v+c].to_bits(), "count={count} unequal={unequal} inherited={inherited} round={round} voice={v} frame={i}");
                            } }
                        }
                    }
                }
            }
        }
    }

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
    /// Kontakt measurement (docs/re/kontakt-bindings/legacy-eq-responses.tsv): stored +12 dB /
    /// 1 oct applies as an RBJ peak of 24.08 dB, Q 0.667 (twice the stored dB and octaves).
    #[test]
    fn stored_twelve_db_one_octave_measures_twenty_four_db() {
        let hz = 1000_f32;
        let c = coefficients(
            48000,
            KontaktLegacyEqSettings::frequency_knob(hz),
            KontaktLegacyEqSettings::bandwidth_knob(1.),
            KontaktLegacyEqSettings::gain_knob(12.),
        )
        .map(f64::from);
        let f = f64::from(KontaktLegacyEqSettings::frequency_hz(KontaktLegacyEqSettings::frequency_knob(hz), 48000));
        let w = std::f64::consts::TAU * f / 48000.;
        let mag = |p: [f64; 3]| {
            let (re, im) = (p[0] + p[1] * w.cos() + p[2] * (2. * w).cos(), -p[1] * w.sin() - p[2] * (2. * w).sin());
            re.hypot(im)
        };
        let db = 20. * (mag([c[0], c[1], c[2]]) / mag([1., -c[3], -c[4]])).log10();
        assert!((db - 24.08).abs() < 0.01, "{db}");
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
    /// Wrapper 0x140918420: a settled, unmodulated band freezes its countdown; a modulated
    /// cutoff snaps to the buffer value when the countdown reloads instead of smoothing.
    #[test]
    fn settled_bands_freeze_the_cadence_and_modulated_cutoff_snaps() {
        let eq = settings(0.5, 0.5, 9.);
        let mut state = ProcessorState::default();
        let (mut left, mut right) = ([0.; 40], [0.; 40]);
        eq.process_samples(&mut state, &[], [&mut left[..], &mut right[..]], 0);
        assert_eq!(state.delay_position, 32, "settled: countdown frozen");

        let mut lanes = Vec::new();
        let eq = KontaktLegacyEqSettings {
            frequency: Parameter::Control(ControlRange { control: crate::ControlId(1), low: 0., high: 1., ramp_frames: 0 }),
            bandwidth: Parameter::Constant(0.5),
            gain_db: Parameter::Constant(9.),
        }
        .compile(48000, &mut lanes)
        .unwrap();
        let mut ramp = ControlRamp::test_ramp(0.5, 0.5, 0, 0);
        ramp.add_modulation(0.25, lanes[0]);
        let mut state = ProcessorState::default();
        eq.process_samples(&mut state, &[ramp], [&mut left[..], &mut right[..]], 0);
        assert_eq!(state.aux[0] as f32, 0.75, "dirty start takes the modulated cutoff");
        let mut moved = ControlRamp::test_ramp(0.5, 0.5, 0, 0);
        moved.add_modulation(-0.25, lanes[0]);
        eq.process_samples(&mut state, &[moved], [&mut left[..10], &mut right[..10]], 40);
        assert_eq!(state.aux[0] as f32, 0.75, "held until the countdown reloads");
        eq.process_samples(&mut state, &[moved], [&mut left[..30], &mut right[..30]], 50);
        assert_eq!(state.aux[0] as f32, 0.25, "snapped, not smoothed");
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
