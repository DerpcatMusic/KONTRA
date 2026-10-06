//! [`Core`] over `sampler-core`: one [`Runtime`] per rack part.
//!
//! Wired: host and MIDI notes (exact CLAP/VST3 ownership and NOTE_END), all
//! sound/notes off, rendering, the part gain/mute/solo/output-bus mix, peaks,
//! the scope tap, audition, voices, panic. Loading: WAV files, as one region
//! across the keyboard.
//!
//! Not yet (silently ignored on the audio thread, refused by the loader where a
//! request needs them): Kontakt translation, scripts and their UI, persistence,
//! automation and controllers (bend, pressure, CC other than 120/123, pedals),
//! note expressions and initial tuning, alignment, pan/tune/aux/mic outs, bus
//! faders, macros, load shedding, streaming, rack growth, sample-rate change
//! (needs a re-prepare).

use std::path::Path;

use sampler_core::{Envelope, Frame, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime};

use super::event::{HostNote, In};
use super::mix::{Mix, Peaks};
use super::view::{Persisted, Refresh};
use super::{
    BUSES, Block, BlockInfo, Core, CoreError, CoreLoader, Description, LoadRequest, MAX_BLOCK, Macros, Progress,
    Rendered, Transport, Voices,
};

/// Host notes tracked for ownership and NOTE_END across the rack.
const HELD: usize = 1024;
/// [`Held::part`] of a note whose runtime was replaced: ends at the next block.
const ORPHAN: usize = usize::MAX;
const AUDITION_CHANNEL: u8 = 15;

/// A replaced runtime, dropped on a worker.
#[derive(Default)]
pub struct Retired(pub Option<Box<Runtime>>);

#[derive(Clone, Copy)]
struct Held {
    part: usize,
    note: HostNote,
}

pub struct V2Core {
    parts: Vec<Option<Box<Runtime>>>,
    rate: f64,
    mix: Mix,
    held: Vec<Held>,
    auditions: Vec<Option<u8>>,
    /// Notes the ownership table had no room for.
    overflow: u64,
    buses: Box<[Block; BUSES]>,
    scratch: Box<[Frame; MAX_BLOCK]>,
    tap: Option<usize>,
    tapped: Box<[f32; MAX_BLOCK]>,
    peaks: Peaks,
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
            scratch: Box::new([[0.0; 2]; MAX_BLOCK]),
            tap: None,
            tapped: Box::new([0.0; MAX_BLOCK]),
            peaks,
        }
    }

    fn reaches(&self, part: usize, port: u8, channel: u8) -> bool {
        self.mix.parts.get(part).is_some_and(|c| c.port == port && (c.channel < 0 || c.channel == i16::from(channel)))
    }

    /// `event` to one part, if it has a runtime.
    fn deliver(&mut self, part: usize, port: u8, event: In) {
        let Some(Some(rt)) = self.parts.get_mut(part) else { return };
        let midi = |channel, key| Input { protocol: Protocol::Midi1, port: port.into(), group: 0, channel, key, external_id: None };
        match event {
            In::HostOn(note, velocity, _tune) => {
                if self.held.len() == HELD {
                    self.overflow += 1;
                } else if rt.trigger(host_input(note), note.key, f64::from(velocity) / 127.0).is_ok() {
                    self.held.push(Held { part, note });
                }
            }
            In::HostOff(pattern) | In::HostChoke(pattern) => {
                for held in self.held.iter().filter(|h| h.part == part && pattern.matches(h.note)) {
                    let _ = rt.note_off(host_input(held.note), None);
                }
            }
            In::NoteOn(channel, key, 0) | In::NoteOff(channel, key) => {
                let _ = rt.note_off(midi(channel, key), None);
            }
            In::NoteOn(channel, key, velocity) => {
                let _ = rt.trigger(midi(channel, key), key, f64::from(velocity) / 127.0);
            }
            In::Cc(_, 120 | 123, _) => rt.panic(),
            _ => {}
        }
    }

    /// The channel `event` arrives on, for part routing; None reaches every part.
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

    fn routed(&mut self, port: u8, event: In, mut reached: Option<&mut [bool]>) {
        // A host note goes to one part only, so it has exactly one owner.
        let first_only = matches!(event, In::HostOn(..));
        for part in 0..self.parts.len() {
            if self.reaches(part, port, Self::channel(event).unwrap_or(0)) || Self::channel(event).is_none() {
                self.deliver(part, port, event);
                if let Some(r) = reached.as_deref_mut().and_then(|r| r.get_mut(part)) {
                    *r = true;
                }
                if first_only {
                    return;
                }
            }
        }
    }
}

