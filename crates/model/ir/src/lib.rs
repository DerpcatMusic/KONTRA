//! The instrument model: what a format reads from a vendor's files and what
//! engine prepare builds a plan from (06-FORMATS.md). Data only, load time
//! only. Everything a translator could not carry over is a [`Diagnostic`].

use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

mod address;
mod bus;
mod drive;
mod effects;
mod engine_par;
mod generator;
mod graph;
mod link;
mod model;
mod mods;
mod output;
mod readout;
mod reverb;
mod validate;
mod wavetable;
pub use address::{EnvStage, LfoParam, ModRoute, ParamAddr, RouteLaw, Scope};
pub use bus::{Bus, BusSend};
pub use drive::{
    AmpStack, AmpVoicing, Cabinet, Clipper, ColorSat, Converter, Crush, DriveCurve, EffectParam,
    KnobModel, KnobTarget, Overdrive, RackAt, RingMod, RingSource, Shaper, SlotKnob, SlotLaw,
    SlotShape, SlotShapes, SlotType,
};
pub use effects::{
    COMB_LOWEST_HZ, Cascade, CascadeStage, Clocked, ClockedShape, Comb, CombBank, CombMode,
    Compressor, Delay, DelayTime, Detector, DualDelay, ENSEMBLE_VOICES, Ensemble, EnsembleVoices,
    Formant, IDENTITY, ImpulseCuts, ImpulseKnob, Limiter, Matrix, Notches, Phaser, Polarity,
    Reverb, ScanShape, Scanner, Shifter, SpectralMorph, StereoDetune, Strip, Transient, VowelSet,
    Wah,
};
pub use engine_par::{
    Drives, EnginePar, HeldSet, ParRole, ScriptPar, ZonePar, held_key, unit_read,
};
pub use generator::Generator;
pub use graph::{AudioGraph, ChannelLayout, EffectId, GraphEdge, GraphNode, GraphNodeKind, NodeId};
pub use kontra_law::{FormulaId, Section, Sections, Stage, common};
pub use link::{CcLink, KnobMod, Linked};
pub use model::{
    EffectModel, ModelId, eval_model, eval_model_effect, model, register_model, register_test_model,
};
pub use mods::{
    Attractor, Blend, EnvelopeFollower, Exp2, Factor, GainCurve, GainCurve as Curve, NoteExpr,
    Source, Term, TimeScale, Transfer, Wave,
};
pub use output::{Balance, Output, OutputControllers};
pub use readout::{Form, Readout};
pub use reverb::{
    CombNetwork, HadamardNetwork, Law, NETWORK_LINES, Network, ReverbControls, ReverbNetwork,
    ReverbPar,
};
pub use validate::validate;
pub use wavetable::{
    FormShaper, Inharmonic, ModOsc, ModOscKind, Quality, Side, TableValue, TuneUnit, Warp,
    WarpShape, Wavetable,
};
// Tempo-synced slicing and rotary units, kept apart from the list above.
pub use effects::{BeatMasher, BeatSlicer, Gater, ReverseGrain, Rotary, TransposeStretch};

