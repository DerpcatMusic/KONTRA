//! [`Core`] over `sampler-core`: one [`Runtime`] per rack part, fed through a
//! `sampler-midi` zone.
//!
//! Wired: host and MIDI notes (exact CLAP/VST3 ownership and NOTE_END, layered
//! parts), sustain and sostenuto, controllers, pitch bend (RPN sensitivity),
//! channel pressure, CC74, per-note host expressions (tuning, gain, pan,
//! pressure, brightness) and polyphonic aftertouch, all notes/sound off; the
//! mixer's part gain, pan, tune, mute, solo, output and aux bus, bus faders,
//! peaks and the scope tap; audition, voices, panic. Loading: Kontakt
//! instruments through `sampler-kontakt` (cancelable), WAV files as one region.
//!
//! Not yet: script UI and persistence (wait on the KSP compiler), alignment,
//! macros, mic outs, load shedding and streaming, member-channel MPE (every
//! channel plays the zone's manager channel), sample-rate change without reload.

use std::path::Path;

use sampler_core::{
    ChannelAddress, Envelope, Expression, Frame, Input, Limits, NoteId, Pcm, Playback, Prepared, Protocol, Region,
    Runtime,
};
use sampler_midi::{Mpe, Packets, Zone};

use super::event::{HostExpression, HostNote, In};
use super::mix::{Mix, Peaks};
use super::view::{Persisted, Refresh};
use super::{
    BUSES, Block, BlockInfo, Core, CoreError, CoreLoader, Description, LoadRequest, MAX_BLOCK, Macros, Progress,
    RACK_SLOTS, Rendered, Transport, Voices,
};

/// Host notes tracked for ownership and NOTE_END across the rack.
const HELD: usize = 1024;
/// Notes a part holds at once, sounding or awaiting NOTE_END.
const NOTES: usize = 128;
/// [`Held::part`] of a note whose runtime was replaced: ends at the next block.
const ORPHAN: usize = usize::MAX;
const AUDITION_CHANNEL: u8 = 15;
/// Every input reaches a part's zone as MIDI 1.0 on its manager channel, so a
/// bend, pedal or controller on any channel reaches every note of the part.
const WIRE: ChannelAddress = ChannelAddress { protocol: Protocol::Midi1, port: 0, group: 0, channel: 0 };

/// One playable part: its runtime and the MIDI zone in front of it.
pub struct Part {
    runtime: Runtime,
    mpe: Mpe,
    tune: f32,
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
    auditions: Vec<Option<u8>>,
    /// Notes the ownership table had no room for.
    overflow: u64,
    buses: Box<[Block; BUSES]>,
    written: [bool; BUSES],
    scratch: Box<[Frame; MAX_BLOCK]>,
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
/// without one (VST3) get a negative ID per channel.
fn host_input(note: HostNote) -> Input {
    wire(note.key, Some(if note.id >= 0 { note.id } else { -1 - i32::from(note.channel) }))
}

/// Linear gains `[left, right]`; NaN or infinity silences.
fn balance(gain: f32, pan: f32) -> [f32; 2] {
    if !gain.is_finite() || pan.is_nan() {
        return [0.0; 2];
    }
    [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))]
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |p, x| p.max(x.abs()))
}

/// A MIDI 1.0 channel voice message into `part`'s zone.
fn wire_event(part: &mut Part, status: u8, a: u8, b: u8) {
    let words = [0x2000_0000 | u32::from(status) << 20 | u32::from(a & 127) << 8 | u32::from(b & 127)];
    if let Some(Ok(packet)) = Packets::new(&words).next() {
        let _ = part.mpe.apply(&mut part.runtime, packet);
    }
}

/// Change one note's expression in place.
fn express(runtime: &mut Runtime, note: NoteId, change: impl FnOnce(&mut Expression)) {
    let Ok(owner) = runtime.expression_id(note) else { return };
    let Ok(mut expression) = runtime.expression(owner) else { return };
    change(&mut expression);
    expression.gain = expression.gain.clamp(0.0, 1.0);
    expression.pan = expression.pan.clamp(-1.0, 1.0);
    let _ = runtime.set_expressions(&[(owner, expression)]);
}

fn full_scale(value: u8) -> u32 {
    (u64::from(value.min(127)) * u64::from(u32::MAX) / 127) as u32
}

