//! Script-free retained instrument-bank playback. Plugin admission remains separate.
use super::{Engine, HostExpression, HostNote, HostPattern, MAX_BLOCK, Velocity};
use anyhow::{Context, Result, ensure};
use std::path::Path;

/// The caller associates saved host selections 0/1 with this stereo sink.
/// This is explicit KONTRA routing, not a native output-record index cast.
#[derive(Clone, Copy, Debug)]
pub struct MainStereo;
#[cfg(all(test, feature = "plugin"))]
#[path = "program_bank_tests.rs"]
mod tests;
use super::group_start::Receiver;
use super::host_notes::Storage;
use crate::midi::{Bend, Control, ControlValue, Midi1Decoder, ParameterData};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Accepted,
    Empty,
    WrongAddress,
    Invalid,
    Unsupported,
    Exhausted,
}
struct Program {
    slot: u8,
    engine: Engine,
}

/// Inner audio owners have no public ingress or source-replacement handle.
/// Only this owner consumes wire transaction and velocity-prefix state.
pub struct ProgramBank {
    programs: Vec<Program>,
    lookup: [Option<u8>; 128],
    selected: u8,
    port: u8,
    channel: u8,
    received: Midi1Decoder,
    roots: Storage,
    raw: Box<Receiver>,
    requested_sustain: u128,
    controls: super::instrument::InstrumentControls,
    amplifier: super::instrument::ProgramAmplifier,
    now: u64,
    block_start: u64,
    saved_tempo: f64,
    scratch: [[f32; MAX_BLOCK]; 2],
    unsupported: u64,
    warnings: Vec<String>,
}

/// Restore both canonical owners even when an inner operation unwinds.
struct Loan<'a> {
    engine: &'a mut Engine,
    roots: &'a mut Storage,
    raw: &'a mut Box<Receiver>,
}
impl<'a> Loan<'a> {
    fn new(engine: &'a mut Engine, roots: &'a mut Storage, raw: &'a mut Box<Receiver>) -> Self {
        engine.player.host_notes.swap_storage(roots);
        std::mem::swap(&mut engine.player.library_receiver, raw);
        Self { engine, roots, raw }
    }
}
impl Drop for Loan<'_> {
    fn drop(&mut self) {
        self.engine.player.host_notes.swap_storage(self.roots);
        std::mem::swap(&mut self.engine.player.library_receiver, self.raw);
    }
}

