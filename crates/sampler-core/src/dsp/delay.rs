use super::ProcessorState;
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

    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        samples: &mut [[f64; 2]],
        input: [f64; 2],
    ) -> [f64; 2] {
        // Reset validity, not the potentially long ring. A reused voice or a
        // faulted chain must never read samples from its previous lifetime.
        let delayed = if state.delay_filled == self.frames {
            samples[state.delay_position as usize]
        } else {
            [0.; 2]
        };
        let next: [f64; 2] = std::array::from_fn(|i| {
            input[i] + self.feedback[i][0] * delayed[0] + self.feedback[i][1] * delayed[1]
        });
        if next.iter().any(|v| !v.is_finite()) {
            return [f64::NAN; 2]; // Existing chain containment resets validity.
        }
        samples[state.delay_position as usize] =
            next.map(|v| if v.is_subnormal() { 0. } else { v });
        state.delay_filled = state.delay_filled.saturating_add(1).min(self.frames);
        state.delay_position = if state.delay_position == self.frames - 1 {
            0
        } else {
            state.delay_position + 1
        };
        std::array::from_fn(|i| self.dry * input[i] + self.wet * delayed[i])
    }
}
