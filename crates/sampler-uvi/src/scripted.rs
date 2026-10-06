//! Runs a program's Lua scripts against the core runtime.
//!
//! The physical note is admitted but silent (`Runtime::note_on` selects no
//! source). Each `playNote` the script makes becomes a core child of it whose
//! group selection allows only the (layer, oscillator) groups named, then its
//! attack is forwarded: the same ownership path the other frontends use, with
//! no parallel voice mechanism. The script host only runs when a note event or
//! a `wait` is due, so an idle program costs nothing per block.
mod thread;
use crate::script::{Command, Play, ScriptHost};
use crate::OscGroup;
use sampler_core::{
    Error, Expression, Frame, Inheritance, Input, Limits, NoteId, Prepared, Protocol, Runtime,
};
use std::collections::HashMap;
pub use thread::{Loaded, ScriptThread};

/// What the driver needs of a script runtime: a [`ScriptHost`] run inline (offline,
/// deterministic), or a [`ScriptThread`] that keeps Lua off the audio thread.
pub trait Script {
    fn handles_notes(&self) -> bool;
    fn set_time(&mut self, ms: f64);
    fn note_on(&mut self, id: u64, key: u8, velocity: u8);
    fn note_off(&mut self, id: u64, key: u8);
    fn advance(&mut self, ms: f64);
    fn next_due(&mut self) -> Option<f64>;
    /// Append the commands issued since the last call to `out`.
    fn drain(&mut self, out: &mut Vec<Command>);
    /// The audio clock at the start of a block, in milliseconds.
    fn tick(&mut self, _now_ms: f64) {}
}

impl Script for ScriptHost {
    fn handles_notes(&self) -> bool {
        ScriptHost::handles_notes(self)
    }
    fn set_time(&mut self, ms: f64) {
        ScriptHost::set_time(self, ms)
    }
    fn note_on(&mut self, id: u64, key: u8, velocity: u8) {
        ScriptHost::note_on(self, id, key, velocity, 0)
    }
    fn note_off(&mut self, id: u64, key: u8) {
        ScriptHost::note_off(self, id, key, 64, 0)
    }
    fn advance(&mut self, ms: f64) {
        ScriptHost::advance(self, ms)
    }
    fn next_due(&mut self) -> Option<f64> {
        ScriptHost::next_due(self)
    }
    fn drain(&mut self, out: &mut Vec<Command>) {
        out.extend(ScriptHost::take_commands(self));
    }
}

/// A translated program whose scripts are loaded.
pub struct Program {
    pub instrument: sampler_ir::Instrument,
    pub plan: Prepared,
    pub host: ScriptHost,
    pub groups: Vec<OscGroup>,
}

/// Plays made once their originating note was released sound at most this long.
const DETACHED_MS: f64 = 5000.0;
/// Gliding script modulations are stepped this often.
const GLIDE_STEP_MS: f64 = 5.0;
/// Script wake-ups handled at one instant before time is forced forward.
const SPIN: usize = 64;

/// A `sendScriptModulation` gliding to its value.
struct Glide {
    id: u16,
    voice: Option<u64>,
    from: f64,
    to: f64,
    start_ms: f64,
    ms: f64,
}

pub struct Driver<S: Script> {
    host: S,
    groups: Vec<OscGroup>,
    rate: f64,
    /// Script event / voice id -> core note.
    notes: HashMap<u64, NoteId>,
    /// Sounding physical notes by key (script event id).
    held: HashMap<u8, u64>,
    next: u64,
    /// Plays that asked for something the runtime does not model.
    unmodeled: Vec<&'static str>,
    /// Script event modulation values: for all voices (also given to later
    /// ones) and per voice.
    global: HashMap<u16, f64>,
    voice_values: HashMap<(u64, u16), f64>,
    glides: Vec<Glide>,
    /// Frame of the next glide step.
    glide_at: u64,
    /// Commands being applied, and ids of notes found ended: kept so the
    /// audio thread allocates nothing once warm.
    inbox: Vec<Command>,
    ended: Vec<u64>,
    /// Notes played since the host last looked, when it asked to hear of them.
    adopt: bool,
    spawned: Vec<Spawn>,
    /// (physical note, note played from it), for per-note expression.
    family: Vec<(NoteId, NoteId)>,
}

