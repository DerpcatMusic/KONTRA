//! Presentation-independent control state. Metadata is prepared off audio; values
//! have one audio-side writer and are captured into caller-owned storage.
mod script_state;
mod snapshot_overlay;
pub(crate) mod snapshot_menu;
pub use snapshot_menu::{SnapshotMenuPosition, WidgetMenuStorage};
mod snapshot_program;
pub(crate) use snapshot_program::snapshot_callback_safe;
pub use snapshot_overlay::{
    NativeSnapshotOverlay, SnapshotOverlayEpoch, SnapshotOverlayFence, SnapshotOverlayRefusal,
    SNAPSHOT_OVERLAY_VALUE_CAPACITY,
};
pub use script_state::{
    ScriptStateAddress, ScriptStateBuffer, ScriptStateCallback, ScriptStateEntry, ScriptStateValue,
};

use super::{Error, Instruction, PlanId, Prepared, Runtime};
mod transfer;
pub(super) use transfer::ControlQueues;
pub use transfer::{
    ControlClient, ControlOperation, ControlQueueError, ControlReply, ControlRequest,
    RejectedControls,
};

/// Source-free ownership sample taken at a refused callback admission. Numeric
/// program/stage addresses belong to `plan`; they contain no script values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehaviorAdmissionOwner {
    pub owner: crate::BehaviorOwner,
    pub plan: PlanId,
    pub program: usize,
    pub source_slot: i32,
    pub stage: usize,
    pub pc: usize,
    pub waiting: bool,
    pub preempted: bool,
    pub outcome: Option<crate::Outcome>,
}

/// Failure-time callback ownership, returned with a rejected control operation.
/// The fixed-size samples and capped rejection-only traversal never allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehaviorAdmissionFailure {
    pub required_callbacks: usize,
    pub used: usize,
    pub capacity: usize,
    pub available: usize,
    pub reserved: usize,
    /// Counts below cover only `scanned` occupied continuations.
    pub scanned: usize,
    pub scan_truncated: bool,
    pub note_owned: usize,
    pub plan_owned: usize,
    pub waiting: usize,
    pub preempted: usize,
    pub terminal: usize,
    pub finished: usize,
    /// Scheduler queue depth, including callbacks parked behind an earlier yield.
    pub yielded_queue: usize,
    pub pending_commands: usize,
    pub reserved_commands: usize,
    pub stalled_effects: u64,
    pub outbox_used: usize,
    pub outbox_capacity: usize,
    /// Occupied-list order, with an explicit count of omitted owner samples.
    pub owners: [Option<BehaviorAdmissionOwner>; 8],
    pub owners_omitted: usize,
}

/// Persistent semantic identity assigned by the source frontend/composition root.
/// Never derive this from a widget position, dense index or randomized hash.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "cache", serde(try_from = "String", into = "String"))]
/// 8-byte aligned: a u128 forces 16, padding every enum that holds one
/// (script instructions and saved-state addresses grow from 24 to 32 bytes).
#[repr(packed(8))]
pub struct ControlId(pub u128);
#[cfg(feature = "cache")]
impl From<ControlId> for String {
    fn from(id: ControlId) -> Self {
        format!("{:032x}", { id.0 })
    }
}
#[cfg(feature = "cache")]
impl TryFrom<String> for ControlId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        if value.len() != 32 {
            return Err(Error::InvalidInput);
        }
        u128::from_str_radix(&value, 16)
            .map(Self)
            .map_err(|_| Error::InvalidInput)
    }
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
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

