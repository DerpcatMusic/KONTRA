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

/// A control callback's position in the prepared source-module route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlCallback {
    pub control: ControlId,
    pub program: usize,
    pub stage: usize,
}

/// Musical target of a UI interaction. The caller supplies the real performance
/// domain and address; a UI callback never borrows or fabricates a host note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlContext {
    pub performance: crate::PerformanceId,
    pub origin: crate::ChannelAddress,
    pub channels: u16,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ControlEvent {
    pub performance: usize,
    pub origin: crate::ChannelAddress,
    pub channels: u16,
    pub stage: usize,
    pub interaction: crate::WidgetInteraction,
}
impl ControlEvent {
    pub fn scope(self) -> crate::ChannelScope {
        crate::ChannelScope {
            protocol: self.origin.protocol,
            port: self.origin.port,
            group: self.origin.group,
            channels: self.channels,
        }
    }
}

pub(super) struct ControlState {
    values: Box<[ControlValue]>,
    pub(super) revision: u64,
    /// Future writes reserve both this generation and one revision increment each.
    pub(super) pending: usize,
}
impl ControlState {
    pub(super) fn new(plan: &Prepared) -> Self {
        Self {
            values: plan.controls.iter().map(|c| c.default).collect(),
            revision: 0,
            pending: 0,
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
        self.validate_dsp_controls()?;
        for binding in &self.control_programs {
            self.control_index(binding.control)?;
        }
        Ok(self)
    }

    /// Bind one callback per control. Plain script writes/recall do not invoke it;
    /// interaction dispatch is explicit. Replacing programs clears these bindings.
    pub fn with_control_programs(
        mut self,
        mut callbacks: Vec<ControlCallback>,
    ) -> Result<Self, Error> {
        callbacks.sort_unstable_by_key(|c| c.control);
        if callbacks.windows(2).any(|c| c[0].control == c[1].control) {
            return Err(Error::InvalidInput);
        }
        for binding in &callbacks {
            self.control_index(binding.control)?;
            let program = self
                .programs
                .get(binding.program)
                .ok_or(Error::InvalidInput)?;
            if binding.stage >= self.stages.len()
                || program.requires_note
                || program.requires_controller
                || program.wait_lifetime != super::WaitLifetime::Callback
            {
                return Err(Error::InvalidInput);
            }
        }
        self.control_programs = callbacks.into_boxed_slice();
        Ok(self)
    }

    pub fn controls(&self) -> &[ControlDefinition] {
        &self.controls
    }

    pub(super) fn control_index(&self, id: ControlId) -> Result<usize, Error> {
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
    /// Apply an interaction and start its prepared handler as one admission. Full
    /// callback capacity rejects before changing the value. Handler faults are
    /// retained outcomes; a committed edit is not rolled back after execution.
    /// Returns (resulting revision, callback identity), including synchronous work.
    pub fn invoke_control(
        &mut self,
        context: ControlContext,
        plan: PlanId,
        expected_revision: Option<u64>,
        write: ControlWrite,
    ) -> Result<(u64, Option<super::BehaviorId>), Error> {
        let performance = self.performance_index(context.performance)?;
        if context.origin.group >= 16 || context.origin.channel >= 16 || context.channels == 0 {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let bindings = &generation.prepared.control_programs;
        let binding = bindings
            .binary_search_by_key(&write.id, |c| c.control)
            .ok()
            .map(|index| bindings[index]);
        let event = binding.map(|binding| {
            crate::behavior::PlanContext::Control(ControlEvent {
                performance,
                origin: context.origin,
                channels: context.channels,
                stage: binding.stage,
                interaction: crate::WidgetInteraction::default(),
            })
        });
        if let Some(binding) = binding {
            self.validate_plan_context(plan, binding.program, event.unwrap())?;
        }
        self.edit_controls_now(plan, expected_revision, &[write])?;
        let callback = binding.map(|binding| {
            self.start_plan_context(plan, binding.program, event.unwrap())
                .expect("preflighted control callback admission")
        });
        Ok((self.control_revision(plan)?, callback))
    }

    pub fn control_value(&self, plan: PlanId, id: ControlId) -> Result<ControlValue, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        Ok(generation.controls.values[generation.prepared.control_index(id)?])
    }

    /// The control's id, domain and default in `plan`.
    /// The immutable schema of an addressed generation, for an off-audio producer.
    pub fn control_definitions(&self, plan: PlanId) -> Result<&[ControlDefinition], Error> {
        Ok(&self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.controls)
    }

    pub fn control_definition(
        &self,
        plan: PlanId,
        id: ControlId,
    ) -> Result<ControlDefinition, Error> {
        let prepared = &self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared;
        Ok(prepared.controls[prepared.control_index(id)?])
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

    pub(super) fn validate_controls(
        &self,
        plan: PlanId,
        expected_revision: Option<u64>,
        writes: &[ControlWrite],
    ) -> Result<u64, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
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
        revision
            .checked_add(
                u64::try_from(generation.controls.pending)
                    .map_err(|_| Error::ArithmeticOverflow)?,
            )
            .ok_or(Error::ArithmeticOverflow)?;
        Ok(revision)
    }

    pub(super) fn edit_controls_now(
        &mut self,
        plan: PlanId,
        expected_revision: Option<u64>,
        writes: &[ControlWrite],
    ) -> Result<u64, Error> {
        let revision = self.validate_controls(plan, expected_revision, writes)?;
        let generation = self.plans.get_mut(plan.0).unwrap();
        let definitions = &generation.prepared;
        for write in writes {
            // All lookups/values validated above; one writer, no reentrancy.
            let index = definitions.control_index(write.id).unwrap();
            generation.controls.values[index] = write.value;
            generation
                .dsp
                .edit_control(definitions, index, write.value, self.now);
        }
        generation.controls.revision = revision;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_writes_reserve_revision_space_until_execution_or_cancellation() {
        let id = ControlId(1);
        let plan = Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_controls(vec![ControlDefinition {
                id,
                domain: ControlDomain::Toggle,
                default: ControlValue::Toggle(false),
            }])
            .unwrap();
        let mut rt = Runtime::new(
            plan,
            crate::Limits {
                notes: 1,
                channels: 0,
                performances: 1,
                families: 0,
                expressions: 1,
                voices: 0,
                decisions: 0,
                commands: 2,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
                note_cells: 0,
            },
        )
        .unwrap();
        let plan = rt.active_plan();
        rt.plans.get_mut(plan.0).unwrap().controls.revision = u64::MAX - 1;
        let write = ControlWrite {
            id,
            value: ControlValue::Toggle(true),
        };
        rt.schedule_event(1, crate::Event::Control(plan, write))
            .unwrap();
        assert_eq!(
            rt.edit_controls(plan, None, &[write]),
            Err(Error::ArithmeticOverflow)
        );
        assert_eq!(
            rt.schedule_event(2, crate::Event::Control(plan, write)),
            Err(Error::ArithmeticOverflow)
        );
        assert_eq!(rt.control_revision(plan), Ok(u64::MAX - 1));
        assert_eq!(rt.cancel_control_events(plan), Ok(1));
        rt.schedule_event(1, crate::Event::Control(plan, write))
            .unwrap();
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.control_revision(plan), Ok(u64::MAX));
        assert_eq!(rt.control_value(plan, id), Ok(ControlValue::Toggle(true)));
        assert_eq!(rt.edit_controls(plan, None, &[]), Ok(u64::MAX));
        assert_eq!(
            rt.schedule_event(2, crate::Event::Control(plan, write)),
            Err(Error::ArithmeticOverflow)
        );
        assert_eq!(rt.pending_commands(), 0);
    }
}

/// What a runtime-writable effect slot parameter does; see
/// [`Processor::Mix`](crate::Processor::Mix).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotKind {
    /// Nonzero routes the dry signal around the slot.
    Bypass,
    /// Linear gain of the slot's processed signal, 0..=16.
    Output,
    /// Linear gain of the signal that passes around the slot, 0..=16.
    Dry,
}
impl SlotKind {
    /// Largest value the control takes.
    pub const fn max(self) -> f64 {
        match self {
            Self::Bypass => 1.,
            Self::Output | Self::Dry => 16.,
        }
    }
}

/// Whether `id` came from [`slot_control`]: a plan owns these, so a script
/// binding that replaces the control schema keeps them.
pub fn is_slot_control(id: ControlId) -> bool {
    id.0 >> 104 == 0x4d_4958
}

/// The slot number of an instrument bus's volume control (`generic` is the bus address).
pub const BUS_VOLUME_SLOT: i32 = 0x4255;

/// The control behind one slot parameter. The identity encodes the address,
/// so a script's runtime write finds it without a table.
pub fn slot_control(kind: SlotKind, group: i32, slot: i32, generic: i32) -> ControlId {
    ControlId(
        (0x004d_4958_u128 << 104)
            | (u128::from(kind as u8) << 96)
            | (u128::from(group as u32) << 64)
            | (u128::from(slot as u32) << 32)
            | u128::from(generic as u32),
    )
}
