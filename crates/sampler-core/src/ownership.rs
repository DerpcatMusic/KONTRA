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
    /// Gain, pan and lifetime are the child's own; per-note pitch bend,
    /// pressure and timbre follow the parent live (MPE movement carries to
    /// notes a script plays).
    Expression,
}

/// Largest note gain an expression carries: CLAP's note gain expression
/// range (0..=4, linear, +12 dB).
pub const MAX_EXPRESSION_GAIN: f64 = 4.0;

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
    /// Raw pitch-bend position, -1..=1, independent of the bend range that
    /// turns it into `pitch_semitones`; read by pitch-bend modulation routes.
    pub bend: f64,
}

impl Default for Expression {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            pitch_semitones: 0.0,
            pressure: 0,
            // Centre, so timbre-darkening laws are identity for non-MPE notes.
            timbre: 0x8000_0000,
            bend: 0.0,
        }
    }
}

impl Expression {
    pub(super) fn valid(self) -> bool {
        self.gain.is_finite()
            && (0.0..=MAX_EXPRESSION_GAIN).contains(&self.gain)
            && self.pan.is_finite()
            && (-1.0..=1.0).contains(&self.pan)
            && self.pitch_semitones.is_finite()
            && (-1.0..=1.0).contains(&self.bend)
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
    /// The expression whose pitch, pressure, timbre and bend this one copies.
    pub follows: Option<ExpressionId>,
}

#[derive(Clone, Copy)]
pub(super) struct Family {
    pub note: NoteId,
    pub voices: usize,
    pub open: bool,
    pub gate: bool,
    pub trigger: super::Trigger,
    pub siblings: super::Siblings,
    pub first_voice: Option<super::Index>,
    pub decision: Option<super::Index>,
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
        self.follow_expression();
        Ok(())
    }

    /// Rewrite every expression owner of a live note (held or releasing)
    /// admitted from `address`, or only from `key` there. Channel-wide gestures
    /// (plain MIDI bend and pressure) use this; owners shared by several notes
    /// change once. All-or-nothing like [`Self::set_expressions`] and
    /// allocation-free. Returns the number of owners changed.
    pub fn set_input_expressions(
        &mut self,
        address: super::ChannelAddress,
        key: Option<u8>,
        update: impl Fn(Expression) -> Expression,
    ) -> Result<usize, Error> {
        self.apply_due();
        self.expression_changes.fill(None);
        let mut pitch_changed = false;
        let mut changed = 0;
        for i in 0..self.notes.slots.len() {
            let Some(note) = &self.notes.slots[i].value else {
                continue;
            };
            if !note.input.is_some_and(|input| {
                input.channel_address() == address && key.is_none_or(|key| input.key == key)
            }) {
                continue;
            }
            let id = note.expression;
            if self.expression_changes[id.0.index].is_some() {
                continue;
            }
            let owner = self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
            let value = update(owner.value);
            if !value.valid() {
                return Err(Error::InvalidInput);
            }
            let rendered = self.project_expression(owner.program, value, Some(owner))?;
            pitch_changed |= rendered.ratio != owner.rendered.ratio;
            self.expression_changes[id.0.index] = Some(rendered);
            changed += 1;
        }
        if pitch_changed {
            self.validate_source_pitches(|id| {
                self.expression_changes[id.0.index].map(|rendered| rendered.ratio)
            })?;
        }
        for index in 0..self.expression_changes.len() {
            if let Some(rendered) = self.expression_changes[index] {
                // `update` is pure over the unchanged owner value.
                let owner = self.expressions.slots[index].value.as_mut().unwrap();
                owner.value = update(owner.value);
                owner.rendered = rendered;
            }
        }
        self.follow_expression();
        Ok(changed)
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
        self.follow_expression();
        Ok(())
    }

    /// Copy each follower's pitch, pressure, timbre and bend from the owner it
    /// follows. A follower whose new pitch cannot be projected keeps its last.
    pub(super) fn follow_expression(&mut self) {
        if !self.expression_followers {
            return;
        }
        for index in 0..self.expressions.slots.len() {
            let Some(owner) = self.expressions.slots[index].value else {
                continue;
            };
            let Some(from) = owner.follows.and_then(|id| self.expressions.get(id.0)) else {
                continue;
            };
            let value = Expression {
                pitch_semitones: from.value.pitch_semitones,
                pressure: from.value.pressure,
                timbre: from.value.timbre,
                bend: from.value.bend,
                ..owner.value
            };
            if value == owner.value {
                continue;
            }
            if let Ok(rendered) = self.project_expression(owner.program, value, Some(&owner)) {
                let owner = self.expressions.slots[index].value.as_mut().unwrap();
                owner.value = value;
                owner.rendered = rendered;
            }
        }
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
            follows: owner.follows,
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
        self.create_family_for(note, super::Trigger::Attack)
    }

