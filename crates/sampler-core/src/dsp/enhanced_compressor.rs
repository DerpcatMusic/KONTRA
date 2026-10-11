//! NI Kontakt 8 Effect Reference: Compressor / Enhanced, Stereo Link.
//! Same-SHA Ghidra FUN_140ab9220/FUN_140ab2c80; radare2 confirms setter
//! 0x140ab938d..0x140ab9470 and attack/hold audio 0x140ab31f1..0x140ab3276.
use super::{Planar, ProcessorState};

// Eight native linear segments. Constants confirmed at 0x144fc3330..0x144fc4600.
const CURVE: [[f32; 3]; 8] = [
    [0.5, 0.46, 0.],
    [1.207, 0.381895, 0.0390524],
    [2.207, 0.34, 0.08962],
    [3.621, 0.29703, 0.184456],
    [5.621, 0.26, 0.31854],
    [8.449, 0.23338, 0.468168],
    [12.449, 0.215, 0.623465],
    [f32::INFINITY, 0.198195, 0.832671],
];
pub(crate) struct Enhanced {
    pub(crate) knobs: [f32; 4],
    threshold: f32,
    excess_scale: f32,
    attack: f32,
    release: f32,
    link: bool,
}
impl Enhanced {
    pub(super) fn compile(knobs: [f32; 4], link: bool, rate: u32) -> Self {
        let [db, ratio, attack_ms, release_ms] = knobs;
        let threshold = (db * f32::from_bits(0x3e2aaaab)).exp2();
        let slope = if ratio < 0.04 {
            5.
        } else {
            1. / (ratio + 0.13) - 0.884
        };
        let attack_factor = (f64::from(attack_ms).ln() * 0.2895294507215381 + 1.) as f32;
        let excess_scale = (slope / threshold.max(0.0004)) * attack_factor;
        let attack = 2513.2 / (rate as f32 * attack_ms.max(1.));
        let release = if release_ms < 50. {
            1. - 35.566 / rate as f32
        } else {
            1. - 1778.3 / (rate as f32 * release_ms)
        };
        Self {
            knobs,
            threshold,
            excess_scale,
            attack,
            release,
            link,
        }
    }
    pub(super) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        for i in 0..len {
            let input = [block[0][i] as f32, block[1][i] as f32];
            let linked = input[0] * 0.5 + input[1] * 0.5;
            for channel in 0..2 {
                let excess =
                    (if self.link { linked } else { input[channel] }).abs() - self.threshold;
                let detected = if excess >= 0. {
                    let x = excess * self.excess_scale;
                    let [_, slope, intercept] = CURVE
                        .iter()
                        .find(|row| x <= row[0])
                        .copied()
                        .unwrap_or(CURVE[7]);
                    x * slope + intercept
                } else {
                    0.
                };
                let [velocity, filtered] = state.z[channel].map(|x| x as f32);
                let velocity = (detected - (velocity * 1.8 + filtered)) * self.attack + velocity;
                let filtered = (velocity * self.attack + 1e-12) + filtered;
                let held = (state.aux[channel] as f32 * self.release).max(filtered);
                state.z[channel] = [f64::from(velocity), f64::from(filtered)];
                state.aux[channel] = f64::from(held);
                block[channel][i] = f64::from((1. / (held + 1.)) * input[channel]);
            }
        }
    }
}