// Logical IDs remain the public schema; these handles address homogeneous cells.
// Handles are minted/remapped only against the owning Prepared's exact layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RealControlIndex(usize);
impl RealControlIndex {
    #[cfg(test)]
    pub(crate) fn test(slot: usize) -> Self {
        Self(slot)
    }
}
/// Retain the historical non-Real engine-binding admission explicitly. Its
/// offset is recorded, while its playing scalar remains its base scalar.
#[derive(Clone, Copy)]
enum EngineSlot {
    Integer(usize),
    Real {
        slot: RealControlIndex,
        playback: usize,
    },
    Toggle(usize),
}
#[derive(Clone, Copy)]
pub(super) struct EngineControl {
    slot: EngineSlot,
}
#[derive(Clone, Copy, Debug)]
enum ControlKind {
    Integer(usize),
    Real(RealControlIndex),
    Toggle(usize),
}
/// Logical scalar ownership and the last sorted binding's validation law.
/// All scalar domains participate, including historically admitted non-Real
/// engine bindings. This replaces the Real-only binding metadata.
#[derive(Clone, Copy)]
struct ControlSlot {
    kind: ControlKind,
    engine: Option<usize>,
}
#[derive(Clone, Copy)]
struct IntegerDefinition {
    min: i64,
    max: i64,
    default: i64,
}
#[derive(Clone, Copy)]
struct RealDefinition {
    logical: usize,
    min: f64,
    max: f64,
    default: f64,
}

#[derive(Default)]
pub(super) struct ControlLayout {
    slots: Box<[ControlSlot]>,
    integers: Box<[IntegerDefinition]>,
    reals: Box<[RealDefinition]>,
    toggles: Box<[bool]>,
}
impl ControlLayout {
    fn new(definitions: &[ControlDefinition]) -> Result<Self, Error> {
        let counts = definitions.iter().fold([0usize; 3], |mut counts, c| {
            counts[match c.domain {
                ControlDomain::Integer { .. } => 0,
                ControlDomain::Real { .. } => 1,
                ControlDomain::Toggle => 2,
            }] += 1;
            counts
        });
        let mut slots = Vec::with_capacity(definitions.len());
        let mut integers = Vec::with_capacity(counts[0]);
        let mut reals = Vec::with_capacity(counts[1]);
        let mut toggles = Vec::with_capacity(counts[2]);
        for (logical, c) in definitions.iter().enumerate() {
            if !c.domain.accepts(c.default) {
                return Err(Error::InvalidInput);
            }
            slots.push(ControlSlot {
                kind: match (c.domain, c.default) {
                    (ControlDomain::Integer { min, max }, ControlValue::Integer(default)) => {
                        let slot = integers.len();
                        integers.push(IntegerDefinition { min, max, default });
                        ControlKind::Integer(slot)
                    }
                    (ControlDomain::Real { min, max }, ControlValue::Real(default)) => {
                        let slot = RealControlIndex(reals.len());
                        reals.push(RealDefinition {
                            logical,
                            min,
                            max,
                            default,
                        });
                        ControlKind::Real(slot)
                    }
                    (ControlDomain::Toggle, ControlValue::Toggle(default)) => {
                        let slot = toggles.len();
                        toggles.push(default);
                        ControlKind::Toggle(slot)
                    }
                    _ => return Err(Error::InvalidInput),
                },
                engine: None,
            });
        }
        Ok(Self {
            slots: slots.into_boxed_slice(),
            integers: integers.into_boxed_slice(),
            reals: reals.into_boxed_slice(),
            toggles: toggles.into_boxed_slice(),
        })
    }
    pub(super) fn real(&self, logical: usize) -> Result<RealControlIndex, Error> {
        match self.slots.get(logical).map(|slot| slot.kind) {
            Some(ControlKind::Real(slot)) => Ok(slot),
            _ => Err(Error::InvalidInput),
        }
    }
    pub(crate) fn initial(&self, logical: usize) -> ControlProjection {
        match self.slots[logical].kind {
            ControlKind::Integer(slot) => {
                let c = self.integers[slot];
                ControlProjection::Integer {
                    min: c.min,
                    max: c.max,
                    value: c.default,
                }
            }
            ControlKind::Real(slot) => {
                let c = self.reals[slot.0];
                ControlProjection::Real {
                    min: c.min,
                    max: c.max,
                    value: c.default,
                }
            }
            ControlKind::Toggle(slot) => ControlProjection::Toggle(self.toggles[slot]),
        }
    }
    fn edit(&self, index: usize, value: ControlValue) -> Result<ControlEdit, Error> {
        let value = match (self.slots[index].kind, value) {
            (ControlKind::Integer(slot), ControlValue::Integer(value)) => {
                let c = self.integers[slot];
                if !(c.min..=c.max).contains(&value) {
                    return Err(Error::InvalidInput);
                }
                TypedEdit::Integer { slot, value }
            }
            (ControlKind::Real(slot), ControlValue::Real(value)) => {
                let c = self.reals[slot.0];
                if !value.is_finite() || !(c.min..=c.max).contains(&value) {
                    return Err(Error::InvalidInput);
                }
                TypedEdit::Real { slot, value }
            }
            (ControlKind::Toggle(slot), ControlValue::Toggle(value)) => {
                TypedEdit::Toggle { slot, value }
            }
            _ => return Err(Error::InvalidInput),
        };
        Ok(ControlEdit { index, value })
    }
}

