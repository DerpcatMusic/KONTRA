//! The boundary between the KSP runtime and the sampler engine. Every call is made
//! on the audio thread at a frame offset within the current block.

pub use crate::engine::{EventId, GroupMask};
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
    /// Explicit zone from `$EVENT_PAR_ZONE_ID`, or -1 for normal mapping.
    pub zone: i32,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EnginePar {
    pub id: i32,
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
}

pub trait KspEngine {
    fn play_note(&mut self, at: u32, note: &NoteSpec<'_>) -> Option<EventId>;
    /// Release a voice; release triggers may start in `groups`.
    fn note_off(&mut self, at: u32, voice: EventId, groups: &GroupMask);
    fn fade(&mut self, at: u32, voice: EventId, fade: Fade);
    fn set_par(&mut self, at: u32, voice: EventId, par: VoicePar, value: i32);
    /// A controller that passed every slot: 0..127, 128 pitch bend (-8192..8191),
    /// 129 channel pressure.
    fn controller(&mut self, at: u32, cc: u8, value: i32);
    fn group_count(&self) -> usize;
    fn group_name(&self, group: usize) -> &str;
    fn sample_rate(&self) -> f64;
    /// Returns false when the engine does not implement the parameter; the runtime
    /// then stores the value itself and reports it.
    fn set_engine_par(&mut self, par: EnginePar, value: i32) -> bool;
    fn engine_par(&self, par: EnginePar) -> Option<i32>;
    /// Index of a named modulator within a group, if the engine knows it.
    fn find_mod(&self, _group: usize, _name: &str) -> Option<usize> {
        None
    }
    /// Index of a named modulation target, if the engine knows it.
    fn find_target(&self, _group: usize, _modulator: usize, _name: &str) -> Option<usize> {
        None
    }
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
/// parameters, and knows only group names.
#[derive(Debug, Default)]
pub struct LogEngine {
    pub groups: Vec<String>,
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

    fn note_off(&mut self, at: u32, voice: EventId, _groups: &GroupMask) {
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

    fn group_name(&self, group: usize) -> &str {
        self.groups.get(group).map_or("", String::as_str)
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn set_engine_par(&mut self, par: EnginePar, value: i32) -> bool {
        let mut name = String::new();
        match super::builtins::engine_par_name(par.id) {
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
}
