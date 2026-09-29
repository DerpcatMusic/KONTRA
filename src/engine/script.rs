//! The engine side of the KSP boundary. On the audio thread a borrowed
//! [`Host`] turns runtime calls into [`Command`]s stamped with their frame in
//! the next render; `Engine::render` applies them there, so script timing is
//! sample-accurate without per-sample checks. [`ScriptSetup`] serves `on init`
//! off the audio thread, where nothing can play.

use super::{Ahdsr, Bank, EventChange, EventId, GroupMask, NoteEvent, Player};
use crate::import::Group;
use crate::ksp::{EnginePar, Fade, KspEngine, NoteSpec, VoicePar};

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
        groups: GroupMask,
    },
    /// Release, with the groups its release triggers may start in.
    Release(GroupMask),
    Fade(Fade),
    Change(EventChange),
    Controller {
        cc: u8,
        value: i32,
    },
}

/// The engine as the runtime sees it during one call, borrowed from `Engine`.
pub(super) struct Host<'a> {
    pub bank: Option<&'a Bank>,
    pub player: &'a mut Player,
    pub commands: &'a mut Vec<Command>,
}

impl Host<'_> {
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
            groups: *n.groups,
        };
        self.push(at, id, kind).then_some(id)
    }

    fn note_off(&mut self, at: u32, voice: EventId, groups: &GroupMask) {
        self.push(at, voice, Kind::Release(*groups));
    }

    fn fade(&mut self, at: u32, voice: EventId, fade: Fade) {
        self.push(at, voice, Kind::Fade(fade));
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

    // Engine parameters are not modelled yet: the runtime keeps the values.
    fn set_engine_par(&mut self, _par: EnginePar, _value: i32) -> bool {
        false
    }

    fn engine_par(&self, _par: EnginePar) -> Option<i32> {
        None
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
            }
            Kind::Release(groups) => self.release_event(bank, id, groups, defaults),
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
            // Channel pressure and other virtual controllers: nothing in the engine uses them.
            Kind::Controller { .. } => {}
        }
    }
}

/// Engine view for `on init` off the audio thread: group names and the sample
/// rate. Nothing plays during init.
pub struct ScriptSetup<'a> {
    pub groups: &'a [Group],
    pub rate: f64,
}

impl KspEngine for ScriptSetup<'_> {
    fn play_note(&mut self, _at: u32, _note: &NoteSpec<'_>) -> Option<EventId> {
        None
    }

    fn note_off(&mut self, _at: u32, _voice: EventId, _groups: &GroupMask) {}

    fn fade(&mut self, _at: u32, _voice: EventId, _fade: Fade) {}

    fn set_par(&mut self, _at: u32, _voice: EventId, _par: VoicePar, _value: i32) {}

    fn controller(&mut self, _at: u32, _cc: u8, _value: i32) {}

    fn group_count(&self) -> usize {
        self.groups.len()
    }

    fn group_name(&self, group: usize) -> &str {
        self.groups.get(group).map_or("", |g| &g.name)
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn set_engine_par(&mut self, _par: EnginePar, _value: i32) -> bool {
        false
    }

    fn engine_par(&self, _par: EnginePar) -> Option<i32> {
        None
    }
}
