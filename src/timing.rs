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

use crate::articulate::{self, In, MAX_ARTICULATIONS, Router};
use crate::engine::{Engine, MAX_BLOCK, RACK_SLOTS, Rack};
use serde::{Deserialize, Serialize};

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
    pub first: [Option<f32>; 3],
    pub legato: [Option<f32>; 3],
}

impl Delay {
    /// The delay of a note: legato or first, at its velocity's bucket, else
    /// the nearest bucket measured, else the other kind.
    pub fn ms(&self, legato: bool, velocity: u8) -> Option<f32> {
        let near = |row: &[Option<f32>; 3]| {
            let b = bucket(velocity) as isize;
            (0..3isize).flat_map(|d| [b - d, b + d]).filter(|i| (0..3).contains(i)).find_map(|i| row[i as usize])
        };
        let (want, other) = if legato { (&self.legato, &self.first) } else { (&self.first, &self.legato) };
        near(want).or_else(|| near(other))
    }

    /// The latest it sounds.
    pub fn max(&self) -> Option<f32> {
        self.first.iter().chain(&self.legato).flatten().copied().reduce(f32::max)
    }

    /// A legato script plays it: at some velocity a legato sounds more than
    /// 15 ms later than a first note.
    pub fn mono(&self) -> bool {
        self.first.iter().zip(&self.legato).any(|(f, l)| matches!((f, l), (Some(f), Some(l)) if l - f > 15.0))
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
    /// What the library's own panel says, ms ([`declared`]): used over
    /// what was measured.
    pub declared: Option<f32>,
    /// Every note of the part is this late (ms), whatever was measured.
    pub override_ms: Option<f32>,
    /// Not aligned: plays as late as the library does.
    pub exclude: bool,
}

/// What a [`Timing`] was measured for.
pub fn source(path: &str, program: u32) -> String {
    format!("{path}#{program}")
}

impl Timing {
    pub fn measured(&self, path: &str, program: u32) -> bool {
        !path.is_empty() && self.source == source(path, program)
    }

    /// What was measured for an articulation, by name (`None`: as loaded).
    fn delay(&self, art: Option<&str>) -> &Delay {
        art.and_then(|name| self.arts.iter().find(|d| d.name == name)).unwrap_or(&self.loaded)
    }

    /// How late a note is, by articulation name (`None`: as loaded).
    fn own(&self, art: Option<&str>, legato: bool, velocity: u8) -> f32 {
        if self.exclude {
            return 0.0;
        }
        if let Some(ms) = self.override_ms.or(self.declared) {
            return ms;
        }
        self.delay(art).ms(legato, velocity).unwrap_or(0.0)
    }

