//! Plan-owned programs that start when their plan becomes active, without a
//! note or interaction (KSP `on listener` timers), and `StartProgram` targets.
use crate::behavior::PlanContext;
use crate::{Error, Instruction, Prepared, Program, Runtime};

/// A program started once per plan activation, in the performance context
/// of the first performance and `stage`'s source-module route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanProgram {
    pub program: usize,
    pub stage: usize,
}

/// A program `Instruction::Signal { signal }` starts, in the signalling
/// callback's context on `stage`'s route (KSP `on pgs_changed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalProgram {
    pub signal: u16,
    pub program: usize,
    pub stage: usize,
}

/// Internal inter-script message kinds, unrelated to external MIDI CC assembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterKind {
    Rpn,
    Nrpn,
}

/// A receiving callback on a caller-selected route in the current stage array.
/// Its instance must agree with every ordinary callback and parameter receiver
/// on that stage. An empty ordinary stage may be explicitly assigned a receiver;
/// neither instance numbers nor physical source-slot IDs identify route positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterProgram {
    pub kind: ParameterKind,
    pub program: usize,
    pub stage: usize,
}

/// The sender instance resolves its source slot in the retained prepared plan.
/// The receiving continuation's owner/context retain plan generation and origin.
#[derive(Clone, Copy, Debug)]
pub(super) struct ParameterMessage {
    pub address: u16,
    pub value: u16,
    pub source: crate::ScriptInstanceId,
}

/// Programs started without a note or controller.
fn startable(program: Option<&Program>) -> bool {
    program.is_some_and(|p| {
        !p.requires_note
            && !p.requires_controller
            && p.wait_lifetime == crate::WaitLifetime::Callback
    })
}

pub(super) fn validate_starts(programs: &[Program]) -> Result<(), Error> {
    for op in programs.iter().flat_map(|p| p.code.iter()) {
        if let Instruction::StartProgram { program } = *op
            && !startable(programs.get(program as usize))
        {
            return Err(Error::InvalidInput);
        }
    }
    Ok(())
}

impl Prepared {
    /// Programs to start each time this plan becomes active. Replacing the
    /// program table clears them.
    pub fn with_plan_programs(mut self, programs: Vec<PlanProgram>) -> Result<Self, Error> {
        if programs
            .iter()
            .any(|p| !startable(self.programs.get(p.program)) || p.stage >= self.stages.len())
        {
            return Err(Error::InvalidInput);
        }
        self.plan_programs = programs.into_boxed_slice();
        Ok(self)
    }

    /// Bind RPN/NRPN receivers in ascending stage order. There is at most one
    /// callback of each kind per stage. Runtime delivery uses only later stages
    /// of the sender's retained plan and excludes the sender's script instance.
    /// The receiving instance must match every ordinary callback on its stage
    /// and every other receiver bound there. Empty ordinary stages accept an
    /// explicitly chosen receiver instance, not an inferred native module identity.
    /// To change routing, clear this table, replace stages, then rebind it. The
    /// complete program reset also clears it. This is KONTRA routing policy, not
    /// a Kontakt timing/echo parity claim.
    pub fn with_parameter_programs(
        mut self,
        mut programs: Vec<ParameterProgram>,
    ) -> Result<Self, Error> {
        if programs.iter().any(|p| {
            !startable(self.programs.get(p.program))
                || p.stage >= self.stages.len()
                || self.programs[p.program].script_instance.is_none()
        }) {
            return Err(Error::InvalidInput);
        }
        programs.sort_by_key(|p| p.stage);
        for (i, p) in programs.iter().enumerate() {
            let instance = self.programs[p.program].script_instance;
            let stage = self.stages[p.stage];
            if [stage.note, stage.release, stage.controller]
                .into_iter()
                .flatten()
                .any(|program| self.programs[program].script_instance != instance)
                || programs[..i].iter().any(|other| {
                    other.stage == p.stage
                        && (other.kind == p.kind
                            || self.programs[other.program].script_instance != instance)
                })
            {
                return Err(Error::InvalidInput);
            }
        }
        self.parameter_programs = programs.into_boxed_slice();
        Ok(self)
    }

    /// Programs each signal starts, in order. Replacing the program table
    /// clears them.
    pub fn with_signal_programs(mut self, programs: Vec<SignalProgram>) -> Result<Self, Error> {
        if programs
            .iter()
            .any(|p| !startable(self.programs.get(p.program)) || p.stage >= self.stages.len())
        {
            return Err(Error::InvalidInput);
        }
        self.signal_programs = programs.into_boxed_slice();
        Ok(self)
    }
}

