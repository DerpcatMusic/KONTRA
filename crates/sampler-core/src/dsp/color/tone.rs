//! UVI BitCrusher `Cutoff` tilt filter (Workstation 0x1412d1770 kernel tail).
//!
//! One public control, -1..1: negative selects a low-pass (0x1412dc110), positive
//! a high-pass (0x141336700), zero bypasses. The setter (0x1412d1730 / 0x14156b1e0)
//! indexes a 801-entry table built at start-up (0x14156bc02) with
//! `400 - (int)(cutoff * -400)`, clamps the entry to 0.001..0.999 and scales it by
//! 22050 Hz. Both filters are RBJ biquads with Q = 0.5 (the resonance field stays
//! at its zeroed constructor value), run in Direct Form 1 per channel.
use super::super::Planar;

const STEP: f32 = f32::from_bits(0x3b23_d70a);

/// Table entry `index` (0..=800) as the native start-up loops compute it.
fn table(index: i32) -> f32 {
    if index < 400 {
        (index as f32 * STEP).powf(2.0) * f32::from_bits(0x3f7e_b852) + f32::from_bits(0x3ba3_d70a)
    } else if index < 800 {
        ((index - 400) as f32 * STEP).powf(4.0) * f32::from_bits(0x3e99_999a)
    } else {
        let square = 400f32 * 400f32;
        square * square * f32::from_bits(0x2d4e_288e)
    }
}

/// Cutoff frequency in Hz for a public Cutoff value.
fn frequency(cutoff: f32) -> f32 {
    let index = 400 - (cutoff * -400.0) as i32;
    let entry = table(index.clamp(0, 800));
    let mut clamped = f32::from_bits(0x3a83_126f);
    if clamped <= entry {
        clamped = entry;
        if f32::from_bits(0x3f7f_be77) <= entry {
            clamped = f32::from_bits(0x3f7f_be77);
        }
    }
    clamped * f32::from_bits(0x46ac_4400)
}

/// Biquad coefficients `[b0, b1, b2, a1, a2]`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Tone([f32; 5]);

/// Direct Form 1 histories per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct ToneState {
    x1: [f32; 2],
    x2: [f32; 2],
    y1: [f32; 2],
    y2: [f32; 2],
}

impl ToneState {
    pub(super) fn finite(&self) -> bool {
        [self.x1, self.x2, self.y1, self.y2].iter().flatten().all(|v| v.is_finite())
    }
}

impl Tone {
    /// `None` for Cutoff 0 (no filtering).
    pub(super) fn new(cutoff: f32, rate: u32) -> Option<Self> {
        if cutoff == 0.0 {
            return None;
        }
        let w = frequency(cutoff) * f32::from_bits(0x40c9_0fdb) / rate as f32;
        let (sin, cos) = w.sin_cos();
        // Resonance is 0: 2Q = res * 12 + 1 (low-pass) or res * 24 + 1 (high-pass).
        let alpha = sin / (0.0 * 12.0 + 1.0);
        let norm = 1.0 / (alpha + 1.0);
        let a1 = cos * -2.0 * norm;
        let a2 = (1.0 - alpha) * norm;
        Some(Self(if cutoff < 0.0 {
            let b1 = (1.0 - cos) * norm;
            [b1 * 0.5, b1, b1 * 0.5, a1, a2]
        } else {
            let b0 = (cos + 1.0) * 0.5 * norm;
            [b0, -((cos + 1.0) * norm), b0, a1, a2]
        }))
    }

    pub(super) fn process(&self, state: &mut ToneState, block: &mut Planar, len: usize) {
        let [b0, b1, b2, a1, a2] = self.0;
        for (channel, buffer) in block.iter_mut().enumerate() {
            let (mut x1, mut x2, mut y1, mut y2) =
                (state.x1[channel], state.x2[channel], state.y1[channel], state.y2[channel]);
            for sample in &mut buffer[..len] {
                let x = *sample as f32;
                let y = (((x1 * b1 + x * b0) + b2 * x2) - y1 * a1) - a2 * y2;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                *sample = f64::from(y);
            }
            (state.x1[channel], state.x2[channel], state.y1[channel], state.y2[channel]) = (x1, x2, y1, y2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::BLOCK;
    use super::*;

    #[test]
    fn cutoff_table_matches_the_start_up_loops() {
        // Negative half rises as 0.005 + 0.995 x^2; positive half is 0.3 x^4.
        assert!((frequency(-0.5) - 0.25375 * 22050.0).abs() < 0.5);
        assert!((frequency(0.5) - 0.01875 * 22050.0).abs() < 0.5);
        // Fully open low-pass clamps just below 0.999 * 22050.
        assert!(frequency(-0.001) <= 0.999 * 22050.0 + 1e-3);
        assert!((frequency(1.0) - 0.3 * 22050.0).abs() < 0.5);
    }

    #[test]
    fn low_pass_passes_dc_and_high_pass_removes_it() {
        let run = |cutoff: f32| {
            let tone = Tone::new(cutoff, 48_000).unwrap();
            let mut state = ToneState::default();
            let mut block = [[1.0; BLOCK]; 2];
            for _ in 0..40 {
                block = [[1.0; BLOCK]; 2];
                tone.process(&mut state, &mut block, BLOCK);
            }
            block[0][BLOCK - 1]
        };
        assert!((run(-0.5) - 1.0).abs() < 1e-3, "{}", run(-0.5));
        assert!(run(0.5).abs() < 1e-3, "{}", run(0.5));
        assert!(Tone::new(0.0, 48_000).is_none());
    }

    #[test]
    fn first_sample_is_the_direct_form_one_response() {
        // x = 1 into a fresh filter: y = b0.
        let tone = Tone::new(-0.5, 48_000).unwrap();
        let mut block = [[1.0; BLOCK]; 2];
        tone.process(&mut ToneState::default(), &mut block, 1);
        assert_eq!(block[0][0], f64::from(tone.0[0]));
    }
}
