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
    Error, Expression, Frame, Inheritance, Input, Limits, ModTarget, NoteId, NotePitch, Prepared, Protocol,
    Runtime, Stealing,
};
use std::collections::HashMap;
pub use thread::{Loaded, ScriptThread, UiBridge, DiagnosticAcknowledgement, DiagnosticPhase,
    DiagnosticPoll, DiagnosticProgress, DiagnosticRequest, DiagnosticRequestError};

/// What the driver needs of a script runtime: a [`ScriptHost`] run inline (offline,
/// deterministic), or a [`ScriptThread`] that keeps Lua off the audio thread.
pub trait Script {
    fn handles_notes(&self) -> bool;
    /// Off audio only: bounded owner query; threaded hosts may wait for a reply.
    fn sample_load_status(&mut self)->Result<crate::script::SampleLoadStatus,String> {Err("sample status service unavailable".into())}
    fn bind_sample_control(&mut self, _control: sampler_core::SampleControl) -> Result<(), String> {
        Err("sample source service is not installed on this script owner".into())
    }
    fn bind_sample_stream(&mut self, _sources: sampler_kontakt::DynamicSources) -> Result<(), String> {
        Err("sample streaming service is not installed on this script owner".into())
    }
    fn set_time(&mut self, ms: f64);
    fn note_on(&mut self, id: u64, key: u8, velocity: u8);
    fn note_off(&mut self, id: u64, key: u8);
    /// Physical callback channel (0-based). Legacy script implementations retain
    /// their existing callbacks; real UVI owners override this transport boundary.
    fn note_on_channel(&mut self, id: u64, key: u8, velocity: u8, _channel: u8) {
        self.note_on(id, key, velocity)
    }
    fn note_off_channel(&mut self, id: u64, key: u8, velocity: u8, _channel: u8) {
        self.note_off_velocity(id, key, velocity)
    }
    fn note_off_velocity(&mut self, id: u64, key: u8, _velocity: u8) {
        self.note_off(id, key)
    }
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
    fn sample_load_status(&mut self)->Result<crate::script::SampleLoadStatus,String> {Ok(ScriptHost::sample_load_status(self))}
    fn bind_sample_control(&mut self, control: sampler_core::SampleControl) -> Result<(), String> { ScriptHost::bind_sample_control(self, control) }
    fn bind_sample_stream(&mut self, sources: sampler_kontakt::DynamicSources) -> Result<(), String> { ScriptHost::bind_sample_stream(self, sources) }
    fn tick(&mut self, _now_ms: f64) { self.poll_sample_loads(); }
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
    fn note_off_velocity(&mut self, id: u64, key: u8, velocity: u8) {
        ScriptHost::note_off(self, id, key, velocity, 0)
    }
    fn note_on_channel(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        ScriptHost::note_on(self, id, key, velocity, channel)
    }
    fn note_off_channel(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        ScriptHost::note_off(self, id, key, velocity, channel)
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

/// Maximum accepted pending script MIDI outputs in one driver. This logical
/// bound is independent of the allocator's reported spare capacity.
pub const MIDI_OUTPUT_CAPACITY: usize = 256;

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

#[derive(Clone, Copy)]
struct TrackedNote {
    note: NoteId,
    channel: u8,
    // Preserve callback identity until real key-up, even if core cancellation
    // already retired its source. This consumes the existing TRACKED budget.
    physical_down: bool,
}

pub struct Driver<S: Script> {
    host: S,
    groups: Vec<OscGroup>,
    rate: f64,
    /// Script event / voice id -> core note.
    notes: HashMap<u64, TrackedNote>,
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
    /// Control/diagnostic query only; never call from an audio callback.
    pub fn sample_load_status(&mut self)->Result<crate::script::SampleLoadStatus,String> {self.host.sample_load_status()}
    /// Hand the dedicated sample queue to its sole script/control producer.
    pub fn bind_sample_control(&mut self, control: sampler_core::SampleControl) -> Result<(), String> {
        self.host.bind_sample_control(control)
    }
    pub fn bind_sample_stream(&mut self, sources: sampler_kontakt::DynamicSources) -> Result<(), String> {
        self.host.bind_sample_stream(sources)
    }
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
            midi: Vec::with_capacity(MIDI_OUTPUT_CAPACITY),
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

    /// Copy and consume only the output prefix that fits. A full or empty
    /// consumer does not discard a later output; the remaining FIFO stays owned
    /// by this driver's preallocated queue. No Core acceptance is implied.
    pub fn drain_midi_batch(&mut self, out: &mut [Option<MidiOut>]) -> usize {
        out.fill(None);
        let count = out.len().min(self.midi.len());
        for (slot, message) in out.iter_mut().zip(self.midi.drain(..count)) {
            *slot = Some(message);
        }
        count
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
        let channel = rt.input_source(note)?.ok_or(Error::InvalidInput)?.channel;
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
        self.notes.insert(id, TrackedNote { note, channel, physical_down: true });
        held.push_back(id);
        self.host.set_time(self.now_ms(rt));
        self.host.note_on_channel(id, key, (velocity * 127.0).round() as u8, channel);
        if !self.host.handles_notes() {
            // No onNote: the program sounds as authored, with the script's
            // standing modulation values.
            for (id, value) in &self.global {
                rt.set_note_script_value(note, *id, *value)?;
            }
            rt.forward_attack(note)?;
        }
        self.apply(rt, false)
    }

    pub fn note_off(&mut self, rt: &mut Runtime, key: u8) -> Result<(), Error> {
        let Some(id) = self.held.get_mut(&key).and_then(|ids| ids.pop_front()) else {
            return Ok(());
        };
        self.release_id(rt, id, key, None)
    }

    /// Release the physical note paired by the MIDI adapter, across overlapping channels.
    pub fn note_off_note(&mut self, rt: &mut Runtime, note: NoteId, key: u8) -> Result<(), Error> {
        self.note_off_note_velocity(rt, note, key, None)
    }

    /// Retain original normalized physical release velocity in the core and
    /// its MIDI projection in the owning script callback.
    pub fn note_off_note_velocity(&mut self, rt: &mut Runtime, note: NoteId, key: u8,
        velocity: Option<f64>) -> Result<(), Error> {
        if velocity.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
            return Err(Error::InvalidInput);
        }
        let Some(ids) = self.held.get_mut(&key) else {
            return Ok(());
        };
        let Some(at) = ids.iter().position(|id| self.notes.get(id).map(|entry| entry.note) == Some(note)) else {
            return Ok(());
        };
        let id = ids.remove(at).unwrap();
        self.release_id(rt, id, key, velocity)
    }

    fn release_id(&mut self, rt: &mut Runtime, id: u64, key: u8,
        velocity: Option<f64>) -> Result<(), Error> {
        let physical = self.notes.get(&id).copied();
        self.host.set_time(self.now_ms(rt));
        self.host.note_off_channel(id, key,
            velocity.map_or(64, |v| (v * 127.).round() as u8),
            physical.map_or(0, |entry| entry.channel));
        // Plays made by onRelease must not be linked to the closing gate.
        // Callback failure cannot prevent the original physical key-up.
        let applied = self.apply(rt, true);
        if let Some(entry) = self.notes.get_mut(&id) { entry.physical_down = false; }
        let closed = physical.map_or(Ok(()), |entry| stale_ok(rt.key_up(entry.note, velocity)));
        applied.and(closed)
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
                Command::Impulse { slot, upload } => {
                    if upload.swap(rt, slot).is_err() {
                        let category = "convolver impulse without a DSP slot";
                        if !self.unmodeled.contains(&category)
                            && self.unmodeled.len() < self.unmodeled.capacity()
                        {
                            self.unmodeled.push(category);
                        }
                    }
                }
                Command::Play(play) => self.play(rt, &play, closing)?,
                Command::Release { id, at_ms } => {
                    if let Some(note) = self.notes.get(&id).map(|entry| entry.note) {
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
                    if let Some(note) = self.notes.get(&id).map(|entry| entry.note) {
                        let target = match what {
                            Change::Decibels => ModTarget::Decibels,
                            Change::Pan => ModTarget::Pan,
                            Change::Tune => ModTarget::Pitch,
                        };
                        match rt
                            .set_note_param_with_immediate(note, target, value, relative, immediate)
                        {
                            Err(Error::StaleHandle) => {
                                if self.notes.get(&id).is_some_and(|entry| !entry.physical_down) {
                                    self.notes.remove(&id);
                                }
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
                    if let Some(note) = self.notes.get(&id).map(|entry| entry.note) {
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
                                if self.notes.get(&id).is_some_and(|entry| !entry.physical_down) {
                                    self.notes.remove(&id);
                                }
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
                    if self.midi.len() < MIDI_OUTPUT_CAPACITY {
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
        if param == Param::Gain && let Scope::Keygroup(node) = scope {
            let id = crate::engine_parameters::scope_gain_control(node)?;
            let plan = rt.active_plan();
            if rt.control_definition(plan, id).is_ok() {
                // Exact absolute KG control; zero opens/closes without a dB
                // epsilon or callback key allocation. Missing is a legacy plan,
                // whose existing group-offset path remains explicit below.
                rt.edit_controls(plan, None, &[sampler_core::ControlWrite {
                    id, value: sampler_core::ControlValue::Real(value),
                }])?;
                return Ok(());
            }
        }
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
        self.notes.retain(|_, entry| entry.physical_down || rt.note(entry.note).is_ok());
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
                if let Some(note) = self.notes.get(&v).map(|entry| entry.note) {
                    match rt.set_note_script_value(note, id, value) {
                        Err(Error::StaleHandle) => self.ended.push(v),
                        other => other?,
                    }
                }
            }
            None => {
                self.global.insert(id, value);
                for (v, note) in &self.notes {
                    match rt.set_note_script_value(note.note, id, value) {
                        Err(Error::StaleHandle) => self.ended.push(*v),
                        other => other?,
                    }
                }
            }
        }
        for v in self.ended.drain(..) {
            if self.notes.get(&v).is_some_and(|entry| !entry.physical_down) {
                self.notes.remove(&v);
            }
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
            if let Some(note) = self.notes.get(&play.id).map(|entry| entry.note) {
                self.select(rt, note, play)?;
                return stale_ok(rt.forward_attack(note).map(|_| ()));
            }
        }
        if self.notes.len() >= TRACKED {
            return Ok(());
        }
        let velocity = f64::from(play.velocity) / 127.0;
        let parent = play.parent.and_then(|p| self.notes.get(&p).map(|entry| entry.note));
        if play.parent.is_some() && parent.is_none() {
            // A retired owner cannot turn its late command into an unlinked
            // generated note after driver pruning or input cancellation.
            return Ok(());
        }
        let follows_creator = matches!(play.creator, crate::script::PlayCreator::NoteOn(_));
        let parent_open = parent.is_some_and(|note| rt.note(note).is_ok_and(|(_, _, gate)| gate));
        let open = parent_open && follows_creator && !closing && play.duration_ms.is_none();
        let note = match parent {
            Some(parent) => (|| {
                // UVI callbacks report the physical input key. The MIDI2
                // one-note absolute pitch is a separate base component: keep
                // its offset when the authored script maps/transposes that key.
                // Channel/PN expressions still follow through the ordinary
                // Expression inheritance path and must not be added here.
                let pitch = match (rt.note_pitch(parent)?, rt.input_source(parent)?) {
                    (NotePitch::Absolute(pitch), Some(source)) =>
                        NotePitch::Absolute(f64::from(play.key) + pitch - f64::from(source.key)),
                    _ => NotePitch::Key(play.key),
                };
                rt.child_pitched(parent, pitch, velocity, open, Inheritance::Expression)
            })(),
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
        self.notes.insert(play.id, TrackedNote { note, channel: 0, physical_down: false });
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
            None if play.origin == crate::script::PlayOrigin::PostedEvent => {},
            None if play.creator == crate::script::PlayCreator::None => self.release(rt, note, now)?,
            None if follows_creator && (!parent_open || closing) => self.release(rt, note, now)?,
            None => {}
        }
        if play.duration_ms == Some(0.0)
            || (play.duration_ms.is_none() && play.creator == crate::script::PlayCreator::Event) {
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
        let mut driver=Driver::new(program.host, program.groups, rate);
        if rt.sample_target_count()>0 {
            if let Some(s)=&stream { driver.bind_sample_stream(s._keep.0.dynamic_sources()).map_err(|_|Error::NotReady)?; }
            let (next,control)=rt.with_sample_updates(64)?;rt=next;
            driver.bind_sample_control(control).map_err(|_|Error::NotReady)?;
        }
        Ok(Self {
            rt,
            driver,
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

#[cfg(test)]
mod midi_output_batch_tests {
    use super::*;

    #[test]
    fn zero_short_and_full_midi_consumers_keep_every_remaining_output_in_fifo_order() {
        let host = ScriptHost::new("<UVI4><Program/></UVI4>", (), Default::default()).unwrap();
        let mut driver = Driver::new(host, vec![], 48000);
        let expected: Vec<_> = (0..MIDI_OUTPUT_CAPACITY).map(|index| MidiOut {
            status: 0xb0 | ((index / 128) as u8), a: 20, b: (index % 128) as u8,
        }).collect();
        driver.midi.extend_from_slice(&expected);
        let original_capacity = driver.midi.capacity();
        assert_eq!(driver.drain_midi_batch(&mut []), 0);
        assert_eq!(driver.midi, expected);
        let mut seen = Vec::new();
        for length in [1, 64, 65, 256] {
            let mut buffer = vec![Some(MidiOut { status: 0xff, a: 127, b: 127 }); length];
            let before = driver.midi.len();
            let accepted = driver.drain_midi_batch(&mut buffer);
            assert_eq!(accepted, length.min(before));
            assert!(buffer[accepted..].iter().all(Option::is_none));
            seen.extend(buffer[..accepted].iter().copied().map(Option::unwrap));
            assert_eq!(driver.midi, expected[seen.len()..]);
            assert_eq!(driver.midi.capacity(), original_capacity);
        }
        assert_eq!(seen, expected);
        assert_eq!(driver.drain_midi_batch(&mut [None; 64]), 0);
    }

    #[test]
    fn four_product_sized_batches_retain_the_sixty_fifth_and_last_accepted_output() {
        let host = ScriptHost::new("<UVI4><Program/></UVI4>", (), Default::default()).unwrap();
        let mut driver = Driver::new(host, vec![], 48000);
        let expected: Vec<_> = (0..MIDI_OUTPUT_CAPACITY).map(|index| MidiOut {
            status: 0xb0 | ((index / 128) as u8), a: 21, b: (index % 128) as u8,
        }).collect();
        driver.midi.extend_from_slice(&expected);
        let mut delivered = Vec::new();
        for _ in 0..MIDI_OUTPUT_CAPACITY.div_ceil(64) {
            let mut batch = [None; 64];
            let count = driver.drain_midi_batch(&mut batch);
            assert_eq!(count, 64);
            delivered.extend(batch.into_iter().flatten());
        }
        assert_eq!(delivered, expected);
        assert_eq!(delivered[64], expected[64]);
        assert_eq!(delivered.last(), expected.last());
        assert!(driver.midi.is_empty());
    }
}

#[cfg(test)]
mod detached_lifetime_tests {
    use super::*;

    fn fixture(looping: bool) -> (Runtime, Driver<ScriptHost>) {
        let playback = sampler_core::Playback {
            end: Some(256),
            loop_range: looping.then_some(sampler_core::Loop {
                start: 0, end: 128, mode: sampler_core::LoopMode::UntilRelease,
                shape: sampler_core::LoopShape::Wrap, passes: None,
            }),
            ..Default::default()
        };
        let plan = Prepared::new(48000,
            vec![sampler_core::Pcm::new(48000, vec![[0.25; 2]; 256].into_boxed_slice()).unwrap()],
            vec![sampler_core::Region {
                sample: 0, key_low: 60, key_high: 60, root_key: Some(60),
                velocity_low: 0., velocity_high: 1., gain: 1.,
                envelope: sampler_core::Envelope::new(0, 0, 0, 1., 64).unwrap(), playback,
            }], 1).unwrap();
        let limits = Limits::for_plan(&plan, 8, 8);
        let host = ScriptHost::new(r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
            local voice
            function onController(e)
              if e.controller == 1 then voice = playNote(60, 100, -1)
              elseif e.controller == 2 then releaseVoice(voice) end
            end
        ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#, (), Default::default()).unwrap();
        (Runtime::new(plan, limits).unwrap(), Driver::new(host, vec![], 48000))
    }

    #[test]
    fn non_note_creator_negative_duration_loop_survives_five_seconds_until_explicit_release() {
        let (mut runtime, mut driver) = fixture(true);
        driver.input(&runtime, HostInput::Controller { cc: 1, value: 127, channel: 4 });
        driver.wake(&mut runtime).unwrap();
        assert_eq!(runtime.voice_count(), 1);
        let mut last = [[0.; 2]; 128];
        for _ in 0..2000 {
            driver.wake(&mut runtime).unwrap();
            runtime.render(&mut last).unwrap();
        }
        assert!(runtime.now() > 5 * 48000);
        assert_eq!(runtime.voice_count(), 1, "non-note playNote(-1) has no five-second automatic release");
        assert!(last.iter().flatten().any(|value| value.abs() > 0.01));
        assert_eq!(runtime.note_count(), 1);
        assert!(runtime.live_notes().all(|note| runtime.input_source(note).unwrap().is_none()),
            "a generated loop does not invent a physical input source");
        driver.input(&runtime, HostInput::Controller { cc: 2, value: 127, channel: 4 });
        driver.wake(&mut runtime).unwrap();
        for _ in 0..8 { runtime.render(&mut last).unwrap(); }
        runtime.flush_ended(|_| true);
        assert_eq!(runtime.voice_count(), 0);
        assert_eq!(runtime.note_count(), 0);
        assert!(last.iter().flatten().all(|value| *value == 0.));
        assert!(driver.unmodeled().is_empty());
    }

    #[test]
    fn non_note_creator_negative_duration_one_shot_retires_when_its_source_finishes() {
        let (mut runtime, mut driver) = fixture(false);
        driver.input(&runtime, HostInput::Controller { cc: 1, value: 127, channel: 0 });
        driver.wake(&mut runtime).unwrap();
        let mut output = [[0.; 2]; 128];
        runtime.render(&mut output).unwrap();
        assert!(output.iter().flatten().any(|value| value.abs() > 0.01));
        for _ in 0..8 { runtime.render(&mut output).unwrap(); }
        runtime.flush_ended(|_| true);
        assert!(runtime.now() < 5 * 48000);
        assert_eq!(runtime.voice_count(), 0);
        assert_eq!(runtime.note_count(), 0, "manual generated ownership ends at natural EOF without an invented timeout");
        assert!(driver.unmodeled().is_empty());
    }
}
