//! Runs a program's Lua scripts against the core runtime.
//!
//! The physical note is admitted but silent (`Runtime::note_on` selects no
//! source). Each `playNote` the script makes becomes a core child of it whose
//! group selection allows only the (layer, oscillator) groups named, then its
//! attack is forwarded: the same ownership path the other frontends use, with
//! no parallel voice mechanism. The script host only runs when a note event or
//! a `wait` is due, so an idle program costs nothing per block.
#[cfg(test)]
mod lifecycle_tests;
mod thread;
use crate::OscGroup;
use crate::script::{Change, Command, MidiOut, Param, Play, Scope, ScriptHost};
use sampler_core::{
    Error, Expression, Frame, Inheritance, Input, Limits, ModTarget, NoteId, Prepared, Protocol,
    Runtime, Stealing,
};
use std::collections::HashMap;
pub use thread::{Loaded, ScriptThread, UiBridge};

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
    /// A host message other than a note.
    fn input(&mut self, _input: HostInput) {}
}

/// What the host tells a script besides notes (channels are 0-based).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostInput {
    Controller {
        cc: u8,
        value: u8,
        channel: u8,
    },
    /// -1..=1.
    Bend {
        value: f64,
        channel: u8,
    },
    Touch {
        value: u8,
        channel: u8,
    },
    PolyTouch {
        key: u8,
        value: u8,
        channel: u8,
    },
    Program {
        value: u8,
        channel: u8,
    },
    Transport(bool),
    Tempo(f64),
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
    fn input(&mut self, input: HostInput) {
        match input {
            HostInput::Controller { cc, value, channel } => self.controller(cc, value, channel),
            HostInput::Bend { value, channel } => self.pitch_bend(value, channel),
            HostInput::Touch { value, channel } => self.after_touch(value, channel),
            HostInput::PolyTouch {
                key,
                value,
                channel,
            } => self.poly_after_touch(key, value, channel),
            HostInput::Program { value, channel } => self.program_change(value, channel),
            HostInput::Transport(playing) => self.transport(playing),
            HostInput::Tempo(bpm) => self.set_tempo(bpm),
        }
    }
}

/// A translated program whose scripts are loaded.
pub struct Program {
    pub instrument: sampler_ir::Instrument,
    pub plan: Prepared,
    pub host: ScriptHost,
    pub groups: Vec<OscGroup>,
    /// Where each insert element sits in the IR chains.
    pub inserts: Vec<crate::InsertNode>,
    /// Present when the samples stream.
    pub stream: Option<Stream>,
}

/// A streamed program's page cache, and what must outlive it.
pub struct Stream {
    /// Handed to the runtime that plays the program.
    pub cache: Option<sampler_core::StreamCache>,
    /// Frames each start keeps resident.
    pub horizon: usize,
    pub _keep: (sampler_kontakt::Streamer, Vec<sampler_core::Pcm>),
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

/// Plays what the scripts generate (`controlChange`, `postEvent`...) back into
/// the runtime on channel 0, as a host does with a part's script output.
pub struct MidiFeed(sampler_midi::Ingress);

impl Default for MidiFeed {
    fn default() -> Self {
        let mut groups = [None; 16];
        groups[0] = Some(sampler_midi::Version::Midi1);
        Self(sampler_midi::Ingress::new(0, groups))
    }
}

impl MidiFeed {
    pub fn pump<S: Script>(&mut self, driver: &mut Driver<S>, rt: &mut Runtime) {
        let ingress = &mut self.0;
        driver.drain_midi(|out| {
            let word = [0x2000_0000
                | u32::from(out.status) << 16
                | u32::from(out.a & 127) << 8
                | u32::from(out.b & 127)];
            if let Some(Ok(packet)) = sampler_midi::Packets::new(&word).next() {
                let _ = ingress.apply(rt, packet);
            }
        });
    }
}

pub struct Driver<S: Script> {
    host: S,
    groups: Vec<OscGroup>,
    rate: f64,
    /// Script event / voice id -> core note.
    notes: HashMap<u64, NoteId>,
    /// Sounding physical notes by key (script event id).
    held: HashMap<u8, std::collections::VecDeque<u64>>,
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
    /// Commands being applied, and ids of notes found ended: kept off the heap
    /// during audio callbacks.
    inbox: Vec<Command>,
    ended: Vec<u64>,
    /// MIDI the scripts generated, for the host to play into the part.
    midi: Vec<MidiOut>,
}

/// Notes and values the driver tracks at once; beyond it, plays are dropped.
const TRACKED: usize = 1024;

#[cfg(feature = "scan")]
impl Driver<ScriptThread> {
    pub fn scan_faults(&self) -> crate::script::ScanFaults {
        self.host.scan_faults()
    }
}

impl<S: Script> Driver<S> {
    /// The driver of `host`; the plan it plays on is the caller's runtime.
    pub fn new(host: S, groups: Vec<OscGroup>, rate: u32) -> Self {
        Self {
            host,
            groups,
            rate: f64::from(rate),
            notes: HashMap::with_capacity(TRACKED),
            // Any key can hold the entire tracked-note budget; prepare every queue.
            held: (0..128)
                .map(|key| (key, std::collections::VecDeque::with_capacity(TRACKED)))
                .collect(),
            next: 1,
            unmodeled: Vec::with_capacity(32),
            global: HashMap::with_capacity(32),
            voice_values: HashMap::with_capacity(TRACKED * 2),
            glides: Vec::with_capacity(64),
            glide_at: 0,
            inbox: Vec::with_capacity(256),
            ended: Vec::with_capacity(TRACKED),
            midi: Vec::with_capacity(256),
        }
    }