#[derive(Clone, Debug, PartialEq)]
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
    /// Which voice makes room at [`Self::voices`], released ones first when
    /// [`Self::steal_released_first`], fading over [`Self::steal_fade_ms`].
    pub steal: Steal,
    pub steal_released_first: bool,
    pub steal_fade_ms: f32,
    /// The key switch the instrument saved as held at load, before any key
    /// is played; `None` falls back to the lowest switch range.
    pub default_keyswitch: Option<u8>,
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
    /// Its scripts' saved values are a preset recalled onto this
    /// instrument after it loaded with its own: a script that keeps its
    /// state across a recall sees them
    /// land again after its first persistence-changed callback, and that
    /// callback runs once more, as it would on a running instrument.
    pub recalled: bool,
    /// Responses [`EffectKind::Convolution`] slots play.
    pub impulses: Vec<Impulse>,
    /// Responses a script may load into a convolution slot by name
    /// (lowercase, without its extension), by index into [`Self::samples`].
    pub responses: Vec<(String, usize)>,
    /// How [`KnobTarget::Slot`] knobs move the effects they address.
    pub slot_laws: Vec<SlotLaw>,
    /// Slots whose knobs [`KnobModel`]s rebuild together.
    pub slot_models: Vec<(RackAt, u8, KnobModel)>,
    /// The filter shapes a script may switch slots to.
    pub slot_shapes: SlotShapes,
    /// Routes that move a (Lua) script's knobs by name.
    pub knob_mods: Vec<KnobMod>,
    /// What [`KnobMod::source`] reads: blends and the free oscillators they
    /// read.
    pub knob_blends: Vec<Blend>,
    pub knob_lfos: Vec<Lfo>,
    /// Optional dynamic multi-channel audio routing DAG (IR vNext Phase 3).
    /// When `None`, the engine runs the standard bus/insert/send/main racks.
    pub graph: Option<AudioGraph>,
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
        steal: Steal::Oldest,
        steal_released_first: true,
        steal_fade_ms: 10.0,
        default_keyswitch: None,
        voice_groups: Vec::new(),
        inserts: Vec::new(),
        sends: Vec::new(),
        main: Vec::new(),
        output: Output::UNITY,
        route_pitch: Exp2::Exact,
        steal_whole_groups: false,
        recalled: false,
        impulses: Vec::new(),
        responses: Vec::new(),
        slot_laws: Vec::new(),
        slot_models: Vec::new(),
        slot_shapes: SlotShapes::NONE,
        knob_mods: Vec::new(),
        knob_blends: Vec::new(),
        knob_lfos: Vec::new(),
        graph: None,
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
    /// The early and late parts' lengths over their own (1 unchanged),
    /// stretched without changing pitch; they part where [`ImpulseCuts`]'s
    /// crossover does, else where the decay turns diffuse.
    pub size: [f32; 2],
    /// The share of the response kept (1 the whole of it, untouched): below
    /// 1 its first `max(2, length·frames)` frames, faded to silence by the
    /// raised cosine `(1 + cos(π·i/(n + 1)))/2`, before [`Self::mix`].
    pub length: f32,
    /// That window over the whole response too, at `length` 1. One window
    /// either way, chosen from the length the response is prepared at now.
    pub window_whole: bool,
}

impl Impulse {
    /// A convolution's index before any response is loaded.
    pub const NONE: u16 = u16::MAX;

    /// A response as recorded: what a slot with none loaded prepares the
    /// first one it loads as.
    pub const PLAIN: Self = Self {
        sample: 0,
        reverse: false,
        normalize: None,
        predelay_ms: 0.0,
        cuts: None,
        envelope: None,
        mix: None,
        size: [1.0, 1.0],
        length: 1.0,
        window_whole: false,
    };
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
    /// Set when the bytes are bare PCM with no header (a SoundFont's `smpl`):
    /// `len` bytes from `offset` hold `frames` of `channels` at `rate`.
    pub raw: Option<RawPcm>,
}

/// The encoding of a headerless sample ([`Sample::raw`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawPcm {
    /// Signed 16-bit little-endian, interleaved.
    I16Le,
}

/// In-memory byte transform applied to encrypted sample/container reads
/// (XOR streams, block ciphers, vendor container transforms). Keys live
/// strictly in memory and are never formatted by `Debug` or serialized.
pub trait ByteTransform: Send + Sync {
    /// Decrypts (or encrypts) `bytes` read from file offset `offset`.
    fn apply(&self, offset: u64, bytes: &mut [u8]);
}

/// Keyed transform of bytes a sample file stores encrypted: decrypts (or
/// encrypts) the file's bytes as the engine reads them. Held in memory only;
/// `Debug` never prints the key.
#[derive(Clone)]
pub struct Keystream(Arc<dyn ByteTransform>);

impl Keystream {
    /// Wraps a format-supplied [`ByteTransform`].
    #[must_use]
    pub fn new(transform: Arc<dyn ByteTransform>) -> Self {
        Self(transform)
    }

    /// Decrypts (or encrypts) `bytes` read from file offset `at`.
    pub fn apply(&self, at: u64, bytes: &mut [u8]) {
        self.0.apply(at, bytes);
    }
}

