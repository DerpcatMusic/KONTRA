//! The note ledger: one table of sounding notes, addressed by generational handles.
//! A note owns its voices; every operation on voices goes through their note.

use kontra_events::{HostNote, HostPattern};
use kontra_time::SampleTime;

use kontra_behaviour::RrDraw;

/// A note handle. Stale handles (the note ended) match nothing.
/// Generations a slot cycles through: 13 bits, so a note's event ID (the
/// generation above bit 16) never sets the sign or `event::BY_MARKS`.
const GENERATIONS: u32 = 0x1FFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteRef {
    index: u32,
    generation: u32,
}

impl NoteRef {
    /// The note's slot in the ledger, for tables kept beside it.
    pub fn slot(self) -> usize {
        self.index as usize
    }

    /// The script-visible event id: positive, unique among live notes, and
    /// stale once the note ends. Ledgers hold at most 65536 notes.
    pub fn event_id(self) -> i32 {
        (((self.generation & GENERATIONS) << 16) | (self.index & 0xFFFF)) as i32
    }

    /// The handle an event id names; `None` for a non-note id.
    pub fn from_event_id(id: i32) -> Option<Self> {
        let id = u32::try_from(id).ok().filter(|&id| id >> 16 != 0)?;
        Some(Self {
            index: id & 0xFFFF,
            generation: id >> 16,
        })
    }
}

/// Where a note came from and how it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A host note: released by its note-off.
    Host,
    /// A script note released with its parent (`play_note` duration -1).
    Child(NoteRef),
    /// A script note released only by the script or its own duration.
    Script,
}

/// What a note's release sounds: its release-trigger zones once released, or,
/// for a note a release callback plays (that release's own sound), with its
/// start when it plays the whole sample and never when it is timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Releases {
    OnRelease,
    AtStart,
    Never,
}

#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub host: HostNote,
    pub origin: Origin,
    /// The key that sounds (a script may move it before the note starts).
    pub key: u8,
    /// 0..=1.
    pub velocity: f64,
    /// MIDI velocity 1..=127, what zones and crossfades select on.
    pub velocity_byte: u8,
    pub at: SampleTime,
    /// The round-robin draw of the attack; release triggers reuse it.
    pub draw: RrDraw,
    /// Initial tuning from the host, semitones.
    pub tune: f64,
    pub expression_tune: f64,
    pub expression_gain: f32,
    pub expression_pan: f32,
    /// Key released while the sustain pedal was down.
    pub pedal_held: bool,
    /// Key released (directly or by the pedal coming up).
    pub released: bool,
    /// Key up, but a release callback ignored it: the voices play on.
    pub key_up: bool,
    /// Voices started (or the note was ignored): later script changes move
    /// sounding voices instead of choosing zones.
    pub started: bool,
    /// A script ignored the event: it starts no voices.
    pub ignored: bool,
    /// What its release sounds.
    pub releases: Releases,
    /// Its player ended it with `note_off`: only the slots after hear it.
    pub noted_off: bool,
    /// The last fade a script asked of it (target, frames): its attack
    /// voices yet to start begin with it.
    pub fade: Option<(kontra_voice::FadeTarget, u32)>,
    /// Script tuning, millicents.
    pub script_tune: i32,
    /// Script volume, millidecibels.
    pub script_volume: i32,
    /// Script pan, per mille.
    pub script_pan: i32,
    /// Sample start offset a script asked for, microseconds.
    pub offset: u32,
    /// `EVENT_PAR_0..3`, free for the script.
    pub custom: [i32; 4],
    /// The zone slot the note starts; -1 for any.
    pub zone_slot: i32,
    /// "From script" modulator values, `(index, value)`, ±1 000 000.
    // ponytail: four indices per note; a ring when a library sends more.
    pub mod_values: [Option<(u16, Ramp)>; 4],
    /// The first script slot that sees the note: 0 for a host note, one past
    /// the slot that played it.
    pub first: u8,
    /// Script slots whose note callback has let it through.
    pub pass: u8,
    /// Event marks (`set_event_mark`), one bit each.
    pub marks: u32,
}

