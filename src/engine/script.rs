//! The engine side of the KSP boundary. On the audio thread a borrowed
//! [`Host`] turns runtime calls into [`Command`]s stamped with their frame in
//! the next render; `Engine::render` applies them there, so script timing is
//! sample-accurate without per-sample checks. [`ScriptSetup`] serves `on init`
//! off the audio thread, where nothing can play.

use super::params::{self, Address, GroupPar, MAX_WRITES, Write};
use super::{Ahdsr, Bank, EventChange, EventId, Expression, GroupMask, GroupSettings, NoteEvent, Player};
use crate::fx::{FxParam, FxProcessor, Kind as FxKind, Load, ProgramFx, Rack, ScriptIr};
use crate::import::{Group, Instrument};
use crate::ksp::{EnginePar, Fade, KspEngine, NoteLength, NoteSpec, Persisted, Runtime, VoicePar};

/// Script output buses, as Kontakt's default output section.
const SCRIPT_OUTPUTS: usize = crate::fx::OUTS;

/// Initialize an instrument's scripts off the audio thread (`on init` may take
/// a while), restoring `persisted` values. `None` when it has no scripts.
/// Failing slots stay in place, passing events through; their errors return.
pub fn load_scripts(
    instrument: &Instrument,
    persisted: Vec<Persisted>,
    rate: f64,
) -> (Option<Box<Runtime>>, Vec<String>) {
    load_scripts_with_ir(instrument, persisted, rate, &[])
}

pub fn load_scripts_with_ir(
    instrument: &Instrument,
    persisted: Vec<Persisted>,
    rate: f64,
    ir_settings: &[crate::fx::IrSlotSettings],
) -> (Option<Box<Runtime>>, Vec<String>) {
    load_scripts_with_state(instrument, persisted, rate, ir_settings, &[])
}

/// Restore supported native edits before authored initialization reads them.
pub fn load_scripts_with_state(
    instrument: &Instrument, persisted: Vec<Persisted>, rate: f64,
    ir_settings: &[crate::fx::IrSlotSettings], engine_state: &[crate::ksp::engine::NativeEdit],
) -> (Option<Box<Runtime>>, Vec<String>) {
    if instrument.scripts.is_empty() {
        return (None, Vec::new());
    }
    let mut setup = ScriptSetup::new(instrument, rate);
    // Kontakt saves engine state separately from script variables. Seed it
    // before init so authored get_engine_par calls see the restored values.
    let mut restore_errors = Vec::new();
    for saved in ir_settings {
        let (rack, slot, settings) = (saved.rack, saved.slot, saved.settings);
        if setup.fx.param(rack, slot, FxParam::Convolution(0)).is_none()
            || !settings.values.iter().chain([&settings.size]).all(|v| v.is_finite() && (0. ..=1.).contains(v)) { continue }
        if let Some(file) = &saved.file {
            match ScriptIr::load(rack, slot, file.clone()) {
                Ok(ir) => setup.loads.push(ir),
                Err(error) => {
                    let error = format!("restore impulse response {}: {error:#}", file.display());
                    crate::diagnostics::resource(&instrument.path, &file.to_string_lossy(), &error);
                    restore_errors.push(error);
                }
            }
        }
        setup.loads.push(ScriptIr { rack, slot, load: Load::Convolution(settings) });
        for n in 0..5 {
            if let Some(value) = settings.value(n) {
                setup.effects.push((Address::Fx(rack, slot, FxParam::Convolution(n)), value));
            }
        }
    }
    setup.saved = engine_state.iter().copied().filter(|edit| {
        Address::resolve(edit.par, setup.groups).is_some_and(|a|
            !matches!(a, Address::GroupType(..) | Address::Fx(_, _, FxParam::Type)))
    }).collect();
    for &crate::ksp::engine::NativeEdit { par, value } in engine_state {
        if Address::resolve(par, setup.groups).is_some_and(|a| !matches!(a, Address::GroupType(..) | Address::Fx(_, _, FxParam::Type)))
            && setup.engine_par(par).is_some() {
            setup.set_engine_par(0, par, value);
        }
    }
    let (mut rt, errors) =
        Runtime::with_scripts(&instrument.scripts, &mut setup, SCRIPT_OUTPUTS, persisted);
    // Authored init may restore defaults. Apply saved edits last, as well as
    // seeding the getter state before init. Only accepted addresses are replayed.
    let mut restored = Vec::new();
    for &crate::ksp::engine::NativeEdit { par, value } in engine_state {
        if Address::resolve(par, setup.groups).is_some_and(|a| !matches!(a, Address::GroupType(..) | Address::Fx(_, _, FxParam::Type)))
            && setup.engine_par(par).is_some()
            && setup.set_engine_par(0, par, value) { restored.push((par, value)); }
        else { restore_errors.push(format!("Saved engine parameter is unavailable: {par:?}")); }
    }
    rt.native_state = setup.prepare_native_state();
    for &(par, value) in &restored {
        if let Some(address) = setup.address(par) { rt.native_state.restored(address, par, value); }
    }
    rt.init_engine_pars = setup.pars;
    rt.init_controllers = setup.controllers;
    rt.init_irs = setup.loads;
    let errors = restore_errors.into_iter().chain(errors
        .into_iter()
        .enumerate()
        .filter_map(|(slot, e)| Some(format!("Script {}: {}", slot + 1, e?))))
        .collect();
    crate::audio::trim_heap();
    (Some(Box::new(rt)), errors)
}