impl PartialEq for Keystream {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Keystream {}

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
    /// Its voices of a key stop when that key is played again (Kontakt's
    /// release-trigger Monophonic).
    pub repeat_cuts: bool,
    /// A script's start offset past a zone's `start_mod` range plays from
    /// the zone's start instead (only that much of a streamed sample is
    /// held ready).
    pub offset_limited: bool,
    /// Start rules as one unparenthesized condition: `And`/`AndNot`
    /// bind tighter than `Or`; empty always starts.
    pub start: Vec<(Join, StartRule)>,
    /// Voices of this group that may sound at once.
    pub polyphony: Option<u16>,
    /// Voices of this group stop when a voice of that voice group (index
    /// into [`Instrument::voice_groups`]) starts, as their own voice group
    /// stops a voice (hi-hat choke).
    pub off_by: Option<usize>,
    pub bus: usize,
    /// Saved unloaded: no preload until a script loads it.
    pub purged: bool,
    /// Muted (or silenced because another group is soloed): zones remain
    /// loaded and visible to scripts, but start no voices.
    pub muted: bool,
    /// How much of each zone's sample stays in memory; `None` keeps it whole.
    pub stream: Option<Stream>,
    /// Its zones are wavetables.
    pub wavetable: Option<Wavetable>,
    /// Optional unified generator descriptor (real-time oscillator, granular,
    /// or explicit PCM/wavetable override; IR vNext Phase 3).
    pub generator: Option<Generator>,
    /// Its source stretches time: voices play at this speed (1 as
    /// recorded) at their own pitch. `None`: speed follows pitch.
    pub speed: Option<f32>,
    /// Index into [`Instrument::voice_groups`].
    pub voice_group: Option<usize>,
    /// Saved group controls a script reads back with no processor behind
    /// them ([`crate::EnginePar::GroupHeld`]): index (−1: the whole group),
    /// [`crate::held_key`], raw value.
    pub held: Vec<(i8, u32, i32)>,
    /// Its notes slide in pitch from the note before (portamento).
    pub glide: Option<Glide>,
    /// Effects on this group's voices, by slot.
    pub inserts: Vec<Option<Effect>>,
    /// First insert slot that runs after the voice's amplitude envelope,
    /// volume and pan (`0..amp_split` are pre-amp; `amp_split..` are
    /// post-amp). Values `>= inserts.len()` place every insert pre-amp.
    pub amp_split: u8,
    /// Shapes [`Source::Envelope`] reads, by index.
    pub envelopes: Vec<Breakpoints>,
    /// Oscillators [`Source::Lfo`] reads, by index.
    pub lfos: Vec<Lfo>,
    /// Transfer tables [`Source::Shaped`] reads, by index.
    pub shapers: Vec<Transfer>,
    /// Sums [`Source::Blend`] reads, by index.
    pub blends: Vec<Blend>,
    /// Audio-rate envelope followers on this group, by index.
    pub followers: Vec<EnvelopeFollower>,
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
    /// Each target's signed depth as saved; what a script reads and
    /// writes on a route the engine does not carry.
    pub depths: Vec<f32>,
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
        repeat_cuts: false,
        offset_limited: false,
        off_by: None,
        bus: 0,
        purged: false,
        muted: false,
        stream: None,
        wavetable: None,
        generator: None,
        speed: None,
        voice_group: None,
        held: Vec::new(),
        glide: None,
        inserts: Vec::new(),
        amp_split: u8::MAX,
        envelopes: Vec::new(),
        lfos: Vec::new(),
        shapers: Vec::new(),
        blends: Vec::new(),
        followers: Vec::new(),
        routes: Vec::new(),
        zones: Vec::new(),
    };

    /// Slot index where the amplitude stage sits, clamped to `self.inserts.len()`.
    #[must_use]
    pub fn amp_split_index(&self) -> usize {
        usize::from(self.amp_split).min(self.inserts.len())
    }

    /// Effective wavetable descriptor: prefers [`Self::generator`] when it holds
    /// [`Generator::Wavetable`], falling back to the legacy [`Self::wavetable`] field.
    #[must_use]
    pub fn effective_wavetable(&self) -> Option<&Wavetable> {
        match &self.generator {
            Some(Generator::Wavetable(w)) => Some(w),
            _ => self.wavetable.as_ref(),
        }
    }
}

const _: () = {
    const fn assert_partial_eq<T: PartialEq>() {}
    assert_partial_eq::<Instrument>();
};

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

