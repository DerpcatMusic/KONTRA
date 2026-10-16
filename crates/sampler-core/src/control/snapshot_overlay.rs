//! Same-generation snapshot admission. This operation never drains scheduled
//! work before deciding whether the complete producer-owned batch can commit.
use super::script_state::ResolvedStateWrite;
use super::{ControlWrite, ScriptStateAddress, ScriptStateBuffer};
use crate::{Error, PlanId, Runtime};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Maximum combined script/native scalar homes committed at one boundary.
pub const SNAPSHOT_OVERLAY_VALUE_CAPACITY: usize = 8192;

/// An exact live owner/value fence, captured by the audio owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotOverlayFence {
    pub plan: PlanId,
    pub control_revision: u64,
    pub script_revision: u64,
}

/// Product-owned source/load ordering. The payload retains its Arc through the
/// reply; neither success nor rejection destroys it on the audio thread.
#[derive(Debug)]
pub struct SnapshotOverlayEpoch {
    pub sequence: Arc<AtomicU64>,
    pub expected: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotOverlayRefusal {
    InactivePlan,
    RevisionConflict,
    StaleEpoch,
    ActiveEvents,
    AudioTail,
    HostState,
    PendingCommands,
    ActiveCallbacks,
    PendingServices,
    PendingStartup,
    UnsafeCallback,
    InvalidInput,
    Capacity,
}
impl SnapshotOverlayRefusal {
    fn error(self) -> Error {
        match self {
            Self::InactivePlan => Error::StaleHandle,
            Self::RevisionConflict | Self::StaleEpoch => Error::RevisionConflict,
            Self::InvalidInput | Self::UnsafeCallback => Error::InvalidInput,
            Self::Capacity => Error::Capacity,
            _ => Error::NotReady,
        }
    }
}

/// Prepared off audio. `state` contains only frontend-validated incoming
/// snapshot homes; native registry scalars remain a distinct, disjoint roster.
/// A successful commit retains the Runtime, PlanId, banks, sparse stores, PGS,
/// and prepared code. Callback waits/faults are outcomes after admission.
#[derive(Debug)]
pub struct NativeSnapshotOverlay {
    pub expected: SnapshotOverlayFence,
    pub state: ScriptStateBuffer,
    pub native: Box<[ControlWrite]>,
    /// Native saved ordinals, resolved only against the current audio-owned roster.
    pub menus: Box<[super::SnapshotMenuPosition]>,
    /// At most two product generation/load gates.
    pub epochs: Box<[SnapshotOverlayEpoch]>,
    pub refusal: Option<SnapshotOverlayRefusal>,
    /// Admission acknowledgement, independent of callback completion/fault.
    pub committed: Option<SnapshotOverlayFence>,
    linearized: bool,
}
impl NativeSnapshotOverlay {
    pub fn new(
        expected: SnapshotOverlayFence,
        state: ScriptStateBuffer,
        native: Box<[ControlWrite]>,
    ) -> Self {
        Self {
            expected,
            state,
            native,
            menus: Box::new([]),
            epochs: Box::new([]),
            refusal: None,
            committed: None,
            linearized: false,
        }
    }

    /// Incoming preset semantics: preceding FIFO writes are retained, then
    /// snapshot homes replace their current values at this queue boundary.
    /// Requires at least one product-owned epoch gate. The exact current value
    /// fence is stamped immediately before the same complete preflight.
    pub fn linearized(plan: PlanId, state: ScriptStateBuffer, native: Box<[ControlWrite]>) -> Self {
        let mut overlay = Self::new(
            SnapshotOverlayFence {
                plan,
                control_revision: 0,
                script_revision: 0,
            },
            state,
            native,
        );
        overlay.linearized = true;
        overlay
    }

