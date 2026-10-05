//! The engine side of the KSP boundary. On the audio thread a borrowed
//! [`Host`] turns runtime calls into [`Command`]s stamped with their frame in
//! the next render; `Engine::render` applies them there, so script timing is
//! sample-accurate without per-sample checks. [`ScriptSetup`] serves `on init`
//! off the audio thread, where nothing can play.

use super::commands::{Commands, Handle, Note, Release, Start};
use super::instrument::{InstrumentControls, InstrumentState};
use super::params::{self, Address, MAX_WRITES, Write};
use super::release::{Capture, ReleaseRequest, ReleaseSample, ReleaseSnapshot};
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
    load_scripts_with_delay_state(instrument, persisted, rate, ir_settings, engine_state, &[])
}

/// Physical Delay caches accompany final-address edits in new host states.
/// The existing entry point keeps legacy/empty-state behavior unchanged.
pub fn load_scripts_with_delay_state(
    instrument: &Instrument, persisted: Vec<Persisted>, rate: f64,
    ir_settings: &[crate::fx::IrSlotSettings], engine_state: &[crate::ksp::engine::NativeEdit],
    delay_state: &[crate::fx::DelayState],
) -> (Option<Box<Runtime>>, Vec<String>) {
    if instrument.scripts.is_empty() {
        return (None, Vec::new());
    }
    let mut setup = ScriptSetup::new(instrument, rate);
    // Kontakt saves engine state separately from script variables. Seed it
    // before init so authored get_engine_par calls see the restored values.
    let mut restore_errors = Vec::new();
    setup.saved_delays = delay_state.iter().copied().filter(|saved| {
        let mut fields = [0.0; crate::fx::blocks::FIELDS]; saved.apply(&mut fields)
    }).collect();
    for saved in &setup.saved_delays { setup.fx.to_mut().restore_delay(saved); }
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
    let saved_delays = std::mem::take(&mut setup.saved_delays);
    let mut restored_delays = Vec::new();
    for saved in &saved_delays {
        if setup.fx.to_mut().restore_delay(saved) { restored_delays.push(*saved); }
        else { restore_errors.push(format!("Saved Delay state is unavailable: {:?} slot {}", saved.rack, saved.slot)); }
    }
    if saved_delays.len() != delay_state.len() {
        restore_errors.push("Invalid saved Delay state rejected".into());
    }
    rt.native_state = setup.prepare_native_state();
    for saved in restored_delays { rt.native_state.restored_delay(saved); }
    for &(par, value) in &restored {
        if let Some(address) = setup.address(par) { rt.native_state.restored(address, par, value); }
    }
    rt.init_engine_pars = setup.pars;
    rt.init_zone_edits = setup.zone_edits;
    rt.init_zone_source = instrument.zones.as_ptr() as usize;
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
pub use super::commands::MAX_COMMANDS;

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
    /// Consumed commands stay in the bounded render queue for chronology, but
    /// no longer predict pending notes or controls already present in Player.
    pub applied: bool,
}

