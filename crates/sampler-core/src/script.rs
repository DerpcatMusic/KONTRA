//! Script-instance integer state. Storage travels with its prepared generation.
use crate::ops::{ScriptInitial, ScriptResources};
use crate::{BehaviorId, Error, PlanId, Prepared, Program, Runtime};

/// Dense instance identity scoped to a prepared plan, not a callback or note.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptInstanceId(pub u16);

/// Bounded integer-array view in the program's own script-instance bank.
/// Preparation validates the entire view, including unreachable instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptArray {
    pub offset: u32,
    pub len: u32,
}
impl ScriptArray {
    pub(super) fn end(self) -> Result<usize, Error> {
        if self.len == 0 {
            return Err(Error::InvalidInput);
        }
        let end = self.offset.checked_add(self.len).ok_or(Error::Capacity)?;
        usize::try_from(end).map_err(|_| Error::Capacity)
    }
    pub(super) fn cell(self, index: i64) -> Result<u32, Error> {
        let index = u32::try_from(index).map_err(|_| Error::InvalidInput)?;
        if index >= self.len {
            return Err(Error::InvalidInput);
        }
        self.offset.checked_add(index).ok_or(Error::InvalidInput)
    }
}

impl Prepared {
    /// Initial integer banks for independently owned script instances. Shared
    /// callbacks bind explicitly to one bank; no instruction can select another.
    pub fn with_script_instances(mut self, instances: Vec<Vec<i64>>) -> Result<Self, Error> {
        let addressable = usize::from(u16::MAX) + 1;
        if instances.len() > addressable
            || instances.iter().any(|v| u32::try_from(v.len()).is_err())
        {
            return Err(Error::Capacity);
        }
        let instances: Box<[_]> = instances
            .into_iter()
            .map(|cells| ScriptInitial {
                cells: cells.into_boxed_slice(),
                ..Default::default()
            })
            .collect();
        validate(&self.programs, &instances)?;
        self.script_initial = instances;
        Ok(self)
    }

    /// Text, keyed-state and control-table resources, one per script instance in
    /// `with_script_instances` order. Replaces earlier resources.
    pub fn with_script_resources(mut self, resources: Vec<ScriptResources>) -> Result<Self, Error> {
        if resources.len() != self.script_initial.len() {
            return Err(Error::InvalidInput);
        }
        let mut banks = self.script_initial.to_vec();
        for (bank, resources) in banks.iter_mut().zip(resources) {
            resources.apply(bank)?;
        }
        let banks = banks.into_boxed_slice();
        validate(&self.programs, &banks)?;
        for control in banks.iter().flat_map(|b| b.controls.iter().flatten()) {
            let index = self.control_index(*control)?;
            if !matches!(
                self.controls[index].domain,
                crate::ControlDomain::Integer { .. }
            ) {
                return Err(Error::InvalidInput);
            }
        }
        self.script_initial = banks;
        Ok(self)
    }

    pub(super) fn validate_program_scripts(&self, programs: &[Program]) -> Result<(), Error> {
        validate(programs, &self.script_initial)
    }
}

fn validate(programs: &[Program], instances: &[ScriptInitial]) -> Result<(), Error> {
    for program in programs {
        if program.texts.len() < program.text_constants {
            return Err(Error::InvalidInput);
        }
        match program.script_instance {
            Some(id)
                if instances.get(usize::from(id.0)).is_some_and(|v| {
                    v.cells.len() >= program.script_cells && v.texts.len() >= program.script_texts
                }) => {}
            None if program.script_cells == 0 && program.script_texts == 0 => {}
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
        cell: u32,
    ) -> Result<i64, Error> {
        let cell = usize::try_from(cell).map_err(|_| Error::InvalidInput)?;
        self.plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .scripts
            .get(usize::from(instance.0))
            .and_then(|bank| bank.cells.get(cell))
            .copied()
            .ok_or(Error::InvalidInput)
    }

    pub(super) fn behavior_script_cell(&self, id: BehaviorId, cell: u32) -> Result<&i64, Error> {
        let cell = usize::try_from(cell).map_err(|_| Error::InvalidInput)?;
        let continuation = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(continuation.owner)?;
        let program = continuation.program;
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation.prepared.programs[program]
            .script_instance
            .ok_or(Error::InvalidInput)?;
        generation
            .scripts
            .get(usize::from(instance.0))
            .and_then(|bank| bank.cells.get(cell))
            .ok_or(Error::InvalidInput)
    }
    pub(super) fn behavior_write_script_cell(
        &mut self, id: BehaviorId, cell: u32, value: i64,
    ) -> Result<(), Error> {
        let cell = usize::try_from(cell).map_err(|_| Error::InvalidInput)?;
        let continuation = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(continuation.owner)?;
        let program = continuation.program;
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation.prepared.programs[program].script_instance.ok_or(Error::InvalidInput)?;
        let bank = generation.scripts.get_mut(usize::from(instance.0)).ok_or(Error::InvalidInput)?;
        let target = bank.cells.get_mut(cell).ok_or(Error::InvalidInput)?;
        // Port v1 refresh_range's changed-value test into the shared writer.
        let changed = *target != value;
        *target = value;
        if changed && super::ops::captures_cell(bank.captured_cells.as_deref(), cell) {
            generation.script_revision = generation.script_revision.wrapping_add(1);
        }
        Ok(())
    }
}
