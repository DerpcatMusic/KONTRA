//! Canonical typed receiver ownership. Explicit offaudio binding moves actual
//! state into this Runtime; adapters retain a handle, never a mutable mirror.
pub(crate) mod external;
pub(crate) mod task;
pub(crate) mod event_value;
pub use event_value::{EventValueId,EventPostId,EventValueSnapshot,EventPostOutcome,EventValueMemory};
pub use task::{SourceTaskFamily,SourceTaskHostContext,SourceTaskReason,SourceTaskCreator,NonMusicalTaskOrigin,PreparedGlobalTaskOrigin,PreparedGlobalSourceTask,PreparedSourceTaskBinding,SourceTaskEvent};
pub(crate) mod hierarchy;
pub use external::{ExternalInputId,ExternalTurn,ExternalEvent,ExternalOutput,ExternalOutputResult,ExternalTarget,ExternalAbortReport
};
pub(crate) mod
components;
mod per_note;
mod state;
mod types;
pub(crate) mod channel_event;
pub use channel_event::{ProgramBankIntent,ProgramSelection,ProgramPlaybackCapability};

pub use state::{OwnedReceiver, TerminalRelease, Zone};
pub use types::{
    Applied, ApplyError, Attribute, ChannelVoice, Controllers, Message, Value, Version,
    ReceiverWireEpoch,RawAdmissionId,RawTerminalId,RawDecisionPhase,RawRejection,RawAdmission,RawAttackIntent,CapturedLogicalRawAttack,RawTerminalClaim,RawTerminalOutcome,WireAdmissionStatus,
};

pub const fn is_midi2_legacy_control(index: u8) -> bool {
    matches!(index, 0 | 6 | 32 | 38 | 98..=101)
}

pub use per_note::{KeyState, PerNoteState, Snapshot};

use crate::{Arena, Error, Handle, Runtime};

/// Actual Runtime arena generation, not a port/group-derived identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiverId(pub(crate) Handle);

pub(crate) struct ReceiverOwner {
    state: OwnedReceiver,
    /// External terminal delivery owns a real callback reference until return.
    callback_references: usize,
    event_value_references:usize,
    terminal_sequence: u64,
    terminal_delivery: Option<(u64, state::QueuedTerminal,bool)>,
}

/// Actual generational receipt lease. Completing a different/reused sequence
/// cannot discharge a newer delivery on the same receiver.
#[derive(Clone, Copy, Debug)]
pub struct ReceiverTerminalDelivery {
    pub release: TerminalRelease,
    receiver: ReceiverId,
    sequence: u64,
}

pub(crate) fn allocation_bytes(
    expressions: usize,
    receivers: usize,
    changes: usize,
    policies: usize,
) -> Result<usize, Error> {
    let rows = [
        (
            expressions,
            std::mem::size_of::<crate::Slot<crate::ExpressionOwner>>(),
        ),
        (receivers, std::mem::size_of::<crate::Slot<ReceiverOwner>>()),
        (expressions.div_ceil(64), std::mem::size_of::<u64>()),
        (receivers.div_ceil(64), std::mem::size_of::<u64>()),
        (
            changes,
            std::mem::size_of::<(crate::ExpressionId, crate::Expression)>(),
        ),
        (
            expressions,
            std::mem::size_of::<Option<components::ReceiverComponents>>(),
        ),
        (
            policies,
            std::mem::size_of::<(ReceiverId, components::GestureProposal)>(),
        ),
    ];
    rows.into_iter().try_fold(0usize, |total, (count, size)| {
        total
            .checked_add(count.checked_mul(size).ok_or(Error::Capacity)?)
            .ok_or(Error::Capacity)
    })
}

impl Runtime {
    /// Explicit offaudio minimum registry reserve. Existing owners, notes or
    /// callbacks make growth busy. Smaller requests retain existing storage and
    /// generation history; no apply/render path grows or recreates the arena.
    pub fn with_receiver_capacity(mut self, capacity: usize) -> Result<Self, Error> {
        if capacity == 0 {
            return Err(Error::InvalidInput);
        }
        if self.receivers.count() != 0 || self.notes.count() != 0 || self.behaviors.count() != 0 {
            return Err(Error::Busy);
        }
        if capacity <= self.receivers.slots.len() {
            return Ok(self);
        }
        let required = allocation_bytes(
            self.expressions.slots.len(),
            capacity,
            self.receiver_changes.capacity(),
            capacity,
        )?;
        let old = std::mem::size_of::<crate::Slot<ReceiverOwner>>()
            .checked_mul(self.receivers.slots.len())
            .and_then(|bytes| {
                bytes.checked_add(
                    std::mem::size_of::<(ReceiverId, components::GestureProposal)>()
                        .checked_mul(self.receiver_policy_changes.capacity())?,
                )
            })
            .and_then(|bytes| {
                bytes
                    .checked_add(std::mem::size_of::<u64>().checked_mul(self.receivers.free.len())?)
            })
            .ok_or(Error::Capacity)?;
        required.checked_add(old).ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<crate::Slot<ReceiverOwner>>(capacity)
            .map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<u64>(capacity.div_ceil(64)).map_err(|_| Error::Capacity)?;
        let mut policies = Vec::new();
        policies
            .try_reserve_exact(capacity)
            .map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<(ReceiverId, GestureProposal)>(policies.capacity())
            .map_err(|_| Error::Capacity)?;
        let (mut slots, mut free) = Arena::blank(capacity);
        self.receivers.grow(&mut slots, &mut free);
        self.receiver_policy_changes = policies;
        // `slots`/`free` now own only old storage and are freed offaudio. Every
        // old slot generation/quarantine bit was transferred at its real index.
        Ok(self)
    }

    /// Consume an already prepared receiver into its actual canonical owner.
    /// No source state is cloned and no callback can observe an absent Slot.
    pub fn bind_receiver(&mut self, state: OwnedReceiver) -> Result<ReceiverId, Error> {
        if state.runtime != self.id() {
            return Err(Error::StaleHandle);
        }
        if state.registration.is_some()
            || !state.bindings.is_empty()
            || state.wire_fenced
            || !state.physical.is_empty()
            || !state.terminal_releases.is_empty()
        {
            return Err(Error::Busy);
        }
        if self.publishing_receiver || self.dispatching_behavior {
            return Err(Error::Busy);
        }
        if self
            .receivers
            .slots
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|owner| state.overlaps(&owner.state))
        {
            return Err(Error::InvalidInput);
        }
        let id = ReceiverId(self.receivers.insert(ReceiverOwner {
            state,
            callback_references: 0,
            event_value_references:0,
            terminal_sequence: 0,
            terminal_delivery: None,
        })?);
        self.receivers
            .get_mut(id.0)
            .ok_or(Error::StaleHandle)?
            .state
            .registration = Some(id);
        Ok(id)
    }

    /// Read actual current policy/state. Queries never read a facade cache.
    pub fn receiver(&self, id: ReceiverId) -> Result<&OwnedReceiver, Error> {
        Ok(&self.receivers.get(id.0).ok_or(Error::StaleHandle)?.state)
    }

    /// Offaudio transfer back to the unbound facade. A completed-but-unflushed
    /// captured callback still owns its real context and therefore remains busy.
    fn receiver_unbind_busy(&self,id:ReceiverId)->Result<bool,Error>{
        let owner=self.receivers.get(id.0).ok_or(Error::StaleHandle)?;
        Ok(self.publishing_receiver
            || owner.callback_references != 0
            || owner.event_value_references != 0
            || owner.state.wire_fenced
            || !owner.state.physical.is_empty()
            || !owner.state.terminal_releases.is_empty()
            || !owner.state.bindings.is_empty()
            || self.expressions.slots.iter().any(|slot| {
                slot.value.as_ref().is_some_and(|owner| {
                    owner
                        .receiver
                        .is_some_and(|component| component.receiver == id)
                })
            })
            || self.behaviors.slots.iter().any(|slot| {
                slot.value
                    .as_ref()
                    .is_some_and(|callback| callback.context.receiver() == Some(id))
            }))
    }
    pub fn unbind_receiver(&mut self,id:ReceiverId)->Result<OwnedReceiver,Error>{
        if self.receiver_unbind_busy(id)?{return Err(Error::Busy);}
        let mut owner = self.receivers.take(id.0).ok_or(Error::StaleHandle)?;
        owner.state.registration = None;
        Ok(owner.state)
    }
}

impl Runtime {
    fn prepare_receiver_gesture(
        &mut self,
        id: ReceiverId,
        domain: state::ChannelDomain,
        channel: u8,
        gesture: components::Gesture,
        value: Value,
        allowed: [u16; 3],
    ) -> Result<(components::GestureProposal, bool), Error> {
        self.receiver_changes.clear();
        self.receiver_component_changes.fill(None);
        let mut proposal = self
            .receiver(id)?
            .prepare_gesture(domain, channel, gesture, value)?;
        let state = self.receiver(id)?;
        for target in ChannelDomain::ALL {
            for at in 0..16 {
                if allowed[target.index()] & (1 << at) == 0 {
                    proposal.managers[target.index()][at] = state.managers[target.index()][at];
                    proposal.controls[target.index()][at] = state.wire[target.index()].controls[at];
                }
            }
            if proposal.channels[target.index()]
                .is_some_and(|channel| allowed[target.index()] & (1 << channel) == 0)
            {
                proposal.bends[target.index()] = None;
            }
        }
        let state = &self.receivers.get(id.0).ok_or(Error::StaleHandle)?.state;
        for index in 0..self.expressions.slots.len() {
            let Some(owner) = self.expressions.slots[index].value else {
                continue;
            };
            let Some(component) = owner.receiver.filter(|component| component.receiver == id)
            else {
                continue;
            };
            if let Some((value, next)) =
                state.project_component(component, owner.value, proposal, allowed)
            {
                // At most one row per actual expression slot; capacity was
                // prepared for the complete arena, including detached owners.
                if self.receiver_changes.len() == self.receiver_changes.capacity() {
                    return Err(Error::Capacity);
                }
                self.receiver_changes
                    .push((crate::ExpressionId(self.expressions.id(index)), value));
                self.receiver_component_changes[index] = Some(next);
            }
        }
        let pitch_changed = self.prepare_receiver_expression_changes()?;
        Ok((proposal, pitch_changed))
    }

    fn commit_receiver_gesture(
        &mut self,
        id: ReceiverId,
        proposal: components::GestureProposal,
        pitch_changed: bool,
    ) -> Result<(), Error> {
        self.commit_receiver_expression_changes(pitch_changed)?;
        let owner = self.receivers.get_mut(id.0).ok_or(Error::StaleHandle)?;
        for (wire, controls) in owner.state.wire.iter_mut().zip(proposal.controls) {
            wire.controls = controls;
        }
        owner.state.managers = proposal.managers;
        for domain in state::ChannelDomain::ALL {
            if let Some((channel, value)) = proposal.bends[domain.index()] {
                owner.state.wire[domain.index()].bends[usize::from(channel)] = value;
            }
        }
        Ok(())
    }
}

use crate::{
    ChannelAddress, ChannelScope, Expression, ExpressionId, Input, NoteId, NotePitch, Protocol,
};
use components::{
    CapturedReceiver, Gesture, GestureProposal, ReceiverActionKind, ReceiverComponents,
    ReceiverProvenance,
};
use state::{ChannelDomain, Controls, PhysicalKind, PhysicalPair,PhysicalEventContext,QueuedTerminal
};

impl Runtime {
    pub(crate) fn receiver_host_epochs(
        &self,
        id: ReceiverId,
        performance: usize,
    ) -> Result<Option<[u64; 16]>, Error> {
        let state = self.receiver(id)?;
        Ok(state.host_projection.then(|| {
            self.performance_state.epochs(
                performance,
                ChannelScope {
                    protocol: Protocol::Native,
                    port: state.port,
                    group: state.group,
                    channels: u16::MAX,
                },
            )
        }))
    }
    fn receiver_state_mut(&mut self, id: ReceiverId) -> Result<&mut OwnedReceiver, Error> {
        Ok(&mut self
            .receivers
            .get_mut(id.0)
            .ok_or(Error::StaleHandle)?
            .state)
    }

    /// The real owner remains queryable throughout this transaction. Only Ready
    /// execution/due adoption is deferred; no arena state is borrowed across an
    /// authored callback and no facade temporarily owns canonical state.
    fn publish_receiver_transaction<T>(
        &mut self,
        commit: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let previous = self.publishing_receiver;
        self.publishing_receiver = true;
        let result = commit(self);
        self.publishing_receiver = previous;
        if !previous {
            self.drain_behavior();
        }
        result
    }