    pub(super) fn create_family_for(
        &mut self,
        note: NoteId,
        trigger: super::Trigger,
    ) -> Result<FamilyId, Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if trigger == super::Trigger::Attack && !n.gate() {
            return Err(Error::ClosedNote);
        }
        let count = n.families.checked_add(1).ok_or(Error::Capacity)?;
        let next_sibling = n.first_family;
        let family = FamilyId(self.families.insert(Family {
            note,
            voices: 0,
            open: true,
            gate: true,
            trigger,
            siblings: super::Siblings {
                previous: None,
                next: next_sibling,
            },
            first_voice: None,
            decision: None,
        })?);
        let index = super::Index::new(family.0.index);
        if let Some(next) = next_sibling {
            self.families.at_mut(next).siblings.previous = Some(index);
        }
        let note = self.notes.get_mut(note.0).unwrap();
        note.families = count;
        note.first_family = Some(index);
        Ok(family)
    }

    pub fn family_trigger(&self, family: FamilyId) -> Result<super::Trigger, Error> {
        Ok(self
            .families
            .get(family.0)
            .ok_or(Error::StaleHandle)?
            .trigger)
    }

    /// Close this family's source gates using their own envelopes and loop exits.
    /// This does not consume its logical note's physical key or other families.
    pub fn release_family(&mut self, family: FamilyId) -> Result<(), Error> {
        self.schedule_event(self.now, super::Event::ReleaseFamily(family))
    }

    pub(super) fn release_family_now(&mut self, id: FamilyId) {
        let Some(family) = self.families.get_mut(id.0) else {
            return;
        };
        if !family.gate {
            return;
        }
        family.gate = false;
        family.open = false;
        let plan = self.notes.get(family.note.0).unwrap().plan;
        let mut voice = family.first_voice;
        while let Some(index) = voice {
            let state = self.voices.at_mut(index);
            voice = state.siblings.next;
            state.envelope.release();
            state.cursor.release();
            let g = self.plans.get_mut(plan.0).unwrap();
            g.modulation
                .release(&g.prepared.voice_modulation, index.get());
            let done = state.chain.map_or_else(
                || state.envelope.done(),
                |chain| self.plans.get(plan.0).unwrap().prepared.voice_chains[chain].done(state),
            );
            if !state.started || done {
                self.end_voice(VoiceId(self.voices.id(index.get())));
            }
        }
        self.retire_family(id);
    }

    pub fn family_note(&self, id: FamilyId) -> Result<NoteId, Error> {
        Ok(self.families.get(id.0).ok_or(Error::StaleHandle)?.note)
    }

    /// Every live logical note, input and behavior-generated, in slot order.
    pub fn live_notes(&self) -> impl Iterator<Item = NoteId> + '_ {
        (0..self.notes.slots.len())
            .filter(|&i| self.notes.slots[i].value.is_some())
            .map(|i| NoteId(self.notes.id(i)))
    }

    /// Borrow the note's currently retained families without exposing slot indices.
    pub fn note_families(
        &self,
        note: NoteId,
    ) -> Result<impl Iterator<Item = FamilyId> + '_, Error> {
        let mut next = self
            .notes
            .get(note.0)
            .ok_or(Error::StaleHandle)?
            .first_family;
        Ok(std::iter::from_fn(move || {
            let index = next?;
            next = self.families.slots[index.get()]
                .value
                .unwrap()
                .siblings
                .next;
            Some(FamilyId(self.families.id(index.get())))
        }))
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
        self.choke_family(id, 0)
    }

    /// Seal this family, cancel delayed starts and fade each sounding source from
    /// its current envelope level over at most `frames`. Existing shorter tails
    /// are unchanged. Playback continues; logical gates and sibling families are
    /// unaffected. Zero frames is a hard stop. No queue capacity is required.
    pub fn choke_family(&mut self, id: FamilyId, frames: u32) -> Result<(), Error> {
        self.schedule_event(self.now, super::Event::ChokeFamily(id, frames))
    }

    pub(super) fn choke_family_now(&mut self, id: FamilyId, frames: u32) {
        // A scheduled choke does not retain a naturally completed family. The
        // generation check makes its later execution harmless after slot reuse.
        let Some(family) = self.families.get_mut(id.0) else {
            return;
        };
        family.open = false;
        let mut voice = family.first_voice;
        while let Some(index) = voice {
            voice = self.voices.at_mut(index).siblings.next;
            self.choke_voice(index, frames);
        }
        self.retire_family(id);
        self.cancel_closed_work();
    }

    /// Fade one voice from its current level over at most `frames`; zero or an
    /// unstarted voice ends now. Existing shorter tails are unchanged.
    pub(super) fn choke_voice(&mut self, index: super::Index, frames: u32) {
        let state = self.voices.at_mut(index);
        if frames == 0 || !state.started {
            self.end_voice(VoiceId(self.voices.id(index.get())));
        } else if state.chain.is_some() {
            if state
                .tail_remaining
                .is_none_or(|remaining| frames < remaining)
            {
                let current = state.dsp_fade.map_or(1., |(total, initial)| {
                    initial * state.tail_remaining.unwrap() as f32 / total as f32
                });
                state.tail_remaining = Some(frames);
                state.dsp_fade = Some((frames, current));
            }
        } else {
            state.envelope.choke(frames);
        }
    }

    pub(super) fn retire_family(&mut self, id: FamilyId) {
        if let Some(f) = self.families.get(id.0).copied()
            && !f.open
            && f.voices == 0
        {
            debug_assert!(f.first_voice.is_none());
            if let Some(previous) = f.siblings.previous {
                self.families.at_mut(previous).siblings.next = f.siblings.next;
            } else {
                self.notes.get_mut(f.note.0).unwrap().first_family = f.siblings.next;
            }
            if let Some(next) = f.siblings.next {
                self.families.at_mut(next).siblings.previous = f.siblings.previous;
            }
            self.notes.get_mut(f.note.0).unwrap().families -= 1;
            self.families.remove(id.0);
            // ponytail: bounded command scan per retirement; index targets if queue workloads require it.
            self.commands.retain(|command| !matches!(command.action,
                super::Action::Event(super::Event::ReleaseFamily(target) | super::Event::ChokeFamily(target, _)) if target == id));
        }
    }

    pub(super) fn end_voice(&mut self, id: VoiceId) {
        let v = *self.voices.get(id.0).unwrap();
        if let Some(previous) = v.siblings.previous {
            self.voices.at_mut(previous).siblings.next = v.siblings.next;
        } else {
            self.families.get_mut(v.family.0).unwrap().first_voice = v.siblings.next;
        }
        if let Some(next) = v.siblings.next {
            self.voices.at_mut(next).siblings.previous = v.siblings.previous;
        }
        let plan = self
            .notes
            .get(self.families.get(v.family.0).unwrap().note.0)
            .unwrap()
            .plan;
        self.plans
            .get_mut(plan.0)
            .unwrap()
            .modulation
            .stop(id.0.index);
        self.stolen -= usize::from(v.stolen);
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
    pub(super) fn reclaim_internal_notes(&mut self, required: super::ReleaseReserve) {
        if self.notes.available() != 0
            && self.expressions.available() != 0
            && self.decisions.available() >= required.decisions
            && self.voices.available() >= required.voices
            && self.families.available() >= required.families
            && self.available_commands() >= required.commands
        {
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
            let quiet = !n.input_down && n.pins == 0 && n.work == 0 && n.families == 0;
            if quiet
                && n.children == 0
                && n.gate()
                && !n.key_down()
                && self.release_times[id.0.index].held
            {
                // Nothing can resume an audible release: close the held gate
                // without release triggers and retire below.
                // ponytail: a script that kept this ID to note_off it later
                // (to fire native release groups) finds it gone; keep such
                // notes while a script holds the ID if a library needs that.
                self.close_gate(id, super::ReleaseCause::Silent);
                self.cleanup_closed_notes();
                continue;
            }
            if (n.gate() && !n.retire_when_silent)
                || n.input_down
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
            if n.retire_when_silent {
                // Source-owned notes end without inventing a musical release.
                // Return unused release quotas before removing their owning note.
                debug_assert!(n.input.is_none());
                self.run_release_behavior(id, false);
                self.run_release(id, super::Trigger::KeyRelease, false);
                self.run_release(id, super::Trigger::GateRelease, false);
            }
            debug_assert!(n.first_child.is_none() && n.first_family.is_none());
            if let Some(parent) = n.parent {
                if let Some(previous) = n.siblings.previous {
                    self.notes.at_mut(previous).siblings.next = n.siblings.next;
                } else {
                    self.notes.get_mut(parent.0).unwrap().first_child = n.siblings.next;
                }
                if let Some(next) = n.siblings.next {
                    self.notes.at_mut(next).siblings.previous = n.siblings.previous;
                }
            }
            let mut decision = n.first_decision;
            while let Some(index) = decision {
                decision = self.decisions.slots[index.get()].value.unwrap().next;
                self.decisions.remove(self.decisions.id(index.get()));
            }
            self.notes.remove(id.0);
            self.performance_state
                .release(self.selections[id.0.index].snapshot);
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
