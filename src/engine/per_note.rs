//! Native per-note wire controls are shared by a physical address until Detach.
//! Exact host overlays and MPE member snapshots keep their separate contracts.
use std::collections::HashMap;

use super::Expression;
#[cfg(test)]
use super::HostExpression;
use crate::midi::{Bend, ControlValue, NoteControl};

/// Physical MIDI input address; independent of the library's delivered channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InputAddress {
    pub port: u8,
    pub channel: u8,
    pub key: u8,
}

impl InputAddress {
    fn valid(self) -> bool { self.channel < 16 && self.key < 128 }
}

/// Shared native control generation, inherited by a note's generated children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PerNoteRef {
    index: u16,
    generation: u32,
}

/// Root-note input captured before a script can wait or change the delivered key.
/// Generated notes inherit this binding, including its original bend sensitivity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerNoteInput {
    pub(crate) controls: PerNoteRef,
    pub(crate) address: InputAddress,
    pub(crate) bend_range: f64,
    pub(crate) attribute_pitch: Option<f64>,
    pub(crate) pitch_revision: u64,
}

#[derive(Clone, Copy)]
struct Slot {
    address: InputAddress,
    expression: Expression,
    controllers: [Option<ControlValue>; 128],
    bend: Bend,
    pitch: Option<f64>,
    pitch_revision: u64,
    generation: u32,
    live: bool,
    current: bool,
    pinned: bool,
}

impl Slot {
    fn supports_controller(index: u8) -> bool {
        matches!(index, 1 | 2 | 7 | 8 | 10 | 11 | 70..=79 | 91..=95)
    }

    fn pitch(&mut self, value: f64) {
        self.pitch = Some(value);
        self.pitch_revision = self.pitch_revision.saturating_add(1);
    }

    fn controller(&mut self, index: u8, value: ControlValue) -> bool {
        if !Self::supports_controller(index) { return false; }
        self.controllers[index as usize] = Some(value);
        match index {
            7 => self.expression.gain = (value.normalized() * 4.0) as f32,
            10 => self.expression.pan = (value.normalized() * 2.0 - 1.0) as f32,
            74 => self.expression.note_cc74 = Some(value),
            _ => {},
        }
        true
    }
}

/// Prepared bounded storage for current addresses and detached sounding tails.
/// Current records persist across key-up and reuse, including pre-On controls.
pub(crate) struct InputControls {
    slots: Box<[Slot]>,
    free: Vec<u16>,
    active: Vec<PerNoteRef>,
    current: HashMap<InputAddress, PerNoteRef>,
    pub(crate) dropped: u64,
}

impl InputControls {
    pub(crate) fn new() -> Self {
        let count = crate::ksp::EVENT_CAPACITY;
        const { assert!(crate::ksp::EVENT_CAPACITY <= u16::MAX as usize); }
        let empty = Slot { address: InputAddress { port: 0, channel: 0, key: 0 },
            expression: Expression::default(), controllers: [None; 128], bend: Bend::ZERO,
            pitch: None, pitch_revision: 0, generation: 0,
            live: false, current: false, pinned: false };
        Self {
            slots: vec![empty; count].into_boxed_slice(),
            free: (0..count as u16).rev().collect(),
            active: Vec::with_capacity(count),
            current: HashMap::with_capacity(count),
            dropped: 0,
        }
    }

    /// Prepared script-free bank programs do not own physical address-current
    /// generations. This first capability has no per-note input registry.
    pub(super) fn disabled() -> Self {
        Self { slots: Box::new([]), free: Vec::new(), active: Vec::new(),
            current: HashMap::new(), dropped: 0 }
    }

    fn allocate(&mut self, address: InputAddress, state: Option<Slot>) -> Option<PerNoteRef> {
        let Some(index) = self.free.pop() else {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        };
        let slot = &mut self.slots[index as usize];
        let generation = slot.generation.wrapping_add(1).max(1);
        *slot = Slot { address, generation, live: true, current: true, pinned: false,
            ..state.unwrap_or(Slot { address, expression: Expression::default(),
                controllers: [None; 128], bend: Bend::ZERO,
                pitch: None, pitch_revision: 0, generation: 0,
                live: false, current: false, pinned: false }) };
        let id = PerNoteRef { index, generation };
        self.active.push(id);
        Some(id)
    }

    pub(super) fn available(&self) -> usize { self.free.len() }
    pub(super) fn contains_address(&self, address: InputAddress) -> bool { self.current.contains_key(&address) }

