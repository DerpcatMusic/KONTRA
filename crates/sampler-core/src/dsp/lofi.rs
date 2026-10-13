//! Kontakt BParFXLoFi channel processor (native 0x140aad440). Laws from the wave-3
//! effects spec ("LoFi", audit CORE-EFFECTS-004/005/006): hold 0x140aad4e4..0x140aad679,
//! truncating quantizer 0x140aad679..0x140aad89f, LCG noise 0x140aad8a2..0x140aadacb,
//! parameter derivation 0x140aa3653..0x140aa3758.
//! Still unproved (reported, not guessed): which saved field feeds each control
//! (here bits->b, frequency->r, noise->N, color->t), the P-truncation flag (+0x1f4;
//! P is kept fractional), host bypass lifecycle, and the native reset seed
//! (GetTickCount at 0x14094c5b6; here a fixed non-native default seed).
use super::{Planar, ProcessorState};

/// Controls below this are active (native threshold bits 0x3f7fff58).
const ACTIVE_BELOW: f32 = f32::from_bits(0x3f7f_ff58);
/// Non-native default seed used when the state holds none.
const DEFAULT_SEED: u32 = 0x9E37_79B9;

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
        let quant = (self.bits < ACTIVE_BELOW).then(|| {
            let q = 2f64.powf(f64::from((16.0 * self.bits).max(1.0))) as f32;
            (q, 1.0 / q)
        });
        let fs = rate as f32;
        let period = (self.frequency < ACTIVE_BELOW)
            .then(|| (fs / (fs * self.frequency).max(50.0)).max(1.0));
        LoFi {
            quant,
            period,
            noise_active: self.noise >= 0.001,
            noise_gain: (f64::from(self.noise) * 0.075) as f32,
            alpha: 1.0 - 0.9 * self.color.clamp(0.0, 1.0),
        }
    }
}

pub(crate) struct LoFi {
    quant: Option<(f32, f32)>,
    period: Option<f32>,
    noise_active: bool,
    noise_gain: f32,
    alpha: f32,
}
impl LoFi {
    pub(crate) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        // aux: [held0, held1, noise0, noise1, phase]
        let mut aux = std::array::from_fn::<_, 5, _>(|i| state.aux[i] as f32);
        let start_phase = aux[4];
        let mut seed = if state.delay_position == 0 {
            DEFAULT_SEED
        } else {
            state.delay_position
        };
        let mut end_phase = start_phase;
        // Native order: each channel runs the whole block (hold, quantize, noise); the
        // noise stream is shared, so each channel draws a contiguous run.
        for ch in 0..2 {
            let mut phase = start_phase;
            let (mut held, mut h) = (aux[ch], aux[2 + ch]);
            for i in 0..len {
                let mut y = block[ch][i] as f32;
                if let Some(p) = self.period {
                    if phase >= p {
                        phase -= p;
                        held = y;
                    } else {
                        phase += 1.0;
                    }
                    y = held;
                }
                if let Some((q, inv)) = self.quant {
                    y = ((y * q) as i32) as f32 * inv; // truncation toward zero
                }
                if self.noise_active {
                    seed = seed.wrapping_mul(0x0bb3_8435).wrapping_add(0x3619_636b);
                    let u = f32::from_bits((seed & 0x007f_ffff) | 0x4000_0000) - 3.0;
                    h += self.alpha * (u - h);
                    y += h * self.noise_gain;
                }
                block[ch][i] = f64::from(y);
            }
            (aux[ch], aux[2 + ch], end_phase) = (held, h, phase);
        }
        aux[4] = end_phase;
        for (out, v) in state.aux.iter_mut().zip(aux) {
            *out = f64::from(v);
        }
        state.delay_position = seed;
    }
    pub(crate) fn trace_parameters(&self) -> [(&'static str, f64); 4] {
        [
            ("quantization_step", self.quant.map_or(0.0, |(_, inv)| inv.into())),
            ("hold_period", self.period.map_or(0.0, f64::from)),
            ("noise_gain", if self.noise_active { self.noise_gain.into() } else { 0.0 }),
            ("noise_alpha", self.alpha.into()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::super::BLOCK;
    use super::*;

    fn kernel(bits: f32, frequency: f32, noise: f32, color: f32) -> LoFi {
        LoFiSettings { bits, frequency, noise, color }.compile(48_000)
    }

    #[test]
    fn quantizer_truncates_toward_zero_not_nearest() {
        // b=0 -> Q=2: 0.9*2=1.8 truncates to 1 (round would give 2).
        let mut block = [[0.9; BLOCK], [-0.9; BLOCK]];
        kernel(0.0, 1.0, 0.0, 0.0).process(&mut ProcessorState::default(), &mut block, BLOCK);
        assert_eq!((block[0][0], block[1][0]), (0.5, -0.5));
    }

    #[test]
    fn hold_has_p_plus_one_cadence() {
        // r=0.5 at 48 kHz -> P=2: capture on frames 2, 5, 8 (phase>=P checked first).
        let mut block = [[0.; BLOCK]; 2];
        for i in 0..BLOCK {
            block[0][i] = (i + 1) as f64;
        }
        kernel(1.0, 0.5, 0.0, 0.0).process(&mut ProcessorState::default(), &mut block, BLOCK);
        assert_eq!(&block[0][..9], &[0., 0., 3., 3., 3., 6., 6., 6., 9.]);
    }

    #[test]
    fn noise_is_the_native_lcg_and_survives_reset() {
        // seed 1 -> 0x41cce7a0 -> u = 0.20163727; alpha=1 (color 0) so h=u; gain N*0.075.
        let k = kernel(1.0, 1.0, 1.0, 0.0);
        let mut state = ProcessorState::default();
        state.delay_position = 1;
        let mut block = [[0.; BLOCK]; 2];
        k.process(&mut state, &mut block, BLOCK);
        assert!((block[0][0] - 0.015122795104980468).abs() < 1e-9, "{}", block[0][0]);
        // Channel 1 continues the shared stream after channel 0's run.
        assert_ne!(block[0][0], block[1][0]);
        assert!(state.finite());
        let (mut again, mut s2) = ([[0.; BLOCK]; 2], ProcessorState::default());
        s2.delay_position = 1;
        k.process(&mut s2, &mut again, BLOCK);
        assert_eq!(block, again);
    }
}
