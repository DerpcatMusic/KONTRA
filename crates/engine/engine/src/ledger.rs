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

impl NoteRef {
    /// The note's slot in the ledger, for tables kept beside it.
    pub(crate) fn slot(self) -> usize {
        self.index as usize
    }

    /// The script-visible event id: positive, unique among live notes, and
    /// stale once the note ends. Ledgers hold at most 65536 notes.
    pub(crate) fn event_id(self) -> i32 {
        (((self.generation & 0x7FFF) << 16) | (self.index & 0xFFFF)) as i32
    }

    pub(crate) fn from_event_id(id: i32) -> Option<Self> {
        let id = u32::try_from(id).ok().filter(|&id| id >> 16 != 0)?;
        Some(Self {
            index: id & 0xFFFF,
            generation: id >> 16,
        })
    }
}

/// Where a note came from and how it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A host note: released by its note-off.
    Host,
    /// A script note released with its parent (`play_note` duration -1).
    Child(NoteRef),
    /// A script note released only by the script or its own duration.
    Script,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Note {
    pub(crate) host: HostNote,
    pub(crate) origin: Origin,
    /// The key that sounds (a script may move it before the note starts).
    pub(crate) key: u8,
    /// 0..=1.
    pub(crate) velocity: f64,
    /// MIDI velocity 1..=127, what zones and crossfades select on.
    pub(crate) velocity_byte: u8,
    pub(crate) at: SampleTime,
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
    /// Voices started (or the note was ignored): later script changes move
    /// sounding voices instead of choosing zones.
    pub(crate) started: bool,
    /// A script ignored the event: it starts no voices.
    pub(crate) ignored: bool,
    /// Script tuning, millicents.
    pub(crate) script_tune: i32,
    /// Script volume, millidecibels.
    pub(crate) script_volume: i32,
    /// Script pan, per mille.
    pub(crate) script_pan: i32,
    /// Sample start offset a script asked for, microseconds.
    pub(crate) offset: u32,
    /// `EVENT_PAR_0..3`, free for the script.
    pub(crate) custom: [i32; 4],
    /// "From script" modulator values, `(index, value)`, ±1 000 000.
    // ponytail: four indices per note; a ring when a library sends more.
    pub(crate) mod_values: [Option<(u16, i32)>; 4],
    /// The first script slot that sees the note: 0 for a host note, one past
    /// the slot that played it.
    pub(crate) first: u8,
    /// Script slots whose note callback has let it through.
    pub(crate) pass: u8,
    /// Event marks (`set_event_mark`), one bit each.
    pub(crate) marks: u32,
}

impl Note {
    pub(crate) fn new(
        host: HostNote,
        velocity: f64,
        tune: f64,
        at: SampleTime,
        draw: RrDraw,
    ) -> Self {
        let velocity = velocity.clamp(0.0, 1.0);
        Self {
            host,
            origin: Origin::Host,
            key: host.key,
            velocity,
            velocity_byte: (velocity * 127.0).round().max(1.0) as u8,
            at,
            draw,
            tune,
            expression_tune: 0.0,
            expression_gain: 1.0,
            expression_pan: 0.0,
            pedal_held: false,
            released: false,
            started: false,
            ignored: false,
            script_tune: 0,
            script_volume: 0,
            script_pan: 0,
            offset: 0,
            custom: [0; 4],
            mod_values: [None; 4],
            first: 0,
            pass: 0,
            marks: 0,
        }
    }

    /// The value sent for "from script" modulator `index`; 0 until sent.
    pub(crate) fn mod_value(&self, index: u16) -> i32 {
        self.mod_values
            .iter()
            .flatten()
            .find(|(i, _)| *i == index)
            .map_or(0, |&(_, v)| v)
    }

    /// Keeps `value` for `index` when a slot is free or already holds it.
    pub(crate) fn set_mod_value(&mut self, index: u16, value: i32) {
        let value = value.clamp(-1_000_000, 1_000_000);
        if let Some(slot) = self
            .mod_values
            .iter_mut()
            .find(|m| m.is_none_or(|(i, _)| i == index))
        {
            *slot = Some((index, value));
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

    /// Host notes the pattern addresses; script notes answer only to the script.
    pub(crate) fn matching(&self, pattern: HostPattern, out: &mut Vec<NoteRef>) {
        self.select(out, |n| n.origin == Origin::Host && pattern.matches(n.host));
    }
}
