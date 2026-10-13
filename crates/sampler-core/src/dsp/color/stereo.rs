//! Verified two-channel Stereo Modeller laws (KONTAKT_DSP_LAWS).
use super::super::{ControlRamp, Parameter, Planar, control::PreparedParameter};

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
        self.width.valid()
            && self.pan.valid()
            && self.width.bounds().iter().all(|v| (0.0..=1.0).contains(v))
            && self.pan.bounds().iter().all(|v| (-1.0..=1.0).contains(v))
            && !matches!(self.width, Parameter::Expression { .. })
            && !matches!(self.pan, Parameter::Expression { .. })
    }
    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<super::super::ControlRange>) -> Stereo {
        Stereo {
            width: self.width.compile(bindings),
            pan: self.pan.compile(bindings),
            pseudo: self.pseudo,
            rate: rate as f32,
        }
    }
}
/// Width and balance smoothing, and the pseudo-stereo ring cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct StereoState {
    pub(super) width: f32,
    pub(super) pan: f32,
    /// Whether the controls have snapped to their first targets.
    pub(super) ready: bool,
    pub(super) cursor: u32,
    pub(super) filled: u32,
}

pub(crate) struct Stereo {
    width: PreparedParameter,
    pan: PreparedParameter,
    pseudo: bool,
    rate: f32,
}
impl Stereo {
    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 3] {
        [
            ("width", self.width),
            ("pan", self.pan),
            (
                "pseudo",
                PreparedParameter::Constant(f64::from(self.pseudo)),
            ),
        ]
    }
    pub(super) fn batches(&self) -> bool {
        !self.pseudo
    }
    pub(super) fn targets(&self, parameters: &[ControlRamp], at: u64) -> [f32; 2] {
        [
            self.width.value(parameters, at, None) as f32,
            self.pan.value(parameters, at, None) as f32,
        ]
    }
    pub(super) fn process(
        &self,
        state: &mut StereoState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        ring: &mut [[f64; 2]],
    ) {
        let (mut width, mut pan) = (state.width, state.pan);
        let mut pan_delta = 0.0;
        let [left, right] = block;
        for i in 0..len {
            let [target_width, target_pan] = self.targets(parameters, at + i as u64);
            if !state.ready {
                (width, pan, state.ready) = (target_width, target_pan, true);
            }
            let (l, r) = (left[i] as f32, right[i] as f32);
            let (l, r) = if self.pseudo {
                let delay = (self.rate * 0.01f32 * width * width * width) as usize;
                let delay = delay.min(1023);
                let cursor = state.cursor as usize;
                ring[cursor][1] = f64::from(r);
                let delayed = if delay <= state.filled as usize {
                    ring[(cursor + 1024 - delay) & 1023][1] as f32
                } else {
                    0.0
                };
                state.cursor = ((cursor + 1) & 1023) as u32;
                state.filled = (state.filled + 1).min(1023);
                (l, delayed)
            } else {
                matrix(l, r, width)
            };
            [left[i], right[i]] = balance(l, r, pan);
            [width, pan, pan_delta] =
                advance([width, pan, pan_delta], [target_width, target_pan], i, len);
        }
        (state.width, state.pan) = (width, pan);
    }
}

#[inline(always)]
pub(super) fn balance(l: f32, r: f32, pan: f32) -> [f64; 2] {
    [
        f64::from(l * (1.0 - pan.max(0.0))),
        f64::from(r * (1.0 + pan.min(0.0))),
    ]
}

#[inline(always)]
pub(super) fn advance(
    [width, mut pan, mut delta]: [f32; 3],
    [tw, tp]: [f32; 2],
    i: usize,
    len: usize,
) -> [f32; 3] {
    let width = super::super::kernels::one_pole32(width, tw, 1.0f32 / 180.0);
    // Native SIMD reuses the third delta on the fourth sample; groups restart per call.
    if i >= len / 4 * 4 || i % 4 != 3 {
        delta = (tp - pan) * f32::from_bits(0x3a11a2b4);
    }
    pan += delta;
    [width, pan, delta]
}