/// Beats per cycle of a tempo-synced rate knob's 30 places, slowest first
/// (a format's note values: 32 bars down to a 32nd triplet).
pub const NOTE_LADDER: [f32; 30] = [
    128.0,
    64.0,
    32.0,
    28.0,
    24.0,
    20.0,
    16.0,
    12.0,
    8.0,
    6.0,
    4.0,
    3.0,
    8.0 / 3.0,
    2.0,
    1.5,
    4.0 / 3.0,
    1.0,
    0.75,
    2.0 / 3.0,
    0.5,
    0.375,
    1.0 / 3.0,
    0.25,
    0.1875,
    1.0 / 6.0,
    0.125,
    0.093_75,
    1.0 / 12.0,
    0.0625,
    1.0 / 24.0,
];

/// `beats` moved `shift` places along [`NOTE_LADDER`] from its nearest place,
/// held on the ladder.
#[must_use]
pub fn ladder_beats(beats: f32, shift: f32) -> f32 {
    let near = |b: f32| (b / beats.max(1e-6)).ln().abs();
    let at = NOTE_LADDER
        .iter()
        .enumerate()
        .min_by(|a, b| near(*a.1).total_cmp(&near(*b.1)))
        .map_or(0, |(i, _)| i);
    let to = (at as f32 + shift).round().clamp(0.0, 29.0) as usize;
    NOTE_LADDER.get(to).copied().unwrap_or(beats)
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
    /// Decay shape k of `expm1(k·t)/expm1(k)`.
    pub decay_shape: f32,
    /// Release shape k of `expm1(k·t)/expm1(k)`.
    pub release_shape: f32,
    /// Seconds an attack stays silent before its curve runs in the rest of
    /// the attack time; none without an attack.
    pub lag: f32,
    /// Silent time before the attack, independent of its duration.
    pub delay: f32,
    /// The voice gain is `floor + depth·level`: a volume route's intensity
    /// below full either scales the envelope or keeps a floor under it.
    pub floor: f32,
    pub depth: f32,
    /// AHD-only one-shot mode: ignores note-off, decays to 0, and ends after decay.
    pub one_shot: bool,
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
        decay_shape: 0.0,
        release_shape: 0.0,
        lag: 0.0,
        delay: 0.0,
        floor: 0.0,
        depth: 1.0,
        one_shot: false,
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
    pub route: ModRoute,
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
    /// The limit; 0 none (a line a script may make mono later).
    pub voices: u16,
    pub steal: Steal,
    /// Steal released voices before held ones.
    pub prefer_released: bool,
    /// Fade of a stolen or excluded voice.
    pub fade_ms: f32,
    /// Starting a voice fades every voice of the other voice groups in this set.
    pub exclusive: Option<u8>,
    /// `voices` counts notes: a note past it takes every voice of the note
    /// it steals, however many zones each sounds.
    pub per_note: bool,
    /// One note at a time (`voices` 1, `per_note`): a new note while the
    /// group's note is held takes over its voices, which keep playing and
    /// move to the new pitch (sliding with the group's glide), instead of
    /// starting its own.
    pub legato: bool,
    /// Last-note priority: releasing the group's note while an older key of
    /// it is still down brings that note back (its voices taken back when
    /// `legato`, started again otherwise).
    pub returns: bool,
    /// Voices of one key the group may sound: a new one past it stops the
    /// key's earlier voices, oldest first, or with `quieter_only` only those
    /// started no louder than it, quietest first.
    pub per_key: Option<u16>,
    pub quieter_only: bool,
    /// A voice it stops (stolen, excluded or choked) plays its release
    /// instead of fading over `fade_ms`.
    pub release: bool,
}

impl VoiceGroup {
    /// No limit to speak of: oldest first, released first, a 10 ms fade.
    pub const DEFAULT: Self = Self {
        voices: u16::MAX,
        steal: Steal::Oldest,
        prefer_released: true,
        fade_ms: 10.0,
        exclusive: None,
        per_note: false,
        legato: false,
        returns: false,
        per_key: None,
        quieter_only: false,
        release: false,
    };
}

