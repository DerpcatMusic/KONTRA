//! Musical ownership, independent of host channel reuse and render voice lifetime.
use super::{Error, Handle, Input, NoteId, Runtime, VoiceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FamilyId(pub(super) Handle);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpressionId(pub(super) Handle);

/// Child expression policy is independent of whether release follows the parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inheritance {
    Linked,
    Snapshot,
    Independent,
}

/// Canonical note expression. Gain, stereo balance and pitch affect resident PCM.
/// Integer pressure/timbre retain all 32 bits for future modulation routing.
/// Protocol decoding and member-channel assignment remain adapter responsibilities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Expression {
    pub gain: f64,
    pub pan: f64,
    pub pitch_semitones: f64,
    pub pressure: u32,
    pub timbre: u32,
}

impl Default for Expression {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            pitch_semitones: 0.0,
            pressure: 0,
            timbre: 0,
        }
    }
}

impl Expression {
    pub(super) fn valid(self) -> bool {
        self.gain.is_finite()
            && (0.0..=1.0).contains(&self.gain)
            && self.pan.is_finite()
            && (-1.0..=1.0).contains(&self.pan)
            && self.pitch_semitones.is_finite()
    }

    pub(super) fn gains(self) -> [f32; 2] {
        // Stereo balance, not an equal-power mono panner. Preserve center unity.
        [
            (self.gain * (1.0 - self.pan.max(0.0))) as f32,
            (self.gain * (1.0 + self.pan.min(0.0))) as f32,
        ]
    }
}

#[derive(Clone, Copy)]
pub(super) struct ExpressionOwner {
    pub value: Expression,
    pub rendered: super::RenderedExpression,
    pub program: Option<super::PlanId>,
    pub notes: usize,
}

#[derive(Clone, Copy)]
pub(super) struct Family {
    pub note: NoteId,
    pub voices: usize,
    pub open: bool,
}

impl Runtime {
    pub fn family_count(&self) -> usize {
        self.families.count()
    }
    pub fn expression_count(&self) -> usize {
        self.expressions.count()
    }

    pub fn expression_id(&self, note: NoteId) -> Result<ExpressionId, Error> {
        Ok(self.notes.get(note.0).ok_or(Error::StaleHandle)?.expression)
    }

    pub fn expression(&self, id: ExpressionId) -> Result<Expression, Error> {
        Ok(self.expressions.get(id.0).ok_or(Error::StaleHandle)?.value)
    }

    /// Changes the owner, including all explicitly linked children. Adapters retain
    /// this handle per admitted note; they must never retarget tails by channel alone.
    pub fn set_expression(&mut self, id: ExpressionId, value: Expression) -> Result<(), Error> {
        self.apply_due();
        self.set_expression_now(id, value)
    }

    /// Apply one controller gesture to several owners without partial updates.
    /// Due work runs first, as with other immediate mutations. Every entry must
    /// be valid and each owner may occur once. The caller bounds the slice.
    pub fn set_expressions(&mut self, changes: &[(ExpressionId, Expression)]) -> Result<(), Error> {
        self.apply_due();
        if changes.is_empty() {
            return Ok(());
        }
        // Scratch is private, allocated with the expression arena, and cleared
        // before every batch. An error never mutates an expression owner.
        self.expression_changes.fill(None);
        let mut pitch_changed = false;
        for &(id, value) in changes {
            if !value.valid() {
                return Err(Error::InvalidInput);
            }
            let owner = self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
            if self.expression_changes[id.0.index].is_some() {
                return Err(Error::InvalidInput);
            }
            let rendered = self.project_expression(owner.program, value, Some(owner))?;
            pitch_changed |= rendered.ratio != owner.rendered.ratio;
            self.expression_changes[id.0.index] = Some(rendered);
        }
        if pitch_changed {
            self.validate_source_pitches(|id| {
                self.expression_changes[id.0.index].map(|rendered| rendered.ratio)
            })?;
        }
        for &(id, value) in changes {
            let owner = self.expressions.get_mut(id.0).unwrap();
            owner.rendered = self.expression_changes[id.0.index].unwrap();
            owner.value = value;
        }
        Ok(())
    }

    pub(super) fn set_expression_now(
        &mut self,
        id: ExpressionId,
        value: Expression,
    ) -> Result<(), Error> {
        if !value.valid() {
            return Err(Error::InvalidInput);
        }
        let rendered = self.validate_expression_change(id, value)?;
        let owner = self.expressions.get_mut(id.0).unwrap();
        owner.value = value;
        owner.rendered = rendered;
        Ok(())
    }

    /// Freeze a shared note at its current expression. Capacity failure leaves its
    /// previous link intact; a uniquely owned expression needs no replacement slot.
    pub fn detach_expression(&mut self, note: NoteId) -> Result<ExpressionId, Error> {
        self.apply_due();
        let old = self.expression_id(note)?;
        let owner = *self.expressions.get(old.0).unwrap();
        if owner.notes == 1 {
            return Ok(old);
        }
        let new = ExpressionId(self.expressions.insert(ExpressionOwner {
            value: owner.value,
            rendered: owner.rendered,
            program: owner.program,
            notes: 1,
        })?);
        self.expressions.get_mut(old.0).unwrap().notes -= 1;
        self.notes.get_mut(note.0).unwrap().expression = new;
        Ok(new)
    }

