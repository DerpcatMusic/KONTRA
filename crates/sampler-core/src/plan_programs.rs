//! Plan-owned programs that start when their plan becomes active, without a
//! note or interaction (KSP `on listener` timers), and `StartProgram` targets.
use crate::{Error, Instruction, Prepared, Program, Runtime};

/// A program started once per plan activation, in the performance context
/// of the first performance and `stage`'s source-module route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanProgram {
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
}

impl Runtime {
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
            let context = crate::behavior::PlanContext::Control(crate::control::ControlEvent {
                performance: 0,
                origin: crate::ChannelAddress {
                    protocol: crate::Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                },
                channels: 1,
                stage: p.stage,
            });
            let _ = self.start_plan_context(plan, p.program, context);
        }
    }
}
