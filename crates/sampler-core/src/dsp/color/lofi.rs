//! Sample hold, bit quantizer and noise stages.
//!
//! Kontakt BParFXLoFi channel processor (native 0x140aad440). Laws from the wave-3
//! effects spec ("LoFi", audit CORE-EFFECTS-004/005/006): hold 0x140aad4e4..0x140aad679,
//! truncating quantizer 0x140aad679..0x140aad89f, LCG noise 0x140aad8a2..0x140aadacb,
//! parameter derivation 0x140aa3653..0x140aa3758. Saved fields (probe-measured,
//! BParFXLoFi v0x70 `FFBFF`): Bits, Frequency, flag, Noise level, Noise color.
//! Still unproved (reported, not guessed): the +0x1f4 P-truncation flag (P is kept
//! fractional), host bypass lifecycle, and the native reset seed (GetTickCount at
//! 0x14094c5b6; here a fixed non-native default seed).
//!
//! The UVI BitCrusher kernel (0x1412d1770, verify_shape_decimation.py `bitcrusher`)
//! shares the stage slots with its own hold, quantizer and drive laws.
use super::super::Planar;

/// Controls below this are active (native threshold bits 0x3f7fff58).
const ACTIVE_BELOW: f32 = f32::from_bits(0x3f7f_ff58);
/// Non-native default seed used when the state holds none.
const DEFAULT_SEED: u32 = 0x9E37_79B9;

/// Held samples and the shared hold phase.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct HoldState {
    pub held: [f32; 2],
    pub phase: f32,
}

/// Per-channel noise colour history and the shared LCG seed (0 = unseeded).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct NoiseState {
    pub history: [f32; 2],
    pub seed: u32,
}

/// Native hold period for a normalized Frequency control, or none when inactive.
pub(super) fn native_period(frequency: f32, rate: u32) -> Option<f32> {
    let fs = rate as f32;
    (frequency < ACTIVE_BELOW).then(|| (fs / (fs * frequency).max(50.0)).max(1.0))
}

/// Native quantizer step `(Q, 1/Q)` for a normalized Bits control, or none when inactive.
pub(super) fn native_step(bits: f32) -> Option<(f32, f32)> {
    (bits < ACTIVE_BELOW).then(|| {
        let q = 2f64.powf(f64::from((16.0 * bits).max(1.0))) as f32;
        (q, 1.0 / q)
    })
}

/// Native noise parameters `(active, gain, alpha)`.
pub(super) fn native_noise(level: f32, color: f32) -> (bool, f32, f32) {
    (level >= 0.001, (f64::from(level) * 0.075) as f32, 1.0 - 0.9 * color.clamp(0.0, 1.0))
}

/// Kontakt hold: each channel restarts from the block-start phase, the last
/// channel's final phase persists. Capture when `phase >= period`, else
/// `phase += 1` (cadence `period + 1`).
pub(super) fn hold_native(period: f32, state: &mut HoldState, block: &mut Planar, len: usize) {
    let start = state.phase;
    let mut end = start;
    for (channel, buffer) in block.iter_mut().enumerate() {
        let (mut phase, mut held) = (start, state.held[channel]);
        for x in &mut buffer[..len] {
            if phase >= period {
                phase -= period;
                held = *x as f32;
            } else {
                phase += 1.0;
            }
            *x = f64::from(held);
        }
        (state.held[channel], end) = (held, phase);
    }
    state.phase = end;
}

/// UVI BitCrusher hold: capture when the frame index of this call reaches the
/// phase, which then advances by `interval` (`Fs / target rate`, binary32).
pub(super) fn hold_rate(interval: f32, state: &mut HoldState, block: &mut Planar, len: usize) {
    for (channel, buffer) in block.iter_mut().enumerate() {
        let (mut phase, mut held) = (0f32, state.held[channel]);
        for (i, x) in buffer[..len].iter_mut().enumerate() {
            if i as f32 >= phase {
                held = *x as f32;
                phase += interval;
            }
            *x = f64::from(held);
        }
        state.held[channel] = held;
    }
}

/// Kontakt quantizer: truncation toward zero, not round-to-nearest.
pub(super) fn quantize_native((q, inverse): (f32, f32), block: &mut Planar, len: usize) {
    for buffer in block.iter_mut() {
        for x in &mut buffer[..len] {
            *x = f64::from((((*x as f32) * q) as i32) as f32 * inverse);
        }
    }
}

/// UVI quantizer: `floor(x * 2^bits)`, plus one below zero (so a negative exact
/// integer moves toward zero by a whole step), over `2^bits`.
pub(super) fn quantize_floor(bits: f64, block: &mut Planar, len: usize) {
    let scale = 2f64.powf(bits) as f32;
    let reciprocal = 1.0 / scale;
    for buffer in block.iter_mut() {
        for x in &mut buffer[..len] {
            let scaled = (*x as f32) * scale;
            let quantized = scaled.floor() + f32::from(u8::from(scaled < 0.0));
            *x = f64::from(quantized * reciprocal);
        }
    }
}

/// UVI BitCrusher drive: `2q / (1 + |2q| a) * (1 + a) / 2`, `a = d*.95/(1-d*.95)`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Destructor {
    amount: f32,
    normalization: f32,
}
impl Destructor {
    pub(super) fn new(drive: f64) -> Self {
        let scaled = (drive as f32) * 0.95f32;
        let amount = scaled / (1.0 - scaled);
        Self { amount, normalization: (amount + 1.0) * 0.5 }
    }
    pub(super) fn process(&self, block: &mut Planar, len: usize) {
        for buffer in block.iter_mut() {
            for x in &mut buffer[..len] {
                let doubled = (*x as f32) + (*x as f32);
                *x = f64::from(doubled / ((doubled.abs() * self.amount) + 1.0) * self.normalization);
            }
        }
    }
}

