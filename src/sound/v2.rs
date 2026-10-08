//! [`Core`] over `sampler-core`: one [`Runtime`] per rack part, fed through a
//! `sampler-midi` zone, mixed through the part's output tree.
//!
//! Wired: host notes (exact CLAP/VST3 ownership and NOTE_END, layered parts)
//! with per-note tuning, gain, pan, pressure and brightness; MIDI 1.0 and 2.0
//! channel voice packets (notes, sustain and sostenuto, controllers, pitch bend
//! with RPN sensitivity, channel and polyphonic pressure, all notes/sound off);
//! the mixer's part gain, pan, tune, mute, solo, output pair and aux send, pair
//! faders, peaks and the scope tap; every tree node's gain, pan, mute, solo and
//! output (its parent or a DAW pair). Loading: Kontakt instruments through
//! `sampler-kontakt` (cancelable, every group a mixer node), WAV files as one region.
//!
//! Kontakt samples stream: start data stays resident and the rest is read
//! from disk ahead of each voice.
//!
//! A part plays its MIDI on the zone's manager channel, or as an MPE lower
//! zone with member channels at its bend range.
//!
//! Not yet: per-note controllers and program changes (counted), a sample-rate change
//! without reloading.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use sampler_core::{
    BusMix, ChannelAddress, ControlContext, ControlDefinition, ControlDomain, ControlValue, ControlWrite, Envelope, Expression, Frame, Input, Limits, NoteId, PAGE_FRAMES, Pcm, PlanControl, Playback, Prepared, Protocol,
    Region, Runtime, Stealing, StreamCache, Threads,
};
use sampler_ir as ir;
use sampler_midi::{ApplyError, Articulator, Intercept, Mpe, Packets, Zone};

use super::event::{Event, HostNote, NoteExpression};
use super::mix::{Mix, PartControls, Peaks, balance};
use super::report::{LoadReport, Missing, RuntimeProblems};
use super::tree::{self, MixNode, MixTree, NodeKind, NodeMix, NodeOutput};
use super::{
    BUSES, Block, BlockInfo, Core, CoreError, CoreLoader, Description, LoadFailure, LoadRequest, Loaded, ScriptUi, Stream, MAX_BLOCK, Progress,
    RACK_SLOTS, Rendered, Voices,
};

/// Host notes tracked for ownership and NOTE_END across the rack.
const HELD: usize = 1024;
/// Notes a part holds at once, sounding or awaiting NOTE_END.
const NOTES: usize = 128;
/// [`Held::part`] of a note whose runtime was replaced: ends at the next block.
const ORPHAN: usize = usize::MAX;
/// Every input reaches a part's zone as MIDI 1.0 on its manager channel, so a
/// bend, pedal or controller on any channel reaches every note of the part.
const WIRE: ChannelAddress = ChannelAddress { protocol: Protocol::Midi1, port: 0, group: 0, channel: 0 };

/// One playable part: its runtime, the MIDI zone in front of it and its tree.
pub struct Part {
    runtime: Runtime,
    mpe: Mpe,
    tune: f32,
    /// Per tree node, its runtime bus (none for the root).
    buses: Box<[Option<usize>]>,
    tree: MixTree,
    /// Node settings as last set, after the root; preallocated to the tree.
    nodes: Vec<NodeMix>,
    audible: Box<[bool]>,
    /// DAW pairs some node plays to directly, as a bit set.
    direct: u32,
    problems: RuntimeProblems,
    /// Velocity, channel, CC or program selecting articulations, when not keys.
    articulator: Option<Articulator>,
    /// The part's switching for each driver, from its instrument, and which
    /// [`PartControls::switching`] byte is applied.
    drivers: Vec<(sampler_core::Switching, Vec<sampler_core::Keyswitch>)>,
    switching: u8,
    /// The instrument's own driver, for [`Self::drivers`].
    inherited: usize,
    /// The instrument's default articulation, when the runtime holds the
    /// articulation (it numbers that one 0).
    articulations: Option<usize>,
    /// Behavior-owned switching: each articulation's first switch key. The
    /// script holds the selection, so it is read from the key the driver tapped.
    tap_keys: Option<Vec<Option<u8>>>,
    /// Notes keep their member channel ([`PartControls::mpe`]).
    mpe_zone: bool,
    /// The bend range last sent to the zone, 0 for its default.
    bend_range: u8,
    /// Frames ahead of the clock that streamed voices read, if any stream.
    horizon: Option<u32>,
    /// Kept alive while the part plays; dropped with it, off the audio thread.
    _stream: Option<Arc<Stream>>,
    /// Grows the voice pool off the audio thread; stopped when the part drops.
    grower: Option<Grower>,
    /// The program's Lua scripts: they choose which of its oscillators play.
    script: Option<Box<ScriptDriver>>,
}

/// Bytes the system could give a new allocation without swapping, from
/// `/proc/meminfo`; `None` where that is unavailable.
fn mem_available() -> Option<usize> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kib: usize = line.split_whitespace().nth(1)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// A growth may take at most this share of available memory.
const GROWTH_SHARE: usize = 4;

/// A thread that sleeps until the audio side reports a nearly full voice pool
/// (or a growth coming back), then doubles the pool while the new voices fit a
/// quarter of the memory available at that moment (and note capacity, sized
/// for `ceiling` voices, allows). Past that, only stealing is left.
struct Grower {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Grower {
    fn start(
        runtime: &mut Runtime,
        mut control: PlanControl,
        ceiling: usize,
        per_voice: usize,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new().name("sampler-grow".into()).spawn({
            let stop = stop.clone();
            move || {
                while !stop.load(Ordering::Relaxed) {
                    let capacity = control.voice_capacity();
                    if control.voice_pressure() && capacity < ceiling {
                        let next = (capacity * 2).min(ceiling);
                        let fits = mem_available()
                            .is_none_or(|free| (next - capacity).saturating_mul(per_voice) <= free / GROWTH_SHARE);
                        if fits {
                            let _ = control.grow_voices(next);
                        }
                    }
                    std::thread::park();
                }
            }
        })?;
        runtime.set_growth_waker(thread.thread().clone());
        Ok(Self { stop, thread: Some(thread) })
    }
}

impl Drop for Grower {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

impl Part {
    fn new(runtime: Runtime, tree: MixTree) -> Result<Self, CoreError> {
        let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
        let mpe = Mpe::new(&runtime, WIRE.port, WIRE.group, Zone::Lower, 15, NOTES).map_err(core)?;
        // Plans without articulations have nothing to drive.
        let articulator = runtime.performance(0).and_then(|p| Articulator::new(&runtime, p, WIRE.port)).ok();
        let count = tree.nodes.len();
        Ok(Self {
            runtime,
            mpe,
            tune: 0.0,
            buses: (0..count).map(|n| n.checked_sub(1)).collect(),
            tree,
            nodes: vec![NodeMix::default(); count.saturating_sub(1)],
            audible: vec![true; count].into_boxed_slice(),
            direct: 0,
            problems: RuntimeProblems::default(),
            articulator,
            drivers: Vec::new(),
            switching: 0,
            inherited: 0,
            articulations: None,
            tap_keys: None,
            mpe_zone: false,
            bend_range: 0,
            horizon: None,
            _stream: None,
            grower: None,
            script: None,
        })
    }

    /// Follow the part's tuning, MPE and bend range settings.
    fn configure(&mut self, c: &PartControls) {
        if self.tune != c.tune && self.mpe.transpose(&mut self.runtime, f64::from(c.tune)).is_ok() {
            self.tune = c.tune;
        }
        if c.switching != self.switching && !self.drivers.is_empty() {
            self.switching = c.switching;
            // The instrument's own driver until the player remaps.
            let driver = if c.switching & 0x80 != 0 { usize::from(c.switching >> 1 & 7) } else { self.inherited };
            if let Some((switching, keys)) = self.drivers.get(driver) {
                // ponytail: set_switching frees its key table on this thread; a few hundred bytes per remap.
                let _ = self.runtime.set_switching(switching.clone(), keys.clone());
            }
        }
        self.mpe_zone = c.mpe;
        if self.bend_range != c.bend_range {
            // 0 keeps what the instrument and the player's controller say.
            self.mpe.set_bend_range((c.bend_range > 0).then_some(c.bend_range));
            self.bend_range = c.bend_range;
        }
    }

    /// Keep one switching table per driver for `instrument`'s articulations,
    /// for a remap to swap in without lowering the plan again.
    fn set_drivers(&mut self, instrument: &ir::Instrument) {
        if instrument.articulations.is_empty() {
            return;
        }
        let mut with_alternatives = instrument.clone();
        with_alternatives.assign_alternatives(32);
        self.inherited = instrument.switching.driver as usize;
        self.drivers.clear();
        for driver in [ir::Driver::Keys, ir::Driver::Velocity, ir::Driver::Channel, ir::Driver::Controller, ir::Driver::Program] {
            let switching = ir::Switching { driver, ..instrument.switching };
            let Ok((keys, switching)) = sampler_core::lower::switching(&with_alternatives, switching) else { break };
            self.drivers.push((switching, keys));
        }
    }

