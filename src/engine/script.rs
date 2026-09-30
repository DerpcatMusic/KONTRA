//! The engine side of the KSP boundary. On the audio thread a borrowed
//! [`Host`] turns runtime calls into [`Command`]s stamped with their frame in
//! the next render; `Engine::render` applies them there, so script timing is
//! sample-accurate without per-sample checks. [`ScriptSetup`] serves `on init`
//! off the audio thread, where nothing can play.

use super::params::{self, Address, GroupPar, MAX_WRITES, Write};
use super::{Ahdsr, Bank, EventChange, EventId, GroupMask, GroupSettings, NoteEvent, Player};
use crate::fx::{FxProcessor, ProgramFx};
use crate::import::{Group, Instrument};
use crate::ksp::{EnginePar, Fade, KspEngine, NoteLength, NoteSpec, Persisted, Runtime, VoicePar};

/// Script output buses, as Kontakt's default output section.
const SCRIPT_OUTPUTS: usize = 8;

/// Initialize an instrument's scripts off the audio thread (`on init` may take
/// a while), restoring `persisted` values. `None` when it has no scripts.
/// Failing slots stay in place, passing events through; their errors return.
pub fn load_scripts(
    instrument: &Instrument,
    persisted: Vec<Persisted>,
    rate: f64,
) -> (Option<Box<Runtime>>, Vec<String>) {
    if instrument.scripts.is_empty() {
        return (None, Vec::new());
    }
    let mut setup = ScriptSetup::new(instrument, rate);
    let (mut rt, errors) =
        Runtime::with_scripts(&instrument.scripts, &mut setup, SCRIPT_OUTPUTS, persisted);
    rt.init_engine_pars = setup.pars;
    rt.init_controllers = setup.controllers;
    let errors = errors
        .into_iter()
        .enumerate()
        .filter_map(|(slot, e)| Some(format!("Script {}: {}", slot + 1, e?)))
        .collect();
    crate::audio::trim_heap();
    (Some(Box::new(rt)), errors)
}

/// Script engine calls one render can hold; the rest are dropped and counted.
pub const MAX_COMMANDS: usize = 256;

/// One engine call, applied at frame `at` of the next render.
#[derive(Clone, Copy)]
pub(super) struct Command {
    pub at: u32,
    /// Target event; unused by controllers.
    pub id: EventId,
    pub kind: Kind,
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Start {
        note: u8,
        velocity: u8,
        offset_us: u64,
        volume: f32,
        tune: f64,
        pan: f32,
        /// Whole-sample notes (`play_note` duration 0) also fire release
        /// triggers at once, which is how scripts play release samples.
        whole: bool,
        groups: GroupMask,
    },
    /// Key release; `trigger` carries note, velocity and groups for release
    /// triggers unless the note already fired them.
    Release {
        trigger: Option<(u8, u8)>,
        groups: GroupMask,
    },
    Fade(Fade),
    Change(EventChange),
    Controller {
        cc: u8,
        value: i32,
    },
    /// `reset_rls_trig_counter(note)`.
    ResetCounter(u8),
}

/// The engine as the runtime sees it during one call, borrowed from `Engine`.
pub(super) struct Host<'a> {
    pub bank: Option<&'a Bank>,
    pub fx: &'a FxProcessor,
    pub player: &'a mut Player,
    pub commands: &'a mut Vec<Command>,
    pub writes: &'a mut Vec<Write>,
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
    fn push(&mut self, at: u32, id: EventId, kind: Kind) -> bool {
        if self.commands.len() == MAX_COMMANDS {
            self.player.dropped_commands += 1;
            return false;
        }
        let i = self.commands.partition_point(|c| c.at <= at);
        self.commands.insert(i, Command { at, id, kind });
        true
    }
}

