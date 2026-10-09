//! Authored Kontakt object settings, retained separately from admitted playback
//! semantics. Indices are original source indices, including muted/missing
//! entries. The native lowerer does not consume these records automatically.

#[derive(Clone, Debug, PartialEq)]
pub struct Objects {
    pub program: Program,
    pub voice_groups: Option<VoiceGroups>,
    pub groups: Vec<Group>,
    pub zones: Vec<Zone>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub version: u16,
    pub name: String,
    pub num_bytes_samples_total: f64,
    pub transpose: i8,
    pub volume: f32,
    pub pan: f32,
    pub tune: f32,
    pub low_velocity: u8,
    pub high_velocity: u8,
    pub low_key: u8,
    pub high_key: u8,
    pub default_key_switch: i16,
    pub dfd_channel_preload_size: i32,
    pub group_solo: bool,
    pub library_id: i32,
    pub fingerprint: u32,
    pub loading_flags: u32,
    pub cat_icon_idx: i32,
    pub instrument_credits: String,
    pub instrument_author: String,
    pub instrument_url: String,
    pub instrument_cat1: i16,
    pub instrument_cat2: i16,
    pub instrument_cat3: i16,
    /// Bounded public suffix after the common prefix. The verified Program
    /// reader decodes its versioned layout; these bytes carry no guessed semantics.
    pub unknown_tail: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceLimit {
    pub name: String,
    pub kill_mode: i16,
    pub prefer_released: bool,
    pub max_num_voices: i32,
    pub ms_fade_time: i32,
    pub exclusion_group: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceGroups {
    pub program: VoiceLimit,
    /// Exactly 128 original slots; holes remain `None`.
    pub groups: Vec<Option<VoiceLimit>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub version: u16,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub tune: f32,
    pub key_tracking: bool,
    pub reverse: bool,
    pub release_trigger: bool,
    pub release_trigger_note_monophonic: bool,
    pub rls_trig_counter: i32,
    pub midi_channel: i16,
    /// 1-based assignment, zero unassigned. Do not confuse with zone owner.
    pub voice_group_index: i32,
    pub fx_idx_amp_split_point: i32,
    pub muted: bool,
    pub soloed: bool,
    pub interp_quality: i32,
    pub unknown_tail: Vec<u8>,
    pub criteria_mask: u8,
    pub criteria: Vec<Criterion>,
    pub criteria_unknown_tail: Vec<u8>,
    pub source: Option<Source>,
    pub source_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Criterion {
    pub mode: i32,
    pub next_criteria: i32,
    pub key_min: i16,
    pub key_max: i16,
    pub controller: i16,
    pub cc_min: i16,
    pub cc_max: i16,
    pub cycle_class: i32,
    pub slice_zone_idx: i32,
    pub slice_zone_slice_idx: i32,
    pub sequencer_only: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub flag: u8,
    pub version: u16,
    pub mode: u32,
    pub bytes: u16,
    pub fields: Vec<SourceField>,
    /// Group-private state after the bounded source record, kept in memory.
    pub private_tail: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceField {
    pub offset: u16,
    pub name: &'static str,
    pub value: SourceValue,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SourceValue {
    Float(f32),
    Integer(u32),
    Flag(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub version: u16,
    pub group: u32,
    pub sample_start: i32,
    /// Signed offset from the source end: zero means full source length.
    pub sample_end: i32,
    pub sample_start_mod_range: i32,
    pub low_velocity: i16,
    pub high_velocity: i16,
    pub low_key: i16,
    pub high_key: i16,
    pub fade_low_velocity: i16,
    pub fade_high_velocity: i16,
    pub fade_low_key: i16,
    pub fade_high_key: i16,
    pub root_key: i16,
    pub zone_volume: f32,
    pub zone_pan: f32,
    pub zone_tune: f32,
    pub filename_prefix: Option<[u8; 6]>,
    pub filename_id: i32,
    pub sample_data_type: i32,
    pub sample_rate: i32,
    pub num_channels: u8,
    pub num_frames: i32,
    pub reserved1: i32,
    pub reserved2: Option<i32>,
    pub root_note: i32,
    pub tuning: f32,
    pub reserved3: u8,
    pub reserved4: i32,
    pub unknown_tail: Vec<u8>,
    pub loops: Vec<Loop>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub slot: u8,
    pub mode: i32,
    pub loop_start: i32,
    pub loop_length: i32,
    pub loop_count: i32,
    pub alternating_loop: bool,
    pub loop_tuning: f32,
    pub x_fade_length: i32,
}

/// Original source/target settings before init writes and playback admission.
/// Physical identity lives in SourceModulator; target ordinals are vector indices.
#[derive(Clone, Debug, PartialEq)]
pub struct Modulation {
    pub version: u16,
    pub source: ModulationSource,
    pub targets: Vec<ModulationTarget>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum ModulationSource {
    Internal {
        flags: [u8; 4],
        unknown_id: u32,
        source: InternalSource,
    },
    External {
        source: ExternalSource,
        unknown_id: u32,
        unknown_source_data: Vec<u8>,
        unknown_tail: Vec<u8>,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub enum InternalSource {
    Ahdsr {
        attack_curve: f32,
        attack_ms: f32,
        hold_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
        ahd_only: u8,
        unknown_tail: Vec<u8>,
    },
    Flex {
        points: Vec<FlexPoint>,
        sustain: u32,
        unknown_index: u32,
        unknown_tail: Vec<u8>,
    },
    Lfo(Lfo),
    Other {
        chunk_id: u16,
    },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexPoint {
    /// Delta from the preceding point in milliseconds, not absolute time.
    pub time_ms: f32,
    pub level: f32,
    /// Serialized 0..1 curve, with 0.5 linear, not an exponential coefficient.
    pub curve: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Lfo {
    pub structured: bool,
    pub version: u16,
    pub waveform: u32,
    /// Delay, frequency/count, width, phase in original units.
    pub initial_values: [f32; 4],
    /// Packed fields cross native sync-record boundaries; preserve byte order.
    pub records: [LfoRecord; 2],
    pub trailing_flag: bool,
    pub trailing_values: Option<[f32; 5]>,
    pub additional_flag: Option<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfoRecord {
    pub flag: bool,
    pub values: [f32; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalSource {
    PitchBend,
    PolyAftertouch,
    MonoAftertouch,
    MidiCc(u8),
    KeyPosition,
    Velocity,
    ReleaseVelocity,
    ReleaseTriggerCounter,
    Constant,
    RandomUnipolar,
    RandomBipolar,
    Script(u32),
    Unassigned,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ModulationTarget {
    pub param: String,
    /// Saved magnitude; flags bit 1 stores depth sign independently of invert.
    pub intensity: f32,
    pub lag_ms: u16,
    pub name: String,
    /// Physical owning module slot, never a dense processor index.
    pub slot: Option<u8>,
    pub invert: bool,
    pub shaper: Option<ModulationShaper>,
    pub unknown_i16: i16,
    pub flags: u8,
}
impl ModulationTarget {
    pub fn signed_intensity(&self) -> f32 {
        if self.flags & 2 != 0 {
            -self.intensity
        } else {
            self.intensity
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ModulationShaper {
    pub enabled: bool,
    pub curve: ShaperCurve,
}
#[derive(Clone, Debug, PartialEq)]
pub enum ShaperCurve {
    Table(Vec<f32>),
    Breakpoints(Vec<ShaperPoint>),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaperPoint {
    pub x: f32,
    pub y: f32,
    pub curve: f32,
}
