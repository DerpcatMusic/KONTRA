//! Auto-align timing (experimental).
//!
//! Many libraries sound late on purpose: a legato transition, a breath or
//! bow noise, or a script that waits before it plays puts the audible
//! attack 50–350 ms after the note-on. Each part's articulations are
//! measured once ([`measure`], off the audio thread, kept with the part),
//! the latest of them all is reported to the host as the plugin's latency,
//! and every note is held back by what its own articulation lacks of that
//! ([`Scheduler`]): the host plays the MIDI early by the latency, and each
//! attack lands on the grid.
//!
//! The note is held back, not the audio, and each event of a note (its
//! release, its per-note expression) by as much as the note. Articulations
//! whose legato sounds later than their first note have a legato script:
//! for those a note never plays before an earlier note-on or release, so a
//! legato stays a legato and a detached note stays detached; a note that
//! would have to (a transition due sooner than the note it leaves) waits
//! and sounds late by the difference. Other articulations are polyphonic,
//! and their notes play whenever they are due.

use crate::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest, RACK_SLOTS,
    event::Event,
    v2::{V2Core, V2Loader},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
/// The latest a part may sound and still be aligned; later is reported as this.
pub const MAX_MS: f32 = 500.0;
/// Where a note's attack is: its 5 ms loudness first within this of its loudest.
pub const ONSET_DB: f32 = -20.0;
/// Velocities measured, one per [`bucket`]: legato speeds follow velocity
/// in most libraries that have them (≤64 slow, 65–100 medium, 101+ fast).
pub const VELOCITIES: [u8; 3] = [48, 90, 120];

/// The velocity bucket `velocity` falls in.
pub fn bucket(velocity: u8) -> usize {
    match velocity {
        0..=64 => 0,
        65..=100 => 1,
        _ => 2,
    }
}

/// How late one articulation sounds, in ms after the note-on: as a first
/// note and as a legato (another note held), per velocity bucket; `None`
/// where nothing could be measured.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Delay {
    pub name: String,
    /// Stable articulation source identity; names may repeat.
    pub identity: String,
    pub first: [Option<f32>; 3],
    pub legato: [Option<f32>; 3],
}

impl Delay {
    /// The delay of a note: legato or first, at its velocity's bucket, else
    /// the nearest bucket measured, else the other kind.
    pub fn ms(&self, legato: bool, velocity: u8) -> Option<f32> {
        let near = |row: &[Option<f32>; 3]| {
            let b = bucket(velocity) as isize;
            (0..3isize)
                .flat_map(|d| [b - d, b + d])
                .filter(|i| (0..3).contains(i))
                .find_map(|i| row[i as usize])
        };
        let (want, other) = if legato {
            (&self.legato, &self.first)
        } else {
            (&self.first, &self.legato)
        };
        near(want).or_else(|| near(other))
    }

    /// The latest it sounds.
    pub fn max(&self) -> Option<f32> {
        self.first
            .iter()
            .chain(&self.legato)
            .flatten()
            .copied()
            .reduce(f32::max)
    }

    /// A legato script plays it: at some velocity a legato sounds more than
    /// 15 ms later than a first note.
    pub fn mono(&self) -> bool {
        self.first
            .iter()
            .zip(&self.legato)
            .any(|(f, l)| matches!((f, l), (Some(f), Some(l)) if l - f > 15.0))
    }
}

/// A part's timing, saved with it: what was measured and for which
/// instrument, and the player's override.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Timing {
    /// [`source`] of the instrument measured; another's measurements are stale.
    pub source: String,
    /// The instrument as it loads, before any articulation is picked.
    pub loaded: Delay,
    /// Per articulation of the part's list, by name.
    pub arts: Vec<Delay>,
    /// What the library's own panel says, ms ([`declared`]): one figure for
    /// the whole patch, so used only where nothing could be measured.
    pub declared: Option<f32>,
    /// Every note of the part is this late (ms), whatever was measured.
    pub override_ms: Option<f32>,
    /// Not aligned: plays as late as the library does.
    pub exclude: bool,
}

/// What a [`Timing`] was measured for.
pub fn source(path: &str, program: u32, snapshot: &str) -> String {
    serde_json::to_string(&(path, program, snapshot)).unwrap()
}

impl Timing {
    pub fn measured(&self, path: &str, program: u32, snapshot: &str) -> bool {
        !path.is_empty() && self.source == source(path, program, snapshot)
    }

    /// What was measured for an articulation, by name (`None`: as loaded).
    fn delay(&self, art: Option<&str>) -> &Delay {
        art.and_then(|name| self.arts.iter().find(|d| d.identity == name))
            .unwrap_or(&self.loaded)
    }

    /// How late a note is, by articulation name (`None`: as loaded).
    fn own(&self, art: Option<&str>, legato: bool, velocity: u8) -> f32 {
        if self.exclude {
            return 0.0;
        }
        if let Some(ms) = self.override_ms {
            return ms;
        }
        self.delay(art)
            .ms(legato, velocity)
            .or(self.declared)
            .unwrap_or(0.0)
    }

    /// The latest any note of the part sounds, as aligned (0 excluded).
    pub fn latest(&self) -> f32 {
        if self.exclude {
            return 0.0;
        }
        if let Some(ms) = self.override_ms {
            return ms.clamp(0.0, MAX_MS);
        }
        (self.arts.iter().chain([&self.loaded]))
            .filter_map(Delay::max)
            .reduce(f32::max)
            .or(self.declared)
            .unwrap_or(0.0)
            .clamp(0.0, MAX_MS)
    }

    /// Where the part's figure comes from, for its menu.
    pub fn basis(&self) -> &'static str {
        if self.exclude {
            "excluded"
        } else if self.override_ms.is_some() {
            "set by hand"
        } else if self
            .arts
            .iter()
            .chain([&self.loaded])
            .any(|d| d.max().is_some())
        {
            "measured"
        } else if self.declared.is_some() {
            "as the library states"
        } else if self.source.is_empty() {
            "not measured yet"
        } else {
            "nothing measured"
        }
    }
}