    /// Borrow an existing physical lane for acceptance. A prospective lane has
    /// the same empty expression as allocation, without publishing an ID.
    pub(super) fn preview_overlay(&self, address: InputAddress, bend_range: f64,
        attribute_pitch: Option<f64>, mut expression: Expression) -> Expression {
        if let Some(&controls) = self.current.get(&address) {
            self.overlay(Some(PerNoteInput { controls, address, bend_range, attribute_pitch,
                pitch_revision: self.slots[controls.index as usize].pitch_revision }), expression)
        } else {
            if let Some(pitch) = attribute_pitch { expression.tune += (pitch - f64::from(address.key)) as f32; }
            expression
        }
    }

    /// Reject without taking another address's state when prepared capacity fills.
    pub(crate) fn attach(&mut self, address: InputAddress) -> Option<PerNoteRef> {
        if !address.valid() { return None; }
        let id = if let Some(&id) = self.current.get(&address) { id }
            else {
                let id = self.allocate(address, None)?;
                self.current.insert(address, id);
                id
            };
        self.pin(Some(id));
        Some(id)
    }

    pub(crate) fn input(&mut self, address: InputAddress, bend_range: f64,
        attribute_pitch: Option<f64>) -> Option<PerNoteInput> {
        if !bend_range.is_finite() || !(0.0..128.0).contains(&bend_range)
            || attribute_pitch.is_some_and(|pitch| !pitch.is_finite() || !(0.0..128.0).contains(&pitch)) {
            return None;
        }
        let controls = self.attach(address)?;
        Some(PerNoteInput { controls, address, bend_range, attribute_pitch,
            pitch_revision: self.slots[controls.index as usize].pitch_revision })
    }

    fn slot(&self, id: PerNoteRef) -> Option<&Slot> {
        self.slots.get(id.index as usize)
            .filter(|slot| slot.live && slot.generation == id.generation)
    }

    fn slot_mut(&mut self, id: PerNoteRef) -> Option<&mut Slot> {
        self.slots.get_mut(id.index as usize)
            .filter(|slot| slot.live && slot.generation == id.generation)
    }

    #[cfg(test)]
    pub(crate) fn read(&self, id: PerNoteRef) -> Option<Expression> {
        self.slot(id).map(|slot| slot.expression)
    }

    /// Update the current/future generation, even before any note is playing.
    /// True means accepted; repeated values do not imply capacity rejection.
    #[cfg(test)]
    pub(crate) fn update(&mut self, address: InputAddress, change: HostExpression) -> bool {
        // Invalid controls cannot consume prepared address capacity.
        let valid = match change {
            HostExpression::Gain(value) => value.is_finite() && (0.0..=4.0).contains(&value),
            HostExpression::Tune(value) => value.is_finite(),
            HostExpression::Pan(value) => value.is_finite() && (-1.0..=1.0).contains(&value),
            HostExpression::Brightness(_) | HostExpression::Pressure(_) => true,
        };
        if !valid { return false; }
        let Some(id) = self.attach(address) else { return false; };
        let x = &mut self.slots[id.index as usize].expression;
        match change {
            HostExpression::Gain(value) if value.is_finite() && (0.0..=4.0).contains(&value) => x.gain = value,
            HostExpression::Tune(value) if value.is_finite() => x.tune = value,
            HostExpression::Pan(value) if value.is_finite() && (-1.0..=1.0).contains(&value) => x.pan = value,
            HostExpression::Brightness(value) => x.note_cc74 = Some(value),
            HostExpression::Pressure(value) => x.note_pressure = Some(value),
            _ => return false,
        }
        true
    }

    /// Preserve wire bend precision; each root evaluates its captured sensitivity.
    #[cfg(test)]
    pub(crate) fn bend(&mut self, address: InputAddress, value: Bend) -> bool {
        let Some(id) = self.attach(address) else { return false; };
        self.slots[id.index as usize].bend = value;
        true
    }

    /// Registered per-note controller 3 sets persistent absolute Pitch 7.25.
    #[cfg(test)]
    pub(crate) fn pitch(&mut self, address: InputAddress, value: f64) -> bool {
        if !value.is_finite() || !(0.0..128.0).contains(&value) { return false; }
        let Some(id) = self.attach(address) else { return false; };
        self.slots[id.index as usize].pitch(value);
        true
    }

