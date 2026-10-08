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