    fn prepare_receiver_projection(
        &mut self,
        id: ReceiverId,
        proposal: GestureProposal,
        allowed: [u16; 3],
    ) -> Result<bool, Error> {
        self.receiver_changes.clear();
        self.receiver_component_changes.fill(None);
        for index in 0..self.expressions.slots.len() {
            let Some(owner) = self.expressions.slots[index].value else {
                continue;
            };
            let Some(component) = owner.receiver.filter(|component| component.receiver == id)
            else {
                continue;
            };
            let next =
                self.receiver(id)?
                    .project_component(component, owner.value, proposal, allowed);
            if let Some((value, component)) = next {
                if self.receiver_changes.len() == self.receiver_changes.capacity() {
                    return Err(Error::Capacity);
                }
                self.receiver_changes
                    .push((ExpressionId(self.expressions.id(index)), value));
                self.receiver_component_changes[index] = Some(component);
            }
        }
        self.prepare_receiver_expression_changes()
    }

    fn receiver_allowed(
        &self,
        id: ReceiverId,
        primary: ChannelScope,
        secondary: Option<ChannelScope>,
    ) -> Result<[u16; 3], Error> {
        let state = self.receiver(id)?;
        let mut allowed = [0; 3];
        for scope in [Some(primary), secondary].into_iter().flatten() {
            if scope.port != state.port || scope.group != state.group {
                return Err(Error::InvalidInput);
            }
            let domain = OwnedReceiver::domain(scope.protocol)?;
            allowed[domain.index()] |= scope.channels;
        }
        Ok(allowed)
    }

