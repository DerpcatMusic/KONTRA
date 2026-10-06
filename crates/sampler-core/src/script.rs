//! Script-instance integer state. Storage travels with its prepared generation.
use crate::{BehaviorId, Error, PlanId, Prepared, Program, Runtime};

/// Dense instance identity scoped to a prepared plan, not a callback or note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptInstanceId(pub u16);

impl Prepared {
    /// Initial integer banks for independently owned script instances. Shared
    /// callbacks bind explicitly to one bank; no instruction can select another.
    pub fn with_script_instances(mut self, instances: Vec<Vec<i64>>) -> Result<Self, Error> {
        let addressable = usize::from(u16::MAX) + 1;
        if instances.len() > addressable || instances.iter().any(|v| v.len() > addressable) {
            return Err(Error::Capacity);
        }
        let instances: Box<[_]> = instances.into_iter().map(Vec::into_boxed_slice).collect();
        validate(&self.programs, &instances)?;
        self.script_initial = instances;
        Ok(self)
    }

    pub(super) fn validate_program_scripts(&self, programs: &[Program]) -> Result<(), Error> {
        validate(programs, &self.script_initial)
    }
}

fn validate(programs: &[Program], instances: &[Box<[i64]>]) -> Result<(), Error> {
    for program in programs {
        match program.script_instance {
            Some(id)
                if instances
                    .get(usize::from(id.0))
                    .is_some_and(|v| v.len() >= program.script_cells) => {}
            None if program.script_cells == 0 => {}
            _ => return Err(Error::InvalidInput),
        }
    }
    Ok(())
}

impl Runtime {
    /// Audio-owner query. This is script state, not a UI parameter or snapshot of
    /// all language values. A retained plan identifies the exact generation.
    pub fn script_cell(
        &self,
        plan: PlanId,
        instance: ScriptInstanceId,
        cell: u16,
    ) -> Result<i64, Error> {
        self.plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .scripts
            .get(usize::from(instance.0))
            .and_then(|bank| bank.get(usize::from(cell)))
            .copied()
            .ok_or(Error::InvalidInput)
    }

    pub(super) fn behavior_script_cell_mut(
        &mut self,
        id: BehaviorId,
        cell: u16,
    ) -> Result<&mut i64, Error> {
        let continuation = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(continuation.owner)?;
        let program = continuation.program;
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation.prepared.programs[program]
            .script_instance
            .ok_or(Error::InvalidInput)?;
        generation
            .scripts
            .get_mut(usize::from(instance.0))
            .and_then(|bank| bank.get_mut(usize::from(cell)))
            .ok_or(Error::InvalidInput)
    }
}