/// Portamento: a new note's voices start at the pitch the previous note of
/// their `scope` sounded and slide to their own, in semitones along `curve`.
/// A script's tuning (`change_tune` loops) adds on top. docs/engine/voices.md.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glide {
    /// Groups with the same scope share the previous note (a layer, or the
    /// whole instrument).
    pub scope: u16,
    /// The share of the interval the slide starts from: 1 the whole of it,
    /// negative from beyond the note (overshooting into it).
    pub depth: f32,
    /// The most semitones of interval a slide covers before `depth`; a
    /// wider jump slides this far and starts from there.
    pub range: f32,
    /// The previous note is the last one the instrument heard (a played
    /// key, one a script ignored or replaced too), not the last that
    /// sounded in the scope.
    pub from_heard: bool,
    /// How long the slide takes, or with `per_semitone` how long it takes
    /// for each semitone of the interval (a constant rate).
    pub seconds: f32,
    pub per_semitone: bool,
    /// Only while another note of the scope is held; otherwise from the
    /// last note played, held or not.
    pub legato: bool,
    /// A slide cut short by the next note hands on the pitch it had reached;
    /// otherwise the next starts from the previous note's own pitch.
    pub resume: bool,
    /// -1..=1, 0 straight: the share of the interval covered half way
    /// through is `(1 - curve)/2`, along an exponential segment, so a
    /// positive curve leaves the old pitch slowly and arrives fast.
    pub curve: f32,
}

impl Glide {
    /// The share of the interval covered at phase `p` (0..=1): with `k` the
    /// share at half time and `r = (1 - k)/k`, `(r^(2p) - 1)/(r^2 - 1)`.
    #[must_use]
    pub fn covered(self, p: f32) -> f32 {
        let p = p.clamp(0.0, 1.0);
        let k = (0.5 * (1.0 - self.curve)).clamp(1e-4, 0.9999);
        let r = (1.0 - k) / k;
        if (r - 1.0).abs() < 1e-6 {
            p
        } else {
            (r.powf(2.0 * p) - 1.0) / (r * r - 1.0)
        }
    }

    /// How long a slide over `semitones` takes, seconds.
    #[must_use]
    pub fn duration(self, semitones: f32) -> f32 {
        if self.per_semitone {
            self.seconds * semitones.abs()
        } else {
            self.seconds
        }
    }
}

/// Which voice a full voice group gives up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steal {
    Oldest,
    Newest,
    Highest,
    Lowest,
    Quietest,
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

/// Which counter a [`StartRule::RoundRobin`] steps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RrScope {
    /// One counter for the whole instrument (Kontakt default).
    #[default]
    Global,
    /// One independent counter per MIDI key (SFZ / per-note round robin).
    Key,
    /// One counter per group.
    Group,
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
    /// Step `position` of a cycle of `length`, counted per [`RrScope`].
    RoundRobin {
        scope: RrScope,
        position: u8,
        length: u8,
        random: bool,
        /// When `random`, exclude the previous winner so consecutive hits do not repeat.
        no_repeat: bool,
        /// When `random`, draw a fresh value on release triggers instead of reusing the attack's.
        release_redraws: bool,
    },
    /// One zone slot of the group per note, in turn or at random (no repeat), per group.
    Cycle { random: bool },
    /// Random probability window on `[0, 10_000)` (`lorand`/`hirand` × 10,000);
    /// matches when `lo <= draw < hi`.
    Random { lo: u16, hi: u16 },
}

/// Gain curve of a [`Crossfade`] across its ramp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CrossfadeLaw {
    /// Linear amplitude ramp: `t` fading in, `1 − t` fading out (Kontakt's measured law).
    #[default]
    Linear,
    /// Equal-power sine/cosine ramp: `sin(π/2 · t)` fading in, `cos(π/2 · t)` fading out.
    EqualPower,
}

/// What a [`Crossfade`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CrossfadeControl {
    Velocity,
    Key,
    /// A MIDI CC (0..=127), evaluated live on sounding voices.
    Cc(u8),
}

/// A crossfade over 0..=127 values of `control`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Crossfade {
    pub control: CrossfadeControl,
    /// Silent at or below `.0`, full at or above `.1`.
    pub fade_in: Option<(i16, i16)>,
    /// Full at or below `.0`, silent at or above `.1`.
    pub fade_out: Option<(i16, i16)>,
    pub law: CrossfadeLaw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// The id scripts address it by: its place in the format's zone list.
    pub id: u32,
    /// Index into [`Instrument::samples`].
    pub sample: usize,
    pub keys: RangeInclusive<u8>,
    pub velocities: RangeInclusive<u8>,
    /// Velocity steps over which the zone fades in above its low edge and out
    /// below its high edge (a linear amplitude ramp, Kontakt's measured law).
    pub velocity_fade: (u8, u8),
    /// The same over keys.
    pub key_fade: (u8, u8),
    /// Explicit crossfades (equal-power velocity/key fades or live MIDI CC crossfades).
    /// When present for `Velocity` or `Key`, overrides [`Self::velocity_fade`] or
    /// [`Self::key_fade`] at prepare time.
    pub crossfades: Vec<Crossfade>,
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
    /// Per-zone sample-position envelopes read by [`Source::ZoneEnvelope`].
    pub envelopes: Vec<ZoneEnvelope>,
}