    pub(super) fn drop_expression(&mut self, id: ExpressionId) {
        let owner = self.expressions.get_mut(id.0).unwrap();
        owner.notes -= 1;
        if owner.notes == 0 {
            self.expressions.remove(id.0);
        }
    }

    /// A family coordinates source admissions for one selection decision. The caller
    /// seals it with finish_family after adding all layers; an open family is retained.
    pub fn create_family(&mut self, note: NoteId) -> Result<FamilyId, Error> {
        self.apply_due();
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if !n.gate {
            return Err(Error::ClosedNote);
        }
        let count = n.families.checked_add(1).ok_or(Error::Capacity)?;
        let family = FamilyId(self.families.insert(Family {
            note,
            voices: 0,
            open: true,
        })?);
        self.notes.get_mut(note.0).unwrap().families = count;
        Ok(family)
    }

    pub fn family_note(&self, id: FamilyId) -> Result<NoteId, Error> {
        Ok(self.families.get(id.0).ok_or(Error::StaleHandle)?.note)
    }

    pub fn family_voice_count(&self, id: FamilyId) -> Result<usize, Error> {
        Ok(self.families.get(id.0).ok_or(Error::StaleHandle)?.voices)
    }

    /// Seal admissions; already admitted sources (including delayed starts) continue.
    /// Empty sealed families retire immediately. Sealing does not release a note.
    pub fn finish_family(&mut self, id: FamilyId) -> Result<(), Error> {
        self.apply_due();
        self.families.get_mut(id.0).ok_or(Error::StaleHandle)?.open = false;
        self.retire_family(id);
        Ok(())
    }

    /// Stop only this family's voices and delayed starts. Sibling families and the
    /// logical note are unaffected. This cleanup requires no queue capacity.
    pub fn stop_family(&mut self, id: FamilyId) -> Result<(), Error> {
        self.apply_due();
        self.families.get(id.0).ok_or(Error::StaleHandle)?;
        for i in 0..self.voices.slots.len() {
            if self.voices.slots[i].value.is_some_and(|v| v.family == id) {
                self.end_voice(VoiceId(self.voices.id(i)));
            }
        }
        self.commands.retain(
            |c| !matches!(c.action, super::Action::Start(v) if self.voices.get(v.0).is_none()),
        );
        // end_voice can retire an already sealed family when its last voice ends.
        if let Some(f) = self.families.get_mut(id.0) {
            f.open = false;
            self.retire_family(id);
        }
        Ok(())
    }

    pub(super) fn retire_family(&mut self, id: FamilyId) {
        if let Some(f) = self.families.get(id.0).copied()
            && !f.open
            && f.voices == 0
        {
            self.notes.get_mut(f.note.0).unwrap().families -= 1;
            self.families.remove(id.0);
        }
    }

    pub(super) fn end_voice(&mut self, id: VoiceId) {
        let v = *self.voices.get(id.0).unwrap();
        self.voices.remove(id.0);
        self.voice_activity[id.0.index / 64] &= !(1 << (id.0.index % 64));
        self.families.get_mut(v.family.0).unwrap().voices -= 1;
        self.retire_family(v.family);
    }

    /// Consume terminal notifications only after acceptance. The sink must be bounded
    /// and non-allocating on an audio thread. A rejection stops retries for this call.
    pub fn flush_ended(&mut self, mut accept: impl FnMut(Input) -> bool) {
        for i in 0..self.notes.slots.len() {
            if !self.retire_note_chain(NoteId(self.notes.id(i)), &mut accept) {
                break;
            }
        }
    }

    // Completed internal notes have no terminal sink. Reclaim them under admission
    // pressure, skipping external owners whose notifications still need acceptance.
    pub(super) fn reclaim_internal_notes(&mut self) {
        if self.notes.available() != 0 && self.expressions.available() != 0 {
            return;
        }
        for i in 0..self.notes.slots.len() {
            self.retire_note_chain(NoteId(self.notes.id(i)), &mut |_| false);
        }
    }

    fn retire_note_chain(
        &mut self,
        mut id: NoteId,
        accept: &mut impl FnMut(Input) -> bool,
    ) -> bool {
        // Each removal visits its parent once. No recursion, scratch queue or
        // repeated pool scans: O(reserved slots + retired notes).
        while let Some(n) = self.notes.get(id.0).copied() {
            if n.gate
                || (n.input.is_some() && n.key_down)
                || n.pins != 0
                || n.work != 0
                || n.families != 0
                || n.children != 0
            {
                break;
            }
            if n.input.is_some_and(|input| !accept(input)) {
                return false;
            }
            self.notes.remove(id.0);
            self.drop_expression(n.expression);
            self.plans.get_mut(n.plan.0).unwrap().notes -= 1;
            let Some(parent) = n.parent else {
                break;
            };
            self.notes.get_mut(parent.0).unwrap().children -= 1;
            id = parent;
        }
        true
    }
}
