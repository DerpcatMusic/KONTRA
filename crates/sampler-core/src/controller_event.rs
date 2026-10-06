use crate::{
    BehaviorId, ChannelAddress, ChannelScope, Error, PerformanceId, Prepared, Runtime, WaitLifetime,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct ControllerEvent {
    pub performance: usize,
    pub origin: ChannelAddress,
    pub channels: u16,
    pub number: u8,
    pub value: u32,
    pub pending: bool,
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
    pub fn with_controller_program(mut self, program: usize) -> Result<Self, Error> {
        if !self
            .programs
            .get(program)
            .is_some_and(|p| !p.requires_note && p.wait_lifetime == WaitLifetime::Callback)
        {
            return Err(Error::InvalidInput);
        }
        self.controller_program = Some(program);
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
        };
        self.apply_due();
        let plan = self.active_plan;
        if let Some(program) = self.plans.get(plan.0).unwrap().prepared.controller_program {
            self.start_plan_context(plan, program, Some(event))
                .map(Some)
        } else {
            self.publish_controller(performance, event.scope(), number, value)?;
            self.performance_state.input_controllers[performance][usize::from(number)] = value;
            Ok(None)
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
            self.publish_controller(event.performance, event.scope(), event.number, event.value)?;
            self.controller_event_mut(id)?.pending = false;
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