/// The value and its scalar domain are one exhaustive input to DSP projection.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ControlProjection {
    Integer { min: i64, max: i64, value: i64 },
    Real { min: f64, max: f64, value: f64 },
    Toggle(bool),
}
#[derive(Clone, Copy)]
enum TypedEdit {
    Integer { slot: usize, value: i64 },
    Real { slot: RealControlIndex, value: f64 },
    Toggle { slot: usize, value: bool },
}
#[derive(Clone, Copy)]
pub(super) struct ControlEdit {
    index: usize,
    value: TypedEdit,
}
#[derive(Clone, Copy)]
pub(crate) struct RealCell {
    base: f64,
    playing: f64,
    offset: f32,
}
impl RealCell {
    fn new(value: f64) -> Self {
        Self {
            base: value,
            playing: value,
            offset: 0.,
        }
    }
    #[cfg(test)]
    pub(crate) fn test(value: f64) -> Self {
        Self::new(value)
    }
}
#[derive(Clone, Copy)]
pub(crate) struct RealValuesRef<'a>(&'a [RealCell]);
impl<'a> RealValuesRef<'a> {
    pub(crate) fn new(cells: &'a [RealCell]) -> Self {
        Self(cells)
    }
    pub(crate) fn get(self, slot: RealControlIndex) -> f64 {
        self.0[slot.0].base
    }
}