/// `event` into one part.
fn deliver(part: &mut Part, index: usize, held: &mut Vec<Held>, overflow: &mut u64, event: In) {
    let mut each = |channel: u8, key: u8, change: &dyn Fn(&mut Expression)| {
        for h in held.iter().filter(|h| h.part == index && h.note.channel == channel && h.note.key == key) {
            express(&mut part.runtime, h.id, change);
        }
    };
    match event {
        In::NoteTune(c, k, semitones) => {
            let tune = f64::from(part.tune);
            each(c, k, &|e| e.pitch_semitones = f64::from(semitones) + tune);
        }
        In::NoteGain(c, k, gain) => each(c, k, &|e| e.gain = f64::from(gain)),
        In::NotePan(c, k, pan) => each(c, k, &|e| e.pan = f64::from(pan)),
        In::NotePressure(c, k, v) | In::PolyAt(c, k, v) => each(c, k, &|e| e.pressure = full_scale(v)),
        In::NoteBrightness(c, k, v) => each(c, k, &|e| e.timbre = full_scale(v)),
        In::HostOn(note, velocity, tune) => {
            if held.len() == HELD {
                *overflow += 1;
                return;
            }
            let input = host_input(note);
            let Ok(id) = part.mpe.trigger(&mut part.runtime, WIRE.channel, input, f64::from(velocity) / 127.0) else {
                return;
            };
            held.push(Held { part: index, note, input, id });
            if tune != 0.0 {
                express(&mut part.runtime, id, |e| e.pitch_semitones += f64::from(tune));
            }
        }
        In::HostOff(pattern) | In::HostChoke(pattern) => {
            for h in held.iter().filter(|h| h.part == index && pattern.matches(h.note)) {
                let _ = part.runtime.note_off(h.input, None);
            }
        }
        In::HostExpression(pattern, expression) => {
            let tune = f64::from(part.tune);
            for h in held.iter().filter(|h| h.part == index && pattern.matches(h.note)) {
                express(&mut part.runtime, h.id, |e| match expression {
                    HostExpression::Gain(gain) => e.gain = f64::from(gain),
                    HostExpression::Pan(pan) => e.pan = f64::from(pan),
                    HostExpression::Tune(semitones) => e.pitch_semitones = f64::from(semitones) + tune,
                });
            }
        }
        In::NoteOn(_, key, 0) | In::NoteOff(_, key) => wire_event(part, 0x8, key, 0),
        In::NoteOn(_, key, velocity) => wire_event(part, 0x9, key, velocity),
        In::Cc(_, 120, _) => {
            let _ = part.runtime.all_sound_off(WIRE);
        }
        In::Cc(_, 123, _) => {
            let _ = part.runtime.all_notes_off(WIRE);
        }
        In::Cc(_, index, value) => wire_event(part, 0xb, index, value),
        In::Bend(_, value) => wire_event(part, 0xe, (value & 127) as u8, (value >> 7) as u8),
        In::Pressure(_, value) => wire_event(part, 0xd, value, 0),
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
            auditions: vec![None; parts],
            overflow: 0,
            buses: Box::new([[[0.0; MAX_BLOCK]; 2]; BUSES]),
            written: [false; BUSES],
            scratch: Box::new([[0.0; 2]; MAX_BLOCK]),
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
        for (old, new) in self.auditions.iter().zip(&mut grown.auditions) {
            *new = *old;
        }
        std::mem::swap(&mut self.auditions, &mut grown.auditions);
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
            c.port == port && (c.channel < 0 || channel.is_none_or(|channel| c.channel == i16::from(channel)))
        })
    }

    /// The channel `event` arrives on, for part routing; None reaches every channel.
    fn channel(event: In) -> Option<u8> {
        match event {
            In::HostOn(note, ..) => Some(note.channel),
            In::HostOff(p) | In::HostChoke(p) | In::HostExpression(p, _) => u8::try_from(p.channel).ok(),
            In::NoteOn(c, ..) | In::NoteOff(c, _) | In::Cc(c, ..) | In::Bend(c, _) | In::Pressure(c, _) | In::PolyAt(c, ..) => {
                Some(c)
            }
            In::NoteTune(c, ..) | In::NotePressure(c, ..) | In::NoteGain(c, ..) | In::NotePan(c, ..) | In::NoteBrightness(c, ..) => {
                Some(c)
            }
        }
    }

    fn deliver(&mut self, part: usize, event: In) {
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        deliver(p, part, &mut self.held, &mut self.overflow, event);
    }

    fn routed(&mut self, port: u8, event: In, mut reached: Option<&mut [bool]>) {
        let channel = Self::channel(event);
        for part in 0..self.parts.len() {
            if self.reaches(part, port, channel) {
                self.deliver(part, event);
                if let Some(r) = reached.as_deref_mut().and_then(|r| r.get_mut(part)) {
                    *r = true;
                }
            }
        }
    }
}

