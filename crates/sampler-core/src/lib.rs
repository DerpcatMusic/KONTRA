#![forbid(unsafe_code)]
//! Experimental native ownership kernel. No plugin, file, or language dependencies.
//!
//! Construction/destruction are control-thread operations. After preparation, methods
//! do not allocate or free. Prepared PCM is owned by the runtime. This slice
//! supports resident stereo PCM at unity rate, native linear envelopes and
//! sample-time commands; it does not claim resampling or vendor fidelity.

use std::sync::atomic::{AtomicU64, Ordering};

mod behavior;
use behavior::Continuation;
pub use behavior::{BehaviorId, Duration, Instruction, Outcome, Program, Velocity, WaitLifetime};
mod source;
pub use source::{Direction, Loop, LoopMode, Playback};
mod envelope;
pub use envelope::Envelope;
use envelope::EnvelopeState;
mod gate;
mod ownership;
mod pitch;
mod plans;
mod prepare;
mod render;
mod resample;
use plans::{Generation, PlanQueues};
pub use plans::{PlanControl, PlanError, PlanId, PlanTransfer, RejectedPlan};
pub use prepare::{Pcm, Prepared, Region};
mod schedule;
use gate::Channel;
pub use gate::{ChannelAddress, ChannelId};
pub use ownership::{Expression, ExpressionId, FamilyId, Inheritance};
use ownership::{ExpressionOwner, Family};
pub use schedule::Event;
use schedule::{Action, Scheduled};

pub type Frame = [f32; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Handle {
    runtime: u64,
    index: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteId(Handle);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceId(Handle);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Native,
    Midi1,
    Midi2,
    Clap,
    Vst3,
}

/// Original host address, never the transposed playback address. Adapters own
/// wildcard matching and protocol-specific interpretation of absent/signed IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub protocol: Protocol,
    pub port: u16,
    /// UMP group; zero for transports without group addressing.
    pub group: u8,
    pub channel: u8,
    pub key: u8,
    pub external_id: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity,
    InvalidInput,
    StaleHandle,
    DuplicateInput,
    ClosedNote,
    ClosedFamily,
    PastEvent,
    ClockOverflow,
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub notes: usize,
    pub channels: usize,
    pub families: usize,
    pub expressions: usize,
    pub voices: usize,
    pub commands: usize,
    pub behaviors: usize,
    pub behavior_fuel: usize,
    pub behavior_cells: usize,
}

#[derive(Clone, Copy)]
enum NoteOrigin {
    Input(Input, Expression),
    Child(NoteId, bool, Inheritance),
}

#[derive(Clone, Copy, Debug)]
struct Note {
    input: Option<Input>,
    address: ChannelAddress,
    plan: PlanId,
    parent: Option<NoteId>,
    linked_release: bool,
    release_checked: bool,
    key: u8,
    velocity: f64,
    gate: bool,
    key_down: bool,
    sostenuto: bool,
    work: usize,
    pins: usize,
    order: u64,
    expression: ExpressionId,
    families: usize,
    children: usize,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    family: FamilyId,
    sample: usize,
    cursor: source::Cursor,
    base_step: f64,
    envelope: EnvelopeState,
    gain: f32,
    started: bool,
}

#[derive(Clone, Copy, Debug)]
struct Slot<T> {
    generation: u64,
    value: Option<T>,
}

struct Arena<T> {
    runtime: u64,
    slots: Box<[Slot<T>]>,
}

impl<T> Arena<T> {
    fn new(runtime: u64, capacity: usize) -> Self {
        Self {
            runtime,
            slots: std::iter::repeat_with(|| Slot {
                generation: 0,
                value: None,
            })
            .take(capacity)
            .collect(),
        }
    }

