//! NI Kontakt 8 Effect Reference: Compressor / Enhanced, Stereo Link.
//! Same-SHA Ghidra FUN_140ab9220/FUN_140ab2c80; radare2 confirms setter
//! 0x140ab938d..0x140ab9470 and attack/hold audio 0x140ab31f1..0x140ab3276.
use super::{Planar, ProcessorState, Shared};
use sampler_ir::fx::dynamics::Link;

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

struct Coefficients {
    threshold: f32,
    excess_scale: f32,
    attack: f32,
    release: f32,
}

impl Coefficients {
    /// `knobs` = [threshold dB, saved ratio, attack s, release s].
    fn new([db, ratio, attack, release]: [f64; 4], rate: u32) -> Self {
        let [db, ratio, attack_ms, release_ms] =
            [db as f32, ratio as f32, (attack * 1000.) as f32, (release * 1000.) as f32];
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
            threshold,
            excess_scale,
            attack,
            release,
        }
    }
}

/// State per channel: `z[c] = [attack velocity, filtered]`, `aux[c]` the hold.
pub(super) fn process(
    shared: &Shared,
    knobs: [f64; 4],
    rate: u32,
    state: &mut ProcessorState,
    block: &mut Planar,
    len: usize,
) {
    let c = Coefficients::new(knobs, rate);
    let link = shared.link != Link::Off;
    for i in 0..len {
        let input = [block[0][i] as f32, block[1][i] as f32];
        let linked = input[0] * 0.5 + input[1] * 0.5;
        for channel in 0..2 {
            let excess = (if link { linked } else { input[channel] }).abs() - c.threshold;
            let detected = if excess >= 0. {
                let x = excess * c.excess_scale;
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
            let velocity = (detected - (velocity * 1.8 + filtered)) * c.attack + velocity;
            let filtered = (velocity * c.attack + 1e-12) + filtered;
            let held = (state.aux[channel] as f32 * c.release).max(filtered);
            state.z[channel] = [f64::from(velocity), f64::from(filtered)];
            state.aux[channel] = f64::from(held);
            block[channel][i] =
                f64::from((1. / (held + 1.)) * input[channel]) * shared.factor(1.);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented witness: -6 dB, ratio .5, attack 1 ms, release 100 ms,
    /// 48 kHz, four repeated `[1, -.5]` frames (extracted-law vector).
    #[test]
    fn extracted_law_witness() {
        let shared = Shared { link: Link::Off, makeup: 1., mix: 1. };
        let mut state = ProcessorState::default();
        let mut block = [[0.; super::super::super::BLOCK]; 2];
        for i in 0..4 {
            (block[0][i], block[1][i]) = (1., -0.5);
        }
        process(&shared, [-6., 0.5, 0.001, 0.1], 48_000, &mut state, &mut block, 4);
        let left = [0.999157310, 0.997557640, 0.995283782, 0.992414534];
        for (got, want) in block[0][..4].iter().zip(left) {
            assert!((got - want).abs() < 1e-8, "{got} vs {want}");
        }
        assert!(block[1][..4].iter().all(|v| *v == -0.5));
    }

    #[test]
    fn linked_opposite_channels_pass_unchanged() {
        let shared = Shared { link: Link::Mean, makeup: 1., mix: 1. };
        let mut state = ProcessorState::default();
        let mut block = [[0.; super::super::super::BLOCK]; 2];
        for i in 0..4 {
            (block[0][i], block[1][i]) = (1., -1.);
        }
        process(&shared, [-6., 0.5, 0.001, 0.1], 48_000, &mut state, &mut block, 4);
        assert!(block[0][..4].iter().all(|v| *v == 1.) && block[1][..4].iter().all(|v| *v == -1.));
    }
}