impl Note {
    pub fn new(host: HostNote, velocity: f64, tune: f64, at: SampleTime, draw: RrDraw) -> Self {
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
            key_up: false,
            started: false,
            ignored: false,
            releases: Releases::OnRelease,
            noted_off: false,
            fade: None,
            script_tune: 0,
            script_volume: 0,
            script_pan: 0,
            offset: 0,
            custom: [0; 4],
            zone_slot: -1,
            mod_values: [None; 4],
            first: 0,
            pass: 0,
            marks: 0,
        }
    }

    /// The key went up, whether or not its release was ignored.
    pub fn is_up(&self) -> bool {
        self.released || self.key_up
    }

    /// The value sent this note for "from script" modulator `index`, at `now`.
    pub fn mod_value(&self, index: u16, now: SampleTime) -> Option<i32> {
        ramp_at(&self.mod_values, index, now)
    }

    /// Moves `index` to `value` over `frames` from `now`, starting where it
    /// is (`from` when this note was never sent it).
    pub fn set_mod_value(
        &mut self,
        index: u16,
        value: i32,
        now: SampleTime,
        frames: u64,
        from: i32,
    ) {
        set_ramp(&mut self.mod_values, index, value, now, frames, from);
    }
}

/// A script modulation value moving linearly from `from` to `to` over
/// `frames` from `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ramp {
    from: i32,
    to: i32,
    start: u64,
    frames: u64,
}

impl Ramp {
    fn at(self, now: SampleTime) -> i32 {
        let done = now.0.saturating_sub(self.start);
        if done >= self.frames {
            return self.to;
        }
        let t = done as f64 / self.frames as f64;
        (f64::from(self.from) + (f64::from(self.to) - f64::from(self.from)) * t) as i32
    }
}

/// One resting ramp for each "from script" index the plan's routes read, so
/// every index a route reads keeps its value however many a script sends,
/// and [`SPARE_RAMPS`] free ones that keep values only scripts read back.
pub fn script_ramps(plan: &kontra_plan::Plan) -> Box<[Option<(u16, Ramp)>]> {
    use kontra_ir::Source;
    let blends = plan.buses.iter().flat_map(|b| &b.blends);
    let inputs = blends
        .flat_map(|b| &b.terms)
        .flat_map(|t| std::iter::once(&t.input).chain(t.factors.iter().map(|f| &f.input)))
        .map(|t| t.source)
        .chain(
            plan.groups
                .iter()
                .flat_map(|g| &g.shapers)
                .map(|t| t.source),
        );
    let routes = (plan.groups.iter().flat_map(|g| &g.mods).map(|m| &m.kind))
        .chain(plan.buses.iter().flat_map(|b| &b.mods));
    let mut indices: Vec<u16> = (routes.filter_map(kontra_plan::source))
        .chain(inputs)
        .filter_map(|s| match s {
            Source::Script(i) => Some(i),
            _ => None,
        })
        .collect();
    indices.sort_unstable();
    indices.dedup();
    let rest = Ramp {
        from: 0,
        to: 0,
        start: 0,
        frames: 0,
    };
    let spare = std::iter::repeat_n(None, SPARE_RAMPS);
    indices
        .into_iter()
        .map(|i| Some((i, rest)))
        .chain(spare)
        .collect()
}

/// Indices no route reads that keep their all-events value for scripts.
// ponytail: eight; the ninth such index is dropped.
const SPARE_RAMPS: usize = 8;

/// The value of `index` among `ramps` at `now`.
pub fn ramp_at(ramps: &[Option<(u16, Ramp)>], index: u16, now: SampleTime) -> Option<i32> {
    ramps
        .iter()
        .flatten()
        .find(|(i, _)| *i == index)
        .map(|&(_, r)| r.at(now))
}

/// Starts `index` towards `to` over `frames` from where it is at `now`
/// (`from` when absent), in a free slot or the one it holds.
pub fn set_ramp(
    ramps: &mut [Option<(u16, Ramp)>],
    index: u16,
    to: i32,
    now: SampleTime,
    frames: u64,
    from: i32,
) {
    let from = ramp_at(ramps, index, now).unwrap_or(from);
    if let Some(slot) = ramps.iter_mut().find(|m| m.is_none_or(|(i, _)| i == index)) {
        *slot = Some((
            index,
            Ramp {
                from,
                to: to.clamp(-1_000_000, 1_000_000),
                start: now.0,
                frames,
            },
        ));
    }
}

struct Entry {
    generation: u32,
    note: Option<Note>,
}

/// Fixed capacity. Queries fill a caller's list up to its capacity and never allocate.
pub struct Ledger {
    entries: Vec<Entry>,
}

