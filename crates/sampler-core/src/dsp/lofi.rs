//! Port from v1 0cb7a8a0:src/fx/blocks.rs (DriveKind::LoFi).
use super::{Planar, ProcessorState};

/// Saved normalized Bits, Frequency, NoiseLevel and NoiseColor controls.
#[derive(Clone, Copy, Debug)]
pub struct LoFiSettings {
    pub bits: f32,
    pub frequency: f32,
    pub noise: f32,
    pub color: f32,
}
impl LoFiSettings {
    pub(super) fn valid(self) -> bool {
        [self.bits, self.frequency, self.noise, self.color]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }
    pub(super) fn compile(self, rate: u32) -> LoFi {
        let bits = 1.0 + 31.0 * self.bits;
        let step = if bits >= 24.0 {
            0.0
        } else {
            (1.0 - bits).exp2()
        };
        // ponytail: v1's hold proxy; native frequency/interpolation need calibration.
        let hold = 1.0 + 63.0 * (1.0 - self.frequency).powi(3);
        let noise = if self.noise > 0.0 {
            10f32.powf((-96.0 + 90.0 * self.noise) / 20.0)
        } else {
            0.0
        };
        let hz = 20_000.0 * 0.02f32.powf(self.color);
        let color = 1.0 - (-std::f32::consts::TAU * hz.min(0.45 * rate as f32) / rate as f32).exp();
        LoFi {
            step,
            rate: 1.0 / hold,
            noise,
            color,
        }
    }
}

