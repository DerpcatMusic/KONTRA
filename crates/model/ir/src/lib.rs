//! The instrument model: what a format reads from a vendor's files and what
//! engine prepare builds a plan from (06-FORMATS.md). Data only, load time
//! only. Everything a translator could not carry over is a [`Diagnostic`].

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

mod bus;
mod drive;
mod effects;
mod link;
mod mods;
mod output;
mod readout;
mod reverb;
mod route_names;
mod wavetable;
pub use bus::{Bus, BusSend};
pub use drive::{
    Clipper, Crush, Curve as DriveCurve, EffectParam, KnobTarget, Overdrive, RackAt, Shaper,
    SlotKnob, SlotLaw, SlotShape, SlotShapes, SlotType,
};
pub use effects::{
    Cascade, CascadeStage, Clocked, ClockedShape, Compressor, Delay, DelayTime, Detector, Formant,
    IDENTITY, ImpulseCuts, ImpulseKnob, Limiter, Matrix, Phaser, Reverb, Transient, VowelSet,
};
pub use kontra_law::{FormulaId, common};
pub use link::{CcLink, Linked};
pub use mods::{Attractor, Blend, Curve, Exp2, Factor, ModKind, Term, Transfer, Wave};
pub use output::{Balance, Output, OutputControllers};
pub use readout::{Form, Readout};
pub use reverb::{
    CombNetwork, HadamardNetwork, Law, NETWORK_LINES, Network, ReverbNetwork, ReverbPar,
};
pub use route_names::RouteNames;
pub use wavetable::{FormShaper, Side, Warp, WarpShape, Wavetable};

#[derive(Clone, Debug)]
pub struct Instrument {
    pub name: String,
    /// Source file, when this instrument was loaded from a path.
    pub source_path: Option<std::path::PathBuf>,
    /// Library root discovered when loading, when known.
    pub library_path: Option<std::path::PathBuf>,
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
    /// The fader and pan after every effect.
    pub output: Output,
    /// How the pitch routes' semitones become a playback ratio.
    pub route_pitch: Exp2,
    /// Whether a voice stolen at [`Self::voices`] takes its note's other
    /// voices in the same group with it: a group's zones sound as one voice.
    pub steal_whole_groups: bool,
    /// Responses [`EffectKind::Convolution`] slots play.
    pub impulses: Vec<Impulse>,
    /// Responses a script may load into a convolution slot by name
    /// (lowercase, without its extension), by index into [`Self::samples`].
    pub responses: Vec<(String, usize)>,
    /// How [`KnobTarget::Slot`] knobs move the effects they address.
    pub slot_laws: Vec<SlotLaw>,
    /// The filter shapes a script may switch slots to.
    pub slot_shapes: SlotShapes,
    pub diagnostics: Vec<Diagnostic>,
}

impl Instrument {
    /// Nothing yet.
    pub const DEFAULT: Self = Self {
        name: String::new(),
        source_path: None,
        library_path: None,
        samples: Vec::new(),
        groups: Vec::new(),
        buses: Vec::new(),
        scripts: Vec::new(),
        voices: 64,
        voice_groups: Vec::new(),
        inserts: Vec::new(),
        sends: Vec::new(),
        main: Vec::new(),
        output: Output::UNITY,
        route_pitch: Exp2::Exact,
        steal_whole_groups: false,
        impulses: Vec::new(),
        responses: Vec::new(),
        slot_laws: Vec::new(),
        slot_shapes: SlotShapes::NONE,
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
    pub cuts: Option<ImpulseCuts>,
    /// Eight knots `[time fraction, dB]`, interpolated in amplitude.
    pub envelope: Option<[[f32; 2]; 8]>,
    /// The new left and right channels as `[from left, from right]`, after
    /// the envelope and before normalizing.
    pub mix: Option<[[f32; 2]; 2]>,
}

/// Scales a response's channel energy toward `energy`, by at most `max_gain`;
/// energy at or under `floor` is left alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normalize {
    pub energy: f32,
    pub max_gain: f32,
    pub floor: f32,
    pub channels: EnergyChannels,
    /// Apply after converting the response to the host rate.
    pub after_resample: bool,
}

/// How stereo channel energies are combined when normalizing an impulse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnergyChannels {
    Loudest,
    Mean,
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

