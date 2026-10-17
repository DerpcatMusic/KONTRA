use crate::{
    BehaviorId, ChannelAddress, ChannelScope, Error, PerformanceId, PlanId, Prepared, Runtime,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct ControllerEvent {
    pub performance: usize,
    pub origin: ChannelAddress,
    pub channels: u16,
    /// Captured target-bank revisions survive controller waits and plan changes.
    pub epochs: [u64; 16],
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
        self.apply_due();
        let event = ControllerEvent {
            performance,
            origin,
            channels,
            epochs: self.performance_state.epochs(performance, ChannelScope {
                protocol: origin.protocol, port: origin.port, group: origin.group, channels }),
            number,
            poly_key: None,
            poly_recipients: None,
            value,
            pending: true,
            stage: 0,
            reserved: 0,
        };
        self.validate_controller_event(self.active_plan, event)?;
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
            performance, origin, channels: 1 << origin.channel,
            epochs: self.performance_state.epochs(performance, ChannelScope {
                protocol: origin.protocol, port: origin.port, group: origin.group, channels: 1 << origin.channel }),
            number: 130,
            poly_key: Some(key), poly_recipients: recipients, value, pending: true, stage: 0, reserved: 0,
        })?;
        let owners = self.poly_pressure_owners.wrapping_sub(previous);
        Ok((callback, owners))
    }

    fn validate_controller_event(&self, plan: PlanId, event: ControllerEvent) -> Result<(), Error> {
        let scope = ChannelScope { channels: event.channels | (1 << event.origin.channel), ..event.scope() };
        self.performance_state.validate_scope(event.performance, scope)?;
        self.plans.get(plan.0).ok_or(Error::StaleHandle)?.controllers.validate_scope(event.performance, scope)
    }

    fn controller_origin(&self, id: BehaviorId) -> Result<Option<ChannelAddress>, Error> {
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        Ok(match callback.context {
            super::behavior::PlanContext::Controller(event) => Some(event.origin),
            super::behavior::PlanContext::Control(event) => event.controller_epochs.map(|_| event.origin),
            _ => match callback.owner {
                crate::BehaviorOwner::Note(note) => Some(self.notes.get(note.0).ok_or(Error::StaleHandle)?.address),
                _ => None,
            },
        })
    }

    pub(super) fn controller_target_epochs(&self, id: BehaviorId) -> Result<Option<[u64; 16]>, Error> {
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        Ok(match callback.context {
            super::behavior::PlanContext::Controller(event) => Some(event.epochs),
            super::behavior::PlanContext::Control(event) => event.controller_epochs,
            super::behavior::PlanContext::Bare => match callback.owner {
                crate::BehaviorOwner::Note(note) => {
                    let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
                    let mut epochs = self.performance_state.epochs(self.selections[note.0.index].performance, ChannelScope {
                        protocol: n.address.protocol, port: n.address.port, group: n.address.group, channels: 1 << n.address.channel });
                    epochs[usize::from(n.address.channel)] = self.selections[note.0.index].controller_epoch;
                    Some(epochs)
                },
                _ => None,
            },
        })
    }

    fn admit_controller(
        &mut self,
        plan: PlanId,
        mut event: ControllerEvent,
    ) -> Result<Option<BehaviorId>, Error> {
        event.channels = self.performance_state.surviving_scope(event.performance, event.scope(), event.epochs).channels;
        if event.channels == 0 { return Ok(None); }
        self.validate_controller_event(plan, event)?;
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
        let scope = self.performance_state.surviving_scope(event.performance, event.scope(), event.epochs);
        if scope.channels == 0 { return; }
        let origin_live = self.performance_state.epochs(event.performance, scope)[usize::from(event.origin.channel)]
            == event.epochs[usize::from(event.origin.channel)];
        let observed = ChannelScope { channels: scope.channels |
            if origin_live { 1 << event.origin.channel } else { 0 }, ..scope };
        if event.stage == 0 {
            if let Some(key) = event.poly_key {
                self.performance_state.scoped_poly(event.performance, observed, key, event.value, true);
            } else {
                self.performance_state.scoped_input(event.performance, observed, event.number, event.value);
            }
        }
        let state = &mut self.plans.get_mut(plan.0).unwrap().controllers;
        for stage in event.stage.max(1)..end {
            state.receive(ControllerEvent { stage, channels: observed.channels, ..event });
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
        let origin = self.controller_origin(id)?;
        if stage != 0 {
            let plan = self.behavior_plan(callback.owner)?;
            let generation = self.plans.get(plan.0).unwrap();
            if stage == generation.prepared.stages.len() {
                return match origin {
                    Some(address) => self.channel_controller(self.performance(performance)?, address, number),
                    None => self.controller(self.performance(performance)?, number),
                };
            }
            return Ok(generation.controllers.bank(stage, performance, origin)[usize::from(number)]);
        }
        Ok(match origin {
            Some(address) => self.performance_state.input_bank(performance, address)[usize::from(number)],
            None => self.performance_state.input_controllers[performance][usize::from(number)],
        })
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
            let Some(epochs) = event.controller_epochs else {
                return self.set_controller(self.performance(event.performance)?, number, value);
            };
            self.admit_controller(
                self.behavior_plan(callback.owner)?,
                ControllerEvent {
                    performance: event.performance,
                    origin: event.origin,
                    channels: event.channels,
                    epochs,
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
                    epochs: self.controller_target_epochs(id)?.ok_or(Error::InvalidInput)?,
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
            let scope = match self.controller_target_epochs(id)? {
                Some(epochs) => self.performance_state.surviving_scope(performance, scope, epochs),
                None => scope,
            };
            if scope.channels == 0 { return Ok(()); }
            self.publish_controller(performance, scope, number, value)
        }
    }

    /// Latest admitted raw controller in one explicit physical domain.
    /// Global input_controller remains the separate channel-less API domain.
    pub fn channel_input_controller(&self, performance: PerformanceId, address: ChannelAddress, number: u8) -> Result<u32, Error> {
        if address.group >= 16 || address.channel >= 16 { return Err(Error::InvalidInput); }
        self.performance_state.input_bank(self.performance_index(performance)?, address)
            .get(usize::from(number)).copied().ok_or(Error::InvalidInput)
    }

    pub fn channel_poly_pressure(&self, performance: PerformanceId, address: ChannelAddress, key: u8, input: bool) -> Result<u32, Error> {
        if address.group >= 16 || address.channel >= 16 { return Err(Error::InvalidInput); }
        self.performance_state.poly_bank(self.performance_index(performance)?, address, input)
            .get(usize::from(key)).copied().ok_or(Error::InvalidInput)
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
        self.performance_state.validate_scope(performance, scope)?;
        if matches!(number, 64 | 66) {
            if number == 64 { self.sustain_scope(scope, value >= 0x8000_0000)?; }
            else { self.sostenuto_scope(scope, value >= 0x8000_0000)?; }
        }
        self.performance_state.scoped_controller(performance, scope, number, value)
    }

    fn publish_input_event(&mut self, event: ControllerEvent) -> Result<(), Error> {
        let scope = self.performance_state.surviving_scope(event.performance, event.scope(), event.epochs);
        if scope.channels == 0 { return Ok(()); }
        if let Some(key) = event.poly_key {
            let update = |e| crate::Expression { pressure: event.value, ..e };
            let owners = if let Some(selector) = event.poly_recipients {
                self.set_source_expressions(selector, update)?
            } else {
                self.set_input_expressions(event.origin, Some(key), update)?
            };
            self.poly_pressure_owners = self.poly_pressure_owners.wrapping_add(owners);
            self.performance_state.scoped_poly(event.performance, scope, key, event.value, false);
            Ok(())
        } else {
            self.publish_controller(event.performance, scope, event.number, event.value)
        }
    }

    /// Latest projected per-key input in this callback's stage, including the
    /// currently consumed input. This differs from downstream expression state.
    pub(super) fn behavior_poly_pressure(&self, id: BehaviorId, key: u8) -> Result<u32, Error> {
        if key >= 128 { return Err(Error::InvalidInput); }
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let stage = self.behavior_stage(id)?;
        let (performance, _) = self.behavior_performance(id)?;
        let origin = self.controller_origin(id)?;
        if stage == 0 { return Ok(origin.map_or(self.performance_state.input_poly_pressure[performance][usize::from(key)],
            |address| self.performance_state.poly_bank(performance, address, true)[usize::from(key)])); }
        let plan = self.plans.get(self.behavior_plan(callback.owner)?.0).unwrap();
        if stage == plan.prepared.stages.len() {
            Ok(origin.map_or(self.performance_state.poly_pressure[performance][usize::from(key)],
                |address| self.performance_state.poly_bank(performance, address, false)[usize::from(key)]))
        } else {
            Ok(plan.controllers.poly_bank(stage, performance, origin)[usize::from(key)])
        }
    }

    pub fn input_poly_pressure(&self, performance: PerformanceId, key: u8) -> Result<u32, Error> {
        self.performance_state.input_poly_pressure[self.performance_index(performance)?]
            .get(usize::from(key)).copied().ok_or(Error::InvalidInput)
    }

    pub fn reset_poly_pressure(&mut self, performance: PerformanceId) -> Result<(), Error> {
        let performance = self.performance_index(performance)?;
        self.performance_state.reset_poly(performance);
        for slot in &mut self.plans.slots {
            if let Some(plan) = &mut slot.value { plan.controllers.reset_poly(performance); }
        }
        Ok(())
    }

    /// Clear stored input and downstream poly pressure only in these physical
    /// channels. Other ports, groups, protocols and Performance owners retain
    /// their values. Like the global reset, this does not synthesize callbacks.
    pub fn reset_channel_poly_pressure(&mut self, performance: PerformanceId, scope: ChannelScope) -> Result<(), Error> {
        let performance = self.performance_index(performance)?;
        if scope.group >= 16 || scope.channels == 0 { return Err(Error::InvalidInput); }
        self.performance_state.reset_poly_scope(performance, scope);
        for slot in &mut self.plans.slots {
            if let Some(plan) = &mut slot.value { plan.controllers.reset_poly_scope(performance, scope); }
        }
        Ok(())
    }
}

/// Generation-owned projected CC inputs. Stage zero uses the runtime's raw input
/// bank; later stages update only when an event actually reaches that stage.
pub(super) struct ControllerState {
    banks: Box<[[u32; 130]]>,
    poly_banks: Box<[[u32; 128]]>,
    physical_banks: Box<[[u32; 130]]>,
    physical_poly: Box<[[u32; 128]]>,
    physical: Vec<(usize, ChannelAddress)>,
    domains: usize,
    performances: usize,
    stages: usize,
}
impl ControllerState {
    pub fn new(prepared: &Prepared, performances: usize, channels: usize) -> Result<Self, Error> {
        let stages = prepared.stages.len().saturating_sub(1);
        let count = stages.checked_mul(performances).ok_or(Error::Capacity)?;
        let domains = performances.checked_mul(channels.max(16)).ok_or(Error::Capacity)?;
        let physical = stages.checked_mul(domains).ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 130]>(count).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 128]>(count).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 130]>(physical).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 128]>(physical).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<(usize, ChannelAddress)>(domains).map_err(|_| Error::Capacity)?;
        fn filled<T: Clone>(count: usize, value: T) -> Result<Box<[T]>, Error> {
            let mut rows = Vec::new(); rows.try_reserve_exact(count).map_err(|_| Error::Capacity)?;
            rows.resize(count, value); Ok(rows.into_boxed_slice())
        }
        let mut reset = [0; 130]; reset[11] = u32::MAX; reset[128] = 0x8000_0000;
        let mut keys = Vec::new(); keys.try_reserve_exact(domains).map_err(|_| Error::Capacity)?;
        Ok(Self { banks: filled(count, reset)?, poly_banks: filled(count, [0;128])?,
            physical_banks: filled(physical, reset)?, physical_poly: filled(physical, [0;128])?,
            physical: keys, domains, performances, stages })
    }
    fn index(&self, performance: usize, address: ChannelAddress) -> Option<usize> {
        self.physical.iter().position(|row| *row == (performance, address))
    }
    pub fn validate_scope(&self, performance: usize, scope: ChannelScope) -> Result<(), Error> {
        let missing = (0..16u8).filter(|&channel| scope.channels & (1 << channel) != 0
            && self.index(performance, ChannelAddress { protocol: scope.protocol, port: scope.port, group: scope.group, channel }).is_none()).count();
        if missing > self.domains - self.physical.len() { return Err(Error::Capacity); }
        Ok(())
    }
    fn ensure_scope(&mut self, performance: usize, scope: ChannelScope) {
        for channel in 0..16u8 {
            if scope.channels & (1 << channel) == 0 { continue; }
            let address = ChannelAddress { protocol: scope.protocol, port: scope.port, group: scope.group, channel };
            if self.index(performance, address).is_none() { self.physical.push((performance,address)); }
        }
    }
    pub fn bank(&self, stage: usize, performance: usize, address: Option<ChannelAddress>) -> &[u32; 130] {
        match address.and_then(|address| self.index(performance,address)) {
            Some(index) => &self.physical_banks[(stage - 1) * self.domains + index],
            None => &self.banks[(stage - 1) * self.performances + performance],
        }
    }
    pub fn receive(&mut self, event: ControllerEvent) {
        self.ensure_scope(event.performance, event.scope());
        for index in 0..self.physical.len() {
            let (performance,address) = self.physical[index];
            if performance != event.performance || !event.scope().contains(address) { continue; }
            let index = (event.stage - 1) * self.domains + index;
            if let Some(key) = event.poly_key { self.physical_poly[index][usize::from(key)] = event.value; }
            else { self.physical_banks[index][usize::from(event.number)] = event.value; }
        }
    }
    pub fn poly_bank(&self, stage: usize, performance: usize, address: Option<ChannelAddress>) -> &[u32; 128] {
        match address.and_then(|address| self.index(performance,address)) {
            Some(index) => &self.physical_poly[(stage - 1) * self.domains + index],
            None => &self.poly_banks[(stage - 1) * self.performances + performance],
        }
    }
    pub fn reset_scope(&mut self, performance: usize, scope: ChannelScope) {
        for index in 0..self.physical.len() {
            let (domain,address) = self.physical[index];
            if domain != performance || !scope.contains(address) { continue; }
            for stage in 0..self.stages {
                let bank = &mut self.physical_banks[stage*self.domains+index];
                bank.fill(0); bank[11]=u32::MAX; bank[128]=0x8000_0000;
                self.physical_poly[stage*self.domains+index].fill(0);
            }
        }
    }
    fn reset_poly(&mut self, performance: usize) {
        for bank in self.poly_banks.iter_mut().skip(performance).step_by(self.performances) { bank.fill(0); }
        for (index,(domain,_)) in self.physical.iter().enumerate() {
            if *domain == performance {
                for stage in 0..self.stages { self.physical_poly[stage*self.domains+index].fill(0); }
            }
        }
    }
    fn reset_poly_scope(&mut self, performance: usize, scope: ChannelScope) {
        for (index, (domain, address)) in self.physical.iter().enumerate() {
            if *domain == performance && scope.contains(*address) {
                for stage in 0..self.stages {
                    self.physical_poly[stage * self.domains + index].fill(0);
                }
            }
        }
    }
}