/// A note a script played, for a host that carries bend and MPE to it.
#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    pub note: NoteId,
    /// The physical note it was played from, if any.
    pub parent: Option<NoteId>,
    /// Its script `tune` in semitones, which the host's gestures must keep.
    pub tune: f64,
}

/// Notes and values the driver tracks at once; beyond it, plays are dropped.
const TRACKED: usize = 1024;

impl<S: Script> Driver<S> {
    /// The driver of `host`; the plan it plays on is the caller's runtime.
    pub fn new(host: S, groups: Vec<OscGroup>, rate: u32) -> Self {
        Self {
            host,
            groups,
            rate: f64::from(rate),
            notes: HashMap::with_capacity(TRACKED),
            held: HashMap::with_capacity(128),
            next: 1,
            unmodeled: Vec::with_capacity(32),
            global: HashMap::with_capacity(32),
            voice_values: HashMap::with_capacity(TRACKED * 2),
            glides: Vec::with_capacity(64),
            glide_at: 0,
            inbox: Vec::with_capacity(256),
            ended: Vec::with_capacity(TRACKED),
            adopt: false,
            spawned: Vec::with_capacity(256),
            family: Vec::with_capacity(TRACKED),
        }
    }

    /// Record the notes scripts play for [`Self::drain_spawns`].
    pub fn track_spawns(&mut self) {
        self.adopt = true;
    }

    /// The notes played since the last call.
    pub fn drain_spawns(&mut self, mut each: impl FnMut(Spawn)) {
        self.spawned.drain(..).for_each(&mut each);
    }

    /// The notes played from `parent` that still may sound.
    pub fn family(&self, parent: NoteId, mut each: impl FnMut(NoteId)) {
        for (p, child) in &self.family {
            if *p == parent {
                each(*child);
            }
        }
    }

    /// Whether the scripts take over note selection.
    pub fn handles_notes(&self) -> bool {
        self.host.handles_notes()
    }

    /// What plays asked for that was ignored, once each.
    pub fn unmodeled(&self) -> &[&'static str] {
        &self.unmodeled
    }

    fn now_ms(&self, rt: &Runtime) -> f64 {
        rt.now() as f64 * 1000.0 / self.rate
    }

    fn frames(&self, ms: f64) -> u64 {
        (ms * self.rate / 1000.0).ceil().max(0.0) as u64
    }

    /// The physical note `note` (admitted by the caller with
    /// `Runtime::note_on`, so silent) went down.
    pub fn note_on(
        &mut self,
        rt: &mut Runtime,
        note: NoteId,
        key: u8,
        velocity: f64,
    ) -> Result<(), Error> {
        let id = self.next;
        self.next += 1;
        self.notes.insert(id, note);
        self.held.insert(key, id);
        self.host.set_time(self.now_ms(rt));
        self.host.note_on(id, key, (velocity * 127.0).round() as u8);
        if !self.host.handles_notes() {
            // No onNote: the program sounds as authored.
            rt.forward_attack(note)?;
        }
        self.apply(rt, false)
    }

    pub fn note_off(&mut self, rt: &mut Runtime, key: u8) -> Result<(), Error> {
        let Some(id) = self.held.remove(&key) else {
            return Ok(());
        };
        self.host.set_time(self.now_ms(rt));
        self.host.note_off(id, key);
        // Plays made by onRelease must not be linked to the closing gate.
        self.apply(rt, true)?;
        if let Some(note) = self.notes.get(&id).copied() {
            rt.key_up(note, None)?;
        }
        Ok(())
    }

    /// Run what is due now (script wake-ups, glide steps); the frames until
    /// the next one, `None` when nothing is pending. Call before each block.
    pub fn wake(&mut self, rt: &mut Runtime) -> Result<Option<usize>, Error> {
        let mut spins = 0;
        self.host.tick(self.now_ms(rt));
        // Commands a script thread issued since the last block.
        self.apply(rt, false)?;
        loop {
            if !self.glides.is_empty() && rt.now() >= self.glide_at {
                self.step_glides(rt);
            }
            let glide = (!self.glides.is_empty()).then(|| self.glide_at - rt.now());
            let host = self
                .host
                .next_due()
                .map(|ms| self.frames(ms).saturating_sub(rt.now()));
            if host == Some(0) && spins < SPIN {
                spins += 1;
                self.host.advance(self.now_ms(rt));
                self.apply(rt, false)?;
                continue;
            }
            return Ok(match (host, glide) {
                (Some(a), Some(b)) => Some(a.min(b) as usize),
                (a, b) => a.or(b).map(|d| d as usize),
            });
        }
    }

