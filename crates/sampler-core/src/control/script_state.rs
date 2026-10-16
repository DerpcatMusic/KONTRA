//! Owned script-state snapshots use the same native admission boundary as UI
//! edits. The producer retains every allocation through success or rejection.
use crate::{
    BehaviorId, ControlId, ControlValue, Error, Outcome, PlanId, Runtime, ScriptInstanceId, Text,
};

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
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

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScriptStateValue {
    Control(ControlValue),
    /// Reals retain their exact IEEE-754 bits. The source schema owns the type.
    Cell(i64),
    /// Index into [`ScriptStateBuffer::texts`]: texts are rare and 320 bytes,
    /// so entries stay small (Areia keeps ~1.3M cell entries).
    Text(u32),
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
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
    /// Text values, one slot per text entry; see [`ScriptStateValue::Text`].
    pub texts: Vec<Text>,
}

impl ScriptStateBuffer {
    /// A new text slot holding `text`, as a value for one text entry.
    pub fn push_text(&mut self, text: Text) -> ScriptStateValue {
        self.texts.push(text);
        ScriptStateValue::Text(self.texts.len() as u32 - 1)
    }

    /// The text a [`ScriptStateValue::Text`] value names.
    pub fn text(&self, value: ScriptStateValue) -> Option<&Text> {
        match value {
            ScriptStateValue::Text(slot) => self.texts.get(slot as usize),
            _ => None,
        }
    }

    /// Address order keeping each address's first entry (a stable sort, then
    /// dedup). Loads emit rows mostly in order, so the sort runs only when an
    /// entry is out of order.
    pub fn sort_dedup(&mut self) {
        if !self.values.is_sorted_by_key(|entry| entry.address) {
            self.values.sort_by_key(|entry| entry.address);
        }
        self.values.dedup_by_key(|entry| entry.address);
    }

    /// As pushing `rows` then [`Self::sort_dedup`] (existing entries win on
    /// an equal address), without re-sorting the ordered values or copying
    /// them: only rows at new addresses are added, merged in place from the end.
    /// Text rows name slots in `self.texts`. Returns the added addresses, sorted.
    pub fn merge(&mut self, mut rows: Vec<ScriptStateEntry>) -> Vec<ScriptStateAddress> {
        rows.sort_by_key(|entry| entry.address);
        rows.dedup_by_key(|entry| entry.address);
        let mut addresses = rows.iter().map(|row| row.address).collect();
        self.merge_addresses(&mut addresses, |address, _| {
            rows[rows
                .binary_search_by_key(&address, |row| row.address)
                .expect("row address")]
            .value
        });
        addresses
    }

    /// As [`Self::merge`] for rows whose value follows from their address;
    /// `value` runs once per added address and may add text slots. Leaves
    /// only the added addresses in `addresses`.
    pub fn merge_addresses(
        &mut self,
        addresses: &mut Vec<ScriptStateAddress>,
        mut value: impl FnMut(ScriptStateAddress, &mut Vec<Text>) -> ScriptStateValue,
    ) {
        self.sort_dedup();
        addresses.sort_unstable();
        addresses.dedup();
        addresses.retain(|address| {
            self.values
                .binary_search_by_key(address, |entry| entry.address)
                .is_err()
        });
        let (mut old, mut new) = (self.values.len(), addresses.len());
        self.values.reserve_exact(new);
        // Placeholders, overwritten below: every slot past `old` is written.
        self.values.extend(addresses.iter().map(|&address| ScriptStateEntry {
            address,
            value: ScriptStateValue::Cell(0),
        }));
        // Addresses are distinct now: fill from the back, largest first.
        while new > 0 {
            let write = old + new - 1;
            if old > 0 && self.values[old - 1].address > addresses[new - 1] {
                self.values[write] = self.values[old - 1];
                old -= 1;
            } else {
                let address = addresses[new - 1];
                self.values[write] = ScriptStateEntry {
                    address,
                    value: value(address, &mut self.texts),
                };
                new -= 1;
            }
        }
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    #[test]
    fn merge_matches_push_then_stable_sort_dedup() {
        let cell = |index, value| ScriptStateEntry {
            address: ScriptStateAddress::Cell { instance: ScriptInstanceId(0), index },
            value: ScriptStateValue::Cell(value),
        };
        let base = vec![cell(1, 10), cell(3, 30), cell(5, 50)];
        let rows = vec![cell(4, 41), cell(3, 31), cell(0, 1), cell(4, 42), cell(9, 91)];
        let mut pushed = ScriptStateBuffer { values: base.clone(), ..Default::default() };
        pushed.values.extend(rows.iter().copied());
        pushed.values.sort_by_key(|entry| entry.address);
        pushed.values.dedup_by_key(|entry| entry.address);
        let mut merged = ScriptStateBuffer { values: base, ..Default::default() };
        merged.merge(rows);
        assert_eq!(merged.values, pushed.values);
    }

    /// Persistent arrays reach millions of entries; texts live in a side table.
    #[test]
    fn entries_stay_small() {
        use std::mem::size_of;
        eprintln!(
            "SIZES address={} entry={} instruction={}",
            size_of::<ScriptStateAddress>(),
            size_of::<ScriptStateEntry>(),
            size_of::<crate::Instruction>()
        );
        assert!(size_of::<ScriptStateAddress>() <= 24);
        assert!(size_of::<ScriptStateEntry>() <= 40);
    }
}

/// One checked destination/value pair, held only for the current row. Keeping
/// the operation on the stack avoids per-snapshot scratch (persistent state can
/// contain millions of rows). A complete pure pass resolves EVERY row before
/// callback admission or writes; commit resolves the same immutable rows again.
/// Admission does not execute code. Writes change values/dirty bits/DSP state,
/// never prepared domains, IDs, script bank lengths or producer text slots.
/// Consequently the second resolution has the same result for every row.
/// No operation survives a Runtime boundary or is reused against another plan.
pub(super) enum ResolvedStateWrite {
    Control(super::ControlEdit),
    Cell {
        instance: usize,
        index: usize,
        value: i64,
    },
    Text {
        instance: usize,
        index: usize,
        slot: usize,
    },
}
impl ResolvedStateWrite {
    pub(super) fn control(
        generation: &crate::Generation,
        id: ControlId,
        value: ControlValue,
    ) -> Result<Self, Error> {
        Ok(Self::Control(generation.prepared.resolve_control(id,value)?))
    }

