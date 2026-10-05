//! Experimental native ownership kernel. No plugin, file, or language dependencies.
//!
//! Construction/destruction are control-thread operations. After preparation, methods
//! do not allocate or free. PCM is borrowed and must outlive the runtime. This first
//! slice supports resident stereo PCM at unity rate, immediate gate release, and
//! sample-time commands; it does not claim resampling, envelopes or vendor fidelity.

use std::sync::atomic::{AtomicU64, Ordering};

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
    Clap,
    Vst3,
}

/// Original host address, never the transposed playback address. Adapters own
/// wildcard matching and protocol-specific interpretation of absent/signed IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub protocol: Protocol,
    pub port: u16,
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
    PastEvent,
    ClockOverflow,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub notes: usize,
    pub voices: usize,
    pub commands: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Pcm<'a> {
    pub rate: u32,
    pub frames: &'a [Frame],
}

#[derive(Clone, Copy, Debug)]
struct Note {
    input: Option<Input>,
    parent: Option<NoteId>,
    linked_release: bool,
    key: u8,
    velocity: f64,
    gate: bool,
    pins: usize,
    order: u64,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    note: NoteId,
    sample: usize,
    cursor: usize,
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

    fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.value.is_some()).count()
    }
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Start(VoiceId),
    Release(NoteId),
}

#[derive(Clone, Copy, Debug)]
struct Scheduled {
    at: u64,
    action: Action,
}

/// All capacities are supplied at preparation. Terminal delivery uses the note's
/// existing slot, so a full command queue cannot discard its cleanup or notification.
pub struct Runtime<'a> {
    rate: u32,
    pcm: &'a [Pcm<'a>],
    notes: Arena<Note>,
    voices: Arena<Voice>,
    commands: Vec<Scheduled>,
    command_limit: usize,
    now: u64,
    order: u64,
    nonfinite_frames: u64,
}