impl Runtime {
    /// Admission is atomic for an emitted message's immediate fanout. No heap
    /// queue: each admitted receiver owns its payload in the bounded callback
    /// arena and is scheduled by the existing explicit dispatch stack.
    pub(super) fn send_parameter(
        &mut self,
        id: crate::BehaviorId,
        kind: ParameterKind,
        address: i64,
        value: i64,
    ) -> Result<(), Error> {
        let raw = |n: i64| -> Result<u16, Error> {
            if !(0..=16383).contains(&n) {
                return Err(Error::InvalidInput);
            }
            Ok(n as u16)
        };
        let callback = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(callback.owner)?;
        let stage = self.behavior_stage(id)?;
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let source = generation.prepared.programs[callback.program]
            .script_instance
            .ok_or(Error::InvalidInput)?;
        let message = ParameterMessage {
            address: raw(address)?,
            value: raw(value)?,
            source,
        };
        // Keep the captured physical address, not a guessed channel from a mask.
        let (performance, origin, channels) = match callback.context {
            PlanContext::Controller(event) => (event.performance, event.origin, event.channels),
            PlanContext::Control(event) => (event.performance, event.origin, event.channels),
            PlanContext::Bare => {
                let note = match callback.owner {
                    crate::BehaviorOwner::Note(note) => note,
                    crate::BehaviorOwner::Plan(_) => return Err(Error::InvalidInput),
                };
                let origin = self.notes.get(note.0).ok_or(Error::StaleHandle)?.address;
                (
                    self.selections[note.0.index].performance,
                    origin,
                    1 << origin.channel,
                )
            }
        };
        let eligible = |p: &ParameterProgram, prepared: &Prepared| {
            p.kind == kind
                && p.stage > stage
                && prepared.programs[p.program].script_instance != Some(message.source)
        };
        let needed = generation
            .prepared
            .parameter_programs
            .iter()
            .filter(|p| eligible(p, &generation.prepared))
            .count();
        generation
            .callbacks
            .checked_add(needed)
            .ok_or(Error::Capacity)?;
        self.last_callback_id
            .checked_add(i32::try_from(needed).map_err(|_| Error::Capacity)?)
            .ok_or(Error::Capacity)?;
        if !self.behavior_room(needed) {
            return Err(Error::Capacity);
        }
        let count = self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .parameter_programs
            .len();
        let deferred = self
            .yielded
            .iter()
            .any(|&y| self.yielded_plan(y) == Some(plan));
        // The ready stack is LIFO; preemption's deferred queue is FIFO.
        // All admissions precede execution; no receiver can mutate this fanout.
        for offset in 0..count {
            let i = if deferred { offset } else { count - 1 - offset };
            let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
            let p = generation.prepared.parameter_programs[i];
            if !eligible(&p, &generation.prepared) {
                continue;
            }
            let context = PlanContext::Control(crate::control::ControlEvent {
                performance,
                origin,
                channels,
                stage: p.stage,
                interaction: crate::WidgetInteraction::default(),
            });
            let receiver = self.admit_plan_context(plan, p.program, context)?;
            self.behaviors
                .get_mut(receiver.0)
                .ok_or(Error::StaleHandle)?
                .parameter = Some(message);
            self.queue_behavior(receiver);
        }
        Ok(())
    }

    /// Start every program bound to `signal` in the performance of callback
    /// `id`, on each program's stage. Full callback capacity skips the rest.
    pub(super) fn signal_programs(
        &mut self,
        id: crate::BehaviorId,
        plan: crate::PlanId,
        signal: u16,
    ) -> Result<(), Error> {
        let performance = self.behavior_performance(id).ok();
        let count = self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .signal_programs
            .len();
        for i in 0..count {
            let p = self.plans.get(plan.0).unwrap().prepared.signal_programs[i];
            if p.signal != signal {
                continue;
            }
            let context = performance.map_or(PlanContext::Bare, |(performance, scope)| {
                PlanContext::Control(crate::control::ControlEvent {
                    performance,
                    origin: crate::ChannelAddress {
                        protocol: scope.protocol,
                        port: scope.port,
                        group: scope.group,
                        channel: scope.channels.trailing_zeros().min(15) as u8,
                    },
                    channels: scope.channels,
                    stage: p.stage,
                    interaction: crate::WidgetInteraction::default(),
                })
            });
            match self.start_plan_context(plan, p.program, context) {
                Ok(_) => {}
                Err(Error::Capacity) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Start the active plan's plan programs once. Full callback capacity or
    /// no performance skips them.
    pub(super) fn start_plan_programs(&mut self) {
        self.start_midi_jobs(self.active_plan);
        if self.started_plan == Some(self.active_plan) {
            return;
        }
        self.started_plan = Some(self.active_plan);
        if self.performance_state.current.is_empty() {
            return;
        }
        let plan = self.active_plan;
        let count = self.plans.get(plan.0).unwrap().prepared.plan_programs.len();
        for i in 0..count {
            let p = self.plans.get(plan.0).unwrap().prepared.plan_programs[i];
            let context = PlanContext::Control(crate::control::ControlEvent {
                performance: 0,
                origin: crate::ChannelAddress {
                    protocol: crate::Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                },
                channels: 1,
                stage: p.stage,
                interaction: crate::WidgetInteraction::default(),
            });
            let _ = self.start_plan_context(plan, p.program, context);
        }
    }
}