/// The instrument's effects for `rate`, with the effects and impulse
/// responses `script`'s `on init` loaded. Allocates: build off the audio thread.
pub fn effects(instrument: &Instrument, script: Option<&Runtime>, rate: f32) -> FxProcessor {
    let irs = script.map_or(&[][..], |rt| &rt.init_irs);
    instrument.fx.processor_for_groups(rate, super::MAX_BLOCK, irs, &instrument.groups)
}

/// Script engine calls one render can hold; the rest are dropped and counted.
pub const MAX_COMMANDS: usize = 256;
// Pedal-up and terminal fades have their own quota, never the note-off reserve.
const MAX_STOP_COMMANDS: usize = 256;
// A first freeze per existing voice or accepted queued start; separate from releases.
const MAX_FREEZE_COMMANDS: usize = super::MAX_VOICES + MAX_COMMANDS;
// One release per live script event, beyond the ordinary and stop budgets.
pub(super) const COMMAND_CAPACITY: usize = MAX_COMMANDS + MAX_STOP_COMMANDS + MAX_FREEZE_COMMANDS + crate::ksp::EVENT_CAPACITY;

/// A worker request: copying the name never allocates on the audio thread.
#[derive(Clone)]
pub struct IrRequest {
    pub rack: Rack,
    pub slot: u8,
    pub script_slot: u8,
    pub id: i32,
    pub settings: crate::fx::params::IrSettings,
    file: [u8; 1024],
    len: usize,
}

impl IrRequest {
    pub(crate) fn rebuild(rack: Rack, slot: u8, settings: crate::fx::params::IrSettings) -> Self {
        Self { rack, slot, settings, script_slot: 0, id: -1, file: [0; 1024], len: 0 }
    }
    pub fn file(&self) -> &str {
        std::str::from_utf8(&self.file[..self.len]).unwrap()
    }
}

/// One engine call, applied at frame `at` of the next render.
#[derive(Clone, Copy)]
pub(super) struct Command {
    pub at: u32,
    pub channel: u8,
    pub input_channel: Option<u8>,
    /// Target event; unused by controllers.
    pub id: EventId,
    pub kind: Kind,
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Start {
        host_note: Option<super::HostRef>,
        note: u8,
        velocity: u8,
        owner: Option<(u8, u8)>,
        counter_stop: Option<u64>,
        offset_us: u64,
        volume: f32,
        tune: f64,
        pan: f32,
        /// Whole-sample notes (`play_note` duration 0) also fire release
        /// triggers at once, which is how scripts play release samples.
        whole: bool,
        groups: GroupMask,
        expression: Option<Expression>,
    },
    /// Key release; `trigger` carries note, velocity and groups for release
    /// triggers unless the note already fired them.
    Release {
        host_note: Option<super::HostRef>,
        trigger: Option<(u8, u8)>,
        groups: GroupMask,
        expression: Option<Expression>,
    },
    Freeze(Expression),
    Fade(Fade),
    Change(EventChange),
    Controller {
        cc: u8,
        value: i32,
    },
    /// `reset_rls_trig_counter(note)`.
    ResetCounter(u8),
}

impl Kind {
    fn is_stop(self) -> bool {
        matches!(self, Self::Fade(Fade::Out { stop: true, .. })
            | Self::Controller { cc: 120 | 121 | 123, .. })
            || matches!(self, Self::Controller { cc: 64 | 66, value } if value < 64)
    }
}

/// The engine as the runtime sees it during one call, borrowed from `Engine`.
pub(super) struct Host<'a> {
    pub channel: u8,
    pub bank: Option<&'a Bank>,
    pub fx: &'a FxProcessor,
    pub player: &'a mut Player,
    pub commands: &'a mut Vec<Command>,
    pub writes: &'a mut Vec<Write>,
    pub write_index: &'a mut (u32, std::collections::HashMap<Address, usize>),
    pub ir_requests: &'a mut Vec<IrRequest>,
}