impl<'a> Runtime<'a> {
    pub fn new(rate: u32, pcm: &'a [Pcm<'a>], limits: Limits) -> Result<Self, Error> {
        if rate == 0
            || limits.notes == 0
            || pcm.iter().any(|p| {
                p.rate != rate
                    || p.frames.is_empty()
                    || p.frames.iter().flatten().any(|x| !x.is_finite())
            })
        {
            return Err(Error::InvalidInput);
        }
        static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated, reason = "fetch_update supports the Rust 1.92 minimum")]
        let id = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        Ok(Self {
            rate,
            pcm,
            notes: Arena::new(id, limits.notes),
            voices: Arena::new(id, limits.voices),
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
        if input.channel >= 16 || input.key >= 128 {
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
        self.admit(Some(input), None, false, key, velocity)
    }

    /// Detached children do not release with their parent, but still retain its
    /// provenance until they finish. Linked children cannot attach to a closed gate.
    pub fn child(
        &mut self,
        parent: NoteId,
        key: u8,
        velocity: f64,
        linked_release: bool,
    ) -> Result<NoteId, Error> {
        let p = self.notes.get(parent.0).ok_or(Error::StaleHandle)?;
        if linked_release && !p.gate {
            return Err(Error::ClosedNote);
        }
        self.admit(None, Some(parent), linked_release, key, velocity)
    }

    fn admit(
        &mut self,
        input: Option<Input>,
        parent: Option<NoteId>,
        linked_release: bool,
        key: u8,
        velocity: f64,
    ) -> Result<NoteId, Error> {
        if key >= 128 || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let order = self.order.checked_add(1).ok_or(Error::ClockOverflow)?;
        let id = self.notes.insert(Note {
            input,
            parent,
            linked_release,
            key,
            velocity,
            gate: true,
            pins: 0,
            order,
        })?;
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
        let id = self
            .notes
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.value
                    .filter(|n| n.gate && n.input == Some(input))
                    .map(|n| (i, n.order))
            })
            .min_by_key(|(_, order)| *order)
            .map(|(i, _)| NoteId(self.notes.id(i)))
            .ok_or(Error::StaleHandle)?;
        self.release(id)?;
        Ok(id)
    }

    /// Queue capacity and a voice slot are reserved together. A failed start leaves
    /// no partial voice. Delayed starts are canceled by release, never resurrected.
    pub fn start(
        &mut self,
        note: NoteId,
        sample: usize,
        at: u64,
        gain: f32,
    ) -> Result<VoiceId, Error> {
        if !self.notes.get(note.0).ok_or(Error::StaleHandle)?.gate {
            return Err(Error::ClosedNote);
        }
        if sample >= self.pcm.len() || !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
            return Err(Error::InvalidInput);
        }
        self.check_time(at)?;
        if at > self.now && self.commands.len() == self.command_limit {
            return Err(Error::Capacity);
        }
        let id = VoiceId(self.voices.insert(Voice {
            note,
            sample,
            cursor: 0,
            gain,
            started: at == self.now,
        })?);
        if at > self.now {
            self.queue(at, Action::Start(id));
        }
        Ok(id)
    }

    pub fn release_at(&mut self, note: NoteId, at: u64) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        self.check_time(at)?;
        if at == self.now {
            return self.release(note);
        }
        if self.commands.len() == self.command_limit {
            return Err(Error::Capacity);
        }
        self.queue(at, Action::Release(note));
        Ok(())
    }

    fn check_time(&self, at: u64) -> Result<(), Error> {
        if at < self.now {
            Err(Error::PastEvent)
        } else {
            Ok(())
        }
    }

    fn queue(&mut self, at: u64, action: Action) {
        // ponytail: sorted bounded Vec; use a heap if measured command traffic needs it.
        let index = self.commands.partition_point(|c| c.at <= at);
        self.commands.insert(index, Scheduled { at, action });
    }

    pub fn stop_voice(&mut self, id: VoiceId) -> Result<(), Error> {
        self.voices.get(id.0).ok_or(Error::StaleHandle)?;
        self.commands
            .retain(|c| !matches!(c.action, Action::Start(v) if v == id));
        self.voices.remove(id.0);
        Ok(())
    }

    pub fn voice_active(&self, id: VoiceId) -> bool {
        self.voices.get(id.0).is_some()
    }

    pub fn release(&mut self, id: NoteId) -> Result<(), Error> {
        self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?.gate = false;
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
                    self.notes.slots[i].value.as_mut().unwrap().gate = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for slot in &mut self.voices.slots {
            if slot
                .value
                .is_some_and(|v| self.notes.get(v.note.0).is_none_or(|n| !n.gate))
            {
                slot.value = None;
            }
        }
        self.commands.retain(|c| match c.action {
            Action::Start(v) => self.voices.get(v.0).is_some(),
            Action::Release(n) => self.notes.get(n.0).is_some_and(|n| n.gate),
        });
        Ok(())
    }

    /// Resolve all voices and future commands. Continuation owners must unpin after
    /// canceling their own work; reset does not invalidate IDs needed for NOTE_END.
    pub fn panic(&mut self) {
        self.commands.clear();
        for s in &mut self.voices.slots {
            s.value = None;
        }
        for s in &mut self.notes.slots {
            if let Some(n) = &mut s.value {
                n.gate = false;
            }
        }
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
                    || self
                        .notes
                        .slots
                        .iter()
                        .any(|s| s.value.is_some_and(|n| n.parent == Some(id)))
                    || self
                        .voices
                        .slots
                        .iter()
                        .any(|s| s.value.is_some_and(|v| v.note == id))
                {
                    continue;
                }
                if n.input.is_some_and(|input| !accept(input)) {
                    return;
                }
                self.notes.remove(id.0);
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
        self.apply_due();
        for frame in output {
            self.apply_due();
            *frame = [0.0; 2];
            for slot in &mut self.voices.slots {
                let Some(v) = &mut slot.value else { continue };
                if !v.started {
                    continue;
                }
                let input = self.pcm[v.sample].frames[v.cursor];
                for c in 0..2 {
                    frame[c] += input[c] * v.gain;
                }
                v.cursor += 1;
                if v.cursor == self.pcm[v.sample].frames.len() {
                    slot.value = None;
                }
            }
            // Finite source data can still overflow during summation.
            if !frame.iter().all(|x| x.is_finite()) {
                *frame = [0.0; 2];
                self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
            }
            self.now += 1;
        }
        debug_assert_eq!(self.now, end);
        Ok(())
    }

    fn apply_due(&mut self) {
        // The queue is finite and actions never generate new commands.
        while self.commands.first().is_some_and(|c| c.at <= self.now) {
            match self.commands.remove(0).action {
                Action::Start(id) => {
                    if let Some(v) = self.voices.get_mut(id.0) {
                        v.started = true;
                    }
                }
                Action::Release(id) => {
                    let _ = self.release(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