    /// Apply node settings to the runtime's buses.
    fn mix_nodes(&mut self, nodes: &[NodeMix]) {
        for (to, from) in self.nodes.iter_mut().zip(nodes) {
            *to = *from;
        }
        tree::audible(&self.tree, &self.nodes, &mut self.audible);
        self.direct = 0;
        for (n, mix) in self.nodes.iter().enumerate() {
            let Some(bus) = self.buses[n + 1] else { continue };
            let output = match mix.output {
                NodeOutput::Pair(pair) if usize::from(pair) < BUSES => {
                    self.direct |= 1 << pair;
                    Some(usize::from(pair))
                }
                _ => None,
            };
            let gain = tree::stereo_gain(mix, self.audible[n + 1]);
            let _ = self.runtime.set_bus_mix(bus, BusMix { gain, output });
        }
    }
}

/// A replaced part, dropped on a worker.
#[derive(Default)]
pub struct Retired(pub Option<Box<Part>>);

#[derive(Clone, Copy)]
struct Held {
    part: usize,
    note: HostNote,
    input: Input,
    id: NoteId,
}

pub struct V2Core {
    parts: Vec<Option<Box<Part>>>,
    rate: f64,
    mix: Mix,
    held: Vec<Held>,
    /// Notes the ownership table had no room for.
    overflow: u64,
    buses: Box<[Block; BUSES]>,
    written: [bool; BUSES],
    scratch: Box<[Frame; MAX_BLOCK]>,
    /// Nodes routed straight to a DAW pair, per pair.
    direct: Box<[[Frame; MAX_BLOCK]; BUSES]>,
    tap: Option<usize>,
    tapped: Box<[f32; MAX_BLOCK]>,
    peaks: Peaks,
}

impl Default for V2Core {
    fn default() -> Self {
        Self::with_parts(RACK_SLOTS, 48000.0)
    }
}

fn wire(key: u8, external_id: Option<i32>) -> Input {
    Input { protocol: WIRE.protocol, port: WIRE.port, group: WIRE.group, channel: WIRE.channel, key, external_id }
}

/// A host note's owner on the wire, distinct from MIDI notes by its ID; notes
/// without one (VST3) get a negative ID per channel. In an MPE zone it keeps
/// its member channel.
fn host_input(note: HostNote, mpe: bool) -> Input {
    let id = if note.id >= 0 { note.id } else { -1 - i32::from(note.channel) };
    Input { channel: if mpe { note.channel & 15 } else { WIRE.channel }, ..wire(note.key, Some(id)) }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |p, x| p.max(x.abs()))
}

/// A MIDI 1.0 channel voice message into `part`'s zone, on its manager
/// channel, or in an MPE zone on its own.
fn wire_event(part: &mut Part, status: u8, a: u8, b: u8) {
    wire_packet(part, &[0x2000_0000 | u32::from(status) << 16 | u32::from(a & 127) << 8 | u32::from(b & 127)]);
}

/// A channel voice packet into `part`'s zone on its group, and on its manager
/// channel unless the zone is MPE.
fn wire_packet(part: &mut Part, words: &[u32]) {
    let mut words = [words[0], words.get(1).copied().unwrap_or(0)];
    words[0] &= if part.mpe_zone { 0xf0ff_ffff } else { 0xf0f0_ffff };
    let words = &words[..if words[0] >> 28 == 4 { 2 } else { 1 }];
    if !articulated(part, words) {
        return;
    }
    if let Some(Ok(packet)) = Packets::new(words).next()
        && let Err(ApplyError::Core(sampler_core::Error::Capacity)) = part.mpe.apply(&mut part.runtime, packet)
    {
        part.problems.capacity_drops += 1;
    }
}

/// Run the articulation driver on a packet; false when it took the packet.
fn articulated(part: &mut Part, words: &[u32]) -> bool {
    let Some(articulator) = part.articulator.as_mut() else { return true };
    let Some(Ok(packet)) = Packets::new(words).next() else { return true };
    !matches!(articulator.intercept(&mut part.runtime, packet), Ok(Intercept::Consumed(_)))
}

/// Change one note's expression in place.
fn express(runtime: &mut Runtime, note: NoteId, change: impl FnOnce(&mut Expression)) {
    let Ok(owner) = runtime.expression_id(note) else { return };
    let Ok(mut expression) = runtime.expression(owner) else { return };
    change(&mut expression);
    expression.gain = expression.gain.clamp(0.0, sampler_core::MAX_EXPRESSION_GAIN);
    expression.pan = expression.pan.clamp(-1.0, 1.0);
    let _ = runtime.set_expressions(&[(owner, expression)]);
}

fn unit_scale(value: f64) -> u32 {
    (value.clamp(0.0, 1.0) * f64::from(u32::MAX)) as u32
}

/// `expression` on one held note; the notes a script played from it follow.
fn note_expression(part: &mut Part, id: NoteId, expression: NoteExpression) {
    let tune = f64::from(part.tune);
    express(&mut part.runtime, id, |e| match expression {
        NoteExpression::Tune(semitones) => e.pitch_semitones = semitones + tune,
        NoteExpression::Gain(gain) => e.gain = gain,
        NoteExpression::Pan(pan) => e.pan = pan,
        NoteExpression::Pressure(v) => e.pressure = unit_scale(v),
        NoteExpression::Brightness(v) => e.timbre = unit_scale(v),
    })
}

/// Controllers, bend, pressure and program changes reach the part's scripts
/// (they still reach the engine: a script's `postEvent` of one adds to it).
fn tell_script(part: &mut Part, kind: u32, status: u8, channel: u8, a: u8, b: u8, data: u32) {
    use sampler_uvi::scripted::HostInput as Input;
    let Part { script: Some(script), runtime, .. } = part else { return };
    let high = (data >> 25) as u8;
    let input = match (kind, status) {
        (2, 0xb0) => Input::Controller { cc: a, value: b, channel },
        (4, 0xb0) => Input::Controller { cc: a, value: high, channel },
        (2, 0xe0) => Input::Bend { value: (f64::from(u16::from(b) << 7 | u16::from(a)) - 8192.0) / 8192.0, channel },
        (4, 0xe0) => Input::Bend { value: f64::from(data) / 2_147_483_648.0 - 1.0, channel },
        (2, 0xd0) => Input::Touch { value: a, channel },
        (4, 0xd0) => Input::Touch { value: high, channel },
        (2, 0xa0) => Input::PolyTouch { key: a, value: b, channel },
        (4, 0xa0) => Input::PolyTouch { key: a, value: high, channel },
        (2 | 4, 0xc0) => Input::Program { value: a, channel },
        _ => return,
    };
    script.input(runtime, input);
}

/// A channel voice packet into one part, MIDI 2.0 values at full precision;
/// per-note messages reach held host notes.
fn packet(part: &mut Part, index: usize, held: &[Held], words: [u32; 2]) {
    let [word, data] = words;
    let (kind, status, a, b) = (word >> 28, (word >> 16) as u8 & 0xf0, (word >> 8) as u8 & 127, word as u8 & 127);
    let channel = (word >> 16) as u8 & 15;
    // Per-note messages reach held host notes on the key at full precision.
    let per_note = |part: &mut Part, expression: NoteExpression| {
        for h in held.iter().filter(|h| h.part == index && h.note.channel == channel && h.note.key == a) {
            note_expression(part, h.id, expression);
        }
    };
    tell_script(part, kind, status, channel, a, b, data);
    match (kind, status) {
        (2, 0xa0) => per_note(part, NoteExpression::Pressure(f64::from(b) / 127.0)),
        (4, 0xa0) => per_note(part, NoteExpression::Pressure(f64::from(data) / f64::from(u32::MAX))),
        // Per-note pitch bend: centre 2^31, ±48 semitones full scale.
        (4, 0x60) => per_note(part, NoteExpression::Tune((f64::from(data) / 2_147_483_648.0 - 1.0) * 48.0)),
        (2 | 4, 0xb0) if a == 120 => {
            let _ = part.runtime.all_sound_off(WIRE);
        }
        (2 | 4, 0xb0) if a == 123 => {
            let _ = part.runtime.all_notes_off(WIRE);
        }
        (2, 0x80 | 0x90 | 0xb0 | 0xd0 | 0xe0) => wire_event(part, status | channel, a, b),
        // Notes, controllers, registered controllers (bend range), pressure, bend.
        (4, 0x80 | 0x90 | 0xb0 | 0xd0 | 0xe0 | 0x20) => wire_packet(part, &[word, data]),
        _ => part.problems.ignored_input += 1,
    }
}

/// `event` into one part.
fn deliver(part: &mut Part, index: usize, held: &mut Vec<Held>, overflow: &mut u64, event: Event) {
    match event {
        Event::NoteOn { note, velocity, tune } => {
            if held.len() == HELD {
                *overflow += 1;
                return;
            }
            // The driver sees the note as MIDI on its own channel.
            let key = u32::from(note.key & 127) << 8 | (velocity.clamp(0.0, 1.0) * 127.0).round().max(1.0) as u32;
            if !articulated(part, &[0x2090_0000 | u32::from(note.channel & 15) << 16 | key]) {
                return;
            }
            let input = host_input(note, part.mpe_zone);
            if let Some(script) = part.script.as_mut() {
                // The scripts play the part's notes: the physical one is silent.
                let velocity = velocity.clamp(0.0, 1.0);
                match part.runtime.note_on(input, note.key, velocity) {
                    Ok(id) => {
                        held.push(Held { part: index, note, input, id });
                        match script.note_on(&mut part.runtime, id, note.key, velocity) {
                            Err(sampler_core::Error::Capacity) => part.problems.capacity_drops += 1,
                            _ => {}
                        }
                    }
                    Err(sampler_core::Error::Capacity) => part.problems.capacity_drops += 1,
                    Err(_) => {}
                }
                return;
            }
            let id = match part.mpe.trigger(&mut part.runtime, input.channel, input, velocity.clamp(0.0, 1.0)) {
                Ok(id) => id,
                Err(ApplyError::Core(sampler_core::Error::Capacity)) => {
                    part.problems.capacity_drops += 1;
                    return;
                }
                Err(_) => return,
            };
            held.push(Held { part: index, note, input, id });
            if tune != 0.0 {
                express(&mut part.runtime, id, |e| e.pitch_semitones += tune);
            }
        }
        Event::NoteOff(pattern) | Event::Choke(pattern) => {
            for h in held.iter().filter(|h| h.part == index && pattern.matches(h.note)) {
                if let Some(script) = part.script.as_mut() {
                    let _ = script.note_off(&mut part.runtime, h.note.key);
                }
                let _ = part.runtime.note_off(h.input, None);
            }
        }
        Event::Expression(pattern, expression) => {
            for h in held.iter().filter(|h| h.part == index && pattern.matches(h.note)) {
                note_expression(part, h.id, expression);
            }
        }
        Event::Ump(words) => packet(part, index, held, words),
    }
}

impl V2Core {
    pub fn with_parts(parts: usize, sample_rate: f64) -> Self {
        let mut mix = Mix::default();
        mix.parts.resize(parts.max(mix.parts.len()), Default::default());
        let mut peaks = Peaks::default();
        peaks.parts.resize(parts.max(peaks.parts.len()), [0.0; 2]);
        Self {
            parts: (0..parts).map(|_| None).collect(),
            rate: sample_rate,
            mix,
            held: Vec::with_capacity(HELD),
            overflow: 0,
            buses: Box::new([[[0.0; MAX_BLOCK]; 2]; BUSES]),
            written: [false; BUSES],
            scratch: Box::new([[0.0; 2]; MAX_BLOCK]),
            direct: Box::new([[[0.0; 2]; MAX_BLOCK]; BUSES]),
            tap: None,
            tapped: Box::new([0.0; MAX_BLOCK]),
            peaks,
        }
    }

    /// Adopt larger worker-prepared storage, keeping every playing part.
    /// The replaced storage stays in `grown` to be dropped off audio.
    pub fn adopt(&mut self, grown: &mut Self) {
        for (old, new) in self.parts.iter_mut().zip(&mut grown.parts) {
            std::mem::swap(old, new);
        }
        std::mem::swap(&mut self.parts, &mut grown.parts);
        for (old, new) in self.mix.parts.iter().zip(&mut grown.mix.parts) {
            *new = *old;
        }
        std::mem::swap(&mut self.mix.parts, &mut grown.mix.parts);
        for (old, new) in self.peaks.parts.iter().zip(&mut grown.peaks.parts) {
            *new = *old;
        }
        std::mem::swap(&mut self.peaks.parts, &mut grown.peaks.parts);
    }

    fn reaches(&self, part: usize, port: u8, channel: Option<u8>) -> bool {
        self.mix.parts.get(part).is_some_and(|c| {
            c.port == port && (c.mpe || c.channel < 0 || channel.is_none_or(|channel| c.channel == i16::from(channel)))
        })
    }

    fn deliver(&mut self, part: usize, event: Event) {
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        deliver(p, part, &mut self.held, &mut self.overflow, event);
    }
}

impl Core for V2Core {
    /// `None` empties the part.
    type Prepared = Option<Box<Part>>;
    type Retired = Retired;

