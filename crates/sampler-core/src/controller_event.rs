use crate::{
    BehaviorId, ChannelAddress, ChannelScope, Error, PerformanceId, PlanId, Prepared, Runtime,
    WaitLifetime,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct ControllerEvent {
    pub performance: usize,
    pub origin: ChannelAddress,
    pub channels: u16,
    pub number: u8,
    pub value: u32,
    pub pending: bool,
    pub stage: usize,
    pub reserved: usize,
}

impl ControllerEvent {
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
    /// only the final projection changes downstream musical state.
    pub fn with_controller_programs(mut self, programs: Vec<usize>) -> Result<Self, Error> {
        if programs.iter().any(|&program| {
            !self
                .programs
                .get(program)
                .is_some_and(|p| !p.requires_note && p.wait_lifetime == WaitLifetime::Callback)
        }) {
            return Err(Error::InvalidInput);
        }
        self.controller_programs = programs.into_boxed_slice();
        Ok(self)
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
        let performance = self.performance_index(performance)?;
        if number >= 128 || origin.group >= 16 || origin.channel >= 16 || channels == 0 {
            return Err(Error::InvalidInput);
        }
        let event = ControllerEvent {
            performance,
            origin,
            channels,
            number,
            value,
            pending: true,
            stage: 0,
            reserved: 0,
        };
        self.apply_due();
        self.admit_controller(self.active_plan, event)
    }

    fn admit_controller(
        &mut self,
        plan: PlanId,
        mut event: ControllerEvent,
    ) -> Result<Option<BehaviorId>, Error> {
        let programs = &self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .controller_programs;
        if let Some(&program) = programs.get(event.stage) {
            let needed = programs.len() - event.stage;
            if self.behaviors.available() < needed {
                return Err(Error::Capacity);
            }
            event.reserved = needed - 1;
            self.behaviors.reserve(event.reserved);
            match self.start_plan_context(plan, program, Some(event)) {
                Ok(id) => Ok(Some(id)),
                Err(error) => {
                    self.behaviors.unreserve(event.reserved);
                    Err(error)
                }
            }
        } else {
            self.publish_controller(event.performance, event.scope(), event.number, event.value)?;
            if event.stage == 0 {
                self.performance_state.input_controllers[event.performance]
                    [usize::from(event.number)] = event.value;
            }
            Ok(None)
        }
    }

    pub(super) fn release_controller_reserve(&mut self, id: BehaviorId) {
        if let Some(event) = self.behaviors.get_mut(id.0).unwrap().controller.as_mut() {
            let reserved = std::mem::take(&mut event.reserved);
            self.behaviors.unreserve(reserved);
        }
    }

    pub(super) fn behavior_input_controller(
        &self,
        id: BehaviorId,
        number: u8,
    ) -> Result<u32, Error> {
        if number >= 128 {
            return Err(Error::InvalidInput);
        }
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        if let Some(event) = callback.controller
            && event.stage != 0
        {
            let plan = self.behavior_plan(callback.owner)?;
            return Ok(self
                .plans
                .get(plan.0)
                .unwrap()
                .controllers
                .bank(event.stage, event.performance)[usize::from(number)]);
        }
        let (performance, _) = self.behavior_performance(id)?;
        Ok(self.performance_state.input_controllers[performance][usize::from(number)])
    }

    pub(super) fn write_behavior_controller(
        &mut self,
        id: BehaviorId,
        number: u8,
        value: u32,
    ) -> Result<(), Error> {
        if number >= 128 {
            return Err(Error::InvalidInput);
        }
        let callback = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        if let Some(mut event) = callback.controller {
            event.stage += 1;
            event.number = number;
            event.value = value;
            event.pending = true;
            event.reserved = 0;
            self.admit_controller(self.behavior_plan(callback.owner)?, event)?;
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
        self.behaviors
            .get_mut(id.0)
            .ok_or(Error::StaleHandle)?
            .controller
            .as_mut()
            .ok_or(Error::InvalidInput)
    }

    pub(super) fn behavior_performance(
        &self,
        id: BehaviorId,
    ) -> Result<(usize, ChannelScope), Error> {
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        if let Some(event) = callback.controller {
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
            let next = event.stage + 1;
            if let Some(&program) = self
                .plans
                .get(plan.0)
                .unwrap()
                .prepared
                .controller_programs
                .get(next)
            {
                let remaining = event
                    .reserved
                    .checked_sub(1)
                    .expect("owned downstream controller slot");
                let current = self.controller_event_mut(id)?;
                current.pending = false;
                current.reserved = 0;
                self.behaviors.unreserve(1);
                self.start_plan_context(
                    plan,
                    program,
                    Some(ControllerEvent {
                        stage: next,
                        reserved: remaining,
                        ..event
                    }),
                )
                .expect("reserved controller continuation");
            } else {
                self.publish_controller(
                    event.performance,
                    event.scope(),
                    event.number,
                    event.value,
                )?;
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
        if number >= 128 {
            return Err(Error::InvalidInput);
        }
        let performance = self.performance(performance)?;
        if matches!(number, 64 | 66) {
            self.set_pedal_controller(performance, scope, number, value)
        } else {
            self.set_controller(performance, number, value)
        }
    }
}

/// Generation-owned projected CC inputs. Stage zero uses the runtime's raw input
/// bank; later stages update only when an event actually reaches that stage.
pub(super) struct ControllerState {
    banks: Box<[[u32; 128]]>,
    performances: usize,
}
impl ControllerState {
    pub fn new(prepared: &Prepared, performances: usize) -> Result<Self, Error> {
        let count = prepared
            .controller_programs
            .len()
            .saturating_sub(1)
            .checked_mul(performances)
            .ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<[u32; 128]>(count).map_err(|_| Error::Capacity)?;
        let mut banks = Vec::new();
        banks
            .try_reserve_exact(count)
            .map_err(|_| Error::Capacity)?;
        banks.resize(count, [0; 128]);
        Ok(Self {
            banks: banks.into_boxed_slice(),
            performances,
        })
    }
    pub fn bank(&self, stage: usize, performance: usize) -> &[u32; 128] {
        &self.banks[(stage - 1) * self.performances + performance]
    }
    pub fn receive(&mut self, event: ControllerEvent) {
        self.banks[(event.stage - 1) * self.performances + event.performance]
            [usize::from(event.number)] = event.value;
    }
}
