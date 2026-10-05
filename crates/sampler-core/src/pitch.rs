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
        let mut range = PitchRange::constant(owner.rendered.ratio);
        if pending {
            for command in &self.commands {
                if let Action::Event(Event::Expression(note, value)) = command.action
                    && self.notes.get(note.0).unwrap().expression == id
                {
                    let ratio = self
                        .project_expression(owner.program, value, Some(owner))?
                        .ratio;
                    range.minimum = range.minimum.min(ratio);
                    range.maximum = range.maximum.max(ratio);
                }
            }
        }
        Ok(range)
    }

    /// Preflight the complete projected pitch, including controller modulation.
    pub(super) fn validate_expression_change(
        &self,
        id: ExpressionId,
        value: crate::Expression,
    ) -> Result<crate::RenderedExpression, Error> {
        let owner = self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
        let rendered = self.project_expression(owner.program, value, Some(owner))?;
        if rendered.ratio != owner.rendered.ratio {
            self.validate_source_pitches(|owner| (owner == id).then_some(rendered.ratio))?;
        }
        Ok(rendered)
    }

    pub(super) fn validate_source_pitches(
        &self,
        mut proposed: impl FnMut(ExpressionId) -> Option<f64>,
    ) -> Result<(), Error> {
        // Walk occupied words rather than every reserved Voice record. This is
        // bounded by the admitted voices and bitmap size, not source duration.
        for (word, &bits) in self.voice_activity.iter().enumerate() {
            let mut occupied = bits;
            while occupied != 0 {
                let index = word * 64 + occupied.trailing_zeros() as usize;
                occupied &= occupied - 1;
                let voice = self.voices.slots[index].value.as_ref().unwrap();
                let family = self.families.get(voice.family.0).unwrap();
                let owner = self.notes.get(family.note.0).unwrap().expression;
                if let Some(ratio) = proposed(owner) {
                    PitchRange::constant(ratio).apply(voice.base_step)?;
                }
            }
        }
        Ok(())
    }
}
