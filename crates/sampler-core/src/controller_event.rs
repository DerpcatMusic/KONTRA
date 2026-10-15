use crate::{
    BehaviorId, ChannelAddress, ChannelScope, Error, PerformanceId, PlanId, Prepared, Runtime,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct ControllerEvent {
    pub performance: usize,
    pub origin: ChannelAddress,
    pub channels: u16,
    pub number: u8,
    pub poly_key: Option<u8>,
    pub poly_recipients: Option<crate::InputSelector>,
    pub value: u32,
    pub pending: bool,
    pub stage: usize,
    pub reserved: usize,
}

impl ControllerEvent {
    fn program(self, stage: &crate::Stage) -> Option<usize> {
        if self.poly_key.is_some() { stage.poly_at } else { stage.controller }
    }
    fn scope(self) -> ChannelScope {
        ChannelScope {
            protocol: self.origin.protocol,
            port: self.origin.port,
            group: self.origin.group,
            channels: self.channels,
        }
    }
}

impl Prepared {
    /// Controller callbacks own their plan, captured input and performance domain.
    /// They cannot borrow a note or depend on a note gate for wait lifetime.
    pub fn with_controller_program(self, program: usize) -> Result<Self, Error> {
        self.with_controller_programs(vec![program])
    }

    /// Ordered controller stages. Generated writes enter the following stage;
    /// only the final projection changes downstream musical state. Like
    /// `with_stages`, changed routing requires parameter receivers to be cleared
    /// and explicitly rebound; reinstalling identical stages is accepted.
    pub fn with_controller_programs(self, programs: Vec<usize>) -> Result<Self, Error> {
        let mut stages = self.stages.to_vec();
        stages.resize(stages.len().max(programs.len()), crate::Stage::default());
        for stage in &mut stages {
            stage.controller = None;
        }
        for (stage, program) in stages.iter_mut().zip(programs) {
            stage.controller = Some(program);
        }
        self.with_stages(stages)
    }
}

impl Runtime {
    /// `origin` is the captured physical input address; `channels` selects the
    /// downstream targets on that port/group (for example, an entire MPE zone).
    /// The origin channel need not be among those targets.
    ///
    /// Admit a CC before projecting it into musical state. A bound callback decides
    /// whether/when to forward; otherwise forwarding is immediate. Failed admission
    /// leaves both input and downstream values unchanged. Callback faults are retained
    /// outcomes, and do not roll back already-published musical side effects.
    pub fn dispatch_controller(
        &mut self,
        performance: PerformanceId,
        origin: ChannelAddress,
        channels: u16,
        number: u8,
        value: u32,
    ) -> Result<Option<BehaviorId>, Error> {
        let context = crate::ControlContext {
            performance,
            origin,
            channels,
        };
        let performance = self.performance_index(performance)?;
        if number >= 130 || origin.group >= 16 || origin.channel >= 16 || channels == 0 {
            return Err(Error::InvalidInput);
        }
        let event = ControllerEvent {
            performance,
            origin,
            channels,
            number,
            poly_key: None,
            poly_recipients: None,
            value,
            pending: true,
            stage: 0,
            reserved: 0,
        };
        self.apply_due();
        let needed = self
            .plans
            .get(self.active_plan.0)
            .unwrap()
            .prepared
            .stages
            .iter()
            .filter(|s| s.controller.is_some())
            .count();
        if needed == 0 && matches!(number, 64 | 66) && value >= 0x8000_0000 {
            self.missing_pedal_channels(event.scope())?;
        }
        self.dispatch_automation(
            context,
            self.active_plan,
            crate::AutomationSource::Controller(number),
            f64::from(value) / f64::from(u32::MAX),
            needed,
        )?;
        self.admit_controller(self.active_plan, event)
    }

    /// Admit a poly-pressure input before changing note expression. A callback
    /// may consume or delay the event; captured key/value survive waits and
    /// downstream modules receive only forwarded inputs.
    pub fn dispatch_poly_pressure(
        &mut self,
        performance: PerformanceId,
        origin: ChannelAddress,
        key: u8,
        value: u32,
    ) -> Result<(Option<BehaviorId>, usize), Error> {
        self.dispatch_poly_pressure_selected(performance, origin, key, value, None)
    }

    /// A host adapter may preserve physical recipients separately from its
    /// mapped script channel. The selector survives callback waits unchanged.
    pub fn dispatch_poly_pressure_selected(
        &mut self,
        performance: PerformanceId,
        origin: ChannelAddress,
        key: u8,
        value: u32,
        recipients: Option<crate::InputSelector>,
    ) -> Result<(Option<BehaviorId>, usize), Error> {
        let performance = self.performance_index(performance)?;
        if key >= 128 || origin.group >= 16 || origin.channel >= 16
            || recipients.is_some_and(|s| s.key.is_some_and(|v| v >= 128)
                || s.channel.is_some_and(|v| v >= 16) || s.group.is_some_and(|v| v >= 16)) {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        let previous = self.poly_pressure_owners;
        let callback = self.admit_controller(self.active_plan, ControllerEvent {
            performance, origin, channels: 1 << origin.channel, number: 130,
            poly_key: Some(key), poly_recipients: recipients, value, pending: true, stage: 0, reserved: 0,
        })?;
        let owners = self.poly_pressure_owners.wrapping_sub(previous);
        Ok((callback, owners))
    }

    fn admit_controller(
        &mut self,
        plan: PlanId,
        mut event: ControllerEvent,
    ) -> Result<Option<BehaviorId>, Error> {
        let stages = &self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .stages;
        let next = stages
            .iter()
            .enumerate()
            .skip(event.stage)
            .find_map(|(index, stage)| event.program(stage).map(|program| (index, program)));
        if let Some((next, program)) = next {
            let needed = stages[event.stage..]
                .iter()
                .filter(|stage| event.program(stage).is_some())
                .count();
            if !self.behavior_room(needed) {
                return Err(Error::Capacity);
            }
            self.validate_plan_context(
                plan,
                program,
                super::behavior::PlanContext::Controller(event),
            )?;
            event.reserved = needed - 1;
            self.behaviors.reserve(event.reserved);
            self.receive_controller_path(plan, event, next + 1);
            event.stage = next;
            Ok(Some(
                self.start_plan_context(
                    plan,
                    program,
                    super::behavior::PlanContext::Controller(event),
                )
                .expect("preflighted controller chain admission"),
            ))
        } else {
            let end = stages.len();
            self.publish_input_event(event)?;
            self.receive_controller_path(plan, event, end);
            Ok(None)
        }
    }

    fn receive_controller_path(&mut self, plan: PlanId, event: ControllerEvent, end: usize) {
        if event.stage == 0 {
            if let Some(key) = event.poly_key {
                self.performance_state.input_poly_pressure[event.performance][usize::from(key)] = event.value;
            } else {
                self.performance_state.input_controllers[event.performance][usize::from(event.number)] = event.value;
            }
        }
        let state = &mut self.plans.get_mut(plan.0).unwrap().controllers;
        for stage in event.stage.max(1)..end {
            state.receive(ControllerEvent { stage, ..event });
        }
    }

    pub(super) fn release_controller_reserve(&mut self, id: BehaviorId) {
        if let super::behavior::PlanContext::Controller(event) =
            &mut self.behaviors.get_mut(id.0).unwrap().context
        {
            let reserved = std::mem::take(&mut event.reserved);
            self.behaviors.unreserve(reserved);
        }
    }

    pub(super) fn behavior_input_controller(
        &self,
        id: BehaviorId,
        number: u8,
    ) -> Result<u32, Error> {
        if number >= 130 {
            return Err(Error::InvalidInput);
        }
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let stage = self.behavior_stage(id)?;
        let (performance, _) = self.behavior_performance(id)?;
        if stage != 0 {
            let plan = self.behavior_plan(callback.owner)?;
            let generation = self.plans.get(plan.0).unwrap();
            if stage == generation.prepared.stages.len() {
                return self.controller(self.performance(performance)?, number);
            }
            return Ok(generation.controllers.bank(stage, performance)[usize::from(number)]);
        }
        Ok(self.performance_state.input_controllers[performance][usize::from(number)])
    }

    pub(super) fn write_behavior_controller(
        &mut self,
        id: BehaviorId,
        number: u8,
        value: u32,
    ) -> Result<(), Error> {
        if number >= 130 {
            return Err(Error::InvalidInput);
        }
        let callback = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        if let super::behavior::PlanContext::Controller(mut event) = callback.context {
            event.stage += 1;
            event.number = number;
            event.poly_key = None;
            event.poly_recipients = None;
            event.value = value;
            event.pending = true;
            event.reserved = 0;
            self.admit_controller(self.behavior_plan(callback.owner)?, event)?;
            Ok(())
        } else if let super::behavior::PlanContext::Control(event) = callback.context {
            self.admit_controller(
                self.behavior_plan(callback.owner)?,
                ControllerEvent {
                    performance: event.performance,
                    origin: event.origin,
                    channels: event.channels,
                    number,
                    poly_key: None,
                    poly_recipients: None,
                    value,
                    stage: event.stage + 1,
                    pending: true,
                    reserved: 0,
                },
            )?;
            Ok(())
        } else if let Some(stage) = callback.note_stage {
            let note = callback.owner.note()?;
            let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
            self.admit_controller(
                n.plan,
                ControllerEvent {
                    performance: self.selections[note.0.index].performance,
                    origin: n.address,
                    channels: 1 << n.address.channel,
                    number,
                    poly_key: None,
                    poly_recipients: None,
                    value,
                    stage: stage.index() + 1,
                    pending: true,
                    reserved: 0,
                },
            )?;
            Ok(())
        } else {
            let (performance, scope) = self.behavior_performance(id)?;
            self.publish_controller(performance, scope, number, value)
        }
    }

    /// Latest admitted input CC, including events consumed by a callback. This is
    /// distinct from `controller`, the value visible to downstream region selection.
    pub fn input_controller(&self, performance: PerformanceId, number: u8) -> Result<u32, Error> {
        self.performance_state.input_controllers[self.performance_index(performance)?]
            .get(usize::from(number))
            .copied()
            .ok_or(Error::InvalidInput)
    }

    pub(super) fn controller_event_mut(
        &mut self,
        id: BehaviorId,
    ) -> Result<&mut ControllerEvent, Error> {
        match &mut self
            .behaviors
            .get_mut(id.0)
            .ok_or(Error::StaleHandle)?
            .context
        {
            super::behavior::PlanContext::Controller(event) => Ok(event),
            _ => Err(Error::InvalidInput),
        }
    }

    pub(super) fn behavior_performance(
        &self,
        id: BehaviorId,
    ) -> Result<(usize, ChannelScope), Error> {
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        if let super::behavior::PlanContext::Controller(event) = callback.context {
            return Ok((event.performance, event.scope()));
        }
        if let super::behavior::PlanContext::Control(event) = callback.context {
            return Ok((event.performance, event.scope()));
        }
        if let crate::BehaviorOwner::Note(note) = callback.owner {
            let address = self.notes.get(note.0).ok_or(Error::StaleHandle)?.address;
            return Ok((
                self.selections[note.0.index].performance,
                ChannelScope {
                    protocol: address.protocol,
                    port: address.port,
                    group: address.group,
                    channels: 1 << address.channel,
                },
            ));
        }
        Err(Error::InvalidInput)
    }

    pub(super) fn forward_controller(&mut self, id: BehaviorId) -> Result<(), Error> {
        let event = *self.controller_event_mut(id)?;
        if event.pending {
            let owner = self.behaviors.get(id.0).unwrap().owner;
            let plan = self.behavior_plan(owner)?;
            let stages = &self.plans.get(plan.0).unwrap().prepared.stages;
            let next = stages
                .iter()
                .enumerate()
                .skip(event.stage + 1)
                .find_map(|(index, stage)| event.program(stage).map(|program| (index, program)));
            let end = stages.len();
            if let Some((next, program)) = next {
                let remaining = event
                    .reserved
                    .checked_sub(1)
                    .expect("owned downstream controller slot");
                let current = self.controller_event_mut(id)?;
                current.pending = false;
                current.reserved = 0;
                self.behaviors.unreserve(1);
                self.receive_controller_path(
                    plan,
                    ControllerEvent {
                        stage: event.stage + 1,
                        ..event
                    },
                    next + 1,
                );
                self.start_plan_context(
                    plan,
                    program,
                    super::behavior::PlanContext::Controller(ControllerEvent {
                        stage: next,
                        reserved: remaining,
                        ..event
                    }),
                )
                .expect("reserved controller continuation");
            } else {
                self.publish_input_event(event)?;
                self.receive_controller_path(
                    plan,
                    ControllerEvent {
                        stage: event.stage + 1,
                        ..event
                    },
                    end,
                );
                self.controller_event_mut(id)?.pending = false;
            }
        }
        Ok(())
    }

    pub(super) fn publish_controller(
        &mut self,
        performance: usize,
        scope: ChannelScope,
        number: u8,
        value: u32,
    ) -> Result<(), Error> {
        if number >= 130 {
            return Err(Error::InvalidInput);
        }
        let performance = self.performance(performance)?;
        if matches!(number, 64 | 66) {
            self.set_pedal_controller(performance, scope, number, value)
        } else {
            self.set_controller(performance, number, value)
        }
    }

    fn publish_input_event(&mut self, event: ControllerEvent) -> Result<(), Error> {
        if let Some(key) = event.poly_key {
            let update = |e| crate::Expression { pressure: event.value, ..e };
            let owners = if let Some(selector) = event.poly_recipients {
                self.set_source_expressions(selector, update)?
            } else {
                self.set_input_expressions(event.origin, Some(key), update)?
            };
            self.poly_pressure_owners = self.poly_pressure_owners.wrapping_add(owners);
            self.performance_state.poly_pressure[event.performance][usize::from(key)] = event.value;
            Ok(())
        } else {
            self.publish_controller(event.performance, event.scope(), event.number, event.value)
        }
    }

    /// Latest projected per-key input in this callback's stage, including the
    /// currently consumed input. This differs from downstream expression state.
    pub(super) fn behavior_poly_pressure(&self, id: BehaviorId, key: u8) -> Result<u32, Error> {
        if key >= 128 { return Err(Error::InvalidInput); }
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let stage = self.behavior_stage(id)?;
        let (performance, _) = self.behavior_performance(id)?;
        if stage == 0 { return Ok(self.performance_state.input_poly_pressure[performance][usize::from(key)]); }
        let plan = self.plans.get(self.behavior_plan(callback.owner)?.0).unwrap();
        if stage == plan.prepared.stages.len() {
            Ok(self.performance_state.poly_pressure[performance][usize::from(key)])
        } else {
            Ok(plan.controllers.poly_bank(stage, performance)[usize::from(key)])
        }
    }

    pub fn input_poly_pressure(&self, performance: PerformanceId, key: u8) -> Result<u32, Error> {
        self.performance_state.input_poly_pressure[self.performance_index(performance)?]
            .get(usize::from(key)).copied().ok_or(Error::InvalidInput)
    }

    pub fn reset_poly_pressure(&mut self, performance: PerformanceId) -> Result<(), Error> {
        let performance = self.performance_index(performance)?;
        self.performance_state.input_poly_pressure[performance].fill(0);
        self.performance_state.poly_pressure[performance].fill(0);
        for slot in &mut self.plans.slots {
            if let Some(plan) = &mut slot.value { plan.controllers.reset_poly(performance); }
        }
        Ok(())
    }
}

/// Generation-owned projected CC inputs. Stage zero uses the runtime's raw input
/// bank; later stages update only when an event actually reaches that stage.
pub(super) struct ControllerState {
    banks: Box<[[u32; 130]]>,
    poly_banks: Box<[[u32; 128]]>,
    performances: usize,
}
impl ControllerState {
    pub fn new(prepared: &Prepared, performances: usize) -> Result<Self, Error> {
        let count = prepared
            .stages
            .len()
            .saturating_sub(1)
            .checked_mul(performances)
            .ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 130]>(count).map_err(|_| Error::Capacity)?;
        let mut banks = Vec::new();
        banks
            .try_reserve_exact(count)
            .map_err(|_| Error::Capacity)?;
        let mut reset = [0; 130];
        reset[128] = 0x8000_0000;
        banks.resize(count, reset);
        Ok(Self {
            banks: banks.into_boxed_slice(),
            poly_banks: vec![[0; 128]; count].into_boxed_slice(),
            performances,
        })
    }
    pub fn bank(&self, stage: usize, performance: usize) -> &[u32; 130] {
        &self.banks[(stage - 1) * self.performances + performance]
    }
    pub fn receive(&mut self, event: ControllerEvent) {
        let index = (event.stage - 1) * self.performances + event.performance;
        if let Some(key) = event.poly_key { self.poly_banks[index][usize::from(key)] = event.value; }
        else { self.banks[index][usize::from(event.number)] = event.value; }
    }
    pub fn poly_bank(&self, stage: usize, performance: usize) -> &[u32; 128] {
        &self.poly_banks[(stage - 1) * self.performances + performance]
    }
    fn reset_poly(&mut self, performance: usize) {
        for bank in self.poly_banks.iter_mut().skip(performance).step_by(self.performances) { bank.fill(0); }
    }
}
