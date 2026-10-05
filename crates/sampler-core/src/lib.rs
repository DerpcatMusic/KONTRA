#![forbid(unsafe_code)]
//! Experimental native ownership kernel. No plugin, file, or language dependencies.
//!
//! Construction/destruction are control-thread operations. After preparation, methods
//! do not allocate or free. Prepared PCM is owned by the runtime. This slice
//! supports resident stereo PCM at unity rate, native linear envelopes and
//! sample-time commands; it does not claim resampling or vendor fidelity.

use std::sync::atomic::{AtomicU64, Ordering};

mod envelope;
pub use envelope::Envelope;
use envelope::EnvelopeState;
mod gate;
mod ownership;
mod prepare;
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
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub notes: usize,
    pub channels: usize,
    pub families: usize,
    pub expressions: usize,
    pub voices: usize,
    pub commands: usize,
}

#[derive(Clone, Copy, Debug)]
struct Note {
    input: Option<Input>,
    parent: Option<NoteId>,
    linked_release: bool,
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
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    family: FamilyId,
    sample: usize,
    cursor: usize,
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

impl<T: Copy> Arena<T> {
    fn new(runtime: u64, capacity: usize) -> Self {
        Self {
            runtime,
            slots: vec![
                Slot {
                    generation: 0,
                    value: None
                };
                capacity
            ]
            .into_boxed_slice(),
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
    plan: Prepared,
    notes: Arena<Note>,
    channels: Arena<Channel>,
    voices: Arena<Voice>,
    families: Arena<Family>,
    expressions: Arena<ExpressionOwner>,
    commands: Vec<Scheduled>,
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
        static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated, reason = "fetch_update supports the Rust 1.92 minimum")]
        let id = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        Ok(Self {
            rate: plan.rate,
            plan,
            notes: Arena::new(id, limits.notes),
            channels: Arena::new(id, limits.channels),
            voices: Arena::new(id, limits.voices),
            families: Arena::new(id, limits.families),
            expressions: Arena::new(id, limits.expressions),
            commands: Vec::with_capacity(limits.commands),
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
        self.apply_due();
        if input.channel >= 16 || input.group >= 16 || input.key >= 128 {
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
        self.admit(
            Some(input),
            None,
            false,
            key,
            velocity,
            Inheritance::Independent,
        )
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
            None,
            Some(parent),
            linked_release,
            key,
            velocity,
            inheritance,
        )
    }

    fn admit(
        &mut self,
        input: Option<Input>,
        parent: Option<NoteId>,
        linked_release: bool,
        key: u8,
        velocity: f64,
        inheritance: Inheritance,
    ) -> Result<NoteId, Error> {
        if key >= 128 || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let order = self.order.checked_add(1).ok_or(Error::ClockOverflow)?;
        let parent_expression = parent.map(|p| self.notes.get(p.0).unwrap().expression);
        let expression = match (inheritance, parent_expression) {
            (Inheritance::Linked, Some(id)) => {
                let owner = self.expressions.get_mut(id.0).unwrap();
                owner.notes = owner.notes.checked_add(1).ok_or(Error::Capacity)?;
                id
            }
            (policy, parent) => {
                let value = if policy == Inheritance::Snapshot {
                    parent
                        .map(|p| self.expressions.get(p.0).unwrap().value)
                        .unwrap_or_default()
                } else {
                    Expression::default()
                };
                ExpressionId(
                    self.expressions
                        .insert(ExpressionOwner { value, notes: 1 })?,
                )
            }
        };
        let id = match self.notes.insert(Note {
            input,
            parent,
            linked_release,
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
        }) {
            Ok(id) => id,
            Err(error) => {
                self.drop_expression(expression);
                return Err(error);
            }
        };
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
        let result = self.start_family(family, sample, at, gain, Envelope::default());
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
    ) -> Result<VoiceId, Error> {
        self.check_time(at)?;
        if at == self.now {
            self.apply_due();
        }
        let f = self.families.get(family.0).ok_or(Error::StaleHandle)?;
        if !f.open {
            return Err(Error::ClosedFamily);
        }
        if sample >= self.plan.pcm.len() || !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
            return Err(Error::InvalidInput);
        }
        let count = f.voices.checked_add(1).ok_or(Error::Capacity)?;
        self.check_time(at)?;
        if at > self.now && self.commands.len() == self.command_limit {
            return Err(Error::Capacity);
        }
        let id = VoiceId(self.voices.insert(Voice {
            family,
            sample,
            cursor: 0,
            envelope: EnvelopeState::new(envelope),
            gain,
            started: at == self.now,
        })?);
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
        // ponytail: bounded O(notes²) tree propagation; maintain child adjacency if profiling requires it.
        for _ in 0..self.notes.slots.len() {
            let mut changed = false;
            for i in 0..self.notes.slots.len() {
                if let Some(n) = self.notes.slots[i].value
                    && n.gate
                    && n.linked_release
                    && n.parent
                        .is_some_and(|p| self.notes.get(p.0).is_some_and(|p| !p.gate))
                {
                    let n = self.notes.slots[i].value.as_mut().unwrap();
                    n.gate = false;
                    n.key_down = false;
                    n.sostenuto = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Resolve all voices and future commands. Continuation owners must unpin after
    /// canceling their own work; reset does not invalidate IDs needed for NOTE_END.
    pub fn panic(&mut self) {
        for s in &mut self.notes.slots {
            if let Some(n) = &mut s.value {
                n.gate = false;
                n.key_down = false;
                n.sostenuto = false;
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

    /// Consume terminal notifications only after acceptance. The sink must be bounded
    /// and non-allocating on an audio thread. A rejection stops retries for this call.
    pub fn flush_ended(&mut self, mut accept: impl FnMut(Input) -> bool) {
        for _ in 0..self.notes.slots.len() {
            let mut removed = false;
            for i in 0..self.notes.slots.len() {
                let Some(n) = self.notes.slots[i].value else {
                    continue;
                };
                let id = NoteId(self.notes.id(i));
                if n.gate
                    || n.pins != 0
                    || n.work != 0
                    || n.families != 0
                    || self
                        .notes
                        .slots
                        .iter()
                        .any(|s| s.value.is_some_and(|n| n.parent == Some(id)))
                {
                    continue;
                }
                if n.input.is_some_and(|input| !accept(input)) {
                    return;
                }
                self.notes.remove(id.0);
                self.drop_expression(n.expression);
                removed = true;
            }
            if !removed {
                break;
            }
        }
    }

    /// Events at the exclusive block end stay pending until the next render (including
    /// an empty block). Overflow is rejected before any output/state mutation.
    pub fn render(&mut self, output: &mut [Frame]) -> Result<(), Error> {
        let end = self
            .now
            .checked_add(output.len() as u64)
            .ok_or(Error::ClockOverflow)?;
        output.fill([0.0; 2]);
        self.apply_due();
        let mut offset = 0;
        while self.now < end {
            self.apply_due();
            let boundary = self.commands.first().map_or(end, |c| c.at.min(end));
            let len = (boundary - self.now) as usize;
            let segment = &mut output[offset..offset + len];
            // Voice-major contiguous work: scan reserved capacity once per event
            // segment, not once per sample. Slot order preserves deterministic sums.
            for i in 0..self.voices.slots.len() {
                let Some(v) = &mut self.voices.slots[i].value else {
                    continue;
                };
                if !v.started {
                    continue;
                }
                // Retention invariant: live voice -> counted family -> counted note
                // -> expression owner. Each owner retires only after its dependents.
                let f = self.families.get(v.family.0).unwrap();
                let n = self.notes.get(f.note.0).unwrap();
                let gains = self.expressions.get(n.expression.0).unwrap().value.gains();
                // Admission validates the immutable sample index. Only this loop
                // advances cursor, clamped to the remaining source length.
                let pcm = &self.plan.pcm[v.sample].frames;
                let count = len.min(pcm.len() - v.cursor).min(v.envelope.remaining());
                let unity = v.envelope.unity();
                for (frame, input) in segment[..count]
                    .iter_mut()
                    .zip(&pcm[v.cursor..v.cursor + count])
                {
                    let level = if unity { 1.0 } else { v.envelope.next() };
                    for c in 0..2 {
                        frame[c] += input[c] * v.gain * gains[c] * level;
                    }
                }
                v.cursor += count;
                if v.cursor == pcm.len() || v.envelope.done() {
                    self.end_voice(VoiceId(self.voices.id(i)));
                }
            }
            for frame in segment {
                if !frame.iter().all(|x| x.is_finite()) {
                    *frame = [0.0; 2];
                    self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
                }
            }
            self.now = boundary;
            offset += len;
        }
        debug_assert_eq!(self.now, end);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