    fn dispatch_receiver_controller(
        &mut self,
        id: ReceiverId,
        input: Input,
        source: Input,
        number: u8,
        value: Value,
        kind: ReceiverActionKind,
    ) -> Result<Applied, Error> {
        let state = self.receiver(id)?;
        let domain = OwnedReceiver::domain(input.protocol)?;
        let performance = self.performance_index(state.performance)?;
        let channels = state.controller_channels(domain, input.channel);
        let origin = input.channel_address();
        let scope = ChannelScope {
            protocol: origin.protocol,
            port: origin.port,
            group: origin.group,
            channels,
        };
        let native_per_note = matches!(number, 128 | 129)
            || matches!(kind, ReceiverActionKind::TimbreBridge)
            || number == 74 && state.zone_domain(domain);
        let secondary = if number == 120 {
            None
        } else {
            state.host_targets(input.channel, native_per_note)
        };
        let event = crate::controller_event::ControllerEvent {
            original_frame:self.now,
            performance,
            origin,
            channels,
            epochs: self.performance_state.epochs(performance, scope),
            secondary: secondary.map(|scope| crate::controller_event::CapturedControllerTarget {
                scope,
                epochs: self.performance_state.epochs(performance, scope),
            }),
            receiver: Some(CapturedReceiver {
                receiver: id,
                kind,
                provenance: ReceiverProvenance::Wire(value),
                source: source.channel_address(),
                policy_epochs: self.performance_state.epochs(performance, scope),
                host_epochs: state.host_projection.then(|| {
                    self.performance_state.epochs(
                        performance,
                        ChannelScope {
                            protocol: Protocol::Native,
                            port: state.port,
                            group: state.group,
                            channels: u16::MAX,
                        },
                    )
                }),
            }),
            number,
            poly_key: None,
            poly_recipients: None,
            value: value.full_scale(),
            pending: true,
            stage: 0,
            reserved: 0,
        };
        self.validate_controller_event(self.active_plan, event)?;
        let callbacks = self
            .plans
            .get(self.active_plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .stages
            .iter()
            .filter(|stage| stage.controller.is_some())
            .count();
        if !self.behavior_room(callbacks) {
            return Err(Error::Capacity);
        }
        // Saved automation receives the SAME explicit authority. Its callback
        // can author a replacement, never fall back to a port-derived owner.
        self.dispatch_automation(
            crate::ControlContext {
                performance: self.receiver(id)?.performance,
                origin,
                channels,
            },
            self.active_plan,
            crate::AutomationSource::Controller(number),
            f64::from(event.value) / f64::from(u32::MAX),
            callbacks,
            event.secondary,
            event.receiver,
        )?;
        // No callbacks means refusal is returned before raw/stage input storage.
        // With a callback, downstream refusal is its retained terminal outcome.
        self.admit_controller(self.active_plan, event)?;
        Ok(if matches!(number, 64 | 66) {
            Applied::Pedal
        } else {
            Applied::Controller
        })
    }

    /// Final forwarding is the sole point at which the canonical protocol
    /// gesture becomes effective. This is also used after Wait/fork; components
    /// are recomputed from the current real owner rather than captured aggregates.
    pub(crate) fn publish_receiver_controller(
        &mut self,
        event: crate::controller_event::ControllerEvent,
        primary: ChannelScope,
        secondary: Option<ChannelScope>,
    ) -> Result<(), Error> {
        let capture = event.receiver.ok_or(Error::InvalidInput)?;
        let id = capture.receiver;
        let domain = OwnedReceiver::domain(event.origin.protocol)?;
        let allowed = self.receiver_allowed(id, primary, secondary)?;
        let raw = match capture.provenance {
            ReceiverProvenance::Wire(value) => value,
            ReceiverProvenance::Script => {
                if event.number == 128 {
                    Value::Bits14((event.value >> 18) as u16)
                } else {
                    Value::Bits32(event.value)
                }
            }
        };
        let gesture = if matches!(capture.kind, ReceiverActionKind::TimbreBridge) {
            Some(Gesture::Timbre)
        } else {
            match event.number {
                128 => Some(Gesture::Pitch),
                129 => Some(Gesture::Pressure),
                74 if self.receiver(id)?.zone_domain(domain) => Some(Gesture::Timbre),
                _ => None,
            }
        };
        if let Some(gesture) = gesture {
            let (proposal, pitch) = self.prepare_receiver_gesture(
                id,
                domain,
                event.origin.channel,
                gesture,
                raw,
                allowed,
            )?;
            self.publish_receiver_transaction(|runtime| {
                runtime.commit_receiver_gesture(id, proposal, pitch)?;
                if primary.channels != 0 {
                    runtime.publish_controller(
                        event.performance,
                        primary,
                        event.number,
                        event.value,
                    )?;
                }
                if let Some(scope) = secondary {
                    runtime.publish_controller(
                        event.performance,
                        scope,
                        event.number,
                        event.value,
                    )?;
                }
                Ok(())
            })
        } else if matches!(event.number, 120 | 123) {
            if event.value != 0 {
                return Err(Error::InvalidInput);
            }
            self.receiver_channel_terminal(id, domain, primary, secondary, event.number == 120)?;
            Ok(())
        } else if event.number == 121 {
            if event.value != 0 {
                return Err(Error::InvalidInput);
            }
            self.reset_receiver_controllers(id, domain, event.origin.channel, primary, secondary)
        } else {
            self.publish_receiver_transaction(|runtime| {
                if domain == ChannelDomain::Midi1 && matches!(event.number, 6 | 38 | 96..=101) {
                    let value = match raw {
                        Value::Bits7(value) => value,
                        _ => (event.value >> 25) as u8,
                    };
                    runtime.receiver_parameter(
                        id,
                        event.origin.channel,
                        event.number,
                        value,
                        Some(event),
                    )?;
                }
                if primary.channels != 0 {
                    runtime.publish_controller(
                        event.performance,
                        primary,
                        event.number,
                        event.value,
                    )?;
                }
                if let Some(scope) = secondary {
                    runtime.publish_controller(
                        event.performance,
                        scope,
                        event.number,
                        event.value,
                    )?;
                }
                Ok(())
            })
        }
    }

    fn reset_receiver_controllers(
        &mut self,
        id: ReceiverId,
        domain: ChannelDomain,
        channel: u8,
        primary: ChannelScope,
        secondary: Option<ChannelScope>,
    ) -> Result<(), Error> {
        let state = self.receiver(id)?;
        if state.zone_domain(domain) && channel != state.manager_channel() {
            return Ok(());
        }
        let performance = state.performance;
        self.validate_protocol_controller_reset_targets(performance, primary, secondary)?;
        let allowed = self.receiver_allowed(id, primary, secondary)?;
        let mut controls = self.receiver(id)?.wire.map(|wire| wire.controls);
        let mut managers = self.receiver(id)?.managers;
        for target in ChannelDomain::ALL {
            for at in 0..16 {
                if allowed[target.index()] & (1 << at) != 0 {
                    let value = &mut controls[target.index()][at];
                    value.pitch = 0.;
                    value.bend = 0.;
                    value.pressure = 0;
                    let manager = &mut managers[target.index()][at];
                    manager.controls.pitch = 0.;
                    manager.controls.bend = 0.;
                    manager.controls.pressure = 0;
                    manager.bend = if target == ChannelDomain::Midi1 {
                        Value::Bits14(8192)
                    } else {
                        Value::Bits32(per_note::CENTER)
                    };
                }
            }
        }
        let proposal = GestureProposal {
            controls,
            managers,
            domains: allowed
                .iter()
                .enumerate()
                .fold(0, |mask, (index, &channels)| {
                    mask | if channels != 0 { 1 << index } else { 0 }
                }),
            channels: [None; 3],
            gesture: Gesture::Reset,
            all_notes: true,
            bends: [None; 3],
        };
        let pitch = self.prepare_receiver_projection(id, proposal, allowed)?;
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_gesture(id, proposal, pitch)?;
            runtime.reset_protocol_controller_targets(performance, primary, secondary)?;
            let state = runtime.receiver_state_mut(id)?;
            for target in ChannelDomain::ALL {
                for at in 0..16 {
                    if allowed[target.index()] & (1 << at) != 0 {
                        state.wire[target.index()].bends[at] = if target == ChannelDomain::Midi1 {
                            Value::Bits14(8192)
                        } else {
                            Value::Bits32(per_note::CENTER)
                        };
                        state.wire[target.index()].parameters[at] = state::Parameter::default();
                    }
                }
            }
            Ok(())
        })
    }

    fn admit_receiver_input(&mut self,id:ReceiverId,input:Input,source:Input,pitch:NotePitch,velocity:f64,tuning:f64,silent:bool)->Result<NoteId,Error>{
        // Explicit host identities and remapped physical host sources keep the
        // legacy direct-trigger law; packet apply reserves raw intent itself.
        if matches!(input.protocol,Protocol::Midi1|Protocol::Midi2)&&input.external_id.is_none()
            &&source.protocol==input.protocol&&source.external_id.is_none(){
            let state=self.receiver(id)?;
            let voice=ChannelVoice{version:if input.protocol==Protocol::Midi2{Version::Midi2}else{Version::Midi1},group:state.group,channel:input.channel,
                message:Message::NoteOn{key:input.key,velocity:Value::Bits32((velocity.clamp(0.,1.)*f64::from(u32::MAX)).round() as u32),attribute:Attribute::default()}};
            if !velocity.is_finite()||!(0.0..=1.0).contains(&velocity)||!pitch.valid()||!tuning.is_finite(){return Err(Error::InvalidInput);}
            let epoch=self.receiver_wire_epoch(id)?;
            match self.reserve_raw_attack(epoch,RawAttackIntent{voice,source,frame:self.now})?{
                RawAdmission::Pending(admission)=>self.commit_raw_attack(admission,CapturedLogicalRawAttack{input,pitch,velocity,tuning,silent}),
                RawAdmission::Rejected{reason,..}=>Err(match reason{RawRejection::Core(error)=>error,RawRejection::Profile=>Error::InvalidInput,_=>Error::Capacity}),
            }
        }else{self.admit_receiver_input_decision(id,input,source,pitch,velocity,tuning,silent,None)}
    }

    fn admit_receiver_input_decision(
        &mut self,
        id: ReceiverId,
        input: Input,
        source: Input,
        pitch: NotePitch,
        velocity: f64,
        tuning: f64,
        silent: bool,
        decision:Option<Handle>,
    ) -> Result<NoteId, Error> {
        if !tuning.is_finite() || source.group >= 16 || source.channel >= 16 || source.key >= 128 {
            return Err(Error::InvalidInput);
        }
        let state = self.receiver(id)?;
        let domain = OwnedReceiver::domain(input.protocol)?;
        if !state.accepts_wire_channel(domain, input.channel) {
            return Err(Error::InvalidInput);
        }
        let kind = if domain != ChannelDomain::HostChannel {
            Some(PhysicalKind::Wire)
        } else if state::native_source(source) {
            Some(PhysicalKind::Native)
        } else {
            None
        };
        if kind == Some(PhysicalKind::Native)
            && source.external_id.is_some_and(|value| value >= 0)
            && state.physical.iter().any(|pair| {
                pair.kind == PhysicalKind::Native
                    && pair.source == source
                    && !pair.terminal_received
            })
        {
            return Err(Error::DuplicateInput);
        }
        let wire = state
            .physical
            .iter()
            .filter(|pair| pair.kind == PhysicalKind::Wire)
            .count();
        let legacy = self
            .notes
            .slots
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .filter(|note| {
                note.input.is_some()
                    && note.address.protocol == Protocol::Native
                    && self
                        .expression_receiver(note.expression)
                        .is_some_and(|component| {
                            component.receiver == id && !state::native_source(component.source)
                        })
            })
            .count();
        let native = state
            .physical
            .iter()
            .filter(|pair| pair.kind == PhysicalKind::Native)
            .count()
            .checked_add(legacy)
            .ok_or(Error::Capacity)?;
        if kind == Some(PhysicalKind::Wire) && wire == state.limit
            || domain == ChannelDomain::HostChannel && native == state.limit
            || kind.is_some() && decision.is_none() && state.physical.available() == 0
        {
            return Err(Error::Capacity);
        }
        let member = if state.zone_domain(domain) && input.channel == state.manager_channel() {
            Controls::default()
        } else {
            state.wire[domain.index()].controls[usize::from(input.channel)]
        };
        let per_note = if domain == ChannelDomain::Midi2 {
            Some(
                state
                    .per_note
                    .snapshot(state.group, input.channel, input.key)?,
            )
        } else {
            None
        };
        let manager = if state.zone_domain(domain) {
            state.managers[domain.index()][usize::from(input.channel)].controls
        } else {
            Controls::default()
        };
        let mut expression = member.expression(manager);
        expression.pitch_semitones +=
            state.transpose + tuning + per_note.map_or(0., |value| value.pitch);
        let component = ReceiverComponents {
            receiver: id,
            origin: None,
            domain,
            channel: input.channel,
            key: input.key,
            source,
            member,
            manager,
            tuning,
            per_note,
            held: true,
        };
        let intrinsic=if domain==ChannelDomain::Midi2&&!matches!(pitch,NotePitch::Absolute(_)){
            state.per_note.key(state.group,input.channel,input.key)?.1.absolute.map(|value|f64::from(value)/33_554_432.)
        }else{None};
        let performance = state.performance;
        let event_plan=kind.and_then(|_|self.plans.get(self.active_plan.0)
            .filter(|generation|generation.prepared.external_programs).map(|_|self.active_plan));
        if let Some(plan)=event_plan {self.validate_physical_event_add(plan,1)?;}
        let active=usize::from(self.receivers.get(id.0).ok_or(Error::StaleHandle)?.terminal_delivery.is_some());
        state.validate_receipt_union(active,usize::from(decision.is_none()&&kind.is_some()&&(silent||event_plan.is_some())))?;
        if let Some(decision)=decision{
            let row=state.physical.decisions.get(decision).ok_or(Error::StaleHandle)?;
            if !matches!(row.phase,state::PhysicalPhase::Pending(_))||row.epoch!=state.wire_epoch||row.source!=source{return Err(Error::StaleHandle);}
        }

        self.publish_receiver_transaction(|runtime| {
            let note=if silent {runtime.note_on_owned_pitch_base(performance,input,source,pitch,intrinsic,velocity,expression,Some(component))?}
                else{runtime.trigger_owned_pitch_base(performance,input,source,pitch,intrinsic,velocity,expression,Some(component))?};
            let expression=runtime.expression_id(note)?;
            if let Some(plan)=event_plan {runtime.retain_physical_event(plan)?;}
            if let Some(kind)=kind {
                let pair=PhysicalPair{note,expression:Some(expression),execution:input,source,kind,released:false,scripted:silent,terminal_received:false,choked:false,event_plan};
                if let Some(decision)=decision {
                    runtime.receiver_state_mut(id)?.physical.decisions.get_mut(decision).ok_or(Error::StaleHandle)?.phase=state::PhysicalPhase::Accepted(pair);
                }else{runtime.receiver_state_mut(id)?.physical.push(pair)?;}
            }
            Ok(note)
        })
    }

    fn freeze_receiver_expression(&mut self, id: ReceiverId, expression: Option<ExpressionId>) {
        let Some(expression) = expression else {
            return;
        };
        // This is a real generational component origin, not channel/key/source
        // equality: overlapping same-key physical pairs stay independent.
        for slot in &mut self.expressions.slots {
            if let Some(owner) = &mut slot.value {
                if let Some(component) = &mut owner.receiver {
                    if component.receiver == id && component.origin == Some(expression) {
                        component.held = false;
                    }
                }
            }
        }
    }

    fn release_receiver_wire(&mut self,id:ReceiverId,source:Input,velocity:Option<f64>)->Result<Option<NoteId>,Error>{
        let epoch=self.receiver_wire_epoch(id)?;
        let Some(claim)=self.claim_raw_terminal(epoch,source,velocity)?else{return Ok(None);};
        match self.commit_raw_terminal(claim.terminal())?{RawTerminalOutcome::Released(note)=>Ok(Some(note)),_=>Ok(None)}
    }

    pub fn trigger_receiver(
        &mut self,
        id: ReceiverId,
        channel: u8,
        input: Input,
        source: Option<Input>,
        velocity: f64,
        tuning: f64,
        silent: bool,
    ) -> Result<NoteId, Error> {
        let state = self.receiver(id)?;
        if input.port != state.port
            || input.group != state.group
            || input.channel != channel
            || input.channel >= 16
            || input.group >= 16
            || input.key >= 128
            || source.is_some_and(|source| {
                source.group >= 16 || source.channel >= 16 || source.key >= 128
            })
            || !velocity.is_finite()
            || !(0.0..=1.0).contains(&velocity)
            || !tuning.is_finite()
        {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        self.admit_receiver_input(
            id,
            input,
            source.unwrap_or(input),
            NotePitch::Key(input.key),
            velocity,
            tuning,
            silent,
        )
    }

    pub fn apply_receiver(
        &mut self,
        id: ReceiverId,
        voice: Option<ChannelVoice>,
        silent: bool,
        source: Option<Input>,
    ) -> Result<Applied, ApplyError> {
        self.receiver(id)?;
        let Some(voice) = voice else {
            return Ok(Applied::Unsupported);
        };
        if self.external_execution_aborted&&!matches!(voice.message,Message::NoteOff{..}){return Err(Error::Canceled.into());}
        if
!voice.valid()
            || source
                .is_some_and(|value| value.group >= 16 || value.channel >= 16 || value.key >= 128)
        {
            return Err(Error::InvalidInput.into());
        }
        let state = self.receiver(id)?;
        if voice.group != state.group {
            return Err(ApplyError::DisabledGroup);
        }
        let domain = if voice.version == Version::Midi2 {
            ChannelDomain::Midi2
        } else {
            ChannelDomain::Midi1
        };
        let input = Input {
            protocol: domain.protocol(),
            port: state.port,
            group: state.group,
            channel: voice.channel,
            key: 0,
            external_id: None,
        };
        if let Message::NoteOn{key,velocity,attribute}=voice.message {
            if attribute.kind!=0&&voice.version!=Version::Midi2{return Ok(Applied::Unsupported);}
            let input=Input{key,..input};let source=source.unwrap_or(input);
            let epoch=self.receiver_wire_epoch(id)?;
            return match self.reserve_raw_attack(epoch,RawAttackIntent{voice,source,frame:self.now})?{
                RawAdmission::Rejected{reason:RawRejection::Profile,..}=>Ok(if self.receiver(id)?.configured{Applied::Ignored}else{Applied::Unsupported}),
                RawAdmission::Rejected{reason,..}=>Err(match reason{RawRejection::Core(error)=>error,_=>Error::Capacity}.into()),
                RawAdmission::Pending(admission)=>{
                    let pitch=if voice.version==Version::Midi2&&attribute.kind==3{NotePitch::Absolute(f64::from(attribute.data)/512.)}else{NotePitch::Key(key)};
                    Ok(Applied::Started(self.commit_raw_attack(admission,CapturedLogicalRawAttack{input,pitch,velocity:velocity.normalized(),tuning:0.,silent})?))
                }
            };
        }
        let terminal = if let Message::NoteOff { key, .. } = voice.message {
            Some(source.unwrap_or(Input { key, ..input }))
        } else {
            None
        };
        let captured=terminal.is_some_and(|source|state.has_wire_source(source));
        let selected = state.wire[0].parameters[usize::from(voice.channel)];
        let mcm = voice.version == Version::Midi1 && selected.registered && selected.rpn == [0, 6];
        if mcm
            && matches!(
                voice.message,
                Message::Control {
                    index: 38 | 96 | 97,
                    ..
                }
            )
        {
            return Ok(Applied::Ignored);
        }
        let parameter = voice.version == Version::Midi1
            && matches!(voice.channel, 0 | 15)
            && (matches!(
                voice.message,
                Message::Control {
                    index: 98..=101,
                    ..
                }
            ) || mcm && matches!(voice.message, Message::Control { index: 6, .. }));
        if !state.accepts_wire_channel(domain, voice.channel) && !captured && !parameter {
            return Ok(if state.configured || terminal.is_some() {
                Applied::Ignored
            } else {
                Applied::Unsupported
            });
        }
        self.apply_due();
        Ok(match voice.message {
            Message::NoteOn {
                key,
                velocity,
                attribute,
            } if attribute.kind == 0 || voice.version == Version::Midi2 => {
                let input = Input { key, ..input };
                let source = source.unwrap_or(input);
                let pitch = if voice.version == Version::Midi2 && attribute.kind == 3 {
                    NotePitch::Absolute(f64::from(attribute.data) / 512.)
                } else {
                    NotePitch::Key(key)
                };
                Applied::Started(self.admit_receiver_input(
                    id,
                    input,
                    source,
                    pitch,
                    velocity.normalized(),
                    0.,
                    silent,
                )?)
            }
            Message::NoteOff {
                velocity,
                attribute,
                ..
            } => match self.release_receiver_wire(
                id,
                terminal.ok_or(Error::InvalidInput)?,
                velocity.map(Value::normalized),
            )? {
                Some(note) => Applied::Released {
                    note,
                    velocity,
                    attribute,
                },
                None => Applied::Ignored,
            },
            Message::PitchBend(value) => self.dispatch_receiver_controller(
                id,
                input,
                source.unwrap_or(input),
                128,
                value,
                ReceiverActionKind::Controller,
            )?,
            Message::ChannelPressure(value) => self.dispatch_receiver_controller(
                id,
                input,
                source.unwrap_or(input),
                129,
                value,
                ReceiverActionKind::Controller,
            )?,
            Message::Program{program,bank}=>{
                if self.receiver(id)?.zone_domain(domain)&&voice.channel!=self.receiver(id)?.manager_channel(){Applied::Ignored}
                else{
                    let performance=self.receiver(id)?.performance;
                    let (_,targets)=self.dispatch_program_change_targets(performance,input.channel_address(),source.unwrap_or(input).channel_address(),1<<input.channel,program,
                        if voice.version==Version::Midi1{ProgramBankIntent::UsePendingBankSelect}else{bank.map_or(ProgramBankIntent::KeepSelected,ProgramBankIntent::Select)},None,Some(id),channel_event::ProgramProvenance::Wire(voice.version))?;
                    Applied::ProgramChange{targets}
                }
            }
            Message::PolyPressure { key, value } => {
                let performance=self.receiver(id)?.performance;
                let at=self.performance_index(performance)?;
                let address=input.channel_address();
                let scope=ChannelScope{protocol:address.protocol,port:address.port,group:address.group,channels:1<<address.channel};
                let capture=CapturedReceiver{receiver:id,kind:ReceiverActionKind::Controller,provenance:ReceiverProvenance::Wire(value),
                    source:source.unwrap_or(input).channel_address(),policy_epochs:self.performance_state.epochs(at,scope),host_epochs:self.receiver_host_epochs(id,at)?};
                let recipients=source.map(|source|crate::InputSelector{protocol:Some(source.protocol),port:Some(source.port),group:Some(source.group),channel:Some(source.channel),key:Some(source.key),external_id:source.external_id});
                let (_,owners)=self.dispatch_poly_pressure_captured(performance,address,key,value.full_scale(),recipients,Some(capture))?;
                Applied::Expression{owners}
            }
            Message::Control { index: 121, value } if value.full_scale() == 0 => {
                if self.receiver(id)?.zone_domain(domain)
                    && voice.channel != self.receiver(id)?.manager_channel()
                {
                    Applied::Ignored
                } else {
                    self.dispatch_receiver_controller(
                        id,
                        input,
                        source.unwrap_or(input),
                        121,
                        value,
                        ReceiverActionKind::Controller,
                    )?
                }
            }
            Message::Control {
                index: index @ (120 | 123),
                value,
            } if value.full_scale() == 0 => {
                if self.receiver(id)?.zone_domain(domain)
                    && voice.channel != self.receiver(id)?.manager_channel()
                {
                    Applied::Ignored
                } else {
                    self.dispatch_receiver_controller(
                        id,
                        input,
                        source.unwrap_or(input),
                        index,
                        value,
                        ReceiverActionKind::Controller,
                    )?
                }
            }
            Message::Control { index: 74, value } if self.receiver(id)?.zone_domain(domain) => {
                let number = self.receiver(id)?.timbre_controller.unwrap_or(74);
                self.dispatch_receiver_controller(
                    id,
                    input,
                    source.unwrap_or(input),
                    number,
                    value,
                    ReceiverActionKind::TimbreBridge,
                )?
            }
            Message::Control { index, value }
                if index < 120
                    && !(voice.version == Version::Midi2 && is_midi2_legacy_control(index)) =>
            {
                if !self.receiver(id)?.zone_domain(domain)
                    || voice.channel == self.receiver(id)?.manager_channel()
                    || parameter
                {
                    self.dispatch_receiver_controller(
                        id,
                        input,
                        source.unwrap_or(input),
                        index,
                        value,
                        ReceiverActionKind::Controller,
                    )?
                } else {
                    Applied::Ignored
                }
            }
            Message::Control { index, .. }
                if voice.version == Version::Midi2 && is_midi2_legacy_control(index) =>
            {
                Applied::Ignored
            }
            Message::PerNoteControl{space:Controllers::Registered,key,index:3,value} if voice.version==Version::Midi2=>{
                self.receiver_per_note_absolute(id,voice.channel,key,value)?
            }
            Message::PerNotePitch { key, value } if voice.version == Version::Midi2 => {
                self.receiver_per_note_pitch(id, voice.channel, key, value)?
            }
            Message::PerNoteManagement { key, detach, reset }
                if voice.version == Version::Midi2 =>
            {
                self.receiver_per_note_management(id, voice.channel, key, detach, reset)?
            }
            Message::ChannelControl {
                space: Controllers::Registered,
                bank: 0,
                index: 7,
                value,
            } if voice.version == Version::Midi2 => {
                self.receiver_per_note_range(id, voice.channel, f64::from(value) / 33_554_432.)?
            }
            Message::ChannelControl {
                space: Controllers::Registered,
                bank: 0,
                index: 0,
                value,
            } => self.receiver_range(
                id,
                domain,
                voice.channel,
                state::Sensitivity {
                    coarse: (value >> 25) as u8,
                    cents: ((value >> 18) & 127) as u8,
                },
            )?,
            _ => Applied::Unsupported,
        })
    }
}

