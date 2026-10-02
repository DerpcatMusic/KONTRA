//! The boundary between the KSP runtime and the sampler engine. Every call is made
//! on the audio thread at a frame offset within the current block.

pub use super::builtins::{ENGINE_PAR_BASE, engine_par_name};
pub use crate::engine::{EventId, Expression, GroupMask};
use serde::Serialize;
use std::fmt::Write as _;

/// How long the engine should play a note when no note-off arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteLength {
    /// Sustain until the runtime calls `note_off` (duration -1 or a timed duration).
    UntilNoteOff,
    /// Play the whole sample (duration 0).
    Sample,
}

/// A note event that has passed every script slot.
#[derive(Clone, Copy, Debug)]
pub struct NoteSpec<'a> {
    /// KSP event ID, for logging and correlation only.
    pub event: i32,
    pub channel: u8,
    /// A released parent's expression, inherited by generated release samples.
    pub frozen_expression: Option<Expression>,
    /// Physical channel and original input key, inherited by following notes.
    pub owner: Option<(u8, u8)>,
    /// Cancellation provenance, inherited even by independent generated notes.
    pub input_channel: Option<u8>,
    pub note: u8,
    pub velocity: u8,
    /// Sample start offset in microseconds.
    pub sample_offset_us: i64,
    pub length: NoteLength,
    /// Event volume in millidecibels (0 = unity).
    pub volume_mdb: i32,
    /// Event tuning in millicents.
    pub tune_mc: i32,
    /// Event pan, -1000 (left) to 1000 (right).
    pub pan: i32,
    pub groups: &'a GroupMask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fade {
    /// Start silent and rise to unity.
    In { duration_us: i32 },
    /// Fall to silence, optionally stopping the voice at the end.
    Out { duration_us: i32, stop: bool },
}

/// Per-event parameters that can change while a voice plays. Values are absolute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoicePar {
    VolumeMdb,
    TuneMc,
    Pan,
}

/// `get/set_engine_par` address. `id` is from [`super::builtins::ENGINE_PARS`]
/// (`ENGINE_PAR_BASE + index`) or a script-local symbol the engine cannot know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, serde::Deserialize)]
pub struct EnginePar {
    pub id: i32,
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
}

/// One supported native parameter edit, saved independently of script variables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct NativeEdit {
    pub par: EnginePar,
    pub value: i32,
}