impl Host<'_> {
    /// Engine address of a parameter the installed bank and effects hold.
    fn address(&self, par: EnginePar) -> Option<Address> {
        let address = Address::resolve(par, self.bank?.groups())?;
        match address {
            Address::Fx(rack, slot, param) => self.fx.param(rack, slot, param).map(|_| address),
            _ => Some(address),
        }
    }

    /// Queue in frame order (stable for equal frames) within the preallocated capacity.
    fn push(&mut self, at: u32, channel: u8, id: EventId, kind: Kind) -> bool {
        self.push_from(at, channel, None, id, kind)
    }

    fn push_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, id: EventId, kind: Kind) -> bool {
        let stop = kind.is_stop();
        if stop {
            // Coalesce identical same-frame stops, without crossing a later
            // fade or opposite pedal state for the same target.
            let prior = self.commands.iter().rev().find(|c| c.at == at && c.channel == channel && c.input_channel == input_channel && match (c.kind, kind) {
                (Kind::Fade(_), Kind::Fade(_)) => c.id == id,
                (Kind::Controller { cc: a, .. }, Kind::Controller { cc: b, .. }) => a == b,
                _ => false,
            });
            if prior.is_some_and(|c| match (c.kind, kind) {
                (Kind::Fade(Fade::Out { duration_us: a, stop: true }), Kind::Fade(Fade::Out { duration_us: b, stop: true })) => a == b,
                (Kind::Controller { value: a, .. }, Kind::Controller { value: b, .. }) => a == b,
                _ => false,
            }) { return true }
        }
        // ponytail: bounded quota scans over at most COMMAND_CAPACITY entries;
        // cache counts if dense MIDI profiles make this a hot path.
        let full = self.commands.len() == COMMAND_CAPACITY
            || if stop {
                self.commands.iter().filter(|c| c.kind.is_stop()).count() >= MAX_STOP_COMMANDS
            } else if matches!(kind, Kind::Freeze(_)) {
                self.commands.iter().filter(|c| matches!(c.kind, Kind::Freeze(_))).count() >= MAX_FREEZE_COMMANDS
            } else {
                !matches!(kind, Kind::Release { .. }) && self.commands.iter()
                    .filter(|c| !c.kind.is_stop() && !matches!(c.kind, Kind::Release { .. } | Kind::Freeze(_)))
                    .count() >= MAX_COMMANDS
            };
        if full {
            self.player.dropped_commands += 1;
            if stop && channel < 16 { self.player.stop_overflow |= 1 << channel; }
            return false;
        }
        let i = self.commands.partition_point(|c| c.at <= at);
        self.commands.insert(i, Command { at, channel, input_channel, id, kind });
        true
    }
}