pub(crate) struct LoFi {
    step: f32,
    rate: f32,
    noise: f32,
    color: f32,
}
impl LoFi {
    pub(crate) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        let mut retained = std::array::from_fn::<_, 5, _>(|i| state.aux[i] as f32);
        let mut phase = retained[4];
        let mut rng = if state.delay_position == 0 {
            0x9E37_79B9
        } else {
            state.delay_position
        };
        for i in 0..len {
            phase += self.rate;
            if phase >= 1.0 {
                phase -= 1.0;
                retained[0] = block[0][i] as f32;
                retained[1] = block[1][i] as f32;
            }
            for ch in 0..2 {
                let mut y = retained[ch];
                if self.step > 0.0 {
                    y = (y / self.step).round() * self.step;
                }
                if self.noise > 0.0 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let white = rng as i32 as f32 * (1.0 / 2_147_483_648.0);
                    retained[2 + ch] = super::kernels::biased_one_pole32(retained[2 + ch], self.color * (white - retained[2 + ch]));
                    y += self.noise * retained[2 + ch];
                }
                block[ch][i] = f64::from(y);
            }
        }
        retained[4] = phase;
        for (out, v) in state.aux.iter_mut().zip(retained) {
            *out = f64::from(v);
        }
        state.delay_position = rng;
    }
    pub(crate) fn trace_parameters(&self) -> [(&'static str, f64); 4] {
        [
            ("quantization_step", self.step.into()),
            ("hold_rate", self.rate.into()),
            ("noise_gain", self.noise.into()),
            ("noise_color_coefficient", self.color.into()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::super::BLOCK;
    use super::*;

    #[test]
    fn v1_hold_and_quantization_keep_the_stereo_sample_at_the_native_crossing() {
        let kernel = LoFiSettings {
            bits: 0.,
            frequency: 0.,
            noise: 0.,
            color: 0.,
        }
        .compile(48_000);
        let mut state = ProcessorState::default();
        let mut first = [[0.74; BLOCK], [-0.74; BLOCK]];
        kernel.process(&mut state, &mut first, BLOCK);
        assert_eq!(&first[0][..63], &[0.; 63]);
        assert_eq!(&first[1][..63], &[0.; 63]);
        assert_eq!([first[0][63], first[1][63]], [1., -1.]);
        let mut second = [[-0.74; BLOCK], [0.74; BLOCK]];
        kernel.process(&mut state, &mut second, BLOCK);
        assert_eq!(&second[0][..63], &[1.; 63]);
        assert_eq!(&second[1][..63], &[-1.; 63]);
        assert_eq!([second[0][63], second[1][63]], [-1., 1.]);
    }

    #[test]
    fn colored_noise_and_fractional_hold_are_fragment_independent_and_resettable() {
        let kernel = LoFiSettings {
            bits: 0.17,
            frequency: 0.37,
            noise: 0.63,
            color: 0.7,
        }
        .compile(44_100);
        let source = std::array::from_fn::<_, 2, _>(|c| {
            std::array::from_fn::<_, BLOCK, _>(|i| ((i + 11 * c) as f64 * 0.31).sin() * 0.1)
        });
        let mut state = ProcessorState::default();
        let mut whole = source;
        kernel.process(&mut state, &mut whole, BLOCK);
        let expected_state = (state.aux, state.delay_position);
        state = ProcessorState::default();
        let mut split = source;
        for (start, end) in [(0, 1), (1, 23), (23, 32), (32, BLOCK)] {
            let mut block = [[0.; BLOCK]; 2];
            for c in 0..2 {
                block[c][..end - start].copy_from_slice(&source[c][start..end]);
            }
            kernel.process(&mut state, &mut block, end - start);
            for c in 0..2 {
                split[c][start..end].copy_from_slice(&block[c][..end - start]);
            }
        }
        assert_eq!(whole, split);
        assert_eq!((state.aux, state.delay_position), expected_state);
        assert!(state.finite());
        let mut reset = source;
        kernel.process(&mut ProcessorState::default(), &mut reset, BLOCK);
        assert_eq!(reset, whole);
    }
}

#[cfg(test)]
mod frozen_kernel_tests {
    use super::*;
    fn frozen_process(kernel: &LoFi, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        let mut retained = std::array::from_fn::<_, 5, _>(|i| state.aux[i] as f32);
        let mut phase = retained[4];
        let mut rng = if state.delay_position == 0 {
            0x9E37_79B9
        } else {
            state.delay_position
        };
        for i in 0..len {
            phase += kernel.rate;
            if phase >= 1.0 {
                phase -= 1.0;
                retained[0] = block[0][i] as f32;
                retained[1] = block[1][i] as f32;
            }
            for ch in 0..2 {
                let mut y = retained[ch];
                if kernel.step > 0.0 {
                    y = (y / kernel.step).round() * kernel.step;
                }
                if kernel.noise > 0.0 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let white = rng as i32 as f32 * (1.0 / 2_147_483_648.0);
                    retained[2 + ch] += kernel.color * (white - retained[2 + ch]) + 1e-20;
                    y += kernel.noise * retained[2 + ch];
                }
                block[ch][i] = f64::from(y);
            }
        }
        retained[4] = phase;
        for (out, v) in state.aux.iter_mut().zip(retained) {
            *out = f64::from(v);
        }
        state.delay_position = rng;
    }

    #[test]
    fn shared_lofi_matches_frozen_pcm_and_state_bits() {
        for rate in [44100, 48000, 96000] {
            let kernel = LoFiSettings { bits: 0.17, frequency: 0.37, noise: 0.63, color: 0.7 }.compile(rate);
            for len in [0, 1, 3, 4, 17, super::super::BLOCK] {
                let (mut state, mut expected_state) = (ProcessorState::default(), ProcessorState::default());
                for block_index in 0..8 {
                    let mut actual = std::array::from_fn(|c| std::array::from_fn(|i| match block_index {
                        0 => if i == 0 { 1. } else { 0. },
                        1 => ((i + 11 * c) as f64 * 0.31).sin() * 0.1,
                        _ => if i % 2 == 0 { -0. } else { f64::from_bits(1) },
                    }));
                    let mut expected = actual;
                    kernel.process(&mut state, &mut actual, len);
                    frozen_process(&kernel, &mut expected_state, &mut expected, len);
                    assert_eq!(actual.map(|c| c.map(f64::to_bits)), expected.map(|c| c.map(f64::to_bits)));
                    assert_eq!(state.aux.map(f64::to_bits), expected_state.aux.map(f64::to_bits));
                    assert_eq!(state.delay_position, expected_state.delay_position);
                }
            }
        }
    }
}