pub trait KspEngine {
    fn play_note(&mut self, at: u32, note: &NoteSpec<'_>) -> Option<EventId>;
    /// Freeze per-note controls before a release callback can wait or create notes.
    fn release_expression(&self, _at: u32, _voice: Option<EventId>, _channel: u8, _note: u8) -> Option<Expression> { None }
    /// Stop following member controls at this frame while a child callback waits.
    fn freeze_expression(&mut self, _at: u32, _voice: EventId, _expression: Expression) {}
    /// Release a voice. `event` is its note event now: release triggers use its
    /// key, velocity and groups (whole-sample notes fired them at the start).
    fn note_off(&mut self, at: u32, voice: EventId, event: &NoteSpec<'_>);
    fn fade(&mut self, at: u32, voice: EventId, fade: Fade);
    fn set_par(&mut self, at: u32, voice: EventId, par: VoicePar, value: i32);
    fn freeze_expression_from(&mut self, at: u32, _channel: u8, _input_channel: Option<u8>, voice: EventId, expression: Expression) {
        self.freeze_expression(at, voice, expression);
    }
    fn fade_from(&mut self, at: u32, _channel: u8, _input_channel: Option<u8>, voice: EventId, fade: Fade) {
        self.fade(at, voice, fade);
    }
    fn set_par_from(&mut self, at: u32, _channel: u8, _input_channel: Option<u8>, voice: EventId, par: VoicePar, value: i32) {
        self.set_par(at, voice, par, value);
    }
    /// A controller that passed every slot: 0..127, 128 pitch bend (-8192..8191),
    /// 129 channel pressure.
    fn controller(&mut self, at: u32, cc: u8, value: i32);
    fn controller_on_channel(&mut self, at: u32, _channel: u8, cc: u8, value: i32) {
        self.controller(at, cc, value);
    }
    fn controller_from(&mut self, at: u32, channel: u8, _input_channel: Option<u8>, cc: u8, value: i32) {
        self.controller_on_channel(at, channel, cc, value);
    }
    fn group_count(&self) -> usize;
    fn zone_count(&self) -> usize { 0 }
    /// Opaque positive identity of the source zone at `index`.
    fn zone_id(&self, index: usize) -> Option<i32> {
        (index < self.zone_count()).then(|| index as i32 + 1)
    }
    fn group_name(&self, group: usize) -> &str;
    fn sample_rate(&self) -> f64;
    /// Set at frame `at`. Returns false when the engine does not implement the
    /// parameter; the runtime then stores the value itself and reports it.
    fn set_engine_par(&mut self, at: u32, par: EnginePar, value: i32) -> bool;
    fn engine_par(&self, par: EnginePar) -> Option<i32>;
    /// Index within a group of the first modulator whose name `is` accepts.
    fn find_mod(&self, _group: usize, _is: &dyn Fn(&str) -> bool) -> Option<usize> {
        None
    }
    /// Index of the first modulation target whose name `is` accepts.
    fn find_target(&self, _group: usize, _modulator: usize, _is: &dyn Fn(&str) -> bool) -> Option<usize> {
        None
    }
    /// The instrument file, from engines that run off the audio thread
    /// (`on init`), where scripts may read files (`load_array`,
    /// `get_folder`). `None` while playing.
    fn instrument_path(&self) -> Option<&std::path::Path> {
        None
    }
    /// `load_ir_sample`: load impulse response `file` into convolution
    /// `slot` of rack `generic`. Whether it loaded; `None` when this engine
    /// cannot load impulse responses.
    fn load_ir_sample(&mut self, _file: &str, _slot: i32, _generic: i32) -> Option<bool> {
        None
    }
    /// Queue an IR load off the audio thread. True defers completion until
    /// the host installs it; false fails immediately; None uses the init loader.
    fn request_ir_sample(&mut self, _file: &str, _slot: i32, _generic: i32, _script_slot: u8, _id: i32) -> Option<bool> {
        None
    }
    /// `reset_rls_trig_counter`: restart `note`'s release-trigger counter.
    fn reset_release_counter(&mut self, _at: u32, _note: u8) {}
    fn reset_release_counter_on_channel(&mut self, at: u32, _channel: u8, note: u8) {
        self.reset_release_counter(at, note);
    }
    fn reset_release_counter_from(&mut self, at: u32, channel: u8, _input_channel: Option<u8>, note: u8) {
        self.reset_release_counter_on_channel(at, channel, note);
    }
    /// Highest zone identity among this event's sounding or queued voices;
    /// -1 when an accepted queued event matches no playable zone.
    fn voice_zone(&self, _voice: EventId) -> Option<i32> { None }
    /// Whether a voice is still sounding; drives `event_status` for sample-length notes.
    fn voice_active(&self, _voice: EventId) -> bool {
        true
    }
}

/// Recorded engine call, serialized by `kontakto ksp-run`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum EngineCall {
    PlayNote {
        time: u64,
        voice: EventId,
        event: i32,
        channel: u8,
        note: u8,
        velocity: u8,
        sample_offset_us: i64,
        length: NoteLength,
        volume_mdb: i32,
        tune_mc: i32,
        pan: i32,
        groups: Vec<usize>,
    },
    NoteOff {
        time: u64,
        voice: EventId,
    },
    Fade {
        time: u64,
        voice: EventId,
        fade: Fade,
    },
    SetPar {
        time: u64,
        voice: EventId,
        par: VoicePar,
        value: i32,
    },
    Controller {
        time: u64,
        cc: u8,
        value: i32,
    },
    SetEnginePar {
        par: String,
        group: i32,
        slot: i32,
        generic: i32,
        value: i32,
    },
}

/// Test and CLI engine: records calls with absolute sample times, stores engine
/// parameters, and knows only group names and modulator names.
#[derive(Debug, Default)]
pub struct LogEngine {
    pub groups: Vec<String>,
    pub zones: usize,
    /// Per group: modulator names with their target names, in `find_mod` order.
    pub modulators: Vec<Vec<(String, Vec<String>)>>,
    /// The instrument file, for scripts that read files near it.
    pub instrument: Option<std::path::PathBuf>,
    pub calls: Vec<EngineCall>,
    pub block_start: u64,
    pub rate: f64,
    pars: std::collections::BTreeMap<EnginePar, i32>,
    next_voice: EventId,
}

impl LogEngine {
    pub fn new(groups: Vec<String>, rate: f64) -> Self {
        Self {
            groups,
            zones: 0,
            rate,
            ..Self::default()
        }
    }