    fn parts(&self) -> usize {
        self.parts.len()
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn reset(&mut self, sample_rate: f64) {
        // ponytail: parts keep their prepared rate; the shell reloads them on a rate change.
        self.rate = sample_rate;
        self.panic();
    }

    fn panic(&mut self) {
        for p in self.parts.iter_mut().flatten() {
            p.runtime.panic();
        }
    }

    fn install(&mut self, part: usize, mut prepared: Option<Box<Part>>) -> Retired {
        let Some(slot) = self.parts.get_mut(part) else { return Retired(prepared) };
        for held in self.held.iter_mut().filter(|h| h.part == part) {
            held.part = ORPHAN;
        }
        if let (Some(p), Some(c)) = (prepared.as_mut(), self.mix.parts.get(part)) {
            p.configure(c);
        }
        Retired(std::mem::replace(slot, prepared))
    }

    fn begin_block(&mut self, _block: &BlockInfo) {}

    fn event(&mut self, port: u8, event: Event) {
        let channel = event.channel();
        for part in 0..self.parts.len() {
            if self.reaches(part, port, channel) {
                self.deliver(part, event);
            }
        }
    }

    fn play(&mut self, part: usize, event: Event) {
        self.deliver(part, event);
    }

    fn key_held(&self, channel: u8, key: u8) -> bool {
        self.held.iter().any(|h| h.part != ORPHAN && h.note.channel == channel && h.note.key == key)
    }

    fn render(&mut self, frames: usize) -> Rendered<'_> {
        let n = frames.min(MAX_BLOCK);
        for bus in self.buses.iter_mut() {
            bus[0][..n].fill(0.0);
            bus[1][..n].fill(0.0);
        }
        self.written = [false; BUSES];
        self.tapped[..n].fill(0.0);
        let solo = self.mix.parts.iter().take(self.parts.len()).any(|c| c.solo);
        for (index, part) in self.parts.iter_mut().enumerate() {
            let Some(part) = part else { continue };
            let pairs = |direct: u32| (0..BUSES).filter(move |pair| direct & 1 << pair != 0);
            for pair in pairs(part.direct) {
                self.direct[pair][..n].fill([0.0; 2]);
            }
            // About 128 instructions per frame, so a long block keeps its script
            // throughput per second; never below the 64-frame measured 8192.
            let fuel = (n * 128).max(8192);
            if part.runtime.behavior_block_fuel() != fuel {
                part.runtime.set_behavior_block_fuel(fuel);
            }
            if let Some(script) = part.script.as_mut() {
                let _ = script.wake(&mut part.runtime);
                // What the scripts generated plays into the part.
                let mut midi = [None; 64];
                let mut n = 0;
                script.drain_midi(|out| {
                    if n < midi.len() {
                        midi[n] = Some(out);
                        n += 1;
                    }
                });
                for out in midi.iter().flatten() {
                    wire_event(part, out.status, out.a, out.b);
                }
            }
            if let Some(horizon) = part.horizon {
                // Pending pages play silent and count as underruns.
                let _ = part.runtime.service_streaming(horizon);
            }
            let out = &mut self.scratch[..n];
            let mut outs: [&mut [Frame]; BUSES] = self.direct.each_mut().map(|d| &mut d[..n]);
            if part.runtime.render_split(out, &mut outs).is_err() {
                continue;
            }
            if let Some((program, error)) = part.runtime.take_fault() {
                part.problems.fault_program = program as u64 + 1;
                part.problems.fault_error = sampler_core::Error::ALL.iter().position(|e| *e == error).unwrap_or(0) as u64;
            }
            if let Some(silent) = part.runtime.take_silent_note() {
                part.problems.silent_notes += 1;
                part.problems.silent = silent.pack();
            }
            let c = self.mix.parts[index];
            if c.mute || solo && !c.solo {
                continue;
            }
            // Nodes routed to their own pairs leave the instrument's fader.
            for pair in pairs(part.direct) {
                self.written[pair] = true;
                let [bl, br] = &mut self.buses[pair];
                for ((ol, or), [l, r]) in bl[..n].iter_mut().zip(&mut br[..n]).zip(self.direct[pair][..n].iter()) {
                    *ol += l;
                    *or += r;
                }
            }
            let [gl, gr] = balance(c.gain, c.pan);
            let mut level = [0f32; 2];
            for [l, r] in out.iter_mut() {
                *l *= gl;
                *r *= gr;
                level = [level[0].max(l.abs()), level[1].max(r.abs())];
            }
            let meter = &mut self.peaks.parts[index];
            *meter = [meter[0].max(level[0]), meter[1].max(level[1])];
            if level == [0.0; 2] {
                continue;
            }
            if self.tap == Some(index) {
                for (t, [l, r]) in self.tapped[..n].iter_mut().zip(out.iter()) {
                    *t = (l + r) * 0.5;
                }
            }
            let aux = usize::from(c.aux) < BUSES && c.aux != c.output && c.aux_gain != 0.0;
            for (bus, gain) in [(usize::from(c.output), 1.0), (usize::from(c.aux), c.aux_gain)].into_iter().take(1 + usize::from(aux)) {
                let bus = bus.min(BUSES - 1);
                self.written[bus] = true;
                let [bl, br] = &mut self.buses[bus];
                for ((ol, or), [l, r]) in bl[..n].iter_mut().zip(&mut br[..n]).zip(out.iter()) {
                    *ol += l * gain;
                    *or += r * gain;
                }
            }
        }
        let solo = self.mix.buses.iter().any(|c| c.solo);
        for (bus, c) in self.mix.buses.iter().enumerate().filter(|(bus, _)| self.written[*bus]) {
            let gains = if c.mute || solo && !c.solo { [0.0; 2] } else { balance(c.gain, c.pan) };
            let meter = &mut self.peaks.buses[bus];
            for ((signal, g), m) in self.buses[bus].iter_mut().zip(gains).zip(meter.iter_mut()) {
                if g != 1.0 {
                    signal[..n].iter_mut().for_each(|x| *x *= g);
                }
                *m = m.max(peak(&signal[..n]));
            }
        }
        Rendered { buses: &self.buses, live: self.written }
    }

    fn owns(&self, note: HostNote) -> bool {
        self.held.iter().any(|h| h.part != ORPHAN && h.note == note)
    }

    fn end_block(&mut self, _frames: usize, end: &mut dyn FnMut(HostNote) -> bool) -> u64 {
        let Self { parts, held, .. } = self;
        // A layered note ends once its last part lets it go.
        let last = |held: &[Held], at: usize| held.iter().filter(|h| h.note == held[at].note).count() == 1;
        // Notes of replaced parts end now; their sound went with the part.
        let mut i = 0;
        while i < held.len() {
            if held[i].part != ORPHAN {
                i += 1;
            } else if !held[i].note.clap || !last(held, i) || end(held[i].note) {
                held.swap_remove(i);
            } else {
                return 1;
            }
        }
        let mut refused = 0;
        for (index, part) in parts.iter_mut().enumerate() {
            let Some(part) = part else { continue };
            part.runtime.flush_ended(|input| {
                let Some(at) = held.iter().position(|h| h.part == index && h.input == input) else { return true };
                if held[at].note.clap && last(held, at) && !end(held[at].note) {
                    refused = 1;
                    return false;
                }
                held.swap_remove(at);
                true
            });
            if refused > 0 {
                break;
            }
        }
        refused
    }

    fn set_mix(&mut self, mix: &Mix) {
        // Field-wise so the parts vector keeps its audio-thread allocation.
        for (to, from) in self.mix.parts.iter_mut().zip(&mix.parts) {
            *to = *from;
        }
        self.mix.buses = mix.buses;
        for (index, (p, c)) in self.parts.iter_mut().zip(&self.mix.parts).enumerate() {
            let Some(p) = p else { continue };
            p.configure(c);
            p.mix_nodes(mix.nodes.get(index).map_or(&[], Vec::as_slice));
        }
    }

    fn bus_ports(&self) -> [u8; BUSES] {
        self.mix.buses.map(|b| b.port)
    }

    fn set_tap(&mut self, part: Option<usize>) {
        self.tap = part;
    }

    fn tapped(&self, frames: usize) -> Option<&[f32]> {
        self.tap.map(|_| &self.tapped[..frames.min(MAX_BLOCK)])
    }

    fn peaks_mut(&mut self) -> &mut Peaks {
        &mut self.peaks
    }

    fn take_node_peaks(&mut self, part: usize, each: &mut dyn FnMut(usize, [f32; 2])) {
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        let nodes = &p.buses;
        p.runtime.take_bus_peaks(|bus, peak| {
            if let Some(node) = nodes.iter().position(|&b| b == Some(bus)) {
                each(node, peak);
            }
        });
    }

    fn take_effects(&mut self, part: usize, each: &mut dyn FnMut(usize, &sampler_core::Effect) -> bool) {
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        p.runtime.drain_effects(|e| e.instance.is_none_or(|i| each(usize::from(i.0), e)));
    }

    fn voices(&self) -> Voices {
        let active = self.parts.iter().flatten().map(|p| p.runtime.voice_count()).sum();
        Voices { active, audible: active, dropouts: self.overflow }
    }

    fn problems(&self, part: usize) -> RuntimeProblems {
        let Some(Some(p)) = self.parts.get(part) else { return RuntimeProblems::default() };
        let stats = p.runtime.stats();
        RuntimeProblems {
            nonfinite: stats.nonfinite_frames,
            underruns: stats.stream_underruns,
            capacity_drops: p.problems.capacity_drops + stats.voice_drops,
            stolen_voices: p.runtime.steals(),
            refused_starts: stats.refused_starts,
            ..p.problems
        }
    }

    fn articulation(&self, part: usize) -> Option<usize> {
        let p = self.parts.get(part)?.as_ref()?;
        let default = p.articulations?;
        if let Some(keys) = &p.tap_keys {
            // A script holds the selection: the last switch key, tapped or pressed.
            let tapped = p.articulator.as_ref().and_then(Articulator::selected);
            return Some(tapped.and_then(|k| keys.iter().position(|&f| f == Some(k))).unwrap_or(default));
        }
        let id = p.runtime.articulation(p.runtime.performance(0).ok()?).ok()? as usize;
        // Undo the runtime's numbering: the default is 0, those before it shift up.
        Some(match id {
            0 => default,
            id if id <= default => id - 1,
            id => id,
        })
    }

    fn clock(&self, part: usize) -> u64 {
        self.parts.get(part).and_then(Option::as_ref).map_or(0, |p| p.runtime.now())
    }

    fn latency(&self) -> u32 {
        0
    }

    fn set_control(&mut self, part: usize, control: sampler_ui_ir::ControlId, value: f64) -> bool {
        let Some(Some(p)) = self.parts.get_mut(part) else { return false };
        let rt = &mut p.runtime;
        let (plan, id) = (rt.active_plan(), sampler_core::ControlId(control.0));
        let Ok(ControlDefinition { domain, .. }) = rt.control_definition(plan, id) else { return false };
        let value = match domain {
            ControlDomain::Integer { min, max } => ControlValue::Integer((value.round() as i64).clamp(min, max)),
            ControlDomain::Real { min, max } => ControlValue::Real(value.clamp(min, max)),
            ControlDomain::Toggle => ControlValue::Toggle(value >= 0.5),
        };
        let Ok(performance) = rt.performance(0) else { return false };
        let context = ControlContext { performance, origin: WIRE, channels: 1 };
        rt.invoke_control(context, plan, None, ControlWrite { id, value }).is_ok()
    }