#[inline(always)]
pub(super) fn matrix(l: f32, r: f32, width: f32) -> (f32, f32) {
    if width >= 0.5 {
        let spread = 2.0 * width - 1.0;
        (
            (1.0 + spread) * l - spread * r,
            (1.0 + spread) * r - spread * l,
        )
    } else {
        let a = 0.5 - width;
        (l + a * (r - l), r + a * (l - r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings(width: f64, pan: f64, pseudo: bool) -> Stereo {
        StereoSettings {
            width: Parameter::Constant(width),
            pan: Parameter::Constant(pan),
            pseudo,
        }
        .compile(48000, &mut Vec::new())
    }
    #[test]
    fn settled_matrix_and_pseudo_impulse_follow_native_laws() {
        for (width, expected) in [(0.0, [1.5, 1.5]), (0.5, [1.0, 2.0]), (1.0, [0.0, 3.0])] {
            let mut block = [[0.0; super::super::super::BLOCK]; 2];
            (block[0][0], block[1][0]) = (1.0, 2.0);
            settings(width, 0.0, false).process(
                &mut StereoState::default(),
                &[],
                &mut block,
                1,
                0,
                &mut [],
            );
            assert_eq!([block[0][0], block[1][0]], expected);
        }
        let mut state = StereoState::default();
        let mut ring = [[0.0; 2]; 1024];
        let mut block = [[0.0; super::super::super::BLOCK]; 2];
        (block[0][0], block[1][0]) = (1.0, 1.0);
        settings(0.5, 0.0, true).process(&mut state, &[], &mut block, 64, 0, &mut ring);
        assert_eq!(block[0][0], 1.0);
        assert!(block[1][..60].iter().all(|v| *v == 0.0));
        assert_eq!(block[1][60], 1.0);
    }
    #[test]
    fn moving_width_and_pan_preserve_the_native_four_frame_recurrence() {
        let mut state = StereoState::default();
        let mut block = [[1.0; super::super::super::BLOCK]; 2];
        settings(0.6, -0.8, false).process(&mut state, &[], &mut block, 1, 0, &mut []);
        settings(0.9, -0.2, false).process(&mut state, &[], &mut block, 4, 1, &mut []);
        assert_eq!(state.width, 0.6066113710403442f32);
        assert_eq!(state.pan, -0.7986676692962646f32);
    }
}

#[cfg(test)]
mod frozen_kernel_tests {
    use super::*;
    fn frozen_process(
        stereo: &Stereo,
        state: &mut StereoState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        ring: &mut [[f64; 2]],
    ) {
        let (mut width, mut pan) = (state.width, state.pan);
        let mut pan_delta = 0.0;
        let [left, right] = block;
        for i in 0..len {
            let [target_width, target_pan] = stereo.targets(parameters, at + i as u64);
            if !state.ready {
                (width, pan, state.ready) = (target_width, target_pan, true);
            }
            let (l, r) = (left[i] as f32, right[i] as f32);
            let (l, r) = if stereo.pseudo {
                let delay = (stereo.rate * 0.01f32 * width * width * width) as usize;
                let delay = delay.min(1023);
                let cursor = state.cursor as usize;
                ring[cursor][1] = f64::from(r);
                let delayed = if delay <= state.filled as usize {
                    ring[(cursor + 1024 - delay) & 1023][1] as f32
                } else {
                    0.0
                };
                state.cursor = ((cursor + 1) & 1023) as u32;
                state.filled = (state.filled + 1).min(1023);
                (l, delayed)
            } else {
                matrix(l, r, width)
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
        (state.width, state.pan) = (width, pan);
    }
    #[test]
    fn shared_stereo_pseudo_matches_frozen_pcm_and_state_bits() {
        for pseudo in [false, true] {
            for len in [0, 1, 3, 4, 5, 17, super::super::super::BLOCK] {
                let (mut state, mut expected_state) =
                    (StereoState::default(), StereoState::default());
                let (mut ring, mut expected_ring) = ([[0.; 2]; 1024], [[0.; 2]; 1024]);
                for block_index in 0..32 {
                    let stereo = StereoSettings {
                        width: Parameter::Constant(if block_index < 3 { 0.1 } else { 0.9 }),
                        pan: Parameter::Constant(if block_index < 2 { -0.8 } else { 0.7 }),
                        pseudo,
                    }
                    .compile(48000, &mut Vec::new());
                    let mut actual = std::array::from_fn(|c| {
                        std::array::from_fn(|i| {
                            if block_index == 0 {
                                if i == 0 { 1. } else { 0. }
                            } else {
                                ((i + 11 * c) as f64 * 0.31).sin() * 0.1
                            }
                        })
                    });
                    let mut expected = actual;
                    stereo.process(&mut state, &[], &mut actual, len, 0, &mut ring);
                    frozen_process(
                        &stereo,
                        &mut expected_state,
                        &[],
                        &mut expected,
                        len,
                        0,
                        &mut expected_ring,
                    );
                    assert_eq!(
                        actual.map(|c| c.map(f64::to_bits)),
                        expected.map(|c| c.map(f64::to_bits))
                    );
                    assert_eq!(state, expected_state);
                    assert_eq!(
                        (state.cursor, state.filled),
                        (expected_state.cursor, expected_state.filled)
                    );
                    assert_eq!(
                        ring.map(|c| c.map(f64::to_bits)),
                        expected_ring.map(|c| c.map(f64::to_bits))
                    );
                }
            }
        }
    }
}