impl KspEngine for Host<'_> {
    fn play_note(&mut self, at: u32, n: &NoteSpec<'_>) -> Option<EventId> {
        self.bank?;
        let id = self.player.next_id();
        let kind = Kind::Start {
            note: n.note,
            velocity: n.velocity,
            offset_us: n.sample_offset_us.max(0) as u64,
            volume: 10f32.powf(n.volume_mdb as f32 / 20_000.0),
            tune: f64::from(n.tune_mc) / 100_000.0,
            pan: n.pan.clamp(-1000, 1000) as f32 / 1000.0,
            whole: n.length == NoteLength::Sample,
            groups: *n.groups,
        };
        self.push(at, id, kind).then_some(id)
    }

    fn note_off(&mut self, at: u32, voice: EventId, n: &NoteSpec<'_>) {
        let kind = Kind::Release {
            trigger: (n.length != NoteLength::Sample).then_some((n.note, n.velocity)),
            groups: *n.groups,
        };
        self.push(at, voice, kind);
    }

    fn fade(&mut self, at: u32, voice: EventId, fade: Fade) {
        self.push(at, voice, Kind::Fade(fade));
    }

    fn reset_release_counter(&mut self, at: u32, note: u8) {
        self.push(at, EventId::default(), Kind::ResetCounter(note));
    }

    fn set_par(&mut self, at: u32, voice: EventId, par: VoicePar, value: i32) {
        let change = match par {
            VoicePar::VolumeMdb => EventChange::Volume(10f32.powf(value as f32 / 20_000.0)),
            VoicePar::TuneMc => EventChange::Tune(f64::from(value) / 100_000.0),
            VoicePar::Pan => EventChange::Pan(value.clamp(-1000, 1000) as f32 / 1000.0),
        };
        self.push(at, voice, Kind::Change(change));
    }

    fn controller(&mut self, at: u32, cc: u8, value: i32) {
        self.push(at, EventId::default(), Kind::Controller { cc, value });
    }

    fn group_count(&self) -> usize {
        self.bank.map_or(0, |b| b.groups().len())
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
            return false;
        };
        if self.writes.len() == MAX_WRITES {
            self.player.dropped_commands += 1;
            return true;
        }
        let i = self.writes.partition_point(|w| w.at <= at);
        let value = address.decode(value);
        self.writes.insert(i, Write { at, address, value });
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
                _ => params::read(&self.bank?.settings, address)?,
            },
        };
        Some(address.encode(value))
    }

    fn find_mod(&self, group: usize, name: &str) -> Option<usize> {
        params::find_mod(self.bank?.groups(), group, name)
    }

    fn find_target(&self, group: usize, modulator: usize, name: &str) -> Option<usize> {
        params::find_target(self.bank?.groups(), group, modulator, name)
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
    pub(super) fn apply(&mut self, bank: &Bank, c: &Command, channel: u8, defaults: Ahdsr) {
        let id = c.id;
        match &c.kind {
            &Kind::Start {
                note,
                velocity,
                offset_us,
                volume,
                tune,
                pan,
                whole,
                ref groups,
            } => {
                let event = NoteEvent {
                    channel,
                    note,
                    velocity,
                    groups: Some(groups),
                    offset_us,
                    volume,
                    tune,
                    pan,
                };
                self.start(bank, &event, id, false, defaults);
                if whole {
                    self.start(bank, &event, id, true, defaults);
                }
            }
            Kind::Release { trigger, groups } => {
                self.release_voices(bank, id);
                if let &Some((note, velocity)) = trigger {
                    self.trigger_release(bank, (channel, note, velocity), groups, defaults);
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
            }
            &Kind::Controller { cc, value } if cc < 128 => {
                let value = value.clamp(0, 127) as u8;
                self.cc(Some(bank), channel, cc, value, defaults);
            }
            &Kind::Controller { cc: 129, value } => {
                self.pressure[channel as usize] = value.clamp(0, 127) as u8;
            }
            // Other virtual controllers: nothing in the engine uses them.
            Kind::Controller { .. } => {}
            &Kind::ResetCounter(note) => {
                self.key_on[channel as usize & 15][note as usize & 127] = self.now;
            }
        }
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
    fx: &'a ProgramFx,
    rate: f64,
    settings: Vec<GroupSettings>,
    instrument: (f32, f32, f32),
    /// Effect values written so far, by address.
    effects: Vec<(Address, f32)>,
    pars: Vec<(EnginePar, i32)>,
    controllers: Vec<(u8, u8)>,
}

impl<'a> ScriptSetup<'a> {
    pub fn new(instrument: &'a Instrument, rate: f64) -> Self {
        Self {
            groups: &instrument.groups,
            fx: &instrument.fx,
            rate,
            settings: instrument.groups.iter().map(GroupSettings::from).collect(),
            instrument: (1.0, 0.0, 0.0),
            effects: Vec::new(),
            pars: Vec::new(),
            controllers: Vec::new(),
        }
    }

    fn address(&self, par: EnginePar) -> Option<Address> {
        let address = Address::resolve(par, self.groups)?;
        match address {
            Address::Fx(rack, slot, param) => self.fx.param(rack, slot, param).map(|_| address),
            _ => Some(address),
        }
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

    fn group_name(&self, group: usize) -> &str {
        self.groups.get(group).map_or("", |g| &g.name)
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn set_engine_par(&mut self, _at: u32, par: EnginePar, value: i32) -> bool {
        let Some(address) = self.address(par) else {
            return false;
        };
        let v = address.decode(value);
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
                params::write(&mut self.settings, address, v);
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
            _ => params::read(&self.settings, address)?,
        };
        Some(address.encode(value))
    }

    fn find_mod(&self, group: usize, name: &str) -> Option<usize> {
        params::find_mod(self.groups, group, name)
    }

    fn find_target(&self, group: usize, modulator: usize, name: &str) -> Option<usize> {
        params::find_target(self.groups, group, modulator, name)
    }
}
