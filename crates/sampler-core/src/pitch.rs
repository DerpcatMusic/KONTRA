//! Note-scoped pitch constraints, including expression already accepted by the queue.
use super::{
    Action, Error, Event, ExpressionId, Runtime,
    resample::{MAX_STEP, MIN_STEP},
};

/// Inherent note pitch, separate from its physical input address and live expression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NotePitch {
    /// Use the original plan's tuning for this logical key.
    Key(u8),
    /// Absolute semitone pitch on the A4=69=440 Hz scale, in [0, 128).
    /// Overrides the plan's tuning table; its integer part selects regions.
    Absolute(f64),
}

impl NotePitch {
    /// Region-selection key. Admission rejects invalid pitches before using this.
    pub fn key(self) -> u8 {
        match self {
            Self::Key(key) => key,
            Self::Absolute(pitch) => pitch as u8,
        }
    }

    pub(super) fn valid(self) -> bool {
        match self {
            Self::Key(key) => key < 128,
            Self::Absolute(pitch) => (0.0..128.0).contains(&pitch),
        }
    }

    pub(super) fn transpose(self, semitones: i8) -> Result<Self, Error> {
        let pitch = match self {
            Self::Key(key) => {
                let key = i16::from(key) + i16::from(semitones);
                if !(0..128).contains(&key) {
                    return Err(Error::InvalidInput);
                }
                Self::Key(key as u8)
            }
            Self::Absolute(pitch) => Self::Absolute(pitch + f64::from(semitones)),
        };
        if pitch.valid() {
            Ok(pitch)
        } else {
            Err(Error::InvalidInput)
        }
    }
}

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
        if self.voices.reserved != 0 {
            for (index, slot) in self.notes.slots.iter().enumerate() {
                let Some(note) = &slot.value else {
                    continue;
                };
                let phases = self.release_times[index].selection;
                if phases
                    .iter()
                    .all(|&phase| phase != crate::ReleaseStatus::Pending)
                {
                    continue;
                }
                if let Some(ratio) = proposed(note.expression) {
                    let prepared = &self.plans.get(note.plan.0).unwrap().prepared;
                    for trigger in [crate::Trigger::KeyRelease, crate::Trigger::GateRelease] {
                        if phases[trigger.release_index().unwrap()] == crate::ReleaseStatus::Pending
                        {
                            prepared.validate_release_pitch(
                                note.pitch,
                                trigger,
                                PitchRange::constant(ratio),
                                prepared.release_velocity(
                                    trigger,
                                    note.velocity,
                                    note.key_release.is_some(),
                                    self.release_times[index].velocity,
                                ),
                                prepared.pending_articulation(
                                    trigger,
                                    self.performance_state.states[self.selections[index].snapshot]
                                        .articulation,
                                ),
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