    /// Accept against the generation captured at ingress, even if Detach followed
    /// while a script callback waited. Callback acceptance orders these authored
    /// writes separately from immediate wire Detach. Recycled generations reject.
    pub(crate) fn pressure(&mut self, input: PerNoteInput, value: ControlValue) -> bool {
        let Some(slot) = self.slot_mut(input.controls) else { return false; };
        slot.expression.note_pressure = Some(value);
        true
    }

    pub(crate) fn note_controller(&mut self, input: PerNoteInput, value: NoteControl) -> bool {
        let Some(slot) = self.slot_mut(input.controls) else { return false; };
        match value {
            NoteControl::Registered { index: 3, value } => {
                slot.pitch(f64::from(value) / 33554432.0);
                true
            }
            NoteControl::Registered { index, value } => slot.controller(index, ControlValue::from_midi2(value)),
            NoteControl::Pitch(value) => { slot.bend = value; true },
            NoteControl::Assignable { .. } => false,
        }
    }

    pub(crate) fn supports_controller(value: NoteControl) -> bool {
        match value {
            NoteControl::Registered { index, .. } => index == 3 || Slot::supports_controller(index),
            NoteControl::Pitch(_) => true,
            NoteControl::Assignable { .. } => false,
        }
    }

    pub(crate) fn controllers(&self, input: Option<PerNoteInput>) -> Option<&[Option<ControlValue>; 128]> {
        input.and_then(|input| self.slot(input.controls)).map(|slot| &slot.controllers)
    }

    /// RP-015 resets polyphonic pressure on every note of the selected channel.
    /// Detach freezes registered controls; it does not exempt channel-wide pressure
    /// reset. UMP B.2 excludes registered controllers and per-note bend from CC121.
    pub(crate) fn reset_pressure(&mut self, channels: u16, port: Option<u8>) -> bool {
        let mut changed = false;
        for id in &self.active {
            let slot = &mut self.slots[id.index as usize];
            // Detached sounding generations keep their final per-note values.
            if slot.current && channels & (1u16 << slot.address.channel) != 0
                && port.is_none_or(|port| slot.address.port == port) {
                changed |= slot.expression.note_pressure.take().is_some();
            }
        }
        changed
    }

    /// Detach preserves the old generation's final values for all its tails.
    /// Reset then affects only the current/future generation (D precedes S).
    pub(crate) fn manage(&mut self, address: InputAddress, detach: bool, reset: bool) -> bool {
        if !address.valid() { return false; }
        if !detach && !reset { return true; }
        // An unseen lane already has default controls and no generation to detach.
        if !self.current.contains_key(&address) { return self.attach(address).is_some(); }
        let Some(id) = self.attach(address) else { return false; };
        let mut current = id;
        if detach {
            // Reserve first: exhaustion must not partly detach/reset a lane.
            let state = self.slots[id.index as usize];
            let Some(next) = self.allocate(address, Some(state)) else { return false; };
            self.slots[id.index as usize].current = false;
            self.current.insert(address, next);
            current = next;
        }
        if reset {
            let slot = &mut self.slots[current.index as usize];
            slot.expression = Expression::default();
            slot.controllers.fill(None);
            slot.bend = Bend::ZERO;
            slot.pitch = None;
            slot.pitch_revision = slot.pitch_revision.saturating_add(1);
        }
        true
    }

    pub(crate) fn overlay(&self, input: Option<PerNoteInput>, mut x: Expression) -> Expression {
        if let Some(input) = input
            && let Some(slot) = self.slot(input.controls) {
            let native = slot.expression;
            x.gain *= native.gain;
            // Attribute Pitch overrides prior persistent pitch only for this root.
            // Later controller-3 writes win; Reset preserves the root's attribute.
            let pitch = if slot.pitch_revision == input.pitch_revision {
                input.attribute_pitch.or(slot.pitch)
            } else { slot.pitch.or(input.attribute_pitch) };
            let tune = pitch.map_or(0.0, |pitch| pitch - f64::from(input.address.key))
                + slot.bend.normalized() * input.bend_range;
            x.tune += native.tune + tune as f32;
            x.pan += native.pan;
            x.note_cc74 = native.note_cc74.or(x.note_cc74);
            x.note_pressure = native.note_pressure.or(x.note_pressure);
        }
        x
    }

    pub(crate) fn needs_census(&self) -> bool { !self.slots.is_empty() && self.free.is_empty() }

    pub(crate) fn begin_mark(&mut self) {
        for id in &self.active { self.slots[id.index as usize].pinned = false; }
    }