    pub(super) fn linearized_ingress(&self) -> bool {
        self.linearized
    }
}

impl Runtime {
    /// Pure audio-owner query: does not run timers or admit any callback.
    pub fn snapshot_overlay_fence(&self, plan: PlanId) -> Result<SnapshotOverlayFence, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if plan != self.active_plan {
            return Err(Error::StaleHandle);
        }
        Ok(SnapshotOverlayFence {
            plan,
            control_revision: generation.controls.revision,
            script_revision: generation.script_revision,
        })
    }

    fn snapshot_parked_listener(&self, id: crate::BehaviorId, plan: PlanId) -> bool {
        let Some(c) = self.behaviors.get(id.0) else {
            return false;
        };
        if c.owner != crate::BehaviorOwner::Plan(plan)
            || c.outcome.is_some()
            || !c.waiting
            || c.yielded_at.is_some()
            || c.async_wait.is_some()
            || c.async_result.is_some()
        {
            return false;
        }
        let Some(generation) = self.plans.get(plan.0) else {
            return false;
        };
        let Some(program) = generation.prepared.programs.get(c.program) else {
            return false;
        };
        let Some(body) = program.snapshot_listener_driver else {
            return false;
        };
        let Some(body) = generation.prepared.programs.get(body as usize) else {
            return false;
        };
        body.snapshot_overlay_safe
            && body.script_instance == program.script_instance
            && body.source_slot == program.source_slot
            && c.pc
                .checked_sub(1)
                .and_then(|pc| program.code.get(pc))
                .is_some_and(|op| {
                    matches!(
                        op,
                        crate::Instruction::Wait(_) | crate::Instruction::WaitLocal { .. }
                    )
                })
            && generation
                .prepared
                .plan_programs
                .iter()
                .any(|p| p.program == c.program && c.context.stage() == Some(p.stage))
    }