impl ProgramBank {
    /// Worker-side preparation from the actual NKM container and sparse roster.
    pub fn load(
        path: &Path,
        outer_slot: u32,
        port: u8,
        rate: f64,
        output: MainStereo,
    ) -> Result<Self> {
        Self::prepare(
            crate::import::read_instrument_bank(path, outer_slot)?,
            port,
            rate,
            output,
        )
    }
    pub(crate) fn prepare(
        imported: crate::import::InstrumentBank,
        port: u8,
        rate: f64,
        _output: MainStereo,
    ) -> Result<Self> {
        ensure!(
            rate.is_finite() && rate > 0.,
            "Invalid instrument-bank sample rate"
        );
        let p = imported
            .container
            .private_params()
            .context("Instrument-bank container private state")?;
        ensure!(
            p.mode == 2 && p.flags == [0; 4] && p.selected < 128,
            "Unsupported instrument-bank mode/flags/selection"
        );
        let c = imported
            .container
            .params()
            .context("Instrument-bank container controls")?;
        ensure!(
            c.volume.is_finite()
                && c.volume >= 0.
                && c.pan.is_finite()
                && (-1. ..=1.).contains(&c.pan),
            "Invalid instrument-bank controls"
        );
        let slots = imported
            .bank
            .slot_settings()
            .context("Instrument-bank receive settings")?;
        let route = slots
            .get(imported.outer_slot as usize)
            .context("Instrument-bank outer slot is outside its receive roster")?;
        ensure!(
            (1..=16).contains(&route.channel)
                && route.flags == [0; 2]
                && route.auxiliary.is_empty(),
            "Unsupported instrument-bank receive/auxiliary settings"
        );
        let master = imported
            .bank
            .params()
            .context("Instrument-bank master controls")?;
        ensure!(
            master.master_volume == 1. && master.master_tune == 1. && master.master_tempo > 0,
            "Unsupported instrument-bank master gain/tune/tempo"
        );
        let saved_tempo = f64::from(master.master_tempo) / 1000.;
        ensure!(
            imported
                .container
                .0
                .children
                .iter()
                .all(|c| matches!(c.id, 0x2b | 0x36)),
            "Unsupported instrument-bank container child"
        );
        ensure!(
            imported.bank.0.private_data.is_empty(),
            "Unsupported instrument-bank private metadata"
        );
        ensure!(
            imported
                .bank
                .0
                .children
                .iter()
                .all(|c| matches!(c.id, 6 | 0x3e | 0x37 | 0x48 | 0x47)),
            "Unsupported instrument-bank parent child"
        );
        let output = imported
            .bank
            .output_configuration()
            .context("Instrument-bank output settings")?;
        ensure!(
            output.records.len() == 1 && p.output_geometry == 0x20000,
            "Unsupported instrument-bank output geometry"
        );
        let out = &output.records[0];
        ensure!(
            out.index == 0
                && out.channels == 2
                && out.auxiliary == -1
                && out.volume == 1.
                && out.fx.is_empty(),
            "Unsupported instrument-bank output processing"
        );
        // Checked against the caller's explicit MainStereo association.
        ensure!(
            output.mapping == [0, 1],
            "Unsupported instrument-bank host output mapping"
        );
        let budget = imported.container.voice_group()?.params()?;
        ensure!(
            budget.kill_mode <= 4 && budget.prefer_released <= 1 && budget.exclusion_group < 0,
            "Unsupported instrument-bank shared voice rule"
        );
        ensure!(
            budget.max_voices > 0 && budget.fade_ms <= i32::MAX as u32,
            "Invalid instrument-bank voice budget/fade"
        );
        let voice_limit = crate::import::VoiceLimit {
            max_voices: budget.max_voices,
            kill_mode: budget.kill_mode as i16,
            prefer_released: budget.prefer_released != 0,
            fade_ms: budget.fade_ms,
            exclusion_group: budget.exclusion_group,
        };
        ensure!(
            !imported.programs.is_empty(),
            "Empty instrument-bank roster"
        );
        let mut programs = Vec::with_capacity(imported.programs.len());
        let mut lookup = [None; 128];
        let mut roots = None;
        let mut warnings = Vec::new();
        for (slot, mut instrument) in imported.programs {
            ensure!(
                instrument.scripts.iter().all(|s| s.trim().is_empty()),
                "Scripted instrument banks are unsupported"
            );
            ensure!(
                instrument
                    .groups
                    .iter()
                    .all(|g| g.native_shared_volume_slot.is_none()),
                "Instrument-bank shared AHDSR accepted ranges are unsupported"
            );
            ensure!(
                instrument.fx.buses.iter().all(|b| b.output == -1)
                    && instrument.fx.send_outputs.iter().all(|s| s.channel == -1),
                "Instrument-bank direct program output routing is unsupported"
            );
            // Mode2 returned sends encounter a separate retained program amp
            // before the inner amp. Until that owner exists, admit only its
            // neutral setting, or programs with no returned-send effects.
            ensure!(
                instrument.fx.send.slots.is_empty()
                    || (instrument.program.gain == 1. && instrument.program.pan == 0.),
                "Instrument-bank nonunit amplifier with returned-send FX is unsupported"
            );
            ensure!(
                lookup[slot as usize].is_none(),
                "Duplicate instrument-bank program slot"
            );
            // Native bank admission uses the container rule, not the old program rule.
            // The container supplies the rule; each retained inner roster counts
            // and selects its own victims. Other programs never enter this census.
            instrument.voice_limit = Some(voice_limit);
            let options = instrument
                .native_program_private
                .as_ref()
                .context("Instrument-bank requires known native controller policy")?
                .controller_options()?
                .context("Opaque instrument-bank controller policy")?;
            ensure!(
                !options.standard_volume_pan,
                "Instrument-bank inner standard volume/pan is unsupported"
            );
            warnings.extend(
                instrument
                    .warnings
                    .iter()
                    .map(|w| format!("Program {slot}: {w}")),
            );
            let bank = super::Bank::load(&instrument)?;
            warnings.extend(bank.issues.iter().map(|w| format!("Program {slot}: {w}")));
            if let Some(w) = &bank.warning {
                warnings.push(format!("Program {slot}: {w}"));
            }
            let fx = super::effects(&instrument, None, rate as f32);
            let mut engine = Engine::default();
            engine.reset(rate);
            engine.set_bank(Some(Box::new(bank)));
            engine.set_fx(fx);
            engine.set_home_channel((route.channel - 1) as u8);
            engine.set_transport(false, saved_tempo, 0., (4, 4));
            engine.player.per_note = super::per_note::InputControls::disabled();
            let storage = engine.player.host_notes.take_storage();
            if roots.is_none() {
                roots = Some(storage);
            }
            lookup[slot as usize] = Some(programs.len() as u8);
            programs.push(Program { slot, engine });
        }
        Ok(Self {
            programs,
            lookup,
            selected: p.selected as u8,
            port,
            channel: (route.channel - 1) as u8,
            received: Midi1Decoder::default(),
            roots: roots.unwrap(),
            raw: Box::default(),
            requested_sustain: 0,
            controls: super::instrument::InstrumentControls {
                gain: c.volume,
                pan: c.pan,
                tune: 0.,
            },
            amplifier: Default::default(),
            now: 0,
            block_start: 0,
            saved_tempo,
            scratch: [[0.; MAX_BLOCK]; 2],
            unsupported: 0,
            warnings,
        })
    }
    fn with_program<R>(&mut self, index: usize, f: impl FnOnce(&mut Engine) -> R) -> R {
        let loan = Loan::new(
            &mut self.programs[index].engine,
            &mut self.roots,
            &mut self.raw,
        );
        f(loan.engine)
    }
    fn selected_index(&self) -> Option<usize> {
        self.lookup[self.selected as usize].map(usize::from)
    }
    fn matches(&self, port: u8, channel: u8) -> bool {
        port == self.port && channel == self.channel
    }
    fn reject(&mut self) -> Admission {
        self.unsupported = self.unsupported.saturating_add(1);
        Admission::Unsupported
    }
    pub fn selected(&self) -> u8 {
        self.selected
    }
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
    pub(crate) fn reject_input(&mut self) {
        self.reject();
    }
    pub(crate) fn host_choke(&mut self, pattern: HostPattern) {
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.host_note_choke(pattern));
        }
    }
    pub fn unsupported_inputs(&self) -> u64 {
        self.unsupported
    }
    pub fn active_voices(&self) -> usize {
        self.programs.iter().map(|p| p.engine.active_voices()).sum()
    }
    pub fn begin_audio_block(&mut self, frames: usize, offline: bool) {
        self.block_start = self.now;
        let count = self.programs.len();
        for index in 0..count {
            self.with_program(index, |e| e.begin_audio_block(frames, count, offline));
        }
    }
    /// Caller renders preceding frames before supplying each ordered receipt.
    /// MPE, MIDI2/PCK, articulation, CC7/10 and custom requests are outside this slice.
    pub fn midi1(&mut self, port: u8, bytes: &[u8]) -> Admission {
        let Some(&status) = bytes.first() else {
            return Admission::Invalid;
        };
        if !(0x80..0xf0).contains(&status) {
            return self.reject();
        }
        let kind = status >> 4;
        let channel = status & 15;
        let count = if matches!(kind, 12 | 13) { 2 } else { 3 };
        if bytes.len() != count || bytes[1..].iter().any(|v| *v >= 128) {
            return Admission::Invalid;
        }
        if !self.matches(port, channel) {
            return Admission::WrongAddress;
        }
        let key = bytes[1];
        match kind {
            8 => {
                let v = self
                    .received
                    .release_velocity(bytes[2])
                    .unwrap_or_else(|| Velocity::from_midi1(bytes[2]));
                self.off(channel, key, v);
                Admission::Accepted
            }
            9 if bytes[2] == 0 => {
                self.received.velocity(0);
                self.off(channel, key, Velocity::from_midi1(64));
                Admission::Accepted
            }
            9 => {
                let v = self
                    .received
                    .velocity(bytes[2])
                    .unwrap_or_else(|| Velocity::from_midi1(bytes[2]));
                let Some(index) = self.selected_index() else {
                    return Admission::Empty;
                };
                self.with_program(index, |e| e.note_on_from_velocity(channel, channel, key, v));
                Admission::Accepted
            }
            12 => {
                self.selected = key;
                if self.selected_index().is_some() {
                    Admission::Accepted
                } else {
                    Admission::Empty
                }
            }
            11 => {
                let received = self.received.cc(key, bytes[2]).unwrap();
                self.raw.control(channel, received.control);
                if matches!(key, 7 | 10 | 39 | 42) {
                    return self.reject();
                }
                if key == 121 {
                    let defaults = self.selected_index().map_or([None; 128], |i| {
                        self.programs[i].engine.player.midi.controller_defaults
                    });
                    self.received.reset_controllers(&defaults);
                }
                let target = self.selected_index();
                let selected_slot = self.selected;
                let requested_control = received.control;
                let unsupported_parameter = received
                    .parameter
                    .is_some_and(|p| !p.registered || !matches!(p.address, 0..=2));
                if let Some(index) = target {
                    // Configuration and accepted playback share the same decoded receipt.
                    let mut baseline = None;
                    self.with_program(index, |e| {
                        if let Some(parameter) = received.parameter {
                            let applied = e.player.midi.apply_midi1_parameter(channel, parameter);
                            if applied
                                && matches!(
                                    parameter.data,
                                    ParameterData::Increment | ParameterData::Decrement
                                )
                            {
                                baseline = Some(parameter.address);
                            }
                        }
                        e.control_from_received_scoped_inner(
                            channel,
                            channel,
                            received,
                            Some(port),
                            false,
                        );
                    });
                    if let Some(address) = baseline {
                        self.programs[index]
                            .engine
                            .player
                            .midi
                            .seed_parameter_baseline(channel, address, &mut self.received);
                    }
                }
                if key == 64 {
                    let requests = std::mem::take(&mut self.requested_sustain);
                    for index in 0..self.programs.len() {
                        let slot = self.programs[index].slot;
                        if requests & (1u128 << slot) != 0
                            && !(target.is_some() && slot == selected_slot)
                        {
                            self.with_program(index, |e| {
                                // Requested programs accept the same canonical
                                // pedal value, without another wire decoder.
                                e.control_from_received_scoped_inner(
                                    channel,
                                    channel,
                                    crate::midi::ReceivedControl::without_parameter(
                                        requested_control,
                                    ),
                                    Some(port),
                                    false,
                                )
                            });
                        }
                    }
                }
                if unsupported_parameter {
                    self.reject()
                } else if target.is_some() {
                    Admission::Accepted
                } else {
                    Admission::Empty
                }
            }
            13 | 14 => {
                let control = if kind == 13 {
                    Control::Pressure(ControlValue::from_midi1(key))
                } else {
                    Control::Bend(Bend::from_midi1(
                        u16::from(key) | (u16::from(bytes[2]) << 7),
                    ))
                };
                self.raw.control(channel, control);
                let Some(index) = self.selected_index() else {
                    return Admission::Empty;
                };
                self.with_program(index, |e| {
                    e.control_from_scoped(channel, channel, control, false)
                });
                Admission::Accepted
            }
            _ => self.reject(),
        }
    }
    fn off(&mut self, channel: u8, key: u8, velocity: Velocity) {
        for index in 0..self.programs.len() {
            let request = self.with_program(index, |e| {
                if !e.input_key_down_from(channel, channel, key) {
                    return false;
                }
                let request =
                    e.player.native_sustain && e.player.pedals.holds(channel, false, false);
                e.note_off_from_velocity(channel, channel, key, velocity);
                request
            });
            if request {
                self.requested_sustain |= 1u128 << self.programs[index].slot;
            }
        }
    }
    pub fn host_on(&mut self, note: HostNote, velocity: Velocity, tune: f32) -> Admission {
        if !self.matches(note.port, note.channel) {
            return Admission::WrongAddress;
        }
        if note.key >= 128 || !tune.is_finite() {
            return Admission::Invalid;
        }
        let Some(index) = self.selected_index() else {
            return Admission::Empty;
        };
        if self.with_program(index, |e| {
            e.host_note_on_velocity_inner(note, note.channel, note.key, velocity, tune, false)
        }) {
            Admission::Accepted
        } else {
            Admission::Exhausted
        }
    }
    pub fn host_off(&mut self, pattern: HostPattern, velocity: Velocity) {
        let channel = self.channel;
        for index in 0..self.programs.len() {
            let request = self.with_program(index, |e| {
                let held = e
                    .player
                    .host_notes
                    .active
                    .iter()
                    .filter_map(|id| e.player.host_notes.get(*id))
                    .any(|o| o.held && pattern.matches(o.note));
                let request =
                    held && e.player.native_sustain && e.player.pedals.holds(channel, false, false);
                e.host_note_off_velocity(pattern, velocity);
                request
            });
            if request {
                self.requested_sustain |= 1u128 << self.programs[index].slot;
            }
        }
    }
    pub fn host_expression(&mut self, pattern: HostPattern, expression: HostExpression) {
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.host_expression(pattern, expression));
        }
    }
    /// Host rejection retains the exact tuple and its already recorded terminal time.
    pub fn finish_host_notes(&mut self, mut emit: impl FnMut(HostNote, u64) -> bool) {
        for index in 0..self.programs.len() {
            self.with_program(index, |e| {
                e.mark_host_notes();
                let mut row = 0;
                while let Some((note, pending)) = e.host_note_at(row) {
                    if !pending
                        && let Some(end) = e.host_note_end_at(note)
                        && emit(note, end)
                    {
                        e.retire_host_note(note);
                    } else {
                        row += 1;
                    }
                }
            });
        }
    }
    /// Host tempo overrides the retained native milli-BPM fallback.
    pub fn set_transport(
        &mut self,
        playing: bool,
        tempo: Option<f64>,
        beats: f64,
        signature: (u8, u8),
    ) -> bool {
        if tempo
            .is_some_and(|t| !t.is_finite() || t <= 0. || !(t as f32).is_finite() || t as f32 <= 0.)
        {
            return false;
        }
        let tempo = tempo.unwrap_or(self.saved_tempo);
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.set_transport(playing, tempo, beats, signature));
        }
        true
    }
    pub fn panic(&mut self) {
        self.requested_sustain = 0;
        self.received.reset();
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.panic());
        }
    }
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let _ftz = super::FlushDenormals::new();
        left.fill(0.);
        right.fill(0.);
        let frames = left.len().min(right.len());
        self.render_begin(frames);
        let mut at = 0;
        loop {
            self.render_boundary(at);
            if at == frames {
                break;
            }
            let end = self.render_next_boundary(at).min(frames);
            debug_assert!(end > at);
            self.render_interval(at, &mut left[at..end], &mut right[at..end]);
            at = end;
        }
        self.render_finish();
    }

    pub(super) fn render_begin(&mut self, frames: usize) {
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.render_begin(frames));
        }
    }
    pub(super) fn render_boundary(&mut self, at: usize) {
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.render_boundary(at));
        }
    }
    pub(super) fn render_next_boundary(&self, at: usize) -> usize {
        self.programs
            .iter()
            .map(|p| p.engine.render_next_boundary(at))
            .min()
            .unwrap_or(at)
    }
    pub(super) fn render_interval(
        &mut self,
        at: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> bool {
        left.fill(0.);
        right.fill(0.);
        let n = left.len().min(right.len());
        let window = self.programs[0].engine.player.release_counter.span();
        let fresh = self.amplifier.is_fresh();
        let mut scratch = std::mem::replace(&mut self.scratch, [[0.; MAX_BLOCK]; 2]);
        let mut fed = false;
        for index in 0..self.programs.len() {
            let [l, r] = &mut scratch;
            fed |= self.with_program(index, |e| {
                e.render_interval_with_program_fresh(at, &mut l[..n], &mut r[..n], Some(fresh))
            });
            for i in 0..n {
                left[i] += l[i];
                right[i] += r[i];
            }
        }
        self.scratch = scratch;
        self.amplifier
            .process(self.controls, window, self.rate() as f32, fed, left, right);
        self.now = self.now.saturating_add(n as u64);
        fed
    }
    pub(super) fn render_finish(&mut self) {
        for index in 0..self.programs.len() {
            self.with_program(index, Engine::render_finish);
        }
    }

    pub(crate) fn port(&self) -> u8 {
        self.port
    }
    pub(crate) fn matches_input(&self, port: u8, channel: u8) -> bool {
        self.matches(port, channel)
    }
    pub fn rate(&self) -> f64 {
        self.programs[0].engine.rate()
    }
    pub fn audible_voices(&self) -> usize {
        self.programs
            .iter()
            .map(|p| p.engine.audible_voices())
            .sum()
    }
    pub fn underruns(&self) -> u64 {
        self.programs.iter().map(|p| p.engine.underruns()).sum()
    }
    pub fn dropped_commands(&self) -> u64 {
        self.programs
            .iter()
            .map(|p| p.engine.dropped_commands())
            .sum()
    }
    pub fn pending_work(&self) -> [usize; 3] {
        self.programs.iter().fold([0; 3], |mut sum, p| {
            for (total, count) in sum.iter_mut().zip(p.engine.pending_work()) {
                *total += count;
            }
            sum
        })
    }
    pub fn take_stream_error(&self) -> Option<super::StreamError> {
        self.programs
            .iter()
            .find_map(|p| p.engine.take_stream_error())
    }
    pub(crate) fn set_blocking_streams(&mut self, offline: bool) {
        for p in &mut self.programs {
            p.engine.blocking_streams = offline;
        }
    }
    pub(crate) fn set_load(&mut self, load: f32) {
        for p in &mut self.programs {
            p.engine.load = load;
        }
    }
    pub(crate) fn set_sampler_defaults(&mut self, attack: f32, release: f32, cutoff: f32) {
        for p in &mut self.programs {
            p.engine.attack = attack;
            p.engine.release = release;
            p.engine.cutoff = cutoff;
        }
    }
    pub(crate) fn set_tune(&mut self, tune: f32) {
        for p in &mut self.programs {
            p.engine.tune = tune;
        }
    }
    pub(crate) fn initialize_frame(&mut self, now: u64) {
        self.now = now;
        for p in &mut self.programs {
            p.engine.initialize_frame(now);
        }
    }
    pub(crate) fn reset(&mut self, rate: f64) {
        self.received.reset();
        self.requested_sustain = 0;
        self.amplifier.idle();
        let channel = self.channel;
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.reset_input(rate, channel, false));
        }
    }
    pub(crate) fn audio_block_start(&self) -> u64 {
        self.block_start
    }
    pub(crate) fn mark_host_notes(&mut self) {
        for index in 0..self.programs.len() {
            self.with_program(index, Engine::mark_host_notes);
        }
    }
    pub(crate) fn host_note_at(&self, index: usize) -> Option<(HostNote, bool)> {
        self.programs
            .iter()
            .flat_map(|p| &p.engine.player.host_notes.active)
            .nth(index)
            .and_then(|id| self.roots.get(*id))
            .map(|o| (o.note, o.pinned))
    }
    pub(crate) fn host_note_present(&self, note: HostNote) -> bool {
        self.programs
            .iter()
            .flat_map(|p| &p.engine.player.host_notes.active)
            .any(|id| self.roots.get(*id).is_some_and(|o| o.note == note))
    }
    pub(crate) fn host_note_pending(&self, note: HostNote) -> bool {
        self.programs
            .iter()
            .flat_map(|p| &p.engine.player.host_notes.active)
            .any(|id| {
                self.roots
                    .get(*id)
                    .is_some_and(|o| o.note == note && o.pinned)
            })
    }
    pub(crate) fn host_note_end_at(&self, note: HostNote) -> Option<u64> {
        self.programs
            .iter()
            .flat_map(|p| &p.engine.player.host_notes.active)
            .filter_map(|id| self.roots.get(*id))
            .filter(|o| o.note == note && !o.pinned)
            .map(|o| {
                o.pending_end
                    .unwrap_or(o.start)
                    .max(o.stop.unwrap_or(o.start))
            })
            .max()
    }
    pub(crate) fn host_key_held(&self, channel: u8, key: u8) -> bool {
        self.programs
            .iter()
            .flat_map(|p| &p.engine.player.host_notes.active)
            .any(|id| {
                self.roots
                    .get(*id)
                    .is_some_and(|o| o.held && o.note.channel == channel && o.note.key == key)
            })
    }
    pub(crate) fn host_note_drops(&self) -> u64 {
        self.programs
            .iter()
            .map(|p| p.engine.host_note_drops())
            .sum()
    }
    pub(crate) fn retire_host_note(&mut self, note: HostNote) {
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.retire_host_note(note));
        }
    }
    pub(super) fn roots(&self) -> &Storage {
        &self.roots
    }
    pub(super) fn roots_mut(&mut self) -> &mut Storage {
        &mut self.roots
    }
    pub(super) fn detach_terminal_refs(
        &mut self,
        ended: &mut Vec<super::host_notes::HostRef>,
    ) -> bool {
        let count: usize = self
            .programs
            .iter()
            .map(|p| p.engine.player.host_notes.active.len())
            .sum();
        if count > ended.capacity() - ended.len() {
            return false;
        }
        if self
            .programs
            .iter()
            .flat_map(|p| &p.engine.player.host_notes.active)
            .any(|id| self.roots.terminal(*id).is_none())
        {
            return false;
        }
        for index in 0..self.programs.len() {
            self.with_program(index, |e| {
                e.player.host_notes.detach_terminal_refs(ended);
            });
        }
        true
    }
}