    fn time(&self, at: u32) -> u64 {
        self.block_start + u64::from(at)
    }
}

impl KspEngine for LogEngine {
    fn play_note(&mut self, at: u32, n: &NoteSpec<'_>) -> Option<EventId> {
        self.next_voice.0 += 1;
        let call = EngineCall::PlayNote {
            time: self.time(at),
            voice: self.next_voice,
            event: n.event,
            channel: n.channel,
            note: n.note,
            velocity: n.velocity,
            sample_offset_us: n.sample_offset_us,
            length: n.length,
            volume_mdb: n.volume_mdb,
            tune_mc: n.tune_mc,
            pan: n.pan,
            groups: n.groups.iter(self.groups.len()).collect(),
        };
        self.calls.push(call);
        Some(self.next_voice)
    }

    fn note_off(&mut self, at: u32, voice: EventId, _event: &NoteSpec<'_>) {
        self.calls.push(EngineCall::NoteOff {
            time: self.time(at),
            voice,
        });
    }

    fn fade(&mut self, at: u32, voice: EventId, fade: Fade) {
        self.calls.push(EngineCall::Fade {
            time: self.time(at),
            voice,
            fade,
        });
    }

    fn set_par(&mut self, at: u32, voice: EventId, par: VoicePar, value: i32) {
        self.calls.push(EngineCall::SetPar {
            time: self.time(at),
            voice,
            par,
            value,
        });
    }

    fn controller(&mut self, at: u32, cc: u8, value: i32) {
        self.calls.push(EngineCall::Controller {
            time: self.time(at),
            cc,
            value,
        });
    }

    fn group_count(&self) -> usize {
        self.groups.len()
    }

    fn zone_count(&self) -> usize { self.zones }

    fn group_name(&self, group: usize) -> &str {
        self.groups.get(group).map_or("", String::as_str)
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn set_engine_par(&mut self, _at: u32, par: EnginePar, value: i32) -> bool {
        let mut name = String::new();
        match engine_par_name(par.id) {
            Some(n) => name.push_str(n),
            None => {
                let _ = write!(name, "#{}", par.id);
            }
        }
        self.calls.push(EngineCall::SetEnginePar {
            par: name,
            group: par.group,
            slot: par.slot,
            generic: par.generic,
            value,
        });
        self.pars.insert(par, value);
        true
    }

    fn engine_par(&self, par: EnginePar) -> Option<i32> {
        self.pars.get(&par).copied()
    }

    fn instrument_path(&self) -> Option<&std::path::Path> {
        self.instrument.as_deref()
    }

    fn find_mod(&self, group: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        self.modulators.get(group)?.iter().position(|m| is(&m.0))
    }

    fn find_target(&self, group: usize, modulator: usize, is: &dyn Fn(&str) -> bool) -> Option<usize> {
        let targets = &self.modulators.get(group)?.get(modulator)?.1;
        targets.iter().position(|t| is(t))
    }
}