impl KspEngine for Host<'_> {
    fn request_ir_sample(&mut self, file: &str, slot: i32, generic: i32, script_slot: u8, id: i32) -> Option<bool> {
        let Some((rack, slot)) = params::rack(generic).zip(u8::try_from(slot).ok())
            .filter(|&(r, s)| self.fx.param(r, s, FxParam::Type) == Some(f32::from(crate::fx::Kind::Convolution.ser_id())))
        else { return Some(false) };
        if file.len() > 1024 || self.ir_requests.len() == self.ir_requests.capacity() {
            return Some(false);
        }
        let settings = self.fx.ir_settings(rack, slot).unwrap_or(crate::fx::params::IrSettings::DEFAULT);
        let mut request = IrRequest { rack, slot, script_slot, id, settings, file: [0; 1024], len: file.len() };
        request.file[..file.len()].copy_from_slice(file.as_bytes());
        self.ir_requests.push(request);
        Some(true)
    }
    fn play_note(&mut self, at: u32, n: &NoteSpec<'_>) -> Option<EventId> {
        self.bank?;
        let id = self.player.next_id();
        let kind = Kind::Start {
            host_note: n.host_note,
            note: n.note,
            velocity: n.velocity,
            owner: n.owner,
            counter_stop: None,
            offset_us: n.sample_offset_us.max(0) as u64,
            volume: 10f32.powf(n.volume_mdb as f32 / 20_000.0),
            tune: f64::from(n.tune_mc) / 100_000.0,
            pan: n.pan.clamp(-1000, 1000) as f32 / 1000.0,
            whole: n.length == NoteLength::Sample,
            groups: *n.groups,
            expression: n.frozen_expression,
        };
        self.push_from(at, n.channel, n.input_channel, id, kind).then_some(id)
    }

    fn release_expression(&self, at: u32, voice: Option<EventId>, channel: u8, note: u8) -> Option<Expression> {
        if !self.player.mpe_zone.is_some_and(|(_, members)| channel < 16 && members & (1 << channel) != 0) { return None; }
        voice.and_then(|id| {
            self.commands.iter().find_map(|c| match c.kind {
                Kind::Start { expression, .. } if c.id == id => expression,
                _ => None,
            }).or_else(|| self.player.voices.iter().find(|v| v.event == id).and_then(|v| v.frozen_expression))
        }).or_else(|| Some(self.player.release_snapshot(self.commands, at, channel, note)))
    }

    fn note_off(&mut self, at: u32, voice: EventId, n: &NoteSpec<'_>) {
        let kind = Kind::Release {
            host_note: n.host_note,
            trigger: (n.length != NoteLength::Sample).then_some((n.note, n.velocity)),
            groups: *n.groups,
            expression: n.frozen_expression.or_else(|| self.commands.iter().find_map(|c| match c.kind {
                Kind::Start { expression, .. } if c.id == voice => expression,
                _ => None,
            })).or_else(|| self.player.release_expression(voice, n.channel, n.note)),
        };
        self.push_from(at, n.channel, n.input_channel, voice, kind);
    }

    fn freeze_expression(&mut self, at: u32, voice: EventId, expression: Expression) {
        self.freeze_expression_from(at, self.channel, None, voice, expression);
    }

    fn freeze_expression_from(&mut self, at: u32, _channel: u8, input_channel: Option<u8>, voice: EventId, expression: Expression) {
        // First freeze wins until a new Start for this event. Ignore events
        // without an accepted engine source; they cannot consume this quota.
        for c in self.commands.iter().rev().filter(|c| c.id == voice) {
            match c.kind {
                Kind::Freeze(_) => return,
                Kind::Start { .. } => break,
                _ => {}
            }
        }
        let active = self.player.voices.iter().find(|v| v.event == voice);
        if active.is_some_and(|v| v.frozen_expression.is_some()) { return; }
        let channel = active.map(|v| v.channel);
        if let Some(c) = self.commands.iter_mut().find(|c| c.id == voice && matches!(c.kind, Kind::Start { .. })) {
            if c.at >= at && let Kind::Start { expression: x, .. } = &mut c.kind {
                x.get_or_insert(expression);
                return;
            }
            let channel = c.channel;
            self.push_from(at, channel, input_channel, voice, Kind::Freeze(expression));
        } else if let Some(channel) = channel {
            self.push_from(at, channel, input_channel, voice, Kind::Freeze(expression));
        }
    }

    fn fade(&mut self, at: u32, voice: EventId, fade: Fade) {
        self.push(at, self.channel, voice, Kind::Fade(fade));
    }

    fn fade_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, voice: EventId, fade: Fade) {
        self.push_from(at, channel, input_channel, voice, Kind::Fade(fade));
    }

    fn reset_release_counter(&mut self, at: u32, note: u8) {
        self.reset_release_counter_on_channel(at, self.channel, note);
    }

    fn reset_release_counter_on_channel(&mut self, at: u32, channel: u8, note: u8) {
        self.push(at, channel, EventId::default(), Kind::ResetCounter(note));
    }

    fn reset_release_counter_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, note: u8) {
        self.push_from(at, channel, input_channel, EventId::default(), Kind::ResetCounter(note));
    }

    fn set_par(&mut self, at: u32, voice: EventId, par: VoicePar, value: i32) {
        self.set_par_from(at, self.channel, None, voice, par, value);
    }

    fn set_par_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, voice: EventId, par: VoicePar, value: i32) {
        let change = match par {
            VoicePar::VolumeMdb => EventChange::Volume(10f32.powf(value as f32 / 20_000.0)),
            VoicePar::TuneMc => EventChange::Tune(f64::from(value) / 100_000.0),
            VoicePar::Pan => EventChange::Pan(value.clamp(-1000, 1000) as f32 / 1000.0),
        };
        self.push_from(at, channel, input_channel, voice, Kind::Change(change));
    }

    fn controller(&mut self, at: u32, cc: u8, value: i32) {
        self.controller_on_channel(at, self.channel, cc, value);
    }

    fn controller_on_channel(&mut self, at: u32, channel: u8, cc: u8, value: i32) {
        self.push(at, channel, EventId::default(), Kind::Controller { cc, value });
    }

    fn controller_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, cc: u8, value: i32) {
        self.push_from(at, channel, input_channel, EventId::default(), Kind::Controller { cc, value });
    }

    fn group_count(&self) -> usize {
        self.bank.map_or(0, |b| b.groups().len())
    }

    fn zone_count(&self) -> usize { self.bank.map_or(0, Bank::zone_count) }

    fn group_name(&self, group: usize) -> &str {
        self.bank
            .and_then(|b| b.groups().get(group))
            .map_or("", |g| &g.name)
    }

    fn sample_rate(&self) -> f64 {
        self.player.rate
    }

    /// Modelled parameters are queued for their frame, like notes.
    fn set_engine_par(&mut self, at: u32, par: EnginePar, value: i32) -> bool {
        let Some(address) = self.address(par) else {
            return self.bank.is_some_and(|b| Address::inert(par, b.groups()));
        };
        let native = value;
        let value = address.decode(value);
        if let Address::GroupType(g, s) = address {
            return self.bank.and_then(|b| params::group_type(b.groups(), g, s)) == Some(value);
        }
        if !loaded(address, value, |r, s| self.fx.param(r, s, FxParam::Type)) {
            return false;
        }
        // Render applies every parameter before notes at the same sample. Keep
        // its last value, including what later callbacks read, without queuing
        // a full group-envelope restore again for each articulation switch.
        if self.write_index.0 != at {
            self.write_index.0 = at;
            self.write_index.1.clear();
            self.write_index.1.extend(self.writes.iter().enumerate()
                .filter(|(_, w)| w.at == at).map(|(i, w)| (w.address, i)));
        }
        if let Some(&i) = self.write_index.1.get(&address) {
            self.writes[i].value = value;
            self.writes[i].par = par;
            self.writes[i].native = native;
            return true;
        }
        if self.writes.len() == MAX_WRITES {
            self.player.dropped_commands += 1;
            return true;
        }
        let i = self.writes.partition_point(|w| w.at <= at);
        // Insertion follows every existing write at this sample, so it cannot
        // move an index cached for the current batch, even with future writes.
        self.writes.insert(i, Write { at, address, value, par, native });
        self.write_index.1.insert(address, i);
        true
    }

    /// The latest queued value, else the engine's current one.
    fn engine_par(&self, par: EnginePar) -> Option<i32> {
        let address = self.address(par)?;
        let queued = self.writes.iter().rev().find(|w| w.address == address);
        let value = match queued {
            Some(w) => w.value,
            None => match address {
                Address::Fx(rack, slot, param) => self.fx.param(rack, slot, param)?,
                Address::Instrument(p) => instrument(self.player.instrument, p)?,
                Address::GroupType(g, s) => params::group_type(self.bank?.groups(), g, s)?,
                _ => params::read(&self.bank?.base, address)?,
            },
        };
        Some(address.encode(value))
    }

    fn find_mod(&self, group: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        params::find_mod(self.bank?.groups(), group, is)
    }

    fn find_target(&self, group: usize, modulator: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        params::find_target(self.bank?.groups(), group, modulator, is)
    }

    fn voice_zone(&self, voice: EventId) -> Option<i32> {
        if let Some(id) = self.player.voices.iter().filter(|v| v.event == voice).map(|v| v.zone_id).max() {
            return Some(id as i32);
        }
        // Scripts advance before command rendering. Query the same mapping and
        // bounded selection as Player::start rather than inventing a group ID.
        let command = self.commands.iter().find(|c| c.id == voice && matches!(c.kind, Kind::Start { .. }))?;
        let Kind::Start { note, velocity, whole, groups, .. } = command.kind else { return None };
        let bank = self.bank?;
        let normal = bank.matching_zones(command.channel, note, velocity, false, &groups).take(super::MAX_VOICES);
        let releases = bank.matching_zones(command.channel, note, velocity, true, &groups)
            .take(if whole || !self.player.native_release_triggers { super::MAX_VOICES } else { 0 });
        Some(normal.chain(releases).map(|z| bank.plays[z as usize].zone_id as i32).max().unwrap_or(-1))
    }

    fn voice_active(&self, voice: EventId) -> bool {
        self.player.voices.iter().any(|v| v.event == voice)
            || self
                .commands
                .iter()
                .any(|c| c.id == voice && matches!(c.kind, Kind::Start { .. }))
    }
}