    fn control_value(&self, part: usize, control: sampler_ui_ir::ControlId) -> Option<f64> {
        let rt = &self.parts.get(part)?.as_ref()?.runtime;
        rt.control_value(rt.active_plan(), sampler_core::ControlId(control.0)).ok().map(number)
    }
}

fn number(value: ControlValue) -> f64 {
    match value {
        ControlValue::Integer(n) => n as f64,
        ControlValue::Real(r) => r,
        ControlValue::Toggle(on) => f64::from(u8::from(on)),
    }
}

/// Prepares [`V2Core`] parts from Kontakt instruments and WAV files.
#[derive(Default)]
pub struct V2Loader;

/// Voice-rendering threads per part: `KONTRA_THREADS` (`auto` or a count)
/// wins, then the player's setting; one (the audio thread alone) otherwise.
fn render_threads(request: &LoadRequest) -> Threads {
    match std::env::var("KONTRA_THREADS").as_deref() {
        Ok("auto") => Threads::Auto,
        Ok(n) => Threads::Fixed(n.parse().unwrap_or(1)),
        Err(_) => match request.threads {
            Some(super::ThreadChoice::Auto) => Threads::Auto,
            Some(super::ThreadChoice::Fixed(n)) => Threads::Fixed(n),
            None => Threads::Fixed(1),
        },
    }
}

/// Memory a part preallocates for per-voice state. Voices start sized to this,
/// not to a fixed polyphony, and the pool doubles off the audio thread (see
/// `Grower`) past three quarters full, up to `GROWTH` times as many if memory allows. A note is
/// refused only when that is exhausted too, and it is counted
/// (`RuntimeStats::voice_drops`).
const VOICE_BUDGET: usize = 256 << 20;
const MIN_VOICES: usize = 512;
const MAX_VOICES: usize = 16384;
const GROWTH: usize = 8;
/// Per-voice bytes beyond the plan's state: voice slot, activity bit, parallel scratch.
const VOICE_OVERHEAD: usize = 4096;

/// Capacities of a part, sized for its plan's script state and voice cost, and
/// the voice count the pool may grow to. Notes, families and decisions are
/// sized for that ceiling so growing voices is not capped by them.
fn limits(plan: &Prepared) -> (Limits, usize) {
    let voices = (VOICE_BUDGET / plan.voice_state_bytes().max(1)).clamp(MIN_VOICES, MAX_VOICES);
    let ceiling = voices * GROWTH;
    // Notes outlive their voices only in release, and each holds a few voices.
    // Capped: script cells are allocated per note.
    let notes = (ceiling / 4).clamp(NOTES, 16384);
    let limits = Limits {
        families: (ceiling / 2).clamp(256, 32768),
        decisions: (ceiling / 2).clamp(256, 32768),
        ..Limits::for_plan(plan, notes, voices)
    };
    (limits, ceiling)
}

fn is_wav(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

fn is_kontakt(path: &Path) -> bool {
    path.extension().is_some_and(|e| ["nki", "nkm", "nkb", "nksn"].iter().any(|k| e.eq_ignore_ascii_case(k)))
}

fn is_uvi(path: &Path) -> bool {
    path.ancestors().any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
        || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("uvip"))
}

fn unsupported(path: &Path) -> CoreError {
    if is_kontakt(path) || is_uvi(path) || is_wav(path) {
        CoreError::Invalid("unreadable instrument".into())
    } else {
        CoreError::Unsupported("translating this instrument format to sampler-core")
    }
}

fn insert_names(instrument: &ir::Instrument, chain: Option<ir::ChainRef>) -> Vec<String> {
    let Some(chain) = chain.and_then(|c| instrument.chains.get(c.0)) else { return Vec::new() };
    chain
        .pre_amplitude
        .iter()
        .chain(&chain.post_amplitude)
        .map(|p| match p {
            ir::Processor::Gain(_) => "Gain",
            ir::Processor::Gainer { .. } => "Gainer",
            ir::Processor::StereoModeller { .. } => "Stereo Modeller",
            ir::Processor::Pan(_) => "Pan",
            ir::Processor::StereoMatrix(_) => "Stereo",
            ir::Processor::Reverb(_) => "Reverb",
            ir::Processor::Compressor(_) => "Compressor",
            ir::Processor::Rectify(_) => "Rectify",
            ir::Processor::Daft(_) => "Daft",
            ir::Processor::Branch { .. } => "Branch",
            ir::Processor::Convolution { .. } => "Convolution",
            ir::Processor::Filter(_) => "Filter",
            ir::Processor::Delay { .. } => "Delay",
            ir::Processor::Mix { .. } => "Mix",
        })
        .map(String::from)
        .collect()
}

/// Give every group a bus of its own after the source's buses, so each is a
/// mixer node, and describe the result as the part's tree: node 0 is the
/// instrument, node `n > 0` is runtime bus `n - 1`.
fn nest(instrument: &mut ir::Instrument) -> MixTree {
    let node = |output: ir::Output| match output {
        ir::Output::Master => 0,
        ir::Output::Bus(bus) => bus.0 + 1,
    };
    let mut tree = MixTree::instrument(&instrument.name);
    for bus in &instrument.buses {
        tree.nodes.push(MixNode {
            name: bus.name.clone(),
            kind: NodeKind::Bus,
            parent: Some(node(bus.output)),
            inserts: insert_names(instrument, bus.chain),
            sends: bus.sends.iter().map(|s| (node(s.to), s.gain.linear() as f32)).collect(),
        });
    }
    // Microphone positions: a source bus the groups already play through, else
    // a bus made for the position; groups play through their mic.
    let mut mic_of = vec![None; instrument.groups.len()];
    for (name, groups) in super::mics::infer(instrument) {
        let existing = match instrument.groups[groups[0]].output {
            ir::Output::Bus(b) if instrument.buses[b.0].name == name => Some(b.0 + 1),
            _ => None,
        };
        let at = existing.unwrap_or_else(|| {
            let output = instrument.groups[groups[0]].output;
            instrument.buses.push(ir::Bus { name: name.clone(), chain: None, sends: Vec::new(), output, gain: ir::Gain::UNITY });
            tree.nodes.push(MixNode { name, kind: NodeKind::Mic, parent: Some(node(output)), inserts: Vec::new(), sends: Vec::new() });
            instrument.buses.len()
        });
        tree.nodes[at].kind = NodeKind::Mic;
        for g in groups {
            mic_of[g] = Some(at);
        }
    }
    for index in 0..instrument.groups.len() {
        let group = &instrument.groups[index];
        let name = if group.name.is_empty() { format!("Group {}", index + 1) } else { group.name.clone() };
        let output = match mic_of[index] {
            Some(at) => ir::Output::Bus(ir::BusRef(at - 1)),
            None => group.output,
        };
        tree.nodes.push(MixNode {
            name: name.clone(),
            kind: NodeKind::Group,
            parent: Some(node(output)),
            inserts: insert_names(instrument, group.chain),
            sends: Vec::new(),
        });
        let bus = ir::BusRef(instrument.buses.len());
        instrument.buses.push(ir::Bus { name, chain: None, sends: Vec::new(), output, gain: ir::Gain::UNITY });
        instrument.groups[index].output = ir::Output::Bus(bus);
        instrument.tap_group(index, bus);
        let post = instrument.groups[index].tap.as_ref().map_or(&[][..], |t| &t.post[..]);
        let fader = instrument.buses[bus.0].gain.linear();
        let sends = (instrument.buses[bus.0].sends.iter().enumerate())
            .map(|(n, s)| (node(s.to), (s.gain.linear() * if post.contains(&n) { fader } else { 1.0 }) as f32))
            .collect();
        tree.nodes.last_mut().expect("pushed above").sends = sends;
    }
    tree
}

/// A plan and, when its samples stream, the runtime's page cache.
type Plan = (Prepared, Option<StreamCache>, Option<ScriptDriver>);

/// A UVI program's Lua scripts, driving the part's runtime from their own thread.
pub type ScriptDriver = sampler_uvi::scripted::Driver<sampler_uvi::scripted::ScriptThread>;

/// The instrument a snapshot was saved from: `<name>.nki` somewhere under a
/// folder above the snapshot, named by its metadata or by the snapshot's folder.
fn snapshot_parent(snapshot: &Path, name: &str) -> Option<PathBuf> {
    fn find(dir: &Path, wanted: &[String], depth: usize) -> Option<PathBuf> {
        let mut folders = Vec::new();
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki"))
                && path.file_stem().is_some_and(|s| wanted.iter().any(|w| s.eq_ignore_ascii_case(w.as_str())))
            {
                return Some(path);
            }
        }
        if depth == 0 {
            return None;
        }
        folders.sort();
        folders.iter().find_map(|d| find(d, wanted, depth - 1))
    }
    let folder = snapshot.parent()?.file_name()?.to_string_lossy().into_owned();
    let wanted: Vec<String> = [name.to_owned(), folder].into_iter().filter(|n| !n.is_empty()).collect();
    snapshot.ancestors().skip(1).take(4).find_map(|dir| find(dir, &wanted, 3))
}

fn kontakt(
    request: &LoadRequest,
    progress: &mut dyn FnMut(Progress),
    canceled: &(dyn Fn() -> bool + Sync),
) -> Result<Loaded<Plan>, CoreError> {
    let load = |e: sampler_kontakt::LoadError| match e {
        sampler_kontakt::LoadError::Canceled => CoreError::Canceled,
        e => CoreError::Load((&e).into()),
    };
    let extension = |e: &str| request.path.extension().is_some_and(|x| x.eq_ignore_ascii_case(e));
    // A snapshot is the saved state of an instrument found beside it in the library.
    let snapshot = if extension("nksn") {
        let state = sampler_kontakt::read_snapshot(&request.path).map_err(load)?;
        let parent = snapshot_parent(&request.path, &state.instrument).ok_or_else(|| {
            CoreError::Load(LoadFailure::message(format!("no instrument \"{}\" found for snapshot", state.instrument)))
        })?;
        Some((parent, state))
    } else {
        None
    };
    let path = snapshot.as_ref().map_or(&request.path, |(parent, _)| parent);
    let mut source = match &snapshot {
        Some((parent, state)) => sampler_kontakt::read_with_snapshot(parent, state),
        None if extension("nkm") => sampler_kontakt::read_program(path, request.program as usize),
        None => sampler_kontakt::read(path),
    }
    .map_err(load)?;
    let mut report = LoadReport::of(&source.instrument, &request.path, source.locations.len());
    let tree = nest(&mut source.instrument);
    let options = sampler_kontakt::Options {
        rate: request.sample_rate as u32,
        library: Some(path.clone()),
        mpe: request.mpe.then(|| sampler_core::lower::MpeDefaults::for_instrument(&source.instrument)),
        dynamics_start: request.dynamics_start,
        control_values: request.control_values.iter().filter(|(_, value)| value.is_finite()).map(|&(id, value)| (sampler_core::ControlId(id.0), value.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)).collect(),
        ..Default::default()
    };
    let progress = |p: sampler_kontakt::Progress<'_>| {
        progress(Progress(match p {
            sampler_kontakt::Progress::Translated { .. } => 50,
            sampler_kontakt::Progress::Decoding { done, total, .. } => (100 + 800 * done / total.max(1)) as u16,
            sampler_kontakt::Progress::Lowering => 950,
        }))
    };
    if canceled() {
        return Err(CoreError::Canceled);
    }
    let streamed = sampler_kontakt::load_read_streamed(source, &options, &Default::default(), progress).map_err(load)?;
    let sampler_kontakt::Streamed { loaded, assets, cache, streamer, report: stream } = streamed;
    report.decoded.full_bytes = stream.full_bytes;
    report.decoded.dynamics = loaded.dynamics().iter().map(|&(cc, v)| (cc, (v * 127.).round().clamp(0., 127.) as u8)).collect();
    report.decoded.needs_controller = loaded.needs_controller();
    // Loading adds what it found unplayable (samples, keys) and scripts that failed.
    report.missing = loaded.instrument.unsupported.iter().map(Missing::from).collect();
    report.decoded.zones = loaded.instrument.zones.len();
    report.decoded.keys = super::report::key_bits(&loaded.instrument);
    report.decoded.samples = loaded.plan.sample_count();
    Ok(Loaded {
        part: (loaded.plan, Some(cache), None),
        tree,
        report,
        interfaces: loaded.interfaces,
        controls: Vec::new(),
        instrument: Some(Arc::new(loaded.instrument)),
        scripts: ScriptUi { views: loaded.scripts, resources: loaded.resources },
        stream: Some(Arc::new(Stream { streamer, assets, report: stream })),
    })
}

/// A UVI program (loose or in a bank): its layers become mixer nodes as groups do. Samples
/// stream from the bank or file; its Lua scripts run on their own thread.
fn uvi(request: &LoadRequest) -> Result<Loaded<Plan>, CoreError> {
    let load = |e: &dyn std::fmt::Display| CoreError::Load(LoadFailure::message(e));
    let mut t = sampler_uvi::translate_path(&request.path).map_err(|e| load(&*e))?;
    let rate = request.sample_rate as u32;
    let attached = t.attach_script(rate, sampler_uvi::script::Config::realtime()).map_err(|e| load(&e))?;
    let mut report = LoadReport::of(&t.instrument, &request.path, t.locations.len());
    let tree = nest(&mut t.instrument);
    let streamed = sampler_uvi::assemble_translated_streamed(t, rate, &Default::default()).map_err(|e| load(&*e))?;
    let sampler_kontakt::Streamed { mut loaded, assets, cache, streamer, report: stream } = streamed;
    report.decoded.full_bytes = stream.full_bytes;
    let driver = attached.map(|a| {
        // Loading reports a script it has no frontend for; this one runs.
        loaded.instrument.unsupported.retain(|u| !(u.feature == "script" && u.value.contains("no frontend")));
        loaded.interfaces.push(a.interface);
        a.driver
    });
    report.missing = loaded.instrument.unsupported.iter().map(Missing::from).collect();
    report.decoded.zones = loaded.instrument.zones.len();
    report.decoded.keys = super::report::key_bits(&loaded.instrument);
    report.decoded.samples = loaded.plan.sample_count();
    Ok(Loaded {
        part: (loaded.plan, Some(cache), driver),
        tree,
        report,
        interfaces: loaded.interfaces,
        controls: Vec::new(),
        instrument: Some(Arc::new(loaded.instrument)),
        scripts: ScriptUi { views: loaded.scripts, resources: loaded.resources },
        stream: Some(Arc::new(Stream { streamer, assets, report: stream })),
    })
}

