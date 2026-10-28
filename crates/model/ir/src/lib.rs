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
    /// Effects on everything the instrument plays, by slot.
    pub inserts: Vec<Option<Effect>>,
    /// Parallel effects fed by [`EffectKind::Sends`], by send slot; their
    /// output joins the inserts' before [`Self::main`].
    pub sends: Vec<Option<Effect>>,
    /// Effects after the inserts and sends, by slot.
    pub main: Vec<Option<Effect>>,
    /// Responses [`EffectKind::Convolution`] slots play.
    pub impulses: Vec<Impulse>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Instrument {
    /// Nothing yet.
    pub const DEFAULT: Self = Self {
        name: String::new(),
        samples: Vec::new(),
        groups: Vec::new(),
        buses: Vec::new(),
        scripts: Vec::new(),
        voices: 64,
        voice_groups: Vec::new(),
        inserts: Vec::new(),
        sends: Vec::new(),
        main: Vec::new(),
        impulses: Vec::new(),
        diagnostics: Vec::new(),
    };
}

/// An impulse response: a sample and how to prepare it, in this order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impulse {
    /// Index into [`Instrument::samples`].
    pub sample: usize,
    pub reverse: bool,
    pub normalize: Option<Normalize>,
    /// Silence before the response, truncated to whole frames.
    pub predelay_ms: f32,
}

/// Scales a response so its louder channel's energy (sum of squares) is
/// `energy`, by at most `max_gain`; one at or under `floor` is left alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normalize {
    pub energy: f32,
    pub max_gain: f32,
    pub floor: f32,
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
    /// Start rules folded left to right by their joins; empty always starts.
    pub start: Vec<(Join, StartRule)>,
    /// Voices of this group that may sound at once.
    pub polyphony: Option<u16>,
    /// Voices of this group fade when a voice of that group starts (hi-hat choke).
    pub off_by: Option<usize>,
    pub bus: usize,
    /// Saved unloaded: no preload until a script loads it.
    pub purged: bool,
    /// How much of each zone's sample stays in memory; `None` keeps it whole.
    pub stream: Option<Stream>,
    /// Index into [`Instrument::voice_groups`].
    pub voice_group: Option<usize>,
    /// Effects on this group's voices, by slot.
    pub inserts: Vec<Option<Effect>>,
    /// Shapes [`Source::Envelope`] reads, by index.
    pub envelopes: Vec<Breakpoints>,
    /// Oscillators [`Source::Lfo`] reads, by index.
    pub lfos: Vec<Lfo>,
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
        stream: None,
        voice_group: None,
        inserts: Vec::new(),
        envelopes: Vec::new(),
        lfos: Vec::new(),
        zones: Vec::new(),
    };
}

/// A per-voice envelope of free breakpoints, 0..=1. It starts at `start`,
/// moves through `points` in order and holds at `sustain` while the key is
/// down; key-up goes on from the level it has to the points after `sustain`.
#[derive(Clone, Debug, PartialEq)]
pub struct Breakpoints {
    pub start: f32,
    pub points: Vec<Breakpoint>,
    pub sustain: Option<usize>,
}

/// A segment to `level` over `ms`. It covers `(e^(k·t) − 1)/(e^k − 1)` of
/// the way at fraction `t`, `k` = `curve`; 0 is a straight line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breakpoint {
    pub ms: f32,
    pub level: f32,
    pub curve: f32,
}

/// A per-voice (or free-running) oscillator, −1..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub wave: Wave,
    pub hz: f32,
    /// Start phase, cycles.
    pub phase: f32,
    /// Ramps the output in from 0 over this long after the voice starts.
    pub fade_ms: f32,
    /// Restarts at each voice; else runs from the engine's start.
    pub retrigger: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Sine,
    Triangle,
    /// Falling.
    Saw,
    /// High for `width` of the cycle.
    Square {
        width: f32,
    },
    /// A new random level each cycle.
    Random,
}

/// Times in seconds; decay and release to fall about 60 dB; sustain 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// Attack shape k of `expm1(k·t)/expm1(k)`: 0 is linear, positive starts slowly.
    pub attack_shape: f32,
    /// Decay and release shape, the same law.
    pub fall_shape: f32,
}