/// XOR keying of bytes a sample file stores encrypted: decrypts (or encrypts)
/// the file's bytes as the engine reads them. Held in memory only; `Debug`
/// never prints the key.
#[derive(Clone, PartialEq, Eq)]
pub enum Keystream {
    /// File byte `origin + n` is stored XOR `stream[n % len]`.
    Repeat { stream: Arc<[u8]>, origin: u64 },
    /// Each `block` bytes from `origin` are XORed with a stream of 32-bit
    /// little-endian words. The stream restarts at every block, seeded by `key`
    /// and the block's file offset, so any read decrypts without its neighbours.
    Blocks { key: u64, origin: u64, block: u32 },
}

impl Keystream {
    /// Decrypts (or encrypts) `bytes` read from file offset `at`.
    pub fn apply(&self, at: u64, bytes: &mut [u8]) {
        match self {
            Self::Repeat { stream, origin } => repeat(stream, *origin, at, bytes),
            Self::Blocks { key, origin, block } => {
                blocks(*key, *origin, u64::from(*block), at, bytes);
            }
        }
    }
}

fn repeat(stream: &[u8], origin: u64, at: u64, bytes: &mut [u8]) {
    let len = stream.len() as u64;
    if len == 0 {
        return;
    }
    // Whole runs of the key at a time, so the XOR vectorises.
    let mut position = (at.wrapping_sub(origin) % len) as usize;
    let mut rest = bytes;
    while !rest.is_empty() {
        let key = stream.get(position..).unwrap_or_default();
        let n = key.len().min(rest.len());
        let (head, tail) = std::mem::take(&mut rest).split_at_mut(n);
        head.iter_mut().zip(key).for_each(|(b, k)| *b ^= k);
        rest = tail;
        position = 0;
    }
}

/// The block-restarted word stream: block `n` of `origin` starts at
/// `origin + n·block`; the stream is the one a whole-block pass would XOR.
fn blocks(key: u64, origin: u64, block: u64, at: u64, bytes: &mut [u8]) {
    if block == 0 {
        return;
    }
    let Some(mut into) = at.checked_sub(origin) else {
        return;
    };
    let mut rest = bytes;
    while !rest.is_empty() {
        let start = into / block * block;
        let skip = (into - start) as usize;
        let n = (block as usize - skip).min(rest.len());
        let (head, tail) = std::mem::take(&mut rest).split_at_mut(n);
        let mask = word_stream(key, origin + start, skip + n);
        head.iter_mut()
            .zip(mask.get(skip..).unwrap_or_default())
            .for_each(|(b, m)| *b ^= m);
        rest = tail;
        into += n as u64;
    }
}

/// The first `len` bytes of the word stream seeded by `key` and `nonce`, a
/// multiplicative state stepped per word (uvi-file's `transform`; uvi tests it).
fn word_stream(key: u64, nonce: u64, len: usize) -> Vec<u8> {
    const MIX: u64 = 0xc6a4_a793_5bd1_e995;
    const STEP: u64 = 0x5851_f42d_4c95_7f2d;
    let mix = |value: u64| {
        let value = value.wrapping_mul(MIX);
        (value ^ (value >> 47)).wrapping_mul(MIX)
    };
    let mut state = (mix(nonce) ^ key).wrapping_mul(MIX);
    let mut out = Vec::with_capacity(len + 4);
    while out.len() < len {
        let word = (((state >> 22) ^ state) >> (22 + (state >> 61))) as u32;
        out.extend(word.to_le_bytes());
        state = state.wrapping_mul(STEP);
    }
    out.truncate(len);
    out
}