    fn apply(&mut self, rt: &mut Runtime, closing: bool) -> Result<(), Error> {
        let mut inbox = std::mem::take(&mut self.inbox);
        self.host.drain(&mut inbox);
        let result = self.apply_all(rt, closing, &mut inbox);
        inbox.clear();
        self.inbox = inbox;
        result
    }

    fn apply_all(
        &mut self,
        rt: &mut Runtime,
        closing: bool,
        inbox: &mut Vec<Command>,
    ) -> Result<(), Error> {
        if !inbox.is_empty() && (self.notes.len() >= TRACKED / 2 || self.family.len() >= TRACKED / 2) {
            self.prune(rt);
        }
        for command in inbox.drain(..) {
            match command {
                Command::Play(play) => self.play(rt, &play, closing)?,
                Command::Release { id, at_ms } => {
                    if let Some(note) = self.notes.get(&id).copied() {
                        self.release(rt, note, at_ms)?;
                    }
                }
                Command::Modulation {
                    id,
                    value,
                    glide_ms,
                    voice,
                    at_ms,
                } => self.modulate(rt, id, value, glide_ms, voice, at_ms)?,
            }
        }
        Ok(())
    }

    /// Forget notes the runtime no longer holds.
    fn prune(&mut self, rt: &Runtime) {
        self.notes.retain(|_, note| rt.note(*note).is_ok());
        self.family.retain(|(_, child)| rt.note(*child).is_ok());
        let notes = &self.notes;
        self.voice_values.retain(|(v, _), _| notes.contains_key(v));
    }

    fn modulate(
        &mut self,
        rt: &mut Runtime,
        id: u16,
        value: f64,
        glide_ms: f64,
        voice: Option<u64>,
        at_ms: f64,
    ) -> Result<(), Error> {
        self.glides.retain(|g| !(g.id == id && g.voice == voice));
        if glide_ms <= 0.0 {
            return self.set_value(rt, id, voice, value);
        }
        let from = match voice {
            Some(v) => self.voice_values.get(&(v, id)),
            None => self.global.get(&id),
        }
        .copied()
        .unwrap_or(0.0);
        if self.glides.is_empty() {
            self.glide_at = rt.now();
        }
        self.glides.push(Glide {
            id,
            voice,
            from,
            to: value,
            start_ms: at_ms,
            ms: glide_ms,
        });
        Ok(())
    }

    fn step_glides(&mut self, rt: &mut Runtime) {
        let now = self.now_ms(rt);
        let mut i = 0;
        while i < self.glides.len() {
            let g = &self.glides[i];
            let (id, voice) = (g.id, g.voice);
            let t = ((now - g.start_ms) / g.ms).clamp(0.0, 1.0);
            let value = g.from + (g.to - g.from) * t;
            let _ = self.set_value(rt, id, voice, value);
            if t < 1.0 {
                i += 1;
            } else {
                self.glides.swap_remove(i);
            }
        }
        self.glide_at = rt.now() + self.frames(GLIDE_STEP_MS).max(1);
    }

    /// Set Script Event Modulation `id` now. Voices that ended are forgotten.
    fn set_value(
        &mut self,
        rt: &mut Runtime,
        id: u16,
        voice: Option<u64>,
        value: f64,
    ) -> Result<(), Error> {
        self.ended.clear();
        match voice {
            Some(v) => {
                self.voice_values.insert((v, id), value);
                if let Some(note) = self.notes.get(&v).copied() {
                    match rt.set_note_script_value(note, id, value) {
                        Err(Error::StaleHandle) => self.ended.push(v),
                        other => other?,
                    }
                }
            }
            None => {
                self.global.insert(id, value);
                for (v, note) in &self.notes {
                    match rt.set_note_script_value(*note, id, value) {
                        Err(Error::StaleHandle) => self.ended.push(*v),
                        other => other?,
                    }
                }
            }
        }
        for v in self.ended.drain(..) {
            self.notes.remove(&v);
        }
        Ok(())
    }