impl Envelope {
    /// Full level while held, a short release.
    pub const GATE: Self = Self {
        attack: 0.0,
        hold: 0.0,
        decay: 0.0,
        sustain: 1.0,
        release: 0.001_3,
        attack_shape: 0.0,
        fall_shape: 0.0,
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
    /// `source` times `semitones` added to the pitch.
    Pitch { source: Source, semitones: f32 },
    /// `source × amount` added to the pan (−1..=1).
    Pan { source: Source, amount: f32 },
    /// `source × depth` of the zone's [`Zone::start_mod`] frames added to
    /// where its voices start, read at note-on.
    SampleStart { source: Source, depth: f32 },
}

/// A per-note modulation source, 0..=1 unless noted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Velocity,
    Controller(u8),
    /// The channel's pitch bend, −1..=1.
    PitchBend,
    /// The note's key over 127.
    Key,
    /// The group's [`Group::envelopes`] entry.
    Envelope(u8),
    /// The group's [`Group::lfos`] entry, −1..=1.
    Lfo(u8),
    /// Always 1.
    Constant,
    /// 1 at note-on, falling linearly to 0 after `span_ms` held; frozen at
    /// key-up, so a release sample is quieter the longer its key was held.
    HeldTime {
        span_ms: u32,
    },
}

/// How a volume modulation turns its source into gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// Linear in dB: `intensity ×` this many dB down at source 0.
    Decibels(f32),
    /// `(1 − (1 − x) × intensity)^power`: the source pulls the gain down from
    /// unity by its depth; power 1 is linear in amplitude.
    Depth { power: u8 },
}

impl Curve {
    /// Linear gain for source value `x` (0..=1) at `intensity`.
    #[must_use]
    pub fn gain(self, intensity: f32, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Decibels(range) => 10f32.powf(-range * intensity * (1.0 - x) / 20.0),
            Self::Depth { power } => (1.0 - (1.0 - x) * intensity)
                .max(0.0)
                .powi(i32::from(power)),
        }
    }
}

/// A streamed zone's resident head: `bytes_per_channel` of stored audio
/// from the start, plus twice the start-offset range, rounded up to `grain`
/// bytes. The rest is read from disk while the voice plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stream {
    pub bytes_per_channel: u32,
    pub grain: u32,
}

/// A voice limit shared by the groups that name it.
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

/// How a start rule joins the rules before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Join {
    #[default]
    And,
    AndNot,
    Or,
}

/// A condition on which a group starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartRule {
    /// A latch over every switch range of the instrument: a key inside their
    /// span sets the ranges that contain it and clears the rest; a key outside
    /// the span keeps them. No key is silenced by it.
    Keyswitch { range: RangeInclusive<u8> },
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

/// An effect slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effect {
    pub kind: EffectKind,
    pub bypass: bool,
    /// Linear gain on the slot's output.
    pub output: f32,
    /// Linear level of the unprocessed signal mixed back in.
    pub dry: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectKind {
    /// Linear gain.
    Gain(f32),
    /// Polarity flip, then left and right swapped.
    Invert { phase: bool, swap: bool },
    /// Width 0..=1 (0 mono, 0.5 unchanged, 1 doubled side), then a -1..=1 balance.
    Stereo { width: f32, pan: f32 },
    /// Peaking bands in series; a 0 dB band is flat.
    Eq([Bell; 3]),
    /// Copies its input to the instrument's send slot n at `levels[n]`
    /// (linear) and passes it on unchanged.
    Sends([f32; 8]),
    /// The input convolved with [`Instrument::impulses`] at this index (the
    /// wet path; `dry` mixes the input back).
    Convolution { impulse: u16 },
    /// Not carried, an insert: audio passes unchanged (its output gain
    /// trims processing that does not run).
    /// The translator reports it.
    Other,
    /// Not carried, a parallel path (a reverb, a delay): only the dry level
    /// passes. The translator reports it.
    OtherWet,
}

/// A peaking band: `gain_db` at `hz`, width by `q` (the cookbook bell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bell {
    pub hz: f32,
    pub q: f32,
    pub gain_db: f32,
}

impl Bell {
    pub const FLAT: Self = Self {
        hz: 1000.0,
        q: 1.0,
        gain_db: 0.0,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub volume_db: f32,
    /// -1..=1 balance.
    pub pan: f32,
    /// Effects on what the bus carries, by slot.
    pub inserts: Vec<Option<Effect>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    pub name: String,
    pub dialect: Dialect,
    pub source: String,
    /// Saved persistent values in the dialect's own form.
    pub saved: Vec<String>,
    /// Library data files the script loads (Kontakt `load_array(%x, 2)`:
    /// `data/x.nka`), by lowercase array name without its sigil.
    pub data: Vec<(String, Vec<u8>)>,
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