    /// A host message for the scripts.
    pub fn input(&mut self, rt: &Runtime, input: HostInput) {
        self.host.set_time(self.now_ms(rt));
        self.host.input(input);
    }

    /// The MIDI the scripts generated since the last call.
    pub fn drain_midi(&mut self, mut each: impl FnMut(MidiOut)) {
        self.midi.drain(..).for_each(&mut each);
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
        if self.notes.len() >= TRACKED {
            self.prune(rt);
        }
        let held = self.held.get_mut(&key).ok_or(Error::InvalidInput)?;
        if self.notes.len() >= TRACKED || held.len() >= TRACKED {
            rt.release(note)?;
            return Err(Error::Capacity);
        }
        let id = self.next;
        self.next += 1;
        self.notes.insert(id, note);
        held.push_back(id);
        self.host.set_time(self.now_ms(rt));
        self.host.note_on(id, key, (velocity * 127.0).round() as u8);
        if !self.host.handles_notes() {
            // No onNote: the program sounds as authored.
            rt.forward_attack(note)?;
        }
        self.apply(rt, false)
    }

    pub fn note_off(&mut self, rt: &mut Runtime, key: u8) -> Result<(), Error> {
        let Some(id) = self.held.get_mut(&key).and_then(|ids| ids.pop_front()) else {
            return Ok(());
        };
        self.release_id(rt, id, key)
    }

    /// Release the physical note paired by the MIDI adapter, across overlapping channels.
    pub fn note_off_note(&mut self, rt: &mut Runtime, note: NoteId, key: u8) -> Result<(), Error> {
        let Some(ids) = self.held.get_mut(&key) else {
            return Ok(());
        };
        let Some(at) = ids.iter().position(|id| self.notes.get(id) == Some(&note)) else {
            return Ok(());
        };
        let id = ids.remove(at).unwrap();
        self.release_id(rt, id, key)
    }