impl Runtime {
    fn stage_receiver_note_pitch(
        &mut self,id:ReceiverId,
        select:impl FnMut(&OwnedReceiver,ReceiverComponents)->Result<Option<ReceiverComponents>,Error>,
    )->Result<bool,Error>{
        self.stage_receiver_note_pitch_rows(id,select)?;self.prepare_receiver_expression_changes()
    }
    fn stage_receiver_note_pitch_rows(
        &mut self,id:ReceiverId,
        mut select:impl FnMut(&OwnedReceiver,ReceiverComponents)->Result<Option<ReceiverComponents>,Error>,
    )->Result<(),Error>{
        self.receiver_changes.clear();
        self.receiver_component_changes.fill(None);
        for index in 0..self.expressions.slots.len() {
            let Some(owner) = self.expressions.slots[index].value else {
                continue;
            };
            let Some(component) = owner.receiver.filter(|component| component.receiver == id)
            else {
                continue;
            };
            let state = self.receiver(id)?;
            let Some(next) = select(state, component)? else {
                continue;
            };
            let combined = next.member.expression(next.manager);
            let value = Expression {
                pitch_semitones: combined.pitch_semitones
                    + state.transpose
                    + next.tuning
                    + next.per_note.map_or(0., |value| value.pitch),
                bend: combined.bend,
                ..owner.value
            };
            if self.receiver_changes.len() == self.receiver_changes.capacity() {
                return Err(Error::Capacity);
            }
            self.receiver_changes
                .push((ExpressionId(self.expressions.id(index)), value));
            self.receiver_component_changes[index] = Some(next);
        }
        Ok(())
    }

    fn receiver_per_note_pitch(
        &mut self,
        id: ReceiverId,
        channel: u8,
        key: u8,
        bend: u32,
    ) -> Result<Applied, Error> {
        let state = self.receiver(id)?;
        let (key_id, before) = state.per_note.key(state.group, channel, key)?;
        let channel_id = state.per_note.channel(state.group, channel)?;
        let next = per_note::KeyState { bend, ..before };
        let projected = per_note::PerNoteState::project(next, state.per_note.range(channel_id));
        let pitch = self.stage_receiver_note_pitch(id, |_, component| {
            Ok((component.domain == ChannelDomain::Midi2
                && component.channel == channel
                && component.key == key
                && component
                    .per_note
                    .is_some_and(|previous| previous.cohort == before.cohort))
            .then_some(ReceiverComponents {
                per_note: Some(projected),
                ..component
            }))
        })?;
        let owners = self.receiver_changes.len();
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_expression_changes(pitch)?;
            runtime
                .receiver_state_mut(id)?
                .per_note
                .commit_key(key_id, next);
            Ok(Applied::Expression { owners })
        })
    }

    fn receiver_per_note_absolute(&mut self,id:ReceiverId,channel:u8,key:u8,value:u32)->Result<Applied,Error>{
        let state=self.receiver(id)?;let (key_id,before)=state.per_note.key(state.group,channel,key)?;
        self.receiver_changes.clear();self.receiver_component_changes.fill(None);
        for index in 0..self.expressions.slots.len(){
            let Some(owner)=self.expressions.slots[index].value else{continue;};
            let Some(component)=owner.receiver.filter(|component|component.receiver==id&&component.domain==ChannelDomain::Midi2&&component.channel==channel&&component.key==key&&component.per_note.is_some_and(|snapshot|snapshot.cohort==before.cohort))else{continue;};
            let _=component; // Absolute base is independent of the accepted relative expression.
            if self.receiver_changes.len()==self.receiver_changes.capacity(){return Err(Error::Capacity);}
            self.receiver_changes.push((ExpressionId(self.expressions.id(index)),owner.value));
        }
        let absolute=Some(f64::from(value)/33_554_432.);self.prepare_receiver_intrinsic_pitch(absolute)?;
        let owners=self.receiver_changes.len();
        self.publish_receiver_transaction(|runtime|{
            runtime.commit_receiver_intrinsic_pitch(absolute)?;
            runtime.receiver_state_mut(id)?.per_note.commit_key(key_id,per_note::KeyState{absolute:Some(value),..before});
            Ok(Applied::Expression{owners})
        })
    }

    fn receiver_per_note_management(
        &mut self,
        id: ReceiverId,
        channel: u8,
        key: u8,
        detach: bool,
        reset: bool,
    ) -> Result<Applied, Error> {
        if !detach && !reset {
            return Ok(Applied::Ignored);
        }
        let state = self.receiver(id)?;
        let (key_id, before) = state.per_note.key(state.group, channel, key)?;
        let next = per_note::PerNoteState::managed(before, detach, reset)?;
        if !detach && reset {
            let channel_id=state.per_note.channel(state.group,channel)?;
            let projected=per_note::PerNoteState::project(next,state.per_note.range(channel_id));
            self.stage_receiver_note_pitch_rows(id,|_,component|Ok((component.domain==ChannelDomain::Midi2&&component.channel==channel&&component.key==key&&component.per_note.is_some_and(|snapshot|snapshot.cohort==before.cohort)).then_some(ReceiverComponents{per_note:Some(projected),..component})))?;
            self.prepare_receiver_intrinsic_pitch(None)?;
            return self.publish_receiver_transaction(|runtime|{
                runtime.commit_receiver_intrinsic_pitch(None)?;
                runtime.receiver_state_mut(id)?.per_note.commit_key(key_id,next);Ok(Applied::Configuration)
            });
        }
        self.receiver_state_mut(id)?.per_note.commit_key(key_id,next);Ok(Applied::Configuration)
    }

    fn receiver_per_note_range(
        &mut self,
        id: ReceiverId,
        channel: u8,
        range: f64,
    ) -> Result<Applied, Error> {
        if !range.is_finite() || range < 0. {
            return Err(Error::InvalidInput);
        }
        let state = self.receiver(id)?;
        let channel_id = state.per_note.channel(state.group, channel)?;
        let pitch = self.stage_receiver_note_pitch(id, |state, component| {
            if component.domain != ChannelDomain::Midi2 || component.channel != channel {
                return Ok(None);
            }
            let (_, key) = state.per_note.key(state.group, channel, component.key)?;
            Ok(component
                .per_note
                .filter(|snapshot| snapshot.cohort == key.cohort)
                .map(|_| ReceiverComponents {
                    per_note: Some(per_note::PerNoteState::project(key, range)),
                    ..component
                }))
        })?;
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_expression_changes(pitch)?;
            runtime
                .receiver_state_mut(id)?
                .per_note
                .commit_range(channel_id, range);
            Ok(Applied::Configuration)
        })
    }

    pub fn set_receiver_note_tuning(
        &mut self,
        id: ReceiverId,
        note: NoteId,
        tuning: f64,
    ) -> Result<(), Error> {
        if !tuning.is_finite() {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        let expression = self.expression_id(note)?;
        let component = self
            .expressions
            .get(expression.0)
            .and_then(|owner| owner.receiver)
            .filter(|component| component.receiver == id)
            .ok_or(Error::InvalidInput)?;
        let origin = component.origin;
        let pitch = self.stage_receiver_note_pitch(id, |_, component| {
            Ok((component.origin == origin).then_some(ReceiverComponents {
                tuning,
                ..component
            }))
        })?;
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_expression_changes(pitch)
        })
    }

    pub fn transpose_receiver(&mut self, id: ReceiverId, semitones: f64) -> Result<usize, Error> {
        if !semitones.is_finite() {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        self.receiver(id)?;
        self.receiver_changes.clear();
        self.receiver_component_changes.fill(None);
        for index in 0..self.expressions.slots.len() {
            let Some(owner) = self.expressions.slots[index].value else {
                continue;
            };
            let Some(component) = owner.receiver.filter(|component| component.receiver == id)
            else {
                continue;
            };
            let combined = component.member.expression(component.manager);
            let value = Expression {
                pitch_semitones: combined.pitch_semitones
                    + semitones
                    + component.tuning
                    + component.per_note.map_or(0., |state| state.pitch),
                bend: combined.bend,
                ..owner.value
            };
            if self.receiver_changes.len() == self.receiver_changes.capacity() {
                return Err(Error::Capacity);
            }
            self.receiver_changes
                .push((ExpressionId(self.expressions.id(index)), value));
        }
        let pitch = self.prepare_receiver_expression_changes()?;
        let count = self.receiver_changes.len();
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_expression_changes(pitch)?;
            runtime.receiver_state_mut(id)?.transpose = semitones;
            Ok(count)
        })
    }
}