impl Player {
    /// Apply one script command; its notes play on MIDI `channel`.
    pub(super) fn apply(&mut self, bank: &Bank, c: &Command, defaults: Ahdsr) {
        let id = c.id;
        let channel = c.channel;
        match &c.kind {
            &Kind::Start {
                host_note,
                note,
                velocity,
                owner,
                counter_stop,
                offset_us,
                volume,
                tune,
                pan,
                whole,
                expression,
                ref groups,
            } => {
                let event = NoteEvent {
                    host_note,
                    channel,
                    note,
                    velocity,
                    owner,
                    input_channel: c.input_channel,
                    counter_stop,
                    release_held_ms: None,
                    groups: Some(groups),
                    offset_us,
                    volume,
                    tune,
                    pan,
                    frozen_expression: expression,
                };
                self.start(bank, &event, id, false, defaults);
                // With the system release script bypassed, the script owns
                // when its selected release groups start, for every duration.
                if whole || !self.native_release_triggers {
                    self.start(bank, &event, id, true, defaults);
                }
            }
            Kind::Release { trigger, groups, expression, host_note } => {
                let latched = self.release_voices(bank, id).is_some_and(|event| event.4);
                if let &Some((note, velocity)) = trigger {
                    self.trigger_release(bank, id, (channel, note, velocity), groups, latched, *expression, c.input_channel, *host_note, defaults);
                }
            }
            &Kind::Freeze(expression) => {
                for v in self.voices.iter_mut().filter(|v| v.event == id && v.frozen_expression.is_none()) {
                    v.frozen_expression = Some(expression);
                    v.settled = None;
                }
            }
            &Kind::Fade(Fade::In { duration_us }) => {
                let frames = self.frames(duration_us as f32 * 1e-6);
                for v in self.voices.iter_mut().filter(|v| v.event == id) {
                    v.fade.fade_in(frames);
                }
            }
            &Kind::Fade(Fade::Out { duration_us, stop }) => {
                let frames = self.frames(duration_us as f32 * 1e-6);
                for v in self.voices.iter_mut().filter(|v| v.event == id) {
                    v.fade.start(0.0, frames, stop);
                }
            }
            &Kind::Change(change) => self.change_event(id, change),
            &Kind::Controller { cc: 128, value } => {
                self.bend[channel as usize] = value.clamp(-8192, 8191) as f32 / 8192.0;
                self.touch();
            }
            &Kind::Controller { cc, value } if cc < 128 => {
                let value = value.clamp(0, 127) as u8;
                self.cc(Some(bank), channel, cc, value, defaults);
            }
            &Kind::Controller { cc: 129, value } => {
                self.pressure[channel as usize] = value.clamp(0, 127) as u8;
                self.touch();
            }
            // Other virtual controllers: nothing in the engine uses them.
            Kind::Controller { .. } => {}
            &Kind::ResetCounter(note) => {
                self.key_on[channel as usize & 15][note as usize & 127] = self.now;
                for v in self.voices.iter_mut().filter(|v| v.channel == channel && v.note == note && !v.release_trigger && v.counter_stop.is_none()) {
                    v.counter_start = self.now;
                }
            }
        }
    }
}