    fn preflight_snapshot_overlay(
        &self,
        overlay: &NativeSnapshotOverlay,
    ) -> Result<u64, SnapshotOverlayRefusal> {
        use SnapshotOverlayRefusal as R;
        let fence = overlay.expected;
        if overlay
            .state
            .values
            .len()
            .checked_add(overlay.native.len())
            .and_then(|count| count.checked_add(overlay.menus.len()))
            .is_none_or(|count| count > SNAPSHOT_OVERLAY_VALUE_CAPACITY)
            || overlay.state.callbacks.len() > 5
        {
            return Err(R::Capacity);
        }
        if fence.plan != self.active_plan {
            return Err(R::InactivePlan);
        }
        let generation = self.plans.get(fence.plan.0).ok_or(R::InactivePlan)?;
        if overlay.committed.is_some()
            || generation.controls.revision != fence.control_revision
            || generation.script_revision != fence.script_revision
        {
            return Err(R::RevisionConflict);
        }
        if overlay.epochs.len() > 2 {
            return Err(R::InvalidInput);
        }
        if overlay
            .epochs
            .iter()
            .any(|gate| gate.sequence.load(Ordering::Acquire) != gate.expected)
        {
            return Err(R::StaleEpoch);
        }
        if self.notes.count() != 0 || self.voices.count() != 0 || !self.deferred.is_empty() {
            return Err(R::ActiveEvents);
        }
        if self.plans.slots.iter().filter_map(|slot| slot.value.as_ref())
            .any(|generation| generation.dsp.buses.needs_render())
        {
            return Err(R::AudioTail);
        }
        if generation.start_cursor != generation.prepared.plan_programs.len()
            || generation.initial_residency.pending() != 0
        {
            return Err(R::PendingStartup);
        }
        if generation.controls.pending != 0
            || self.reserved_commands != 0
            || self.commands.iter().any(|command| match command.action {
                crate::schedule::Action::Resume(id) => {
                    command.at <= self.now || !self.snapshot_parked_listener(id, fence.plan)
                }
                _ => true,
            })
        {
            return Err(R::PendingCommands);
        }
        if !self.yielded.is_empty() || !self.behavior_ready.is_empty() || self.dispatching_behavior
        {
            return Err(R::ActiveCallbacks);
        }
        let mut next = self.behaviors.first;
        while let Some(index) = next {
            let slot = &self.behaviors.slots[index];
            next = slot.next;
            if slot.value.is_some_and(|c| c.outcome.is_none())
                && !self.snapshot_parked_listener(
                    crate::BehaviorId(self.behaviors.id(index)),
                    fence.plan,
                )
            {
                return Err(R::ActiveCallbacks);
            }
        }
        if !self.ops.effects.is_empty()
            || !generation.midi_object.jobs.is_empty()
            || generation
                .scripts
                .iter()
                .any(|bank| bank.array_files.jobs.iter().any(|job| job.id != 0))
            || self
                .residency_progress(fence.plan)
                .map_err(|_| R::InvalidInput)?
                .pending
                != 0
        {
            return Err(R::PendingServices);
        }
        let state = &overlay.state;
        if state
            .values
            .windows(2)
            .any(|v| v[0].address >= v[1].address)
            || state
                .callbacks
                .windows(2)
                .any(|v| v[0].program >= v[1].program)
            || overlay.native.windows(2).any(|v| v[0].id >= v[1].id)
            || overlay.menus.windows(2).any(|v| v[0].control >= v[1].control)
            || state
                .callbacks
                .iter()
                .any(|c| c.behavior.is_some() || c.outcome.is_some())
        {
            return Err(R::InvalidInput);
        }
        let next_revision = fence.control_revision.checked_add(1).ok_or(R::Capacity)?;
        generation
            .script_revision
            .checked_add(1)
            .ok_or(R::Capacity)?;
        for entry in &state.values {
            ResolvedStateWrite::resolve(generation, entry, &state.texts, true)
                .map_err(|_| R::InvalidInput)?;
            if let ScriptStateAddress::Control(id) = entry.address
                && (overlay
                    .native
                    .binary_search_by_key(&id, |row| row.id)
                    .is_ok()
                    || overlay
                        .menus
                        .binary_search_by_key(&id, |row| row.control)
                        .is_ok())
            {
                return Err(R::InvalidInput);
            }
        }
        for row in &overlay.native {
            if !self
                .control_definition(fence.plan, row.id)
                .map_err(|_| R::InvalidInput)?
                .domain
                .accepts(row.value)
            {
                return Err(R::InvalidInput);
            }
        }
        for row in &overlay.menus {
            if overlay.native.binary_search_by_key(&row.control, |row| row.id).is_ok() {
                return Err(R::InvalidInput);
            }
            super::snapshot_menu::menu_value(generation, row).map_err(|_| R::InvalidInput)?;
        }
        let mut prior = None;
        for callback in &state.callbacks {
            let program = generation
                .prepared
                .programs
                .get(callback.program)
                .ok_or(R::InvalidInput)?;
            let instance = program.script_instance.ok_or(R::InvalidInput)?;
            if prior.is_some_and(|prior| prior >= instance) {
                return Err(R::InvalidInput);
            }
            prior = Some(instance);
            if !program.snapshot_overlay_safe {
                return Err(R::UnsafeCallback);
            }
            if program.requires_note
                || program.requires_controller
                || program.wait_lifetime != crate::WaitLifetime::Callback
                || (program.requires_performance && self.performance_state.current.is_empty())
            {
                return Err(R::InvalidInput);
            }
        }
        // Unlike behavior_room, this pure preflight never reclaims a finished
        // owner. No capacity refusal can change existing callback diagnostics.
        if self.behaviors.available() < state.callbacks.len() {
            return Err(R::Capacity);
        }
        generation
            .callbacks
            .checked_add(state.callbacks.len())
            .ok_or(R::Capacity)?;
        crate::prepare::zone_routing::callback_id_preflight(
            self.last_callback_id, self.callback_id_reservations, state.callbacks.len())
            .map_err(|_| R::Capacity)?;
        // Preparation may be canceled while the bounded roster is checked.
        // This last acquire is the snapshot's admission linearization point.
        if overlay
            .epochs
            .iter()
            .any(|gate| gate.sequence.load(Ordering::Acquire) != gate.expected)
        {
            return Err(R::StaleEpoch);
        }
        Ok(next_revision)
    }

