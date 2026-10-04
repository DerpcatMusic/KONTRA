//! One rack slot owns either an instrument or an executable instrument bank.
//! Editing an instrument is explicit; a bank never exposes its selected engine.

use super::host_notes::HostRef;
use super::program_bank::ProgramBank;
use super::{Engine, HostExpression, HostNote, HostPattern, StreamError};

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
    fn default() -> Self {
        Engine::default().into()
    }
}

impl From<Engine> for RackPart {
    fn from(engine: Engine) -> Self {
        Self {
            playback: Playback::Instrument(engine),
            retired_roots: Vec::with_capacity(crate::ksp::EVENT_CAPACITY),
        }
    }
}

impl From<Box<ProgramBank>> for RackPart {
    fn from(bank: Box<ProgramBank>) -> Self {
        Self {
            playback: Playback::InstrumentBank(bank),
            retired_roots: Vec::with_capacity(crate::ksp::EVENT_CAPACITY),
        }
    }
}

impl RackPart {
    pub fn reset(&mut self, rate: f64) {
        match &mut self.playback {
            Playback::Instrument(e) => e.reset_input(rate, e.home_channel, false),
            Playback::InstrumentBank(b) => b.reset(rate),
        }
        self.mark_host_notes();
    }

    pub(super) fn reset_input(&mut self, rate: f64, home: u8, port_changed: bool) {
        match &mut self.playback {
            Playback::Instrument(e) => e.reset_input(rate, home, port_changed),
            // Receive channel is saved bank metadata, not a mutable rack filter.
            Playback::InstrumentBank(b) => b.reset(rate),
        }
    }
    pub(crate) fn set_home_channel(&mut self, home: u8) {
        if let Some(e) = self.instrument_mut() {
            e.set_home_channel(home);
        }
    }
    pub fn into_instrument(self) -> Result<Engine, Self> {
        if !self.retired_roots.is_empty() {
            return Err(self);
        }
        match self.playback {
            Playback::Instrument(engine) => Ok(engine),
            _ => Err(self),
        }
    }
    fn roots(&self) -> &super::host_notes::Storage {
        match &self.playback {
            Playback::Instrument(e) => e.player.host_notes.roots(),
            Playback::InstrumentBank(b) => b.roots(),
        }
    }
    fn roots_mut(&mut self) -> &mut super::host_notes::Storage {
        match &mut self.playback {
            Playback::Instrument(e) => e.player.host_notes.roots_mut(),
            Playback::InstrumentBank(b) => b.roots_mut(),
        }
    }
    /// Adopt worker-prepared playback and return the old heap owners in `next`.
    /// Terminal references remain in the same allocation pool across replacement.
    pub(crate) fn replace_prepared(&mut self, next: &mut Self) -> bool {
        if !next.roots().is_empty() || !next.retired_roots.is_empty() {
            return false;
        }
        let rate = next.rate();
        self.reset(rate);
        let detached = match &mut self.playback {
            Playback::Instrument(e) => e
                .player
                .host_notes
                .detach_terminal_refs(&mut self.retired_roots),
            Playback::InstrumentBank(b) => b.detach_terminal_refs(&mut self.retired_roots),
        };
        if !detached {
            return false;
        }
        self.roots_mut().set_shared();
        std::mem::swap(self.roots_mut(), next.roots_mut());
        std::mem::swap(&mut self.playback, &mut next.playback);
        true
    }
    pub(crate) fn audio_block_start(&self) -> u64 {
        match &self.playback {
            Playback::Instrument(e) => e.audio_block_start(),
            Playback::InstrumentBank(b) => b.audio_block_start(),
        }
    }
    pub(crate) fn mark_host_notes(&mut self) {
        match &mut self.playback {
            Playback::Instrument(e) => e.mark_host_notes(),
            Playback::InstrumentBank(b) => b.mark_host_notes(),
        }
    }
    pub(crate) fn host_note_at(&self, index: usize) -> Option<(HostNote, bool)> {
        if index < self.retired_roots.len() {
            return self
                .roots()
                .terminal(self.retired_roots[index])
                .map(|(note, _)| (note, false));
        }
        let index = index - self.retired_roots.len();
        match &self.playback {
            Playback::Instrument(e) => e.host_note_at(index),
            Playback::InstrumentBank(b) => b.host_note_at(index),
        }
    }
    pub(crate) fn host_note_present(&self, note: HostNote) -> bool {
        self.retired_roots
            .iter()
            .any(|id| self.roots().terminal(*id).is_some_and(|(n, _)| n == note))
            || match &self.playback {
                Playback::Instrument(e) => e.host_note_present(note),
                Playback::InstrumentBank(b) => b.host_note_present(note),
            }
    }
    pub(crate) fn host_note_pending(&self, note: HostNote) -> bool {
        match &self.playback {
            Playback::Instrument(e) => e.host_note_pending(note),
            Playback::InstrumentBank(b) => b.host_note_pending(note),
        }
    }
    pub(crate) fn host_note_end_at(&self, note: HostNote) -> Option<u64> {
        self.retired_roots
            .iter()
            .filter_map(|id| self.roots().terminal(*id))
            .filter(|(n, _)| *n == note)
            .map(|(_, end)| end)
            .chain(match &self.playback {
                Playback::Instrument(e) => e.host_note_end_at(note),
                Playback::InstrumentBank(b) => b.host_note_end_at(note),
            })
            .max()
    }
    pub(crate) fn retire_host_note(&mut self, note: HostNote) {
        let mut index = 0;
        while index < self.retired_roots.len() {
            let id = self.retired_roots[index];
            if self.roots().terminal(id).is_some_and(|(n, _)| n == note) {
                self.roots_mut().retire_terminal(id);
                self.retired_roots.remove(index);
            } else {
                index += 1;
            }
        }
        match &mut self.playback {
            Playback::Instrument(e) => e.retire_host_note(note),
            Playback::InstrumentBank(b) => b.retire_host_note(note),
        }
    }
    pub(crate) fn host_key_held(&self, channel: u8, key: u8) -> bool {
        match &self.playback {
            Playback::Instrument(e) => e.host_key_held(channel, key),
            Playback::InstrumentBank(b) => b.host_key_held(channel, key),
        }
    }
    pub(crate) fn host_note_drops(&self) -> u64 {
        match &self.playback {
            Playback::Instrument(e) => e.host_note_drops(),
            Playback::InstrumentBank(b) => b.host_note_drops(),
        }
    }
    pub(crate) fn bank_input(&mut self, port: u8, input: crate::articulate::In) -> bool {
        use crate::articulate::In;
        let Some(bank) = self.program_bank_mut() else {
            return false;
        };
        match input {
            In::Midi1NoteOn(c, k, v) | In::NoteOn(c, k, v) => {
                bank.midi1(port, &[0x90 | c, k, v]);
            }
            In::Midi1NoteOff(c, k, v) => {
                bank.midi1(port, &[0x80 | c, k, v]);
            }
            In::Midi1ZeroNoteOff(c, k) => {
                bank.midi1(port, &[0x90 | c, k, 0]);
            }
            In::NoteOff(c, k) => {
                bank.midi1(port, &[0x80 | c, k, 64]);
            }
            In::Midi1Cc(c, cc, v) | In::Cc(c, cc, v) => {
                bank.midi1(port, &[0xb0 | c, cc, v]);
            }
            In::Bend(c, v) => {
                bank.midi1(port, &[0xe0 | c, (v & 127) as u8, (v >> 7) as u8]);
            }
            In::Pressure(c, v) => {
                bank.midi1(port, &[0xd0 | c, v]);
            }
            In::HostOn(n, v, tune) => {
                bank.host_on(n, v, tune);
            }
            In::HostOff(n) => bank.host_off(n, crate::midi::Velocity::from_midi1(64)),
            In::HostOffVelocity(n, v) => bank.host_off(n, v),
            In::HostExpression(n, v) => bank.host_expression(n, v),
            In::HostPressure(n, v) => bank.host_expression(n, HostExpression::Pressure(v)),
            In::HostChoke(n) => bank.host_choke(n),
            _ => {
                bank.reject_input();
            }
        }
        true
    }
    pub fn note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        match &mut self.playback {
            Playback::Instrument(e) => e.note_on(channel, key, velocity),
            Playback::InstrumentBank(b) => {
                b.midi1(b.port(), &[0x90 | channel, key, velocity]);
            }
        }
    }
    pub fn note_off(&mut self, channel: u8, key: u8) {
        match &mut self.playback {
            Playback::Instrument(e) => e.note_off(channel, key),
            Playback::InstrumentBank(b) => {
                b.midi1(b.port(), &[0x80 | channel, key, 64]);
            }
        }
    }
    pub fn cc(&mut self, channel: u8, cc: u8, value: u8) {
        match &mut self.playback {
            Playback::Instrument(e) => e.cc(channel, cc, value),
            Playback::InstrumentBank(b) => {
                b.midi1(b.port(), &[0xb0 | channel, cc, value]);
            }
        }
    }
    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        match &mut self.playback {
            Playback::Instrument(e) => e.pitch_bend(channel, value),
            Playback::InstrumentBank(b) => {
                b.midi1(
                    b.port(),
                    &[0xe0 | channel, (value & 127) as u8, (value >> 7) as u8],
                );
            }
        }
    }
    pub fn channel_pressure(&mut self, channel: u8, value: u8) {
        match &mut self.playback {
            Playback::Instrument(e) => e.channel_pressure(channel, value),
            Playback::InstrumentBank(b) => {
                b.midi1(b.port(), &[0xd0 | channel, value]);
            }
        }
    }
    pub fn poly_pressure(&mut self, channel: u8, key: u8, value: u8) {
        match &mut self.playback {
            Playback::Instrument(e) => e.poly_pressure(channel, key, value),
            Playback::InstrumentBank(b) => {
                b.reject_input();
            }
        }
    }
    pub(super) fn render_output_offset(&self, at: usize) -> usize {
        self.instrument().map_or(0, |e| e.render_output_offset(at))
    }
    pub(super) fn direct_outs_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (usize, &[f32], &[f32])> {
        self.instrument()
            .into_iter()
            .flat_map(move |e| e.fx().direct_outs_range(range.clone()))
    }
    pub fn instrument(&self) -> Option<&Engine> {
        match &self.playback {
            Playback::Instrument(engine) => Some(engine),
            Playback::InstrumentBank(_) => None,
        }
    }
    pub fn instrument_mut(&mut self) -> Option<&mut Engine> {
        match &mut self.playback {
            Playback::Instrument(engine) => Some(engine),
            Playback::InstrumentBank(_) => None,
        }
    }
    pub fn is_instrument_bank(&self) -> bool {
        matches!(self.playback, Playback::InstrumentBank(_))
    }
    pub(crate) fn program_bank_mut(&mut self) -> Option<&mut ProgramBank> {
        match &mut self.playback {
            Playback::Instrument(_) => None,
            Playback::InstrumentBank(bank) => Some(bank),
        }
    }
    pub(crate) fn input_port(&self) -> Option<u8> {
        match &self.playback {
            Playback::Instrument(_) => None,
            Playback::InstrumentBank(bank) => Some(bank.port()),
        }
    }
    pub(crate) fn matches_input(&self, port: u8, channel: u8) -> bool {
        match &self.playback {
            Playback::Instrument(_) => false,
            Playback::InstrumentBank(bank) => bank.matches_input(port, channel),
        }
    }
    pub fn rate(&self) -> f64 {
        match &self.playback {
            Playback::Instrument(engine) => engine.rate(),
            Playback::InstrumentBank(bank) => bank.rate(),
        }
    }
    pub fn active_voices(&self) -> usize {
        match &self.playback {
            Playback::Instrument(engine) => engine.active_voices(),
            Playback::InstrumentBank(bank) => bank.active_voices(),
        }
    }
    pub fn audible_voices(&self) -> usize {
        match &self.playback {
            Playback::Instrument(engine) => engine.audible_voices(),
            Playback::InstrumentBank(bank) => bank.audible_voices(),
        }
    }
    pub fn underruns(&self) -> u64 {
        match &self.playback {
            Playback::Instrument(engine) => engine.underruns(),
            Playback::InstrumentBank(bank) => bank.underruns(),
        }
    }
    pub fn dropped_commands(&self) -> u64 {
        match &self.playback {
            Playback::Instrument(engine) => engine.dropped_commands(),
            Playback::InstrumentBank(bank) => bank.dropped_commands(),
        }
    }
    pub(crate) fn unsupported_native_controls(&self) -> u64 {
        match &self.playback {
            Playback::Instrument(engine) => engine.unsupported_native_controls(),
            Playback::InstrumentBank(bank) => bank.unsupported_inputs(),
        }
    }
    pub fn pending_work(&self) -> [usize; 3] {
        match &self.playback {
            Playback::Instrument(engine) => engine.pending_work(),
            Playback::InstrumentBank(bank) => bank.pending_work(),
        }
    }
    pub fn take_stream_error(&self) -> Option<StreamError> {
        match &self.playback {
            Playback::Instrument(engine) => engine.take_stream_error(),
            Playback::InstrumentBank(bank) => bank.take_stream_error(),
        }
    }
    pub fn begin_audio_block(&mut self, frames: usize, parts: usize, offline: bool) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.begin_audio_block(frames, parts, offline),
            Playback::InstrumentBank(bank) => bank.begin_audio_block(frames, offline),
        }
    }
    pub fn set_transport(&mut self, playing: bool, tempo: f64, beats: f64, signature: (u8, u8)) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.set_transport(playing, tempo, beats, signature),
            Playback::InstrumentBank(bank) => {
                bank.set_transport(playing, Some(tempo), beats, signature);
            }
        }
    }
    pub fn set_blocking_streams(&mut self, offline: bool) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.blocking_streams = offline,
            Playback::InstrumentBank(bank) => bank.set_blocking_streams(offline),
        }
    }
    pub fn set_load(&mut self, load: f32) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.load = load,
            Playback::InstrumentBank(bank) => bank.set_load(load),
        }
    }
    pub fn set_sampler_defaults(&mut self, attack: f32, release: f32, cutoff: f32) {
        match &mut self.playback {
            Playback::Instrument(engine) => {
                engine.attack = attack;
                engine.release = release;
                engine.cutoff = cutoff;
            }
            Playback::InstrumentBank(bank) => bank.set_sampler_defaults(attack, release, cutoff),
        }
    }
    pub(crate) fn set_tune(&mut self, tune: f32) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.tune = tune,
            Playback::InstrumentBank(bank) => bank.set_tune(tune),
        }
    }
    pub fn panic(&mut self) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.panic(),
            Playback::InstrumentBank(bank) => bank.panic(),
        }
    }
    pub(crate) fn initialize_frame(&mut self, now: u64) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.initialize_frame(now),
            Playback::InstrumentBank(bank) => bank.initialize_frame(now),
        }
    }
    pub(crate) fn with_peer_voices<R>(
        &mut self,
        peers: usize,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self
            .instrument_mut()
            .map(|engine| engine.peer_voices.replace(peers));
        let result = f(self);
        if let (Some(previous), Some(engine)) = (previous, self.instrument_mut()) {
            engine.peer_voices = previous;
        }
        result
    }
    pub(super) fn render_begin(&mut self, frames: usize) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.render_begin(frames),
            Playback::InstrumentBank(bank) => bank.render_begin(frames),
        }
    }
    pub(super) fn render_boundary(&mut self, at: usize) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.render_boundary(at),
            Playback::InstrumentBank(bank) => bank.render_boundary(at),
        }
    }
    pub(super) fn render_next_boundary(&self, at: usize) -> usize {
        match &self.playback {
            Playback::Instrument(engine) => engine.render_next_boundary(at),
            Playback::InstrumentBank(bank) => bank.render_next_boundary(at),
        }
    }
    pub(super) fn render_interval(
        &mut self,
        at: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> bool {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.render_interval(at, left, right),
            Playback::InstrumentBank(bank) => bank.render_interval(at, left, right),
        }
    }
    pub(super) fn render_finish(&mut self) {
        match &mut self.playback {
            Playback::Instrument(engine) => engine.render_finish(),
            Playback::InstrumentBank(bank) => bank.render_finish(),
        }
    }
}