impl Command {
    pub(super) fn standard_control(&self, param: params::GroupPar) -> Option<(crate::midi::ControlTarget, crate::midi::ControlValue)> {
        if self.channel >= 16 { return None; }
        let cc = match param { params::GroupPar::Volume => 7, params::GroupPar::Pan => 10, _ => return None };
        match self.kind {
            Kind::Control { target, control: control @ (crate::midi::Control::Cc(number, value)
                | crate::midi::Control::Cc14 { coarse: number, value, .. }), native_standard_handled: false, .. }
                if number == cc && control.valid() && !matches!(target, crate::midi::ControlTarget::Physical(channel) if channel >= 16) => Some((target, value)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    /// Program-container history precedes this event's callbacks and start.
    ObserveNote(u8),
    Start(Handle),
    Release(Handle),
    HostPressure(super::HostRef, crate::midi::ControlValue),
    PerNotePressure(super::PerNoteInput, crate::midi::ControlValue),
    NoteController(super::PerNoteInput, crate::midi::NoteControl),
    PolyPressure { note: u8, value: crate::midi::ControlValue },
    Freeze(Expression),
    Fade(Fade),
    Change(EventChange),
    Control { target: crate::midi::ControlTarget, control: crate::midi::Control, native_standard_handled: bool,
        /// Physical exact roots already received this CC66/121 at acceptance.
        root_pedal_handled: bool },
    /// `reset_rls_trig_counter(note)`.
    ResetCounter(u8),
}

impl Kind {
    pub(super) fn is_stop(self) -> bool {
        matches!(self, Self::Fade(Fade::Out { stop: true, .. })
            | Self::Control { control: crate::midi::Control::Cc(120 | 121 | 123..=127, _), .. })
            || matches!(self, Self::Control { control: crate::midi::Control::Cc(64 | 66, value), .. } if value.normalized() < 0.5)
    }
}

/// The engine as the runtime sees it during one call, borrowed from `Engine`.
pub(super) struct Host<'a> {
    pub peer_voices: Option<usize>,
    /// Stable origin of this Runtime call's relative command offsets.
    pub frame_base: u64,
    pub channel: u8,
    pub bank: Option<&'a Bank>,
    pub state: &'a InstrumentState,
    pub fx: &'a FxProcessor,
    pub player: &'a mut Player,
    pub commands: &'a mut Commands,
    pub writes: &'a mut Vec<Write>,
    pub write_index: &'a mut (u32, std::collections::HashMap<Address, usize>),
    pub ir_requests: &'a mut Vec<IrRequest>,
    pub zone_inflight: &'a mut usize,
    pub zone_requests: &'a mut std::collections::VecDeque<crate::ksp::engine::ZoneEdit>,
}

impl Host<'_> {
    // Both getters describe the highest active zone. Duplicate-zone voices
    // keep their first stored voice; NI does not specify that tie-break.
    fn event_voice(&self, event: EventId) -> Option<&super::voice::Voice> {
        self.player.voices.iter().filter(|v| v.event == event)
            .reduce(|first, next| if next.zone_id > first.zone_id { next } else { first })
    }

    /// Engine address of a parameter the installed bank and effects hold.
    fn address(&self, par: EnginePar) -> Option<Address> {
        let address = Address::resolve(par, self.bank?.groups())?;
        match address {
            Address::PrimaryBypass(..) => params::read(&self.state.settings, address).map(|_| address),
            // A saved beat multiplier can predate our named KSP unit set.
            // Permit changing its unit without inventing a numeric readback.
            Address::Fx(rack, slot, FxParam::Field(FxKind::Delay, 4))
                if self.fx.delay_fields(rack, slot).is_some() => Some(address),
            Address::Fx(rack, slot, param) => self.fx.param(rack, slot, param).map(|_| address),
            _ => Some(address),
        }
    }

    /// Queue in frame order (stable for equal frames) within the preallocated capacity.
    fn push(&mut self, at: u32, channel: u8, id: EventId, kind: Kind) -> bool {
        self.push_from(at, channel, None, id, kind)
    }

    pub(super) fn native_standard_control(&mut self, param: params::GroupPar, value: crate::midi::ControlValue) {
        // The system action precedes user callbacks. Its original-time write
        // stays independent from accepted controller forwarding and wait().
        self.player.native_instrument_control(param, value);
        let id = match param { params::GroupPar::Volume => params::id::VOLUME, params::GroupPar::Pan => params::id::PAN, _ => return };
        self.queue_write(Write { at: 0, address: Address::Instrument(param),
            value: 0.0, native: 0, par: EnginePar { id, group: -1, slot: -1, generic: -1 },
            standard_controls_before: self.commands.iter().filter(|c| c.at == 0 && c.standard_control(param).is_some()).count(),
            standard_physical: Some(value), standard_authored: false });
    }

    fn queue_write(&mut self, mut w: Write) -> bool {
        if self.write_index.0 != w.at {
            self.write_index.0 = w.at;
            self.write_index.1.clear();
            self.write_index.1.extend(self.writes.iter().enumerate()
                .filter(|(_, previous)| previous.at == w.at).map(|(i, previous)| (previous.address, i)));
        }
        // Delay Time/Unit writes retain ordering because their caches interact.
        let delay_clock = matches!(w.address, Address::Fx(_, _, FxParam::Field(FxKind::Delay, 0 | 4)));
        if !delay_clock && let Some(&i) = self.write_index.1.get(&w.address) {
            if w.standard_physical.is_some() && self.writes[i].standard_authored {
                // A physical scalar replaces the effective control, keeping
                // the latest authored baseline available after it is cleared.
                w.value = self.writes[i].value;
                w.native = self.writes[i].native;
                w.standard_authored = true;
            }
            self.writes[i] = w;
            return true;
        }
        if self.writes.len() == MAX_WRITES {
            self.player.dropped_commands += 1;
            return true;
        }
        let i = self.writes.partition_point(|previous| previous.at <= w.at);
        self.writes.insert(i, w);
        self.write_index.1.insert(w.address, i);
        true
    }

    fn push_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, id: EventId, kind: Kind) -> bool {
        if !self.commands.push(at, channel, input_channel, id, kind) {
            self.player.dropped_commands += 1;
            if kind.is_stop() && channel < 16 { self.player.stop_overflow |= 1 << channel; }
            return false;
        }
        true
    }

    fn push_note(&mut self, at: u32, channel: u8, input_channel: Option<u8>, id: EventId, note: Note) -> bool {
        if !self.commands.push_note(at, channel, input_channel, id, note) {
            self.player.dropped_commands += 1;
            return false;
        }
        true
    }