/// False for `$ENGINE_PAR_EFFECT_TYPE` naming another effect than the one
/// loaded (`kind` of a rack slot): that would allocate on the audio thread.
/// Naming the loaded one, as framework scripts do, keeps it.
fn loaded(address: Address, value: f32, kind: impl Fn(Rack, u8) -> Option<f32>) -> bool {
    match address {
        Address::Fx(rack, slot, FxParam::Type) => kind(rack, slot) == Some(value),
        _ => true,
    }
}

/// Instrument volume, pan or tune from `(volume, pan, tune)`.
fn instrument((volume, pan, tune): (f32, f32, f32), p: GroupPar) -> Option<f32> {
    match p {
        GroupPar::Volume => Some(volume),
        GroupPar::Pan => Some(pan),
        GroupPar::Tune => Some(tune),
        GroupPar::Output => None,
    }
}

/// Engine view for `on init` off the audio thread: group names, modulators,
/// the sample rate and the parameters the engine models, which it records
/// for the playing engine ([`Runtime::init_engine_pars`]). Nothing plays
/// during init.
pub struct ScriptSetup<'a> {
    groups: &'a [Group],
    zones: usize,
    /// The instrument's effects with the ones scripts loaded.
    fx: std::borrow::Cow<'a, ProgramFx>,
    path: &'a std::path::Path,
    rate: f64,
    settings: Vec<GroupSettings>,
    instrument: (f32, f32, f32),
    /// Effect values written so far, by address.
    effects: Vec<(Address, f32)>,
    pars: Vec<(EnginePar, i32)>,
    saved: Vec<crate::ksp::engine::NativeEdit>,
    controllers: Vec<(u8, u8)>,
    /// Effects and impulse responses loaded, in order.
    loads: Vec<ScriptIr>,
}

