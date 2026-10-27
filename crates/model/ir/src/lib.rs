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
    /// Shared voice limits; [`Group::voice_group`] indexes here.
    pub voice_groups: Vec<VoiceGroup>,
    /// Preloaded bytes per channel of each resident zone (05-STREAM.md §1).
    pub preload: u32,
    pub diagnostics: Vec<Diagnostic>,
}

impl Instrument {
    /// Nothing yet, with Kontakt's voice and preload defaults.
    pub const DEFAULT: Self = Self {
        name: String::new(),
        samples: Vec::new(),
        groups: Vec::new(),
        buses: Vec::new(),
        scripts: Vec::new(),
        voices: 64,
        voice_groups: Vec::new(),
        preload: 61_440,
        diagnostics: Vec::new(),
    };
}

/// One sample: where its file's bytes are. The codec is read from the file's
/// own header when the engine opens it; frames, rate and channels are what
/// the instrument declares.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub name: String,
    pub file: PathBuf,
    /// Where the sample file starts in `file` (members of monoliths and archives).
    pub offset: u64,
    /// Bytes of the sample file from `offset`, header included; `None` to the
    /// end of `file`.
    pub len: Option<u64>,
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
        // Whole runs of the key at a time, so the XOR vectorises.
        let mut position = (at.wrapping_sub(self.origin) % len) as usize;
        let mut rest = bytes;
        while !rest.is_empty() {
            let key = self.stream.get(position..).unwrap_or_default();
            let n = key.len().min(rest.len());
            let (head, tail) = std::mem::take(&mut rest).split_at_mut(n);
            head.iter_mut().zip(key).for_each(|(b, k)| *b ^= k);
            rest = tail;
            position = 0;
        }
    }
}

impl std::fmt::Debug for Keystream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keystream(..)")
    }
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
    /// The amplitude envelope.
    pub envelope: Envelope,
    /// Modulation of this group's voices; none: velocity leaves the level alone.
    pub mods: Vec<Mod>,
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
    /// Index into [`Instrument::voice_groups`].
    pub voice_group: Option<usize>,
    pub zones: Vec<Zone>,
}

impl Group {
    pub const DEFAULT: Self = Self {
        name: String::new(),
        volume_db: 0.0,
        pan: 0.0,
        tune: 0.0,
        keytrack: 1.0,
        envelope: Envelope::GATE,
        mods: Vec::new(),
        trigger: Trigger::Attack,
        release_decay_db: 0.0,
        one_shot: false,
        start: Vec::new(),
        polyphony: None,
        off_by: None,
        bus: 0,
        purged: false,
        voice_group: None,
        zones: Vec::new(),
    };
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

impl Envelope {
    /// Full level while held, a short release.
    pub const GATE: Self = Self {
        attack: 0.0,
        hold: 0.0,
        decay: 0.0,
        sustain: 1.0,
        release: 0.001_3,
    };
}

/// One modulation of a group. Scripts find it by `name` and address it by
/// `slot`, the number the format gives it (Kontakt: its modulator slot).
#[derive(Clone, Debug, PartialEq)]
pub struct Mod {
    pub name: String,
    pub slot: u16,
    pub kind: ModKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModKind {
    /// The group's [`Group::envelope`], shaping its level.
    Envelope,
    /// `source` scales the level along `curve`; `intensity` is −1..=1 and live.
    Volume {
        source: Source,
        intensity: f32,
        curve: Curve,
    },
}

/// A per-note modulation source, 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Velocity,
    Controller(u8),
}

/// How a volume modulation turns its source into gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// Linear in dB: `intensity ×` this many dB down at source 0.
    Decibels(f32),
    /// Kontakt's volume law (its "family H"): `1 − (1 − x) × intensity`,
    /// cubed for velocity, key and the other note-time sources (external
    /// types 7..=13), plain for controllers. Kontakt 8.13.1's modulation
    /// compiler `0x140875ac8`/`0x140873120`; the cube at 50/127 matches the
    /// measured velocity table to 0.03 dB RMS.
    Kontakt { cubed: bool },
}

impl Curve {
    /// Linear gain for source value `x` (0..=1) at `intensity`.
    #[must_use]
    pub fn gain(self, intensity: f32, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Decibels(range) => 10f32.powf(-range * intensity * (1.0 - x) / 20.0),
            Self::Kontakt { cubed } => {
                let g = (1.0 - (1.0 - x) * intensity).max(0.0);
                if cubed { g * g * g } else { g }
            }
        }
    }
}

/// A voice limit shared by the groups that name it (Kontakt's voice groups).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceGroup {
    pub voices: u16,
    pub steal: Steal,
    /// Steal released voices before held ones.
    pub prefer_released: bool,
    /// Fade of a stolen or excluded voice.
    pub fade_ms: f32,
    /// Starting a voice fades every voice of the other voice groups in this set.
    pub exclusive: Option<u8>,
}

/// Which voice a full voice group gives up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steal {
    Oldest,
    Newest,
    Highest,
    Lowest,
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
    /// below its high edge (a linear amplitude ramp, Kontakt's measured law).
    pub velocity_fade: (u8, u8),
    /// The same over keys.
    pub key_fade: (u8, u8),
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
    /// Frames before `end` that fade linearly into the frames before `start`.
    pub crossfade: u64,
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