/// A per-zone modulation envelope indexed by sample frames, bipolar −1..=1.
/// Matches [`Source::ZoneEnvelope`] routes with the same `slot`.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneEnvelope {
    pub slot: u8,
    pub points: Vec<ZonePoint>,
    /// Optional loop `(start, length)` in sample frames (`length > 0`).
    pub loop_range: Option<(i32, i32)>,
    /// When true, advances with the voice's sample playback cursor (pitch-locked);
    /// when false, advances at one frame per output sample from the voice's start.
    pub speed_lock: bool,
}

/// One breakpoint of a [`ZoneEnvelope`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZonePoint {
    /// Position in sample frames.
    pub frame: i32,
    /// Bipolar level, −1..=1.
    pub value: f32,
    /// Exponential curvature `k` of the segment starting here:
    /// `(e^(k·t) − 1)/(e^k − 1)` (0 is linear).
    pub curve: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loop {
    pub start: u64,
    pub end: u64,
    /// Frames before `end` that fade linearly into the frames before `start`.
    pub crossfade: u64,
    /// Total passes through the loop; 0 repeats forever (or until release),
    /// 1 plays once without wrapping.
    pub count: u32,
    /// Each repeat plays the loop backwards, then forwards again.
    pub alternating: bool,
    /// Repeats until the note is released, then plays on past `end`.
    pub until_release: bool,
    /// Linear playback-rate multiplier applied on loop repeats (1.0 untuned).
    pub tune_ratio: f32,
}

impl Default for Loop {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            crossfade: 0,
            count: 0,
            alternating: false,
            until_release: false,
            tune_ratio: 1.0,
        }
    }
}

/// Static table data or a heap-allocated owned copy (for dynamic or
/// deserialized presets). Cloning never allocates, and `size_of::<Shared<T>>()`
/// stays 16 bytes regardless of `size_of::<T>()`.
#[derive(Clone)]
pub enum Shared<T: 'static> {
    Borrowed(&'static T),
    Owned(Arc<T>),
}

impl<T: 'static> Shared<T> {
    /// Wraps a static table with zero allocation.
    #[must_use]
    pub const fn borrowed(value: &'static T) -> Self {
        Self::Borrowed(value)
    }

    /// Wraps dynamically constructed or deserialized data.
    #[must_use]
    pub fn owned(value: T) -> Self {
        Self::Owned(Arc::new(value))
    }
}

impl<T: 'static> std::ops::Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        match self {
            Self::Borrowed(v) => v,
            Self::Owned(v) => v,
        }
    }
}

impl<T: PartialEq + 'static> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Eq + 'static> Eq for Shared<T> {}

impl<T: std::fmt::Debug + 'static> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

/// An effect slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub kind: EffectKind,
    pub bypass: bool,
    /// Linear gain on the slot's output.
    pub output: f32,
    /// Linear level of the unprocessed signal mixed back in.
    pub dry: f32,
}