fn host_input(note: HostNote) -> Input {
    Input {
        protocol: if note.clap { Protocol::Clap } else { Protocol::Vst3 },
        port: note.port.into(),
        group: 0,
        channel: note.channel,
        key: note.key,
        external_id: (note.id >= 0).then_some(note.id),
    }
}

impl Core for V2Core {
    type Prepared = Box<Runtime>;
    type Retired = Retired;
    type Live = ();

    fn parts(&self) -> usize {
        self.parts.len()
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn reset(&mut self, sample_rate: f64) {
        // ponytail: runtimes keep their prepared rate; the shell must reload on a rate change.
        self.rate = sample_rate;
        self.panic();
    }

    fn panic(&mut self) {
        for rt in self.parts.iter_mut().flatten() {
            rt.panic();
        }
    }

    fn install(&mut self, part: usize, prepared: Box<Runtime>) -> Retired {
        let Some(slot) = self.parts.get_mut(part) else { return Retired(Some(prepared)) };
        for held in self.held.iter_mut().filter(|h| h.part == part) {
            held.part = ORPHAN;
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
        let frames = frames.min(MAX_BLOCK);
        for bus in self.buses.iter_mut() {
            bus[0][..frames].fill(0.0);
            bus[1][..frames].fill(0.0);
        }
        self.tapped[..frames].fill(0.0);
        let mut live = [false; BUSES];
        let solo = self.mix.parts.iter().take(self.parts.len()).any(|c| c.solo);
        for (part, rt) in self.parts.iter_mut().enumerate() {
            let Some(rt) = rt else { continue };
            let out = &mut self.scratch[..frames];
            if rt.render(out).is_err() {
                continue;
            }
            let c = self.mix.parts[part];
            let gain = if c.mute || solo && !c.solo { 0.0 } else { c.gain };
            let bus = usize::from(c.output).min(BUSES - 1);
            let peak = &mut self.peaks.parts[part];
            for (i, [l, r]) in out.iter().enumerate() {
                let (l, r) = (l * gain, r * gain);
                self.buses[bus][0][i] += l;
                self.buses[bus][1][i] += r;
                peak[0] = peak[0].max(l.abs());
                peak[1] = peak[1].max(r.abs());
                if self.tap == Some(part) {
                    self.tapped[i] = (l + r) * 0.5;
                }
            }
            live[bus] = true;
        }
        for (bus, peak) in self.peaks.buses.iter_mut().enumerate().filter(|(bus, _)| live[*bus]) {
            for (peak, signal) in peak.iter_mut().zip(&self.buses[bus]) {
                *peak = signal[..frames].iter().fold(*peak, |p, x| p.max(x.abs()));
            }
        }
        Rendered { buses: &self.buses, live }
    }

    fn owns(&self, note: HostNote) -> bool {
        self.held.iter().any(|h| h.part != ORPHAN && h.note == note)
    }

    fn end_block(&mut self, _frames: usize, end: &mut dyn FnMut(HostNote) -> bool) -> u64 {
        let Self { parts, held, .. } = self;
        let mut refused = 0;
        // Notes of replaced runtimes end now; their sound went with the runtime.
        let mut i = 0;
        while i < held.len() {
            if held[i].part != ORPHAN {
                i += 1;
            } else if !held[i].note.clap || end(held[i].note) {
                held.swap_remove(i);
            } else {
                return 1;
            }
        }
        for (part, rt) in parts.iter_mut().enumerate() {
            let Some(rt) = rt else { continue };
            rt.flush_ended(|input| {
                if !matches!(input.protocol, Protocol::Clap | Protocol::Vst3) {
                    return true;
                }
                let Some(at) = held.iter().position(|h| h.part == part && host_input(h.note) == input) else { return true };
                let note = held[at].note;
                if note.clap && !end(note) {
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
                let port = self.mix.parts.get(part).map_or(0, |c| c.port);
                self.deliver(part, port, event);
            }
        }
    }

    fn play(&mut self, part: usize, event: In) {
        let port = self.mix.parts.get(part).map_or(0, |c| c.port);
        self.deliver(part, port, event);
    }

    fn preview_channel(&self, part: usize) -> u8 {
        self.mix.parts.get(part).map_or(0, |c| c.channel.clamp(0, 15) as u8)
    }

    fn audition(&mut self, part: usize, note: Option<u8>) {
        self.audition_stop(part);
        let Some(Some(rt)) = self.parts.get_mut(part) else { return };
        let key = note.unwrap_or(60).min(127);
        let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: AUDITION_CHANNEL, key, external_id: None };
        if rt.trigger(input, key, 100.0 / 127.0).is_ok() {
            self.auditions[part] = Some(key);
        }
    }

    fn audition_stop(&mut self, part: usize) {
        let Some(key) = self.auditions.get_mut(part).and_then(Option::take) else { return };
        if let Some(Some(rt)) = self.parts.get_mut(part) {
            let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: AUDITION_CHANNEL, key, external_id: None };
            let _ = rt.note_off(input, None);
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

    fn ui_control(&mut self, _part: usize, _slot: usize, _control: usize, _value: i32) {}

    fn ui_file_selection(&mut self, _part: usize, _slot: usize, _control: usize, _path: &str) {}

    fn script_revision(&self, _part: usize) -> u64 {
        0
    }

    fn refresh_persistence(&self, _part: usize, _saved: &mut [Persisted], at: &mut Refresh, _budget: usize) -> bool {
        at.changed = false;
        true
    }

    fn refresh_live(&self, _part: usize, _live: &mut (), _at: &mut Refresh, _budget: usize, _unchanged: bool) -> bool {
        true
    }

    fn voices(&self) -> Voices {
        let active = self.parts.iter().flatten().map(|rt| rt.voice_count()).sum();
        Voices { active, audible: active, dropouts: self.overflow }
    }

    fn underruns(&self, _part: usize) -> u64 {
        0
    }

    fn latency(&self) -> u32 {
        0
    }
}

/// Prepares [`V2Core`] parts. Only WAV sources exist so far.
#[derive(Default)]
pub struct V2Loader;

fn limits() -> Limits {
    Limits {
        notes: 128,
        channels: 16,
        performances: 1,
        families: 256,
        decisions: 0,
        expressions: 128,
        voices: 256,
        commands: 256,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}

fn is_wav(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

fn unsupported(path: &Path) -> CoreError {
    if is_wav(path) {
        CoreError::Invalid("not a WAV file".into())
    } else {
        CoreError::Unsupported("translating this instrument format to sampler-core")
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

impl CoreLoader for V2Loader {
    type Core = V2Core;

    fn prepare(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Box<Runtime>, CoreError> {
        if !is_wav(&request.path) {
            return Err(unsupported(&request.path));
        }
        if request.persisted.iter().any(|p| !p.is_empty()) {
            return Err(CoreError::Unsupported("restoring script state"));
        }
        let rate = request.sample_rate as u32;
        let (source_rate, frames) = read_wav(&request.path)?;
        if canceled() {
            return Err(CoreError::Canceled);
        }
        let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
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
        let prepared = Prepared::new(rate, vec![pcm], vec![region], 128).map_err(core)?;
        let runtime = Runtime::new(prepared, limits()).map_err(core)?;
        progress(Progress::DONE);
        Ok(Box::new(runtime))
    }

    fn describe(&self, path: &Path, _program: u32) -> Result<Description, CoreError> {
        if !is_wav(path) {
            return Err(unsupported(path));
        }
        hound::WavReader::open(path).map_err(|e| CoreError::Load(e.to_string()))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Description { name, zones: 1, scripts: 0 })
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
    fn kontakt_sources_are_explicitly_unsupported() {
        let request = LoadRequest { path: "x.nki".into(), sample_rate: 48000.0, ..Default::default() };
        let err = V2Loader.prepare(&request, &mut |_| {}, &|| false).err().unwrap();
        assert!(matches!(err, CoreError::Unsupported(_)), "{err}");
    }
}