    pub(super) fn resolve(
        generation: &crate::Generation,
        entry: &ScriptStateEntry,
        texts: &[Text],
        reject_truncated: bool,
    ) -> Result<Self, Error> {
        Ok(match (entry.address, entry.value) {
            (ScriptStateAddress::Control(id), ScriptStateValue::Control(value)) => {
                return Self::control(generation, id, value);
            }
            (ScriptStateAddress::Cell { instance, index }, ScriptStateValue::Cell(value)) => {
                let instance = usize::from(instance.0);
                let index = usize::try_from(index).map_err(|_| Error::InvalidInput)?;
                generation
                    .scripts
                    .get(instance)
                    .and_then(|bank| bank.cells.get(index))
                    .ok_or(Error::InvalidInput)?;
                Self::Cell {
                    instance,
                    index,
                    value,
                }
            }
            (ScriptStateAddress::Text { instance, index }, ScriptStateValue::Text(slot)) => {
                let instance = usize::from(instance.0);
                let index = usize::try_from(index).map_err(|_| Error::InvalidInput)?;
                let slot = usize::try_from(slot).map_err(|_| Error::InvalidInput)?;
                generation
                    .scripts
                    .get(instance)
                    .and_then(|bank| bank.texts.get(index))
                    .ok_or(Error::InvalidInput)?;
                let text = texts.get(slot).ok_or(Error::InvalidInput)?;
                if reject_truncated && text.is_truncated() {
                    return Err(Error::InvalidInput);
                }
                Self::Text {
                    instance,
                    index,
                    slot,
                }
            }
            _ => return Err(Error::InvalidInput),
        })
    }