impl<'a> ScriptSetup<'a> {
    pub fn new(instrument: &'a Instrument, rate: f64) -> Self {
        Self {
            groups: &instrument.groups,
            zones: instrument.zones.len(),
            fx: std::borrow::Cow::Borrowed(&instrument.fx),
            path: &instrument.path,
            rate,
            settings: instrument.groups.iter().map(GroupSettings::from).collect(),
            instrument: (1.0, 0.0, 0.0),
            effects: Vec::new(),
            pars: Vec::new(),
            saved: Vec::new(),
            controllers: Vec::new(),
            loads: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn audit_native_preparation(instrument: &Instrument, rt: &Runtime) -> (usize, std::time::Duration) {
        let mut setup = ScriptSetup::new(instrument, 48000.);
        for &(par, value) in &rt.init_engine_pars { setup.set_engine_par(0, par, value); }
        let start = std::time::Instant::now();
        let state = setup.prepare_native_state();
        (state.capacity().0, start.elapsed())
    }

    fn prepare_native_state(&self) -> super::native_state::NativeState {
        let mut state = super::native_state::NativeState::default();
        let mut add = |par: EnginePar| {
            let Some(address) = self.address(par) else { return };
            if matches!(address, Address::GroupType(..) | Address::Fx(_, _, FxParam::Type)) { return }
            if let Some(value) = self.engine_par(par) { state.prepare(address, par, value); }
        };
        let ids: Vec<_> = (params::id::VOLUME..).map_while(|id| crate::ksp::engine_par_name(id).map(|_| id)).collect();
        for (g, group) in self.groups.iter().enumerate() {
            // Group scalars, populated inserts and declared modulators only.
            for slot in std::iter::once(-1).chain(group.fx.slots.iter().map(|e| e.slot as i32))
                .chain((0..group.modulators.len()).map(|m| m as i32)) {
                for &id in &ids { add(EnginePar { id, group: g as i32, slot, generic: -1 }); }
            }
            for (slot, m) in group.modulators.iter().enumerate() {
                for target in 0..m.targets.len() {
                    for id in [params::id::MOD_TARGET_INTENSITY, params::id::MOD_TARGET_MP_INTENSITY, params::id::INTMOD_INTENSITY] {
                        add(EnginePar { id, group: g as i32, slot: slot as i32, generic: target as i32 });
                    }
                }
            }
        }
        // Existing program racks: send, insert, post and sixteen output buses.
        // Getter validation retains only actually populated, supported addresses.
        for generic in [0, 1, 2].into_iter().chain(1000..1016) {
            for slot in -1..8 {
                for &id in &ids { add(EnginePar { id, group: -1, slot, generic }); }
            }
        }
        state
    }

    fn address(&self, par: EnginePar) -> Option<Address> {
        let address = Address::resolve(par, self.groups)?;
        match address {
            Address::Fx(rack, slot, param) => self.fx.param(rack, slot, param).map(|_| address),
            _ => Some(address),
        }
    }

    /// `$ENGINE_PAR_EFFECT_TYPE` naming another effect: load it here, off
    /// the audio thread; the effects build with it. False for a value
    /// that names no effect.
    fn load_kind(&mut self, rack: Rack, slot: u8, value: f32) -> bool {
        let kind = match value as u16 {
            0 => None,
            id => Some(FxKind::from_ser_id(id)).filter(|k| !matches!(k, FxKind::Unknown(_))),
        };
        if (value != 0.0 && kind.is_none()) || !self.fx.to_mut().load_kind(rack, slot, kind) {
            return false;
        }
        // The old effect's values and impulse response went with it.
        self.effects.retain(|(a, _)| {
            !matches!(a, Address::Fx(r, s, FxParam::Reverb(_) | FxParam::Convolution(_) | FxParam::SendLevel(_)) if (*r, *s) == (rack, slot))
        });
        self.loads.retain(|l| (l.rack, l.slot) != (rack, slot));
        self.loads.push(ScriptIr { rack, slot, load: Load::Kind(kind) });
        // Some scripts create their rack in init. Seed its saved controls as
        // soon as the type exists, before subsequent authored getter calls.
        let saved: Vec<_> = self.saved.iter().copied().filter(|edit|
            matches!(Address::resolve(edit.par, self.groups), Some(Address::Fx(r, s, _)) if (r, s) == (rack, slot))).collect();
        for edit in saved {
            if self.engine_par(edit.par).is_some() { self.set_engine_par(0, edit.par, edit.value); }
        }
        true
    }
}

impl KspEngine for ScriptSetup<'_> {
    fn play_note(&mut self, _at: u32, _note: &NoteSpec<'_>) -> Option<EventId> {
        None
    }

    fn note_off(&mut self, _at: u32, _voice: EventId, _event: &NoteSpec<'_>) {}

    fn fade(&mut self, _at: u32, _voice: EventId, _fade: Fade) {}

    fn set_par(&mut self, _at: u32, _voice: EventId, _par: VoicePar, _value: i32) {}

    /// Recorded for the playing engine: Areia, for one, sets its expression
    /// controllers while loading, and its CC volume modulation is silent
    /// without them.
    fn controller(&mut self, _at: u32, cc: u8, value: i32) {
        if cc < 128 {
            self.controllers.push((cc, value.clamp(0, 127) as u8));
        }
    }

    fn group_count(&self) -> usize {
        self.groups.len()
    }

    fn zone_count(&self) -> usize { self.zones }

    fn group_name(&self, group: usize) -> &str {
        self.groups.get(group).map_or("", |g| &g.name)
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn set_engine_par(&mut self, _at: u32, par: EnginePar, value: i32) -> bool {
        let Some(address) = self.address(par) else {
            return Address::inert(par, self.groups);
        };
        let v = address.decode(value);
        if let Address::Fx(rack, slot, FxParam::Convolution(n)) = address {
            let mut settings = self.loads.iter().rev().find_map(|l| match l.load {
                Load::Convolution(s) if (l.rack, l.slot) == (rack, slot) => Some(s), _ => None,
            }).unwrap_or_else(|| {
                let mut settings = crate::fx::params::IrSettings::DEFAULT;
                for n in 0..5 {
                    if let Some(value) = self.fx.param(rack, slot, FxParam::Convolution(n)) { settings.set(n, value); }
                }
                settings
            });
            if !settings.set(n, v) { return false }
            self.loads.retain(|l| (l.rack, l.slot) != (rack, slot) || !matches!(l.load, Load::Convolution(_)));
            self.loads.push(ScriptIr { rack, slot, load: Load::Convolution(settings) });
        }
        match address {
            Address::GroupType(g, s) => return params::group_type(self.groups, g, s) == Some(v),
            Address::Fx(rack, slot, FxParam::Type) if self.fx.param(rack, slot, FxParam::Type) != Some(v) => {
                if !self.load_kind(rack, slot, v) {
                    return false;
                }
            }
            _ => {}
        }
        match address {
            Address::Fx(..) => match self.effects.iter_mut().find(|e| e.0 == address) {
                Some(e) => e.1 = v,
                None => self.effects.push((address, v)),
            },
            Address::Instrument(p) => {
                let (volume, pan, tune) = &mut self.instrument;
                match p {
                    GroupPar::Volume => *volume = v,
                    GroupPar::Pan => *pan = v,
                    _ => *tune = v,
                }
            }
            _ => {
                if !params::write(&mut self.settings, address, v) { return false }
            }
        }
        self.pars.push((par, value));
        true
    }

    fn engine_par(&self, par: EnginePar) -> Option<i32> {
        let address = self.address(par)?;
        let value = match address {
            Address::Fx(rack, slot, param) => match self.effects.iter().find(|e| e.0 == address) {
                Some(e) => e.1,
                None => self.fx.param(rack, slot, param)?,
            },
            Address::Instrument(p) => instrument(self.instrument, p)?,
            Address::GroupType(g, s) => params::group_type(self.groups, g, s)?,
            _ => params::read(&self.settings, address)?,
        };
        Some(address.encode(value))
    }

    fn find_mod(&self, group: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        params::find_mod(self.groups, group, is)
    }

    fn find_target(&self, group: usize, modulator: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        params::find_target(self.groups, group, modulator, is)
    }

    fn instrument_path(&self) -> Option<&std::path::Path> {
        Some(self.path)
    }

    /// Decoded here, off the audio thread; the effects build with it.
    /// Only a convolution slot takes an impulse response.
    fn load_ir_sample(&mut self, file: &str, slot: i32, generic: i32) -> Option<bool> {
        let convolution = f32::from(crate::fx::Kind::Convolution.ser_id());
        let Some((rack, slot)) = params::rack(generic)
            .zip(u8::try_from(slot).ok())
            .filter(|&(r, s)| self.fx.param(r, s, FxParam::Type) == Some(convolution))
        else {
            return Some(false);
        };
        let Some(ir) = crate::resources::ir_sample(self.path, file)
            .and_then(|path| ScriptIr::load(rack, slot, path).ok())
        else {
            return Some(false);
        };
        self.loads.retain(|l| (l.rack, l.slot) != (rack, slot) || !matches!(l.load, Load::Ir { .. }));
        self.loads.push(ir);
        Some(true)
    }
}