impl std::fmt::Debug for Keystream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keystream(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_keystream_reads_in_any_piece_and_undoes_itself() {
        let plain: Vec<u8> = (0..1500u32).map(|i| (i * 7 + 3) as u8).collect();
        let key = Keystream::Blocks {
            key: 0x1234_5678_9abc_def0,
            origin: 4096,
            block: 512,
        };
        let mut whole = plain.clone();
        key.apply(4096, &mut whole);
        assert_ne!(whole, plain);
        // A read that starts mid-block and crosses two block starts decrypts the
        // same bytes as the whole pass.
        let mut piece = whole[300..1100].to_vec();
        key.apply(4096 + 300, &mut piece);
        assert_eq!(piece, plain[300..1100]);
        key.apply(4096, &mut whole);
        assert_eq!(whole, plain);
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
    /// Its zones are wavetables.
    pub wavetable: Option<Wavetable>,
    /// Index into [`Instrument::voice_groups`].
    pub voice_group: Option<usize>,
    /// Effects on this group's voices, by slot.
    pub inserts: Vec<Option<Effect>>,
    /// Shapes [`Source::Envelope`] reads, by index.
    pub envelopes: Vec<Breakpoints>,
    /// Oscillators [`Source::Lfo`] reads, by index.
    pub lfos: Vec<Lfo>,
    /// Transfer tables [`Source::Shaped`] reads, by index.
    pub shapers: Vec<Transfer>,
    /// Sums [`Source::Blend`] reads, by index.
    pub blends: Vec<Blend>,
    /// Each modulator's route names, in route order, carried or not: what
    /// scripts find routes by.
    pub routes: Vec<Routes>,
    pub zones: Vec<Zone>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Routes {
    /// The modulator's [`Mod::slot`]: one address per group, unique.
    pub slot: u16,
    /// The modulator's name, what scripts find it by.
    pub name: String,
    pub names: Vec<String>,
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
        wavetable: None,
        voice_group: None,
        inserts: Vec::new(),
        envelopes: Vec::new(),
        lfos: Vec::new(),
        shapers: Vec::new(),
        blends: Vec::new(),
        routes: Vec::new(),
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
    /// Instead of holding at `sustain`, cycle back through the points from
    /// this one to `sustain` while the key is down.
    pub loop_from: Option<usize>,
    /// Its four points are attack, hold, decay to `sustain` and release, so
    /// scripts may set them by stage.
    pub stages: bool,
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
    pub rate: Rate,
    /// Start phase, cycles.
    pub phase: f32,
    /// Ramps the output in from 0 over this long after the voice starts.
    pub fade_ms: f32,
    pub clock: Clock,
}

/// Where an [`Lfo`]'s time starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    /// Restarts at each voice.
    Voice,
    /// Runs from the engine's start, shared by every voice.
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rate {
    Hz(f32),
    /// Quarter-note beats per cycle, at the host's tempo.
    Beats(f32),
    /// `count` steps of `beats` quarter-note beats per cycle: a note value
    /// and how many of it. A knob moves the count, whole and at least 1.
    Steps {
        beats: f32,
        count: f32,
    },
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
    /// Which of its modulator's routes this is: scripts address a route by
    /// modulator slot and target.
    pub target: u16,
    /// Its modulator is switched off (scripts may switch it on).
    pub bypass: bool,
    pub kind: ModKind,
}

/// A per-note modulation source, 0..=1 unless noted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Velocity,
    Controller(u8),
    /// Controller `cc`, reading `start` (0..=127) until its first message
    /// on the channel arrives.
    ControllerFrom {
        cc: u8,
        start: u8,
    },
    /// The channel's pitch bend, −1..=1.
    PitchBend,
    /// The note's key over 127.
    Key,
    /// The group's [`Group::envelopes`] entry.
    Envelope(u8),
    /// The group's [`Group::shapers`] entry: its source through its table.
    Shaped(u8),
    /// The group's [`Group::lfos`] entry, −1..=1.
    Lfo(u8),
    /// The group's [`Group::blends`] entry.
    Blend(u8),
    /// Always 1.
    Constant,
    /// The value a script sent this note for "from script" modulator `id`,
    /// −1..=1; 0 until sent.
    Script(u16),
    /// 1 at note-on, falling linearly to 0 after `span_ms` held; frozen at
    /// key-up, so a release sample is quieter the longer its key was held.
    HeldTime {
        span_ms: u32,
    },
}

impl Source {
    /// The controller it reads, if any.
    #[must_use]
    pub fn controller(self) -> Option<u8> {
        match self {
            Self::Controller(cc) | Self::ControllerFrom { cc, .. } => Some(cc),
            _ => None,
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
    /// One zone slot of the group per note, in turn or at random (no repeat), per group.
    Cycle { random: bool },
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
    /// Loops in play order. Each is entered when the playhead reaches its end.
    pub loops: Vec<Loop>,
    pub reverse: bool,
    /// Each sample channel's left and right gain; empty plays mono on both
    /// sides and the first two channels as left and right.
    pub mix: Vec<[f32; 2]>,
    /// The zone's place among the zones of its group that share its keys; a
    /// note that names a slot starts only the zones in it.
    pub slot: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loop {
    pub start: u64,
    pub end: u64,
    /// Frames before `end` that fade linearly into the frames before `start`.
    pub crossfade: u64,
    /// Repeats after the first pass; 0 repeats forever (or until release).
    pub count: u32,
    /// Each repeat plays the loop backwards, then forwards again.
    pub alternating: bool,
    /// Repeats until the note is released, then plays on past `end`.
    pub until_release: bool,
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
    /// Each output side from both inputs, `[[ll, lr], [rl, rr]]`: left out
    /// is `ll·left + lr·right`.
    Matrix([[f32; 2]; 2]),
    /// Bands in series; a 0 dB band is flat.
    Eq([Bell; 4]),
    /// A state-variable filter.
    Filter(Filter),
    /// A vowel formant filter.
    Formant(Formant),
    /// Three morphing filter stages in series.
    Cascade(Cascade),
    /// A filter on a control clock.
    Clocked(Clocked),
    /// A waveshaper.
    Shaper(Shaper),
    /// A sample-rate and bit-depth reducer.
    Crush(Crush),
    /// An overdrive pedal.
    Overdrive(Overdrive),
    /// A diode clipper circuit (the wet path; `dry` mixes the input back).
    Clipper(Clipper),
    /// Copies its input to send slot n at `levels[n]` (linear), passing it on.
    Sends([f32; 8]),
    /// The input convolved with [`Instrument::impulses`] at this index (wet).
    Convolution { impulse: u16 },
    /// An algorithmic reverb (the wet path; `dry` mixes the input back).
    Reverb(Reverb),
    /// A compressor, in place.
    Compressor(Compressor),
    /// A peak limiter, in place.
    Limiter(Limiter),
    /// An echo, chorus or flanger (the wet path; `dry` mixes the input back).
    Delay(Delay),
    /// A phaser (the wet path; `dry` mixes the input back).
    Phaser(Phaser),
    /// A transient shaper, in place.
    Transient(Transient),
    /// Not carried, an insert: audio passes unchanged (its output gain trims
    /// processing that does not run); the translator reports it.
    Other,
    /// Not carried, a parallel path (a reverb, a delay): only the dry level
    /// passes. The translator reports it.
    OtherWet,
}

/// `stages` two-pole state-variable stages in series at `hz` (none: one
/// pole), each with its damping `k` (`1/Q`), then `gain`. A band stage is the
/// raw band tap `s/(s² + ks + 1)`, peaking at `1/k`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub pass: Pass,
    pub hz: f32,
    pub stages: u8,
    /// An alternative kernel; for a resonant ladder the last damping `k`
    /// sets its resonance `1 − k/2`.
    pub ladder: Option<Ladder>,
    pub damping: [f32; 3],
    pub gain: f32,
    /// Octaves a [`ModKind::Cutoff`] depth of 1 moves the cutoff (the range
    /// of the format's cutoff control).
    pub octaves: f32,
    /// Octaves of Q a [`ModKind::Resonance`] depth of 1 moves the damping
    /// (the range of the format's resonance control); 0 when it has none.
    pub q_octaves: f32,
}

/// Alternative filter kernels. Ladder taps 1..=12 select low 1..4 poles,
/// high 1..4, band 2 and 4, then peak and notch, or notch 2 and 4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ladder {
    /// A first-order exponential low/high-pass, with no Nyquist zero in its low-pass.
    OnePole,
    /// Zero-delay (trapezoidal) poles, saturating in the loop.
    ZeroDelay(u8),
    /// Explicit poles with the feedback a sample late and one saturator on
    /// the input; `fat` lifts the input with the resonance (1 holds the
    /// passband). Its input gain is the filter's gain, made up after.
    Explicit { taps: u8, fat: f32 },
    /// A two-pole Sallen-Key, low (`morph` 1) to high (0), its feedback curve `[a, b]`.
    SallenKey { morph: f32, curve: [f32; 2] },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Low,
    High,
    Band,
    Notch,
}

/// A peaking band: `gain_db` at `hz`, width by `q` (the cookbook bell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bell {
    pub shape: Shape,
    pub hz: f32,
    /// Q of a peak; a shelf's slope (1 the steepest knee without overshoot).
    pub q: f32,
    pub gain_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Peak,
    LowShelf,
    HighShelf,
}

impl Bell {
    pub const FLAT: Self = Self {
        shape: Shape::Peak,
        hz: 1000.0,
        q: 1.0,
        gain_db: 0.0,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub name: String,
    /// Its place among the instrument's script slots, empty slots counted:
    /// presets save their values by this place.
    pub slot: usize,
    pub dialect: Dialect,
    pub source: String,
    /// Saved persistent values in the dialect's own form (Lua: `widget=value`).
    pub saved: Vec<String>,
    /// Library data files the script loads (Kontakt `load_array(%x, 2)`:
    /// `data/x.nka`), by lowercase array name without its sigil. Lua: the
    /// modules `require` may load, by their path in the library.
    pub data: Vec<(String, Vec<u8>)>,
    /// MIDI controllers that move this script's controls.
    pub links: Vec<CcLink>,
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
    /// Audible once a script opens what load leaves closed (an aux no send reaches).
    Latent,
    /// Sound differs (a missing effect, a wrong curve).
    Audible,
    /// Notes are missing or wrong.
    Broken,
}