impl Core for V2Core {
    type Prepared = Box<Part>;
    type Retired = Retired;
    type Live = <super::v1::V1Core as Core>::Live;

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

    fn install(&mut self, part: usize, mut prepared: Box<Part>) -> Retired {
        let Some(slot) = self.parts.get_mut(part) else { return Retired(Some(prepared)) };
        for held in self.held.iter_mut().filter(|h| h.part == part) {
            held.part = ORPHAN;
        }
        let tune = self.mix.parts.get(part).map_or(0.0, |c| c.tune);
        if tune != 0.0 && prepared.mpe.transpose(&mut prepared.runtime, f64::from(tune)).is_ok() {
            prepared.tune = tune;
        }
        Retired(slot.replace(prepared))
    }

    fn holding(&self, _playing: bool) -> bool {
        false
    }

    fn set_transport(&mut self, _transport: Transport) {}

    fn begin_block(&mut self, _block: &BlockInfo) {}

    fn event(&mut self, port: u8, event: In, _offset: u32, _holding: bool) {
        self.routed(port, event, None);
    }

    fn key_held(&self, channel: u8, key: u8, _holding: bool) -> bool {
        self.held.iter().any(|h| h.part != ORPHAN && h.note.channel == channel && h.note.key == key)
    }

    fn release_due(&mut self, _at: usize, limit: usize) -> usize {
        limit
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
            let out = &mut self.scratch[..n];
            if part.runtime.render(out).is_err() {
                continue;
            }
            let c = self.mix.parts[index];
            if c.mute || solo && !c.solo {
                continue;
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

    fn event_recorded(&mut self, port: u8, event: In, reached: &mut [bool]) {
        self.routed(port, event, Some(reached));
    }

    fn event_to(&mut self, parts: &mut [bool], event: In) {
        for (part, marked) in parts.iter_mut().enumerate() {
            if std::mem::take(marked) {
                self.deliver(part, event);
            }
        }
    }

    fn play(&mut self, part: usize, event: In) {
        self.deliver(part, event);
    }

    fn preview_channel(&self, part: usize) -> u8 {
        self.mix.parts.get(part).map_or(0, |c| c.channel.clamp(0, 15) as u8)
    }

    fn audition(&mut self, part: usize, note: Option<u8>) {
        self.audition_stop(part);
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        let key = note.unwrap_or(60).min(127);
        let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: AUDITION_CHANNEL, key, external_id: None };
        if p.runtime.trigger(input, key, 100.0 / 127.0).is_ok() {
            self.auditions[part] = Some(key);
        }
    }

    fn audition_stop(&mut self, part: usize) {
        let Some(key) = self.auditions.get_mut(part).and_then(Option::take) else { return };
        if let Some(Some(p)) = self.parts.get_mut(part) {
            let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: AUDITION_CHANNEL, key, external_id: None };
            let _ = p.runtime.note_off(input, None);
        }
    }

    fn set_macros(&mut self, _macros: Macros) {}

    fn set_load(&mut self, _load: f32) {}