/// The most notes a ledger holds: an event id keeps 16 bits of the slot.
pub const MAX_NOTES: usize = 1 << 16;

impl Ledger {
    /// Room for `capacity` notes, at most [`MAX_NOTES`] (more would share
    /// event ids). Not real-time.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: (0..capacity.min(MAX_NOTES))
                .map(|_| Entry {
                    generation: 0,
                    note: None,
                })
                .collect(),
        }
    }

    /// How many notes it holds at most.
    pub fn capacity(&self) -> usize {
        self.entries.len()
    }

    /// Stores `note` in a free slot; `None` when every slot is taken.
    pub fn insert(&mut self, note: Note) -> Option<NoteRef> {
        let (index, entry) = self
            .entries
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.note.is_none())?;
        // event_id carries 13 bits and 0 is invalid: stay in 1..=GENERATIONS.
        entry.generation = entry.generation % GENERATIONS + 1;
        entry.note = Some(note);
        Some(NoteRef {
            index: index as u32,
            generation: entry.generation,
        })
    }

    /// The most recently started live note on `key`, if any.
    pub fn newest(&self, key: u8) -> Option<&Note> {
        self.entries
            .iter()
            .filter_map(|e| e.note.as_ref())
            .filter(|n| n.key == key)
            .max_by_key(|n| n.at.0)
    }

    /// The note `r` names; `None` once it ended.
    pub fn get(&self, r: NoteRef) -> Option<&Note> {
        self.entries
            .get(r.index as usize)
            .filter(|e| e.generation == r.generation)?
            .note
            .as_ref()
    }

    /// As [`Self::get`], to change it.
    pub fn get_mut(&mut self, r: NoteRef) -> Option<&mut Note> {
        self.entries
            .get_mut(r.index as usize)
            .filter(|e| e.generation == r.generation)?
            .note
            .as_mut()
    }

    /// Ends the note `r` names and gives it back; `None` once it ended.
    pub fn remove(&mut self, r: NoteRef) -> Option<Note> {
        self.entries
            .get_mut(r.index as usize)
            .filter(|e| e.generation == r.generation)?
            .note
            .take()
    }

    /// Whether a live note is released with `parent`.
    pub fn parents(&self, parent: NoteRef) -> bool {
        let child = Origin::Child(parent);
        self.entries
            .iter()
            .any(|e| e.note.is_some_and(|n| n.origin == child))
    }

    /// Fills `out` with the live notes `keep` accepts, oldest slot first.
    /// Adds the live notes `keep` accepts to `out`, up to `out`'s capacity:
    /// it never grows `out`, so a list with no room gets none.
    pub fn select(&self, out: &mut Vec<NoteRef>, keep: impl Fn(&Note) -> bool) {
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

    /// The first child of `parent` from slot `from` on, not yet released:
    /// walked a slot at a time, so the walk may release and insert notes.
    pub fn next_child(&self, parent: NoteRef, from: usize) -> Option<NoteRef> {
        let mut entries = self.entries.get(from..)?.iter().enumerate();
        entries.find_map(|(offset, e)| {
            let n = e.note.as_ref()?;
            (n.origin == Origin::Child(parent) && !n.released).then_some(NoteRef {
                index: (from + offset) as u32,
                generation: e.generation,
            })
        })
    }

    /// Marks every live note released: a pending end of one then starts no
    /// release zones and runs no release callback.
    pub fn release_all(&mut self) {
        let notes = self.entries.iter_mut().filter_map(|e| e.note.as_mut());
        notes.for_each(|n| n.released = true);
    }

    /// Every live note with its reference.
    pub fn live(&self) -> impl Iterator<Item = (NoteRef, &Note)> {
        self.entries.iter().enumerate().filter_map(|(index, e)| {
            let r = NoteRef {
                index: index as u32,
                generation: e.generation,
            };
            e.note.as_ref().map(|n| (r, n))
        })
    }

    /// How many live notes `keep` accepts.
    pub fn count(&self, keep: impl Fn(&Note) -> bool) -> usize {
        self.entries
            .iter()
            .filter(|e| e.note.as_ref().is_some_and(&keep))
            .count()
    }

    /// Host notes the pattern addresses; script notes answer only to the script.
    pub fn matching(&self, pattern: HostPattern, out: &mut Vec<NoteRef>) {
        self.select(out, |n| n.origin == Origin::Host && pattern.matches(n.host));
    }
}
