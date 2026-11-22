//! Declarative, versioned schema for `.kontra` native instruments.
//!
//! Serializes cleanly to human-readable TOML, JSON, or KONTRA's compact
//! magic-headed binary envelope while remaining 100% vendor-neutral.

use serde::{Deserialize, Serialize};

/// Current native `.kontra` schema version.
pub const FORMAT_VERSION: u16 = 1;

/// Magic header for binary `.kontra` payloads (`"KTRA"` + version `1` LE).
pub const BINARY_MAGIC: &[u8; 6] = b"KTRA\x01\x00";

fn default_version() -> u16 {
    FORMAT_VERSION
}

fn default_voices() -> u16 {
    64
}

fn default_true() -> bool {
    true
}

fn default_steal_fade_ms() -> f32 {
    10.0
}

fn default_one_f32() -> f32 {
    1.0
}

fn default_one_f64() -> f64 {
    1.0
}

fn default_release_s() -> f32 {
    0.001_3
}

fn default_keys() -> [u8; 2] {
    [0, 127]
}

fn default_velocities() -> [u8; 2] {
    [1, 127]
}

fn default_root() -> u8 {
    60
}

fn default_hirand() -> u16 {
    10_000
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstrumentSpec {
    #[serde(default = "default_version")]
    pub version: u16,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_voices")]
    pub voices: u16,
    #[serde(default)]
    pub steal: StealSpec,
    #[serde(default = "default_true")]
    pub steal_released_first: bool,
    #[serde(default = "default_steal_fade_ms")]
    pub steal_fade_ms: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub voice_groups: Vec<VoiceGroupSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buses: Vec<BusSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<AudioGraphSpec>,
    #[serde(default)]
    pub groups: Vec<GroupSpec>,
}

/// Wave shape for a native [`GeneratorSpec::Oscillator`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "wave", rename_all = "snake_case")]
pub enum OscWaveSpec {
    Sine,
    Saw,
    Ramp,
    Square {
        #[serde(default = "default_half_f32")]
        width: f32,
    },
    Triangle,
    Noise,
}

fn default_half_f32() -> f32 {
    0.5
}

/// Declarative group generator descriptor (IR vNext Phase 3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeneratorSpec {
    Pcm {
        sample: String,
        #[serde(default, skip_serializing_if = "is_zero_u64")]
        start: u64,
        #[serde(default, skip_serializing_if = "is_zero_u64")]
        end: u64,
        #[serde(default, skip_serializing_if = "is_false")]
        reverse: bool,
    },
    Oscillator {
        #[serde(flatten)]
        wave: OscWaveSpec,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        tune_semitones: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        phase: f32,
    },
    Granular {
        sample: String,
        grain_ms: f32,
        density: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        position: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        pitch_semitones: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        spread: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelLayoutSpec {
    Mono,
    #[default]
    Stereo,
    Multi {
        channels: u8,
    },
    Surround51,
    Surround71,
    Ambisonics {
        order: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassSpec {
    Low,
    High,
    Band,
    Notch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqShapeSpec {
    Peak,
    LowShelf,
    HighShelf,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BellSpec {
    pub shape: EqShapeSpec,
    pub hz: f32,
    pub q: f32,
    pub gain_db: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphEffectSpec {
    Gain {
        gain: f32,
        #[serde(default, skip_serializing_if = "is_false")]
        bypass: bool,
        #[serde(default = "default_one_f32")]
        output: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        dry: f32,
    },
    Invert {
        #[serde(default, skip_serializing_if = "is_false")]
        phase: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        swap: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        bypass: bool,
        #[serde(default = "default_one_f32")]
        output: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        dry: f32,
    },
    Stereo {
        width: f32,
        pan: f32,
        #[serde(default, skip_serializing_if = "is_false")]
        bypass: bool,
        #[serde(default = "default_one_f32")]
        output: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        dry: f32,
    },
    Filter {
        pass: PassSpec,
        hz: f32,
        stages: u8,
        damping: [f32; 3],
        #[serde(default = "default_one_f32")]
        gain: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        octaves: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        q_octaves: f32,
        #[serde(default, skip_serializing_if = "is_false")]
        bypass: bool,
        #[serde(default = "default_one_f32")]
        output: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        dry: f32,
    },
    Eq {
        bands: [BellSpec; 4],
        #[serde(default, skip_serializing_if = "is_false")]
        bypass: bool,
        #[serde(default = "default_one_f32")]
        output: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        dry: f32,
    },
    Sends {
        levels: [f32; 8],
        #[serde(default, skip_serializing_if = "is_false")]
        bypass: bool,
        #[serde(default = "default_one_f32")]
        output: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        dry: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphNodeSpec {
    GroupInput {
        group: u16,
    },
    Bus {
        #[serde(default)]
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        effects: Vec<u32>,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        gain_db: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        pan: f32,
        #[serde(default)]
        layout: ChannelLayoutSpec,
        #[serde(default = "default_true")]
        to_main: bool,
    },
    Rack {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        effects: Vec<u32>,
        #[serde(default)]
        layout: ChannelLayoutSpec,
    },
    Split {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pre_amp: Vec<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        post_amp: Vec<u32>,
        #[serde(default = "default_one_f32")]
        amp_gain: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        amp_pan: f32,
    },
    Mix {
        #[serde(default = "default_one_f32")]
        gain: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        pan: f32,
    },
    Output {
        #[serde(default)]
        bus_index: u8,
        #[serde(default)]
        layout: ChannelLayoutSpec,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphEdgeSpec {
    pub from: u16,
    pub to: u16,
    #[serde(default = "default_one_f32")]
    pub gain: f32,
    #[serde(default = "default_true")]
    pub post_fader: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sidechain: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioGraphSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<GraphNodeSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<GraphEdgeSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<GraphEffectSpec>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StealSpec {
    #[default]
    Oldest,
    Newest,
    Highest,
    Lowest,
    Quietest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceGroupSpec {
    #[serde(default = "default_voices")]
    pub voices: u16,
    #[serde(default)]
    pub steal: StealSpec,
    #[serde(default = "default_true")]
    pub prefer_released: bool,
    #[serde(default = "default_steal_fade_ms")]
    pub fade_ms: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive: Option<u8>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub per_note: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub legato: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub returns: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_key: Option<u16>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub quieter_only: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub release: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BusSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub volume_db: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub pan: f32,
    #[serde(default = "default_true")]
    pub to_main: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSpec {
    #[default]
    Attack,
    Release,
    First,
    Legato,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinSpec {
    #[default]
    And,
    AndNot,
    Or,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RrScopeSpec {
    #[default]
    Global,
    Key,
    Group,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartRuleKindSpec {
    Keyswitch {
        range: [u8; 2],
    },
    Controller {
        cc: u8,
        range: [u8; 2],
    },
    RoundRobin {
        #[serde(default)]
        scope: RrScopeSpec,
        position: u8,
        length: u8,
        #[serde(default)]
        random: bool,
    },
    Cycle {
        #[serde(default)]
        random: bool,
    },
    Random {
        #[serde(default)]
        lo: u16,
        #[serde(default = "default_hirand")]
        hi: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRuleSpec {
    #[serde(default)]
    pub join: JoinSpec,
    #[serde(flatten)]
    pub rule: StartRuleKindSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeSpec {
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub delay: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub attack: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub hold: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub decay: f32,
    #[serde(default = "default_one_f32")]
    pub sustain: f32,
    #[serde(default = "default_release_s")]
    pub release: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub attack_shape: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub decay_shape: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub release_shape: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub one_shot: bool,
}

impl Default for EnvelopeSpec {
    fn default() -> Self {
        Self {
            delay: 0.0,
            attack: 0.0,
            hold: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: default_release_s(),
            attack_shape: 0.0,
            decay_shape: 0.0,
            release_shape: 0.0,
            one_shot: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteExprSpec {
    Pressure,
    Brightness,
    Vibrato,
    Expression,
    Tune,
    Gain,
    Pan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SourceSpec {
    Velocity,
    ReleaseVelocity,
    Controller {
        cc: u8,
    },
    ControllerFrom {
        cc: u8,
        start: u8,
    },
    PitchBend,
    Pressure,
    NoteExpr {
        expr: NoteExprSpec,
    },
    PerNoteCc {
        registered: bool,
        index: u8,
    },
    Key,
    Constant,
    HeldTime {
        span_ms: u32,
    },
    Random {
        #[serde(default)]
        bipolar: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "curve", rename_all = "snake_case")]
pub enum CurveSpec {
    Decibels { range: f32 },
    Depth { power: u8 },
    Squared,
    Falling,
    Signed,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scale", rename_all = "snake_case")]
pub enum TimeScaleSpec {
    Linear { power: u8 },
    LogKnob { floor_ms: f32, power: u8 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum ModKindSpec {
    Volume {
        #[serde(flatten)]
        source: SourceSpec,
        #[serde(default = "default_one_f32")]
        intensity: f32,
        #[serde(flatten)]
        curve: CurveSpec,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        tau_ms: f32,
    },
    Pitch {
        #[serde(flatten)]
        source: SourceSpec,
        semitones: f32,
        #[serde(default = "default_one_f32")]
        depth: f32,
        #[serde(default, skip_serializing_if = "is_zero_f32")]
        tau_ms: f32,
    },
    Pan {
        #[serde(flatten)]
        source: SourceSpec,
        amount: f32,
    },
    SampleStart {
        #[serde(flatten)]
        source: SourceSpec,
        #[serde(default = "default_one_f32")]
        depth: f32,
    },
    Release {
        #[serde(flatten)]
        source: SourceSpec,
        #[serde(default = "default_one_f32")]
        intensity: f32,
        #[serde(flatten)]
        scale: TimeScaleSpec,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    #[serde(flatten)]
    pub kind: ModKindSpec,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossfadeLawSpec {
    #[default]
    Linear,
    EqualPower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "control", rename_all = "snake_case")]
pub enum CrossfadeControlSpec {
    Velocity,
    Key,
    Cc { cc: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossfadeSpec {
    #[serde(flatten)]
    pub control: CrossfadeControlSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_in: Option<[i16; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out: Option<[i16; 2]>,
    #[serde(default)]
    pub law: CrossfadeLawSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoopSpec {
    pub start: u64,
    pub end: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub crossfade: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub count: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub alternating: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub until_release: bool,
    #[serde(default = "default_one_f32")]
    pub tune_ratio: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneSpec {
    #[serde(default)]
    pub sample: String,
    #[serde(default = "default_keys")]
    pub keys: [u8; 2],
    #[serde(default = "default_velocities")]
    pub velocities: [u8; 2],
    #[serde(default = "default_root")]
    pub root: u8,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub tune: f64,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub volume_db: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub pan: f32,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub start: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub start_mod: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reverse: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_fade: Option<[u8; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_fade: Option<[u8; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<LoopSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crossfades: Vec<CrossfadeSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub volume_db: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub pan: f32,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub tune: f64,
    #[serde(default = "default_one_f64")]
    pub keytrack: f64,
    #[serde(default)]
    pub trigger: TriggerSpec,
    #[serde(default, skip_serializing_if = "is_false")]
    pub one_shot: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polyphony: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_group: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_by: Option<usize>,
    #[serde(default)]
    pub bus: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<GeneratorSpec>,
    #[serde(default)]
    pub envelope: EnvelopeSpec,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub start: Vec<StartRuleSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<ModSpec>,
    #[serde(default)]
    pub zones: Vec<ZoneSpec>,
}