/// The latency to report, in ms: the latest part.
pub fn reported_ms(latest: impl IntoIterator<Item = f32>) -> f32 {
    latest.into_iter().fold(0.0, f32::max).clamp(0.0, MAX_MS)
}

/// How long a note that sounds `own` ms late waits to land at `reported`.
pub fn hold_ms(reported: f32, own: f32) -> f32 {
    (reported - own.clamp(0.0, MAX_MS)).max(0.0)
}

/// Articulation rows a part's holds keep, the last for "as loaded" or unknown.
/// Source rows keep their immutable native order, with a final loaded row.
pub const LOADED: usize = usize::MAX;

/// A part's holds for the audio thread: per articulation row, first note or
/// legato, and velocity bucket, in tenths of a millisecond; and which rows
/// a legato script plays ([`Delay::mono`]).
#[derive(Clone, PartialEq, Debug)]
pub struct Holds(Vec<[[u16; 3]; 2]>, Vec<bool>);

impl Default for Holds {
    fn default() -> Self {
        Self(vec![[[0; 3]; 2]], vec![false])
    }
}

impl Holds {
    /// Holds for a part with `timing` whose articulation list names `arts`
    /// (in routing order), against `reported` ms.
    pub fn of(timing: &Timing, arts: &[&str], reported: f32) -> Self {
        if timing.exclude {
            return Self::default();
        }
        let loaded = arts.len();
        let mut holds = Self(vec![[[0; 3]; 2]; loaded + 1], vec![false; loaded + 1]);
        for (row, h) in holds.0.iter_mut().enumerate() {
            let art = if row == loaded {
                None
            } else {
                arts.get(row).copied()
            };
            if row != loaded && art.is_none() {
                continue;
            }
            holds.1[row] = timing.delay(art).mono();
            for (legato, per) in h.iter_mut().enumerate() {
                for (b, hold) in per.iter_mut().enumerate() {
                    let own = timing.own(art, legato == 1, VELOCITIES[b]);
                    *hold = (hold_ms(reported, own) * 10.0).round() as u16;
                }
            }
        }
        holds
    }

    pub fn frames(&self, row: usize, legato: bool, velocity: u8, rate: f64) -> u64 {
        let tenths = self.0[row.min(self.0.len() - 1)][usize::from(legato)][bucket(velocity)];
        (f64::from(tenths) * rate / 10_000.0).round() as u64
    }
}

/// Everything the audio thread aligns by, from the loader.
#[derive(Clone, PartialEq, Debug)]
pub struct Plan {
    /// Auto-align is on.
    pub on: bool,
    /// Hold notes back only while the host's transport plays.
    pub transport_only: bool,
    /// Reported latency, ms.
    pub latency_ms: f32,
    pub parts: Vec<Arc<Holds>>,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            on: false,
            transport_only: false,
            latency_ms: 0.0,
            parts: (0..RACK_SLOTS).map(|_| Arc::default()).collect(),
        }
    }
}

impl Plan {
    /// The latency to report, in frames at `rate`.
    pub fn latency(&self, rate: f64) -> u32 {
        if self.on {
            (f64::from(self.latency_ms) * rate / 1000.0).round() as u32
        } else {
            0
        }
    }
}

impl Holds {
    fn row(&self, row: usize) -> usize {
        row.min(self.0.len() - 1)
    }
    fn mono(&self, row: usize) -> bool {
        self.1[self.row(row)]
    }
}
impl moose::core::custom_state::StateField for Timing {
    fn write_field(&self, b: &mut Vec<u8>) {
        serde_json::to_string(self)
            .unwrap_or_default()
            .write_field(b);
    }
    fn read_field(c: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        Some(serde_json::from_str(&String::read_field(c)?).unwrap_or_default())
    }
}
/// Events a part may hold back at once; past this they play at once.
const CAPACITY: usize = 2048;
const UP: u32 = u32::MAX;
/// A keyswitch taken over by the scheduler: its release goes too.
const SWITCH: u32 = u32::MAX - 1;
/// Marks a held note a legato script plays.
const MONO: u32 = 1 << 30;
pub(crate) const NO_ART: usize = usize::MAX;

#[derive(Clone, Copy)]
struct Queued {
    due: u64,
    ev: Event,
    /// Articulation to switch to before this note plays, or [`NO_ART`].
    art: usize,
}

/// One part's held-back events, in the order they play.
#[derive(Clone, Copy)]
struct HostHold {
    note: crate::sound::event::HostNote,
    hold: u64,
    mono: bool,
    held: bool,
}

pub struct Scheduler {
    host: Vec<HostHold>,
    queue: Box<[Queued]>,
    head: usize,
    len: usize,
    /// Articulation row the next note plays, as the keyswitches say so far.
    art: usize,
    /// Per channel and key: frames its note-on was held back (with
    /// [`MONO`]), [`UP`] when not held, [`SWITCH`] for a keyswitch taken over.
    held: [[u32; 128]; 16],
    /// Per lane (channel mode: each channel plays its own articulation,
    /// else one): notes down, and its latest note-on's and release's due
    /// frames. A legato on one channel neither waits for nor counts another's.
    down: [u32; 16],
    /// The latest note-on's, release's and controller's due frames.
    last_on: [u64; 16],
    last_off: [u64; 16],
    /// A fresh note must not pass an earlier queued stop on its physical channel.
    stop_due: [u64; 16],
    last_ctl: u64,
    /// Frames the latest note was held back: controllers go with it.
    ctl_hold: u64,
    /// Events that found the queue full and played at once.
    pub overflows: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        let empty = Queued {
            due: 0,
            ev: Event::midi1(0xd0, 0, 0),
            art: NO_ART,
        };
        Self {
            host: Vec::with_capacity(4096),
            queue: vec![empty; CAPACITY].into_boxed_slice(),
            head: 0,
            len: 0,
            art: LOADED,
            held: [[UP; 128]; 16],
            down: [0; 16],
            last_on: [0; 16],
            last_off: [0; 16],
            stop_due: [0; 16],
            last_ctl: 0,
            ctl_hold: 0,
            overflows: 0,
        }
    }
}

