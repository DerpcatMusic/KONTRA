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
        let exponential = fifth * cutoff * 3. * f32::from_bits(0x3ab60b61)
            + fifth * f32::from_bits(0x3c088889)
            + cube * f32::from_bits(0x3e2aaaab)
            + fourth * f32::from_bits(0x3d2aaaab)
            + square * 0.5 + v + 1.;
        let frequency = (f64::from((f64::from(exponential - 1.) * 0.05431093007467753) as f32)
            * 0.32999999999999996 + 0.02) as f32;
        Self { coefficient: (1. - frequency) / (frequency + 1.), feedback: (f64::from(resonance) * 0.85) as f32 }
    }
    // shortcut: saved filter controls only; bind native cutoff clock when live Phaser controls are admitted.
    pub(super) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        for (channel, samples) in block.iter_mut().enumerate() {
            let base = channel * 8;
            for sample in &mut samples[..len] {
                let dry = *sample as f32;
                let mut input = state.aux[base + 7] as f32 * self.feedback + dry;
                for stage in 0..4 {
                    let output = (input + state.aux[base + 4 + stage] as f32) * self.coefficient - state.aux[base + stage] as f32;
                    state.aux[base + stage] = f64::from(input);
                    state.aux[base + 4 + stage] = f64::from(output);
                    input = output;
                }
                *sample = f64::from((input + dry) * 0.5);
            }
        }
    }
}