/// Kontakt noise: shared LCG, contiguous run per channel, one-pole colour.
pub(super) fn noise(
    (gain, alpha): (f32, f32),
    state: &mut NoiseState,
    block: &mut Planar,
    len: usize,
) {
    let mut seed = if state.seed == 0 { DEFAULT_SEED } else { state.seed };
    for (channel, buffer) in block.iter_mut().enumerate() {
        let mut h = state.history[channel];
        for x in &mut buffer[..len] {
            seed = seed.wrapping_mul(0x0bb3_8435).wrapping_add(0x3619_636b);
            let u = f32::from_bits((seed & 0x007f_ffff) | 0x4000_0000) - 3.0;
            h += alpha * (u - h);
            *x = f64::from((*x as f32) + h * gain);
        }
        state.history[channel] = h;
    }
    state.seed = seed;
}

#[cfg(test)]
mod tests {
    use super::super::super::BLOCK;
    use super::*;

    fn constant(left: f64, right: f64) -> Planar {
        [[left; BLOCK], [right; BLOCK]]
    }

    #[test]
    fn quantizer_truncates_toward_zero_not_nearest() {
        // b=0 -> Q=2: 0.9*2=1.8 truncates to 1 (round would give 2).
        let mut block = constant(0.9, -0.9);
        quantize_native(native_step(0.0).unwrap(), &mut block, BLOCK);
        assert_eq!((block[0][0], block[1][0]), (0.5, -0.5));
    }

    #[test]
    fn hold_has_p_plus_one_cadence() {
        // r=0.5 at 48 kHz -> P=2: capture on frames 2, 5, 8 (phase>=P checked first).
        let mut block = [[0.; BLOCK]; 2];
        for i in 0..BLOCK {
            block[0][i] = (i + 1) as f64;
        }
        let period = native_period(0.5, 48_000).unwrap();
        assert_eq!(period, 2.0);
        hold_native(period, &mut HoldState::default(), &mut block, BLOCK);
        assert_eq!(&block[0][..9], &[0., 0., 3., 3., 3., 6., 6., 6., 9.]);
    }

    #[test]
    fn inactive_controls_report_none() {
        assert!(native_period(1.0, 48_000).is_none());
        assert!(native_step(1.0).is_none());
        assert!(!native_noise(0.0005, 0.0).0);
    }

    #[test]
    fn noise_is_the_native_lcg_and_survives_reset() {
        // seed 1 -> 0x41cce7a0 -> u = 0.20163727; alpha=1 (color 0) so h=u; gain N*0.075.
        let (active, gain, alpha) = native_noise(1.0, 0.0);
        assert!(active);
        let mut state = NoiseState { seed: 1, ..Default::default() };
        let mut block = constant(0., 0.);
        noise((gain, alpha), &mut state, &mut block, BLOCK);
        assert!((block[0][0] - 0.015122795104980468).abs() < 1e-9, "{}", block[0][0]);
        // Channel 1 continues the shared stream after channel 0's run.
        assert_ne!(block[0][0], block[1][0]);
        let (mut again, mut second) = (constant(0., 0.), NoiseState { seed: 1, ..Default::default() });
        noise((gain, alpha), &mut second, &mut again, BLOCK);
        assert_eq!(block, again);
    }

    #[test]
    fn uvi_quantizer_floors_and_moves_negative_integers_toward_zero() {
        // bits 2 -> scale 4. -0.5*4 = -2 exactly: floor(-2) + 1 = -1 -> -0.25 (truncation: -0.5).
        // -0.3*4 = -1.2: floor -2, +1 -> -1 -> -0.25. 0.3*4 = 1.2 -> 1 -> 0.25.
        let mut block = [[-0.5; BLOCK], [0.3; BLOCK]];
        block[0][1] = -0.3;
        quantize_floor(2.0, &mut block, BLOCK);
        assert_eq!((block[0][0], block[0][1], block[1][0]), (-0.25, -0.25, 0.25));
    }

    #[test]
    fn uvi_hold_follows_the_fractional_interval_and_restarts_per_call() {
        // 48 kHz / 16 kHz = interval 3: capture at frames 0, 3, 6.
        let mut block = [[0.; BLOCK]; 2];
        for i in 0..BLOCK {
            block[0][i] = (i + 1) as f64;
        }
        let mut state = HoldState::default();
        hold_rate(3.0, &mut state, &mut block, 8);
        assert_eq!(&block[0][..8], &[1., 1., 1., 4., 4., 4., 7., 7.]);
        // A second call restarts the phase: frame 0 captures again.
        let mut next = constant(9., 9.);
        hold_rate(3.0, &mut state, &mut next, 2);
        assert_eq!(next[0][0], 9.);
    }

    #[test]
    fn uvi_drive_is_identity_at_zero_and_half_scale_is_a_fixed_point() {
        let mut block = constant(0.3, -0.7);
        Destructor::new(0.0).process(&mut block, 4);
        assert!((block[0][0] - 0.3).abs() < 1e-7 && (block[1][0] + 0.7).abs() < 1e-7);
        // |q| = 0.5 -> 1 / (1 + a) * (1 + a) / 2 = 0.5 for any drive.
        let mut half = constant(0.5, -0.5);
        Destructor::new(0.8).process(&mut half, 4);
        assert!((half[0][0] - 0.5).abs() < 1e-6 && (half[1][0] + 0.5).abs() < 1e-6);
        // Drive compresses larger values below the identity.
        let mut loud = constant(1.0, 1.0);
        Destructor::new(0.5).process(&mut loud, 4);
        assert!(loud[0][0] < 1.0 && loud[0][0] > 0.5);
    }
}