    fn insert(&mut self, value: T) -> Result<Handle, Error> {
        // ponytail: bounded linear scan; add a free list if measured admission cost warrants it.
        let (index, slot) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.value.is_none() && s.generation < u64::MAX)
            .ok_or(Error::Capacity)?;
        slot.generation += 1; // Exhausted generations are quarantined, never wrapped.
        slot.value = Some(value);
        Ok(Handle {
            runtime: self.runtime,
            index,
            generation: slot.generation,
        })
    }

    fn get(&self, id: Handle) -> Option<&T> {
        self.slots
            .get(id.index)
            .filter(|s| id.runtime == self.runtime && s.generation == id.generation)?
            .value
            .as_ref()
    }

    fn get_mut(&mut self, id: Handle) -> Option<&mut T> {
        self.slots
            .get_mut(id.index)
            .filter(|s| id.runtime == self.runtime && s.generation == id.generation)?
            .value
            .as_mut()
    }

    fn remove(&mut self, id: Handle) {
        if self.get(id).is_some() {
            self.slots[id.index].value = None;
        }
    }

    fn id(&self, index: usize) -> Handle {
        Handle {
            runtime: self.runtime,
            index,
            generation: self.slots[index].generation,
        }
    }

    fn available(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.value.is_none() && s.generation < u64::MAX)
            .count()
    }

    fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.value.is_some()).count()
    }
}

/// All capacities are supplied at preparation. Terminal delivery uses the note's
/// existing slot, so a full command queue cannot discard its cleanup or notification.
pub struct Runtime {
    rate: u32,
    plans: Arena<Generation>,
    active_plan: PlanId,
    plan_queues: Option<PlanQueues>,
    notes: Arena<Note>,
    channels: Arena<Channel>,
    voices: Arena<Voice>,
    voice_activity: Box<[u64]>,
    kernel: &'static resample::Kernel,
    families: Arena<Family>,
    expressions: Arena<ExpressionOwner>,
    commands: Vec<Scheduled>,
    behaviors: Arena<Continuation>,
    behavior_fuel: usize,
    behavior_stride: usize,
    behavior_locals: Box<[i64]>,
    executing_due: bool,
    command_limit: usize,
    now: u64,
    order: u64,
    nonfinite_frames: u64,
}

