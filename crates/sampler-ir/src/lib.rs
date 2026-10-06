//! Format-neutral semantic instrument description.
//!
//! Source translators (Kontakt, SFZ, ...) emit an [`Instrument`]; the native
//! runtime lowers it to prepared execution tables. Plain data: no I/O, no
//! audio-thread state, no dependencies. Quantities keep source units
//! ([`units`]), every modulator and processor names its [`Scope`], and meaning
//! a translator cannot express is listed in [`Instrument::unsupported`] rather
//! than dropped.
#![forbid(unsafe_code)]

pub mod units;
mod validate;

pub use units::{Frequency, Gain, Pan, PanLaw, Pitch, Resonance, SourceFrames, Span, Time};
pub use validate::{Reference, ValidationError};

macro_rules! reference {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub usize);
    )*};
}

reference! {
    /// Index into [`Instrument::assets`].
    AssetRef,
    /// Index into [`Instrument::groups`].
    GroupRef,
    /// Index into [`Instrument::zones`].
    ZoneRef,
    /// Index into [`Instrument::sequences`].
    SequenceRef,
    /// Index into [`Instrument::articulations`].
    ArticulationRef,
    /// Index into [`Instrument::modulators`].
    ModulatorRef,
    /// Index into [`Instrument::chains`].
    ChainRef,
    /// Index into [`Instrument::buses`].
    BusRef,
    /// Index into [`Instrument::controls`].
    ControlRef,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Instrument {
    pub name: String,
    pub source: SourceFormat,
    pub assets: Vec<Asset>,
    pub groups: Vec<Group>,
    pub zones: Vec<Zone>,
    pub sequences: Vec<Sequence>,
    pub articulations: Vec<Articulation>,
    pub modulators: Vec<Modulator>,
    pub routes: Vec<Route>,
    pub chains: Vec<Chain>,
    pub buses: Vec<Bus>,
    pub controls: Vec<Control>,
    pub behaviors: Vec<Behavior>,
    /// Source meaning this description does not carry. Lowering never reads it;
    /// it exists so a caller can show or reject what was not translated.
    pub unsupported: Vec<Unsupported>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SourceFormat {
    #[default]
    Native,
    Kontakt {
        /// Serialized object version of the program records.
        version: u16,
    },
    Sfz,
    /// UVI Falcon / Workstation program XML.
    Uvi,
}

// ---------------------------------------------------------------- assets

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub location: AssetLocation,
    pub encoding: Encoding,
    /// Recorded pitch stored with the audio, if any.
    pub root_key: Option<u8>,
    /// Loops stored with the audio (e.g. a WAV `smpl` chunk).
    pub loops: Vec<LoopRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetLocation {
    /// Path as authored, relative to the instrument file unless absolute.
    Path(String),
    /// Entry of a Kontakt file table, resolved by the container profile.
    KontaktFile { id: i32 },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Unknown,
    Wav,
    Aiff,
    Flac,
    Ogg,
    /// Native Instruments compressed wave.
    Ncw,
}

// ---------------------------------------------------------------- mapping

/// A Kontakt group or other source-level layer: shared settings for its zones.
/// SFZ `<group>`/`<master>` headers are inheritance, not scopes, so the SFZ
/// translator resolves them into zones and emits no groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub name: String,
    pub gain: Gain,
    pub pan: Pan,
    pub tune: Pitch,
    /// Group-scope chain; its processors see the sum of this group's voices.
    pub chain: Option<ChainRef>,
    pub output: Output,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub asset: AssetRef,
    pub group: Option<GroupRef>,
    pub keys: KeyRange,
    pub velocities: VelocityRange,
    pub conditions: Vec<ControllerRange>,
    pub trigger: Trigger,
    pub selection: Option<Selection>,
    pub articulation: Option<ArticulationRef>,
    pub pitch: KeyTracking,
    pub tune: Pitch,
    pub gain: Gain,
    /// How note velocity scales this zone's amplitude.
    pub velocity: VelocityResponse,
    pub pan: Pan,
    pub playback: Playback,
    /// Voice-scope processing for this zone, before group/bus processing.
    pub chain: Option<ChainRef>,
    /// Voice-scope amplitude envelope; `None` is a gate (instant on/off).
    pub amplitude: Option<ModulatorRef>,
}

impl Instrument {
    /// Keep only the zones `keep` accepts and the assets they use, renumbering
    /// assets in their original order. Returns the original index of each
    /// remaining asset, so a caller can load just those.
    pub fn retain_zones(&mut self, mut keep: impl FnMut(&Zone) -> bool) -> Vec<usize> {
        self.zones.retain(|zone| keep(zone));
        let mut used = vec![false; self.assets.len()];
        for zone in &self.zones {
            if let Some(used) = used.get_mut(zone.asset.0) {
                *used = true;
            }
        }
        let kept: Vec<usize> = (0..self.assets.len()).filter(|&i| used[i]).collect();
        let mut renumbered = vec![0; self.assets.len()];
        for (new, &old) in kept.iter().enumerate() {
            renumbered[old] = new;
        }
        let mut index = 0;
        self.assets.retain(|_| {
            index += 1;
            used[index - 1]
        });
        for zone in &mut self.zones {
            zone.asset = AssetRef(
                renumbered
                    .get(zone.asset.0)
                    .copied()
                    .unwrap_or(zone.asset.0),
            );
        }
        kept
    }
}

impl Zone {
    /// Full key and velocity range, root key 60, no processing.
    pub fn new(asset: AssetRef) -> Self {
        Self {
            asset,
            group: None,
            keys: KeyRange::FULL,
            velocities: VelocityRange::FULL,
            conditions: Vec::new(),
            trigger: Trigger::Attack,
            selection: None,
            articulation: None,
            pitch: KeyTracking::Tracked { root: 60 },
            tune: Pitch::NONE,
            gain: Gain::UNITY,
            velocity: VelocityResponse::Linear,
            pan: Pan::CENTER,
            playback: Playback::default(),
            chain: None,
            amplitude: None,
        }
    }
}

/// Velocity-to-amplitude response; selection always uses the raw velocity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VelocityResponse {
    /// Velocity does not change amplitude.
    None,
    /// Amplitude is velocity / 127.
    #[default]
    Linear,
    /// Amplitude is (velocity / 127) ^ exponent.
    Power(f64),
}