impl Runtime {
    pub fn receiver_has_native_note(&self, id: ReceiverId, note: NoteId) -> bool {
        self.receivers.get(id.0).is_some_and(|owner| {
            owner.state.has_native_physical_note(note)
                || owner.terminal_delivery.is_some_and(|(_, release,_
)| {
                    state::native_source(release.source) && release.note == note
                })
        })
    }
    pub fn validate_receiver_native_releases(
        &self,
        id: ReceiverId,
        matches: impl Fn(NoteId, Input) -> bool,
    ) -> Result<(), Error> {
        let owner=self.receivers.get(id.0).ok_or(Error::StaleHandle)?;
        owner.state.validate_receipt_union(usize::from(owner.terminal_delivery.is_some()),0)?;
        owner.state.validate_native_physical_releases(matches)
    }
    pub fn release_receiver_native(
        &mut self,
        id: ReceiverId,
        matches: impl Fn(NoteId, Input) -> bool,
        velocity: Option<f64>,
    ) -> Result<usize, Error> {
        if velocity.is_some_and(|value| !(0.0..=1.0).contains(&value)) {
            return Err(Error::InvalidInput);
        }
        self.validate_receiver_native_releases(id, &matches)?;
        for pair in &self.receiver(id)?.physical {
            if pair.kind==PhysicalKind::Native&&!pair.terminal_received&&matches(pair.note,pair.source){self.validate_physical_event_row(*pair)?;}
        }
        self.publish_receiver_transaction(|runtime| {
            let mut count = 0;
            let mut cursor=runtime.receiver(id)?.physical.first_index();
            while let Some(at)=cursor {
                cursor=runtime.receiver(id)?.physical.next_index(at);
                let Some(pair) = runtime.receiver(id)?.physical.get(at).copied() else{continue;};
                if pair.kind != PhysicalKind::Native
                    || pair.terminal_received
                    || !matches(pair.note, pair.source)
                {
                    continue;
                }
                match runtime.note_off_exact(pair.note, velocity) {
                    Ok(()) | Err(Error::StaleHandle) | Err(Error::ClosedNote) => {}
                    Err(error) => return Err(error),
                }
                runtime.freeze_receiver_expression(id, pair.expression);
                let transfer=!pair.released&&(pair.scripted||pair.event_plan.is_some());
                if transfer {let receipt=runtime.physical_terminal(pair,velocity);runtime.receiver_state_mut(id)?.terminal_releases.push(receipt);}
                else if let Some(plan)=pair.event_plan {runtime.release_physical_event(plan)?;}
                let state=runtime.receiver_state_mut(id)?;
                state.physical.get_mut(at).ok_or(Error::StaleHandle)?.event_plan=None;
                state.physical.get_mut(at).ok_or(Error::StaleHandle)?.released = true;
                state.physical.get_mut(at).ok_or(Error::StaleHandle)?.terminal_received = true;
                count += 1;
            }
            runtime
                .receiver_state_mut(id)?
                .physical
                .retain(|pair| !pair.terminal_received);
            Ok(count)
        })
    }
    pub fn choke_receiver_native(
        &mut self,
        id: ReceiverId,
        matches: impl Fn(NoteId, Input) -> bool,
    ) -> Result<usize, Error> {
        self.receiver(id)?;
        self.publish_receiver_transaction(|runtime| {
            let mut count = 0;
            let mut cursor=runtime.receiver(id)?.physical.first_index();
            while let Some(at)=cursor {
                cursor=runtime.receiver(id)?.physical.next_index(at);
                let Some(pair) = runtime.receiver(id)?.physical.get(at).copied() else{continue;};
                if pair.kind != PhysicalKind::Native
                    || pair.terminal_received
                    || pair.choked
                    || !matches(pair.note, pair.source)
                {
                    continue;
                }
                match runtime.cancel_input_note(pair.note) {
                    Ok(()) | Err(Error::StaleHandle) | Err(Error::ClosedNote) => {}
                    Err(error) => return Err(error),
                }
                runtime.receiver_state_mut(id)?.physical.get_mut(at).ok_or(Error::StaleHandle)?.choked = true;
                count += 1;
            }
            Ok(count)
        })
    }
    pub fn validate_receiver_all_releases(&self, id: ReceiverId) -> Result<(), Error> {
        let owner=self.receivers.get(id.0).ok_or(Error::StaleHandle)?;
        owner.state.validate_receipt_union(usize::from(owner.terminal_delivery.is_some()),0)?;
        self.receiver(id)?.validate_all_physical_releases()
    ?;self.validate_physical_terminal_copies(id,|pair|!pair.released)
    }
    pub fn queue_receiver_all_releases(&mut self,id:ReceiverId)->Result<usize,Error>{
        self.validate_receiver_all_releases(id)?;
        self.publish_receiver_transaction(|runtime| {
            let mut cursor=runtime.receiver(id)?.physical.first_index();let mut count=0;
            while let Some(at)=cursor {
                cursor=runtime.receiver(id)?.physical.next_index(at);
                let Some(pair)=runtime.receiver(id)?.physical.get(at).copied() else{continue;};if pair.released{continue;}
                runtime.freeze_receiver_expression(id,pair.expression);
                if pair.scripted||pair.event_plan.is_some() {runtime.queue_physical_terminal_copy(id,pair,None)?;}
                runtime.receiver_state_mut(id)?.physical.get_mut(at).ok_or(Error::StaleHandle)?.released = true;
                count += 1;
            }
            Ok(count)
        })
    }
    /// One receipt is removed before the caller runs Lua. Its actual owner Slot
    /// is present throughout that callback; the facade never holds mutable state.
    pub fn next_receiver_terminal(
        &mut self,
        id: ReceiverId,
    ) -> Result<Option<ReceiverTerminalDelivery>, Error> {
        let owner = self.receivers.get_mut(id.0).ok_or(Error::StaleHandle)?;
        if owner.terminal_delivery.is_some() {
            return Err(Error::Busy);
        }
        if owner.state.terminal_releases.is_empty() {
            owner.state.physical.retain(|pair| !pair.terminal_received);
            return Ok(None);
        }
        let sequence = owner
            .terminal_sequence
            .checked_add(1)
            .ok_or(Error::ClockOverflow)?;
        let references = owner
            .callback_references
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        let release = owner.state.terminal_releases.remove(0);
        owner.terminal_sequence = sequence;
        owner.callback_references = references;
        owner.terminal_delivery = Some((sequence, release,false));
        Ok(Some(ReceiverTerminalDelivery{release:release.release,receiver:id,sequence}))
    }
    pub fn complete_receiver_terminal(&mut self,delivery:ReceiverTerminalDelivery)->Result<(),Error>{
        let owner=self.receivers.get_mut(delivery.receiver.0).ok_or(Error::StaleHandle)?;
        let (_,receipt,_)=owner.terminal_delivery.filter(|(sequence,_,_)|*sequence==delivery.sequence).ok_or(Error::StaleHandle)?;
        if let Some(event)=receipt.event {self.release_physical_event(event.plan)?;}
        let owner=self.receivers.get_mut(delivery.receiver.0).ok_or(Error::StaleHandle)?;
        owner.terminal_delivery = None;
        owner.callback_references -= 1;
        Ok(())
    }

    fn receiver_channel_terminal(
        &mut self,
        id: ReceiverId,
        domain: ChannelDomain,
        primary: ChannelScope,
        secondary: Option<ChannelScope>,
        sound_off: bool,
    ) -> Result<Applied, Error> {
        let owner=self.receivers.get(id.0).ok_or(Error::StaleHandle)?;
        owner.state.validate_receipt_union(usize::from(owner.terminal_delivery.is_some()),0)?;
        let state = self.receiver(id)?;
        let channels = primary.channels;
        let host = if sound_off { None } else { secondary };
        let matches = |pair: PhysicalPair| {
            pair.kind == PhysicalKind::Wire
                && pair.execution.protocol == domain.protocol()
                && channels & (1 << pair.execution.channel) != 0
                || pair.kind == PhysicalKind::Native
                    && host.is_some_and(|scope| scope.channels & (1 << pair.execution.channel) != 0)
        };
        let needed = state
            .physical
            .iter()
            .filter(|&&pair| !pair.released && (pair.scripted||pair.event_plan.is_some())&&matches(pair)).count();
        if !sound_off&&state.terminal_releases.len().checked_add(needed).is_none_or(|count|count>state.terminal_limit){return Err(Error::Capacity);}
        let port=state.port;let group=state.group;
        if !sound_off {self.validate_physical_terminal_copies(id,|pair|!pair.released&&matches(pair))?;}
        self.publish_receiver_transaction(|runtime| {
            let mut count = 0;
            for channel in 0..16u8 {
                if channels & (1 << channel) != 0 {
                    let address = ChannelAddress {
                        protocol: domain.protocol(),
                        port,
                        group,
                        channel,
                    };
                    count += if sound_off {
                        runtime.all_sound_off(address)?
                    } else {
                        runtime.all_notes_off(address)?
                    };
                }
                if let Some(scope) = host
                    && scope.channels & (1 << channel) != 0
                {
                    count += runtime.all_notes_off(ChannelAddress {
                        protocol: scope.protocol,
                        port: scope.port,
                        group: scope.group,
                        channel,
                    })?;
                }
            }
            if !sound_off {
                let mut cursor=runtime.receiver(id)?.physical.first_index();
                while let Some(at)=cursor {
                    cursor=runtime.receiver(id)?.physical.next_index(at);
                    let Some(pair) = runtime.receiver(id)?.physical.get(at).copied() else{continue;};
                    if pair.released || !matches(pair) {
                        continue;
                    }
                    runtime.freeze_receiver_expression(id, pair.expression);
                    if pair.scripted||pair.event_plan.is_some() {runtime.queue_physical_terminal_copy(id,pair,None)?;}
                    runtime.receiver_state_mut(id)?.physical.get_mut(at).ok_or(Error::StaleHandle)?.released = true;
                }
            }
            Ok(if sound_off {
                Applied::AllSoundOff { stopped: count }
            } else {
                Applied::AllNotesOff { released: count }
            })
        })
    }
}

#[cfg(test)]
mod expression_reference_tests {
    use super::*;
    #[test]
    fn single_follows_dag_releases_a_deep_actual_generation_chain_iteratively() {
        let prepared = crate::Prepared::new(48000, vec![], vec![], 0).unwrap();
        let mut limits = crate::Limits::for_plan(&prepared, 2, 1);
        limits.expressions = 4096;
        let mut runtime = Runtime::new(prepared, limits).unwrap();
        let input = Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(1),
        };
        let root = runtime.note_on(input, 60, 1.).unwrap();
        let origin = runtime.expression_id(root).unwrap();
        let template = *runtime.expressions.get(origin.0).unwrap();
        let mut previous = origin;
        for index in 0..4000 {
            let source = runtime.expressions.get_mut(previous.0).unwrap();
            source.followers = source.followers.checked_add(1).unwrap();
            previous = ExpressionId(
                runtime
                    .expressions
                    .insert(crate::ExpressionOwner {
                        notes: usize::from(index == 3999),
                        followers: 0,captures:0,
                        follows: Some(previous),
                        receiver: None,
                        ..template
                    })
                    .unwrap(),
            );
        }
        assert_eq!(runtime.expression_count(), 4001);
        runtime.drop_expression(previous);
        assert_eq!(runtime.expression_count(), 1);
        assert_eq!(runtime.expressions.get(origin.0).unwrap().followers, 0);
        assert_eq!(runtime.expression(previous), Err(Error::StaleHandle));
        runtime.note_off_exact(root, None).unwrap();
        runtime.flush_ended(|_| true);
        assert_eq!(runtime.expression_count(), 0);
    }
}