#[derive(Clone, Debug, PartialEq)]
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
    /// Four EQ bands plus console pass-filter bank (18 dB/oct HP, 12 dB/oct LP).
    EqPass {
        bands: [Bell; 4],
        hp_hz: Option<f32>,
        lp_hz: Option<f32>,
    },
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
    /// A ring modulator or frequency shifter, its own dry mixed in.
    RingMod(RingMod),
    /// Copies its input to send slot n at `levels[n]` (linear), passing it on.
    Sends([f32; 8]),
    /// The input convolved with [`Instrument::impulses`] at this index (wet);
    /// [`Impulse::NONE`] before a response is loaded (silent).
    Convolution { impulse: u16 },
    /// An algorithmic reverb (the wet path; `dry` mixes the input back).
    Reverb(Reverb),
    /// A compressor, in place.
    Compressor(Compressor),
    /// A compressor into a waveshaper, in place.
    Strip(Strip),
    /// A peak limiter, in place.
    Limiter(Limiter),
    /// An echo, chorus or flanger (the wet path; `dry` mixes the input back).
    Delay(Delay),
    /// A scanner vibrato, its own input mixed in.
    Scanner(Scanner),
    /// A pitch-shifting or reversing echo (the wet path; `dry` mixes the
    /// input back).
    Shifter(Shifter),
    /// A phaser (the wet path; `dry` mixes the input back).
    Phaser(Phaser),
    /// A phaser of second-order allpasses, its input mixed in (`dry` 0).
    Notches(Notches),
    /// A chorus ensemble, its input mixed in (`dry` 0).
    Ensemble(Ensemble),
    /// A bank of tuned combs, its input mixed in (`dry` 0).
    CombBank(CombBank),
    /// A feedback comb run on each voice (`dry` mixes the input back).
    Comb(Comb),
    /// A transient shaper, in place.
    Transient(Transient),
    /// A two-rotor rotary speaker, its input mixed in by `mix`.
    Rotary(Rotary),
    /// A rhythmic amplitude gater.
    Gater(Gater),
    /// A beat-locked loop/stutter masher.
    BeatMasher(BeatMasher),
    /// A beat capture that replays reordered slices.
    BeatSlicer(BeatSlicer),
    /// A reverse granular delay.
    ReverseGrain(ReverseGrain),
    /// An overlap-add pitch shifter and time stretcher.
    TransposeStretch(TransposeStretch),
    /// A dual independent stereo delay (the wet path; `dry` mixes the input back).
    DualDelay(DualDelay),
    /// A stereo pitch-detuning widener, its own dry mixed in.
    StereoDetune(StereoDetune),
    /// A resonant multi-stage wah filter, in place.
    Wah(Wah),
    /// A multi-stage amplifier or distortion/overdrive pedal, in place.
    AmpStack(AmpStack),
    /// A speaker cabinet and room early-reflection simulator, in place.
    Cabinet(Cabinet),
    /// A ten-character harmonic saturator with bass crossover and 3-band EQ.
    ColorSat(ColorSat),
    /// A dynamic four-band spectral/formant morphing filter, in place.
    SpectralMorph(SpectralMorph),
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
    /// Octaves a [`ParamAddr::FilterCutoff`] depth of 1 moves the cutoff (the range
    /// of the format's cutoff control).
    pub octaves: f32,
    /// Octaves of Q a [`ParamAddr::FilterResonance`] depth of 1 moves the damping
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
    /// The folder of the resource container this script came with (the
    /// folder its `get_folder` resource-container kind reads), when the
    /// translator knows it: a multi's MIDI player keeps its own when folded
    /// into the synth.
    pub container: Option<std::path::PathBuf>,
    /// Folded in from a multi's MIDI player part: it runs in front of the
    /// instrument's own scripts, but its page belongs to that player's part,
    /// not to this instrument's performance view.
    pub player: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_synced_rate_moves_along_the_note_ladder_and_stays_on_it() {
        // 2/3 of a beat is place 18; four places on is 1/4 (place 22).
        assert!((ladder_beats(0.666_666_7, 4.0) - 0.25).abs() < 1e-6);
        assert_eq!(ladder_beats(2.0, -40.0), 128.0);
        assert!((ladder_beats(1.0, 0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn keystream_delegates_to_custom_transform_and_redacts_debug() {
        struct OffsetXor(u8);
        impl ByteTransform for OffsetXor {
            fn apply(&self, offset: u64, bytes: &mut [u8]) {
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b ^= self.0.wrapping_add((offset + i as u64) as u8);
                }
            }
        }

        let key = Keystream::new(Arc::new(OffsetXor(0x5a)));
        assert_eq!(format!("{key:?}"), "Keystream(..)");
        assert_eq!(key, key.clone());
        assert_ne!(key, Keystream::new(Arc::new(OffsetXor(0x5a))));

        let plain: Vec<u8> = (0..64u8).collect();
        let mut buf = plain.clone();
        key.apply(100, &mut buf);
        assert_ne!(buf, plain);
        key.apply(100, &mut buf);
        assert_eq!(buf, plain);
    }

    /// An effect travels by value through racks and commands: an effect kind
    /// that grows past this bound states its shared fields once instead.
    /// [`Strip`] (a compressor and a waveshaper, one slot) sets it: 24 bytes
    /// more per slot, against a second slot and its routing.
    #[test]
    fn an_effect_stays_small() {
        let size = std::mem::size_of::<Effect>();
        assert!(size <= 232, "ir::Effect is {size} bytes");
    }
}