    pub(crate) fn pin(&mut self, id: Option<PerNoteRef>) {
        if let Some(id) = id
            && let Some(slot) = self.slots.get_mut(id.index as usize)
            && slot.live && slot.generation == id.generation {
            slot.pinned = true;
        }
    }

    pub(crate) fn finish_mark(&mut self) {
        self.active.retain(|id| {
            let slot = &mut self.slots[id.index as usize];
            if slot.current || slot.pinned { true }
            else { slot.live = false; self.free.push(id.index); false }
        });
    }

    /// Protocol teardown; CC121 deliberately does not call this.
    pub(crate) fn reset(&mut self) {
        for id in &self.active { self.slots[id.index as usize].live = false; }
        self.active.clear();
        self.current.clear();
        self.free.clear();
        self.free.extend((0..self.slots.len() as u16).rev());
    }
}

#[cfg(test)]
#[path = "per_note_reset_tests.rs"]
mod reset_tests;


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_capture_range_and_attribute_while_controllers_share_until_detach() {
        let mut controls = InputControls::new();
        let address = InputAddress { port: 3, channel: 4, key: 60 };
        assert!(controls.bend(address, Bend::new(0.25).unwrap()));
        assert!(controls.pitch(address, 60.25));
        let first = controls.input(address, 12.0, None).unwrap();
        let attributed = controls.input(address, 48.0, Some(61.5)).unwrap();
        let tune = |controls: &InputControls, input| controls.overlay(Some(input), Expression::default()).tune;
        assert_eq!(tune(&controls, first), 3.25);
        assert_eq!(tune(&controls, attributed), 13.5);

        // Controller 3 received after On takes precedence over the On attribute.
        assert!(controls.pitch(address, 62.25));
        assert_eq!(tune(&controls, first), 5.25);
        assert_eq!(tune(&controls, attributed), 14.25);
        assert!(controls.manage(address, true, true));
        let next = controls.input(address, 24.0, None).unwrap();
        assert_ne!(next.controls, first.controls);
        assert_eq!(tune(&controls, first), 5.25);
        assert_eq!(tune(&controls, attributed), 14.25);
        assert_eq!(tune(&controls, next), 0.0);

        let next_attribute = controls.input(address, 24.0, Some(59.0)).unwrap();
        assert!(controls.manage(address, false, true));
        assert_eq!(tune(&controls, next_attribute), -1.0);
        assert!(controls.bend(address, Bend::new(-0.5).unwrap()));
        assert_eq!(tune(&controls, next_attribute), -13.0);
        assert_eq!(tune(&controls, first), 5.25);

        // A still-owned detached generation survives a census, then is reclaimed.
        controls.begin_mark();
        controls.pin(Some(first.controls));
        controls.finish_mark();
        assert!(controls.read(first.controls).is_some());
        controls.begin_mark();
        controls.finish_mark();
        assert!(controls.read(first.controls).is_none());
        assert_eq!(tune(&controls, first), 0.0);
        assert_eq!(tune(&controls, next_attribute), -13.0);
    }

    #[test]
    fn invalid_inputs_and_exhausted_detach_do_not_mutate_a_lane() {
        let mut controls = InputControls::new();
        let address = InputAddress { port: 0, channel: 0, key: 0 };
        let capacity = controls.free.len();
        assert!(!controls.update(address, HostExpression::Tune(f32::NAN)));
        assert!(!controls.pitch(address, f64::INFINITY));
        assert!(controls.input(address, -1.0, None).is_none());
        assert!(controls.input(address, 48.0, Some(128.0)).is_none());
        assert_eq!(controls.free.len(), capacity);

        for index in 0..capacity - 1 {
            let lane = InputAddress { port: (index / 2048) as u8,
                channel: ((index / 128) % 16) as u8, key: (index % 128) as u8 };
            assert!(controls.attach(lane).is_some());
        }
        let index = capacity - 1;
        let unseen = InputAddress { port: (index / 2048) as u8,
            channel: ((index / 128) % 16) as u8, key: (index % 128) as u8 };
        assert!(controls.manage(unseen, true, true));
        assert!(controls.free.is_empty());
        let root = controls.input(address, 48.0, None).unwrap();
        assert!(controls.pitch(address, 1.0));
        assert!(controls.bend(address, Bend::new(0.5).unwrap()));
        assert!(!controls.manage(address, true, true));
        assert_eq!(controls.input(address, 48.0, None).unwrap().controls, root.controls);
        assert_eq!(controls.overlay(Some(root), Expression::default()).tune, 25.0);
        assert_eq!(controls.dropped, 1);
    }
}