/// Inclusive MIDI key range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRange {
    pub low: u8,
    pub high: u8,
}

impl KeyRange {
    pub const FULL: Self = Self { low: 0, high: 127 };
}

/// Inclusive MIDI 1.0 velocity range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VelocityRange {
    pub low: u8,
    pub high: u8,
}

impl VelocityRange {
    pub const FULL: Self = Self { low: 1, high: 127 };
}

/// Plays only while a 7-bit controller is within an inclusive range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerRange {
    pub controller: u8,
    pub low: u8,
    pub high: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    #[default]
    Attack,
    /// On key-up, regardless of sustain pedal.
    KeyRelease,
    /// When the note's gate closes: key-up, or pedal-up for a sustained key.
    GateRelease,
    /// Only when no other key is held.
    First,
    /// Only when another key is already held.
    Legato,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTracking {
    /// Pitch follows the played key relative to `root`, 100 cents per key.
    Tracked { root: u8 },
    /// Pitch follows the key by `cents_per_key` relative to `root`.
    Scaled { root: u8, cents_per_key: i32 },
    /// Every key plays the recorded pitch.
    Fixed,
}

/// The zone's place in a sequence of alternatives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub sequence: SequenceRef,
    pub take: Take,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Take {
    /// Zero-based position in a sequential or uniform-random sequence.
    Index(u32),
    /// Plays when a uniform draw in `[low, high)` lands here (SFZ lorand/hirand).
    Probability { low: f64, high: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sequence {
    pub policy: SequencePolicy,
    /// Number of alternatives; `Take::Index` values are below this.
    pub takes: u32,
    /// What owns the position counter.
    pub counter: CounterScope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequencePolicy {
    RoundRobin,
    Random,
    RandomNoRepeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterScope {
    Instrument,
    Key,
    Channel,
    ChannelKey,
}

/// A selectable articulation, switched by keys and/or a control.
#[derive(Clone, Debug, PartialEq)]
pub struct Articulation {
    pub name: String,
    pub switch_keys: Vec<u8>,
    /// Active before any switch is played.
    pub default: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Playback {
    pub start: SourceFrames,
    /// Exclusive end; `None` plays to the end of the audio.
    pub end: Option<SourceFrames>,
    pub reverse: bool,
    pub looping: Looping,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Looping {
    #[default]
    None,
    /// Ignores note-off; plays to the end (SFZ `one_shot`).
    OneShot,
    Continuous(LoopRange),
    /// Loops while the note is held, then plays past the loop end.
    UntilRelease(LoopRange),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopRange {
    pub start: SourceFrames,
    /// Exclusive end.
    pub end: SourceFrames,
    pub crossfade: Span,
    pub alternating: bool,
}

// ---------------------------------------------------------------- modulation

/// Where state lives and what signal a processor or modulator sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// One instance per playing voice.
    Voice,
    /// One instance per group, over the sum of its voices.
    Group(GroupRef),
    Bus(BusRef),
    /// The instrument output.
    Master,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Modulator {
    pub scope: Scope,
    pub source: ModulationSource,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModulationSource {
    Envelope(Envelope),
    Lfo(Lfo),
    Controller(u8),
    Velocity,
    Key,
    PitchBend,
    ChannelPressure,
    PolyPressure,
}

/// Delay-attack-hold-decay-sustain-release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub delay: Time,
    pub attack: Time,
    pub hold: Time,
    pub decay: Time,
    /// Level reached after decay, 0.0..=1.0 of full scale.
    pub sustain: f64,
    pub release: Time,
    pub attack_shape: Curve,
    pub decay_shape: Curve,
    pub release_shape: Curve,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            delay: Time::ZERO,
            attack: Time::ZERO,
            hold: Time::ZERO,
            decay: Time::ZERO,
            sustain: 1.0,
            release: Time::ZERO,
            attack_shape: Curve::Linear,
            decay_shape: Curve::Linear,
            release_shape: Curve::Linear,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Curve {
    #[default]
    Linear,
    /// expm1(k·t)/expm1(k); positive starts slowly.
    Exponential(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: LfoShape,
    pub rate: Frequency,
    pub delay: Time,
    pub fade_in: Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoShape {
    Sine,
    Triangle,
    Square,
    SawUp,
    SawDown,
    SampleAndHold,
}

/// Modulator output scaled into a target. Depth carries the target's unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    pub source: ModulatorRef,
    pub target: Target,
    pub depth: Depth,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Amplitude,
    Pitch,
    Pan,
    /// A processor parameter, addressed by chain and position.
    Processor {
        chain: ChainRef,
        index: usize,
        parameter: ProcessorParameter,
    },
    Control(ControlRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessorParameter {
    Cutoff,
    Resonance,
    Gain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Depth {
    Gain(Gain),
    Pitch(Pitch),
    /// Fraction of the target's full range.
    Normalized(f64),
}

// ---------------------------------------------------------------- DSP

/// Serial processors in one scope. A voice chain splits around the amplitude
/// envelope because nonlinear stages sound different on each side of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub scope: Scope,
    pub pre_amplitude: Vec<Processor>,
    pub post_amplitude: Vec<Processor>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Processor {
    Gain(Gain),
    Pan(Pan),
    Filter(Filter),
    Delay { time: Time, feedback: f64, mix: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    pub cutoff: Frequency,
    pub resonance: Resonance,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterKind {
    LowPass { poles: u8 },
    HighPass { poles: u8 },
    BandPass { poles: u8 },
    Notch { poles: u8 },
    AllPass,
    Peak { gain: Gain },
    LowShelf { gain: Gain },
    HighShelf { gain: Gain },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Output {
    #[default]
    Master,
    Bus(BusRef),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub chain: Option<ChainRef>,
    pub sends: Vec<Send>,
    pub output: Output,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Send {
    pub to: Output,
    pub gain: Gain,
    pub position: SendPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendPosition {
    PreChain,
    PostChain,
}

// ---------------------------------------------------------------- controls

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    /// Stable identity from the source (script variable, opcode, parameter ID).
    pub key: String,
    pub label: String,
    pub value: ControlValue,
    pub automation: Automation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    Continuous {
        min: f64,
        max: f64,
        default: f64,
        unit: ControlUnit,
    },
    Integer {
        min: i64,
        max: i64,
        default: i64,
    },
    Toggle {
        default: bool,
    },
    Choice {
        options: Vec<String>,
        default: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlUnit {
    None,
    Decibels,
    Hertz,
    Seconds,
    Semitones,
    Percent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Automation {
    #[default]
    None,
    Host,
    Controller(u8),
}

// ---------------------------------------------------------------- behavior

/// A script that reacts to events. The IR carries it in its source language;
/// lowering compiles it to the native behavior instruction set.
#[derive(Clone, Debug, PartialEq)]
pub struct Behavior {
    pub name: String,
    pub language: Language,
    pub source: String,
    /// Persisted values restored before the first callback, keyed by variable.
    pub state: Vec<(String, i64)>,
    pub requires: Vec<Capability>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Ksp,
    Lua,
}

/// A runtime service a behavior needs; lowering rejects modules whose
/// requirements the runtime cannot provide instead of running them partially.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    NoteCallbacks,
    ReleaseCallbacks,
    ControllerCallbacks,
    Waits,
    GeneratedNotes,
    EventEdits,
    GroupSelection,
    ControlCallbacks,
    UserInterface,
    PersistentState,
    FileAccess,
    EngineParameters,
}

// ---------------------------------------------------------------- report

/// Source meaning with no IR representation. Location is in source terms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    /// e.g. `"<region> 12 (line 40)"` or `"zone 3"`.
    pub location: String,
    /// The source feature: opcode, field or header name.
    pub feature: String,
    /// The authored value, verbatim.
    pub value: String,
    pub reason: Reason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The translator does not recognize this feature.
    Unknown,
    /// Recognized, but the IR has no equivalent yet.
    NotModeled,
    /// Recognized, but this value cannot be represented.
    InvalidValue,
}
