//! Verified two-channel Stereo Modeller laws (KONTAKT_DSP_LAWS).
use super::{ControlRamp, Parameter, Planar, ProcessorState, control::PreparedParameter};

#[derive(Clone, Copy, Debug)]
pub struct StereoSettings {
    /// Native width 0..1; .5 is identity.
    pub width: Parameter,
    /// Linear balance -1..1.
    pub pan: Parameter,
    pub pseudo: bool,
}
impl StereoSettings {
    pub(super) fn valid(self) -> bool {
        self.width.valid() && self.pan.valid()
            && self.width.bounds().iter().all(|v| (0.0..=1.0).contains(v))
            && self.pan.bounds().iter().all(|v| (-1.0..=1.0).contains(v))
            && !matches!(self.width, Parameter::Expression { .. })
            && !matches!(self.pan, Parameter::Expression { .. })
    }
    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<super::ControlRange>) -> Stereo {
        Stereo { width: self.width.compile(bindings), pan: self.pan.compile(bindings), pseudo: self.pseudo, rate: rate as f32 }
    }
}
pub(crate) struct Stereo {
    width: PreparedParameter,
    pan: PreparedParameter,
    pseudo: bool,
    rate: f32,
}
impl Stereo {
    pub(super) fn process(&self, state: &mut ProcessorState, parameters: &[ControlRamp], block: &mut Planar, len: usize, at: u64, ring: &mut [[f64; 2]]) {
        let (mut width, mut pan) = (state.aux[0] as f32, state.aux[1] as f32);
        let mut pan_delta = 0.0;
        let [left, right] = block;
        for i in 0..len {
            let target_width = self.width.value(parameters, at + i as u64, None) as f32;
            let target_pan = self.pan.value(parameters, at + i as u64, None) as f32;
            if state.aux[2] == 0.0 {
                (width, pan, state.aux[2]) = (target_width, target_pan, 1.0);
            }
            let (l, r) = (left[i] as f32, right[i] as f32);
            let (l, r) = if self.pseudo {
                let delay = (self.rate * 0.01f32 * width * width * width) as usize;
                let delay = delay.min(1023);
                let cursor = state.delay_position as usize;
                ring[cursor][1] = f64::from(r);
                let delayed = if delay <= state.delay_filled as usize {
                    ring[(cursor + 1024 - delay) & 1023][1] as f32
                } else { 0.0 };
                state.delay_position = ((cursor + 1) & 1023) as u32;
                state.delay_filled = (state.delay_filled + 1).min(1023);
                (l, delayed)
            } else if width >= 0.5 {
                let spread = 2.0 * width - 1.0;
                ((1.0 + spread) * l - spread * r, (1.0 + spread) * r - spread * l)
            } else {
                let a = 0.5 - width;
                (l + a * (r - l), r + a * (l - r))
            };
            left[i] = f64::from(l * (1.0 - pan.max(0.0)));
            right[i] = f64::from(r * (1.0 + pan.min(0.0)));
            width += (target_width - width) * (1.0f32 / 180.0);
            // Native SIMD's fourth advance reuses the third delta. Scalar
            // remainder samples compute a fresh delta (groups restart per call).
            if i >= len / 4 * 4 || i % 4 != 3 {
                pan_delta = (target_pan - pan) * f32::from_bits(0x3a11a2b4);
            }
            pan += pan_delta;
        }
        (state.aux[0], state.aux[1]) = (f64::from(width), f64::from(pan));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings(width: f64, pan: f64, pseudo: bool) -> Stereo {
        StereoSettings { width: Parameter::Constant(width), pan: Parameter::Constant(pan), pseudo }.compile(48000, &mut Vec::new())
    }
    #[test]
    fn settled_matrix_and_pseudo_impulse_follow_native_laws() {
        for (width, expected) in [(0.0, [1.5, 1.5]), (0.5, [1.0, 2.0]), (1.0, [0.0, 3.0])] {
            let mut block = [[0.0; super::super::BLOCK]; 2];
            (block[0][0], block[1][0]) = (1.0, 2.0);
            settings(width, 0.0, false).process(&mut ProcessorState::default(), &[], &mut block, 1, 0, &mut []);
            assert_eq!([block[0][0], block[1][0]], expected);
        }
        let mut state = ProcessorState::default();
        let mut ring = [[0.0; 2]; 1024];
        let mut block = [[0.0; super::super::BLOCK]; 2];
        (block[0][0], block[1][0]) = (1.0, 1.0);
        settings(0.5, 0.0, true).process(&mut state, &[], &mut block, 64, 0, &mut ring);
        assert_eq!(block[0][0], 1.0);
        assert!(block[1][..60].iter().all(|v| *v == 0.0));
        assert_eq!(block[1][60], 1.0);
    }
    #[test]
    fn moving_width_and_pan_preserve_the_native_four_frame_recurrence() {
        let mut state = ProcessorState::default();
        let mut block = [[1.0; super::super::BLOCK]; 2];
        settings(0.6, -0.8, false).process(&mut state, &[], &mut block, 1, 0, &mut []);
        settings(0.9, -0.2, false).process(&mut state, &[], &mut block, 4, 1, &mut []);
        assert_eq!(state.aux[0] as f32, 0.6066113710403442f32);
        assert_eq!(state.aux[1] as f32, -0.7986676692962646f32);
    }
}
