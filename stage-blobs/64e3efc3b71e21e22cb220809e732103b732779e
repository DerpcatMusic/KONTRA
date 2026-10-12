//! Owned script-state snapshots use the same native admission boundary as UI
//! edits. The producer retains every allocation through success or rejection.
use crate::{
    BehaviorId, ControlId, ControlValue, Error, Outcome, PlanId, Runtime, ScriptInstanceId, Text,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptStateAddress {
    Control(ControlId),
    Cell {
        instance: ScriptInstanceId,
        index: u32,
    },
    Text {
        instance: ScriptInstanceId,
        index: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScriptStateValue {
    Control(ControlValue),
    /// Reals retain their exact IEEE-754 bits. The source schema owns the type.
    Cell(i64),
    Text(Text),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScriptStateEntry {
    pub address: ScriptStateAddress,
    pub value: ScriptStateValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptStateCallback {
    pub program: usize,
    pub behavior: Option<BehaviorId>,
    /// None is pending; a fault remains distinct from successful completion.
    pub outcome: Option<Outcome>,
}

/// Values must have unique ascending addresses; callbacks unique ascending
/// program indices, with at most one callback per instance in instance order.
/// Capture fills existing storage. Restore validates the whole
/// batch before writes and runs each callback once after all values are applied.
/// Capacity rejection never splits a snapshot. Callback faults do not roll back
/// a successfully admitted restore; their outcomes are returned independently.
#[derive(Debug, Default)]
pub struct ScriptStateBuffer {
    pub values: Vec<ScriptStateEntry>,
    pub callbacks: Vec<ScriptStateCallback>,
}

impl Runtime {
    fn script_state_context(&self, instance: ScriptInstanceId) -> crate::behavior::PlanContext {
        if self.performance_state.current.is_empty() {
            return crate::behavior::PlanContext::Bare;
        }
        // Persistence uses the same instrument domain and source stage as listeners.
        crate::behavior::PlanContext::Control(crate::control::ControlEvent {
            performance: 0,
            origin: crate::ChannelAddress {
                protocol: crate::Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
            },
            channels: 1,
            stage: usize::from(instance.0),
            interaction: crate::WidgetInteraction::default(),
        })
    }
    fn script_state_callback_outcome(
        &self,
        plan: PlanId,
        callback: &ScriptStateCallback,
    ) -> Result<Option<Outcome>, Error> {
        let Some(id) = callback.behavior else {
            return Ok(None);
        };
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation
            .prepared
            .programs
            .get(callback.program)
            .and_then(|p| p.script_instance)
            .ok_or(Error::InvalidInput)?;
        if let Some(c) = self.behaviors.get(id.0) {
            if self.behavior_plan(c.owner)? != plan || c.program != callback.program {
                return Err(Error::InvalidInput);
            }
            return Ok(c.outcome);
        }
        match generation.scripts[usize::from(instance.0)].persistence_callback {
            Some((saved, outcome)) if saved == id => Ok(outcome),
            _ => Err(Error::StaleHandle),
        }
    }

    // Keep the last persistence completion after ordinary behavior flushing.
    // One authored persistence callback exists per script instance.
    pub(crate) fn record_script_state_outcome(&mut self, id: BehaviorId) {
        let Some(c) = self.behaviors.get(id.0).copied() else {
            return;
        };
        let crate::BehaviorOwner::Plan(plan) = c.owner else {
            return;
        };
        let generation = self.plans.get_mut(plan.0).unwrap();
        let Some(instance) = generation.prepared.programs[c.program].script_instance else {
            return;
        };
        if let Some((saved, outcome)) =
            &mut generation.scripts[usize::from(instance.0)].persistence_callback
        {
            if *saved == id {
                *outcome = c.outcome;
            }
        }
    }

    fn script_state_value(
        &self,
        plan: PlanId,
        address: ScriptStateAddress,
    ) -> Result<ScriptStateValue, Error> {
        Ok(match address {
            ScriptStateAddress::Control(id) => {
                ScriptStateValue::Control(self.control_base_value(plan, id)?)
            }
            ScriptStateAddress::Cell { instance, index } => {
                ScriptStateValue::Cell(self.script_cell(plan, instance, index)?)
            }
            ScriptStateAddress::Text { instance, index } => {
                ScriptStateValue::Text(self.script_text(plan, instance, index)?)
            }
        })
    }

    /// One audio-owner operation captures live values and callback outcomes.
    /// Invalid addresses/handles leave the entire producer-owned buffer intact.
    pub fn capture_script_state(
        &self,
        plan: PlanId,
        output: &mut ScriptStateBuffer,
    ) -> Result<(usize, u64), Error> {
        let revision = self.control_revision(plan)?;
        for entry in &output.values {
            self.script_state_value(plan, entry.address)?;
        }
        for callback in &output.callbacks {
            self.script_state_callback_outcome(plan, callback)?;
        }
        for entry in &mut output.values {
            entry.value = self.script_state_value(plan, entry.address)?;
        }
        for callback in &mut output.callbacks {
            if callback.behavior.is_some() {
                callback.outcome = self.script_state_callback_outcome(plan, callback)?;
            }
        }
        Ok((output.values.len(), revision))
    }

    pub fn restore_script_state(
        &mut self,
        plan: PlanId,
        expected_revision: Option<u64>,
        state: &mut ScriptStateBuffer,
    ) -> Result<(usize, u64), Error> {
        self.apply_due();
        let revision = self.control_revision(plan)?;
        if expected_revision.is_some_and(|r| r != revision) {
            return Err(Error::RevisionConflict);
        }
        if state
            .values
            .windows(2)
            .any(|v| v[0].address >= v[1].address)
            || state
                .callbacks
                .windows(2)
                .any(|v| v[0].program >= v[1].program)
        {
            return Err(Error::InvalidInput);
        }
        let next = revision
            .checked_add(u64::from(!state.values.is_empty()))
            .ok_or(Error::ArithmeticOverflow)?;
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        next.checked_add(generation.controls.pending as u64)
            .ok_or(Error::ArithmeticOverflow)?;
        for entry in &state.values {
            let old = self.script_state_value(plan, entry.address)?;
            match (old, entry.value, entry.address) {
                (
                    ScriptStateValue::Control(_),
                    ScriptStateValue::Control(value),
                    ScriptStateAddress::Control(id),
                ) => {
                    if !self.control_definition(plan, id)?.domain.accepts(value) {
                        return Err(Error::InvalidInput);
                    }
                }
                (ScriptStateValue::Cell(_), ScriptStateValue::Cell(_), _)
                | (ScriptStateValue::Text(_), ScriptStateValue::Text(_), _) => {}
                _ => return Err(Error::InvalidInput),
            }
        }
        if !self.behavior_room(state.callbacks.len()) {
            return Err(Error::Capacity);
        }
        self.last_callback_id
            .checked_add(i32::try_from(state.callbacks.len()).map_err(|_| Error::Capacity)?)
            .ok_or(Error::Capacity)?;
        self.plans
            .get(plan.0)
            .unwrap()
            .callbacks
            .checked_add(state.callbacks.len())
            .ok_or(Error::Capacity)?;
        let mut prior_instance = None;
        for callback in &state.callbacks {
            let instance = self
                .plans
                .get(plan.0)
                .unwrap()
                .prepared
                .programs
                .get(callback.program)
                .and_then(|p| p.script_instance)
                .ok_or(Error::InvalidInput)?;
            if prior_instance.is_some_and(|prior| prior >= instance) {
                return Err(Error::InvalidInput);
            }
            prior_instance = Some(instance);
            self.validate_plan_context(plan, callback.program, self.script_state_context(instance))?;
        }
        for callback in &mut state.callbacks {
            let instance = self.plans.get(plan.0).unwrap().prepared.programs[callback.program]
                .script_instance.unwrap();
            callback.behavior = Some(
                self.admit_plan_context(plan, callback.program, self.script_state_context(instance))
                    .expect("preflighted persistence callback admission"),
            );
            callback.outcome = None;
            let generation = self.plans.get_mut(plan.0).unwrap();
            let instance = generation.prepared.programs[callback.program]
                .script_instance
                .unwrap();
            generation.scripts[usize::from(instance.0)].persistence_callback =
                Some((callback.behavior.unwrap(), None));
        }
        let generation = self.plans.get_mut(plan.0).unwrap();
        for entry in &state.values {
            match (entry.address, entry.value) {
                (ScriptStateAddress::Control(id), ScriptStateValue::Control(value)) => {
                    let index = generation.prepared.control_index(id).unwrap();
                    generation.controls.base[index] = value;
                    let playing = generation.controls.playing(&generation.prepared, index);
                    generation.controls.values[index] = playing;
                    generation.dsp.edit_control(&generation.prepared, index, playing, self.now);
                }
                (ScriptStateAddress::Cell { instance, index }, ScriptStateValue::Cell(value)) => {
                    generation.scripts[usize::from(instance.0)].cells[index as usize] = value
                }
                (ScriptStateAddress::Text { instance, index }, ScriptStateValue::Text(value)) => {
                    generation.scripts[usize::from(instance.0)].texts[index as usize] = value
                }
                _ => unreachable!("preflighted script-state type"),
            }
        }
        generation.controls.revision = next;
        for callback in &mut state.callbacks {
            let id = callback.behavior.unwrap();
            self.resume_behavior(id);
            callback.outcome = self.behavior_outcome(id)?;
        }
        Ok((state.values.len(), self.control_revision(plan)?))
    }
}
