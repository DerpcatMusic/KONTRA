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

    pub(super) fn apply_source(self, base_step: f64, wavetable: bool) -> Result<f64, Error> {
        if wavetable {
            let valid = [self.current, self.minimum, self.maximum]
                .iter()
                .all(|ratio| (base_step * ratio).is_finite() && base_step * ratio > 0.);
            return if valid {
                Ok(base_step * self.current)
            } else {
                Err(Error::InvalidInput)
            };
        }
        if !(MIN_STEP..=MAX_STEP).contains(&(base_step * self.minimum))
            || !(MIN_STEP..=MAX_STEP).contains(&(base_step * self.maximum))
        {
            return Err(Error::InvalidInput);
        }
        Ok(base_step * self.current)
    }
}

/// Canonical intrinsic base. Reference is the actual admitted Key/MTS/Attribute
/// authority; it is not derived later from a physical key or a replacement plan.
#[derive(Clone,Copy)]
pub(super) struct IntrinsicPitch {pub reference:f64,pub absolute:Option<f64>}
impl IntrinsicPitch {pub fn ratio(self)->f64{self.absolute.map_or(1.,|pitch|ratio(pitch-self.reference))}}

impl Runtime {
    /// Actual expression-lineage base frame: immutable admitted reference and
    /// optional absolute controller override. A derived child's authored interval
    /// remains in its own frozen source step, not in a reconstructed key delta.
    pub fn intrinsic_pitch_base(&self,id:ExpressionId)->Result<(f64,Option<f64>),Error>{
        let value=self.intrinsic_pitch(id)?;Ok((value.reference,value.absolute))
    }
    pub(super) fn intrinsic_origin(&self,mut id:ExpressionId)->Result<ExpressionId,Error>{
        for _ in 0..self.expressions.slots.len(){
            let owner=self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
            match owner.follows{Some(parent)=>id=parent,None=>return Ok(id)}
        }Err(Error::InvalidInput)
    }
    pub(super) fn intrinsic_pitch(&self,id:ExpressionId)->Result<IntrinsicPitch,Error>{
        let origin=self.intrinsic_origin(id)?;Ok(self.expressions.get(origin.0).ok_or(Error::StaleHandle)?.intrinsic)
    }

    pub(super) fn pitch_range(&self, id: ExpressionId, pending: bool) -> Result<PitchRange, Error> {
        let owner = self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
        if pending{return self.projected_pitch_range(id,owner.rendered.ratio);}
        Ok(PitchRange::constant(owner.rendered.ratio))
    }
    fn projected_pitch_range(&self,id:ExpressionId,current:f64)->Result<PitchRange,Error>{
        let owner=self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
        let mut range=PitchRange::constant(current);
        {
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
        self.validate_source_pitch_bases(|id|{
            let Some(ratio)=proposed(id)else{return Ok(None);};
            Ok(Some((ratio,self.intrinsic_pitch(id)?.ratio())))
        })
    }
    pub(super) fn validate_source_pitch_bases(
        &self,mut proposed:impl FnMut(ExpressionId)->Result<Option<(f64,f64)>,Error>,
    )->Result<(),Error>{
        // Includes every retained generation and delayed source already charged
        // to this Runtime's ONE pool; unchanged owners retain exact reservations.
        let mut stream_pages=self.stream_reserved;
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
                if let Some((ratio,intrinsic)) = proposed(owner)? {
                    let base_step=voice.inherent_step*if voice.pitch_tracked{intrinsic}else{1.};
                    let pending=self.projected_pitch_range(owner,ratio)?;
                    pending.apply_source(base_step, voice.cursor.wavetable().is_some())?;
                    if voice.stream_pages!=0 {
                        // Preserve already accepted future expression bounds as
                        // well as this write. No expression/base/source changes
                        // occur until the aggregate physical bound is accepted.
                        let mut pages=voice.stream_pages;
                        for ratio in [ratio,pending.minimum,pending.maximum] {
                            let step=base_step*ratio*voice.mod_pitch.1;
                            if !step.is_finite() || step<=0. {return Err(Error::InvalidInput);}
                            pages=pages.max(self.projected_voice_stream_pages(index,step)?);
                        }
                        stream_pages=stream_pages.checked_sub(voice.stream_pages)
                            .and_then(|n|n.checked_add(pages)).ok_or(Error::Capacity)?;
                    }
                }
            }
        }
        self.check_stream_projection(stream_pages)?;
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
                if let Some((ratio,intrinsic)) = proposed(note.expression)? {
                    let prepared = &self.plans.get(note.plan.0).unwrap().prepared;
                    for trigger in [crate::Trigger::KeyRelease, crate::Trigger::GateRelease] {
                        if phases[trigger.release_index().unwrap()] == crate::ReleaseStatus::Pending
                        {
                            prepared.validate_release_pitch(
                                note.pitch,
                                trigger,
                                self.projected_pitch_range(note.expression,ratio)?,
                                intrinsic,
                                prepared.release_velocity(
                                    trigger,
                                    note.velocity,
                                    note.key_release.is_some(),
                                    self.release_times[index].velocity,
                                ),
                                prepared.pending_selection(
                                    trigger,
                                    &self.performance_state.states[self.selections[index].snapshot],
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
