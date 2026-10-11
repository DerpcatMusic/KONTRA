//! NI Kontakt 8 Filter Reference: Phaser. Same-SHA Ghidra FUN_1407c9a70,
//! FUN_14095d770/FUN_140909810; radare2 constructor constants 0x1407c9b19.
use super::{Planar, ProcessorState};

pub(crate) struct Phaser {
    pub(crate) coefficient: f32,
    pub(crate) feedback: f32,
}
impl Phaser {
    pub(super) fn compile([cutoff, resonance]: [f32; 2]) -> Self {
        let v = cutoff * 3.;
        let square = cutoff * cutoff * 9.;
        let cube = square * v;
        let fourth = cube * cutoff * 3.;
        let fifth = fourth * v;
        let exponential = if v.abs() < f32::from_bits(0x3727c5ac) {
            1.
        } else {
            fifth * cutoff * 3. * f32::from_bits(0x3ab60b61)
                + fifth * f32::from_bits(0x3c088889)
                + cube * f32::from_bits(0x3e2aaaab)
                + fourth * f32::from_bits(0x3d2aaaab)
                + square * 0.5
                + v
                + 1.
        };
        let frequency = (f64::from((f64::from(exponential - 1.) * 0.05431093007467753) as f32)
            * 0.32999999999999996
            + 0.02) as f32;
        Self {
            coefficient: (1. - frequency) / (frequency + 1.),
            feedback: (f64::from(resonance) * 0.85) as f32,
        }
    }
    // shortcut: saved filter controls only; bind native cutoff clock when live Phaser controls are admitted.
    pub(super) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        for (channel, samples) in block.iter_mut().enumerate() {
            let base = channel * 8;
            for sample in &mut samples[..len] {
                let dry = *sample as f32;
                let mut input = state.aux[base + 7] as f32 * self.feedback + dry;
                for stage in 0..4 {
                    let output = (input + state.aux[base + 4 + stage] as f32) * self.coefficient
                        - state.aux[base + stage] as f32;
                    state.aux[base + stage] = f64::from(input);
                    state.aux[base + 4 + stage] = f64::from(output);
                    input = output;
                }
                *sample = f64::from((input + dry) * 0.5);
            }
        }
    }
}

/// NI Effect Reference: Legacy Phaser. Same-SHA Ghidra FUN_140aa3830,
/// FUN_1409619b0 (0x140962c75), reset FUN_14094c6d0/FUN_1409600c0.
/// radare2 confirms feedback .85, phase .25 and oscillator step 2*sin(pi*Hz/rate).
pub(crate) struct LegacyPhaser {
    pub(crate) depth: f32,
    pub(crate) feedback: f32,
    phase: f32,
    speed: f32,
    period_unit: Option<f32>,
    rate: u32,
}
impl LegacyPhaser {
    pub(super) fn compile(
        depth: f32,
        feedback: f32,
        phase: f32,
        speed: f32,
        period_unit: Option<f32>,
        rate: u32,
    ) -> Self {
        Self {
            depth,
            feedback: (f64::from(feedback) * 0.85) as f32,
            phase,
            speed,
            period_unit,
            rate,
        }
    }
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        block: &mut Planar,
        len: usize,
        tempo: f64,
    ) {
        if state.delay_filled == 0 {
            state.z[0] = [0., 1.];
            let angle = f64::from(1. - self.phase * 0.25) * std::f64::consts::TAU;
            state.z[1] = [f64::from(angle.sin() as f32), f64::from(angle.cos() as f32)];
            state.delay_filled = 1;
        }
        // FUN_140a736a0: preserve native f32 unit/tempo/speed operation order.
        let hz = self.period_unit.map_or(self.speed, |unit| {
            (1. / (((unit * 60.) / tempo as f32) * self.speed)).clamp(0., 8.)
        });
        let step =
            (2. * (f64::from(hz) * std::f64::consts::PI / f64::from(self.rate)).sin()) as f32;
        for (channel, samples) in block.iter_mut().enumerate() {
            let [mut sine, mut cosine] = state.z[channel].map(|v| v as f32);
            let base = channel * 8;
            for sample in &mut samples[..len] {
                sine -= cosine * step;
                cosine += sine * step;
                let scaled = sine * self.depth;
                let v = scaled * 3.;
                let square = scaled * scaled * 9.;
                let cube = square * v;
                let fourth = cube * v;
                let fifth = fourth * v;
                let exponential = if v.abs() < f32::from_bits(0x3727c5ac) {
                    1.
                } else {
                    fifth * v * f32::from_bits(0x3ab60b61)
                        + fifth * f32::from_bits(0x3c088889)
                        + fourth * f32::from_bits(0x3d2aaaab)
                        + cube * f32::from_bits(0x3e2aaaab)
                        + v
                        + 1.
                        + square * 0.5
                };
                let mapped = (exponential - 1.) * f32::from_bits(0x3c8bc709);
                let coefficient =
                    (f32::from_bits(0x3f7b17e5) - mapped) / (mapped + f32::from_bits(0x3f82740e));
                let mut input = state.aux[base + 7] as f32 * self.feedback + *sample as f32;
                for stage in 0..4 {
                    let output = (input + state.aux[base + stage + 4] as f32) * coefficient
                        - state.aux[base + stage] as f32;
                    state.aux[base + stage] = f64::from(input);
                    state.aux[base + stage + 4] = f64::from(output);
                    input = output;
                }
                *sample = f64::from(input);
            }
            state.z[channel] = [f64::from(sine), f64::from(cosine)];
        }
    }
}