impl Scheduler {
    fn at(&self, i: usize) -> &Queued {
        &self.queue[(self.head + i) % CAPACITY]
    }

    fn at_mut(&mut self, i: usize) -> &mut Queued {
        &mut self.queue[(self.head + i) % CAPACITY]
    }

    /// Queue `q` after everything due no later than it. False when full.
    fn push(&mut self, q: Queued) -> bool {
        if self.len == CAPACITY {
            self.overflows += 1;
            return false;
        }
        let mut i = self.len;
        self.len += 1;
        while i > 0 && self.at(i - 1).due > q.due {
            *self.at_mut(i) = *self.at(i - 1);
            i -= 1;
        }
        *self.at_mut(i) = q;
        true
    }

    /// The frame the next held event is due, if any.
    pub fn next_due(&self) -> Option<u64> {
        (self.len > 0).then(|| self.at(0).due)
    }

    /// Hold `ev`, arriving at frame `now`, back by its articulation's hold.
    /// Returns it when it must play at once (the queue is full).
    pub fn arrive(
        &mut self,
        original: Event,
        now: u64,
        holds: &Holds,
        rate: f64,
        router: &Router,
    ) -> Option<Event> {
        let Some(ev) = In::of(original) else {
            return Some(original);
        };
        if let Some(row) = router.controller_of(ev) {
            self.art = row;
        }
        if let In::HostOff(pattern) | In::HostChoke(pattern) | In::HostExpression(pattern, _) = ev {
            if !self.host.iter().any(|o| pattern.matches(o.note)) {
                return Some(original);
            }
            for i in 0..self.host.len() {
                let owner = self.host[i];
                if !pattern.matches(owner.note) || (matches!(ev, In::HostOff(_)) && !owner.held) {
                    continue;
                }
                let pattern = crate::sound::event::HostPattern {
                    port: i32::from(owner.note.port),
                    channel: i32::from(owner.note.channel),
                    key: i32::from(owner.note.key),
                    id: owner.note.id,
                    clap: owner.note.clap,
                };
                let to = match ev {
                    In::HostOff(_) => Event::NoteOff(pattern),
                    In::HostChoke(_) => Event::Choke(pattern),
                    In::HostExpression(_, x) => Event::Expression(pattern, x),
                    _ => unreachable!(),
                };
                let lane = if router.by_channel() {
                    owner.note.channel as usize
                } else {
                    0
                };
                let due = (now + owner.hold).max(if owner.mono { self.last_on[lane] } else { 0 });
                if matches!(ev, In::HostOff(_) | In::HostChoke(_)) {
                    self.host[i].held = false;
                    if owner.held {
                        self.down[lane] = self.down[lane].saturating_sub(1);
                    }
                    self.last_off[lane] = self.last_off[lane].max(due);
                    if !self.host.iter().any(|o| {
                        o.held
                            && o.note.channel == owner.note.channel
                            && o.note.key == owner.note.key
                    }) {
                        self.held[owner.note.channel as usize][owner.note.key as usize] = UP;
                    }
                }
                if !self.push(Queued {
                    due,
                    ev: to,
                    art: NO_ART,
                }) {
                    return Some(original);
                }
            }
            return None;
        }
        if let In::HostOn(note, ..) = ev {
            if self.host.len() == self.host.capacity()
                || (note.id != -1 && self.host.iter().any(|o| o.note == note))
            {
                self.overflows = self.overflows.saturating_add(1);
                return None;
            }
        }
        let frames = |row, legato, velocity| holds.frames(row, legato, velocity, rate);
        let lane = |channel: u8| {
            if router.by_channel() {
                usize::from(channel & 15)
            } else {
                0
            }
        };
        let (due, art) = match ev {
            In::NoteOn(channel, note, velocity)
            | In::HostOn(
                crate::sound::event::HostNote {
                    channel, key: note, ..
                },
                velocity,
                _,
            ) => {
                let key = &mut self.held[usize::from(channel & 15)][usize::from(note & 127)];
                let (row, switch) = router.articulation_of(channel, note, velocity);
                if switch {
                    // Played before the note it picks for, by the router.
                    if let Some(row) = row {
                        self.art = row;
                        *key = SWITCH;
                        return None;
                    }
                }
                let row = row.unwrap_or(self.art);
                let lane = lane(channel);
                let hold = frames(row, self.down[lane] > 0, velocity);
                // A legato script hears notes and releases in the order played.
                let after = if holds.mono(row) {
                    self.last_on[lane].max(self.last_off[lane])
                } else {
                    0
                };
                let due = (now + hold)
                    .max(after)
                    .max(self.stop_due[usize::from(channel & 15)]);
                self.last_on[lane] = self.last_on[lane].max(due);
                self.ctl_hold = due - now;
                let key = &mut self.held[usize::from(channel & 15)][usize::from(note & 127)];
                if *key == UP || *key == SWITCH || matches!(ev, In::HostOn(..)) {
                    self.down[lane] += 1;
                }
                let mono = if holds.mono(row) { MONO } else { 0 };
                *key = ((due - now) as u32).min(MONO - 1) | mono;
                if let In::HostOn(note, ..) = ev {
                    self.host.push(HostHold {
                        note,
                        hold: due - now,
                        mono: mono != 0,
                        held: true,
                    });
                }
                let art = if row == LOADED { NO_ART } else { row };
                (due, art)
            }
            In::NoteOff(channel, note) => {
                let key = &mut self.held[usize::from(channel & 15)][usize::from(note & 127)];
                let (hold, mono) = match std::mem::replace(key, UP) {
                    SWITCH => return None,
                    UP => (self.ctl_hold, false),
                    held => {
                        self.down[lane(channel)] = self.down[lane(channel)].saturating_sub(1);
                        (u64::from(held & !MONO), held & MONO != 0)
                    }
                };
                // Under a legato script a release never passes a note-on
                // that came before it: an overlap stays one.
                let lane = lane(channel);
                let due = if mono {
                    (now + hold).max(self.last_on[lane])
                } else {
                    now + hold
                };
                self.last_off[lane] = self.last_off[lane].max(due);
                (due, NO_ART)
            }
            // Exact wildcard/old-ID events cannot borrow a newer key row's
            // hold. Queue after the preceding ingress so their owner exists.
            In::HostOff(_) | In::HostChoke(_) | In::HostExpression(_, _) => {
                let latest = if self.len > 0 {
                    self.at(self.len - 1).due
                } else {
                    now
                };
                (latest.max(now), NO_ART)
            }
            // Per note: with its note.
            In::PolyAt(channel, note, _)
            | In::NoteTune(channel, note, _)
            | In::NotePressure(channel, note, _)
            | In::NoteGain(channel, note, _)
            | In::NotePan(channel, note, _)
            | In::NoteBrightness(channel, note, _) => {
                let hold = match self.held[usize::from(channel & 15)][usize::from(note & 127)] {
                    UP | SWITCH => self.ctl_hold,
                    held => u64::from(held & !MONO),
                };
                ((now + hold).max(self.last_ctl), NO_ART)
            }
            // All notes or sound off: after every note already waiting.
            In::Cc(channel, cc @ 120.., _) => {
                let latest = if self.len > 0 {
                    self.at(self.len - 1).due
                } else {
                    0
                };
                let due = (now + self.ctl_hold).max(latest);
                self.last_ctl = due;
                if matches!(cc, 120 | 123) {
                    let channels = router.stop_channels(channel);
                    for c in (0..16).filter(|c| channels & (1 << c) != 0) {
                        self.held[c].fill(UP);
                        self.stop_due[c] = self.stop_due[c].max(due);
                    }
                    self.down.fill(0);
                    for (c, row) in self.held.iter().enumerate() {
                        self.down[lane(c as u8)] += row
                            .iter()
                            .filter(|&&held| held != UP && held != SWITCH)
                            .count() as u32;
                    }
                }
                (due, NO_ART)
            }
            In::Cc(..)
            | In::Bend(..)
            | In::Pressure(..)
            | In::Program(..)
            | In::OtherControl(..) => {
                let due = (now + self.ctl_hold).max(self.last_ctl);
                self.last_ctl = due;
                (due, NO_ART)
            }
        };
        (!self.push(Queued {
            due,
            ev: original,
            art,
        }))
        .then_some(original)
    }