#[derive(Clone, Copy)]
struct IntegerCell {
    value: i64,
    offset: f32,
}
#[derive(Clone, Copy)]
struct ToggleCell {
    value: bool,
    offset: f32,
}
pub(super) struct ControlState {
    // Integer/toggle values have no offset law: their base is also their playing value.
    integers: Box<[IntegerCell]>,
    reals: Box<[RealCell]>,
    toggles: Box<[ToggleCell]>,
    pub(super) automation: Box<[crate::automation::AutomationState]>,
    pub(super) revision: u64,
    /// Future writes reserve both this generation and one revision increment each.
    pub(super) pending: usize,
}
impl ControlState {
    pub(super) fn new(plan: &Prepared) -> Self {
        Self {
            integers: plan
                .control_layout
                .integers
                .iter()
                .map(|c| IntegerCell {
                    value: c.default,
                    offset: 0.,
                })
                .collect(),
            reals: plan
                .control_layout
                .reals
                .iter()
                .map(|c| RealCell::new(c.default))
                .collect(),
            toggles: plan
                .control_layout
                .toggles
                .iter()
                .map(|&value| ToggleCell { value, offset: 0. })
                .collect(),
            automation: vec![crate::automation::AutomationState::default(); plan.automation.len()]
                .into_boxed_slice(),
            revision: 0,
            pending: 0,
        }
    }
    pub(super) fn base_reals(&self) -> RealValuesRef<'_> {
        RealValuesRef::new(&self.reals)
    }
    pub(super) fn real_value(&self, slot: RealControlIndex) -> f64 {
        self.reals[slot.0].playing
    }
    pub(super) fn real_base(&self, slot: RealControlIndex) -> f64 {
        self.reals[slot.0].base
    }
    pub(super) fn offset(&self, control: EngineControl) -> f32 {
        match control.slot {
            EngineSlot::Integer(slot) => self.integers[slot].offset,
            EngineSlot::Real { slot, .. } => self.reals[slot.0].offset,
            EngineSlot::Toggle(slot) => self.toggles[slot].offset,
        }
    }
    pub(super) fn engine_base(&self, control: EngineControl) -> Option<f64> {
        match control.slot {
            EngineSlot::Real { slot, .. } => Some(self.real_base(slot)),
            EngineSlot::Integer(_) | EngineSlot::Toggle(_) => None,
        }
    }
    fn value(&self, layout: &ControlLayout, index: usize, base: bool) -> ControlValue {
        match layout.slots[index].kind {
            ControlKind::Integer(slot) => ControlValue::Integer(self.integers[slot].value),
            ControlKind::Real(slot) => {
                let c = self.reals[slot.0];
                ControlValue::Real(if base { c.base } else { c.playing })
            }
            ControlKind::Toggle(slot) => ControlValue::Toggle(self.toggles[slot].value),
        }
    }
    fn refresh_real(&mut self, prepared: &Prepared, slot: RealControlIndex) -> ControlProjection {
        let c = &mut self.reals[slot.0];
        let definition = prepared.control_layout.reals[slot.0];
        // Plain controls have no offset owner; this is their complete playing law.
        // Engine-owned controls use the same last-binding-wins law as before.
        c.playing = match prepared.control_layout.slots[definition.logical].engine {
            None => c.base,
            Some(binding) => offset_value(
                c.base,
                c.offset,
                &prepared.engine_parameters[binding],
                definition,
            ),
        };
        ControlProjection::Real {
            min: definition.min,
            max: definition.max,
            value: c.playing,
        }
    }
    pub(super) fn set_offset(
        &mut self,
        prepared: &Prepared,
        control: EngineControl,
        offset: f32,
    ) -> ControlProjection {
        match control.slot {
            EngineSlot::Integer(slot) => {
                let c = &mut self.integers[slot];
                c.offset = offset;
                let definition = prepared.control_layout.integers[slot];
                ControlProjection::Integer {
                    min: definition.min,
                    max: definition.max,
                    value: c.value,
                }
            }
            EngineSlot::Real { slot, playback } => {
                let c = &mut self.reals[slot.0];
                let definition = prepared.control_layout.reals[slot.0];
                c.offset = offset;
                c.playing = offset_value(
                    c.base,
                    offset,
                    &prepared.engine_parameters[playback],
                    definition,
                );
                ControlProjection::Real {
                    min: definition.min,
                    max: definition.max,
                    value: c.playing,
                }
            }
            EngineSlot::Toggle(slot) => {
                let c = &mut self.toggles[slot];
                c.offset = offset;
                ControlProjection::Toggle(c.value)
            }
        }
    }
    pub(super) fn apply(
        &mut self,
        prepared: &Prepared,
        edit: ControlEdit,
    ) -> (ControlId, ControlProjection) {
        let projection = match edit.value {
            TypedEdit::Integer { slot, value } => {
                self.integers[slot].value = value;
                let c = prepared.control_layout.integers[slot];
                ControlProjection::Integer {
                    min: c.min,
                    max: c.max,
                    value,
                }
            }
            TypedEdit::Real { slot, value } => {
                self.reals[slot.0].base = value;
                self.refresh_real(prepared, slot)
            }
            TypedEdit::Toggle { slot, value } => {
                self.toggles[slot].value = value;
                ControlProjection::Toggle(value)
            }
        };
        (prepared.controls[edit.index].id, projection)
    }
}