    fn delay_fields(&self, rack: Rack, slot: u8) -> Option<crate::fx::blocks::Fields> {
        let (mut fields, tempo) = self.fx.delay_fields(rack, slot)?;
        for w in self.writes.iter() {
            if let Address::Fx(r, s, FxParam::Field(FxKind::Delay, n)) = w.address
                && (r, s) == (rack, slot) {
                crate::fx::blocks::set_delay_field(&mut fields, n, w.value, tempo);
            }
        }
        Some(fields)
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
    fn note_groups(&mut self, channel: u8, note: u8, groups: GroupMask) -> GroupMask {
        self.bank.map_or(groups, |bank| self.player.select_note_groups(bank, channel, note, false, groups))
    }

    fn observe_note(&mut self, at: u32, note: u8) {
        if self.bank.is_some() {
            self.push(at, self.channel, EventId::default(), Kind::ObserveNote(note));
        }
    }

    fn receive_library_control(&mut self, channel: u8, control: crate::midi::Control) {
        self.player.library_receiver.control(channel, control);
    }

    fn play_note(&mut self, at: u32, n: &NoteSpec<'_>) -> Option<EventId> {
        let bank = self.bank?;
        if n.force_completion {
            let mut zones = bank.matching_zones(n.channel, n.note, n.velocity, false, n.groups).peekable();
            zones.peek()?;
            if self.player.completion_has_release(bank, n.channel, n.note, n.velocity, n.groups, matches!(n.length, NoteLength::Sample))
                || zones.any(|z| !self.player.completion_supported(bank, self.state, z)) {
                self.player.unsupported_native_controls = self.player.unsupported_native_controls.saturating_add(1);
                return None;
            }
        }
        let id = self.player.next_id();
        let note = Note::Start(Start {
            host_note: n.host_note,
            per_note: n.per_note,
            script_mods: *n.script_mods,
            note: n.note,
            velocity: n.velocity,
            onset_velocity: n.onset_velocity,
            release_velocity: n.release_velocity,
            owner: n.owner,
            counter_stop: None,
            offset_us: n.sample_offset_us.max(0) as u64,
            volume: 10f32.powf(n.volume_mdb as f32 / 20_000.0),
            tune: f64::from(n.tune_mc) / 100_000.0,
            pan: n.pan.clamp(-1000, 1000) as f32 / 1000.0,
            whole: n.length == NoteLength::Sample,
            groups: *n.groups,
            group_start_applied: n.group_start_applied,
            native_release: n.native_release,
            force_completion: n.force_completion,
            expression: n.frozen_expression,
        });
        self.push_note(at, n.channel, n.input_channel, id, note).then_some(id)
    }

    fn release_expression(&self, at: u32, voice: Option<EventId>, channel: u8, input_channel: Option<u8>, note: u8) -> Option<Expression> {
        let queued = voice.and_then(|id| self.commands.iter().find(|c| !c.applied && c.id == id && matches!(c.kind, Kind::Start(_))));
        let active = voice.and_then(|id| self.player.voices.iter().find(|v| v.event == id));
        let logical = active.map(|v| v.channel).or_else(|| queued.map(|c| c.channel)).unwrap_or(channel);
        let physical = active.and_then(|v| v.input_channel).or_else(|| queued.and_then(|c| c.input_channel)).or(input_channel).unwrap_or(channel);
        self.player.midi.mpe.member(physical)?;
        let key = active.and_then(|v| v.owner).map_or(note, |(_, key)| key);
        active.and_then(|v| v.frozen_expression)
            .or_else(|| queued.and_then(|c| self.commands.start(c)).and_then(|n| n.expression))
            .or_else(|| Some(self.player.release_snapshot(self.commands.as_slice(), at, logical, physical, key)))
    }

    fn note_off(&mut self, at: u32, voice: EventId, n: &NoteSpec<'_>) {
        let note = Note::Release(Release {
            host_note: n.host_note,
            input_root: n.input_root,
            input_sostenuto: n.input_sostenuto,
            per_note: n.per_note,
            script_mods: *n.script_mods,
            trigger: (n.length != NoteLength::Sample).then_some((n.note, n.velocity)),
            onset_velocity: n.onset_velocity,
            release_velocity: n.release_velocity,
            groups: *n.groups,
            expression: n.frozen_expression.or_else(|| self.commands.iter().filter(|c| !c.applied && c.id == voice)
                .find_map(|c| self.commands.start(c).and_then(|n| n.expression))).or_else(|| self.player.release_expression(voice, n.channel, n.note, n.input_channel)),
        });
        self.push_note(at, n.channel, n.input_channel, voice, note);
    }

    fn freeze_expression(&mut self, at: u32, voice: EventId, expression: Expression) {
        self.freeze_expression_from(at, self.channel, None, voice, expression);
    }

    fn freeze_expression_from(&mut self, at: u32, _channel: u8, input_channel: Option<u8>, voice: EventId, expression: Expression) {
        // First freeze wins until a new Start for this event. Ignore events
        // without an accepted engine source; they cannot consume this quota.
        for c in self.commands.iter().rev().filter(|c| !c.applied && c.id == voice) {
            match c.kind {
                Kind::Freeze(_) => return,
                Kind::Start(_) => break,
                _ => {}
            }
        }
        let active = self.player.voices.iter().find(|v| v.event == voice);
        if active.is_some_and(|v| v.frozen_expression.is_some()) { return; }
        let channel = active.map(|v| v.channel);
        if let Some(c) = self.commands.iter().find(|c| !c.applied && c.id == voice && matches!(c.kind, Kind::Start(_))).copied() {
            if c.at >= at {
                let mut updated = false;
                self.commands.for_each_start_mut(|command, start| {
                    if !updated && !command.applied && command.id == voice {
                        start.expression.get_or_insert(expression);
                        updated = true;
                    }
                });
                return;
            }
            self.push_from(at, c.channel, input_channel, voice, Kind::Freeze(expression));
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
            VoicePar::ReleaseVelocity => EventChange::ReleaseVelocity(crate::midi::Velocity::from_midi1(value.clamp(0, 127) as u8)),
        };
        self.push_from(at, channel, input_channel, voice, Kind::Change(change));
    }

    fn set_script_mod(&mut self, at: u32, channel: u8, input_channel: Option<u8>, voice: EventId, source: u16, value: i32) {
        self.push_from(at, channel, input_channel, voice, Kind::Change(EventChange::ScriptMod { source, value }));
    }

    fn controller(&mut self, at: u32, cc: u8, value: i32) {
        self.controller_on_channel(at, self.channel, cc, value);
    }

    fn controller_on_channel(&mut self, at: u32, channel: u8, cc: u8, value: i32) {
        if let Some(control) = crate::midi::Control::from_legacy(cc, value) {
        self.performance_control_from(at, channel, None, crate::midi::ControlTarget::Authored, control, false);
        }
    }

    fn controller_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, cc: u8, value: i32) {
        if let Some(control) = crate::midi::Control::from_legacy(cc, value) {
        self.performance_control_from(at, channel, input_channel, crate::midi::ControlTarget::Authored, control, false);
        }
    }

