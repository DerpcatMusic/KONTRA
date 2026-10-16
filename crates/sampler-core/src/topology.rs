//! Explicit per-activation shared processing ownership. Definition, source,
//! selection family and physical storage identities are deliberately distinct.
//! The first executable schedule has stereo signal edges; no source channel
//! truncation or implicit multichannel projection is introduced here.
use crate::{
    Envelope, EnvelopeState, Error, FamilyId, Frame, Handle, NoteId, PlanId, Prepared, Runtime,
    VoiceId,
};

const MAX_MEMBERS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessingOwnerId(pub(super) Handle);

#[derive(Clone, Copy, Debug)]
pub struct ProcessingOwnerSnapshot {
    pub id: ProcessingOwnerId,
    pub note: NoteId,
    pub plan: PlanId,
    pub template: usize,
    pub sources: usize,
    pub envelope: f32,
    pub tail_remaining: Option<u32>,
}

pub(super) struct PreparedScope {
    pub chain: Option<usize>,
    pub sum: sampler_ir::ProcessingSumLaw,
    pub envelope: Envelope,
    pub has_amplitude: bool,
    pub bus: Option<usize>,
    pub members: Box<[usize]>,
}

#[derive(Clone, Copy)]
pub(super) struct ProcessingOwner {
    pub note: NoteId,
    pub plan: PlanId,
    pub template: usize,
    activation: u64,
    pub members: [Option<VoiceId>; MAX_MEMBERS],
    envelope: EnvelopeState,
    tail_remaining: Option<u32>,
    pub(super) failed: bool,
    pub(super) published: bool,
}

impl Prepared {
    pub fn processing_scope_count(&self) -> usize {
        self.processing_scopes.len()
    }
}

impl Runtime {
    /// Fixed owner capacity is separate from later physical-source growth.
    /// Tails occupy owners after source slots become reusable.
    pub fn processing_owner_capacity(&self) -> usize {
        self.processing_owners.slots.len()
    }
    pub fn processing_owner_count(&self) -> usize {
        self.processing_owners.count()
    }
    pub fn visit_processing_owners(&self, mut visit: impl FnMut(ProcessingOwnerSnapshot)) {
        for (index, slot) in self.processing_owners.slots.iter().enumerate() {
            if let Some(owner) = slot.value.as_ref().filter(|o| o.published) {
                visit(ProcessingOwnerSnapshot {
                    id: ProcessingOwnerId(self.processing_owners.id(index)),
                    note: owner.note,
                    plan: owner.plan,
                    template: owner.template,
                    sources: owner.members.iter().flatten().count(),
                    envelope: owner.envelope.current(),
                    tail_remaining: owner.tail_remaining,
                });
            }
        }
    }

    pub(super) fn begin_processing_activation(&mut self) -> Option<u64> {
        self.processing_activation = self.processing_activation.checked_add(1)?;
        Some(self.processing_activation)
    }