    pub(super) fn apply(self, generation: &mut crate::Generation, texts: &[Text], now: u64) {
        match self {
            Self::Control(edit) => {
                let (id,projection)=generation.controls.apply(&generation.prepared,edit);
                generation.dsp.edit_control(&generation.prepared,id,projection,now);
            }
            Self::Cell {
                instance,
                index,
                value,
            } => {
                let bank = &mut generation.scripts[instance];
                let changed = bank.cells[index] != value;
                bank.cells[index] = value;
                bank.mark_captured_cell(index, changed);
            }
            Self::Text {
                instance,
                index,
                slot,
            } => {
                generation.scripts[instance].texts[index] = texts[slot];
            }
        }
    }
}

impl Runtime {
    pub(super) fn script_state_context(&self, instance: ScriptInstanceId) -> crate::behavior::PlanContext {
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

    /// Register the value-only snapshot domain during off-thread preparation.
    /// Unregistered plans conservatively track every script write.
    pub fn watch_script_state_values(
        &mut self,
        plan: PlanId,
        values: &[ScriptStateEntry],
    ) -> Result<(), Error> {
        // Validate the entire request before changing the existing capture domain.
        for entry in values {
            self.script_state_value(plan, entry.address)?;
        }
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let mut masks: Vec<Box<[u64]>> = generation
            .scripts
            .iter()
            .map(|bank| vec![0; bank.cells.len().div_ceil(64)].into_boxed_slice())
            .collect();
        for entry in values {
            if let ScriptStateAddress::Cell { instance, index } = entry.address {
                masks[usize::from(instance.0)][index as usize / 64] |= 1 << (index % 64);
            }
        }
        for (bank, mask) in generation.scripts.iter_mut().zip(masks) {
            bank.dirty_cells = Some(vec![0; mask.len()].into_boxed_slice());
            bank.captured_cells = Some(mask);
        }
        generation.script_revision = generation.script_revision.wrapping_add(1);
        Ok(())
    }

    /// Registered snapshot owner: visit changed captured cells without clearing them.
    /// False means a plan has no complete registered domain and needs full capture.
    pub fn visit_dirty_script_cells(
        &self,
        plan: PlanId,
        mut visit: impl FnMut(ScriptStateAddress),
    ) -> Result<bool, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if generation
            .scripts
            .iter()
            .any(|bank| bank.dirty_cells.is_none())
        {
            return Ok(false);
        }
        for (instance, bank) in generation.scripts.iter().enumerate() {
            for (word, &bits) in bank.dirty_cells.as_deref().unwrap().iter().enumerate() {
                let mut bits = bits;
                while bits != 0 {
                    let index = word * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    visit(ScriptStateAddress::Cell {
                        instance: ScriptInstanceId(instance as u16),
                        index: index as u32,
                    });
                }
            }
        }
        Ok(true)
    }

    /// Clear only after the registered snapshot owner successfully captured the batch.
    pub fn clear_dirty_script_cells(&mut self, plan: PlanId) -> Result<(), Error> {
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        for bank in &mut generation.scripts {
            if let Some(dirty) = bank.dirty_cells.as_mut() {
                dirty.fill(0);
            }
        }
        Ok(())
    }

    /// Changes to captured cells/text and base controls, including DSP edits.
    /// Callback outcomes are independent; this token gates value-only snapshots.
    pub fn script_state_revision(&self, plan: PlanId) -> Result<(u64, u64), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        Ok((generation.controls.revision, generation.script_revision))
    }

    #[inline]
    pub(super) fn script_state_value(
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
                self.script_text(plan, instance, index)?;
                ScriptStateValue::Text(u32::MAX)
            }
        })
    }

    /// A text entry's value must name one of `texts`' slots.
    pub(super) fn script_state_slot(entry: &ScriptStateEntry, texts: &[Text]) -> Result<(), Error> {
        match (entry.address, entry.value) {
            (ScriptStateAddress::Text { .. }, ScriptStateValue::Text(slot))
                if (slot as usize) < texts.len() =>
            {
                Ok(())
            }
            (ScriptStateAddress::Text { .. }, _) | (_, ScriptStateValue::Text(_)) => {
                Err(Error::InvalidInput)
            }
            _ => Ok(()),
        }
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
            Self::script_state_slot(entry, &output.texts)?;
        }
        for callback in &output.callbacks {
            self.script_state_callback_outcome(plan, callback)?;
        }
        for entry in &mut output.values {
            match (entry.address, entry.value) {
                (ScriptStateAddress::Text { instance, index }, ScriptStateValue::Text(slot)) => {
                    output.texts[slot as usize] = self.script_text(plan, instance, index)?;
                }
                (address, _) => entry.value = self.script_state_value(plan, address)?,
            }
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
        // Resolve the entire batch without changing live or producer state.
        for entry in &state.values {
            ResolvedStateWrite::resolve(generation, entry, &state.texts, false)?;
        }
        if !self.behavior_room(state.callbacks.len()) {
            return Err(Error::Capacity);
        }
        crate::prepare::zone_routing::callback_id_preflight(
            self.last_callback_id, self.callback_id_reservations, state.callbacks.len())?;
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
            self.validate_plan_context(
                plan,
                callback.program,
                self.script_state_context(instance),
            )?;
        }
        for callback in &mut state.callbacks {
            let instance = self.plans.get(plan.0).unwrap().prepared.programs[callback.program]
                .script_instance
                .unwrap();
            callback.behavior = Some(
                self.admit_plan_context(
                    plan,
                    callback.program,
                    self.script_state_context(instance),
                )
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
            let write = ResolvedStateWrite::resolve(generation, entry, &state.texts, false)?;
            write.apply(generation, &state.texts, self.now);
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