    fn performance_control_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>,
        target: crate::midi::ControlTarget, control: crate::midi::Control, native_standard_handled: bool) {
        self.push_from(at, channel, input_channel, EventId::default(), Kind::Control { target, control, native_standard_handled, root_pedal_handled: false });
    }

    fn pedal_root_control_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>,
        target: crate::midi::ControlTarget, control: crate::midi::Control) -> (u16, u16) {
        use crate::midi::{Control, ControlTarget};
        let Control::Cc(cc @ (66 | 121), value) = control else { return (0, 0); };
        let native = channel < 16 && (matches!(target, ControlTarget::Authored) || self.state.native_note_action(cc));
        let channels = if native { self.player.control_channels(target, control) } else { 0 };
        let mut pedals = self.player.pedals;
        // Accepted queue order supplies the pedal timeline before DSP consumes
        // it. Future edges and reflected commands cannot capture today's roots.
        for command in self.commands.iter().filter(|command| !command.applied && command.at <= at) {
            if let Kind::Control { target, control: Control::Cc(cc @ (66 | 121), prior), .. } = command.kind {
                if matches!(target, ControlTarget::Physical(_)) && !self.state.native_note_action(cc) { continue; }
                let mask = self.player.control_channels(target, Control::Cc(cc, prior));
                pedals.set_sostenuto(mask, cc == 66 && prior.normalized() >= 0.5);
            }
        }
        if !self.push_from(at, channel, input_channel, EventId::default(),
            Kind::Control { target, control, native_standard_handled: false, root_pedal_handled: true }) { return (0, 0); }
        let change = pedals.set_sostenuto(channels, cc == 66 && value.normalized() >= 0.5);
        self.player.host_notes.sostenuto(change.capture, change.up);
        (change.capture, change.up)
    }

    fn host_pressure(&mut self, at: u32, owner: super::HostRef, value: crate::midi::ControlValue) {
        if let Some(note) = self.player.host_notes.get(owner) {
            self.push_from(at, note.channel, Some(note.note.channel), EventId::default(), Kind::HostPressure(owner, value));
        }
    }
    fn per_note_pressure(&mut self, at: u32, input: super::PerNoteInput, value: crate::midi::ControlValue) {
        self.push_from(at, self.channel, Some(input.address.channel), EventId::default(), Kind::PerNotePressure(input, value));
    }
    fn native_note_controller(&mut self, at: u32, input: super::PerNoteInput, value: crate::midi::NoteControl) {
        self.push_from(at, self.channel, Some(input.address.channel), EventId::default(), Kind::NoteController(input, value));
    }
    fn host_callback_finished(&mut self, at: u32, owner: super::HostRef) {
        // This is lifetime metadata, not an audible command. Record the
        // callback's exact future frame before its retained reference disappears.
        self.player.host_notes.record_end(Some(owner), self.frame_base.saturating_add(u64::from(at)));
    }
    fn poly_pressure_from(&mut self, at: u32, channel: u8, input_channel: Option<u8>, note: u8, value: crate::midi::ControlValue) {
        self.push_from(at, channel, input_channel, EventId::default(), Kind::PolyPressure { note, value });
    }
    fn group_count(&self) -> usize {
        self.bank.map_or(0, |b| b.groups().len())
    }

    fn zone_count(&self) -> usize { self.bank.map_or(0, Bank::zone_count) }
    fn zone_par(&self, zone: i32, par: crate::ksp::engine::ZonePar) -> Option<i32> {
        self.bank?.zone_par(zone,par)
    }
    fn request_zone_edit(&mut self, edit: crate::ksp::engine::ZoneEdit) -> Result<(), &'static str> {
        let bank = self.bank.ok_or("set_zone_par: no sample bank")?;
        let id = usize::try_from(edit.zone).map_err(|_|"set_zone_par: invalid source zone ID")?;
        let mut mapping = *bank.zone_state.as_ref().and_then(|s|s.maps.get(id)).ok_or("set_zone_par: invalid source zone ID")?;
        if !bank.zone_editable(id) { return Err("set_zone_par: source zone has no playable sample; mapping unchanged"); }
        mapping.set(edit.par,edit.value,bank.groups().len())?;
        if self.zone_requests.len() == self.zone_requests.capacity() { return Err("set_zone_par: request queue exhausted"); }
        self.zone_requests.push_back(edit);
        *self.zone_inflight += 1;
        Ok(())
    }


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
        let value = self.state.controller_policy.decode(address, value);
        let native_control = self.state.controller_policy.native_control(address);
        let standard_controls_before = if let Some(param) = native_control {
            self.commands.iter().filter(|c| c.at == at && c.standard_control(param).is_some()).count()
        } else { 0 };
        if let Address::GroupType(g, s) = address {
            return self.bank.and_then(|b| params::group_type(b.groups(), g, s)) == Some(value);
        }
        if !loaded(address, value, |r, s| self.fx.param(r, s, FxParam::Type)) {
            return false;
        }
        if let Address::Fx(r, s, FxParam::Field(FxKind::Delay, n @ (0 | 4))) = address {
            let Some(mut fields) = self.delay_fields(r, s) else { return false };
            let tempo = self.fx.delay_fields(r, s).map_or(120.0, |(_, tempo)| tempo);
            if !crate::fx::blocks::set_delay_field(&mut fields, n, value, tempo) { return false }
        }
        self.queue_write(Write { at, address, value, par, native, standard_controls_before,
            standard_physical: None, standard_authored: native_control.is_some() })
    }

    /// The latest queued value, else the engine's current one.
    fn engine_par(&self, par: EnginePar) -> Option<i32> {
        let address = self.address(par)?;
        if let Address::Fx(r, s, FxParam::Field(FxKind::Delay, n @ (0 | 4))) = address {
            let fields = self.delay_fields(r, s)?;
            let tempo = self.fx.delay_fields(r, s)?.1;
            return Some(address.encode(crate::fx::blocks::normalized_field(FxKind::Delay, n, &fields, tempo)?));
        }
        let queued = self.writes.iter().rev().find(|w| w.address == address);
        if let Some(param) = self.state.controller_policy.native_control(address) {
            let authored_value = || self.state.control_value(param).unwrap_or_else(||
                self.state.controller_policy.encode(address, self.state.controls.read(param).unwrap()));
            let encode_physical = |value| self.state.controller_policy.encode(address,
                super::instrument::ControllerPolicy::control_value(param, value, self.state.controller_options.cc7_max_db));
            let last = self.commands.iter().rev().find(|c| c.standard_control(param).is_some());
            let after = last.is_some_and(|c| queued.is_none_or(|w| c.at > w.at
                || c.at == w.at && self.commands.iter()
                    .filter(|c| c.at == w.at && c.standard_control(param).is_some()).count() > w.standard_controls_before));
            if after && let Some((target, value)) = last.and_then(|c| c.standard_control(param)) {
                if matches!(target, crate::midi::ControlTarget::Physical(_))
                    && self.state.controller_options.standard_volume_pan {
                    return Some(encode_physical(value));
                }
                // Authored standard controllers clear physical authority;
                // their output control stays separate from instrument controls.
                return Some(queued.filter(|w| w.standard_authored).map_or_else(authored_value, |w| w.native.clamp(0, 1_000_000)));
            }
            if let Some(w) = queued {
                if self.state.controller_options.standard_volume_pan && let Some(value) = w.standard_physical {
                    return Some(encode_physical(value));
                }
                return Some(if w.standard_authored { w.native.clamp(0, 1_000_000) }
                    else { authored_value() });
            }
            if self.player.physical_instrument_control(self.state, param).is_none()
                && let Some(value) = self.state.control_value(param) { return Some(value); }
        }
        let value = match queued {
            Some(w) => w.value,
            None => match address {
                Address::Fx(rack, slot, param) => self.fx.param(rack, slot, param)?,
                Address::Instrument(p) => self.player.instrument_controls(self.state).read(p)?,
                Address::GroupType(g, s) => params::group_type(self.bank?.groups(), g, s)?,
                _ => params::read(&self.state.base, address)?,
            },
        };
        Some(self.state.controller_policy.encode(address, value))
    }

    fn engine_par_display(&self, par: EnginePar, value: i32) -> Option<super::Disp> {
        let address = self.address(par)?;
        if let Some(param) = self.state.controller_policy.conversion_parameter(address) {
            let value = self.state.controller_policy.decode(address, value);
            return Some(match param { params::GroupPar::Volume => super::Disp::Gain(value), params::GroupPar::Pan => super::Disp::Pan(value), _ => unreachable!() });
        }
        let Address::Fx(r, s, FxParam::Field(FxKind::Delay, 0)) = address else { return None };
        crate::fx::blocks::delay_display(&self.delay_fields(r, s)?, value)
    }

    fn find_mod(&self, group: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        params::find_mod(self.bank?.groups(), group, is)
    }

    fn find_target(&self, group: usize, modulator: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        params::find_target(self.bank?.groups(), group, modulator, is)
    }

    fn voice_zone(&self, voice: EventId) -> Option<i32> {
        self.event_voice(voice).map(|v| v.zone_id as i32)
    }

    fn voice_play_pos(&self, voice: EventId) -> Option<i32> {
        let v = self.event_voice(voice)?;
        let rate = self.bank?.samples[v.sample as usize].rate;
        Some((v.physical_pos()? * 1e6 / f64::from(rate)).clamp(0., i32::MAX as f64) as i32)
    }

    fn played_voices(&self) -> usize { self.player.voices.len() }

    fn played_voices_total(&self) -> Option<usize> {
        self.peer_voices.map(|peers| peers.saturating_add(self.player.voices.len()))
    }

    fn voice_active(&self, voice: EventId) -> bool {
        self.player.voices.iter().any(|v| v.event == voice)
            || self
                .commands
                .iter()
                .any(|c| !c.applied && c.id == voice && matches!(c.kind, Kind::Start(_) | Kind::Release(_)))
    }
}