    fn release_id(&mut self, rt: &mut Runtime, id: u64, key: u8) -> Result<(), Error> {
        self.host.set_time(self.now_ms(rt));
        self.host.note_off(id, key);
        // Plays made by onRelease must not be linked to the closing gate.
        self.apply(rt, true)?;
        if let Some(note) = self.notes.get(&id).copied() {
            // A note whose voices already ended is gone; closing it is a no-op.
            stale_ok(rt.key_up(note, None))?;
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
        if !inbox.is_empty() && self.notes.len() >= TRACKED / 2 {
            self.prune(rt);
        }
        for command in inbox.drain(..) {
            match command {
                Command::EngineParameter { address, value } => {
                    if rt.set_engine_parameter(address, value).is_err() {
                        let category = "insert parameter without a DSP lane";
                        if !self.unmodeled.contains(&category)
                            && self.unmodeled.len() < self.unmodeled.capacity()
                        {
                            self.unmodeled.push(category);
                        }
                    }
                }
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
                Command::Change {
                    id,
                    what,
                    value,
                    relative,
                    immediate,
                    ..
                } => {
                    if let Some(note) = self.notes.get(&id).copied() {
                        let target = match what {
                            Change::Decibels => ModTarget::Decibels,
                            Change::Pan => ModTarget::Pan,
                            Change::Tune => ModTarget::Pitch,
                        };
                        match rt
                            .set_note_param_with_immediate(note, target, value, relative, immediate)
                        {
                            Err(Error::StaleHandle) => {
                                self.notes.remove(&id);
                            }
                            other => other?,
                        }
                    }
                }
                Command::Fade {
                    id,
                    from,
                    to,
                    ms,
                    kill,
                    layer,
                    ..
                } => {
                    if let Some(note) = self.notes.get(&id).copied() {
                        let frames = self.frames(ms);
                        let result = if layer == 0 {
                            rt.fade_note(note, from, to, frames, kill && to <= 0.0)
                        } else {
                            self.groups
                                .iter()
                                .filter(|g| g.layer == layer)
                                .try_for_each(|g| {
                                    rt.fade_note_group(
                                        note,
                                        g.group,
                                        from,
                                        to,
                                        frames,
                                        kill && to <= 0.0,
                                    )
                                })
                        };
                        match result {
                            Err(Error::StaleHandle) => {
                                self.notes.remove(&id);
                            }
                            other => other?,
                        }
                    }
                }
                Command::Parameter {
                    scope,
                    param,
                    value,
                    authored,
                } => {
                    self.parameter(rt, scope, param, value, authored)?;
                }
                Command::Midi(out) => {
                    if self.midi.len() < self.midi.capacity() {
                        self.midi.push(out);
                    }
                }
            }
        }
        Ok(())
    }

    /// A script's `setParameter` on the program or one of its layers. The
    /// runtime edits offsets of the authored gain and pan, so the written
    /// value becomes its difference to the preset's.
    fn parameter(
        &mut self,
        rt: &mut Runtime,
        scope: Scope,
        param: Param,
        value: f64,
        authored: f64,
    ) -> Result<(), Error> {
        if scope != Scope::Program {
            for i in 0..self.groups.len() {
                let g = self.groups[i];
                if match scope {
                    Scope::Layer(layer) => g.layer == layer,
                    Scope::Keygroup(id) => g.keygroup == id,
                    Scope::Oscillator(id) => g.oscillator == id,
                    Scope::Program => false,
                } {
                    self.group_parameter(rt, i64::from(g.group), param, value, authored)?;
                }
            }
            return Ok(());
        }
        self.group_parameter(rt, -1, param, value, authored)
    }

    fn group_parameter(
        &mut self,
        rt: &mut Runtime,
        group: i64,
        param: Param,
        value: f64,
        authored: f64,
    ) -> Result<(), Error> {
        match param {
            Param::Gain => {
                let db = 20.0 * (value.max(1e-6) / authored.max(1e-6)).log10();
                rt.set_group_param(group, ModTarget::Decibels, db, true)
            }
            Param::Pan => rt.set_group_param(group, ModTarget::Pan, value - authored, true),
            Param::Pitch => {
                let address = sampler_core::EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id("ENGINE_PAR_TUNE")
                        .ok_or(Error::InvalidInput)?,
                    group: group as i32,
                    slot: -1,
                    generic: -1,
                };
                let previous = rt.engine_parameter(address)?;
                let semitones = 12. * (value.max(1e-9) / authored.max(1e-9)).log2();
                rt.set_engine_parameter(
                    address,
                    previous + (semitones * 100_000. / 7.2).round() as i32,
                )
            }
            Param::Polyphony => {
                let slots = rt.voice_slots();
                let wanted = (value.round().max(1.0) as usize).min(slots);
                rt.set_voice_stealing(Some(Stealing {
                    fade: (self.rate / 100.0) as u32,
                    headroom: slots - wanted,
                }))
            }
        }
    }

    /// Forget notes the runtime no longer holds.
    fn prune(&mut self, rt: &Runtime) {
        self.notes.retain(|_, note| rt.note(*note).is_ok());
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
        stale_ok(if at <= rt.now() {
            rt.key_up(note, None)
        } else {
            rt.release_at(note, at)
        })
    }