impl Runtime {
    fn receiver_range(
        &mut self,
        id: ReceiverId,
        domain: ChannelDomain,
        channel: u8,
        value: state::Sensitivity,
    ) -> Result<Applied, Error> {
        if value.semitones() > 96. {
            return Err(Error::InvalidInput);
        }
        let state = self.receiver(id)?;
        let domains = state.affected_domains(domain, channel);
        let mut controls = state.control_rows();
        let mut channels = [None; 3];
        let mut managers = state.managers;
        for target in ChannelDomain::ALL {
            if domains & target.bit() == 0 {
                continue;
            }
            let selected = if target == ChannelDomain::HostChannel {
                state.host_channel(channel)
            } else {
                channel
            };
            let manager = selected == state.zone.manager();
            let zone = state.zone_domain(target);
            channels[target.index()] = if zone {
                manager.then_some(selected)
            } else {
                Some(selected)
            };
            if state.bend_override.is_some() {
                continue;
            }
            if zone && manager {
                for target in &mut managers[target.index()] {
                    target.controls.pitch = state::pitch(target.bend, value.semitones());
                }
            }
            for (at, control) in controls[target.index()].iter_mut().enumerate() {
                let at = at as u8;
                if !zone && at == selected
                    || zone
                        && (manager && at == state.zone.manager()
                            || !manager
                                && at != state.zone.manager()
                                && state.accepts_wire_channel(target, at))
                {
                    control.pitch = state::pitch(
                        state.wire[target.index()].bends[usize::from(at)],
                        value.semitones(),
                    );
                }
            }
        }
        let proposal = GestureProposal {
            controls,
            managers,
            domains,
            channels,
            gesture: Gesture::Pitch,
            all_notes: false,
            bends: [None; 3],
        };
        let pitch = self.prepare_receiver_projection(id, proposal, [u16::MAX; 3])?;
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_gesture(id, proposal, pitch)?;
            let state = runtime.receiver_state_mut(id)?;
            for target in ChannelDomain::ALL {
                if domains & target.bit() != 0 {
                    let selected = if target == ChannelDomain::HostChannel {
                        state.host_channel(channel)
                    } else {
                        channel
                    };
                    if target == ChannelDomain::Midi2 && state.midi2_channel_voice {
                        state.wire[target.index()].channel_ranges[usize::from(selected)] = value;
                    } else {
                        state.wire[target.index()].ranges
                            [usize::from(selected != state.zone.manager())] = value;
                    }
                }
            }
            Ok(Applied::Configuration)
        })
    }

    fn receiver_configuration_epochs_live(
        &self,
        id: ReceiverId,
        event: crate::controller_event::ControllerEvent,
        channels: u16,
    ) -> Result<bool, Error> {
        let state = self.receiver(id)?;
        let primary = self
            .performance_state
            .epochs(event.performance, event.scope());
        let same = |now: [u64; 16], then: [u64; 16]| {
            (0..16).all(|at| channels & (1 << at) == 0 || now[at] == then[at])
        };
        let Some(capture) = event.receiver else {
            return Ok(false);
        };
        if !same(primary, capture.policy_epochs) {
            return Ok(false);
        }
        if state.host_projection {
            let Some(before) = event.receiver.and_then(|capture| capture.host_epochs) else {
                return Ok(false);
            };
            let now = self.performance_state.epochs(
                event.performance,
                ChannelScope {
                    protocol: Protocol::Native,
                    port: state.port,
                    group: state.group,
                    channels,
                },
            );
            if !same(now, before) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn receiver_parameter(
        &mut self,
        id: ReceiverId,
        channel: u8,
        index: u8,
        value: u8,
        capture: Option<crate::controller_event::ControllerEvent>,
    ) -> Result<Applied, Error> {
        if channel >= 16 || value >= 128 {
            return Err(Error::InvalidInput);
        }
        let state = self.receiver(id)?;
        let parameter = state.wire[0].parameters[usize::from(channel)];
        if let Some(event) = capture {
            let current = self
                .performance_state
                .epochs(event.performance, event.scope());
            if current[usize::from(channel)] != event.epochs[usize::from(channel)] {
                return Ok(Applied::Ignored);
            }
            if parameter.registered && parameter.rpn == [0, 0] && matches!(index, 6 | 38 | 96 | 97)
            {
                let channels = if channel == state.zone.manager() {
                    state.manager_scope(ChannelDomain::Midi1).channels
                } else {
                    state.manager_scope(ChannelDomain::Midi1).channels
                        & !(1 << state.zone.manager())
                };
                if !self.receiver_configuration_epochs_live(id, event, channels)? {
                    return Ok(Applied::Ignored);
                }
            }
        }
        if parameter.registered && parameter.rpn == [0, 6] {
            match index {
                6 => return self.configure_receiver_zone(id, channel, value, capture),
                38 | 96 | 97 => return Ok(Applied::Ignored),
                _ => {}
            }
        }
        match index {
            100 | 101 => {
                let parameter =
                    &mut self.receiver_state_mut(id)?.wire[0].parameters[usize::from(channel)];
                parameter.rpn[usize::from(index == 100)] = value;
                parameter.registered = true;
                Ok(Applied::Configuration)
            }
            98 | 99 => {
                self.receiver_state_mut(id)?.wire[0].parameters[usize::from(channel)].registered =
                    false;
                Ok(Applied::Configuration)
            }
            6 if parameter.registered && parameter.rpn == [0, 0] => self.receiver_range(
                id,
                ChannelDomain::Midi1,
                channel,
                state::Sensitivity {
                    coarse: value,
                    cents: 0,
                },
            ),
            38 if parameter.registered && parameter.rpn == [0, 0] => self.receiver_range(
                id,
                ChannelDomain::Midi1,
                channel,
                state::Sensitivity {
                    coarse: state.sensitivity(ChannelDomain::Midi1, channel).coarse,
                    cents: value,
                },
            ),
            96 | 97 if parameter.registered && parameter.rpn == [0, 0] => self.receiver_range(
                id,
                ChannelDomain::Midi1,
                channel,
                state
                    .sensitivity(ChannelDomain::Midi1, channel)
                    .relative(index == 96),
            ),
            _ => Ok(Applied::Unsupported),
        }
    }

    fn configure_receiver_zone(
        &mut self,
        id: ReceiverId,
        channel: u8,
        members: u8,
        capture: Option<crate::controller_event::ControllerEvent>,
    ) -> Result<Applied, Error> {
        let changed_zone = match channel {
            0 => Zone::Lower,
            15 => Zone::Upper,
            _ => return Ok(Applied::Ignored),
        };
        if members > 15 {
            return Ok(Applied::Ignored);
        }
        let state = self.receiver(id)?;
        let previous = if state.configured || state.manual_active {
            state.zones
        } else {
            [0, 0]
        };
        let mut zones = previous;
        zones[changed_zone.index()] = members;
        if members > 0 {
            let other = 1 - changed_zone.index();
            zones[other] = zones[other].min(14u8.saturating_sub(members));
        }
        let role = |zones: [u8; 2], channel: u8| -> u8 {
            for zone in [Zone::Lower, Zone::Upper] {
                let members = zones[zone.index()];
                if members > 0 && zone.contains(channel, members) {
                    return 1 + zone.index() as u8 * 2 + u8::from(channel != zone.manager());
                }
            }
            0
        };
        let changed = (0..16u8).fold(0u16, |mask, channel| {
            mask | if role(previous, channel) != role(zones, channel) {
                1 << channel
            } else {
                0
            }
        });
        let performance = state.performance;
        let port = state.port;
        let group = state.group;
        let zone = state.zone;
        let selected = zones[zone.index()];
        let host_projection = state.host_projection;
        if let Some(event) = capture {
            let selected_mask = (0..16u8).fold(0u16, |mask, at| {
                mask | if zone.contains(at, selected) {
                    1 << at
                } else {
                    0
                }
            });
            let affected = changed
                | if changed_zone == zone {
                    selected_mask
                } else {
                    0
                };
            if !self.receiver_configuration_epochs_live(id, event, affected)? {
                return Ok(Applied::Ignored);
            }
        }
        // Reconfiguration cannot silently turn two formerly disjoint real
        // owner routes into an ambiguous lookup. Compare proposed channel roles.
        for slot in &self.receivers.slots {
            let Some(other) = &slot.value else {
                continue;
            };
            if other.state.registration == Some(id)
                || other.state.performance != performance
                || other.state.port != port
                || other.state.group != group
            {
                continue;
            }
            for domain in [ChannelDomain::Midi1, ChannelDomain::HostChannel] {
                if domain == ChannelDomain::HostChannel && !host_projection {
                    continue;
                }
                for channel in 0..16u8 {
                    let accepts = if selected == 0 {
                        zones == [0, 0] && channel == zone.manager()
                    } else {
                        zone.contains(channel, selected)
                    };
                    if accepts && other.state.accepts_wire_channel(domain, channel) {
                        return Err(Error::InvalidInput);
                    }
                }
            }
        }
        let primary = ChannelScope {
            protocol: Protocol::Midi1,
            port,
            group,
            channels: changed,
        };
        let secondary = host_projection.then_some(ChannelScope {
            protocol: Protocol::Native,
            ..primary
        });
        if changed != 0 {
            self.validate_channel_controller_reset_targets(performance, primary, secondary)?;
        }
        let domains = ChannelDomain::Midi1.bit()
            | if host_projection {
                ChannelDomain::HostChannel.bit()
            } else {
                0
            };
        let state = self.receiver(id)?;
        let reset_sensitivity = changed_zone == zone;
        let mut wire = state.wire;
        let mut managers = state.managers;
        for target in ChannelDomain::ALL {
            if domains & target.bit() != 0 {
                for at in 0..16u8 {
                    if changed & (1 << at) != 0 {
                        wire[target.index()].controls[usize::from(at)] = Controls::default();
                        wire[target.index()].bends[usize::from(at)] = Value::Bits14(8192);
                        wire[target.index()].parameters[usize::from(at)] =
                            state::Parameter::default();
                        managers[target.index()][usize::from(at)] = state::ManagerState::default();
                    }
                    if reset_sensitivity && zone.contains(at, selected) {
                        let range = if at == zone.manager() { 2. } else { 48. };
                        wire[target.index()].controls[usize::from(at)].pitch =
                            state::pitch(wire[target.index()].bends[usize::from(at)], range);
                    }
                }
                if reset_sensitivity {
                    for manager in &mut managers[target.index()] {
                        manager.controls.pitch = state::pitch(manager.bend, 2.);
                    }
                }
                if reset_sensitivity {
                    wire[target.index()].ranges = [
                        state::Sensitivity {
                            coarse: 2,
                            cents: 0,
                        },
                        state::Sensitivity {
                            coarse: 48,
                            cents: 0,
                        },
                    ];
                }
            }
        }
        wire[0].parameters[usize::from(channel)] = state.wire[0].parameters[usize::from(channel)];
        let proposal = GestureProposal {
            controls: wire.map(|wire| wire.controls),
            managers,
            domains,
            channels: [None; 3],
            gesture: Gesture::All,
            all_notes: true,
            bends: [None; 3],
        };
        let allowed = ChannelDomain::ALL.map(|domain| {
            if domains & domain.bit() != 0 {
                !changed
            } else {
                0
            }
        });
        let pitch = self.prepare_receiver_projection(id, proposal, allowed)?;
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_expression_changes(pitch)?;
            // Controller/voice validation above completed before any terminal,
            // epoch reset or retained configuration publication.
            for at in 0..runtime.notes.slots.len() {
                let Some(note) = runtime.notes.slots[at].value else {
                    continue;
                };
                if note.input.is_none() {
                    continue;
                }
                let component = runtime
                    .expressions
                    .get(note.expression.0)
                    .and_then(|owner| owner.receiver);
                if component.is_some_and(|component| {
                    component.receiver == id
                        && domains & component.domain.bit() != 0
                        && changed & (1 << component.channel) != 0
                }) {
                    let note = NoteId(runtime.notes.id(at));
                    match runtime.cancel_input_note(note) {
                        Ok(()) | Err(Error::StaleHandle) | Err(Error::ClosedNote) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            if changed != 0 {
                runtime.reset_channel_controller_targets(performance, primary, secondary)?;
                for channel in 0..16u8 {
                    if changed & (1 << channel) != 0 {
                        for domain in ChannelDomain::ALL {
                            if domains & domain.bit() != 0 {
                                runtime.all_sound_off(ChannelAddress {
                                    protocol: domain.protocol(),
                                    port,
                                    group,
                                    channel,
                                })?;
                            }
                        }
                    }
                }
            }
            let state = runtime.receiver_state_mut(id)?;
            state.wire = wire;
            state.managers = managers;
            state.zones = zones;
            state.members = selected;
            state.configured = true;
            state.host_mpe_active = true;
            if reset_sensitivity {
                state.bend_override = None;
            }
            Ok(Applied::Configuration)
        })
    }
}

impl Runtime {
    pub(crate) fn has_receiver_performance(&self, performance: usize) -> bool {
        self.receivers.slots.iter().any(|slot| {
            slot.value.as_ref().is_some_and(|owner| {
                self.performance_index(owner.state.performance) == Ok(performance)
            })
        })
    }
    /// One explicit global/startup gesture reaches each real owner once. It is
    /// not inferred from Native0 or from broadcast copies in physical banks.
    pub(crate) fn publish_global_receiver_controller(
        &mut self,
        performance: usize,
        number: u8,
        value: u32,
    ) -> Result<(), Error> {
        self.receiver_changes.clear();
        self.receiver_component_changes.fill(None);
        self.receiver_policy_changes.clear();
        let raw = if number == 128 {
            Value::Bits14((value >> 18) as u16)
        } else {
            Value::Bits32(value)
        };
        let gesture = match number {
            128 => Gesture::Pitch,
            129 => Gesture::Pressure,
            _ => {
                self.controller_now(performance, number, value);
                return Ok(());
            }
        };
        for at in 0..self.receivers.slots.len() {
            let Some(owner) = self.receivers.slots[at].value.as_ref() else {
                continue;
            };
            if self.performance_index(owner.state.performance)? != performance {
                continue;
            }
            let id = ReceiverId(self.receivers.id(at));
            let state = &owner.state;
            let mut controls = state.control_rows();
            let mut managers = state.managers;
            let mut domains = 0u8;
            for domain in ChannelDomain::ALL {
                if domain == ChannelDomain::HostChannel && !state.host_projection {
                    continue;
                }
                domains |= domain.bit();
                for channel in 0..16u8 {
                    if !state.accepts_wire_channel(domain, channel) {
                        continue;
                    }
                    if state.zone_domain(domain) {
                        let target = &mut managers[domain.index()][usize::from(channel)];
                        match gesture {
                            Gesture::Pitch => {
                                let range =
                                    state.bend_override.map(f64::from).unwrap_or_else(|| {
                                        state.sensitivity(domain, state.zone.manager()).semitones()
                                    });
                                target.controls.pitch = state::pitch(raw, range);
                                target.controls.bend = state::pitch(raw, 1.);
                                target.bend = raw;
                            }
                            Gesture::Pressure => target.controls.pressure = value,
                            _ => return Err(Error::InvalidInput),
                        }
                        if channel == state.zone.manager() {
                            controls[domain.index()][usize::from(channel)] = target.controls;
                        }
                    } else {
                        let target = &mut controls[domain.index()][usize::from(channel)];
                        match gesture {
                            Gesture::Pitch => {
                                let range =
                                    state.bend_override.map(f64::from).unwrap_or_else(|| {
                                        state.sensitivity(domain, channel).semitones()
                                    });
                                target.pitch = state::pitch(raw, range);
                                target.bend = state::pitch(raw, 1.);
                            }
                            Gesture::Pressure => target.pressure = value,
                            _ => return Err(Error::InvalidInput),
                        }
                    }
                }
            }
            let proposal = GestureProposal {
                controls,
                managers,
                domains,
                channels: [None; 3],
                gesture,
                all_notes: true,
                bends: [None; 3],
            };
            if self.receiver_policy_changes.len() == self.receiver_policy_changes.capacity() {
                return Err(Error::Capacity);
            }
            self.receiver_policy_changes.push((id, proposal));
            for index in 0..self.expressions.slots.len() {
                let Some(expression) = self.expressions.slots[index].value else {
                    continue;
                };
                let Some(component) = expression
                    .receiver
                    .filter(|component| component.receiver == id)
                else {
                    continue;
                };
                if let Some((value, next)) = self.receiver(id)?.project_component(
                    component,
                    expression.value,
                    proposal,
                    [u16::MAX; 3],
                ) {
                    if self.receiver_changes.len() == self.receiver_changes.capacity() {
                        return Err(Error::Capacity);
                    }
                    self.receiver_changes
                        .push((ExpressionId(self.expressions.id(index)), value));
                    self.receiver_component_changes[index] = Some(next);
                }
            }
        }
        let pitch = self.prepare_receiver_expression_changes()?;
        self.publish_receiver_transaction(|runtime| {
            runtime.commit_receiver_expression_changes(pitch)?;
            for at in 0..runtime.receiver_policy_changes.len() {
                let (id, proposal) = runtime.receiver_policy_changes[at];
                let state = runtime.receiver_state_mut(id)?;
                state.managers = proposal.managers;
                for domain in ChannelDomain::ALL {
                    if proposal.domains & domain.bit() != 0 {
                        state.wire[domain.index()].controls = proposal.controls[domain.index()];
                        if number == 128 {
                            for channel in 0..16u8 {
                                if state.accepts_wire_channel(domain, channel)
                                    && (!state.zone_domain(domain)
                                        || channel == state.zone.manager())
                                {
                                    state.wire[domain.index()].bends[usize::from(channel)] = raw;
                                }
                            }
                        }
                    }
                }
            }
            runtime.controller_now(performance, number, value);
            Ok(())
        })
    }
}

impl Runtime {
    pub fn set_receiver_manual_active(
        &mut self,
        id: ReceiverId,
        active: bool,
    ) -> Result<(), Error> {
        let state = self.receiver(id)?;
        if state.configured {
            return Ok(());
        }
        for slot in &self.receivers.slots {
            let Some(other) = &slot.value else {
                continue;
            };
            if other.state.registration == Some(id)
                || state.performance != other.state.performance
                || state.port != other.state.port
                || state.group != other.state.group
            {
                continue;
            }
            for channel in 0..16u8 {
                let proposed = state.host_projection
                    && state.accepts_channel(channel)
                    && (active || channel == state.zone.manager());
                if proposed
                    && other
                        .state
                        .accepts_wire_channel(ChannelDomain::HostChannel, channel)
                {
                    return Err(Error::InvalidInput);
                }
            }
        }
        self.receiver_state_mut(id)?.set_manual_active(active);
        Ok(())
    }
    pub fn set_receiver_timbre_controller(
        &mut self,
        id: ReceiverId,
        controller: Option<u8>,
    ) -> Result<(), Error> {
        self.receiver_state_mut(id)?
            .set_timbre_controller(controller);
        Ok(())
    }
    pub fn set_receiver_bend_range(
        &mut self,
        id: ReceiverId,
        range: Option<u8>,
    ) -> Result<(), Error> {
        self.receiver_state_mut(id)?.set_bend_range(range);
        Ok(())
    }
}

/// Checked retained allocations. Registry/expression/scratch fields are totals
/// for this Runtime (not an invented per-owner fraction); state_heap belongs to
/// the selected actual owner. Inline ManagerState/member/epoch masks are already
/// included in Slot<ReceiverOwner> and Slot<ExpressionOwner>.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiverMemory {
    pub registry: usize,
    pub expression_slots: usize,
    pub expression_scratch: usize,
    pub policy_scratch: usize,
    pub state_heap: usize,
}
impl Runtime {
    pub fn receiver_memory(&self, id: ReceiverId) -> Result<ReceiverMemory, Error> {
        let state = self.receiver(id)?;
        let array = |count: usize, size: usize| count.checked_mul(size).ok_or(Error::Capacity);
        let mut state_heap = state.per_note.retained_bytes()?.checked_add(state.physical.retained_bytes()?).ok_or(Error::Capacity)?;
        for (count, size) in [
            (
                state.bindings.capacity(),
                std::mem::size_of::<state::Binding>(),
            ),
            (
                state.changes.capacity(),
                std::mem::size_of::<(ExpressionId, Expression)>(),
            ),
            (
                state.note_changes.capacity(),
                std::mem::size_of::<(usize, per_note::Snapshot)>(),
            ),

            (
                state.terminal_releases.capacity(),
                std::mem::size_of::<QueuedTerminal>(),
            ),
            (state.reset_members.capacity(), std::mem::size_of::<usize>()),
        ] {
            state_heap = state_heap
                .checked_add(array(count, size)?)
                .ok_or(Error::Capacity)?;
        }
        let registry = array(
            self.receivers.slots.len(),
            std::mem::size_of::<crate::Slot<ReceiverOwner>>(),
        )?
        .checked_add(array(
            self.receivers.free.len(),
            std::mem::size_of::<u64>(),
        )?)
        .ok_or(Error::Capacity)?;
        let expression_slots = array(
            self.expressions.slots.len(),
            std::mem::size_of::<crate::Slot<crate::ExpressionOwner>>(),
        )?;
        let expression_scratch = array(
            self.receiver_changes.capacity(),
            std::mem::size_of::<(ExpressionId, Expression)>(),
        )?
        .checked_add(array(
            self.receiver_component_changes.len(),
            std::mem::size_of::<Option<ReceiverComponents>>(),
        )?)
        .ok_or(Error::Capacity)?;
        let policy_scratch = array(
            self.receiver_policy_changes.capacity(),
            std::mem::size_of::<(ReceiverId, GestureProposal)>(),
        )?;
        registry
            .checked_add(expression_slots)
            .and_then(|n| n.checked_add(expression_scratch))
            .and_then(|n| n.checked_add(policy_scratch))
            .and_then(|n| n.checked_add(state_heap))
            .ok_or(Error::Capacity)?;
        Ok(ReceiverMemory {
            registry,
            expression_slots,
            expression_scratch,
            policy_scratch,
            state_heap,
        })
    }
}

impl Runtime {
    fn validate_physical_event_add(&self,plan:crate::PlanId,count:usize)->Result<(),Error>{
        self.plans.get(plan.0).ok_or(Error::StaleHandle)?.physical_events.checked_add(count).ok_or(Error::Capacity)?;Ok(())
    }
    fn retain_physical_event(&mut self,plan:crate::PlanId)->Result<(),Error>{
        let generation=self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        generation.physical_events=generation.physical_events.checked_add(1).ok_or(Error::Capacity)?;Ok(())
    }
    fn release_physical_event(&mut self,plan:crate::PlanId)->Result<(),Error>{
        let generation=self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        generation.physical_events=generation.physical_events.checked_sub(1).ok_or(Error::InvalidInput)?;Ok(())
    }
    fn validate_physical_event_row(&self,pair:PhysicalPair)->Result<(),Error>{
        if let Some(plan)=pair.event_plan {
            if self.plans.get(plan.0).ok_or(Error::StaleHandle)?.physical_events==0{return Err(Error::InvalidInput);}
        }Ok(())
    }
    fn physical_terminal(&self,pair:PhysicalPair,velocity:Option<f64>)->QueuedTerminal {
        QueuedTerminal{release:TerminalRelease{note:pair.note,source:pair.source,velocity},event:pair.event_plan.map(|plan|
            PhysicalEventContext{plan,execution:pair.execution,original_frame:self.now})}
    }
    /// Synthetic receipts copy references; actual Off transfers the row's one
    /// reference. Preflight the full per-generation sum, never independent fits.
    fn validate_physical_terminal_copies(&self,id:ReceiverId,matches:impl Fn(PhysicalPair)->bool)->Result<(),Error>{
        let state=self.receiver(id)?;
        for (index,slot) in self.plans.slots.iter().enumerate(){
            let Some(generation)=slot.value.as_ref()else{continue;};let plan=crate::PlanId(self.plans.id(index));
            let count=state.physical.iter().filter(|pair|pair.event_plan==Some(plan)&&matches(**pair)).count();
            generation.physical_events.checked_add(count).ok_or(Error::Capacity)?;
        }
        for pair in &state.physical{if matches(*pair){self.validate_physical_event_row(*pair)?;}}Ok(())
    }
    fn queue_physical_terminal_copy(&mut self,id:ReceiverId,pair:PhysicalPair,velocity:Option<f64>)->Result<(),Error>{
        let receipt=self.physical_terminal(pair,velocity);
        if let Some(event)=receipt.event{self.retain_physical_event(event.plan)?;}
        self.receiver_state_mut(id)?.terminal_releases.push(receipt);Ok(())
    }
}

impl Runtime {
    fn receiver_physical_delivery(&self,delivery:ReceiverTerminalDelivery)->Result<(QueuedTerminal,bool),Error>{
        let owner=self.receivers.get(delivery.receiver.0).ok_or(Error::StaleHandle)?;
        let (_,receipt,started)=owner.terminal_delivery.filter(|(sequence,_,_)|*sequence==delivery.sequence).ok_or(Error::StaleHandle)?;
        Ok((receipt,started))
    }
    /// Actual retained generation; None identifies the existing nonexternal
    /// legacy receipt. Caller-visible release fields are diagnostic only: the
    /// checked receipt sequence remains the authority.
    pub fn receiver_terminal_plan(&self,delivery:ReceiverTerminalDelivery)->Result<Option<crate::PlanId>,Error>{
        Ok(self.receiver_physical_delivery(delivery)?.0.event.map(|event|event.plan))
    }
    /// Begin one real plan-owned physical Event callback after the input NoteId
    /// may already have retired. No note is resurrected and no old expression
    /// inheritance is implied. Fresh controller epochs are captured NOW.
    pub fn start_receiver_release_behavior(&mut self,delivery:ReceiverTerminalDelivery,program:usize,stage:usize)->Result<crate::BehaviorId,Error>{
        if self.external_execution_aborted{return Err(Error::Canceled);}
        let (receipt,started)=self.receiver_physical_delivery(delivery)?;
        if started{return Err(Error::Busy);}
        let event=receipt.event.ok_or(Error::InvalidInput)?;
        if event.original_frame!=self.now{return Err(Error::PastEvent);}
        if stage>=self.plans.get(event.plan.0).ok_or(Error::StaleHandle)?.prepared.stage_count(){return Err(Error::InvalidInput);}
        let state=self.receiver(delivery.receiver)?;
        let performance=self.performance_index(state.performance)?;
        let origin=event.execution.channel_address();let source=receipt.source.channel_address();
        let scope=ChannelScope{protocol:origin.protocol,port:origin.port,group:origin.group,channels:u16::MAX};
        let policy_epochs=self.performance_state.epochs(performance,scope);
        let context=crate::behavior::NoteControllerContext{performance,origin,source,epoch:policy_epochs[usize::from(origin.channel)],stage,
            receiver:Some(delivery.receiver),policy_epochs,host_epochs:self.receiver_host_epochs(delivery.receiver,performance)?,
            physical_event:Some(PhysicalReleaseEvent{plan:event.plan,physical_note:receipt.note,source:receipt.source,execution:event.execution,
                original_frame:event.original_frame,velocity:receipt.velocity})};
        // validate/admit uses the real preallocated behavior arena and reserves
        // normal generation.callbacks before the physical receipt can be ACKed.
        let behavior=self.admit_plan_context(event.plan,program,crate::behavior::PlanContext::NoteControllers(context))?;
        let owner=self.receivers.get_mut(delivery.receiver.0).ok_or(Error::StaleHandle)?;
        let active=owner.terminal_delivery.as_mut().ok_or(Error::StaleHandle)?;
        active.2=true;
        self.resume_behavior(behavior);Ok(behavior)
    }
    /// A dead/replaced external owner explicitly cancels this real receipt by
    /// completing it. It must never resolve the old processor in a new plan.
    pub fn cancel_receiver_physical_event(&mut self,delivery:ReceiverTerminalDelivery)->Result<(),Error>{
        let (_,started)=self.receiver_physical_delivery(delivery)?;
        if started&&!self.external_execution_aborted
{return Err(Error::Busy);}
        self.complete_receiver_terminal(delivery)
    }
    pub(crate) fn pending_physical_event_frame(&self)->Option<u64>{
        self.receivers.slots.iter().filter_map(|slot|slot.value.as_ref()).flat_map(|owner|
            owner.state.terminal_releases.iter().filter_map(|receipt|receipt.event.map(|event|event.original_frame))
                .chain(owner.terminal_delivery.iter().filter_map(|(_,receipt,_)|receipt.event.map(|event|event.original_frame))))
            .min()
    }
}
/// Plan-owned physical Event creator. It carries actual immutable input metadata
/// without treating a historical NoteId as a live parent or expression owner.
#[derive(Clone,Copy,Debug)]
pub struct PhysicalReleaseEvent {
    /// Historical generational identity copied from this exact physical receipt.
    /// It is an opaque event-map key, not a live parent or expression lease.
    pub physical_note:NoteId,
    pub plan:crate::PlanId,pub source:Input,pub execution:Input,pub original_frame:u64,pub velocity:Option<f64>,
}
impl Runtime {
    /// Recover the actual in-flight sequence receipt after worker fault/cancel.
    /// This reads the real delivery Slot; diagnostic NoteId/source fields never
    /// reconstruct a receipt or a physical key-up obligation.
    pub fn active_receiver_terminal(&self,id:ReceiverId)->Result<Option<ReceiverTerminalDelivery>,Error>{
        let owner=self.receivers.get(id.0).ok_or(Error::StaleHandle)?;
        Ok(owner.terminal_delivery.map(|(sequence,receipt,_)|ReceiverTerminalDelivery{release:receipt.release,receiver:id,sequence}))
    }
}
impl Runtime {
    /// Bind a public physical-controller call to an actual registered owner,
    /// never reconstruct an identity from a port. The declared target mask and
    /// optional Native scope are preserved; this API does not invent a bridge.
    pub(crate) fn capture_registered_controller(
        &self,
        performance: usize,
        origin: ChannelAddress,
        channels: u16,
        number: u8,
        secondary: Option<ChannelScope>,
    ) -> Result<Option<CapturedReceiver>, Error> {
        let domain = match OwnedReceiver::domain(origin.protocol) {
            Ok(domain) => domain,
            Err(_) => return Ok(None),
        };
        let performance_id = self.performance(performance)?;
        let mut found = None;
        for (index, slot) in self.receivers.slots.iter().enumerate() {
            let Some(owner) = slot.value.as_ref() else {
                continue;
            };
            let state = &owner.state;
            if state.performance != performance_id
                || state.port != origin.port
                || state.group != origin.group
            {
                continue;
            }
            let selected = state.wire[0].parameters[usize::from(origin.channel)];
            let mcm = domain == ChannelDomain::Midi1
                && matches!(origin.channel, 0 | 15)
                && (matches!(number, 98..=101)
                    || selected.registered
                        && selected.rpn == [0, 6]
                        && matches!(number, 6 | 38 | 96 | 97));
            if !state.accepts_wire_channel(domain, origin.channel) && !mcm {
                continue;
            }
            let owned = if mcm {
                u16::MAX
            } else {
                state.controller_channels(domain, origin.channel)
            };
            if channels & !owned != 0 {
                return Err(Error::InvalidInput);
            }
            if let Some(scope) = secondary {
                if !state.host_projection
                    || scope.protocol != Protocol::Native
                    || scope.port != state.port
                    || scope.group != state.group
                    || scope.channels
                        & !state.controller_channels(ChannelDomain::HostChannel, origin.channel)
                        != 0
                {
                    return Err(Error::InvalidInput);
                }
            }
            if found.is_some() {
                return Err(Error::InvalidInput);
            }
            let id = ReceiverId(self.receivers.id(index));
            let scope = ChannelScope {
                protocol: origin.protocol,
                port: origin.port,
                group: origin.group,
                channels,
            };
            found = Some(CapturedReceiver {
                receiver: id,
                kind: ReceiverActionKind::Controller,
                provenance: ReceiverProvenance::Script,
                source: origin,
                policy_epochs: self.performance_state.epochs(performance, scope),
                host_epochs: state.host_projection.then(|| {
                    self.performance_state.epochs(
                        performance,
                        ChannelScope {
                            protocol: Protocol::Native,
                            port: state.port,
                            group: state.group,
                            channels: u16::MAX,
                        },
                    )
                }),
            });
        }
        Ok(found)
    }
}

impl Runtime {
    pub(crate) fn capture_registered_program(&self,performance:usize,origin:ChannelAddress,channels:u16)->Result<Option<ReceiverId>,Error>{
        let Ok(domain)=OwnedReceiver::domain(origin.protocol)else{return Ok(None);};
        let performance=self.performance(performance)?;let mut found=None;
        for (index,slot) in self.receivers.slots.iter().enumerate(){
            let Some(owner)=slot.value.as_ref()else{continue;};let state=&owner.state;
            if state.performance!=performance||state.port!=origin.port||state.group!=origin.group||!state.accepts_wire_channel(domain,origin.channel){continue;}
            if (0..16u8).any(|channel|channels&(1<<channel)!=0&&!state.accepts_wire_channel(domain,channel)){return Err(Error::InvalidInput);}
            if found.is_some(){return Err(Error::InvalidInput);}
            found=Some(ReceiverId(self.receivers.id(index)));
        }Ok(found)
    }
}


impl Runtime {
    fn checked_wire_epoch(&self,epoch:ReceiverWireEpoch)->Result<&OwnedReceiver,Error>{
        let state=self.receiver(epoch.receiver)?;
        if state.wire_epoch!=epoch.serial{return Err(Error::StaleHandle);}Ok(state)
    }
    pub fn receiver_wire_epoch(&self,id:ReceiverId)->Result<ReceiverWireEpoch,Error>{
        Ok(ReceiverWireEpoch{receiver:id,serial:self.receiver(id)?.wire_epoch})
    }
    /// Reserve at actual ingress BEFORE scheduling. No Note/Expr/Plan is owned.
    pub fn reserve_raw_attack(&mut self,epoch:ReceiverWireEpoch,intent:RawAttackIntent)->Result<RawAdmission,Error>{
        let state=self.checked_wire_epoch(epoch)?;state.validate_raw_intent(intent)?;
        if intent.frame>self.now{return Err(Error::PastEvent);}
        let active=usize::from(self.receivers.get(epoch.receiver.0).ok_or(Error::StaleHandle)?.terminal_delivery.is_some());
        let (decision,reason)=self.receiver_state_mut(epoch.receiver)?.reserve_wire_decision(intent,active)?;
        let decision=RawAdmissionId{receiver:epoch.receiver,decision};
        Ok(match reason{Some(reason)=>RawAdmission::Rejected{decision,reason},None=>RawAdmission::Pending(decision)})
    }
    pub fn raw_admission_phase(&self,id:RawAdmissionId)->Result<RawDecisionPhase,Error>{
        let state=self.receiver(id.receiver)?;let row=state.physical.decisions.get(id.decision).ok_or(Error::StaleHandle)?;
        if row.epoch!=state.wire_epoch{return Err(Error::StaleHandle);}Ok(row.phase())
    }
    /// Typed actual disposition, without applying due work or constructing IDs.
    pub fn raw_rejection(&self,id:RawAdmissionId)->Result<Option<RawRejection>,Error>{
        self.raw_admission_phase(id)?;
        Ok(match self.receiver(id.receiver)?.physical.decisions.get(id.decision).ok_or(Error::StaleHandle)?.phase{state::PhysicalPhase::Rejected(reason)=>Some(reason),_=>None})
    }
    pub fn reject_raw_attack(&mut self,id:RawAdmissionId,reason:RawRejection)->Result<(),Error>{
        self.raw_admission_phase(id)?;self.receiver_state_mut(id.receiver)?.reject_wire_decision(id.decision,reason)
    }
    pub fn commit_raw_attack(&mut self,id:RawAdmissionId,attack:CapturedLogicalRawAttack)->Result<NoteId,Error>{
        let state=self.receiver(id.receiver)?;let row=*state.physical.decisions.get(id.decision).ok_or(Error::StaleHandle)?;
        let state::PhysicalPhase::Pending(intent)=row.phase else{return Err(Error::StaleHandle);};
        let protocol=if intent.voice.version==Version::Midi2{Protocol::Midi2}else{Protocol::Midi1};
        if row.epoch!=state.wire_epoch{return Err(Error::StaleHandle);}
        if attack.input.protocol!=protocol||attack.input.external_id.is_some()||attack.input.group>=16||attack.input.channel>=16||attack.input.key>=128
            ||!attack.pitch.valid()||!attack.velocity.is_finite()||!(0.0..=1.0).contains(&attack.velocity)||!attack.tuning.is_finite(){return Err(Error::InvalidInput);}
        if self.external_execution_aborted{return Err(Error::Canceled);}
        self.apply_due();
        let result=self.admit_receiver_input_decision(id.receiver,attack.input,row.source,attack.pitch,attack.velocity,attack.tuning,attack.silent,Some(id.decision));
        if let Err(error)=result{self.receiver_state_mut(id.receiver)?.reject_wire_decision(id.decision,RawRejection::Core(error))?;}
        result
    }
    pub fn claim_raw_terminal(&mut self,epoch:ReceiverWireEpoch,source:Input,velocity:Option<f64>)->Result<Option<RawTerminalClaim>,Error>{
        self.checked_wire_epoch(epoch)?;
        let Some(decision)=self.receiver_state_mut(epoch.receiver)?.claim_wire_terminal(source,velocity)?else{return Ok(None);};
        let terminal=RawTerminalId{admission:RawAdmissionId{receiver:epoch.receiver,decision}};
        self.raw_terminal_claim(terminal).map(Some)
    }
    /// Pure recovery of the same real terminal claim from its actual On token.
    /// Used when a synchronous terminal observed a still-pending staged On.
    pub fn raw_admission_terminal(&self,id:RawAdmissionId)->Result<Option<RawTerminalClaim>,Error>{
        self.raw_admission_phase(id)?;
        if self.receiver(id.receiver)?.physical.decisions.get(id.decision).ok_or(Error::StaleHandle)?.terminal.is_none(){return Ok(None);}
        self.raw_terminal_claim(RawTerminalId{admission:id}).map(Some)
    }
    pub fn raw_terminal_claim(&self,terminal:RawTerminalId)->Result<RawTerminalClaim,Error>{
        let id=terminal.admission;let state=self.receiver(id.receiver)?;
        let row=state.physical.decisions.get(id.decision).ok_or(Error::StaleHandle)?;
        if row.epoch!=state.wire_epoch||row.terminal.is_none(){return Err(Error::StaleHandle);}
        Ok(RawTerminalClaim{terminal,phase:row.phase()})
    }
    pub fn commit_raw_terminal(&mut self,terminal:RawTerminalId)->Result<RawTerminalOutcome,Error>{
        self.raw_terminal_claim(terminal)?;let id=terminal.admission;
        let row=*self.receiver(id.receiver)?.physical.decisions.get(id.decision).ok_or(Error::StaleHandle)?;
        let velocity=row.terminal.ok_or(Error::StaleHandle)?;
        match row.phase{
            state::PhysicalPhase::Pending(_)=>Err(Error::NotReady),
            state::PhysicalPhase::Rejected(_)=>{self.receiver_state_mut(id.receiver)?.physical.decisions.remove(id.decision);Ok(RawTerminalOutcome::AbsorbedRejected)},
            state::PhysicalPhase::Accepted(pair)=>{
                if pair.kind!=PhysicalKind::Wire{return Err(Error::InvalidInput);}
                let active=usize::from(self.receivers.get(id.receiver.0).ok_or(Error::StaleHandle)?.terminal_delivery.is_some());
                self.receiver(id.receiver)?.validate_receipt_union(active,0)?;
                self.validate_physical_event_row(pair)?;
                self.publish_receiver_transaction(|runtime|{
                    match runtime.note_off_exact(pair.note,velocity){Ok(())|Err(Error::StaleHandle)|Err(Error::ClosedNote)=>{},Err(error)=>return Err(error)}
                    runtime.freeze_receiver_expression(id.receiver,pair.expression);
                    if !pair.released&&pair.event_plan.is_some(){
                        let receipt=runtime.physical_terminal(pair,velocity);runtime.receiver_state_mut(id.receiver)?.terminal_releases.push(receipt);
                    }else if let Some(plan)=pair.event_plan{runtime.release_physical_event(plan)?;}
                    runtime.receiver_state_mut(id.receiver)?.physical.decisions.remove(id.decision);
                    Ok(if pair.released{RawTerminalOutcome::AbsorbedAlreadySynthetic}else{RawTerminalOutcome::Released(pair.note)})
                })
            }
        }
    }
    pub fn receiver_has_unclaimed_wire_source(&self,id:ReceiverId,source:Input)->Result<bool,Error>{Ok(self.receiver(id)?.has_wire_source(source))}
    pub fn receiver_wire_status(&self,id:ReceiverId)->Result<WireAdmissionStatus,Error>{
        let owner=self.receivers.get(id.0).ok_or(Error::StaleHandle)?;let state=&owner.state;
        let mut pending=0;let mut accepted=0;let mut rejected=0;let mut claimed=0;
        for row in state.physical.decisions.slots.iter().filter_map(|slot|slot.value){
            match row.phase{state::PhysicalPhase::Pending(_)=>pending+=1,state::PhysicalPhase::Accepted(pair)if pair.kind==PhysicalKind::Wire=>accepted+=1,state::PhysicalPhase::Rejected(_)=>rejected+=1,_=>{}}
            claimed+=usize::from(row.terminal.is_some());
        }
        Ok(WireAdmissionStatus{epoch:ReceiverWireEpoch{receiver:id,serial:state.wire_epoch},fenced_unknown_debt:state.wire_fenced,
            decisions:state.physical.decisions.count(),pending,accepted,rejected,claimed,decision_capacity:state.physical.capacity(),receipt_limit:state.terminal_limit,
            queued_receipts:state.terminal_releases.len(),active_receipts:usize::from(owner.terminal_delivery.is_some()),future_receipts:state.future_receipts()?,
            replacement_blocked:self.receiver_unbind_busy(id)?})
    }
}
