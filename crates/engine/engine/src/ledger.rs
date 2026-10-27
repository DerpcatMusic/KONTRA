//! The note ledger: one table of sounding notes, addressed by generational handles.
//! A note owns its voices; every operation on voices goes through their note.

use kontra_events::{HostNote, HostPattern};
use kontra_time::SampleTime;

use crate::behaviour::RrDraw;

/// A note handle. Stale handles (the note ended) match nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteRef {
    index: u32,
    generation: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Note {
    pub(crate) host: HostNote,
    /// 0..=1.
    pub(crate) velocity: f64,
    /// MIDI velocity 1..=127, what zones and crossfades select on.
    pub(crate) velocity_byte: u8,
    pub(crate) started: SampleTime,
    /// The round-robin draw of the attack; release triggers reuse it.
    pub(crate) draw: RrDraw,
    /// Initial tuning from the host, semitones.
    pub(crate) tune: f64,
    pub(crate) expression_tune: f64,
    pub(crate) expression_gain: f32,
    pub(crate) expression_pan: f32,
    /// Key released while the sustain pedal was down.
    pub(crate) pedal_held: bool,
    /// Key released (directly or by the pedal coming up).
    pub(crate) released: bool,
}

impl Note {
    pub(crate) fn new(
        host: HostNote,
        velocity: f64,
        tune: f64,
        started: SampleTime,
        draw: RrDraw,
    ) -> Self {
        let velocity = velocity.clamp(0.0, 1.0);
        Self {
            host,
            velocity,
            velocity_byte: (velocity * 127.0).round().max(1.0) as u8,
            started,
            draw,
            tune,
            expression_tune: 0.0,
            expression_gain: 1.0,
            expression_pan: 0.0,
            pedal_held: false,
            released: false,
        }
    }
}

struct Entry {
    generation: u32,
    note: Option<Note>,
}

/// Fixed capacity. Queries fill a caller's list up to its capacity and never allocate.
pub(crate) struct Ledger {
    entries: Vec<Entry>,
}

impl Ledger {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: (0..capacity)
                .map(|_| Entry {
                    generation: 0,
                    note: None,
                })
                .collect(),
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn insert(&mut self, note: Note) -> Option<NoteRef> {
        let (index, entry) = self
            .entries
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.note.is_none())?;
        entry.generation = entry.generation.wrapping_add(1);
        entry.note = Some(note);
        Some(NoteRef {
            index: index as u32,
            generation: entry.generation,
        })
    }

    pub(crate) fn get(&self, r: NoteRef) -> Option<&Note> {
        self.entries
            .get(r.index as usize)
            .filter(|e| e.generation == r.generation)?
            .note
            .as_ref()
    }

    pub(crate) fn get_mut(&mut self, r: NoteRef) -> Option<&mut Note> {
        self.entries
            .get_mut(r.index as usize)
            .filter(|e| e.generation == r.generation)?
            .note
            .as_mut()
    }

    pub(crate) fn remove(&mut self, r: NoteRef) -> Option<Note> {
        self.entries
            .get_mut(r.index as usize)
            .filter(|e| e.generation == r.generation)?
            .note
            .take()
    }

    /// Fills `out` with the live notes `keep` accepts, oldest slot first.
    pub(crate) fn select(&self, out: &mut Vec<NoteRef>, keep: impl Fn(&Note) -> bool) {
        out.clear();
        for (index, e) in self.entries.iter().enumerate() {
            if out.len() == out.capacity() {
                break;
            }
            if e.note.as_ref().is_some_and(&keep) {
                out.push(NoteRef {
                    index: index as u32,
                    generation: e.generation,
                });
            }
        }
    }

    pub(crate) fn count(&self, keep: impl Fn(&Note) -> bool) -> usize {
        self.entries
            .iter()
            .filter(|e| e.note.as_ref().is_some_and(&keep))
            .count()
    }

    pub(crate) fn matching(&self, pattern: HostPattern, out: &mut Vec<NoteRef>) {
        self.select(out, |n| pattern.matches(n.host));
    }
}
