//! One rack slot owns either an instrument or an executable instrument bank.
//! Editing an instrument is explicit; a bank never exposes its selected engine.

use super::{Engine, HostNote, HostPattern, HostExpression, StreamError};
use super::host_notes::HostRef;
use super::program_bank::ProgramBank;

pub struct RackPart {
    playback: Playback,
    // References stay in the canonical allocation pool until NOTE_END is accepted.
    retired_roots: Vec<HostRef>,
}

enum Playback {
    Instrument(Engine),
    InstrumentBank(Box<ProgramBank>),
}

impl Default for RackPart {
    fn default() -> Self { Engine::default().into() }
}

impl From<Engine> for RackPart {
    fn from(engine: Engine) -> Self {
        Self { playback: Playback::Instrument(engine), retired_roots: Vec::with_capacity(crate::ksp::EVENT_CAPACITY) }
    }
}

impl From<Box<ProgramBank>> for RackPart {
    fn from(bank: Box<ProgramBank>) -> Self {
        Self { playback: Playback::InstrumentBank(bank), retired_roots: Vec::with_capacity(crate::ksp::EVENT_CAPACITY) }
    }
}

impl RackPart {
    pub fn instrument(&self) -> Option<&Engine> {
        match &self.playback { Playback::Instrument(engine) => Some(engine), Playback::InstrumentBank(_) => None }
    }
    pub fn instrument_mut(&mut self) -> Option<&mut Engine> {
        match &mut self.playback { Playback::Instrument(engine) => Some(engine), Playback::InstrumentBank(_) => None }
    }
    pub fn is_instrument_bank(&self) -> bool { matches!(self.playback, Playback::InstrumentBank(_)) }
    pub(crate) fn program_bank_mut(&mut self) -> Option<&mut ProgramBank> {
        match &mut self.playback { Playback::Instrument(_) => None, Playback::InstrumentBank(bank) => Some(bank) }
    }
    pub(crate) fn input_port(&self) -> Option<u8> {
        match &self.playback { Playback::Instrument(_) => None, Playback::InstrumentBank(bank) => Some(bank.port()) }
    }
    pub(crate) fn matches_input(&self, port: u8, channel: u8) -> bool {
        match &self.playback { Playback::Instrument(_) => false, Playback::InstrumentBank(bank) => bank.matches_input(port, channel) }
    }
    pub fn rate(&self) -> f64 {
        match &self.playback { Playback::Instrument(engine) => engine.rate(), Playback::InstrumentBank(bank) => bank.rate() }
    }
    pub fn active_voices(&self) -> usize {
        match &self.playback { Playback::Instrument(engine) => engine.active_voices(), Playback::InstrumentBank(bank) => bank.active_voices() }
    }
    pub fn audible_voices(&self) -> usize {
        match &self.playback { Playback::Instrument(engine) => engine.audible_voices(), Playback::InstrumentBank(bank) => bank.audible_voices() }
    }
    pub fn underruns(&self) -> u64 {
        match &self.playback { Playback::Instrument(engine) => engine.underruns(), Playback::InstrumentBank(bank) => bank.underruns() }
    }
    pub fn dropped_commands(&self) -> u64 {
        match &self.playback { Playback::Instrument(engine) => engine.dropped_commands(), Playback::InstrumentBank(bank) => bank.dropped_commands() }
    }
    pub(crate) fn unsupported_native_controls(&self) -> u64 {
        match &self.playback { Playback::Instrument(engine) => engine.unsupported_native_controls(), Playback::InstrumentBank(bank) => bank.unsupported_inputs() }
    }
    pub fn pending_work(&self) -> [usize; 3] {
        match &self.playback { Playback::Instrument(engine) => engine.pending_work(), Playback::InstrumentBank(bank) => bank.pending_work() }
    }
    pub fn take_stream_error(&self) -> Option<StreamError> {
        match &self.playback { Playback::Instrument(engine) => engine.take_stream_error(), Playback::InstrumentBank(bank) => bank.take_stream_error() }
    }
    pub fn begin_audio_block(&mut self, frames: usize, parts: usize, offline: bool) {
        match &mut self.playback { Playback::Instrument(engine) => engine.begin_audio_block(frames, parts, offline), Playback::InstrumentBank(bank) => bank.begin_audio_block(frames, offline) }
    }
    pub fn set_transport(&mut self, playing: bool, tempo: f64, beats: f64, signature: (u8, u8)) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.set_transport(playing, tempo, beats, signature),
            Playback::InstrumentBank(bank) => { bank.set_transport(playing, Some(tempo), beats, signature); }
        }
    }
    pub fn set_blocking_streams(&mut self, offline: bool) {
        match &mut self.playback { Playback::Instrument(engine) => engine.blocking_streams = offline, Playback::InstrumentBank(bank) => bank.set_blocking_streams(offline) }
    }
    pub fn set_load(&mut self, load: f32) {
        match &mut self.playback { Playback::Instrument(engine) => engine.load = load, Playback::InstrumentBank(bank) => bank.set_load(load) }
    }
    pub fn set_sampler_defaults(&mut self, attack: f32, release: f32, cutoff: f32) {
        match &mut self.playback {
            Playback::Instrument(engine) => { engine.attack = attack; engine.release = release; engine.cutoff = cutoff; }
            Playback::InstrumentBank(bank) => bank.set_sampler_defaults(attack, release, cutoff),
        }
    }
    pub(crate) fn set_tune(&mut self, tune: f32) {
        match &mut self.playback { Playback::Instrument(engine) => engine.tune = tune, Playback::InstrumentBank(bank) => bank.set_tune(tune) }
    }
    pub fn panic(&mut self) {
        match &mut self.playback { Playback::Instrument(engine) => engine.panic(), Playback::InstrumentBank(bank) => bank.panic() }
    }
    pub(crate) fn initialize_frame(&mut self, now: u64) {
        match &mut self.playback { Playback::Instrument(engine) => engine.initialize_frame(now), Playback::InstrumentBank(bank) => bank.initialize_frame(now) }
    }
    pub(crate) fn with_peer_voices<R>(&mut self, peers: usize, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.instrument_mut().map(|engine| engine.peer_voices.replace(peers));
        let result = f(self);
        if let (Some(previous), Some(engine)) = (previous, self.instrument_mut()) { engine.peer_voices = previous; }
        result
    }
    pub(super) fn render_begin(&mut self, frames: usize) {
        match &mut self.playback { Playback::Instrument(engine) => engine.render_begin(frames), Playback::InstrumentBank(bank) => bank.render_begin(frames) }
    }
    pub(super) fn render_boundary(&mut self, at: usize) {
        match &mut self.playback { Playback::Instrument(engine) => engine.render_boundary(at), Playback::InstrumentBank(bank) => bank.render_boundary(at) }
    }
    pub(super) fn render_next_boundary(&self, at: usize) -> usize {
        match &self.playback { Playback::Instrument(engine) => engine.render_next_boundary(at), Playback::InstrumentBank(bank) => bank.render_next_boundary(at) }
    }
    pub(super) fn render_interval(&mut self, at: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        match &mut self.playback { Playback::Instrument(engine) => engine.render_interval(at, left, right), Playback::InstrumentBank(bank) => bank.render_interval(at, left, right) }
    }
    pub(super) fn render_finish(&mut self) {
        match &mut self.playback { Playback::Instrument(engine) => engine.render_finish(), Playback::InstrumentBank(bank) => bank.render_finish() }
    }
}