impl Player {
    /// Apply one script command; its notes play on MIDI `channel`.
    pub(super) fn apply(&mut self, bank: Option<&Bank>, state: &InstrumentState, c: &Command, snapshot: Option<&Note>, defaults: Ahdsr) {
        let id = c.id;
        let channel = c.channel;
        match &c.kind {
            &Kind::ObserveNote(note) => self.note_history.observe(note),
            Kind::Start(_) => {
                let Some(&Note::Start(Start {
                    host_note,
                    per_note,
                    ref script_mods,
                    note,
                    velocity,
                    onset_velocity,
                    release_velocity,
                    owner,
                    counter_stop,
                    offset_us,
                    volume,
                    tune,
                    pan,
                    whole,
                    expression,
                    ref groups,
                    group_start_applied,
                    native_release,
                    force_completion,
                })) = snapshot else { return; };
                let Some(bank) = bank else { return; };
                let event = NoteEvent {
                    host_note,
                    per_note,
                    script_mods,
                    channel,
                    note,
                    velocity,
                    onset_velocity,
                    release_velocity,
                    owner,
                    input_channel: c.input_channel,
                    counter_stop,
                    release_held_ms: None,
                    groups: Some(groups),
                    group_start_applied,
                    native_release,
                    offset_us,
                    volume,
                    tune,
                    pan,
                    frozen_expression: expression,
                };
                if force_completion && self.completion_has_release(bank, channel, note, velocity, groups, whole) {
                    self.unsupported_native_controls = self.unsupported_native_controls.saturating_add(1);
                    return;
                }
                let started = self.start_with_completion(bank, state, &event, id, false, defaults, force_completion);
                if force_completion && started.is_none() { return; }
                // With the system release script bypassed, the script owns
                // when its selected release groups start, for every duration.
                if whole || !self.native_release_triggers {
                    self.start(bank, state, &event, id, true, defaults);
                }
            }
            Kind::Release(_) => {
                let Some(Note::Release(Release { trigger, onset_velocity, release_velocity, groups, expression, host_note, input_root, input_sostenuto, per_note, script_mods })) = snapshot else { return; };
                let Some(bank) = bank else { return; };
                self.accept_release(bank, state, ReleaseRequest {
                    source: id,
                    release_velocity: *release_velocity,
                    capture: Capture::PhysicalFallback(*input_root && *input_sostenuto),
                    sample: trigger.map(|(note, _)| ReleaseSample::Snapshot(ReleaseSnapshot {
                        channel, note, velocity: *onset_velocity, groups, expression: *expression,
                        input_channel: c.input_channel, host_note: *host_note, per_note: *per_note,
                        script_mods: Some(script_mods), held_ms: None,
                    })),
                }, defaults);
            }
            &Kind::HostPressure(owner, value) => {
                if self.host_notes.change(owner, super::HostExpression::Pressure(value)) { self.touch(); }
            }
            &Kind::PerNotePressure(input, value) => {
                if self.per_note.pressure(input, value) { self.touch(); }
            }
            &Kind::NoteController(input, value) => {
                if self.per_note.note_controller(input, value) { self.touch(); }
                else if !super::per_note::InputControls::supports_controller(value) {
                    self.unsupported_native_controls = self.unsupported_native_controls.saturating_add(1);
                }
            }
            &Kind::PolyPressure { note, value } => {
                let mut expression = self.midi.expression[channel as usize & 15][note as usize & 127];
                expression.note_pressure = Some(value);
                self.midi.set_expression(channel, note, expression);
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
            &Kind::Control { target, control, native_standard_handled, root_pedal_handled } => self.control_with_standard_provenance(bank, state, channel, target, control, defaults, native_standard_handled, root_pedal_handled),
            &Kind::ResetCounter(note) => {
                if state.controller_policy == super::instrument::ControllerPolicy::Native {
                    self.release_counter.reset(self.now, note, self.rate);
                    return;
                }
                self.key_on[channel as usize & 15][note as usize & 127] = self.now;
                for (input, keys) in self.input_keys.iter_mut().enumerate() {
                    let key = &mut keys[note as usize & 127];
                    if key.channel == channel && key.onset_velocity.is_some()
                        && c.input_channel.is_none_or(|owner| usize::from(owner) == input)
                    {
                        key.started = self.now;
                    }
                }
                for v in self.voices.iter_mut().filter(|v| v.channel == channel && v.note == note && !v.release_trigger && v.counter_stop.is_none()
                    && c.input_channel.is_none_or(|owner| v.input_channel == Some(owner))) {
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
        Address::LfoFrequency { .. } => value.is_finite(),
        Address::Fx(rack, slot, FxParam::Type) => kind(rack, slot) == Some(value),
        _ => true,
    }
}

/// Engine view for `on init` off the audio thread: group names, modulators,
/// the sample rate and the parameters the engine models, which it records
/// for the playing engine ([`Runtime::init_engine_pars`]). Nothing plays
/// during init.
pub struct ScriptSetup<'a> {
    groups: &'a [Group],
    zones: usize,
    zone_mappings: Vec<super::zone::Mapping>,
    zone_edits: Vec<crate::ksp::engine::ZoneEdit>,
    /// The instrument's effects with the ones scripts loaded.
    fx: std::borrow::Cow<'a, ProgramFx>,
    path: &'a std::path::Path,
    rate: f64,
    settings: Vec<GroupSettings>,
    instrument: InstrumentControls,
    controller_policy: super::instrument::ControllerPolicy,
    /// Effect values written so far, by address.
    effects: Vec<(Address, f32)>,
    pars: Vec<(EnginePar, i32)>,
    saved: Vec<crate::ksp::engine::NativeEdit>,
    saved_delays: Vec<crate::fx::DelayState>,
    controllers: Vec<(u8, u8)>,
    /// Effects and impulse responses loaded, in order.
    loads: Vec<ScriptIr>,
}

impl<'a> ScriptSetup<'a> {
    pub fn new(instrument: &'a Instrument, rate: f64) -> Self {
        let mut settings: Vec<_> = instrument.groups.iter().map(GroupSettings::from).collect();
        super::bank::gate_primary_bypass(&mut settings, &instrument.zones);
        Self {
            groups: &instrument.groups,
            zones: instrument.zones.len(),
            zone_mappings: instrument.zones.iter().map(super::zone::Mapping::from).collect(),
            zone_edits: Vec::new(),
            fx: std::borrow::Cow::Borrowed(&instrument.fx),
            path: &instrument.path,
            rate,
            settings,
            instrument: instrument.program,
            controller_policy: super::instrument::ControllerPolicy::for_instrument(instrument),
            effects: Vec::new(),
            pars: Vec::new(),
            saved: Vec::new(),
            saved_delays: Vec::new(),
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
        state.prepare_delays(&self.fx);
        state
    }

    fn address(&self, par: EnginePar) -> Option<Address> {
        let address = Address::resolve(par, self.groups)?;
        match address {
            Address::PrimaryBypass(..) => params::read(&self.settings, address).map(|_| address),
            // Preserve unclassified native beat values while permitting a
            // transition to one of the documented named units.
            Address::Fx(rack, slot, FxParam::Field(FxKind::Delay, 4))
                if self.fx.delay_fields(rack, slot).is_some() => Some(address),
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
        // A newly created Delay can now consume its validated physical state
        // before subsequent init getters; other kinds never receive it.
        for saved in self.saved_delays.iter().filter(|s| (s.rack, s.slot) == (rack, slot)) {
            self.fx.to_mut().restore_delay(saved);
        }
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
    fn zone_par(&self, zone: i32, par: crate::ksp::engine::ZonePar) -> Option<i32> {
        usize::try_from(zone).ok().and_then(|id|self.zone_mappings.get(id)).map(|m|m.get(par))
    }
    fn request_zone_edit(&mut self, edit: crate::ksp::engine::ZoneEdit) -> Result<(), &'static str> {
        let id = usize::try_from(edit.zone).map_err(|_|"set_zone_par: invalid source zone ID")?;
        if self.zone_edits.len() >= self.zones.saturating_mul(3).saturating_add(64) { return Err("set_zone_par: init request queue exhausted"); }
        self.zone_mappings.get_mut(id).ok_or("set_zone_par: invalid source zone ID")?.set(edit.par,edit.value,self.groups.len())?;
        self.zone_edits.push(edit);
        // Init runs on the loader. These IDs complete only after the bank and
        // the prepared mappings have reached the playing engine.
        Ok(())
    }


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
        let v = self.controller_policy.decode(address, value);
        if let Address::Fx(rack, slot, FxParam::Convolution(n)) = address {
            let mut settings = self.loads.iter().rev().find_map(|l| match l.load {
                Load::Convolution(s) if (l.rack, l.slot) == (rack, slot) => Some(s), _ => None,
            }).unwrap_or_else(|| self.fx.ir_settings_with(&[]).into_iter()
                .find(|s| (s.rack, s.slot) == (rack, slot)).map_or(crate::fx::params::IrSettings::DEFAULT, |s| s.settings));
            if !settings.set(n, v) { return false }
            self.loads.retain(|l| (l.rack, l.slot) != (rack, slot) || !matches!(l.load, Load::Convolution(_)));
            self.loads.push(ScriptIr { rack, slot, load: Load::Convolution(settings) });
        }
        if let Address::Fx(r, s, FxParam::Field(FxKind::Delay, n @ (0 | 4))) = address {
            if !self.fx.to_mut().set_delay_field(r, s, n, v) { return false }
            self.pars.push((par, value));
            return true;
        }
        match address {
            Address::GroupType(g, s) => return params::group_type(self.groups, g, s) == Some(v),
            Address::Fx(rack, slot, FxParam::Type)
                if self.fx.param(rack, slot, FxParam::Type) != Some(v)
                    && !self.load_kind(rack, slot, v) => return false,
            _ => {}
        }
        match address {
            Address::Fx(..) => match self.effects.iter_mut().find(|e| e.0 == address) {
                Some(e) => e.1 = v,
                None => self.effects.push((address, v)),
            },
            Address::Instrument(p) => {
                if !self.instrument.write(p, v) { return false }
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
        if self.controller_policy.native_control(address).is_some()
            && let Some((_, value)) = self.pars.iter().rev().find(|(par, _)| self.address(*par) == Some(address)) {
            return Some((*value).clamp(0, 1_000_000));
        }
        let value = match address {
            Address::Fx(rack, slot, param) => match self.effects.iter().find(|e| e.0 == address) {
                Some(e) => e.1,
                None => self.fx.param(rack, slot, param)?,
            },
            Address::Instrument(p) => self.instrument.read(p)?,
            Address::GroupType(g, s) => params::group_type(self.groups, g, s)?,
            _ => params::read(&self.settings, address)?,
        };
        Some(self.controller_policy.encode(address, value))
    }

    fn engine_par_display(&self, par: EnginePar, value: i32) -> Option<super::Disp> {
        let address = self.address(par)?;
        if let Some(param) = self.controller_policy.conversion_parameter(address) {
            let value = self.controller_policy.decode(address, value);
            return Some(match param { params::GroupPar::Volume => super::Disp::Gain(value), params::GroupPar::Pan => super::Disp::Pan(value), _ => unreachable!() });
        }
        let Address::Fx(r, s, FxParam::Field(FxKind::Delay, 0)) = address else { return None };
        crate::fx::blocks::delay_display(&self.fx.delay_fields(r, s)?, value)
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