fn read_wav(path: &Path) -> Result<(u32, Box<[Frame]>), CoreError> {
    let mut reader = hound::WavReader::open(path).map_err(|e| CoreError::Load(LoadFailure::message(e)))?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 {
        return Err(CoreError::Invalid("WAV without channels".into()));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|s| s as f32 * scale)).collect::<Result<_, _>>()
        }
    }
    .map_err(|e| CoreError::Load(LoadFailure::message(e)))?;
    let frames = samples.chunks_exact(channels).map(|f| [f[0], f[channels.min(2) - 1]]).collect();
    Ok((spec.sample_rate, frames))
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn wav(request: &LoadRequest) -> Result<Loaded<Plan>, CoreError> {
    let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
    let rate = request.sample_rate as u32;
    let (source_rate, frames) = read_wav(&request.path)?;
    let pcm = Pcm::new(source_rate, frames).map_err(core)?;
    let release = (0.05 * f64::from(rate)) as u32;
    let region = Region {
        sample: 0,
        // The resampler steps at most 16x up: four octaves above the root.
        key_low: 0,
        key_high: 108,
        root_key: Some(60),
        velocity_low: 0.0,
        velocity_high: 1.0,
        gain: 1.0,
        envelope: Envelope::new(0, 0, 0, 1.0, release).map_err(core)?,
        playback: Playback::default(),
    };
    let plan = Prepared::new(rate, vec![pcm], vec![region], 128).map_err(core)?;
    let name = stem(&request.path);
    let mut report = LoadReport { name: name.clone(), path: request.path.display().to_string(), ..Default::default() };
    report.decoded.format = "WAV".into();
    report.decoded.zones = 1;
    report.decoded.samples = 1;
    report.decoded.keys = super::report::range_bits(0, 108);
    Ok(Loaded {
        part: (plan, None, None),
        tree: MixTree::instrument(&name),
        report,
        interfaces: Vec::new(),
        controls: Vec::new(),
        instrument: None,
        scripts: ScriptUi::default(),
        stream: None,
    })
}

/// Stack for the loader worker: lowering moves large plans by value and the
/// caller's thread (a UI or test thread) may only have 2 MB.
const LOADER_STACK: usize = 16 << 20;

impl V2Loader {
    fn prepare_on_worker(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Loaded<Option<Box<Part>>>, CoreError> {
        let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
        let Loaded { part: (prepared, cache, script), tree, mut report, interfaces, instrument, scripts, stream, .. } = if is_kontakt(&request.path) {
            kontakt(request, progress, canceled)?
        } else if is_uvi(&request.path) {
            uvi(request)?
        } else if is_wav(&request.path) {
            wav(request)?
        } else {
            return Err(unsupported(&request.path));
        };
        if canceled() {
            return Err(CoreError::Canceled);
        }
        let mut timbre = None;
        if request.mpe {
            let defaults = match &instrument {
                Some(i) if is_kontakt(&request.path) => sampler_core::lower::MpeDefaults::for_instrument(i),
                _ => Default::default(),
            };
            if let sampler_core::lower::TimbreTarget::Controller(cc) = defaults.timbre {
                timbre = Some(cc);
            }
            report.decoded.mpe = super::report::mpe_summary(&defaults);
        }
        let controls = prepared
            .controls()
            .iter()
            .map(|c| (sampler_ui_ir::ControlId(c.id.0), number(c.default)))
            .collect();
        let (limits, ceiling) = limits(&prepared);
        let per_voice = prepared.voice_state_bytes() + VOICE_OVERHEAD;
        report.decoded.script_callbacks = limits.behaviors;
        let voices = limits.voices;
        let (runtime, control) = Runtime::with_plan_updates(prepared, limits, 2, 1).map_err(core)?;
        let mut runtime = runtime.with_threads(render_threads(request));
        // A source whose first window is not resident starts silent and fades in
        // rather than being refused NotReady.
        runtime.set_cold_starts(true);
        // A chord's script work spreads over blocks: 30 notes of a 14k-instruction
        // callback measured 8.0 ms in one block unlimited, 0.70 ms at this cap
        // (sampler-perf dense-strings, 64-frame blocks).
        // render() rescales it to 128 per frame for longer blocks.
        runtime.set_behavior_block_fuel(8192);
        let streams = cache.is_some();
        if let Some(cache) = cache {
            runtime = runtime.with_stream_cache(cache);
        }
        // Full polyphony steals (released, then quietest) rather than refusing notes.
        runtime.set_voice_stealing(Some(Stealing::for_limits(runtime.sample_rate(), voices))).map_err(core)?;
        if runtime.bus_count() + 1 != tree.nodes.len() && tree.nodes.len() > 1 {
            return Err(CoreError::Invalid(format!(
                "{} mixer nodes for {} runtime buses",
                tree.nodes.len() - 1,
                runtime.bus_count()
            )));
        }
        let grower = Grower::start(&mut runtime, control, ceiling, per_voice)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let mut part = Part::new(runtime, tree.clone())?;
        part.mpe.set_timbre_controller(timbre);
        part.grower = Some(grower);
        part.script = script.map(Box::new);
        if let Some(inst) = instrument.as_deref() {
            part.set_drivers(inst);
        }
        let articulated = instrument.as_deref().filter(|i| !i.articulations.is_empty());
        part.articulations = articulated.map(|i| i.articulations.iter().position(|a| a.default).unwrap_or(0));
        part.tap_keys = articulated
            .filter(|i| i.switching.owner == ir::SwitchOwner::Behavior)
            .map(|i| i.articulations.iter().map(|a| a.switch_keys.first().copied()).collect());
        if streams && let Some(stream) = &stream {
            // Heads bound only starts; running voices request a page ahead.
            part.horizon = Some((stream.report.head_frames.max(PAGE_FRAMES) + MAX_BLOCK) as u32);
            part._stream = Some(stream.clone());
        }
        progress(Progress::DONE);
        Ok(Loaded { part: Some(Box::new(part)), tree, report, interfaces, controls, instrument, scripts, stream })
    }
}

impl CoreLoader for V2Loader {
    type Core = V2Core;