    pub fn pop_due(&mut self, now: u64) -> Option<(Event, usize)> {
        if self.len == 0 || self.at(0).due > now {
            return None;
        }
        let q = *self.at(0);
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        Some((q.ev, q.art))
    }
    pub fn picked(&mut self, row: usize) {
        self.art = row;
    }
    pub fn waiting(&self, note: crate::sound::event::HostNote) -> bool {
        (0..self.len).any(|i| match self.at(i).ev {
            Event::NoteOn { note: n, .. } => n == note,
            Event::NoteOff(p) | Event::Choke(p) | Event::Expression(p, _) => p.matches(note),
            _ => false,
        })
    }
    pub fn hosts(&self) -> impl Iterator<Item = crate::sound::event::HostNote> + '_ {
        self.host.iter().map(|h| h.note)
    }
    pub fn host_key_held(&self, channel: u8, key: u8) -> bool {
        self.host
            .iter()
            .any(|o| o.held && o.note.channel == channel && o.note.key == key)
    }
    pub fn retire(&mut self, note: crate::sound::event::HostNote) {
        self.host.retain(|h| h.note != note);
    }
    /// Forget everything held: the host stopped processing.
    pub fn clear(&mut self) {
        self.host.clear();
        self.len = 0;
        self.head = 0;
        self.held = [[UP; 128]; 16];
        (self.down, self.last_on, self.last_off) = ([0; 16], [0; 16], [0; 16]);
        self.stop_due.fill(0);
        (self.last_ctl, self.ctl_hold) = (0, 0);
    }
}

/// Port v1's worker-allocated rack scheduler storage and exact-owner pins.
pub(crate) struct Align {
    pub plan: Arc<Plan>,
    pub parts: Box<[Scheduler]>,
    pub clock: u64,
    pub empty: Holds,
}
impl Align {
    pub fn with_slots(slots: usize) -> Self {
        Self {
            plan: Arc::default(),
            parts: (0..slots).map(|_| Scheduler::default()).collect(),
            clock: 0,
            empty: Holds::default(),
        }
    }
    pub fn adopt_parts(&mut self, prepared: &mut Self) {
        for (current, next) in self.parts.iter_mut().zip(&mut prepared.parts) {
            std::mem::swap(current, next);
        }
        std::mem::swap(&mut self.parts, &mut prepared.parts);
    }
    pub fn holding(&self, playing: bool) -> bool {
        self.plan.on && (playing || !self.plan.transport_only)
    }
    pub fn next_due(&self) -> Option<u64> {
        self.parts.iter().filter_map(Scheduler::next_due).min()
    }
    pub fn host_note_waiting(&self, note: crate::sound::event::HostNote) -> bool {
        self.parts.iter().any(|s| s.waiting(note))
    }
    pub fn host_note_at(&self, mut index: usize) -> Option<(crate::sound::event::HostNote, bool)> {
        for s in &self.parts {
            if let Some(o) = s.host.get(index) {
                return Some((o.note, o.held));
            }
            index = index.saturating_sub(s.host.len());
        }
        None
    }
    pub fn host_key_held(&self, channel: u8, key: u8) -> bool {
        self.parts.iter().any(|s| s.host_key_held(channel, key))
    }
    pub fn retire_host_note(&mut self, note: crate::sound::event::HostNote) {
        for s in &mut self.parts {
            s.retire(note);
        }
    }
    pub fn overflows(&self) -> u64 {
        self.parts.iter().map(|s| s.overflows).sum()
    }
}
impl Scheduler {
    /// Cancel queued work on replacement, retaining NOTE_END owners until the host accepts them.
    pub fn cancel(&mut self) {
        self.len = 0;
        self.head = 0;
        self.held = [[UP; 128]; 16];
        (self.down, self.last_on, self.last_off) = ([0; 16], [0; 16], [0; 16]);
        self.stop_due.fill(0);
        (self.last_ctl, self.ctl_hold) = (0, 0);
        for h in &mut self.host {
            h.held = false;
            h.hold = 0;
        }
    }
}