    pub(super) fn processing_member_admission(
        &mut self,
        note: NoteId,
        activation: Option<u64>,
        region: usize,
    ) -> Result<Option<(ProcessingOwnerId, usize, bool)>, Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let plan = n.plan;
        let prepared = &self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared;
        let Some((template, ordinal)) = prepared.region_processing.get(region).copied().flatten()
        else {
            return Ok(None);
        };
        let activation = activation.ok_or(Error::ArithmeticOverflow)?;
        let scope = &prepared.processing_scopes[template];
        let gate = scope.has_amplitude && !prepared.region_has_amplitude(region);
        let existing = self.processing_owners.slots.iter().position(|slot| {
            slot.value.as_ref().is_some_and(|owner| {
                owner.note == note && owner.template == template && owner.activation == activation
            })
        });
        if let Some(index) = existing {
            let owner = self.processing_owners.slots[index]
                .value
                .as_ref()
                .ok_or(Error::StaleHandle)?;
            if owner.failed || owner.members[ordinal].is_some() {
                return Err(Error::Capacity);
            }
            return Ok(Some((
                ProcessingOwnerId(self.processing_owners.id(index)),
                ordinal,
                gate,
            )));
        }
        if self.voices.available() == 0 {
            return Err(Error::Capacity);
        }
        let next = n.processing_owners.checked_add(1).ok_or(Error::Capacity)?;
        let id = ProcessingOwnerId(self.processing_owners.insert(ProcessingOwner {
            note,
            plan,
            template,
            activation,
            members: [None; MAX_MEMBERS],
            envelope: EnvelopeState::new(scope.envelope),
            tail_remaining: None,
            failed: false,
            published: false,
        })?);
        self.notes
            .get_mut(note.0)
            .ok_or(Error::StaleHandle)?
            .processing_owners = next;
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        generation
            .dsp
            .owner_cells
            .claim(id.0.index)
            .fill(crate::dsp::ProcessorState::default());
        generation.dsp.owner_delays.claim(id.0.index).fill([0.; 2]);
        Ok(Some((id, ordinal, gate)))
    }

    pub(super) fn attach_processing_member(
        &mut self,
        owner: ProcessingOwnerId,
        ordinal: usize,
        voice: VoiceId,
        gate: bool,
    ) {
        if let Some(state) = self.processing_owners.get_mut(owner.0) {
            state.members[ordinal] = Some(voice);
        }
        if let Some(source) = self.voices.get_mut(voice.0) {
            source.processing = Some((owner, ordinal));
            source.processing_gate = gate;
        }
    }
    pub(super) fn fail_processing_owner(&mut self, owner: ProcessingOwnerId) {
        if let Some(state) = self.processing_owners.get_mut(owner.0) {
            state.failed = true;
        }
    }
    pub(super) fn finish_processing_activation(&mut self, activation: Option<u64>) {
        let Some(activation) = activation else {
            return;
        };
        for index in 0..self.processing_owners.slots.len() {
            let Some(owner) = self.processing_owners.slots[index].value else {
                continue;
            };
            if owner.activation != activation || owner.published {
                continue;
            }
            let id = ProcessingOwnerId(self.processing_owners.id(index));
            if owner.failed || owner.members.iter().all(Option::is_none) {
                if let Some(log) = self.selection_log.as_mut().and_then(|log| log.last_mut()) {
                    if let Some(plan) = self.plans.get(owner.plan.0) {
                        for selected in &mut log.candidates {
                            if plan.prepared.processing_scopes[owner.template]
                                .members
                                .contains(&selected.region)
                            {
                                selected.started = None;
                            }
                        }
                    }
                }
                self.end_processing_owner(id);
            } else if let Some(owner) = self.processing_owners.get_mut(id.0) {
                owner.published = true;
            }
        }
    }

    fn owner_has_family(&self, owner: &ProcessingOwner, family: FamilyId) -> bool {
        owner
            .members
            .iter()
            .flatten()
            .any(|id| self.voices.get(id.0).is_some_and(|v| v.family == family))
    }
    pub(super) fn release_processing_note(&mut self, note: NoteId) {
        for slot in &mut self.processing_owners.slots {
            if let Some(owner) = slot.value.as_mut()
                && owner.note == note
            {
                let has = self
                    .plans
                    .get(owner.plan.0)
                    .is_some_and(|p| p.prepared.processing_scopes[owner.template].has_amplitude);
                if has {
                    owner.envelope.release();
                }
            }
        }
    }
    pub(super) fn stop_processing_owners(&mut self) {
        for index in 0..self.processing_owners.slots.len() {
            if self.processing_owners.slots[index].value.is_some() {
                self.end_processing_owner(ProcessingOwnerId(self.processing_owners.id(index)));
            }
        }
    }
    pub(super) fn release_processing_family(&mut self, family: FamilyId) {
        for index in 0..self.processing_owners.slots.len() {
            let Some(owner) = self.processing_owners.slots[index].value else {
                continue;
            };
            if !self.owner_has_family(&owner, family) {
                continue;
            }
            let has = self
                .plans
                .get(owner.plan.0)
                .is_some_and(|p| p.prepared.processing_scopes[owner.template].has_amplitude);
            if has && let Some(owner) = self.processing_owners.slots[index].value.as_mut() {
                owner.envelope.release();
            }
        }
    }
    pub(super) fn choke_processing_family(&mut self, family: FamilyId, frames: u32) {
        for index in 0..self.processing_owners.slots.len() {
            let Some(owner) = self.processing_owners.slots[index].value else {
                continue;
            };
            if !self.owner_has_family(&owner, family) {
                continue;
            }
            if frames == 0 {
                self.end_processing_owner(ProcessingOwnerId(self.processing_owners.id(index)));
            } else if let Some(owner) = self.processing_owners.slots[index].value.as_mut() {
                owner.envelope.choke(frames);
            }
        }
    }
    fn end_processing_owner(&mut self, id: ProcessingOwnerId) {
        let Some(owner) = self.processing_owners.get(id.0).copied() else {
            return;
        };
        for source in owner.members.into_iter().flatten() {
            if self.voices.get(source.0).is_some() {
                self.end_voice(source);
            }
        }
        self.processing_owners.remove(id.0);
        if let Some(note) = self.notes.get_mut(owner.note.0) {
            note.processing_owners -= 1;
        }
    }

    pub(super) fn render_processing_owners(&mut self, output: &mut [Frame], at: u64) {
        // Existing render_segment already splits at CELL and events, so this
        // task runs <=BLOCK and parameter ramps use the identical absolute clock.
        for index in 0..self.processing_owners.slots.len() {
            let Some(before) = self.processing_owners.slots[index].value else {
                continue;
            };
            if !before.published {
                continue;
            }
            let id = ProcessingOwnerId(self.processing_owners.id(index));
            let Some(law) = self
                .plans
                .get(before.plan.0)
                .map(|p| p.prepared.processing_scopes[before.template].sum)
            else {
                continue;
            };
            let mut sum = [[0.; 2]; crate::dsp::BLOCK];
            let source_len = if before.tail_remaining.is_some() {
                0
            } else {
                output.len().min(before.envelope.remaining())
            };
            let mut produced = 0usize;
            // Member order, not physical slot order. No note-to-note state sharing.
            for source in before.members.into_iter().flatten() {
                if self.voices.get(source.0).is_none() {
                    continue;
                }
                let mut part = [[0.; 2]; crate::dsp::BLOCK];
                if source_len != 0 {
                    produced = produced.max(
                        self.render_source_voice::<false>(
                            source.0.index,
                            &mut part[..source_len],
                            at,
                            true,
                        )
                        .unwrap_or(0),
                    );
                }
                for (to, from) in sum.iter_mut().zip(part).take(output.len()) {
                    match law {
                        sampler_ir::ProcessingSumLaw::OrderedF32 => {
                            to[0] += from[0];
                            to[1] += from[1];
                        }
                    }
                }
            }
            // Stop source reads exactly at the owning amplitude's termination;
            // shared post-amplitude state may still render its zero-input tail.
            if source_len < output.len() && before.envelope.remaining() == source_len {
                for source in before.members.into_iter().flatten() {
                    if self.voices.get(source.0).is_some() {
                        self.end_voice(source);
                    }
                }
            }
            let Some(mut owner) = self.processing_owners.get(id.0).copied() else {
                continue;
            };
            let Some(note) = self.notes.get(owner.note.0) else {
                continue;
            };
            let Some(expression) = self.expressions.get(note.expression.0) else {
                continue;
            };
            let generation = match self.plans.get_mut(owner.plan.0) {
                Some(p) => p,
                None => continue,
            };
            let scope = &generation.prepared.processing_scopes[owner.template];
            let chain = scope
                .chain
                .map(|c| generation.prepared.voice_chains.view(c));
            let mut tail_advance = 0usize;
            let len = if let Some(tail) = owner.tail_remaining {
                tail_advance = output.len().min(tail as usize);
                tail_advance
            } else if owner.members.iter().all(Option::is_none) {
                let tail = chain.map_or(0, |c| c.owner_tail_frames());
                let prefix = produced.min(output.len());
                let len = output.len().min(prefix.saturating_add(tail as usize));
                tail_advance = len - prefix;
                owner.tail_remaining = Some(tail);
                len
            } else {
                output.len()
            };
            let mut cells = generation.dsp.owner_cells.claim(index);
            let mut delay = generation.dsp.owner_delays.claim(index);
            let filters = &mut generation.dsp.filters.as_mut_slice()[0];
            filters.clear_source_modulation();
            let mut fault = false;
            if let Some(chain) = chain {
                for &bus in &chain.tap_buses {
                    generation.dsp.feeds[bus].samples = [[0.; crate::dsp::BLOCK]; 2];
                }
                let context = crate::dsp::RenderContext {
                    controls: None,
                    amplifier: None,
                    expression: expression.rendered.gains,
                    delay: &mut delay[..chain.delay_frames],
                    parameters: &generation.dsp.parameters,
                    filters: crate::dsp::svf::FilterContext {
                        tempo: self.tempo,
                        bank: filters,
                        expression: Some((note.expression, expression.value)),
                        spaces: &mut [],
                    },
                    at,
                    feeds: &mut generation.dsp.feeds,
                    trace: None,
                    stretch: None,
                };
                fault = chain.render_owner(
                    &mut owner.envelope,
                    &mut sum[..len],
                    &mut cells[..chain.stages()],
                    context,
                );
                if !fault {
                    for &bus in &chain.tap_buses {
                        let feed = &generation.dsp.feeds[bus].samples;
                        let target = generation.dsp.buses.input(bus, len);
                        for (i, frame) in target.iter_mut().enumerate() {
                            for c in 0..2 {
                                frame[c] +=
                                    (feed[c][i] * f64::from(expression.rendered.gains[c])) as f32;
                            }
                        }
                        generation.dsp.buses.fed(bus, len);
                    }
                }
            } else {
                for frame in &mut sum[..len] {
                    let level = owner
                        .envelope
                        .constant_level()
                        .unwrap_or_else(|| owner.envelope.next());
                    for c in 0..2 {
                        frame[c] *= level * expression.rendered.gains[c];
                        fault |= !frame[c].is_finite();
                    }
                }
            }
            if fault {
                sum.fill([0.; 2]);
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
            }
            let target = match scope.bus {
                Some(bus) => generation.dsp.buses.input(bus, len),
                None => &mut output[..len],
            };
            for (frame, from) in target.iter_mut().zip(sum) {
                for c in 0..2 {
                    frame[c] += from[c];
                }
            }
            if let Some(bus) = scope.bus {
                generation.dsp.buses.fed(bus, len);
            }
            drop((cells, delay));
            if let Some(tail) = owner.tail_remaining {
                owner.tail_remaining = Some(tail.saturating_sub(tail_advance as u32));
            }
            let done = owner.tail_remaining == Some(0);
            if let Some(state) = self.processing_owners.get_mut(id.0) {
                *state = owner;
            }
            if done {
                self.end_processing_owner(id);
            }
        }
    }
}
