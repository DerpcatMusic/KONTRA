//! The instrument model: what a format reads from a vendor's files and what
//! engine prepare builds a plan from (06-FORMATS.md). Data only, load time
//! only. Everything a translator could not carry over is a [`Diagnostic`].

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Instrument {
    pub name: String,
    pub samples: Vec<Sample>,
    pub groups: Vec<Group>,
    /// Output buses; group `bus` indexes here. Empty means one main bus.
    pub buses: Vec<Bus>,
    pub scripts: Vec<Script>,
    /// Voices the instrument may sound at once.
    pub voices: u16,
    /// Preloaded bytes per channel of each resident zone (05-STREAM.md §1).
    pub preload: u32,
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            name: String::new(),
            samples: Vec::new(),
            groups: Vec::new(),
            buses: Vec::new(),
            scripts: Vec::new(),
            voices: 64,
            preload: 61_440,
            diagnostics: Vec::new(),
        }
    }
}

/// One sample's audio, where its bytes are and how they are coded.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub name: String,
    pub file: PathBuf,
    /// Where the sample's data starts in `file` (members of monoliths).
    pub offset: u64,
    /// Bytes of coded data from `offset`.
    pub len: u64,
    pub codec: Codec,
    pub channels: u8,
    pub frames: u64,
    pub rate: u32,
    /// Set when the bytes on disk are encrypted.
    pub keystream: Option<Keystream>,
}

/// A repeating XOR keystream: file byte `origin + n` is stored XOR `stream[n % len]`.
/// Held in memory only; `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct Keystream {
    pub stream: Arc<[u8]>,
    pub origin: u64,
}

impl Keystream {
    /// Decrypts (or encrypts) `bytes` read from file offset `at`.
    pub fn apply(&self, at: u64, bytes: &mut [u8]) {
        let len = self.stream.len() as u64;
        if len == 0 {
            return;
        }
        let mut position = at.wrapping_sub(self.origin) % len;
        for byte in bytes {
            *byte ^= self.stream.get(position as usize).copied().unwrap_or(0);
            position = (position + 1) % len;
        }
    }
}

impl std::fmt::Debug for Keystream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keystream(..)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// Interleaved PCM from the start of the data.
    Pcm {
        bits: u8,
        float: bool,
        big_endian: bool,
    },
    /// Native Instruments' compressed wave.
    Ncw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    pub volume_db: f32,
    /// -1..=1.
    pub pan: f32,
    pub tune: f64,
    /// Pitch follows the key by this much (1 = a semitone per key).
    pub keytrack: f64,
    pub envelope: Envelope,
    pub velocity: Velocity,
    pub trigger: Trigger,
    /// Release triggers: dB lost per second the key was held.
    pub release_decay_db: f32,
    /// Ignores note-off and plays to the end.
    pub one_shot: bool,
    pub start: Vec<StartRule>,
    /// Voices of this group that may sound at once.
    pub polyphony: Option<u16>,
    /// Voices of this group fade when a voice of that group starts (hi-hat choke).
    pub off_by: Option<usize>,
    pub bus: usize,
    /// Saved unloaded: no preload until a script loads it.
    pub purged: bool,
    pub zones: Vec<Zone>,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            name: String::new(),
            volume_db: 0.0,
            pan: 0.0,
            tune: 0.0,
            keytrack: 1.0,
            envelope: Envelope::default(),
            velocity: Velocity::Decibels(24.0),
            trigger: Trigger::Attack,
            release_decay_db: 0.0,
            one_shot: false,
            start: Vec::new(),
            polyphony: None,
            off_by: None,
            bus: 0,
            purged: false,
            zones: Vec::new(),
        }
    }
}

/// Times in seconds; decay and release to fall about 60 dB; sustain 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for Envelope {
    /// A gate: full level while held, a short release.
    fn default() -> Self {
        Self {
            attack: 0.0,
            hold: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.001_3,
        }
    }
}

/// How note velocity scales the level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Velocity {
    Flat,
    /// Linear in dB: this many dB down at velocity 0.
    Decibels(f32),
    /// `1 − depth + depth × velocity^exponent`.
    Power {
        depth: f32,
        exponent: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    #[default]
    Attack,
    Release,
    First,
    Legato,
}

/// A condition every one of which must hold for a group to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartRule {
    /// The last key pressed in this range was `key`.
    Keyswitch { key: u8, range: RangeInclusive<u8> },
    /// The controller's value is in range.
    Controller { cc: u8, range: RangeInclusive<u8> },
    /// Step `position` of a cycle of `length`, counted per instrument.
    RoundRobin {
        position: u8,
        length: u8,
        random: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// Index into [`Instrument::samples`].
    pub sample: usize,
    pub keys: RangeInclusive<u8>,
    pub velocities: RangeInclusive<u8>,
    /// Velocity steps over which the zone fades in above its low edge and out
    /// below its high edge (equal power).
    pub velocity_fade: (u8, u8),
    pub root: u8,
    pub tune: f64,
    pub volume_db: f32,
    pub pan: f32,
    /// Played frames of the sample.
    pub start: u64,
    pub end: u64,
    /// Frames a start offset may move the start (Kontakt's S.Mod).
    pub start_mod: u64,
    pub looped: Option<Loop>,
    pub reverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loop {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub volume_db: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    pub name: String,
    pub dialect: Dialect,
    pub source: String,
    /// Saved persistent values in the dialect's own form.
    pub saved: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Ksp,
    Lua,
}

/// Something the translator could not carry over, and how much it matters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub impact: Impact,
    /// The feature, stable across files, for ranking gaps corpus-wide.
    pub feature: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Impact {
    /// Nothing audible.
    Cosmetic,
    /// Sound differs (a missing effect, a wrong curve).
    Audible,
    /// Notes are missing or wrong.
    Broken,
}