fn offset_value(
    base: f64,
    offset: f32,
    binding: &crate::EngineParameterBinding,
    definition: RealDefinition,
) -> f64 {
    if offset == 0. {
        return base;
    }
    let norm = binding.law.encode(base);
    let to = (f64::from(norm) + f64::from(offset) * 1e6).round() as i32;
    binding.law.decode(to).clamp(definition.min, definition.max)
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
        let layout = ControlLayout::new(&controls)?;
        let previous_layout = std::mem::replace(&mut self.control_layout, layout);
        let previous = std::mem::replace(&mut self.controls, controls.into_boxed_slice());
        self.validate_program_controls(&self.programs)?;
        self.validate_dsp_controls()?;
        for binding in &self.control_programs {
            self.control_index(binding.control)?;
        }
        for control in self
            .engine_parameters
            .iter()
            .map(|b| b.control)
            .chain(self.envelope_controls.iter().flatten().flatten().copied())
        {
            let index = self.control_index(control)?;
            if !matches!(self.controls[index].domain, ControlDomain::Real { .. }) {
                return Err(Error::InvalidInput);
            }
        }
        for lanes in self.wavetable_controls.iter().flatten() {
            for (lane, control) in lanes.iter().enumerate() {
                let index = self.control_index(*control)?;
                if self.controls[index].domain
                    != (ControlDomain::Real {
                        min: 0.,
                        max: if lane < 3 { 1. } else { 33. },
                    })
                {
                    return Err(Error::InvalidInput);
                }
                if lane >= 3 {
                    let ControlValue::Real(value) = self.controls[index].default else {
                        return Err(Error::InvalidInput);
                    };
                    crate::EngineParameterLaw::WavetableForm.normalized_value(value)?;
                }
            }
        }
        for control in self
            .script_initial
            .iter()
            .flat_map(|b| b.controls.iter().flatten())
        {
            let index = self.control_index(*control)?;
            if !matches!(self.controls[index].domain, ControlDomain::Integer { .. }) {
                return Err(Error::InvalidInput);
            }
        }
        for descriptor in self.parameter_registry.descriptors() {
            self.control_index(descriptor.control)?;
        }
        for widget in &self.widgets {
            if let crate::WidgetStorage::Control(id) = widget.storage {
                self.control_index(id)?;
            }
        }
        self.refresh_real_controls()?;
        let controls = &self.controls;
        let layout = &self.control_layout;
        self.voice_modulation.remap_controls(|slot| {
            // The previous schema is the identity of every compiled input cell.
            let logical = previous_layout.reals.get(slot.0).ok_or(Error::InvalidInput)?.logical;
            let id = previous.get(logical).ok_or(Error::InvalidInput)?.id;
            let index = controls
                .binary_search_by_key(&id, |c| c.id)
                .map_err(|_| Error::InvalidInput)?;
            if !matches!(controls[index].domain, ControlDomain::Real { .. }) {
                return Err(Error::InvalidInput);
            }
            layout.real(index)
        })?;
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

    pub(super) fn real_control(&self, id: ControlId) -> Result<RealControlIndex, Error> {
        self.control_layout.real(self.control_index(id)?)
    }
    pub(super) fn resolve_control(
        &self,
        id: ControlId,
        value: ControlValue,
    ) -> Result<ControlEdit, Error> {
        self.control_layout.edit(self.control_index(id)?, value)
    }
    /// Rebuild physical handles whenever IDs or attached metadata change, before publication.
    pub(super) fn refresh_real_controls(&mut self) -> Result<(), Error> {
        for slot in &mut self.control_layout.slots {
            slot.engine = None;
        }
        let mut engine_controls = Vec::with_capacity(self.engine_parameters.len());
        for (playback, b) in self.engine_parameters.iter().enumerate() {
            let logical = self.control_index(b.control)?;
            let slot = match self.control_layout.slots[logical].kind {
                ControlKind::Integer(slot) => EngineSlot::Integer(slot),
                ControlKind::Real(slot) => EngineSlot::Real { slot, playback },
                ControlKind::Toggle(slot) => EngineSlot::Toggle(slot),
            };
            self.control_layout.slots[logical].engine = Some(playback);
            engine_controls.push(EngineControl { slot });
        }
        for control in &mut engine_controls {
            if let EngineSlot::Real { slot, playback } = &mut control.slot {
                let logical = self.control_layout.reals[slot.0].logical;
                *playback = self.control_layout.slots[logical]
                    .engine
                    .ok_or(Error::InvalidInput)?;
            }
        }
        self.engine_controls = engine_controls.into_boxed_slice();
        let mut envelopes = Vec::with_capacity(self.envelope_controls.len());
        for ids in &self.envelope_controls {
            let mut slots = [None; 6];
            for (slot, id) in slots.iter_mut().zip(ids) {
                *slot = id.map(|id| self.real_control(id)).transpose()?;
            }
            envelopes.push(slots);
        }
        self.envelope_real = envelopes.into_boxed_slice();
        let mut waves = Vec::with_capacity(self.wavetable_controls.len());
        for ids in &self.wavetable_controls {
            waves.push(
                ids.map(|[a, b, c, d, e]| {
                    Ok([
                        self.real_control(a)?,
                        self.real_control(b)?,
                        self.real_control(c)?,
                        self.real_control(d)?,
                        self.real_control(e)?,
                    ])
                })
                .transpose()?,
            );
        }
        self.wavetable_real = waves.into_boxed_slice();
        Ok(())
    }
    pub(super) fn validate_program_controls(
        &self,
        programs: &[super::Program],
    ) -> Result<(), Error> {
        for op in super::behavior::distinct_code(programs, false).flat_map(|p| p.code.iter()) {
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

impl crate::Generation {
    pub(super) fn project_wavetable(
        &self,
        cursor: &mut crate::source::Cursor,
        group: Option<u32>,
        voice: usize,
    ) {
        if cursor.wavetable().is_none() {
            return;
        }
        if let Some(Some(lanes)) =
            group.and_then(|g| self.prepared.wavetable_real.get(g as usize))
        {
            cursor.set_wavetable_controls(lanes.map(|slot| self.controls.real_value(slot) as f32));
        }
        cursor.set_wavetable_offsets(self.modulation.wavetable_offsets(voice));
    }
}

impl Runtime {
    /// One cross-format host Tone, independent of authored voice/filter controls.
    pub fn set_part_tone_cutoff(&mut self, cutoff: f64) -> Result<(), Error> {
        if !(20.0..=20_000.).contains(&cutoff) {
            return Err(Error::InvalidInput);
        }
        self.part_tone_cutoff = cutoff;
        Ok(())
    }

    /// A mixed marked/unmarked generation set uses the adapter's single output fallback.
    pub fn has_input_tone(&self) -> bool {
        self.plans
            .slots
            .iter()
            .filter_map(|s| s.value.as_ref())
            .all(|g| g.prepared.buses.input.is_some())
    }

    /// v1 engine defaults: captured by new fallback voices, never authored envelopes.
    pub fn set_fallback_envelope(&mut self, attack: f64, release: f64) -> Result<(), Error> {
        if !(0.0001..=5.).contains(&attack) || !(0.001..=10.).contains(&release) {
            return Err(Error::InvalidInput);
        }
        self.fallback_envelope = Some([
            (attack * f64::from(self.rate)).round() as u32,
            (release * f64::from(self.rate)).round() as u32,
        ]);
        Ok(())
    }

    pub(crate) fn region_envelope(
        &self,
        envelope: crate::Envelope,
        fallback: bool,
    ) -> crate::Envelope {
        if fallback && let Some([attack, release]) = self.fallback_envelope {
            envelope
                .with_stage(crate::EnvelopeStage::Attack, attack)
                .with_stage(crate::EnvelopeStage::Release, release)
        } else {
            envelope
        }
    }

    pub(crate) fn controlled_envelope(
        &self,
        plan: PlanId,
        group: Option<u32>,
        envelope: crate::Envelope,
    ) -> crate::Envelope {
        let generation = self.plans.get(plan.0).unwrap();
        let mut envelope = generation.script.envelope(group, envelope);
        if let Some(controls) =
            group.and_then(|g| generation.prepared.envelope_real.get(g as usize))
        {
            for (stage, id) in crate::engine_parameters::ENVELOPE_STAGES
                .into_iter()
                .zip(controls)
            {
                if let Some(slot) = id {
                    let value = generation.controls.real_value(*slot);
                    if value != generation.prepared.control_layout.reals[slot.0].default {
                        envelope = envelope.with_control(stage, value);
                    }
                }
            }
        }
        envelope
    }

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
        // Pure validation must precede resource admission: a malformed edit is
        // still malformed while other callbacks own the arena. No values or
        // revisions change until the complete callback preflight also succeeds.
        self.validate_controls(plan, expected_revision, &[write])?;
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
        Ok(generation.controls.value(&generation.prepared.control_layout, generation.prepared.control_index(id)?, false))
    }

    /// The authored/script value before the player's editor offsets. Use this
    /// for host persistence so recalling an offset cannot apply it twice.
    pub fn control_base_value(&self, plan: PlanId, id: ControlId) -> Result<ControlValue, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        Ok(generation.controls.value(&generation.prepared.control_layout, generation.prepared.control_index(id)?, true))
    }

    /// The control's id, domain and default in `plan`.
    /// The immutable schema of an addressed generation, for an off-audio producer.
    pub fn control_definitions(&self, plan: PlanId) -> Result<&[ControlDefinition], Error> {
        Ok(&self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .controls)
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
        for (index, (slot, definition)) in output.iter_mut().zip(definitions.iter()).enumerate() {
            *slot = ControlWrite { id:definition.id, value:generation.controls.value(&generation.prepared.control_layout,index,false) };
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
            let edit = definitions.control_layout.edit(index, write.value)?;
            if let Some(binding) = definitions.control_layout.slots[index]
                .engine
                .map(|i| &definitions.engine_parameters[i])
                && binding.law == crate::EngineParameterLaw::WavetableForm
            {
                match edit.value {
                    TypedEdit::Real { value, .. } => {
                        binding.law.normalized_value(value)?;
                    }
                    TypedEdit::Integer { .. } | TypedEdit::Toggle { .. } => {
                        return Err(Error::InvalidInput);
                    }
                }
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
            // Recreate one typed row token after the complete pure preflight.
            // The schema and borrowed writes cannot change between these passes.
            let edit = definitions.resolve_control(write.id,write.value)?;
            let (id,projection)=generation.controls.apply(definitions,edit);
            generation.dsp.edit_control(definitions,id,projection,self.now);
        }
        generation.controls.revision = revision;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_control_owner_layout_receipt() {
        eprintln!("D layout bytes: slot={} integer_definition={} real_definition={} integer_cell={} real_cell={} toggle_cell={} engine_handle={} projection={} edit={} real_handle={} optional_real={} wavetable_row={} layout={} state={} resolved_row={}",
            size_of::<ControlSlot>(),size_of::<IntegerDefinition>(),size_of::<RealDefinition>(),
            size_of::<IntegerCell>(),size_of::<RealCell>(),size_of::<ToggleCell>(),
            size_of::<EngineControl>(),size_of::<ControlProjection>(),size_of::<ControlEdit>(),
            size_of::<RealControlIndex>(),size_of::<Option<RealControlIndex>>(),
            size_of::<Option<[RealControlIndex;5]>>(),size_of::<ControlLayout>(),size_of::<ControlState>(),size_of::<script_state::ResolvedStateWrite>());
        assert!(size_of::<ControlEdit>()<=32);
        assert!(size_of::<ControlProjection>()<=32);
    }

    #[test]
    fn schema_replacement_rejects_invalid_compiled_input_index() {
        let definitions: Vec<_> = [ControlId(1), ControlId(2)]
            .into_iter()
            .map(|id| ControlDefinition {
                id,
                domain: ControlDomain::Real { min: 0., max: 1. },
                default: ControlValue::Real(0.),
            })
            .collect();
        let mut plan = Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_controls(definitions.clone())
            .unwrap();
        // Corrupt preparation data is not reachable through the public resolver.
        plan.voice_modulation = crate::voice_mod::VoiceModulation::new_resolved(
            vec![crate::ModProgram {
                controls: vec![(0, [ControlId(1), ControlId(2)])],
                sources: vec![crate::ModSource::Constant],
                routes: vec![crate::ModRoute {
                    source: 0,
                    target: crate::ModTarget::Pan,
                    depth: 0.5,
                    invert: false,
                    shape: None,
                    lag: 0,
                    scale: None,
                    live_gain: 1.,
                    unipolar: false,
                    more_scales: [None; crate::voice_mod::MORE_SCALES], native: None, }],
                ..Default::default()
            }],
            vec![],
            vec![],
            Vec::new(),
            |_| None,
            |id| Ok(RealControlIndex(if id == ControlId(1) { 0 } else { 9 })),
        )
        .unwrap();
        assert!(matches!(
            plan.with_controls(definitions),
            Err(Error::InvalidInput)
        ));
    }

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
