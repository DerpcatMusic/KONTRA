use super::{Planar, ProcessorState, flush};
use crate::Error;

/// Causal stereo delay in output frames, with an explicit linear dry/wet mix.
/// Feedback rows are input L/R and columns delayed L/R. Each absolute row sum
/// must be strictly below one. Import profiles own time/mix/feedback laws.
#[derive(Clone, Copy, Debug)]
pub struct Delay {
    pub(super) frames: u32,
    feedback: [[f64; 2]; 2],
    dry: f64,
    wet: f64,
}

impl Delay {
    pub(crate) fn trace_frames(&self) -> u32 {
        if self.dry == 0. { self.frames } else { 0 }
    }
    pub(crate) fn trace_parameters(&self) -> [(&'static str, f64); 7] {
        [
            ("delay_frames", self.frames as f64),
            ("dry", self.dry),
            ("wet", self.wet),
            ("feedback_ll", self.feedback[0][0]),
            ("feedback_lr", self.feedback[0][1]),
            ("feedback_rl", self.feedback[1][0]),
            ("feedback_rr", self.feedback[1][1]),
        ]
    }
    pub fn new(frames: u32, feedback: [[f64; 2]; 2], dry: f64, wet: f64) -> Result<Self, Error> {
        if frames == 0
            || !dry.is_finite()
            || !wet.is_finite()
            || feedback
                .iter()
                .any(|row| row.iter().any(|v| !v.is_finite()) || row[0].abs() + row[1].abs() >= 1.)
        {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            frames,
            feedback,
            dry,
            wet,
        })
    }

    /// Delay `len` planar frames in place. Returns whether a nonfinite value
    /// entered the ring, which the caller contains by resetting its validity.
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        samples: &mut [[f64; 2]],
        block: &mut Planar,
        len: usize,
    ) -> bool {
        // Reset validity, not the potentially long ring. A reused voice or a
        // faulted chain must never read samples from its previous lifetime.
        let mut finite = true;
        let (mut position, mut filled) = (state.delay_position, state.delay_filled);
        let [left, right] = block;
        for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
            let input = [*l, *r];
            let delayed = if filled == self.frames {
                samples[position as usize]
            } else {
                [0.; 2]
            };
            let next: [f64; 2] = std::array::from_fn(|i| {
                input[i] + self.feedback[i][0] * delayed[0] + self.feedback[i][1] * delayed[1]
            });
            finite &= next[0].is_finite() & next[1].is_finite();
            samples[position as usize] = next.map(flush);
            filled = filled.saturating_add(1).min(self.frames);
            position = if position == self.frames - 1 {
                0
            } else {
                position + 1
            };
            (*l, *r) = (
                self.dry * input[0] + self.wet * delayed[0],
                self.dry * input[1] + self.wet * delayed[1],
            );
        }
        (state.delay_position, state.delay_filled) = (position, filled);
        !finite
    }
}