    /// Prepares on a worker with an explicit stack; progress is relayed to the
    /// caller's (non-`Send`) callback over a channel.
    fn prepare(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Loaded<Option<Box<Part>>>, CoreError> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = std::thread::Builder::new()
                .name("sampler-load".into())
                .stack_size(LOADER_STACK)
                .spawn_scoped(scope, move || {
                    self.prepare_on_worker(request, &mut |p| drop(tx.send(p)), canceled)
                })
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
            // The sender drops when the worker ends, closing the channel.
            for p in rx {
                progress(p);
            }
            worker.join().unwrap_or_else(|_| Err(CoreError::Invalid("loader panicked".into())))
        })
    }

    fn describe(&self, path: &Path, _program: u32) -> Result<Description, CoreError> {
        if is_kontakt(path) {
            let parent = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nksn")).then(|| {
                let state = sampler_kontakt::read_snapshot(path).ok()?;
                snapshot_parent(path, &state.instrument)
            });
            let path = parent.flatten().unwrap_or_else(|| path.to_path_buf());
            let instrument = sampler_kontakt::read(&path).map_err(|e| CoreError::Load(LoadFailure::message(e)))?.instrument;
            return Ok(Description {
                name: instrument.name.clone(),
                zones: instrument.zones.len(),
                scripts: instrument.behaviors.len(),
                missing: instrument.unsupported.iter().map(Missing::from).collect(),
            });
        }
        if !is_wav(path) {
            return Err(unsupported(path));
        }
        hound::WavReader::open(path).map_err(|e| CoreError::Load(LoadFailure::message(e)))?;
        Ok(Description { name: stem(path), zones: 1, scripts: 0, missing: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::event::HostPattern;

    fn sine(path: &Path) {
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..48000 {
            w.write_sample(((i as f32 * 440.0 / 48000.0 * std::f32::consts::TAU).sin() * 16000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn loud(r: &Rendered<'_>, bus: usize, frames: usize) -> bool {
        r.live[bus] && r.buses[bus][0][..frames].iter().any(|x| x.abs() > 0.01)
    }

    fn on(note: HostNote) -> Event {
        Event::NoteOn { note, velocity: 100.0 / 127.0, tune: 0.0 }
    }

    fn load(path: &Path) -> Option<Box<Part>> {
        let request = LoadRequest { path: path.into(), sample_rate: 48000.0, ..Default::default() };
        V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap().part
    }

    #[test]
    fn host_note_plays_through_the_trait_and_ends_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sine.wav");
        sine(&path);
        let loader = V2Loader;
        assert_eq!(loader.describe(&path, 0).unwrap().name, "sine");
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let mut done = None;
        let loaded = loader.prepare(&request, &mut |p| done = Some(p), &|| false).unwrap();
        assert_eq!(done, Some(Progress::DONE));
        assert_eq!(loaded.tree.nodes.len(), 1);
        assert_eq!(loaded.report.decoded.format, "WAV");
        let part = loaded.part.as_ref().unwrap();
        assert!(part.runtime.voice_stealing().is_some(), "full polyphony steals");

        let mut core = V2Core::with_parts(2, 48000.0);
        assert!(core.install(0, loaded.part).0.is_none());
        let note = HostNote { port: 0, channel: 0, key: 60, id: 7, clap: true };
        core.begin_block(&BlockInfo { frames: 64, ..Default::default() });
        core.event(0, on(note));
        assert!(core.owns(note));
        assert!(core.key_held(0, 60));
        assert!(loud(&core.render(64), 0, 64));
        assert_eq!(core.voices().active, 1);
        assert_eq!(core.end_block(64, &mut |_| panic!("still held")), 0);

        core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: 60, id: -1, clap: true }));
        let mut ended = Vec::new();
        for _ in 0..100 {
            core.render(64);
            core.end_block(64, &mut |n| {
                ended.push(n);
                true
            });
        }
        assert_eq!(ended, [note]);
        assert!(!core.owns(note));
        assert!(!loud(&core.render(64), 0, 64));
    }

    #[test]
    fn refused_note_end_is_retried_and_replaced_runtimes_orphan_their_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        let note = HostNote { port: 0, channel: 0, key: 64, id: 3, clap: true };
        core.event(0, on(note));
        assert!(core.install(0, load(&path)).0.is_some());
        assert!(!core.owns(note));
        assert_eq!(core.end_block(64, &mut |_| false), 1);
        let mut ended = Vec::new();
        assert_eq!(core.end_block(64, &mut |n| { ended.push(n); true }), 0);
        assert_eq!(ended, [note]);
    }

    #[test]
    fn pedals_bends_and_the_mix_reach_host_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        let note = HostNote { port: 0, channel: 3, key: 60, id: 5, clap: true };
        let off = Event::NoteOff(HostPattern { port: -1, channel: -1, key: -1, id: 5, clap: true });
        let expression = |core: &V2Core| {
            let part = core.parts[0].as_ref().unwrap();
            let h = core.held[0];
            part.runtime.expression(part.runtime.expression_id(h.id).unwrap()).unwrap()
        };
        core.event(0, Event::midi1(0xb3, 64, 127));
        core.event(0, on(note));
        core.event(0, Event::midi1(0xe3, 127, 127));
        assert!((expression(&core).pitch_semitones - 2.0).abs() < 1e-3, "bend on any channel moves the part");
        // MIDI 2.0 polyphonic pressure reaches the host note at full precision.
        core.event(0, Event::Ump([0x40a3_3c00, 0x8000_0000]));
        assert_eq!(expression(&core).pressure, 0x8000_0000);
        core.event(0, off);
        for _ in 0..20 {
            core.render(128);
            core.end_block(128, &mut |_| panic!("sustain holds the note"));
        }
        assert!(core.owns(note));

        let mut mix = Mix::default();
        mix.parts[0].pan = 1.0;
        mix.parts[0].tune = -12.0;
        mix.parts[0].aux = 2;
        mix.parts[0].aux_gain = 0.5;
        mix.buses[2].mute = true;
        core.set_mix(&mix);
        assert!((expression(&core).pitch_semitones - -10.0).abs() < 1e-3, "tune adds to the bend");
        let r = core.render(128);
        assert!(r.live[0] && r.live[2]);
        assert!(r.buses[0][0][..128].iter().all(|x| *x == 0.0), "panned hard right");
        assert!(r.buses[0][1][..128].iter().any(|x| x.abs() > 0.01));
        assert!(r.buses[2][1][..128].iter().all(|x| *x == 0.0), "aux bus muted");

        // MIDI 2.0 sustain off, narrowed to the zone's MIDI 1.0.
        core.event(0, Event::Ump([0x40b3_4000, 0]));
        let mut ended = Vec::new();
        for _ in 0..100 {
            core.render(128);
            core.end_block(128, &mut |n| { ended.push(n); true });
        }
        assert_eq!(ended, [note]);
        core.event(0, Event::Ump([0x40c3_0000, 0]));
        assert_eq!(core.problems(0).ignored_input, 1, "program change is counted, not dropped silently");
    }

    #[test]
    fn mpe_member_channels_bend_their_own_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        let mut mix = Mix::default();
        (mix.parts[0].mpe, mix.parts[0].channel, mix.parts[0].bend_range) = (true, 0, 12);
        core.set_mix(&mix);
        core.install(0, load(&path));
        let pitch = |core: &V2Core, at: usize| {
            let part = core.parts[0].as_ref().unwrap();
            let id = part.runtime.expression_id(core.held[at].id).unwrap();
            part.runtime.expression(id).unwrap().pitch_semitones
        };
        core.event(0, on(HostNote { port: 0, channel: 1, key: 60, id: 1, clap: true }));
        core.event(0, on(HostNote { port: 0, channel: 2, key: 64, id: 2, clap: true }));
        core.event(0, Event::midi1(0xe1, 127, 127));
        assert!((pitch(&core, 0) - 12.0).abs() < 0.01, "member bend at the part's range: {}", pitch(&core, 0));
        assert_eq!(pitch(&core, 1), 0.0, "another member's note stays");
        // The manager channel bends the whole zone, at its own range.
        core.event(0, Event::midi1(0xe0, 127, 127));
        assert!(pitch(&core, 1) > 1.0);
    }

    #[test]
    fn a_velocity_driver_selects_and_reports_the_articulation() {
        velocity_driver(ir::SwitchOwner::Native);
    }

    #[test]
    fn a_velocity_driver_reports_what_it_tapped_into_a_script_owned_switch() {
        velocity_driver(ir::SwitchOwner::Behavior);
    }

    fn velocity_driver(owner: ir::SwitchOwner) {
        // Key 60 in three articulations; keys 24..=26 switch; the second is the default.
        let zone = |asset, articulation| ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            articulation: Some(ir::ArticulationRef(articulation)),
            ..ir::Zone::new(ir::AssetRef(asset))
        };
        let asset = |i| ir::Asset {
            location: ir::AssetLocation::Path(format!("{i}.wav")),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: Vec::new(),
        };
        let instrument = ir::Instrument {
            assets: (0..3).map(asset).collect(),
            zones: (0..3).map(|a| zone(a, a)).collect(),
            articulations: (0..3u8)
                .map(|a| ir::Articulation { name: a.to_string(), switch_keys: vec![24 + a], default: a == 1, ..Default::default() })
                .collect(),
            ..Default::default()
        };
        let pcm = (0..3).map(|_| Pcm::new(48000, vec![[0.5; 2]; 4800].into_boxed_slice()).unwrap()).collect();
        let plan = sampler_kontakt::prepare(instrument.clone(), pcm, &Default::default()).unwrap().plan;
        let limits = limits(&plan).0;
        let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("arts")).unwrap();
        part.articulations = Some(1);
        if owner == ir::SwitchOwner::Behavior {
            part.tap_keys = Some(vec![Some(24), Some(25), Some(26)]);
        }
        part.set_drivers(&instrument);
        let mut core = V2Core::with_parts(1, 48000.0);
        let mut mix = Mix::default();
        // Remap to velocity: the writer's Switching bits, driver in bits 1..4.
        mix.parts[0].switching = 0x80 | (ir::Driver::Velocity as u8) << 1;
        core.set_mix(&mix);
        core.install(0, Some(Box::new(part)));
        assert_eq!(core.articulation(0), Some(1), "the default plays first");
        // A script-owned switch would tag no zones, so that plan is built native
        // and only the readback is under test: it follows the last switch key.
        if owner == ir::SwitchOwner::Behavior {
            for key in [25u8, 24] {
                let note = HostNote { port: 0, channel: 0, key, id: i32::from(key), clap: true };
                core.event(0, Event::NoteOn { note, velocity: 0.5, tune: 0.0 });
                core.render(64);
                assert_eq!(core.articulation(0), Some(usize::from(key - 24)), "switch key {key}");
            }
            return;
        }
        // Velocities split 1..=127 in three by lowest switch key.
        for (velocity, articulation) in [(10.0, 0), (120.0, 2), (64.0, 1)] {
            let note = HostNote { port: 0, channel: 0, key: 60, id: velocity as i32, clap: true };
            core.event(0, Event::NoteOn { note, velocity: velocity / 127.0, tune: 0.0 });
            core.render(64);
            assert_eq!(core.articulation(0), Some(articulation), "velocity {velocity}");
        }
    }

    /// A real script-owned instrument (Afflatus Horns KS: its script reads the
    /// switch keys and selects the groups): the readback follows a switch key
    /// pressed directly and the key a velocity driver taps, and the selected
    /// articulation sounds. Set `KONTRA_KONTAKT_LIBRARIES` to run.
    #[test]
    fn a_script_owned_switch_reads_back_direct_presses_and_driver_taps() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let relative = "Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki";
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        let instrument = loaded.instrument.clone().expect("a Kontakt instrument");
        assert_eq!(instrument.switching.owner, ir::SwitchOwner::Behavior, "script-owned");
        let keys: Vec<u8> = instrument.articulations.iter().filter_map(|a| a.switch_keys.first().copied()).collect();
        assert!(keys.len() >= 3, "{} switch keys", keys.len());
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        let sounds = |core: &mut V2Core| {
            let note = HostNote { port: 0, channel: 0, key: 60, id: 60, clap: true };
            core.event(0, on(note));
            let heard = (0..300).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let r = core.render(128);
                r.live[0] && r.buses[0][0][..128].iter().any(|x| x.abs() > 1e-5)
            });
            core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(note.key), id: note.id, clap: true }));
            heard
        };
        // Direct presses: the keys driver (the instrument's own) selects.
        for (index, article) in instrument.articulations.iter().enumerate() {
            let Some(&key) = article.switch_keys.first() else { continue };
            let note = HostNote { port: 0, channel: 0, key, id: 1000 + i32::from(key), clap: true };
            core.event(0, on(note));
            core.render(128);
            core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(note.key), id: note.id, clap: true }));
            core.render(128);
            assert_eq!(core.articulation(0), Some(index), "pressed switch key {key}");
            assert!(sounds(&mut core), "articulation {index} sounds after its key");
        }
        // A velocity driver taps the keys into the script: low to high velocity
        // reaches different articulations, and each read back is one whose key
        // exists and whose zones sound.
        let mut mix = Mix::default();
        mix.parts[0].switching = 0x80 | (ir::Driver::Velocity as u8) << 1;
        core.set_mix(&mix);
        let mut seen = std::collections::BTreeSet::new();
        for velocity in [8.0, 30.0, 60.0, 90.0, 120.0] {
            let note = HostNote { port: 0, channel: 0, key: 60, id: velocity as i32, clap: true };
            core.event(0, Event::NoteOn { note, velocity: velocity / 127.0, tune: 0.0 });
            for _ in 0..8 {
                core.render(128);
            }
            core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(note.key), id: note.id, clap: true }));
            let index = core.articulation(0).expect("a readback");
            assert!(index < keys.len(), "velocity {velocity}: articulation {index}");
            seen.insert(index);
        }
        assert!(seen.len() >= 2, "velocity reached articulations {seen:?}");
    }

    #[test]
    fn midi2_per_note_pitch_bend_tunes_the_held_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
        core.event(0, Event::Ump([0x4060_3c00, 0xc000_0000]));
        let part = core.parts[0].as_ref().unwrap();
        let e = part.runtime.expression(part.runtime.expression_id(core.held[0].id).unwrap()).unwrap();
        assert!((e.pitch_semitones - 24.0).abs() < 1e-6, "three quarters of full scale is +24");
        assert_eq!(core.problems(0).ignored_input, 0);
        // CLAP gain expression up to +12 dB, unclamped.
        core.event(0, Event::Expression(super::super::event::HostPattern { port: -1, channel: -1, key: -1, id: -1, clap: true }, NoteExpression::Gain(3.0)));
        let gain = |core: &V2Core| {
            let part = core.parts[0].as_ref().unwrap();
            part.runtime.expression(part.runtime.expression_id(core.held[0].id).unwrap()).unwrap().gain
        };
        assert_eq!(gain(&core), 3.0);
        // A MIDI 2.0 channel bend keeps its 32 bits: a quarter up over ±2
        // semitones (the zone's pitch replaces the per-note bend).
        core.event(0, Event::Ump([0x40e0_0000, 0xa000_0000]));
        let part = core.parts[0].as_ref().unwrap();
        let e = part.runtime.expression(part.runtime.expression_id(core.held[0].id).unwrap()).unwrap();
        let bend = 2.0 * f64::from(0x2000_0000u32) / f64::from(0x7fff_ffffu32);
        assert!((e.pitch_semitones - bend).abs() < 1e-9, "{}", e.pitch_semitones);
        assert_eq!(core.problems(0).narrowed_input, 0);
    }

    #[test]
    fn groups_become_nodes_that_mix_and_route_to_their_own_pairs() {
        let mut instrument = ir::Instrument { name: "kit".into(), ..Default::default() };
        instrument.buses.push(ir::Bus { name: "room".into(), chain: None, sends: vec![], output: ir::Output::Master, gain: ir::Gain::UNITY });
        instrument.groups.push(ir::Group { name: "kick".into(), output: ir::Output::Bus(ir::BusRef(0)), ..Default::default() });
        instrument.groups.push(ir::Group::default());
        let tree = nest(&mut instrument);
        let names: Vec<_> = tree.nodes.iter().map(|n| (n.name.as_str(), n.parent)).collect();
        assert_eq!(names, [("kit", None), ("room", Some(0)), ("kick", Some(1)), ("Group 2", Some(0))]);
        assert_eq!(instrument.groups[0].output, ir::Output::Bus(ir::BusRef(1)));
        assert_eq!(instrument.buses[1].output, ir::Output::Bus(ir::BusRef(0)));

        // A one-group part: its group node plays to pair 3 instead of the instrument's pair 0.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let part = load(&path).unwrap();
        let plan = part.runtime.bus_count();
        assert_eq!(plan, 0, "a WAV part has no buses");
        let pcm = Pcm::new(48000, read_wav(&path).unwrap().1).unwrap();
        let region = Region {
            sample: 0, key_low: 0, key_high: 108, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let bus = sampler_core::Bus { processors: vec![], sends: vec![sampler_core::BusSend { bus: None, gain: 1.0 }], tail_frames: 0 };
        let plan = Prepared::new(48000, vec![pcm], vec![region], 128).unwrap();
        let plan = plan.with_buses(vec![bus], vec![Some(0)]).unwrap();
        let mut tree = MixTree::instrument("one");
        tree.nodes.push(MixNode { name: "g".into(), kind: NodeKind::Group, parent: Some(0), inserts: vec![], sends: vec![] });
        let limits = limits(&plan).0;
        let part = Box::new(Part::new(Runtime::new(plan, limits).unwrap(), tree).unwrap());
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(part));
        let mut mix = Mix::default();
        mix.nodes[0] = vec![NodeMix { output: NodeOutput::Pair(3), ..NodeMix::default() }];
        core.set_mix(&mix);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
        let r = core.render(128);
        assert!(loud(&r, 3, 128) && !loud(&r, 0, 128), "the node left the instrument for pair 4");
        let mut nodes = Vec::new();
        core.take_node_peaks(0, &mut |node, peak| nodes.push((node, peak[0] > 0.0)));
        assert_eq!(nodes, [(1, true)], "the group node meters its signal");
        mix.nodes[0][0].mute = true;
        core.set_mix(&mix);
        assert!(!loud(&core.render(128), 3, 128), "muted node");
    }

    #[test]
    fn a_widget_edit_runs_the_scripts_ui_control_callback() {
        let source = "on init\n declare ui_knob $k(0, 100, 1)\n declare ui_knob $echo(0, 1000, 1)\nend on\n\
                      on ui_control($k)\n $echo := $k + 1\nend on\n";
        let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let id = |name: &str| {
            let c = script.controls().iter().find(|c| c.variable.ends_with(name)).unwrap();
            sampler_ui_ir::ControlId(c.definition.id.0)
        };
        let (k, echo) = (id("$k"), id("$echo"));
        let pcm = Pcm::new(48000, vec![[0.0; 2]; 512].into_boxed_slice()).unwrap();
        let region = Region {
            sample: 0, key_low: 60, key_high: 60, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let plan = script.bind(Prepared::new(48000, vec![pcm], vec![region], 1).unwrap()).unwrap();
        let limits = limits(&plan).0;
        let part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(Box::new(part)));
        assert!(core.set_control(0, k, 41.6), "rounded into the knob's integer range");
        core.render(16);
        assert_eq!((core.control_value(0, k), core.control_value(0, echo)), (Some(42.0), Some(43.0)));
        assert!(core.set_control(0, k, 500.0));
        core.render(16);
        assert_eq!(core.control_value(0, k), Some(100.0), "clamped");
        assert!(!core.set_control(0, sampler_ui_ir::ControlId(7), 1.0), "no such control");
    }

    #[test]
    fn script_ui_effects_reach_the_interface() {
        let source = "on init\n declare ui_knob $k(0, 100, 1)\n declare ui_label $l(1, 1)\n\
                      set_key_type(36, $NI_KEY_TYPE_CONTROL)\nend on\n\
                      on ui_control($k)\n set_control_par(get_ui_id($l), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\n\
                      set_key_color(60, $KEY_COLOR_RED)\nend on\n";
        let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let k = script.controls().iter().find(|c| c.variable.ends_with("$k")).unwrap().definition.id.0;
        let mut ui = ScriptUi { views: vec![script.view()], resources: None };
        let before = ui.interfaces();
        assert!(ui.keys()[36].control && ui.keys()[60].color.is_none());
        let pcm = Pcm::new(48000, vec![[0.0; 2]; 512].into_boxed_slice()).unwrap();
        let region = Region {
            sample: 0, key_low: 60, key_high: 60, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let plan = script.bind(Prepared::new(48000, vec![pcm], vec![region], 1).unwrap()).unwrap();
        let limits = limits(&plan).0;
        let part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(Box::new(part)));
        assert!(core.set_control(0, sampler_ui_ir::ControlId(k), 1.0));
        core.render(16);
        let mut applied = 0;
        core.take_effects(0, &mut |instance, effect| {
            applied += usize::from(ui.apply(instance, effect));
            true
        });
        assert_eq!(applied, 2);
        assert_ne!(ui.interfaces(), before, "the label is hidden");
        assert_eq!(ui.keys()[60].color, Some(0), "red");
    }

    #[test]
    fn script_capacity_grows_with_the_stages_a_key_passes() {
        let pcm = || Pcm::new(48000, vec![[0.0; 2]; 512].into_boxed_slice()).unwrap();
        let region = Region {
            sample: 0, key_low: 60, key_high: 60, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let plain = Prepared::new(48000, vec![pcm()], vec![region.clone()], 1).unwrap();
        assert_eq!(Limits::script_capacity(&plain), 16);
        let script = |n: u8| {
            sampler_ksp::compile(&format!("on note\n play_note({}, 100, 0, -1)\nend on\n", 60 + n), 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap()
        };
        let one = sampler_ksp::bind_modules(vec![script(0)], Prepared::new(48000, vec![pcm()], vec![region.clone()], 1).unwrap()).unwrap();
        let four = sampler_ksp::bind_modules((0..4).map(script).collect(), Prepared::new(48000, vec![pcm()], vec![region], 1).unwrap()).unwrap();
        assert_eq!((Limits::script_capacity(&one), Limits::script_capacity(&four)), (4 * Limits::SCRIPT_KEYS + 1, 16 * Limits::SCRIPT_KEYS + 4));
        let limits = limits(&four).0;
        assert!(Runtime::new(four, limits).is_ok());
    }

    #[test]
    fn unknown_formats_are_explicitly_unsupported() {
        let request = LoadRequest { path: "x.exs".into(), sample_rate: 48000.0, ..Default::default() };
        let err = V2Loader.prepare(&request, &mut |_| {}, &|| false).err().unwrap();
        assert!(matches!(err, CoreError::Unsupported(_)), "{err}");
    }

    /// Set `KONTRA_KONTAKT_LIBRARIES` to library roots to run; skips otherwise.
    /// Scripted instruments must sound: with their scripts on, a played note is
    /// audible within a few seconds. Set `KONTRA_KONTAKT_LIBRARIES` to run.
    #[test]
    fn real_scripted_instruments_are_not_silent() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let mut silent = Vec::new();
        for (relative, key) in [
            ("Performance Samples Vista/Instruments/Vista - 3 Cellos.nki", 48),
            ("Una Corda Library/Instruments/Una Corda Pure.nki", 60),
            ("Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki", 48),
            ("Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki", 48),
        ] {
            let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
                eprintln!("skipped: {relative} is not installed");
                continue;
            };
            let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
            let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
            let mut core = V2Core::with_parts(1, 48000.0);
            core.install(0, loaded.part);
            core.event(0, on(HostNote { port: 0, channel: 0, key, id: 1, clap: true }));
            // Streamed pages arrive from disk threads: give them real time.
            let heard = (0..300).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(5));
                // Quiet layers are still sound: dynamics start at the softest.
                let r = core.render(128);
                r.live[0] && r.buses[0][0][..128].iter().any(|x| x.abs() > 1e-5)
            });
            if !heard {
                silent.push(format!("{relative}: {} voices, {:?}, {} callbacks", core.voices().active, core.problems(0), loaded.report.decoded.script_callbacks));
            }
        }
        assert!(silent.is_empty(), "silent with scripts on: {silent:#?}");
    }

    /// The source's instrument and send buses are mixer nodes beside its groups.
    /// Set `KONTRA_KONTAKT_LIBRARIES` to library roots to run; skips otherwise.
    #[test]
    fn real_instrument_buses_become_mixer_nodes() {
        let relative = "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false).unwrap();
        let buses: Vec<_> = loaded.tree.nodes.iter().filter(|n| n.kind == NodeKind::Bus).map(|n| n.name.as_str()).collect();
        assert_eq!(buses, ["insert", "send 0", "send 1"]);
        let groups = loaded.tree.nodes.iter().filter(|n| n.kind == NodeKind::Group).count();
        assert!(groups > 100, "every group is a node too: {groups}");
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        // The part accepts a setting for every node.
        let mut mix = Mix::default();
        mix.nodes[0] = vec![NodeMix::default(); loaded.tree.nodes.len() - 1];
        core.set_mix(&mix);
    }

    /// Each program of a Kontakt multi loads as its own rack part.
    #[test]
    fn real_multi_programs_load_as_parts() {
        let relative = "Audio Imperia CHORUS/Multis/10 Chorus - Ensemble - Traditional Syllables.nkm";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let count = sampler_kontakt::read_multi(&path).unwrap().programs.len();
        assert!(count > 1, "a multi has several programs");
        let names: Vec<_> = (0..2)
            .map(|program| {
                let request = LoadRequest { path: path.clone(), program, sample_rate: 48000.0, ..Default::default() };
                V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap().instrument.unwrap().name.clone()
            })
            .collect();
        assert_ne!(names[0], names[1]);
    }

    /// A UVI bank program loads through the host and its layers are mixer nodes.
    #[test]
    fn real_uvi_layers_become_mixer_nodes() {
        let relative = "UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let roots = std::env::split_paths(&roots).map(|r| r.parent().unwrap_or(&r).to_path_buf());
        let Some(path) = roots.map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let loaded = match V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false) {
            Ok(loaded) => loaded,
            Err(e) => {
                eprintln!("skipped: {e:?}");
                return;
            }
        };
        let groups = loaded.tree.nodes.iter().filter(|n| n.kind == NodeKind::Group).count();
        assert!(groups > 0, "layers are nodes");
    }

    /// Peak of the last blocks after applying `mix` to a part with a held note.
    fn settled(core: &mut V2Core, mix: &Mix) -> f32 {
        core.set_mix(mix);
        let mut peak = 0.0f32;
        for block in 0..400 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            let r = core.render(128);
            if block >= 390 {
                peak = r.buses.iter().flat_map(|b| b[0][..128].iter().chain(&b[1][..128])).fold(peak, |p, x| p.max(x.abs()));
            }
        }
        peak
    }

    /// Muting nodes silences the part and soloing one leaves only it. `kind`
    /// picks the nodes the test drives; the instrument must play at `keys`.
    fn mixer_nodes_pass_audio(path: std::path::PathBuf, kind: NodeKind) {
        let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false).unwrap();
        let nodes: Vec<usize> = (1..loaded.tree.nodes.len()).filter(|&n| loaded.tree.nodes[n].kind == kind).collect();
        let count = loaded.tree.nodes.len() - 1;
        // Try the middles of zones across the map until one key sounds.
        let zones = &loaded.instrument.as_ref().unwrap().zones;
        let mut keys: Vec<u8> = (0..8)
            .filter_map(|n| zones.get(n * zones.len() / 8))
            .map(|z| ((u16::from(z.keys.low) + u16::from(z.keys.high)) / 2) as u8)
            .collect();
        keys.dedup();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        let mut heard = false;
        for (id, &key) in keys.iter().enumerate() {
            core.event(0, on(HostNote { port: 0, channel: 0, key, id: id as i32 + 1, clap: true }));
            heard = (0..150).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let r = core.render(128);
                r.live[0] && r.buses[0][0][..128].iter().any(|x| x.abs() > 1e-5)
            });
            if heard {
                break;
            }
        }
        assert!(heard, "silent before any node is touched: keys {keys:?}, {:?}, samples {} zones {} missing {:?}", core.problems(0), loaded.report.decoded.samples, loaded.report.decoded.zones, loaded.report.missing.iter().take(6).collect::<Vec<_>>());
        assert!(nodes.len() >= 2, "{kind:?} nodes: {}", nodes.len());
        let mut mix = Mix::default();
        mix.nodes[0] = vec![NodeMix::default(); count];
        let open = settled(&mut core, &mix);
        assert!(open > 1e-6, "audible with every node open");
        for &n in &nodes {
            mix.nodes[0][n - 1].mute = true;
        }
        let muted = settled(&mut core, &mix);
        let loose: Vec<_> = (1..loaded.tree.nodes.len()).filter(|&n| loaded.tree.nodes[n].kind == NodeKind::Group && loaded.tree.nodes[n].parent.is_some_and(|p| !nodes.contains(&p) && kind == NodeKind::Mic)).map(|n| loaded.tree.nodes[n].name.clone()).collect();
        // An effect's tail may ring on after its input is muted.
        assert!(muted < open * 0.05 + 1e-7, "muting every {kind:?} node leaves {muted} of {open}; groups outside: {loose:?}");
        for &n in &nodes {
            mix.nodes[0][n - 1].mute = false;
        }
        // Soloing a node mutes the rest, so some solo is audible and the
        // soloed set is quieter than or equal to everything.
        let all = settled(&mut core, &mix);
        let mut heard_solo = false;
        for &n in &nodes {
            mix.nodes[0][n - 1].solo = true;
            let solo = settled(&mut core, &mix);
            mix.nodes[0][n - 1].solo = false;
            assert!(solo <= all * 1.01 + 1e-7, "solo {} louder than all: {solo} > {all}", loaded.tree.nodes[n].name);
            heard_solo |= solo > 1e-6;
        }
        assert!(heard_solo, "no {kind:?} node is audible alone");
        // A node with nothing soloed elsewhere: soloing one while every other
        // is muted keeps exactly that one.
        for &n in &nodes {
            mix.nodes[0][n - 1].mute = true;
        }
        mix.nodes[0][nodes[0] - 1].mute = false;
        mix.nodes[0][nodes[0] - 1].solo = true;
        let one = settled(&mut core, &mix);
        mix.nodes[0][nodes[0] - 1].solo = false;
        mix.nodes[0][nodes[0] - 1].mute = true;
        let none = settled(&mut core, &mix);
        assert!(none < open * 0.05 + 1e-7 && one >= none, "one {one}, none {none}");
    }

    #[test]
    fn real_snapshots_read_find_their_instrument_and_load() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(library) = std::env::split_paths(&roots).map(|r| r.join("Una Corda Library")).find(|p| p.is_dir()) else {
            eprintln!("skipped: Una Corda Library is not installed");
            return;
        };
        let files: Vec<_> = walkdir::WalkDir::new(library.join("Snapshots"))
            .into_iter()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "nksn"))
            .map(|e| e.into_path())
            .collect();
        assert!(!files.is_empty());
        let (mut script, mut groups, mut effects) = (0, 0, 0);
        for file in &files {
            let state = sampler_kontakt::read_snapshot(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let parent = snapshot_parent(file, &state.instrument).unwrap_or_else(|| panic!("no instrument for {}", file.display()));
            let plain = sampler_kontakt::read(&parent).unwrap();
            let applied = sampler_kontakt::read_with_snapshot(&parent, &state).unwrap();
            let states = |k: &sampler_kontakt::Kontakt| k.instrument.behaviors.iter().map(|b| b.state.clone()).collect::<Vec<_>>();
            let mix = |k: &sampler_kontakt::Kontakt| format!("{:?}{:?}", k.instrument.buses, k.instrument.chains);
            let levels = |k: &sampler_kontakt::Kontakt| k.instrument.groups.iter().map(|g| (g.gain, g.pan, g.tune)).collect::<Vec<_>>();
            script += usize::from(states(&plain) != states(&applied));
            groups += usize::from(levels(&plain) != levels(&applied));
            effects += usize::from(mix(&plain) != mix(&applied));
        }
        eprintln!("{} snapshots: {script} change script state, {groups} group levels, {effects} effects", files.len());
        assert!(script > 0 && groups > 0 && effects > 0, "script {script} groups {groups} effects {effects} of {}", files.len());
        let request = LoadRequest { path: files[0].clone(), sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        assert!(loaded.instrument.is_some_and(|i| !i.zones.is_empty()));
    }

    #[test]
    fn real_kontakt_mic_nodes_pass_audio() {
        let relative = "Afflatus Chapter II Brass/Instruments/1. Ensembles/Single Instruments/2 Horns/2 Horns Staccato.nki";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        mixer_nodes_pass_audio(path, NodeKind::Mic);
    }

    #[test]
    fn real_uvi_layer_nodes_pass_audio() {
        let relative = "UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let roots = std::env::split_paths(&roots).map(|r| r.parent().unwrap_or(&r).to_path_buf());
        let Some(path) = roots.map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        mixer_nodes_pass_audio(path, NodeKind::Group);
    }

    #[test]
    fn real_uvi_lua_program_sounds_through_the_trait() {
        let relative = "VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip";
        let roots = std::env::var_os("KONTRA_UVI_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.to_string_lossy().contains(".ufs") && p.ancestors().any(|a| a.is_file())) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| (), &|| false).unwrap();
        assert!(
            !loaded.report.missing.iter().any(|m| m.value.contains("no frontend")),
            "the Lua script runs: {:?}",
            loaded.report.missing
        );
        let stream = loaded.stream.clone().expect("UVI samples stream");
        let (held, full) = (stream.resident_bytes(), loaded.report.decoded.full_bytes);
        assert!(held > 0 && held < full, "{held} of {full} bytes resident");
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 36, id: 1, clap: true }));
        // The script runs on its own thread: its note arrives within a few blocks.
        let heard = (0..400).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            loud(&core.render(128), 0, 128)
        });
        assert!(heard, "the scripted program is silent: {:?} / {:?}", core.problems(0), core.voices());
    }

    /// Idle cost of a loaded part: blocks with no note playing. Prints the
    /// share of one core; `KONTRA_KONTAKT_LIBRARIES=… cargo test idle_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn idle_cost_of_a_loaded_instrument() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        for relative in [
            "Una Corda Library/Instruments/Una Corda Pure.nki",
            "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki",
            "Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki",
        ] {
            let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else { continue };
            let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false).unwrap();
            let mut core = V2Core::with_parts(1, 48000.0);
            core.install(0, loaded.part);
            std::thread::sleep(std::time::Duration::from_secs(2));
            let blocks = 48000 / 128 * 20;
            let start = std::time::Instant::now();
            for _ in 0..blocks {
                core.render(128);
            }
            let spent = start.elapsed().as_secs_f64();
            println!("IDLE {relative}: {:.4}% of a core ({:.1} us per 128-frame block)", spent / 20.0 * 100.0, spent / blocks as f64 * 1e6);
        }
    }

    #[test]
    fn real_uvi_lua_program_plays_without_audio_thread_allocation() {
        let relative = "VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip";
        let roots = std::env::var_os("KONTRA_UVI_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.to_string_lossy().contains(".ufs") && p.ancestors().any(|a| a.is_file())) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| (), &|| false).unwrap();
        assert!(
            !loaded.report.missing.iter().any(|m| m.value.contains("no frontend")),
            "the Lua script runs: {:?}",
            loaded.report.missing
        );
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 36, id: 1, clap: true }));
        // The script runs on its own thread: its note arrives within a few blocks.
        let heard = (0..400).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            loud(&core.render(128), 0, 128)
        });
        assert!(heard, "the scripted program is silent: {:?} / {:?}", core.problems(0), core.voices());
        // Warm: the first notes sized the driver's tables. Now play more
        // scripted notes and release them; the audio thread allocates nothing.
        core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: 36, id: -1, clap: true }));
        (0..50).for_each(|_| _ = core.render(128));
        let allocations = crate::plugin::tests::allocations(|| {
            for (id, key) in [(2, 40), (3, 43), (4, 36)] {
                core.event(0, on(HostNote { port: 0, channel: 0, key, id, clap: true }));
                for _ in 0..60 {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    _ = core.render(128);
                }
                core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(key), id: -1, clap: true }));
            }
        });
        assert_eq!(allocations, 0, "the audio thread allocated or freed memory");
    }

    #[test]
    fn pitch_bend_reaches_notes_a_lua_script_played() {
        let relative = "VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip";
        let roots = std::env::var_os("KONTRA_UVI_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.ancestors().any(|a| a.is_file())) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| (), &|| false).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 40, id: 1, clap: true }));
        // Zero crossings of the left channel over `blocks` blocks, scripts given a moment each.
        let crossings = |core: &mut V2Core, blocks: usize| {
            let (mut n, mut last) = (0usize, 0.0f32);
            for _ in 0..blocks {
                std::thread::sleep(std::time::Duration::from_millis(2));
                let r = core.render(128);
                for x in &r.buses[0][0][..128] {
                    n += usize::from(last <= 0.0 && *x > 0.0);
                    last = *x;
                }
            }
            n
        };
        let heard = (0..400).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            loud(&core.render(128), 0, 128)
        });
        assert!(heard);
        let before = crossings(&mut core, 150);
        core.event(0, Event::Ump([0x40e0_0000, 0xffff_ffff]));
        let after = crossings(&mut core, 150);
        assert!(before > 20, "{before} crossings");
        // The default bend range is two semitones: about 12% higher.
        assert!(after as f64 > before as f64 * 1.05, "{before} crossings before the bend, {after} after");
    }

    #[test]
    fn real_kontakt_instrument_plays_through_the_trait() {
        let relative = "Una Corda Library/Instruments/Una Corda Pure.nki";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let description = V2Loader.describe(&path, 0).unwrap();
        assert!(description.zones > 0);
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let mut last = Progress(0);
        let loaded = V2Loader.prepare(&request, &mut |p| last = p, &|| false).unwrap();
        assert_eq!(last, Progress::DONE);
        assert!(loaded.tree.nodes.len() > 1, "groups are mixer nodes");
        assert!(loaded.report.decoded.zones > 0);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
        let heard = (0..40).any(|_| loud(&core.render(128), 0, 128));
        assert!(heard, "no output; missing: {:?}", loaded.report.missing);

        // Samples stream: only start data is resident, and an idle budget drops it.
        let stream = loaded.stream.expect("Kontakt samples stream");
        let (held, full) = (stream.resident_bytes(), loaded.report.decoded.full_bytes);
        assert!(held > 0 && held < full, "{held} of {full} bytes resident");
        core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: 60, id: -1, clap: true }));
        (0..400).for_each(|_| _ = core.render(128));
        assert!(stream.trim(0, core.clock(0)) > 0, "idle start data is dropped");
        assert!(stream.resident_bytes() < held);
        // A purged sample's first start is refused and reloads it; later starts play.
        let again = (2..200).any(|id| {
            core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id, clap: true }));
            std::thread::sleep(std::time::Duration::from_millis(5));
            (0..4).any(|_| loud(&core.render(128), 0, 128))
        });
        assert!(again, "the purged sample reloads");
    }
}