    fn set_mix(&mut self, mix: &Mix) {
        // Field-wise so the parts vector keeps its audio-thread allocation.
        for (to, from) in self.mix.parts.iter_mut().zip(&mix.parts) {
            *to = *from;
        }
        self.mix.buses = mix.buses;
        for (p, c) in self.parts.iter_mut().zip(&self.mix.parts) {
            if let Some(p) = p
                && p.tune != c.tune
                && p.mpe.transpose(&mut p.runtime, f64::from(c.tune)).is_ok()
            {
                p.tune = c.tune;
            }
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

    // Script controls, persistence and the live view wait on the KSP compiler:
    // nothing to edit, save or show yet.
    fn ui_control(&mut self, _part: usize, _slot: usize, _control: usize, _value: i32) {}

    fn ui_file_selection(&mut self, _part: usize, _slot: usize, _control: usize, _path: &str) {}

    fn script_revision(&self, _part: usize) -> u64 {
        0
    }

    fn refresh_persistence(&self, _part: usize, _saved: &mut [Persisted], at: &mut Refresh, _budget: usize) -> bool {
        at.changed = false;
        true
    }

    fn refresh_live(&self, _part: usize, _live: &mut Self::Live, _at: &mut Refresh, _budget: usize, _unchanged: bool) -> bool {
        true
    }

    fn voices(&self) -> Voices {
        let active = self.parts.iter().flatten().map(|p| p.runtime.voice_count()).sum();
        Voices { active, audible: active, dropouts: self.overflow }
    }

    fn underruns(&self, _part: usize) -> u64 {
        0
    }

    fn latency(&self) -> u32 {
        0
    }
}

/// Prepares [`V2Core`] parts from Kontakt instruments and WAV files.
#[derive(Default)]
pub struct V2Loader;

/// Capacities of a part, sized for its plan's script state.
fn limits(plan: &Prepared) -> Limits {
    Limits {
        notes: NOTES,
        channels: 16,
        performances: 1,
        families: 256,
        decisions: 256,
        expressions: 128,
        voices: 512,
        commands: 256,
        behaviors: 16,
        behavior_fuel: 1 << 20,
        behavior_cells: plan.behavior_local_count().saturating_mul(16),
        note_cells: plan.note_cell_count().saturating_mul(128),
    }
}

fn is_wav(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

fn is_kontakt(path: &Path) -> bool {
    path.extension().is_some_and(|e| ["nki", "nkm", "nkb", "nksn"].iter().any(|k| e.eq_ignore_ascii_case(k)))
}

fn unsupported(path: &Path) -> CoreError {
    if is_kontakt(path) || is_wav(path) {
        CoreError::Invalid("unreadable instrument".into())
    } else {
        CoreError::Unsupported("translating this instrument format to sampler-core")
    }
}

fn report(instrument: &sampler_ir::Instrument) -> Vec<String> {
    instrument.unsupported.iter().map(|u| format!("{}: {} = {} ({:?})", u.location, u.feature, u.value, u.reason)).collect()
}

fn kontakt(
    request: &LoadRequest,
    progress: &mut dyn FnMut(Progress),
    canceled: &(dyn Fn() -> bool + Sync),
) -> Result<Prepared, CoreError> {
    let options = sampler_kontakt::Options { rate: request.sample_rate as u32, keys: 0..=127, scripts: true };
    let progress = |p: sampler_kontakt::Progress<'_>| {
        progress(Progress(match p {
            sampler_kontakt::Progress::Translated { .. } => 50,
            sampler_kontakt::Progress::Decoding { done, total, .. } => (100 + 800 * done / total.max(1)) as u16,
            sampler_kontakt::Progress::Lowering => 950,
        }))
    };
    match sampler_kontakt::load_cancelable(&request.path, &options, progress, canceled) {
        Ok(loaded) => Ok(loaded.plan),
        Err(sampler_kontakt::LoadError::Canceled) => Err(CoreError::Canceled),
        Err(e) => Err(CoreError::Load(e.to_string())),
    }
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

fn wav(request: &LoadRequest) -> Result<Prepared, CoreError> {
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
    Prepared::new(rate, vec![pcm], vec![region], 128).map_err(core)
}

impl CoreLoader for V2Loader {
    type Core = V2Core;

    fn prepare(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Box<Part>, CoreError> {
        if request.persisted.iter().any(|p| !p.is_empty()) {
            return Err(CoreError::Unsupported("restoring script state"));
        }
        let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
        let prepared = if is_kontakt(&request.path) {
            kontakt(request, progress, canceled)?
        } else if is_wav(&request.path) {
            wav(request)?
        } else {
            return Err(unsupported(&request.path));
        };
        if canceled() {
            return Err(CoreError::Canceled);
        }
        let limits = limits(&prepared);
        let runtime = Runtime::new(prepared, limits).map_err(core)?;
        let mpe = Mpe::new(&runtime, WIRE.port, WIRE.group, Zone::Lower, 15, NOTES).map_err(core)?;
        progress(Progress::DONE);
        Ok(Box::new(Part { runtime, mpe, tune: 0.0 }))
    }

    fn describe(&self, path: &Path, _program: u32) -> Result<Description, CoreError> {
        if is_kontakt(path) {
            let instrument = sampler_kontakt::read(path).map_err(|e| CoreError::Load(e.to_string()))?.instrument;
            return Ok(Description {
                name: instrument.name.clone(),
                zones: instrument.zones.len(),
                scripts: instrument.behaviors.len(),
                unsupported: report(&instrument),
            });
        }
        if !is_wav(path) {
            return Err(unsupported(path));
        }
        hound::WavReader::open(path).map_err(|e| CoreError::Load(e.to_string()))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Description { name, zones: 1, scripts: 0, unsupported: Vec::new() })
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

    fn loud(r: &Rendered<'_>, frames: usize) -> bool {
        r.live[0] && r.buses[0][0][..frames].iter().any(|x| x.abs() > 0.01)
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
        let prepared = loader.prepare(&request, &mut |p| done = Some(p), &|| false).unwrap();
        assert_eq!(done, Some(Progress::DONE));

        let mut core = V2Core::with_parts(2, 48000.0);
        assert!(core.install(0, prepared).0.is_none());
        let note = HostNote { port: 0, channel: 0, key: 60, id: 7, clap: true };
        core.begin_block(&BlockInfo { frames: 64, ..Default::default() });
        core.event(0, In::HostOn(note, 100, 0.0), 0, false);
        assert!(core.owns(note));
        assert!(core.key_held(0, 60, false));
        assert!(loud(&core.render(64), 64));
        assert_eq!(core.voices().active, 1);
        assert_eq!(core.end_block(64, &mut |_| panic!("still held")), 0);

        core.event(0, In::HostOff(HostPattern { port: -1, channel: -1, key: 60, id: -1, clap: true }), 0, false);
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
        assert!(!loud(&core.render(64), 64));
    }

    #[test]
    fn refused_note_end_is_retried_and_replaced_runtimes_orphan_their_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let load = || V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load());
        let note = HostNote { port: 0, channel: 0, key: 64, id: 3, clap: true };
        core.event(0, In::HostOn(note, 100, 0.0), 0, false);
        assert!(core.install(0, load()).0.is_some());
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
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap());
        let note = HostNote { port: 0, channel: 3, key: 60, id: 5, clap: true };
        let off = In::HostOff(HostPattern { port: -1, channel: -1, key: -1, id: 5, clap: true });
        let pitch = |core: &V2Core| {
            let part = core.parts[0].as_ref().unwrap();
            let h = core.held[0];
            part.runtime.expression(part.runtime.expression_id(h.id).unwrap()).unwrap().pitch_semitones
        };
        core.event(0, In::Cc(3, 64, 127), 0, false);
        core.event(0, In::HostOn(note, 100, 0.0), 0, false);
        core.event(0, In::Bend(3, 16383), 0, false);
        assert!((pitch(&core) - 2.0).abs() < 1e-3, "bend on any channel moves the part");
        core.event(0, off, 0, false);
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
        assert!((pitch(&core) - -10.0).abs() < 1e-3, "tune adds to the bend");
        let r = core.render(128);
        assert!(r.live[0] && r.live[2]);
        assert!(r.buses[0][0][..128].iter().all(|x| *x == 0.0), "panned hard right");
        assert!(r.buses[0][1][..128].iter().any(|x| x.abs() > 0.01));
        assert!(r.buses[2][1][..128].iter().all(|x| *x == 0.0), "aux bus muted");

        core.event(0, In::Cc(3, 64, 0), 0, false);
        let mut ended = Vec::new();
        for _ in 0..100 {
            core.render(128);
            core.end_block(128, &mut |n| { ended.push(n); true });
        }
        assert_eq!(ended, [note]);
    }

    #[test]
    fn unknown_formats_are_explicitly_unsupported() {
        let request = LoadRequest { path: "x.exs".into(), sample_rate: 48000.0, ..Default::default() };
        let err = V2Loader.prepare(&request, &mut |_| {}, &|| false).err().unwrap();
        assert!(matches!(err, CoreError::Unsupported(_)), "{err}");
    }

    /// Set `KONTRA_KONTAKT_LIBRARIES` to library roots to run; skips otherwise.
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
        let prepared = V2Loader.prepare(&request, &mut |p| last = p, &|| false).unwrap();
        assert_eq!(last, Progress::DONE);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, prepared);
        let note = HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true };
        core.event(0, In::HostOn(note, 100, 0.0), 0, false);
        let heard = (0..40).any(|_| loud(&core.render(128), 128));
        assert!(heard, "no output; unsupported: {:?}", description.unsupported);
    }
}