    fn play(&mut self, rt: &mut Runtime, play: &Play, closing: bool) -> Result<(), Error> {
        // Forwarding a physical event keeps its identity and reuses its gate.
        if play.parent == Some(play.id) {
            if let Some(note) = self.notes.get(&play.id).copied() {
                self.select(rt, note, play)?;
                return stale_ok(rt.forward_attack(note).map(|_| ()));
            }
        }
        if self.notes.len() >= TRACKED {
            return Ok(());
        }
        let velocity = f64::from(play.velocity) / 127.0;
        let parent = play.parent.and_then(|p| self.notes.get(&p).copied());
        let open = parent.is_some() && !closing && play.duration_ms.is_none();
        let note = match parent {
            Some(parent) => rt.child(parent, play.key, velocity, open, Inheritance::Expression),
            None => {
                let address = sampler_core::ChannelAddress {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                };
                rt.generated_note(address, play.key, velocity)
            }
        };
        let note = match note {
            Ok(note) => note,
            // The originating note ended meanwhile: nothing to attach to.
            Err(Error::ClosedNote | Error::StaleHandle) => return Ok(()),
            Err(e) => return Err(e),
        };
        self.notes.insert(play.id, note);
        self.select(rt, note, play)?;
        let expression = Expression {
            gain: play.vol.clamp(0.0, 4.0),
            pan: play.pan.clamp(-1.0, 1.0),
            ..Expression::default()
        };
        // The script's tune is the note's own: bend follows the parent.
        if play.tune != 0.0 {
            rt.set_note_param(note, ModTarget::Pitch, play.tune, false)?;
        }
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
        if play.duration_ms == Some(0.0) {
            rt.retire_when_silent(note)?;
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
                && play.osc.is_none_or(|o| o + 1 == g.osc)
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
    /// Frames ahead of the clock that streamed voices read, if any stream.
    horizon: Option<u32>,
    _stream: Option<Stream>,
    feed: MidiFeed,
}

/// A handle freed because its voices finished is not an error for a later
/// release, fade or modulation.
fn stale_ok(r: Result<(), Error>) -> Result<(), Error> {
    match r {
        Err(Error::StaleHandle) => Ok(()),
        other => other,
    }
}

impl Player {
    pub fn new(program: Program, limits: Limits, rate: u32) -> Result<Self, Error> {
        let mut rt = Runtime::new(program.plan, limits)?;
        let mut horizon = None;
        let mut stream = program.stream;
        if let Some(s) = stream.as_mut() {
            // Heads bound only starts; running voices request a page ahead.
            horizon = Some((s.horizon.max(sampler_core::PAGE_FRAMES) + 4096) as u32);
            if let Some(cache) = s.cache.take() {
                rt = rt.with_stream_cache(cache);
            }
        }
        Ok(Self {
            rt,
            driver: Driver::new(program.host, program.groups, rate),
            horizon,
            _stream: stream,
            feed: MidiFeed::default(),
        })
    }

    pub fn runtime(&self) -> &Runtime {
        &self.rt
    }

    /// What plays asked for that was ignored, once each.
    pub fn unmodeled(&self) -> &[&'static str] {
        self.driver.unmodeled()
    }

    /// A host message (controller, bend...) for the scripts.
    pub fn input(&mut self, input: HostInput) {
        self.driver.input(&self.rt, input);
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
            self.feed.pump(&mut self.driver, &mut self.rt);
            let step = due.map_or(left, |d| d.max(1).min(left));
            if let Some(horizon) = self.horizon {
                // Pending pages play silent and count as underruns.
                let _ = self.rt.service_streaming(horizon);
            }
            self.rt.render(&mut out[done..done + step])?;
            done += step;
        }
        self.rt.flush_ended(|_| true);
        Ok(())
    }
}

impl Driver<ScriptThread> {
    pub fn ui(&self) -> &std::sync::Arc<UiBridge> {
        self.host.ui()
    }
    pub fn set_control(&mut self, id: sampler_ui_ir::ControlId, value: f64) -> bool {
        self.host.set_control(id, value)
    }
}
