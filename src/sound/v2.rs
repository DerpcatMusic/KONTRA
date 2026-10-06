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

use std::path::Path;
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
    BUSES, Block, BlockInfo, Core, CoreError, CoreLoader, Description, LoadRequest, Loaded, ScriptUi, Stream, MAX_BLOCK, Progress,
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
}

/// A thread that sleeps until the audio side reports a nearly full voice pool
/// (or a growth coming back), then doubles the pool up to `ceiling`.
struct Grower {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Grower {
    fn start(runtime: &mut Runtime, mut control: PlanControl, ceiling: usize) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new().name("sampler-grow".into()).spawn({
            let stop = stop.clone();
            move || {
                while !stop.load(Ordering::Relaxed) {
                    if control.voice_pressure() && control.voice_capacity() < ceiling {
                        let _ = control.grow_voices((control.voice_capacity() * 2).min(ceiling));
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
            mpe_zone: false,
            bend_range: 0,
            horizon: None,
            _stream: None,
            grower: None,
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

/// `expression` on one held note.
fn note_expression(part: &mut Part, id: NoteId, expression: NoteExpression) {
    let tune = f64::from(part.tune);
    express(&mut part.runtime, id, |e| match expression {
        NoteExpression::Tune(semitones) => e.pitch_semitones = semitones + tune,
        NoteExpression::Gain(gain) => e.gain = gain,
        NoteExpression::Pan(pan) => e.pan = pan,
        NoteExpression::Pressure(v) => e.pressure = unit_scale(v),
        NoteExpression::Brightness(v) => e.timbre = unit_scale(v),
    });
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
            if let Some(horizon) = part.horizon {
                // Pending pages play silent and count as underruns.
                let _ = part.runtime.service_streaming(horizon);
            }
            let out = &mut self.scratch[..n];
            let mut outs: [&mut [Frame]; BUSES] = self.direct.each_mut().map(|d| &mut d[..n]);
            if part.runtime.render_split(out, &mut outs).is_err() {
                continue;
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
            ..p.problems
        }
    }

    fn articulation(&self, part: usize) -> Option<usize> {
        let p = self.parts.get(part)?.as_ref()?;
        let default = p.articulations?;
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

/// Voice-rendering threads per part: `KONTRA_THREADS` is `auto` or a count.
/// One (the audio thread alone) unless set.
fn render_threads() -> Threads {
    match std::env::var("KONTRA_THREADS").as_deref() {
        Ok("auto") => Threads::Auto,
        Ok(n) => Threads::Fixed(n.parse().unwrap_or(1)),
        Err(_) => Threads::Fixed(1),
    }
}

/// Memory a part preallocates for per-voice state. Voices start sized to this,
/// not to a fixed polyphony, and the pool doubles off the audio thread (see
/// `Grower`) past three quarters full, up to `GROWTH` times as many. A note is
/// refused only when that is exhausted too, and it is counted
/// (`RuntimeStats::voice_drops`).
const VOICE_BUDGET: usize = 256 << 20;
const MIN_VOICES: usize = 512;
const MAX_VOICES: usize = 16384;
const GROWTH: usize = 4;

/// Capacities of a part, sized for its plan's script state and voice cost, and
/// the voice count the pool may grow to. Notes, families and decisions are
/// sized for that ceiling so growing voices is not capped by them.
fn limits(plan: &Prepared) -> (Limits, usize) {
    let voices = (VOICE_BUDGET / plan.voice_state_bytes().max(1)).clamp(MIN_VOICES, MAX_VOICES);
    let ceiling = voices * GROWTH;
    // Notes outlive their voices only in release, and each holds a few voices.
    let notes = (ceiling / 4).max(NOTES);
    let limits = Limits {
        families: (ceiling / 2).max(256),
        decisions: (ceiling / 2).max(256),
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
            ir::Processor::Pan(_) => "Pan",
            ir::Processor::StereoMatrix(_) => "Stereo",
            ir::Processor::Reverb(_) => "Reverb",
            ir::Processor::Convolution { .. } => "Convolution",
            ir::Processor::Filter(_) => "Filter",
            ir::Processor::Delay { .. } => "Delay",
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
            instrument.buses.push(ir::Bus { name: name.clone(), chain: None, sends: Vec::new(), output });
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
        instrument.buses.push(ir::Bus { name, chain: None, sends: Vec::new(), output });
        instrument.groups[index].output = ir::Output::Bus(bus);
    }
    tree
}

/// A plan and, when its samples stream, the runtime's page cache.
type Plan = (Prepared, Option<StreamCache>);

fn kontakt(
    request: &LoadRequest,
    progress: &mut dyn FnMut(Progress),
    canceled: &(dyn Fn() -> bool + Sync),
) -> Result<Loaded<Plan>, CoreError> {
    let load = |e: sampler_kontakt::LoadError| match e {
        sampler_kontakt::LoadError::Canceled => CoreError::Canceled,
        e => CoreError::Load(e.to_string()),
    };
    let multi = request.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkm"));
    let mut source = if multi {
        sampler_kontakt::read_program(&request.path, request.program as usize)
    } else {
        sampler_kontakt::read(&request.path)
    }
    .map_err(load)?;
    let mut report = LoadReport::of(&source.instrument, &request.path, source.locations.len());
    let tree = nest(&mut source.instrument);
    let options = sampler_kontakt::Options {
        rate: request.sample_rate as u32,
        library: Some(request.path.clone()),
        mpe: request.mpe.then(Default::default),
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
    // Loading adds what it found unplayable (samples, keys) and scripts that failed.
    report.missing = loaded.instrument.unsupported.iter().map(Missing::from).collect();
    report.decoded.zones = loaded.instrument.zones.len();
    report.decoded.keys = super::report::key_bits(&loaded.instrument);
    report.decoded.samples = loaded.plan.sample_count();
    Ok(Loaded {
        part: (loaded.plan, Some(cache)),
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
/// decode up front (no streaming yet).
fn uvi(request: &LoadRequest) -> Result<Loaded<Plan>, CoreError> {
    let load = |e: &dyn std::fmt::Display| CoreError::Load(e.to_string());
    let mut t = sampler_uvi::translate_path(&request.path).map_err(|e| load(&*e))?;
    let mut report = LoadReport::of(&t.instrument, &request.path, t.locations.len());
    let tree = nest(&mut t.instrument);
    let loaded = sampler_uvi::assemble_translated(t, request.sample_rate as u32).map_err(|e| load(&*e))?;
    report.missing = loaded.instrument.unsupported.iter().map(Missing::from).collect();
    report.decoded.zones = loaded.instrument.zones.len();
    report.decoded.keys = super::report::key_bits(&loaded.instrument);
    report.decoded.samples = loaded.plan.sample_count();
    Ok(Loaded {
        part: (loaded.plan, None),
        tree,
        report,
        interfaces: loaded.interfaces,
        controls: Vec::new(),
        instrument: Some(Arc::new(loaded.instrument)),
        scripts: ScriptUi { views: loaded.scripts, resources: loaded.resources },
        stream: None,
    })
}

fn read_wav(path: &Path) -> Result<(u32, Box<[Frame]>), CoreError> {
    let mut reader = hound::WavReader::open(path).map_err(|e| CoreError::Load(e.to_string()))?;
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
    .map_err(|e| CoreError::Load(e.to_string()))?;
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
        part: (plan, None),
        tree: MixTree::instrument(&name),
        report,
        interfaces: Vec::new(),
        controls: Vec::new(),
        instrument: None,
        scripts: ScriptUi::default(),
        stream: None,
    })
}

impl CoreLoader for V2Loader {
    type Core = V2Core;

    fn prepare(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Loaded<Option<Box<Part>>>, CoreError> {
        let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
        let Loaded { part: (prepared, cache), tree, mut report, interfaces, instrument, scripts, stream, .. } = if is_kontakt(&request.path) {
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
        let controls = prepared
            .controls()
            .iter()
            .map(|c| (sampler_ui_ir::ControlId(c.id.0), number(c.default)))
            .collect();
        let (limits, ceiling) = limits(&prepared);
        report.decoded.script_callbacks = limits.behaviors;
        let voices = limits.voices;
        let (runtime, control) = Runtime::with_plan_updates(prepared, limits, 2, 1).map_err(core)?;
        let mut runtime = runtime.with_threads(render_threads());
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
        let grower = Grower::start(&mut runtime, control, ceiling)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let mut part = Part::new(runtime, tree.clone())?;
        part.grower = Some(grower);
        if let Some(inst) = instrument.as_deref() {
            part.set_drivers(inst);
        }
        part.articulations = instrument.as_deref().filter(|i| {
            !i.articulations.is_empty() && i.switching.owner == ir::SwitchOwner::Native
        }).map(|i| i.articulations.iter().position(|a| a.default).unwrap_or(0));
        if streams && let Some(stream) = &stream {
            // Heads bound only starts; running voices request a page ahead.
            part.horizon = Some((stream.report.head_frames.max(PAGE_FRAMES) + MAX_BLOCK) as u32);
            part._stream = Some(stream.clone());
        }
        progress(Progress::DONE);
        Ok(Loaded { part: Some(Box::new(part)), tree, report, interfaces, controls, instrument, scripts, stream })
    }

    fn describe(&self, path: &Path, _program: u32) -> Result<Description, CoreError> {
        if is_kontakt(path) {
            let instrument = sampler_kontakt::read(path).map_err(|e| CoreError::Load(e.to_string()))?.instrument;
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
        hound::WavReader::open(path).map_err(|e| CoreError::Load(e.to_string()))?;
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
        let limits = limits(&plan);
        let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("arts")).unwrap();
        part.articulations = Some(1);
        part.set_drivers(&instrument);
        let mut core = V2Core::with_parts(1, 48000.0);
        let mut mix = Mix::default();
        // Remap to velocity: the writer's Switching bits, driver in bits 1..4.
        mix.parts[0].switching = 0x80 | (ir::Driver::Velocity as u8) << 1;
        core.set_mix(&mix);
        core.install(0, Some(Box::new(part)));
        assert_eq!(core.articulation(0), Some(1), "the default plays first");
        // Velocities split 1..=127 in three by lowest switch key.
        for (velocity, articulation) in [(10.0, 0), (120.0, 2), (64.0, 1)] {
            let note = HostNote { port: 0, channel: 0, key: 60, id: velocity as i32, clap: true };
            core.event(0, Event::NoteOn { note, velocity: velocity / 127.0, tune: 0.0 });
            core.render(64);
            assert_eq!(core.articulation(0), Some(articulation), "velocity {velocity}");
        }
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
        instrument.buses.push(ir::Bus { name: "room".into(), chain: None, sends: vec![], output: ir::Output::Master });
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
        let limits = limits(&plan);
        let part = Box::new(Part::new(Runtime::new(plan, limits.0).unwrap(), tree).unwrap());
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
        let limits = limits(&plan);
        let part = Part::new(Runtime::new(plan, limits.0).unwrap(), MixTree::instrument("s")).unwrap();
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
        let limits = limits(&plan);
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
        let limits = limits(&four);
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
        assert_eq!(buses, ["insert", "send 0"]);
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