/// Mean power of `x` over 5 ms windows a millisecond apart.
fn power(x: &[f32], rate: f64) -> Vec<f64> {
    let hop = (rate / 1000.0).round().max(1.0) as usize;
    let win = 5 * hop;
    let mut sum = Vec::with_capacity(x.len() + 1);
    sum.push(0f64);
    for &v in x {
        sum.push(sum[sum.len() - 1] + f64::from(v) * f64::from(v));
    }
    (0..x.len().saturating_sub(win) / hop + 1)
        .filter(|i| i * hop + win <= x.len())
        .map(|i| (sum[i * hop + win] - sum[i * hop]) / win as f64)
        .collect()
}

/// When `x`, starting at a note-on, first comes within `db` of its loudest:
/// ms to the middle of the first 5 ms window that does. `None` when silent.
pub fn onset_ms(x: &[f32], rate: f64, db: f32) -> Option<f32> {
    let p = power(x, rate);
    let peak = p.iter().copied().fold(0.0, f64::max);
    if peak < 1e-12 {
        return None;
    }
    let at = p
        .iter()
        .position(|&v| v >= peak * 10f64.powf(f64::from(db) / 10.0))?;
    Some(at as f32 + 2.5)
}

/// Where `x` rises fastest within its first second: the middle of the 10 ms
/// its 5 ms loudness gains most dB over, ms. Research only.
pub fn steepest_ms(x: &[f32], rate: f64) -> Option<f32> {
    let p = power(x, rate);
    let peak = p.iter().copied().fold(0.0, f64::max);
    if peak < 1e-12 {
        return None;
    }
    let db: Vec<f64> = p
        .iter()
        .map(|v| 10.0 * (v.max(peak * 1e-6)).log10())
        .collect();
    let (at, _) = (0..db.len().saturating_sub(10).min(1000))
        .map(|i| (i, db[i + 10] - db[i]))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    Some(at as f32 + 5.0 + 2.5)
}

/// Energy of `x` at `hz` and its next two harmonics, Hann-windowed.
fn partials(x: &[f32], rate: f64, hz: f64) -> f64 {
    let n = x.len() as f64;
    (1..=3)
        .map(|k| {
            let w = 2.0 * std::f64::consts::PI * hz * f64::from(k) / rate;
            let (c, s) = x.iter().enumerate().fold((0.0, 0.0), |(c, s), (i, &v)| {
                let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n).cos();
                let v = f64::from(v) * hann;
                (c + v * (w * i as f64).cos(), s + v * (w * i as f64).sin())
            });
            c * c + s * s
        })
        .sum()
}

/// A MIDI note's frequency.
pub fn hz(note: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0)
}

/// Lead before the second note-on that a legato take keeps, ms.
pub const LEAD_MS: f64 = 60.0;

/// When the new note of a legato pair takes over: `x` starts [`LEAD_MS`]
/// before its note-on, with only the old note sounding. Energy at the new
/// note's first three partials over 40 ms windows 2 ms apart; the
/// transition is the middle of the first window within `db` of the
/// loudest, ms after the note-on. `None` when the old note alone already
/// comes within 6 dB of that (the partials cannot tell them apart).
pub fn transition_ms(x: &[f32], rate: f64, new: u8, db: f32) -> Option<f32> {
    let win = (0.040 * rate) as usize;
    let hop = (0.002 * rate) as usize;
    let lead = (LEAD_MS / 1000.0 * rate) as usize;
    if x.len() < lead + win {
        return None;
    }
    let e: Vec<f64> = (0..=(x.len() - win) / hop)
        .map(|i| partials(&x[i * hop..i * hop + win], rate, hz(new)))
        .collect();
    let peak = e.iter().copied().fold(0.0, f64::max);
    let threshold = peak * 10f64.powf(f64::from(db) / 10.0);
    let before = e[0];
    if peak < 1e-9 || before * 4.0 > threshold {
        return None;
    }
    let at = e.iter().position(|&v| v >= threshold)?;
    Some(((at * hop + win / 2) as f64 - lead as f64) as f32 / rate as f32 * 1000.0)
}