    fn release(&mut self, rt: &mut Runtime, note: NoteId, at_ms: f64) -> Result<(), Error> {
        let at = self.frames(at_ms);
        if at <= rt.now() {
            rt.key_up(note, None)
        } else {
            rt.release_at(note, at)
        }
    }

    fn play(&mut self, rt: &mut Runtime, play: &Play, closing: bool) -> Result<(), Error> {
        if self.notes.len() >= TRACKED {
            return Ok(());
        }
        let velocity = f64::from(play.velocity) / 127.0;
        let parent = play.parent.and_then(|p| self.notes.get(&p).copied());
        let open = parent.is_some() && !closing && play.duration_ms.is_none();
        let note = match parent {
            Some(parent) => rt.child(parent, play.key, velocity, open, Inheritance::Independent),
            None => {
                let input = Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: play.key,
                    external_id: None,
                };
                rt.note_on(input, play.key, velocity)
            }
        };
        let note = match note {
            Ok(note) => note,
            // The originating note ended meanwhile: nothing to attach to.
            Err(Error::ClosedNote | Error::StaleHandle) => return Ok(()),
            Err(e) => return Err(e),
        };
        self.notes.insert(play.id, note);
        if self.adopt && self.spawned.len() < self.spawned.capacity() {
            self.spawned.push(Spawn { note, parent, tune: play.tune });
            if let (Some(parent), true) = (parent, self.family.len() < TRACKED) {
                self.family.push((parent, note));
            }
        }
        self.select(rt, note, play)?;
        let expression = Expression {
            gain: play.vol.clamp(0.0, 4.0),
            pan: play.pan.clamp(-1.0, 1.0),
            pitch_semitones: play.tune,
            ..Expression::default()
        };
        if expression != Expression::default() {
            let id = rt.expression_id(note)?;
            rt.set_expression(id, expression)?;
        }
        for (id, value) in &self.global {
            rt.set_note_script_value(note, *id, *value)?;
        }
        rt.forward_attack(note)?;
        let now = self.now_ms(rt);
        match play.duration_ms {
            Some(ms) if ms > 0.0 => self.release(rt, note, now + ms)?,
            Some(_) => {}
            None if parent.is_none() || closing => self.release(rt, note, now + DETACHED_MS)?,
            None => {}
        }
        Ok(())
    }

    /// Allow only the groups of the layers and oscillator the play names.
    fn select(&mut self, rt: &mut Runtime, note: NoteId, play: &Play) -> Result<(), Error> {
        if play.layers.is_empty() && play.osc.is_none() {
            return Ok(());
        }
        rt.set_note_group(note, None, false)?;
        for g in &self.groups {
            if (play.layers.is_empty() || play.layers.contains(g.layer))
                && play.osc.is_none_or(|o| o == g.osc)
            {
                rt.set_note_group(note, Some(g.group), true)?;
            }
        }
        Ok(())
    }
}

/// A runtime and the driver of its scripts, for hosts that own nothing else.
pub struct Player {
    rt: Runtime,
    driver: Driver<ScriptHost>,
}

impl Player {
    pub fn new(program: Program, limits: Limits, rate: u32) -> Result<Self, Error> {
        Ok(Self {
            rt: Runtime::new(program.plan, limits)?,
            driver: Driver::new(program.host, program.groups, rate),
        })
    }

    pub fn runtime(&self) -> &Runtime {
        &self.rt
    }

    /// What plays asked for that was ignored, once each.
    pub fn unmodeled(&self) -> &[&'static str] {
        self.driver.unmodeled()
    }

    pub fn note_on(&mut self, key: u8, velocity: f64) -> Result<(), Error> {
        let input = Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key,
            external_id: None,
        };
        let note = self.rt.note_on(input, key, velocity)?;
        self.driver.note_on(&mut self.rt, note, key, velocity)
    }

    pub fn note_off(&mut self, key: u8) -> Result<(), Error> {
        self.driver.note_off(&mut self.rt, key)
    }

    /// Render `out`, waking the script exactly when it asked to run.
    pub fn render(&mut self, out: &mut [Frame]) -> Result<(), Error> {
        let mut done = 0;
        while done < out.len() {
            let left = out.len() - done;
            let due = self.driver.wake(&mut self.rt)?;
            let step = due.map_or(left, |d| d.max(1).min(left));
            self.rt.render(&mut out[done..done + step])?;
            done += step;
        }
        Ok(())
    }
}
