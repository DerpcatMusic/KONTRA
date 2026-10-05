//! Presentation-independent control state. Metadata is prepared off audio; values
//! have one audio-side writer and are captured into caller-owned storage.
use super::{Error, Instruction, PlanId, Prepared, Runtime};
mod transfer;
pub(super) use transfer::ControlQueues;
pub use transfer::{
    ControlClient, ControlOperation, ControlQueueError, ControlReply, ControlRequest,
    RejectedControls,
};

/// Persistent semantic identity assigned by the source frontend/composition root.
/// Never derive this from a widget position, dense index or randomized hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlId(pub u128);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlValue {
    Integer(i64),
    Real(f64),
    Toggle(bool),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlDomain {
    Integer { min: i64, max: i64 },
    Real { min: f64, max: f64 },
    Toggle,
}
impl ControlDomain {
    fn accepts(self, value: ControlValue) -> bool {
        match (self, value) {
            (Self::Integer { min, max }, ControlValue::Integer(value)) => {
                min <= value && value <= max
            }
            (Self::Real { min, max }, ControlValue::Real(value)) => {
                min.is_finite()
                    && max.is_finite()
                    && value.is_finite()
                    && min <= value
                    && value <= max
            }
            (Self::Toggle, ControlValue::Toggle(_)) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlDefinition {
    pub id: ControlId,
    pub domain: ControlDomain,
    pub default: ControlValue,
}

/// One exact typed value; the frontend owns normalization, units and clamping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlWrite {
    pub id: ControlId,
    pub value: ControlValue,
}

pub(super) struct ControlState {
    values: Box<[ControlValue]>,
    revision: u64,
}
impl ControlState {
    pub(super) fn new(plan: &Prepared) -> Self {
        Self {
            values: plan.controls.iter().map(|c| c.default).collect(),
            revision: 0,
        }
    }
}

impl Prepared {
    /// Replace the complete control schema off audio. Definition order is not
    /// identity; duplicate IDs, invalid defaults and incompatible programs fail.
    pub fn with_controls(mut self, mut controls: Vec<ControlDefinition>) -> Result<Self, Error> {
        controls.sort_unstable_by_key(|c| c.id);
        if controls.windows(2).any(|c| c[0].id == c[1].id)
            || controls.iter().any(|c| !c.domain.accepts(c.default))
        {
            return Err(Error::InvalidInput);
        }
        self.controls = controls.into_boxed_slice();
        self.validate_program_controls(&self.programs)?;
        Ok(self)
    }

    pub fn controls(&self) -> &[ControlDefinition] {
        &self.controls
    }

    fn control_index(&self, id: ControlId) -> Result<usize, Error> {
        self.controls
            .binary_search_by_key(&id, |c| c.id)
            .map_err(|_| Error::InvalidInput)
    }

    pub(super) fn validate_program_controls(
        &self,
        programs: &[super::Program],
    ) -> Result<(), Error> {
        for op in programs.iter().flat_map(|p| p.code.iter()) {
            if let Instruction::ReadControl { control, .. }
            | Instruction::WriteControl { control, .. } = *op
            {
                let index = self.control_index(control)?;
                if !matches!(self.controls[index].domain, ControlDomain::Integer { .. }) {
                    return Err(Error::InvalidInput);
                }
            }
        }
        Ok(())
    }
}

impl Runtime {
    pub fn control_value(&self, plan: PlanId, id: ControlId) -> Result<ControlValue, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        Ok(generation.controls.values[generation.prepared.control_index(id)?])
    }

    pub fn control_revision(&self, plan: PlanId) -> Result<u64, Error> {
        Ok(self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .controls
            .revision)
    }

    /// Capture one coherent revision into preallocated storage, in ascending ID
    /// order. Returns (count, revision). Insufficient storage leaves it untouched.
    /// Call from the audio owner; the UI receives a copied snapshot, never &Runtime.
    pub fn capture_controls(
        &self,
        plan: PlanId,
        output: &mut [ControlWrite],
    ) -> Result<(usize, u64), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let definitions = &generation.prepared.controls;
        if output.len() < definitions.len() {
            return Err(Error::Capacity);
        }
        for ((slot, definition), value) in output
            .iter_mut()
            .zip(definitions.iter())
            .zip(generation.controls.values.iter())
        {
            *slot = ControlWrite {
                id: definition.id,
                value: *value,
            };
        }
        Ok((definitions.len(), generation.controls.revision))
    }

    /// Transactional edit at the current sample boundary, after previously due
    /// work. Writes must have unique ascending IDs. A stale expected revision
    /// rejects the whole edit; no value is clamped, dropped or partially applied.
    /// Empty writes validate the revision without advancing it.
    pub fn edit_controls(
        &mut self,
        plan: PlanId,
        expected_revision: Option<u64>,
        writes: &[ControlWrite],
    ) -> Result<u64, Error> {
        self.apply_due();
        self.edit_controls_now(plan, expected_revision, writes)
    }

    /// Complete recall: every control must occur exactly once. The same ID/value
    /// snapshot works after prepared-table reordering. Unknown/missing IDs fail.
    pub fn recall_controls(
        &mut self,
        plan: PlanId,
        expected_revision: Option<u64>,
        writes: &[ControlWrite],
    ) -> Result<u64, Error> {
        self.apply_due();
        if self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .controls
            .len()
            != writes.len()
        {
            return Err(Error::InvalidInput);
        }
        self.edit_controls_now(plan, expected_revision, writes)
    }

    pub(super) fn edit_controls_now(
        &mut self,
        plan: PlanId,
        expected_revision: Option<u64>,
        writes: &[ControlWrite],
    ) -> Result<u64, Error> {
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        if expected_revision.is_some_and(|expected| expected != generation.controls.revision) {
            return Err(Error::RevisionConflict);
        }
        let definitions = &generation.prepared;
        if writes.windows(2).any(|w| w[0].id >= w[1].id) {
            return Err(Error::InvalidInput);
        }
        for write in writes {
            let index = definitions.control_index(write.id)?;
            if !definitions.controls[index].domain.accepts(write.value) {
                return Err(Error::InvalidInput);
            }
        }
        let revision = generation
            .controls
            .revision
            .checked_add(u64::from(!writes.is_empty()))
            .ok_or(Error::ArithmeticOverflow)?;
        for write in writes {
            // All lookups/values validated above; one writer, no reentrancy.
            let index = definitions.control_index(write.id).unwrap();
            generation.controls.values[index] = write.value;
        }
        generation.controls.revision = revision;
        Ok(revision)
    }
}