    /// The latest any note of the part sounds, as aligned (0 excluded).
    pub fn latest(&self) -> f32 {
        if self.exclude {
            return 0.0;
        }
        if let Some(ms) = self.override_ms.or(self.declared) {
            return ms.clamp(0.0, MAX_MS);
        }
        (self.arts.iter().chain([&self.loaded]))
            .filter_map(Delay::max)
            .fold(0.0, f32::max)
            .min(MAX_MS)
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
pub const ROWS: usize = MAX_ARTICULATIONS + 1;
const LOADED: usize = ROWS - 1;

/// A part's holds for the audio thread: per articulation row, first note or
/// legato, and velocity bucket, in tenths of a millisecond; and which rows
/// a legato script plays ([`Delay::mono`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Holds([[[u16; 3]; 2]; ROWS], [bool; ROWS]);

impl Default for Holds {
    fn default() -> Self {
        Self([[[0; 3]; 2]; ROWS], [false; ROWS])
    }
}

impl Holds {
    /// Holds for a part with `timing` whose articulation list names `arts`
    /// (in routing order), against `reported` ms.
    pub fn of(timing: &Timing, arts: &[&str], reported: f32) -> Self {
        let mut holds = Self::default();
        for (row, h) in holds.0.iter_mut().enumerate() {
            let art = if row == LOADED { None } else { arts.get(row).copied() };
            if row != LOADED && art.is_none() {
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

    fn frames(&self, row: usize, legato: bool, velocity: u8, rate: f64) -> u64 {
        let tenths = self.0[row.min(LOADED)][usize::from(legato)][bucket(velocity)];
        (f64::from(tenths) * rate / 10_000.0).round() as u64
    }
}

/// Everything the audio thread aligns by, from the loader.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plan {
    /// Auto-align is on.
    pub on: bool,
    /// Hold notes back only while the host's transport plays.
    pub transport_only: bool,
    /// Reported latency, ms.
    pub latency_ms: f32,
    pub parts: [Holds; RACK_SLOTS],
}

impl Default for Plan {
    fn default() -> Self {
        Self { on: false, transport_only: false, latency_ms: 0.0, parts: [Holds::default(); RACK_SLOTS] }
    }
}

impl Plan {
    /// The latency to report, in frames at `rate`.
    pub fn latency(&self, rate: f64) -> u32 {
        if self.on { (f64::from(self.latency_ms) * rate / 1000.0).round() as u32 } else { 0 }
    }
}

/// Events a part may hold back at once; past this they play at once.
const CAPACITY: usize = 2048;
const UP: u32 = u32::MAX;
/// A keyswitch taken over by the scheduler: its release goes too.
const SWITCH: u32 = u32::MAX - 1;
/// Marks a held note a legato script plays.
const MONO: u32 = 1 << 30;
const NO_ART: u8 = u8::MAX;

#[derive(Clone, Copy)]
struct Queued {
    due: u64,
    ev: In,
    /// Articulation to switch to before this note plays, or [`NO_ART`].
    art: u8,
}

/// One part's held-back events, in the order they play.
pub struct Scheduler {
    queue: Box<[Queued]>,
    head: usize,
    len: usize,
    /// Articulation row the next note plays, as the keyswitches say so far.
    art: usize,
    /// Per channel and key: frames its note-on was held back (with
    /// [`MONO`]), [`UP`] when not held, [`SWITCH`] for a keyswitch taken over.
    held: [[u32; 128]; 16],
    down: u32,
    /// The latest note-on's, release's and controller's due frames.
    last_on: u64,
    last_off: u64,
    last_ctl: u64,
    /// Frames the latest note was held back: controllers go with it.
    ctl_hold: u64,
    /// Events that found the queue full and played at once.
    pub overflows: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        let empty = Queued { due: 0, ev: In::Pressure(0, 0), art: NO_ART };
        Self {
            queue: vec![empty; CAPACITY].into_boxed_slice(),
            head: 0,
            len: 0,
            art: LOADED,
            held: [[UP; 128]; 16],
            down: 0,
            last_on: 0,
            last_off: 0,
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
    pub fn arrive(&mut self, ev: In, now: u64, holds: &Holds, rate: f64, router: &Router) -> Option<In> {
        let frames = |row, legato, velocity| holds.frames(row, legato, velocity, rate);
        let (due, art) = match ev {
            In::NoteOn(channel, note, velocity) => {
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
                let picked = row.is_some();
                let row = row.unwrap_or(self.art);
                let hold = frames(row, self.down > 0, velocity);
                // A legato script hears notes and releases in the order played.
                let after = if holds.1[row.min(LOADED)] { self.last_on.max(self.last_off) } else { 0 };
                let due = (now + hold).max(after);
                self.last_on = self.last_on.max(due);
                self.ctl_hold = due - now;
                let key = &mut self.held[usize::from(channel & 15)][usize::from(note & 127)];
                if *key == UP || *key == SWITCH {
                    self.down += 1;
                }
                let mono = if holds.1[row.min(LOADED)] { MONO } else { 0 };
                *key = ((due - now) as u32).min(MONO - 1) | mono;
                let art = if picked || row == LOADED { NO_ART } else { row as u8 };
                (due, art)
            }
            In::NoteOff(channel, note) => {
                let key = &mut self.held[usize::from(channel & 15)][usize::from(note & 127)];
                let (hold, mono) = match std::mem::replace(key, UP) {
                    SWITCH => return None,
                    UP => (self.ctl_hold, false),
                    held => {
                        self.down = self.down.saturating_sub(1);
                        (u64::from(held & !MONO), held & MONO != 0)
                    }
                };
                // Under a legato script a release never passes a note-on
                // that came before it: an overlap stays one.
                let due = if mono { (now + hold).max(self.last_on) } else { now + hold };
                self.last_off = self.last_off.max(due);
                (due, NO_ART)
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
            In::Cc(_, 120.., _) => {
                let latest = if self.len > 0 { self.at(self.len - 1).due } else { 0 };
                let due = (now + self.ctl_hold).max(latest);
                self.last_ctl = due;
                (due, NO_ART)
            }
            In::Cc(..) | In::Bend(..) | In::Pressure(..) => {
                let due = (now + self.ctl_hold).max(self.last_ctl);
                self.last_ctl = due;
                (due, NO_ART)
            }
        };
        (!self.push(Queued { due, ev, art })).then_some(ev)
    }

    /// Play what is due by frame `now` on `slot` of `rack`.
    pub fn release(&mut self, now: u64, rack: &mut Rack, routers: &mut [Router; RACK_SLOTS], slot: usize) {
        while self.len > 0 && self.at(0).due <= now {
            let q = *self.at(0);
            self.head = (self.head + 1) % CAPACITY;
            self.len -= 1;
            play(rack, routers, slot, q);
        }
    }

    /// Forget everything held: the host stopped processing.
    pub fn clear(&mut self) {
        self.len = 0;
        self.head = 0;
        self.held = [[UP; 128]; 16];
        (self.down, self.last_on, self.last_off, self.last_ctl, self.ctl_hold) = (0, 0, 0, 0, 0);
    }
}

fn play(rack: &mut Rack, routers: &mut [Router; RACK_SLOTS], slot: usize, q: Queued) {
    let (e, c, r) = (&mut rack.parts[slot], &rack.controls[slot], &mut routers[slot]);
    if let (In::NoteOn(channel, ..), true) = (q.ev, q.art != NO_ART) {
        r.select(usize::from(q.art), channel, e);
    }
    articulate::feed(r, e, q.ev, u8::try_from(c.channel).unwrap_or(0));
}

/// Every part's scheduler and the plan they follow, on the audio thread.
pub struct Align {
    pub plan: Plan,
    parts: Box<[Scheduler]>,
    /// Frames since processing started.
    pub clock: u64,
}

impl Default for Align {
    fn default() -> Self {
        Self { plan: Plan::default(), parts: (0..RACK_SLOTS).map(|_| Scheduler::default()).collect(), clock: 0 }
    }
}

impl Align {
    /// Whether notes are held back now.
    pub fn holding(&self, playing: bool) -> bool {
        self.plan.on && (playing || !self.plan.transport_only)
    }

    /// Take host input `ev` from `port`, arriving at frame `now`: hold it
    /// back for each part it reaches.
    pub fn arrive(&mut self, rack: &mut Rack, routers: &mut [Router; RACK_SLOTS], port: u8, ev: In, now: u64, rate: f64) {
        let reached = articulate::reach(rack, routers, port, ev);
        for slot in (0..RACK_SLOTS).filter(|s| reached & 1 << s != 0) {
            let holds = &self.plan.parts[slot];
            if let Some(ev) = self.parts[slot].arrive(ev, now, holds, rate, &routers[slot]) {
                articulate::dispatch_to(rack, routers, 1 << slot, ev);
            }
        }
    }

    /// Play everything due by frame `now`.
    pub fn release(&mut self, now: u64, rack: &mut Rack, routers: &mut [Router; RACK_SLOTS]) {
        for (slot, s) in self.parts.iter_mut().enumerate() {
            s.release(now, rack, routers, slot);
        }
    }

    /// A click on the part's articulation list picked `row`: the next notes play it.
    pub fn picked(&mut self, slot: usize, row: Option<usize>) {
        if let (Some(s), Some(row)) = (self.parts.get_mut(slot), row) {
            s.art = row.min(LOADED);
        }
    }

    /// Play everything held, now.
    pub fn flush(&mut self, rack: &mut Rack, routers: &mut [Router; RACK_SLOTS]) {
        self.release(u64::MAX, rack, routers);
        for s in self.parts.iter_mut() {
            s.clear();
        }
    }

    /// The first frame anything held is due.
    pub fn next_due(&self) -> Option<u64> {
        self.parts.iter().filter_map(Scheduler::next_due).min()
    }

    pub fn clear(&mut self) {
        for s in self.parts.iter_mut() {
            s.clear();
        }
        self.clock = 0;
    }

    pub fn overflows(&self) -> u64 {
        self.parts.iter().map(|s| s.overflows).sum()
    }
}

// ---- Measuring ------------------------------------------------------------

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
    let at = p.iter().position(|&v| v >= peak * 10f64.powf(f64::from(db) / 10.0))?;
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
    let db: Vec<f64> = p.iter().map(|v| 10.0 * (v.max(peak * 1e-6)).log10()).collect();
    let (at, _) = (0..db.len().saturating_sub(10).min(1000)).map(|i| (i, db[i + 10] - db[i])).max_by(|a, b| a.1.total_cmp(&b.1))?;
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
    let e: Vec<f64> = (0..=(x.len() - win) / hop).map(|i| partials(&x[i * hop..i * hop + win], rate, hz(new))).collect();
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

/// How a take picks the articulation: a keyswitch, a script control, or
/// nothing (as loaded).
#[derive(Clone, Copy, Debug)]
pub enum Pick {
    Loaded,
    Key(u8),
    Control(usize, usize),
}

/// Frames rendered at a time while measuring: the voice timing's resolution.
const STEP: usize = 16;

fn render(e: &mut Engine, seconds: f64, out: Option<&mut Vec<f32>>, voices_from: Option<(usize, &mut Option<f32>)>) {
    let rate = e.rate();
    let frames = (seconds * rate) as usize;
    let (mut l, mut r) = ([0f32; STEP], [0f32; STEP]);
    let mut out = out;
    let mut voices_from = voices_from;
    let mut done = 0;
    while done < frames {
        let n = STEP.min(frames - done);
        e.render(&mut l[..n], &mut r[..n]);
        if let Some(out) = out.as_deref_mut() {
            out.extend(l[..n].iter().zip(&r[..n]).map(|(l, r)| l + r));
        }
        if let Some((before, at)) = voices_from.as_mut()
            && at.is_none()
            && e.active_voices() > *before
        {
            **at = Some(done as f32 / rate as f32 * 1000.0);
        }
        done += n;
    }
}

/// Let everything sounding go and ring out.
fn quiet(e: &mut Engine) {
    for channel in 0..16 {
        e.cc(channel, 123, 0);
    }
    for _ in 0..40 {
        if e.active_voices() == 0 {
            break;
        }
        render(e, 0.025, None, None);
    }
    let rate = e.rate();
    e.reset(rate);
}

/// Play a take of `pick` at `velocity`: `note`, then a legato to `note + 4`.
pub fn take(e: &mut Engine, pick: Pick, note: u8, velocity: u8) -> Take {
    let rate = e.rate();
    quiet(e);
    // Dynamics and expression as a player leaves them, mostly up.
    e.cc(0, 1, 100);
    e.cc(0, 11, 127);
    match pick {
        Pick::Loaded => {}
        Pick::Key(key) => {
            e.note_on(0, key, 100);
            render(e, 0.01, None, None);
            e.note_off(0, key);
        }
        Pick::Control(slot, control) => e.ui_control(slot, control, 1),
    }
    render(e, 0.05, None, None);
    let second = note.saturating_add(4).min(127);
    let mut first = Vec::new();
    let mut first_voice_ms = None;
    e.note_on(0, note, velocity);
    render(e, 1.0, Some(&mut first), Some((0, &mut first_voice_ms)));
    let lead = (LEAD_MS / 1000.0 * rate) as usize;
    let mut legato = first[first.len().saturating_sub(lead)..].to_vec();
    let mut legato_voice_ms = None;
    let before = e.active_voices();
    e.note_on(0, second, velocity);
    render(e, 0.05, Some(&mut legato), Some((before, &mut legato_voice_ms)));
    e.note_off(0, note);
    render(e, 0.95, Some(&mut legato), None);
    e.note_off(0, second);
    render(e, 0.3, None, None);
    Take { first, legato, first_voice_ms, legato_voice_ms }
}

/// How late `pick` sounds, from its takes at each of [`VELOCITIES`].
pub fn delay(e: &mut Engine, name: &str, pick: Pick, note: u8) -> Delay {
    let rate = e.rate();
    let mut d = Delay { name: name.to_owned(), ..Delay::default() };
    for (b, &velocity) in VELOCITIES.iter().enumerate() {
        let t = take(e, pick, note, velocity);
        d.first[b] = onset_ms(&t.first, rate, ONSET_DB);
        d.legato[b] = transition_ms(&t.legato, rate, note.saturating_add(4).min(127), ONSET_DB);
    }
    d
}

/// Measure a part: `e` holds its bank and a fresh copy of its scripts
/// (running notes through them changes their state); `arts` is its
/// articulation list (name, keyswitch, script control).
pub fn measure(e: &mut Engine, arts: &[articulate::Found], note: u8) -> (Delay, Vec<Delay>) {
    let loaded = delay(e, "", Pick::Loaded, note);
    let arts = arts
        .iter()
        .map(|(name, key, control)| {
            let pick = match (key, control) {
                (Some(key), _) => Pick::Key(*key),
                (None, Some((slot, control))) => Pick::Control(usize::from(*slot), usize::from(*control)),
                _ => Pick::Loaded,
            };
            delay(e, name, pick, note)
        })
        .collect();
    (loaded, arts)
}

/// A note every articulation plays: above the keyswitches, the key with
/// the most zones (all its groups and layers), nearest the middle of those.
pub fn probe_note(i: &crate::import::Instrument, arts: &[articulate::Found]) -> u8 {
    let lowest = arts.iter().filter_map(|f| f.1).max().map_or(0, |k| k.saturating_add(1));
    let mut count = [0usize; 124];
    for z in i.zones.iter().filter(|z| z.available) {
        for k in z.low_key.max(lowest)..=z.high_key.min(123) {
            count[usize::from(k)] += 1;
        }
    }
    let most = count.iter().copied().max().unwrap_or(0);
    let keys: Vec<u8> = (0..124u8).filter(|&k| most > 0 && count[usize::from(k)] * 10 >= most * 9).collect();
    keys.get(keys.len() / 2).copied().unwrap_or(60)
}

const _: () = assert!(MAX_BLOCK >= STEP);

/// The delay a library states on its own panel, ms: a value field
/// labelled as a sample or playback offset holding a negative number of ms
/// (Performance Samples' "Sample Offset", "PLBK Offset"). One such field,
/// or one naming samples or playback among several; else none.
pub fn declared(rt: Option<&crate::ksp::Runtime>) -> Option<f32> {
    let interface = rt?.live().interface?;
    let text = |c: &crate::ksp::Control, p: &str| match c.properties.get(p) {
        Some(crate::ksp::Value::Text(t)) => t.to_lowercase(),
        _ => String::new(),
    };
    let found: Vec<(String, i32)> = (interface.controls.iter())
        .filter(|c| c.kind == "ui_value_edit" && text(c, "$CONTROL_PAR_TEXT").contains("offset"))
        .filter_map(|c| match c.properties.get("$CONTROL_PAR_VALUE") {
            Some(crate::ksp::Value::Int(v)) if (-500..0).contains(v) => Some((text(c, "$CONTROL_PAR_TEXT"), *v)),
            _ => None,
        })
        .collect();
    let named: Vec<_> = found.iter().filter(|(l, _)| ["sample", "plbk", "playback"].iter().any(|k| l.contains(k))).collect();
    match (found.as_slice(), named.as_slice()) {
        ([(_, v)], _) | (_, [(_, v)]) => Some(-*v as f32),
        _ => None,
    }
}

/// An instrument's articulation list as its performance view shows it,
/// from its initialized scripts in `e`.
#[cfg(feature = "plugin")]
pub fn found(i: &crate::import::Instrument, e: &Engine) -> Vec<articulate::Found> {
    let view = crate::plugin::script_interface(e.script());
    let sections = view.interface.as_deref().map_or_else(Vec::new, |interface| {
        let names = interface.controls.iter().filter_map(|c| match c.properties.get("$CONTROL_PAR_PICTURE") {
            Some(crate::ksp::Value::Text(n)) => Some(n.as_str()),
            _ => None,
        });
        crate::ui::sections(interface, &crate::artwork::pictures(&i.path, names))
    });
    crate::ui::articulations(&sections, view.slot, &view.keys)
}

/// A fresh engine playing `i` as the plugin loads it, streaming and
/// waiting for the disk, at `rate`.
pub fn engine_for(i: &crate::import::Instrument, rate: f64, budget: usize) -> anyhow::Result<Engine> {
    let (script, _) = crate::engine::load_scripts(i, i.script_state.clone(), rate);
    let controllers = script.as_deref().map_or(Vec::new(), |rt| rt.init_controllers.clone());
    let bank = crate::engine::Bank::load_counting(i, budget, Default::default(), &controllers, &Default::default())?;
    let mut e = Engine::default();
    e.reset(rate);
    e.blocking_streams = true;
    e.set_bank(Some(Box::new(bank)));
    e.set_script(script);
    Ok(e)
}

/// The research behind `audits/LATENCY.md` for one instrument, as JSON:
/// its script controls named like a delay, its zones' sample starts, and
/// per articulation and velocity the first note's onset at three
/// thresholds, the legato transition, and when voices started. Names and
/// numbers only.
#[cfg(feature = "plugin")]
pub fn audit(path: &std::path::Path) -> anyhow::Result<serde_json::Value> {
    use serde_json::json;
    const RATE: f64 = 48_000.0;
    let i = crate::import::read(path)?;
    let started = std::time::Instant::now();
    let mut e = engine_for(&i, RATE, crate::engine::MEMORY_LIMIT)?;
    let load_ms = started.elapsed().as_millis();
    render(&mut e, 0.5, None, None);
    // Script controls that look like a delay or sample start setting.
    let named = |s: &str| {
        let s = s.to_lowercase();
        ["delay", "latency", "offset", "start", "tight", "pre-roll", "preroll", "look"].iter().any(|k| s.contains(k))
    };
    let view = crate::plugin::script_interface(e.script());
    let text = |v: Option<&crate::ksp::Value>| match v {
        Some(crate::ksp::Value::Text(t)) => t.clone(),
        Some(crate::ksp::Value::Int(n)) => n.to_string(),
        _ => String::new(),
    };
    let controls: Vec<_> = (view.interface.iter().flat_map(|u| &u.controls))
        .filter(|c| named(&c.variable) || named(&text(c.properties.get("$CONTROL_PAR_TEXT"))))
        .map(|c| {
            json!({
                "kind": c.kind,
                "label": text(c.properties.get("$CONTROL_PAR_TEXT")),
                "shows": text(c.properties.get("$CONTROL_PAR_LABEL")),
                "value": text(c.properties.get("$CONTROL_PAR_VALUE")),
                "matched_by": if named(&text(c.properties.get("$CONTROL_PAR_TEXT"))) { "label" } else { "variable name" },
            })
        })
        .collect();
    let zones: Vec<_> = i.zones.iter().filter(|z| z.available).collect();
    let starts: Vec<f64> = zones.iter().map(|z| z.start as f64).collect();
    let mods: Vec<f64> = zones.iter().filter_map(|z| z.start_mod).map(f64::from).collect();
    let stats = |v: &[f64]| {
        if v.is_empty() {
            return json!(null);
        }
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        json!({"zones": v.len(), "min_frames": v[0], "median_frames": v[v.len() / 2], "max_frames": v[v.len() - 1]})
    };
    let arts = found(&i, &e);
    let note = probe_note(&i, &arts);
    let round = |v: Option<f32>| v.map(|v| (v * 10.0).round() / 10.0);
    let mut rows = Vec::new();
    let picks = std::iter::once(("(as loaded)".to_owned(), Pick::Loaded)).chain(arts.iter().map(|(name, key, control)| {
        let pick = match (key, control) {
            (Some(key), _) => Pick::Key(*key),
            (None, Some((slot, control))) => Pick::Control(usize::from(*slot), usize::from(*control)),
            _ => Pick::Loaded,
        };
        (name.clone(), pick)
    }));
    for (name, pick) in picks {
        let mut per = Vec::new();
        for &velocity in &VELOCITIES {
            let t = take(&mut e, pick, note, velocity);
            let second = note.saturating_add(4).min(127);
            per.push(json!({
                "velocity": velocity,
                "first_voice_ms": round(t.first_voice_ms),
                "onset_30": round(onset_ms(&t.first, RATE, -30.0)),
                "onset_20": round(onset_ms(&t.first, RATE, -20.0)),
                "onset_12": round(onset_ms(&t.first, RATE, -12.0)),
                "onset_6": round(onset_ms(&t.first, RATE, -6.0)),
                "steepest": round(steepest_ms(&t.first, RATE)),
                "legato_voice_ms": round(t.legato_voice_ms),
                "legato_20": round(transition_ms(&t.legato, RATE, second, -20.0)),
                "legato_12": round(transition_ms(&t.legato, RATE, second, -12.0)),
            }));
        }
        rows.push(json!({"articulation": name, "takes": per}));
    }
    Ok(json!({
        "instrument": i.name,
        "load_ms": load_ms,
        "measure_ms": started.elapsed().as_millis() - load_ms,
        "scripts": i.scripts.len(),
        "note": note,
        "delay_controls": controls,
        "sample_start": stats(&starts),
        "sample_start_mod": stats(&mods),
        "articulations": rows,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    #[test]
    fn alignment_math() {
        // Two parts, 80 and 120 ms late: report 120, hold the first 40.
        let latest = [80.0, 120.0];
        let reported = reported_ms(latest);
        assert_eq!(reported, 120.0);
        assert_eq!(hold_ms(reported, 80.0), 40.0);
        assert_eq!(hold_ms(reported, 120.0), 0.0);
        // Excluded parts play as late as the library does: held the whole latency.
        let excluded = Timing { exclude: true, ..Timing::default() };
        assert_eq!(hold_ms(reported, excluded.latest()), 120.0);
        // Bounded.
        assert_eq!(reported_ms([900.0]), MAX_MS);
        assert_eq!(hold_ms(100.0, 900.0), 0.0);
        // An override wins over what was measured.
        let d = Delay { name: "Legato".into(), first: [Some(60.0); 3], legato: [Some(330.0), Some(250.0), Some(100.0)] };
        let mut t = Timing { loaded: d.clone(), arts: vec![d], ..Timing::default() };
        assert_eq!(t.latest(), 330.0);
        // Per articulation, legato speed by velocity.
        let h = Holds::of(&t, &["Legato"], 330.0);
        assert_eq!(h.0[0][0], [2700; 3]);
        assert_eq!(h.0[0][1], [0, 800, 2300]);
        t.override_ms = Some(50.0);
        assert_eq!(t.latest(), 50.0);
        assert_eq!(Holds::of(&t, &[], 50.0).0[LOADED], [[0; 3]; 2]);
        // A missing bucket takes the nearest; a missing kind the other.
        let partial = Delay { first: [None, Some(90.0), None], ..Delay::default() };
        assert_eq!(partial.ms(false, 10), Some(90.0));
        assert_eq!(partial.ms(true, 127), Some(90.0));
    }

    #[test]
    fn onsets() {
        // 100 ms of silence, then a tone: the onset is at 100 ms, give or take the window.
        let mut x = vec![0f32; 4800];
        x.extend((0..24000).map(|i| (i as f32 * 0.05).sin() * 0.5));
        let at = onset_ms(&x, RATE, ONSET_DB).unwrap();
        assert!((at - 100.0).abs() <= 3.0, "{at}");
        assert_eq!(onset_ms(&[0.0; 1000], RATE, ONSET_DB), None);
        // A legato: C4 alone, then E4 takes over 150 ms after the second note-on.
        let tone = |note: u8, i: usize| (2.0 * std::f64::consts::PI * hz(note) * i as f64 / RATE).sin() as f32;
        let lead = (LEAD_MS / 1000.0 * RATE) as usize;
        let at = lead + (0.150 * RATE) as usize;
        let x: Vec<f32> = (0..lead + 48000).map(|i| if i < at { tone(60, i) } else { tone(64, i) } * 0.3).collect();
        let t = transition_ms(&x, RATE, 64, ONSET_DB).unwrap();
        assert!((t - 150.0).abs() <= 20.0, "{t}");
    }

    fn holds(first: f32, legato: f32, reported: f32) -> Holds {
        let d = Delay { first: [Some(first); 3], legato: [Some(legato); 3], ..Delay::default() };
        Holds::of(&Timing { loaded: d, ..Timing::default() }, &[], reported)
    }

    fn drain(s: &mut Scheduler) -> Vec<(u64, In)> {
        let mut out = Vec::new();
        while s.len > 0 {
            let q = *s.at(0);
            s.head = (s.head + 1) % CAPACITY;
            s.len -= 1;
            out.push((q.due / 48, q.ev));
        }
        out
    }

    #[test]
    fn notes_wait_by_their_own_delay() {
        let r = Router::default();
        // A legato script: first notes 50 ms late, legatos 250 ms, 250 ms
        // reported. First notes wait 200 ms, legatos not at all; releases,
        // controllers and expressions go with their note.
        let h = holds(50.0, 250.0, 250.0);
        let mut s = Scheduler::default();
        let mut at = |ms: u64, ev| assert_eq!(s.arrive(ev, ms * 48, &h, RATE, &r), None);
        at(0, In::NoteOn(0, 60, 100));
        at(10, In::NoteTune(0, 60, 0.5));
        at(500, In::NoteOn(0, 62, 100));
        at(520, In::NoteOff(0, 60));
        at(530, In::Cc(0, 1, 64));
        at(900, In::NoteOff(0, 62));
        at(1000, In::NoteOn(0, 64, 100));
        at(1100, In::NoteOff(0, 64));
        assert_eq!(
            drain(&mut s),
            [
                (200, In::NoteOn(0, 60, 100)),
                (210, In::NoteTune(0, 60, 0.5)),
                (500, In::NoteOn(0, 62, 100)),
                (530, In::Cc(0, 1, 64)),
                (720, In::NoteOff(0, 60)),
                (900, In::NoteOff(0, 62)),
                (1200, In::NoteOn(0, 64, 100)),
                (1300, In::NoteOff(0, 64)),
            ]
        );
    }

    #[test]
    fn a_legato_script_hears_the_notes_in_order() {
        let r = Router::default();
        let h = holds(50.0, 250.0, 250.0);
        // A transition only 100 ms after its note would be due before it:
        // it waits for it.
        let mut s = Scheduler::default();
        s.arrive(In::NoteOn(0, 60, 100), 0, &h, RATE, &r);
        s.arrive(In::NoteOn(0, 62, 100), 4800, &h, RATE, &r);
        assert_eq!(drain(&mut s), [(200, In::NoteOn(0, 60, 100)), (200, In::NoteOn(0, 62, 100))]);
        // A detached note stays detached: a note due sooner than the release
        // before it waits for that release.
        let mut s = Scheduler::default();
        s.art = 0;
        let mut t = Timing::default();
        t.loaded = Delay { first: [Some(200.0); 3], legato: [Some(250.0); 3], ..Delay::default() };
        t.arts = vec![Delay { name: "Short".into(), first: [Some(20.0); 3], legato: [Some(20.0); 3] }];
        let h = Holds::of(&t, &["Short"], 250.0);
        s.arrive(In::NoteOn(0, 60, 100), 0, &h, RATE, &r);
        s.arrive(In::NoteOff(0, 60), 100 * 48, &h, RATE, &r);
        s.art = LOADED;
        s.arrive(In::NoteOn(0, 62, 100), 110 * 48, &h, RATE, &r);
        assert_eq!(
            drain(&mut s),
            [(230, In::NoteOn(0, 60, 100)), (330, In::NoteOff(0, 60)), (330, In::NoteOn(0, 62, 100))]
        );
        // Polyphonic articulations keep no such order: each note lands on time.
        let h = holds(20.0, 20.0, 250.0);
        let mut s = Scheduler::default();
        s.arrive(In::NoteOn(0, 60, 100), 0, &h, RATE, &r);
        s.arrive(In::NoteOn(0, 64, 100), 0, &h, RATE, &r);
        s.arrive(In::NoteOff(0, 60), 100 * 48, &h, RATE, &r);
        assert_eq!(
            drain(&mut s),
            [(230, In::NoteOn(0, 60, 100)), (230, In::NoteOn(0, 64, 100)), (330, In::NoteOff(0, 60))]
        );
    }
}