#[cfg(test)]
mod send_tests {
    use super::*;

    fn instrument(fader: f64) -> ir::Instrument {
        let mut i = ir::Instrument::default();
        i.buses.push(ir::Bus { name: "aux".into(), chain: None, sends: Vec::new(), output: ir::Output::Master, gain: ir::Gain::UNITY });
        let send = |gain, pre_fader| ir::GroupSend { to: ir::BusRef(0), gain: ir::Gain::Linear(gain), pre_fader };
        i.groups.push(ir::Group { gain: ir::Gain::Linear(fader), sends: vec![send(0.5, true), send(0.5, false)], ..Default::default() });
        i
    }

    fn tapped(fader: f64) -> (ir::Gain, Vec<f64>) {
        let mut i = instrument(fader);
        let tree = nest(&mut i);
        let bus = &i.buses[1];
        assert_eq!(i.groups[0].gain, ir::Gain::UNITY, "voices feed the tap unscaled");
        assert_eq!(tree.nodes[2].sends.len(), 2);
        (bus.gain, tree.nodes[2].sends.iter().map(|s| f64::from(s.1)).collect())
    }

    #[test]
    fn a_closed_fader_silences_the_output_but_not_the_pre_fader_send() {
        let (out, sends) = tapped(0.0);
        assert_eq!((out.linear(), sends), (0.0, vec![0.5, 0.0]));
        assert_eq!(instrument(0.0).groups[0].sends.len(), 2);
    }

    #[test]
    fn pre_and_post_sends_tap_either_side_of_a_minus_12_db_fader() {
        let fader = 10f64.powf(-12.0 / 20.0);
        let (out, sends) = tapped(fader);
        assert_eq!(out.linear(), fader);
        assert!((sends[0] - 0.5).abs() < 1e-6 && (sends[1] - 0.5 * fader).abs() < 1e-6, "{sends:?}");
    }

    #[test]
    fn lowering_hears_the_same_taps_without_a_host_mixer() {
        let routed = instrument(0.25).with_group_taps();
        assert_eq!(routed.buses[1].gain.linear(), 0.25);
        assert_eq!(routed.groups[0].output, ir::Output::Bus(ir::BusRef(1)));
    }
}
