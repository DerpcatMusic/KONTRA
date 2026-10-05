//! Note-scoped pitch constraints, including expression already accepted by the queue.
use super::{
    Action, Error, Event, ExpressionId, Runtime,
    resample::{MAX_STEP, MIN_STEP},
};

pub(super) fn ratio(semitones: f64) -> f64 {
    if semitones == 0.0 {
        1.0
    } else {
        (semitones / 12.0).exp2()
    }
}

#[derive(Clone, Copy)]
pub(super) struct PitchRange {
    current: f64,
    minimum: f64,
    maximum: f64,
}

impl PitchRange {
    pub(super) fn constant(ratio: f64) -> Self {
        Self {
            current: ratio,
            minimum: ratio,
            maximum: ratio,
        }
    }

    pub(super) fn apply(self, base_step: f64) -> Result<f64, Error> {
        if !(MIN_STEP..=MAX_STEP).contains(&(base_step * self.minimum))
            || !(MIN_STEP..=MAX_STEP).contains(&(base_step * self.maximum))
        {
            return Err(Error::InvalidInput);
        }
        Ok(base_step * self.current)
    }
}

impl Runtime {
    pub(super) fn pitch_range(&self, id: ExpressionId, pending: bool) -> Result<PitchRange, Error> {
        let owner = self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
        if !pending {
            return Ok(PitchRange::constant(owner.pitch_ratio));
        }
        let pitch = owner.value.pitch_semitones;
        let (mut low, mut high) = (pitch, pitch);
        for command in &self.commands {
            if let Action::Event(Event::Expression(note, value)) = command.action
                && self.notes.get(note.0).unwrap().expression == id
            {
                low = low.min(value.pitch_semitones);
                high = high.max(value.pitch_semitones);
            }
        }
        Ok(PitchRange {
            current: owner.pitch_ratio,
            minimum: if low == pitch {
                owner.pitch_ratio
            } else {
                ratio(low)
            },
            maximum: if high == pitch {
                owner.pitch_ratio
            } else {
                ratio(high)
            },
        })
    }

    /// Both scheduling and immediate application validate all admitted sources,
    /// including delayed starts. Later source admission checks the pending queue,
    /// so an accepted expression cannot become invalid before its execution.
    pub(super) fn validate_pitch_change(
        &self,
        id: ExpressionId,
        semitones: f64,
    ) -> Result<f64, Error> {
        let owner = self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
        if owner.value.pitch_semitones == semitones {
            return Ok(owner.pitch_ratio);
        }
        let pitch_ratio = ratio(semitones);
        let range = PitchRange::constant(pitch_ratio);
        // Walk occupied words rather than every reserved Voice record. This is
        // bounded by the admitted voices and bitmap size, not source duration.
        for (word, &bits) in self.voice_activity.iter().enumerate() {
            let mut occupied = bits;
            while occupied != 0 {
                let index = word * 64 + occupied.trailing_zeros() as usize;
                occupied &= occupied - 1;
                let voice = self.voices.slots[index].value.as_ref().unwrap();
                let family = self.families.get(voice.family.0).unwrap();
                if self.notes.get(family.note.0).unwrap().expression == id {
                    range.apply(voice.base_step)?;
                }
            }
        }
        Ok(pitch_ratio)
    }
}