impl Runtime {
    pub fn new(plan: Prepared, limits: Limits) -> Result<Self, Error> {
        if limits.notes == 0 {
            return Err(Error::InvalidInput);
        }
        let behavior_stride = limits
            .behavior_cells
            .checked_div(limits.behaviors)
            .unwrap_or(0);
        if plan.programs.iter().any(|p| p.locals > behavior_stride) {
            return Err(Error::Capacity);
        }
        let cells = behavior_stride
            .checked_mul(limits.behaviors)
            .ok_or(Error::Capacity)?;
        if cells > limits.behavior_cells {
            return Err(Error::Capacity);
        }
        std::alloc::Layout::array::<i64>(cells).map_err(|_| Error::Capacity)?;
        static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated, reason = "fetch_update supports the Rust 1.92 minimum")]
        let id = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        let rate = plan.rate;
        let mut plans = Arena::new(id, 1);
        let active_plan = PlanId(plans.insert(Generation {
            request: 0,
            prepared: Box::new(plan),
            notes: 0,
        })?);
        Ok(Self {
            rate,
            plans,
            active_plan,
            plan_queues: None,
            notes: Arena::new(id, limits.notes),
            channels: Arena::new(id, limits.channels),
            voices: Arena::new(id, limits.voices),
            voice_activity: vec![0; limits.voices.div_ceil(64)].into_boxed_slice(),
            kernel: resample::Kernel::shared(),
            families: Arena::new(id, limits.families),
            expressions: Arena::new(id, limits.expressions),
            commands: Vec::with_capacity(limits.commands),
            behaviors: Arena::new(id, limits.behaviors),
            behavior_fuel: limits.behavior_fuel,
            behavior_stride,
            behavior_locals: vec![0; cells].into_boxed_slice(),
            executing_due: false,
            command_limit: limits.commands,
            now: 0,
            order: 0,
            nonfinite_frames: 0,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
    pub fn now(&self) -> u64 {
        self.now
    }
    pub fn note_count(&self) -> usize {
        self.notes.count()
    }
    pub fn voice_count(&self) -> usize {
        self.voices.count()
    }
    pub fn pending_commands(&self) -> usize {
        self.commands.len()
    }

    /// Frames silenced because finite source values overflowed during summation.
    pub fn nonfinite_frames(&self) -> u64 {
        self.nonfinite_frames
    }

    /// Admit ownership before preparing any source or scheduled work. A caller
    /// receiving Err has not admitted an input; protocol rejection remains its job.
    pub fn note_on(&mut self, input: Input, key: u8, velocity: f64) -> Result<NoteId, Error> {
        self.note_on_with_expression(input, key, velocity, Expression::default())
    }

    /// Initial expression is part of ownership admission, before any callback or
    /// source can snapshot it. Unsupported PCM rates are checked at source admission.
    pub fn note_on_with_expression(
        &mut self,
        input: Input,
        key: u8,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        if input.channel >= 16 || input.group >= 16 || input.key >= 128 || !expression.valid() {
            return Err(Error::InvalidInput);
        }
        if input.external_id.is_some()
            && self
                .notes
                .slots
                .iter()
                .any(|s| s.value.is_some_and(|n| n.input == Some(input)))
        {
            return Err(Error::DuplicateInput);
        }
        self.admit(NoteOrigin::Input(input, expression), key, velocity)
    }

    /// Detached children do not release with their parent, but still retain its
    /// provenance until they finish. Linked children cannot attach to a closed gate.
    pub fn child(
        &mut self,
        parent: NoteId,
        key: u8,
        velocity: f64,
        linked_release: bool,
        inheritance: Inheritance,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        let p = self.notes.get(parent.0).ok_or(Error::StaleHandle)?;
        if linked_release && !p.gate {
            return Err(Error::ClosedNote);
        }
        self.admit(
            NoteOrigin::Child(parent, linked_release, inheritance),
            key,
            velocity,
        )
    }

    fn admit(&mut self, origin: NoteOrigin, key: u8, velocity: f64) -> Result<NoteId, Error> {
        let (input, parent, linked_release, inheritance, initial) = match origin {
            NoteOrigin::Input(input, expression) => (
                Some(input),
                None,
                false,
                Inheritance::Independent,
                expression,
            ),
            NoteOrigin::Child(parent, linked, inheritance) => (
                None,
                Some(parent),
                linked,
                inheritance,
                Expression::default(),
            ),
        };
        if key >= 128 || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let order = self.order.checked_add(1).ok_or(Error::ClockOverflow)?;
        let parent_note = parent
            .map(|p| self.notes.get(p.0).ok_or(Error::StaleHandle))
            .transpose()?;
        let address = input
            .map(Input::channel_address)
            .or_else(|| parent_note.map(|n| n.address))
            .ok_or(Error::InvalidInput)?;
        let parent_expression = parent_note.map(|n| n.expression);
        let plan = parent_note.map_or(self.active_plan, |n| n.plan);
        let expression = match (inheritance, parent_expression) {
            (Inheritance::Linked, Some(id)) => {
                let owner = self.expressions.get_mut(id.0).unwrap();
                owner.notes = owner.notes.checked_add(1).ok_or(Error::Capacity)?;
                id
            }
            (policy, parent) => {
                let (value, pitch_ratio) = if policy == Inheritance::Snapshot {
                    parent
                        .map(|p| {
                            let owner = self.expressions.get(p.0).unwrap();
                            (owner.value, owner.pitch_ratio)
                        })
                        .unwrap_or((Expression::default(), 1.0))
                } else {
                    (initial, pitch::ratio(initial.pitch_semitones))
                };
                ExpressionId(self.expressions.insert(ExpressionOwner {
                    value,
                    pitch_ratio,
                    notes: 1,
                })?)
            }
        };
        let id = match self.notes.insert(Note {
            input,
            address,
            plan,
            parent,
            linked_release,
            release_checked: false,
            key,
            velocity,
            gate: true,
            key_down: true,
            sostenuto: false,
            work: 0,
            pins: 0,
            order,
            expression,
            families: 0,
            children: 0,
        }) {
            Ok(id) => id,
            Err(error) => {
                self.drop_expression(expression);
                return Err(error);
            }
        };
        if let Some(parent) = parent {
            // Successful admission bounds this count by the allocated note slots.
            self.notes.get_mut(parent.0).unwrap().children += 1;
        }
        self.plans.get_mut(plan.0).unwrap().notes += 1;
        self.order = order;
        Ok(NoteId(id))
    }

    pub fn note(&self, id: NoteId) -> Result<(u8, f64, bool), Error> {
        let n = self.notes.get(id.0).ok_or(Error::StaleHandle)?;
        Ok((n.key, n.velocity, n.gate))
    }

    /// A continuation pins logical ownership even after source completion/release.
    pub fn pin(&mut self, id: NoteId) -> Result<(), Error> {
        let n = self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?;
        n.pins = n.pins.checked_add(1).ok_or(Error::Capacity)?;
        Ok(())
    }

    pub fn unpin(&mut self, id: NoteId) -> Result<(), Error> {
        let n = self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?;
        n.pins = n.pins.checked_sub(1).ok_or(Error::InvalidInput)?;
        Ok(())
    }

    /// Native anonymous-input fallback: FIFO within the original protocol/port/key.
    pub fn note_off(&mut self, input: Input) -> Result<NoteId, Error> {
        self.apply_due();
        let id = self
            .notes
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.value
                    .filter(|n| n.key_down && n.input == Some(input))
                    .map(|n| (i, n.order))
            })
            .min_by_key(|(_, order)| *order)
            .map(|(i, _)| NoteId(self.notes.id(i)))
            .ok_or(Error::StaleHandle)?;
        self.key_up_now(id)?;
        Ok(id)
    }

    /// Convenience for a single-source selection: creates and seals one family.
    /// Multi-source selections explicitly create a family and use start_family.
    pub fn start(
        &mut self,
        note: NoteId,
        sample: usize,
        at: u64,
        gain: f32,
    ) -> Result<VoiceId, Error> {
        let family = self.create_family(note)?;
        let result = self.start_family(
            family,
            sample,
            at,
            gain,
            Envelope::default(),
            Playback::default(),
        );
        self.finish_family(family)?;
        result
    }

    /// Reserve a voice and its delayed start atomically. A failed admission leaves
    /// the family unchanged. Sealed families cannot admit additional sources.
    pub fn start_family(
        &mut self,
        family: FamilyId,
        sample: usize,
        at: u64,
        gain: f32,
        envelope: Envelope,
        playback: Playback,
    ) -> Result<VoiceId, Error> {
        self.check_time(at)?;
        if at == self.now {
            self.apply_due();
        }
        let f = self.families.get(family.0).ok_or(Error::StaleHandle)?;
        if !f.open {
            return Err(Error::ClosedFamily);
        }
        let plan = &self
            .plans
            .get(self.notes.get(f.note.0).unwrap().plan.0)
            .unwrap()
            .prepared;
        if sample >= plan.pcm.len() || !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
            return Err(Error::InvalidInput);
        }
        let cursor = playback.cursor(
            plan.pcm[sample].frames.len(),
            plan.pcm[sample].rate,
            self.rate,
        )?;
        self.admit_voice(family, sample, at, gain, envelope, cursor)
    }

    // Inputs and cursor are validated either by Prepared or start_family. Both
    // paths drain due work before reaching this sole voice admission boundary.
    fn admit_voice(
        &mut self,
        family: FamilyId,
        sample: usize,
        at: u64,
        gain: f32,
        envelope: Envelope,
        cursor: source::Cursor,
    ) -> Result<VoiceId, Error> {
        let f = self.families.get(family.0).ok_or(Error::StaleHandle)?;
        if !f.open {
            return Err(Error::ClosedFamily);
        }
        let owner = self.notes.get(f.note.0).unwrap().expression;
        let base_step = cursor.step();
        let step = self.pitch_range(owner, true)?.apply(base_step)?;
        let cursor = cursor.with_step(step);
        let count = f.voices.checked_add(1).ok_or(Error::Capacity)?;
        if at > self.now && self.commands.len() == self.command_limit {
            return Err(Error::Capacity);
        }
        let id = VoiceId(self.voices.insert(Voice {
            family,
            sample,
            cursor,
            base_step,
            envelope: EnvelopeState::new(envelope),
            gain,
            started: at == self.now,
        })?);
        self.voice_activity[id.0.index / 64] |= 1 << (id.0.index % 64);
        self.families.get_mut(family.0).unwrap().voices = count;
        if at > self.now {
            self.queue(at, Action::Start(id));
        }
        Ok(id)
    }

    pub fn stop_voice(&mut self, id: VoiceId) -> Result<(), Error> {
        self.apply_due();
        self.voices.get(id.0).ok_or(Error::StaleHandle)?;
        self.commands
            .retain(|c| !matches!(c.action, Action::Start(v) if v == id));
        self.end_voice(id);
        Ok(())
    }

    pub fn voice_active(&self, id: VoiceId) -> bool {
        self.voices.get(id.0).is_some()
    }

    pub fn release(&mut self, id: NoteId) -> Result<(), Error> {
        self.apply_due();
        self.release_now(id)
    }

    fn release_now(&mut self, id: NoteId) -> Result<(), Error> {
        let n = self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?;
        n.gate = false;
        n.key_down = false;
        n.sostenuto = false;
        self.propagate_release();
        self.cleanup_closed_notes();
        Ok(())
    }

    fn propagate_release(&mut self) {
        for slot in &mut self.notes.slots {
            if let Some(note) = &mut slot.value {
                note.release_checked = false;
            }
        }
        for i in 0..self.notes.slots.len() {
            if self.notes.slots[i].value.is_none() {
                continue;
            }
            let start = NoteId(self.notes.id(i));
            let mut id = start;
            // Mark each open linked path once. A previously checked open path
            // reaches an open root or an independent child, so it cannot close.
            let closes = loop {
                let n = self.notes.get_mut(id.0).unwrap();
                if !n.gate {
                    break true;
                }
                if n.release_checked || !n.linked_release {
                    break false;
                }
                n.release_checked = true;
                let Some(parent) = n.parent else {
                    break false;
                };
                id = parent;
            };
            if closes {
                id = start;
                loop {
                    let n = self.notes.get_mut(id.0).unwrap();
                    if !n.gate {
                        break;
                    }
                    n.gate = false;
                    n.key_down = false;
                    n.sostenuto = false;
                    // The discovery walk proved a closed ancestor on this path.
                    id = n.parent.unwrap();
                }
            }
        }
    }

    /// Resolve voices and future commands. Behavior outcomes remain retained until
    /// accepted; external owners must release manual pins. IDs stay valid for NOTE_END.
    pub fn panic(&mut self) {
        for s in &mut self.notes.slots {
            if let Some(n) = &mut s.value {
                n.gate = false;
                n.key_down = false;
                n.sostenuto = false;
            }
        }
        for slot in &mut self.behaviors.slots {
            if let Some(c) = &mut slot.value
                && c.outcome.is_none()
            {
                c.outcome = Some(Outcome::Cancelled);
            }
        }
        // Panic hard-stops tails as well as held voices.
        for i in 0..self.voices.slots.len() {
            if self.voices.slots[i].value.is_some() {
                self.end_voice(VoiceId(self.voices.id(i)));
            }
        }
        self.cleanup_closed_notes();
        self.commands.clear();
        for s in &mut self.channels.slots {
            if let Some(c) = &mut s.value {
                c.sustain = false;
                c.sostenuto = false;
            }
        }
    }

    fn cleanup_closed_notes(&mut self) {
        // One pass per resource domain, including saturated family/voice pools.
        for s in &mut self.families.slots {
            if let Some(f) = &mut s.value
                && !self.notes.get(f.note.0).unwrap().gate
            {
                f.open = false;
            }
        }
        for i in 0..self.voices.slots.len() {
            if let Some(v) = self.voices.slots[i].value.as_mut() {
                let family = self.families.get(v.family.0).unwrap();
                if !self.notes.get(family.note.0).unwrap().gate {
                    v.envelope.release();
                    v.cursor.release();
                    if !v.started || v.envelope.done() {
                        self.end_voice(VoiceId(self.voices.id(i)));
                    }
                }
            }
        }
        for i in 0..self.families.slots.len() {
            self.retire_family(FamilyId(self.families.id(i)));
        }
        self.cancel_closed_work();
    }
}

#[cfg(test)]
mod tests;