/// One articulation's takes at one velocity: a first note from silence and,
/// a second later, a legato to the note a third up, mono, with when the
/// engine first had voices for each (the scripts' own wait).
pub struct Take {
    pub first: Vec<f32>,
    /// From [`LEAD_MS`] before the second note-on.
    pub legato: Vec<f32>,
    pub first_voice_ms: Option<f32>,
    pub legato_voice_ms: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub enum Pick {
    Loaded,
    Articulation(usize),
}
/// Frames rendered at a time while measuring: the voice timing's resolution.
const STEP: usize = 16;

fn render(
    e: &mut Measuring,
    seconds: f64,
    out: Option<&mut Vec<f32>>,
    voices_from: Option<(usize, &mut Option<f32>)>,
) {
    let rate = e.sample_rate();
    let frames = (seconds * rate) as usize;
    let mut out = out;
    let mut voices_from = voices_from;
    let mut done = 0;
    while done < frames {
        if e.stopped || (e.canceled)() {
            e.stopped = true;
            break;
        }
        let n = STEP.min(frames - done);
        e.begin_block(&BlockInfo {
            frames: n,
            offline: true,
            ..Default::default()
        });
        let rendered = e.render(n);
        if let Some(out) = out.as_deref_mut() {
            out.extend(
                rendered.buses[0][0][..n]
                    .iter()
                    .zip(&rendered.buses[0][1][..n])
                    .map(|(l, r)| l + r),
            );
        }
        if let Some((before, at)) = voices_from.as_mut()
            && at.is_none()
            && e.voices().active > *before
        {
            **at = Some(done as f32 / rate as f32 * 1000.0);
        }
        e.take_effects(0, &mut |_, _| true);
        let _ = e.end_block(n, &mut |_| true);
        done += n;
    }
}

/// Let everything sounding go and ring out.
fn quiet(e: &mut Measuring) {
    for channel in 0..16 {
        e.play(0, Event::midi1(0xb0 | channel, 123, 0));
    }
    for _ in 0..40 {
        if e.voices().active == 0 {
            break;
        }
        render(e, 0.025, None, None);
    }
    let rate = e.sample_rate();
    e.reset(rate);
}

/// Play a take of `pick` at `velocity`: `note`, then a legato to `note + 4`.
fn take(e: &mut Measuring, pick: Pick, note: u8, velocity: u8) -> Take {
    let rate = e.sample_rate();
    quiet(e);
    // Dynamics and expression as a player leaves them, mostly up.
    e.play(0, Event::midi1(0xb0, 1, 100));
    e.play(0, Event::midi1(0xb0, 11, 127));
    match pick {
        Pick::Loaded => {}
        Pick::Articulation(row) => {
            let _ = e.select_articulation(0, row);
        }
    }
    render(e, 0.05, None, None);
    let second = note.saturating_add(4).min(127);
    let mut first = Vec::new();
    let mut first_voice_ms = None;
    e.play(0, Event::midi1(0x90, note, velocity));
    render(e, 1.0, Some(&mut first), Some((0, &mut first_voice_ms)));
    let lead = (LEAD_MS / 1000.0 * rate) as usize;
    let mut legato = first[first.len().saturating_sub(lead)..].to_vec();
    let mut legato_voice_ms = None;
    let before = e.voices().active;
    e.play(0, Event::midi1(0x90, second, velocity));
    render(
        e,
        0.05,
        Some(&mut legato),
        Some((before, &mut legato_voice_ms)),
    );
    e.play(0, Event::midi1(0x80, note, 0));
    render(e, 0.95, Some(&mut legato), None);
    e.play(0, Event::midi1(0x80, second, 0));
    render(e, 0.3, None, None);
    Take {
        first,
        legato,
        first_voice_ms,
        legato_voice_ms,
    }
}

/// How late `pick` sounds, from its takes at each of [`VELOCITIES`].
fn delay(e: &mut Measuring, name: &str, pick: Pick, note: u8) -> Delay {
    let rate = e.sample_rate();
    let mut d = Delay {
        name: name.to_owned(),
        ..Delay::default()
    };
    for (b, &velocity) in VELOCITIES.iter().enumerate() {
        if e.stopped {
            break;
        }
        let t = take(e, pick, note, velocity);
        d.first[b] = onset_ms(&t.first, rate, ONSET_DB);
        d.legato[b] = transition_ms(&t.legato, rate, note.saturating_add(4).min(127), ONSET_DB);
    }
    d
}

/// Measure a part: `e` holds its bank and a fresh copy of its scripts
/// (running notes through them changes their state); `arts` is its
/// articulation list (name, keyswitch, script control).
fn measure(e: &mut Measuring, arts: &[sampler_ir::Articulation], note: u8) -> (Delay, Vec<Delay>) {
    let loaded = delay(e, "", Pick::Loaded, note);
    let identities = crate::sound::articulation::identities(arts);
    let arts = arts
        .iter()
        .enumerate()
        .map(|(row, a)| {
            let mut d = delay(e, &a.name, Pick::Articulation(row), note);
            d.identity = identities[row].clone();
            d
        })
        .collect();
    (loaded, arts)
}

/// A note every articulation plays: above the keyswitches, the key with
/// the most zones (all its groups and layers), nearest the middle of those.
pub fn probe_note(i: &sampler_ir::Instrument, arts: &[sampler_ir::Articulation]) -> u8 {
    let lowest = arts
        .iter()
        .flat_map(|f| f.switch_keys.iter().copied())
        .max()
        .map_or(0, |k| k.saturating_add(1));
    let mut count = [0usize; 124];
    for z in i.zones.iter() {
        for k in z.keys.low.max(lowest)..=z.keys.high.min(123) {
            count[usize::from(k)] += 1;
        }
    }
    let most = count.iter().copied().max().unwrap_or(0);
    let keys: Vec<u8> = (0..124u8)
        .filter(|&k| most > 0 && count[usize::from(k)] * 10 >= most * 9)
        .collect();
    keys.get(keys.len() / 2).copied().unwrap_or(60)
}

/// Prepare an isolated native bank on the loader worker, with fresh scripts.
/// Measurements never mutate the playing part or a library's source IR.
pub fn measure_request(
    request: &LoadRequest,
    canceled: &(dyn Fn() -> bool + Sync),
) -> Result<Timing, crate::sound::CoreError> {
    let loaded = V2Loader.prepare(request, &mut |_| {}, canceled)?;
    let declared = declared(&loaded.interfaces, &loaded.controls);
    let instrument = loaded.instrument.clone();
    let mut core = V2Core::with_parts(1, request.sample_rate);
    core.install(0, loaded.part);
    let arts = instrument
        .as_ref()
        .map_or(&[][..], |i| i.articulations.as_slice());
    let note = instrument.as_ref().map_or(60, |i| probe_note(i, arts));
    let mut e = Measuring {
        core,
        canceled,
        stopped: false,
    };
    let (base, measured) = measure(&mut e, arts, note);
    if e.stopped || canceled() {
        return Err(crate::sound::CoreError::Canceled);
    }
    Ok(Timing {
        source: source(
            &request.path.to_string_lossy(),
            request.program,
            &request
                .snapshot
                .as_deref()
                .unwrap_or(std::path::Path::new(""))
                .to_string_lossy(),
        ),
        loaded: base,
        arts: measured,
        declared,
        ..Default::default()
    })
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) enum In {
    HostOn(crate::sound::event::HostNote, u8, f64),
    HostOff(crate::sound::event::HostPattern),
    HostChoke(crate::sound::event::HostPattern),
    HostExpression(
        crate::sound::event::HostPattern,
        crate::sound::event::NoteExpression,
    ),
    NoteOn(u8, u8, u8),
    NoteOff(u8, u8),
    Cc(u8, u8, u8),
    Program(u8, u8),
    Bend(u8, u16),
    Pressure(u8, u8),
    PolyAt(u8, u8, u8),
    NoteTune(u8, u8, f32),
    NotePressure(u8, u8, u8),
    NoteGain(u8, u8, f32),
    NotePan(u8, u8, f32),
    NoteBrightness(u8, u8, u8),
    OtherControl(u8),
}
impl In {
    fn of(event: Event) -> Option<Self> {
        Some(match event {
            Event::NoteOn {
                note,
                velocity,
                tune,
            } => Self::HostOn(
                note,
                (velocity.clamp(0., 1.) * 127.).round().max(1.) as u8,
                tune,
            ),
            Event::NoteOff(p) => Self::HostOff(p),
            Event::Choke(p) => Self::HostChoke(p),
            Event::Expression(p, x) => Self::HostExpression(p, x),
            Event::Ump([word, data]) => {
                let (kind, status, c, a, b) = (
                    word >> 28,
                    (word >> 16) as u8 & 0xf0,
                    (word >> 16) as u8 & 15,
                    (word >> 8) as u8 & 127,
                    word as u8 & 127,
                );
                if !matches!(kind, 2 | 4) {
                    return None;
                }
                let value = if kind == 2 { b } else { (data >> 25) as u8 };
                match status {
                    0x90 if kind == 2 && b == 0 || kind == 4 && data >> 16 == 0 => {
                        Self::NoteOff(c, a)
                    }
                    0x90 => Self::NoteOn(
                        c,
                        a,
                        if kind == 2 {
                            b
                        } else {
                            ((data >> 16) as f64 / 65535. * 127.).round().max(1.) as u8
                        },
                    ),
                    0x80 => Self::NoteOff(c, a),
                    0xb0 => Self::Cc(c, a, value),
                    0xc0 => Self::Program(
                        c,
                        if kind == 2 {
                            a
                        } else {
                            (data >> 24) as u8 & 127
                        },
                    ),
                    0xa0 => Self::PolyAt(c, a, value),
                    0xe0 => Self::Bend(c, 0),
                    0xd0 => Self::Pressure(c, value),
                    0x60 => Self::NoteTune(c, a, 0.),
                    0x00 | 0x10 => Self::NoteBrightness(c, a, 0),
                    _ => Self::OtherControl(c),
                }
            }
        })
    }
}
/// A borrow of the existing native routing table, never another articulation model.
pub(crate) struct Router<'a> {
    pub switching: &'a sampler_core::Switching,
    pub keys: &'a [sampler_core::Keyswitch],
    pub actions: &'a [Option<sampler_core::Switch>],
    pub mpe: bool,
}
impl Router<'_> {
    fn row(&self, s: sampler_core::Switch) -> Option<usize> {
        self.actions.iter().position(|a| *a == Some(s))
    }
    fn by_channel(&self) -> bool {
        !self.mpe && self.switching.driver() == sampler_core::Driver::Channel
    }
    fn stop_channels(&self, _channel: u8) -> u16 {
        u16::MAX
    }
    fn articulation_of(&self, c: u8, key: u8, velocity: u8) -> (Option<usize>, bool) {
        if let Some(s) = self.switching.key_input(key) {
            return (self.row(s), true);
        }
        if self.switching.blocked(key) {
            return (None, true);
        }
        if self.switching.is_switch_key(key) {
            let action = self
                .keys
                .iter()
                .find(|k| k.key == key)
                .map(|k| sampler_core::Switch::Articulation(k.articulation))
                .or_else(|| {
                    self.actions
                        .iter()
                        .flatten()
                        .find(|s| **s == sampler_core::Switch::Tap(key))
                        .copied()
                });
            if let Some(row) = action.and_then(|a| self.row(a)) {
                return (Some(row), true);
            }
        }
        let value = match self.switching.driver() {
            sampler_core::Driver::Velocity => Some(velocity),
            sampler_core::Driver::Channel => Some(c),
            _ => None,
        };
        (
            value
                .and_then(|v| self.switching.select(0, v))
                .and_then(|s| self.row(s)),
            false,
        )
    }
    fn controller_of(&self, ev: In) -> Option<usize> {
        match (self.switching.driver(), ev) {
            (sampler_core::Driver::Controller, In::Cc(_, cc, v)) => {
                self.switching.select(cc, v).and_then(|s| self.row(s))
            }
            (sampler_core::Driver::Program, In::Program(_, v)) => {
                self.switching.select(0, v).and_then(|s| self.row(s))
            }
            _ => None,
        }
    }
}