    /// Whole-batch, allocation-free commit. No apply_due or initializer runs.
    pub fn apply_snapshot_overlay(
        &mut self,
        overlay: &mut NativeSnapshotOverlay,
    ) -> Result<(usize, u64), Error> {
        self.apply_snapshot_overlay_with_gate(overlay, || true)
    }

    /// Check host-owned audio state after the complete pure core preflight and
    /// before any writes or callback admission. The gate runs once on a valid
    /// candidate; refusal neither drains native work nor changes the owner.
    pub fn apply_snapshot_overlay_with_gate(
        &mut self,
        overlay: &mut NativeSnapshotOverlay,
        snapshot_gate: impl FnOnce() -> bool,
    ) -> Result<(usize, u64), Error> {
        if overlay.linearized {
            if overlay.epochs.is_empty() {
                overlay.refusal = Some(SnapshotOverlayRefusal::InvalidInput);
                return Err(Error::InvalidInput);
            }
            match self.snapshot_overlay_fence(overlay.expected.plan) {
                Ok(fence) => overlay.expected = fence,
                Err(error) => {
                    overlay.refusal = Some(SnapshotOverlayRefusal::InactivePlan);
                    return Err(error);
                }
            }
        }
        let next_revision = match self.preflight_snapshot_overlay(overlay) {
            Ok(revision) => revision,
            Err(refusal) => {
                if refusal == SnapshotOverlayRefusal::Capacity {
                    self.record_control_admission_failure(overlay.state.callbacks.len());
                }
                overlay.refusal = Some(refusal);
                return Err(refusal.error());
            }
        };
        if !snapshot_gate() {
            overlay.refusal = Some(SnapshotOverlayRefusal::HostState);
            return Err(Error::NotReady);
        }
        overlay.refusal = None;
        let plan = overlay.expected.plan;
        for callback in &mut overlay.state.callbacks {
            let instance = self.plans.get(plan.0).unwrap().prepared.programs[callback.program]
                .script_instance
                .unwrap();
            let id = self
                .admit_plan_context(plan, callback.program, self.script_state_context(instance))
                .expect("preflighted snapshot callback admission");
            callback.behavior = Some(id);
            self.plans.get_mut(plan.0).unwrap().scripts[usize::from(instance.0)]
                .persistence_callback = Some((id, None));
        }
        let generation = self.plans.get_mut(plan.0).unwrap();
        // The pure whole-batch preflight resolved all of these destinations.
        // Admission ran no authored code; prior writes cannot change their
        // domains, bank shapes, producer slots or the sparse menu roster.
        for row in &overlay.native {
            let write = ResolvedStateWrite::control(generation, row.id, row.value)?;
            write.apply(generation, &overlay.state.texts, self.now);
        }
        for entry in &overlay.state.values {
            let write = ResolvedStateWrite::resolve(generation, entry, &overlay.state.texts, true)?;
            write.apply(generation, &overlay.state.texts, self.now);
        }
        for row in &overlay.menus {
            let value = super::snapshot_menu::menu_value(generation, row)?;
            let write = ResolvedStateWrite::control(generation, row.control, value)?;
            write.apply(generation, &overlay.state.texts, self.now);
        }
        generation.controls.revision = next_revision;
        generation.script_revision += 1;
        overlay.committed = Some(SnapshotOverlayFence {
            plan,
            control_revision: next_revision,
            script_revision: generation.script_revision,
        });
        for callback in &mut overlay.state.callbacks {
            let id = callback.behavior.unwrap();
            self.resume_behavior(id);
            callback.outcome = self.behavior_outcome(id)?;
        }
        Ok((
            overlay.state.values.len() + overlay.native.len() + overlay.menus.len(),
            self.control_revision(plan)?,
        ))
    }
}
