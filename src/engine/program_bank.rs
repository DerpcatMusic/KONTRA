//! Script-free retained instrument-bank playback. Plugin admission remains separate.
use super::{Engine, HostExpression, HostNote, HostPattern, MAX_BLOCK, MAX_VOICES, Velocity};
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
    fn prepare(
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
        let ceiling = imported
            .programs
            .len()
            .checked_mul(MAX_VOICES)
            .context("Instrument-bank capacity overflow")?;
        ensure!(
            u64::from(budget.max_voices) >= ceiling as u64,
            "Instrument-bank shared voice arbitration is unsupported at this limit"
        );
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
            ensure!(
                lookup[slot as usize].is_none(),
                "Duplicate instrument-bank program slot"
            );
            // Native bank admission uses the container rule, not the old program rule.
            instrument.voice_limit = None;
            ensure!(
                instrument.program.gain == 1. && instrument.program.pan == 0.,
                "Nonunit instrument-bank inner amplifier is unsupported"
            );
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
            let mut bank = super::Bank::load(&instrument)?;
            bank.set_polyphony(MAX_VOICES);
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
    pub fn unsupported_inputs(&self) -> u64 {
        self.unsupported
    }
    pub fn active_voices(&self) -> usize {
        self.programs.iter().map(|p| p.engine.active_voices()).sum()
    }
    pub fn begin_audio_block(&mut self, frames: usize, offline: bool) {
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
                let v = self.received.release_velocity(bytes[2])
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
                let v = self.received.velocity(bytes[2])
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
                    .is_some_and(|p| !p.registered || !matches!(p.address, 0 | 1 | 2));
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
        for index in 0..self.programs.len() {
            self.with_program(index, |e| e.render_begin(frames));
        }
        let mut at = 0;
        loop {
            for index in 0..self.programs.len() {
                self.with_program(index, |e| e.render_boundary(at));
            }
            if at == frames {
                break;
            }
            let end = self
                .programs
                .iter()
                .fold(frames, |end, p| end.min(p.engine.render_next_boundary(at)));
            let n = end - at;
            let window = self.programs[0].engine.player.release_counter.span();
            let mut scratch = std::mem::replace(&mut self.scratch, [[0.; MAX_BLOCK]; 2]);
            let mut fed = false;
            for index in 0..self.programs.len() {
                let [l, r] = &mut scratch;
                fed |=
                    self.with_program(index, |e| e.render_interval(at, &mut l[..n], &mut r[..n]));
                for i in 0..n {
                    left[at + i] += l[i];
                    right[at + i] += r[i];
                }
            }
            self.scratch = scratch;
            self.amplifier.process(
                self.controls,
                window,
                self.programs[0].engine.rate() as f32,
                fed,
                &mut left[at..end],
                &mut right[at..end],
            );
            self.now = self.now.saturating_add(n as u64);
            at = end;
        }
        for index in 0..self.programs.len() {
            self.with_program(index, Engine::render_finish);
        }
    }
}