struct Measuring<'a> {
    core: V2Core,
    canceled: &'a (dyn Fn() -> bool + Sync),
    stopped: bool,
}
impl std::ops::Deref for Measuring<'_> {
    type Target = V2Core;
    fn deref(&self) -> &V2Core {
        &self.core
    }
}
impl std::ops::DerefMut for Measuring<'_> {
    fn deref_mut(&mut self) -> &mut V2Core {
        &mut self.core
    }
}
/// Port v1's negative sample/playback-offset recognition over initialized UI-IR.
fn declared(
    faces: &[sampler_ui_ir::Interface],
    values: &[(sampler_ui_ir::ControlId, f64)],
) -> Option<f32> {
    let found: Vec<_> = faces
        .iter()
        .flat_map(|f| &f.widgets)
        .filter(|w| {
            matches!(w.kind, sampler_ui_ir::Kind::ValueEdit { .. })
                && w.text.to_lowercase().contains("offset")
        })
        .filter_map(|w| {
            let sampler_ui_ir::Binding::Control(id) = &w.binding else {
                return None;
            };
            let v = values.iter().find(|(i, _)| i == id)?.1;
            (v.is_finite() && (-500.0..0.0).contains(&v)).then(|| (w.text.to_lowercase(), v))
        })
        .collect();
    let named: Vec<_> = found
        .iter()
        .filter(|(l, _)| ["sample", "plbk", "playback"].iter().any(|k| l.contains(k)))
        .collect();
    match (found.as_slice(), named.as_slice()) {
        ([(_, v)], _) | (_, [(_, v)]) => Some(-*v as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::event::{HostNote, HostPattern, NoteExpression};
    const RATE: f64 = 48000.;
    fn holds(first: f32, legato: f32, reported: f32) -> Holds {
        let d = Delay {
            first: [Some(first); 3],
            legato: [Some(legato); 3],
            ..Default::default()
        };
        Holds::of(
            &Timing {
                loaded: d,
                ..Default::default()
            },
            &[],
            reported,
        )
    }
    fn router(s: &sampler_core::Switching) -> Router<'_> {
        Router {
            switching: s,
            keys: &[],
            actions: &[],
            mpe: false,
        }
    }
    #[test]
    fn v1_exclusion_cancels_host_early_delivery_and_keeps_original_lateness() {
        let excluded = Timing {
            exclude: true,
            ..Default::default()
        };
        assert_eq!(excluded.latest(), 0.);
        assert_eq!(
            Holds::of(&excluded, &[], 120.).frames(LOADED, false, 100, RATE),
            120 * 48,
            "excluded parts hold the whole host latency, so their original late attack is unchanged"
        );
    }
    #[test]
    fn v1_onsets_and_legato_transition_analysis() {
        let mut x = vec![0f32; 4800];
        x.extend((0..24000).map(|i| (i as f32 * 0.05).sin() * 0.5));
        let at = onset_ms(&x, RATE, ONSET_DB).unwrap();
        assert!((at - 100.).abs() <= 3., "{at}");
        assert_eq!(onset_ms(&[0.; 1000], RATE, ONSET_DB), None);
        let tone = |note: u8, i: usize| {
            (2. * std::f64::consts::PI * hz(note) * i as f64 / RATE).sin() as f32
        };
        let lead = (LEAD_MS / 1000. * RATE) as usize;
        let at = lead + (0.150 * RATE) as usize;
        let x: Vec<f32> = (0..lead + 48000)
            .map(|i| if i < at { tone(60, i) } else { tone(64, i) } * 0.3)
            .collect();
        let t = transition_ms(&x, RATE, 64, ONSET_DB).unwrap();
        assert!((t - 150.).abs() <= 20., "{t}");
    }
    #[test]
    fn v1_exact_stacked_owners_keep_their_own_delays_after_key_reuse() {
        let switching = sampler_core::Switching::default();
        let r = router(&switching);
        let mut s = Scheduler::default();
        let first = HostNote {
            port: 0,
            channel: 0,
            key: 60,
            id: 10,
            clap: true,
        };
        let second = HostNote { id: 11, ..first };
        let first_on = Event::NoteOn {
            note: first,
            velocity: 0.7654321,
            tune: 0.123456789,
        };
        let second_on = Event::NoteOn {
            note: second,
            velocity: 0.87654321,
            tune: 0.234567891,
        };
        assert!(
            s.arrive(first_on, 0, &holds(0., 0., 10.), RATE, &r)
                .is_none()
        );
        assert!(
            s.arrive(second_on, 96, &holds(0., 0., 20.), RATE, &r)
                .is_none()
        );
        let old = HostPattern {
            port: 0,
            channel: 0,
            key: 60,
            id: 10,
            clap: true,
        };
        s.arrive(Event::NoteOff(old), 192, &holds(0., 0., 20.), RATE, &r);
        s.arrive(
            Event::Expression(old, NoteExpression::Tune(12.123456)),
            240,
            &holds(0., 0., 20.),
            RATE,
            &r,
        );
        assert_eq!(
            s.pop_due(480),
            Some((first_on, NO_ART)),
            "full velocity and tuning survive the queue"
        );
        assert_eq!(s.pop_due(672), Some((Event::NoteOff(old), NO_ART)));
        assert_eq!(
            s.pop_due(720),
            Some((
                Event::Expression(old, NoteExpression::Tune(12.123456)),
                NO_ART
            ))
        );
        assert_eq!(s.pop_due(1056), Some((second_on, NO_ART)));
        assert!(s.host.iter().find(|o| o.note == second).unwrap().held);
    }
    #[test]
    fn v1_native_midi2_payload_and_ties_survive_alignment() {
        let switching = sampler_core::Switching::default();
        let r = router(&switching);
        let mut s = Scheduler::default();
        let on = Event::Ump([0x40903c03, 0xcafeabcd]);
        let ctl = Event::Ump([0x40b00100, 0x12345678]);
        s.arrive(on, 0, &holds(0., 0., 10.), RATE, &r);
        s.arrive(ctl, 0, &holds(0., 0., 10.), RATE, &r);
        assert_eq!(s.pop_due(480), Some((on, NO_ART)));
        assert_eq!(s.pop_due(480), Some((ctl, NO_ART)));
    }
    #[test]
    fn timing_uses_stable_source_identity_and_nearest_velocity_bucket() {
        let mut t = Timing {
            arts: vec![
                Delay {
                    name: "Same".into(),
                    identity: "source-a".into(),
                    first: [None, Some(90.), None],
                    ..Default::default()
                },
                Delay {
                    name: "Same".into(),
                    identity: "source-b".into(),
                    first: [Some(20.); 3],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let h = Holds::of(&t, &["source-b", "source-a"], 100.);
        assert_eq!(h.frames(0, false, 48, RATE), 80 * 48);
        assert_eq!(h.frames(1, true, 127, RATE), 10 * 48);
        t.override_ms = Some(50.);
        assert_eq!(t.latest(), 50.);
        assert_eq!(reported_ms([900.]), MAX_MS);
        assert_eq!(hold_ms(100., 900.), 0.);
        let s = source("patch.nki", 2, "snapshot.nksn");
        t.source = s;
        assert!(t.measured("patch.nki", 2, "snapshot.nksn"));
        assert!(!t.measured("patch.nki", 2, "another.nksn"));
    }
}
