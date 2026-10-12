//! Group modulation decoded from Kontakt presets, in engine-facing form.
//!
//! Field meanings and confidence are documented in `audits/MODULATION.md`.
//! Nothing here is applied to playback by itself.

use anyhow::Result;
use ni_file::kontakt::objects::{
    ExternalModArray32, Group as RawGroup, InternalModArray16, Modulator as RawModulator,
};
use serde::{Deserialize, Serialize};

use crate::import::Group;

pub use ni_file::kontakt::objects::{
    Breakpoint, EnvelopeAhdsr as Ahdsr, EnvelopeFlex as FlexEnvelope, FlexPoint, ModShaper,
    ModSource, ShaperCurve,
};

const INTERNAL_MODS_ID: u16 = 0x3B;
const EXTERNAL_MODS_ID: u16 = 0x3C;
/// Kontakt groups have sixteen internal-modulator slots (InternalModArray16).
pub(crate) const PITCH_ENVS: usize = 16;
/// Pitch modulation at intensity 1.0 spans one octave (PB_PITCH stores 2/12).
const PITCH_SEMITONES_PER_INTENSITY: f32 = 12.0;

/// Parameter driven by a modulation assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModTarget {
    /// Group amplitude (`volume`).
    Volume,
    /// Group pitch (`pitch`).
    Pitch,
    /// Another group parameter. Constant loopStart/loopLength are processed on
    /// eligible forward Source-mode-0 paths; other group destinations are retained.
    Group(String),
    /// Sample start position (`playPos`), scaled by the zone's `start_mod` range.
    SampleStart,
    /// Attack time of the group's volume AHDSR (`ahdsr_attack` on its slot).
    Attack,
    /// Release time of the group's volume AHDSR (`ahdsr_release` on its slot).
    Release,
    /// Parameter of the module in `slot` (group FX or internal modulator).
    Module { param: String, slot: u8 },
}

/// One external source driving one parameter of a group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModAssignment {
    /// KSP modulator name, e.g. `VEL_VOLUME`.
    pub name: String,
    /// Modulation source.
    pub source: ModSource,
    /// Saved CC substitute before accepted data; -1 disconnects the route.
    /// Absent metadata preserves the legacy cache behavior of authored routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cc_default: Option<i16>,
    /// Modulated parameter.
    pub target: ModTarget,
    /// Physical depth; pitch, filter cutoff and loop bounds include the saved target sign.
    /// Other target magnitudes retain their existing interpretation.
    pub intensity: f32,
    /// Invert button; its order relative to the shaper is unverified.
    pub invert: bool,
    /// Lag (smoothing) in milliseconds.
    pub lag_ms: u16,
    /// Enabled modulation shaper, if any.
    pub shaper: Option<ShaperCurve>,
}

impl ModAssignment {
    /// Apply the shaper (identity when absent) to a 0..=1 source value.
    pub fn shape(&self, value: f32) -> f32 {
        self.shaper
            .as_ref()
            .map_or(value.clamp(0.0, 1.0), |curve| curve.evaluate(value))
    }
}

/// A modulator as KSP addresses it: its position in `Group::modulators` is the
/// `find_mod` index (internal modulators in slot order, then external ones).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Modulator {
    /// `find_mod` name, e.g. `ENV_AHDSR` or `VEL_VOLUME`.
    pub name: String,
    /// `find_target` names, in target order.
    pub targets: Vec<String>,
    /// First of this external modulator's entries in `Group::mods` (one per
    /// target); `None` for internal modulators.
    pub assignments: Option<usize>,
    /// This is the internal envelope imported as `Group::volume_env` (AHDSR
    /// only, the target of `$ENGINE_PAR_ATTACK` and friends).
    pub volume_env: bool,
    /// Saved internal source bypass, independent of its target assignments.
    #[serde(default)]
    pub bypassed: bool,
    /// A flex envelope: it has no AHDSR stages, so Kontakt ignores
    /// `$ENGINE_PAR_ATTACK` and friends addressed to it.
    #[serde(default)]
    pub flex: bool,
    /// Selected first Flex source driving volume, imported as `Group::flex_env`.
    /// Other Flex sources keep their roster identity without amplitude ownership.
    #[serde(default)]
    pub primary_flex: bool,
    /// Index into `Group::envelopes` of an AHDSR driving pitch or module parameters.
    #[serde(default)]
    pub envelope: Option<usize>,
    /// Modulator kind, for audits: `ahdsr`, `flex`, `lfo`, `chunk 0xNN` (an
    /// undecoded internal modulator), `external`, or `undecoded` (an occupied
    /// slot whose parameters could not be read; its empty name is not usable).
    #[serde(default)]
    pub kind: String,
}

/// Native synchronized fade timing, retaining the authored finite coefficient.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LfoFadeSync {
    pub note_value: f32,
    pub count: f32,
}

/// Saved retriggered Multi source driving pitch.
/// This is a bounded implemented subset, not a fallback for other LFO states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchLfo {
    /// Strict saved Digital source shared by its group/original slot.
    /// Absent in older caches, which retain their per-voice source owner.
    #[serde(default)]
    pub shared: bool,
    /// Saved normalized cycle position, copied unchanged into the retriggered
    /// native source phase. Shared Digital starts at constructor phase0; the
    /// saved phase remains reset-only metadata. Live phase writes are unadmitted.
    #[serde(default)]
    pub start_phase: f32,
    pub slot: u8,
    /// Synchronized cycle count, or frequency in Hz when synchronization is disabled.
    pub count: f32,
    /// Saved synchronization note value; values at or below 1e-9 disable synchronization.
    pub note_value: f32,
    pub sine: f32,
    /// Arithmetic Multi Digital components; absent in legacy cached sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digital: Option<DigitalLfo>,
    /// Unsynchronized legacy fade-in duration in milliseconds.
    #[serde(default)]
    pub fade_ms: f32,
    /// Native fade timing synchronization; fade_ms is zero when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_sync: Option<LfoFadeSync>,
    /// Native v73 fade state uses the extended .3..1.3 clamp.
    /// Earlier cached/source versions retain the legacy 0..1 clamp.
    #[serde(default)]
    pub extended_fade: bool,
    pub depth: f32,
    /// Original native pitch-target indices and individual signed depths.
    /// Empty in older cached metadata: live writes must remain unsupported.
    #[serde(default)]
    pub targets: Vec<(u32, f32)>,
    pub bypassed: bool,
}

/// Deterministic saved Multi Digital source. Random remains unadmitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DigitalLfo {
    pub rectangle: f32,
    pub triangle: f32,
    pub saw: f32,
    pub normalize: bool,
    pub pulse_width: f32,
}

impl PitchLfo {
    pub(crate) fn frequency(&self, tempo: f32) -> f32 {
        if self.note_value <= 1e-9 { return self.count; }
        let tempo = if tempo.is_finite() && tempo >= 0.1 { tempo } else { 120. };
        let duration = ((self.note_value * 60.) / tempo) * self.count.max(1.);
        duration.recip().clamp(f32::from_bits(0x3c2c0831), f32::from_bits(0x4355199a))
    }
}

/// One admitted saved Multi source driving one native volume target.
/// Timing is shared with the pitch path at the same native internal slot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VolumeLfo {
    pub source: PitchLfo,
    pub target: u32,
    pub intensity: f32,
    pub negative: bool,
    pub lag_ms: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlideMode { Speed, Time }

/// Verified retriggered, unsynchronized native Glide pitch source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchGlide {
    pub slot: u8,
    pub mode: GlideMode,
    pub time_ms: f32,
    /// Sum of the signed normalized-octave target depths.
    pub depth: f32,
    pub targets: Vec<(u32, f32)>,
    pub bypassed: bool,
}

/// Modulation read from one group, plus notes about what was left out.
#[derive(Debug, Default)]
pub(crate) struct GroupModulation {
    pub volume_env: Option<Ahdsr>,
    pub native_volume_env: bool,
    pub native_shared_volume_slot: Option<u8>,
    pub native_primary_bypass_slot: Option<u8>,
    pub flex_env: Option<FlexEnvelope>,
    pub native_flex_oneshot: bool,
    pub native_flex_loop: bool,
    pub mods: Vec<ModAssignment>,
    pub modulators: Vec<Modulator>,
    pub envelopes: Vec<ModEnvelope>,
    pub pitch_lfos: Vec<PitchLfo>,
    pub volume_lfos: Vec<VolumeLfo>,
    pub pitch_glides: Vec<PitchGlide>,
    pub warnings: Vec<String>,
}

/// An internal AHDSR driving pitch or module parameters (filter cutoff, EQ gain...),
/// not volume. Targets use `ModSource::Unassigned`; the envelope is the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModEnvelope {
    pub env: Ahdsr,
    pub targets: Vec<ModAssignment>,
    /// Saved v11 AHD mode admitted separately from the generic source law.
    #[serde(default)]
    pub native_ahd_only: bool,
}

/// Read internal and external modulators from a group's child chunks.
pub(crate) fn read_group(group: &RawGroup) -> Result<GroupModulation> {
    read_group_impl(group, None)
}

/// Ordinary imports can retain independent, readable slots. Snapshot overlays
/// use `read_group` so a corrupt/incompatible record rejects the transaction.
pub(crate) fn read_group_partial(group: &RawGroup, index: usize, name: &str) -> Result<GroupModulation> {
    read_group_impl(group, Some((index, name)))
}

fn read_group_impl(group: &RawGroup, recover: Option<(usize, &str)>) -> Result<GroupModulation> {
    let mut out = GroupModulation::default();
    let mut amplitude_targets = 0usize;
    let mut complete_sources = true;
    let (mut internal_sources, mut external_sources) = (0usize, 0usize);
    let mut shared_primary = None;
    let mut primary_ahdsr_warning = None;

    // Internal-modulator slot of the volume AHDSR, which module targets address.
    let mut volume_env_slot = None;
    if let Some(chunk) = group.0.find_first(INTERNAL_MODS_ID) {
        let mut skipped = 0;
        let mut skipped_lfos = 0;
        for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
            internal_sources += 1;
            // Script source indices must match the native sixteen-slot array,
            // including holes before an occupied source. DSP and assignments
            // already retain this slot; compressing only the lookup roster
            // makes a named sparse LFO's live controls address another source.
            out.modulators.resize_with(slot, || Modulator {
                name: String::new(), targets: Vec::new(), assignments: None,
                volume_env: false, bypassed: false, flex: false, primary_flex: false, envelope: None,
                kind: "undecoded".into(),
            });
            let params = match modulator.params() {
                Ok(params) => params,
                Err(error) => {
                    let Some(identity) = recover else { return Err(error.into()); };
                    complete_sources = false;
                    out.failed_slot(identity, "internal", slot, modulator.0.version, &error);
                    continue;
                }
            };
            amplitude_targets += params.targets.iter().filter(|t| t.param == "volume").count();
            // The first volume envelope of each kind; the voice multiplies them.
            let volume = params.targets.iter().any(|t| t.param == "volume");
            if let RawModulator::Ahdsr(env) = &params.modulator
                && env.unknown_flag != 0 && volume && out.volume_env.is_some() {
                out.warnings.push("AHDSR mode switches outside the admitted primary volume source are not applied".into());
            }
            let flex = matches!(params.modulator, RawModulator::Flex(_));
            let kind = match params.modulator {
                RawModulator::Ahdsr(_) => "ahdsr".to_owned(),
                RawModulator::Flex(_) => "flex".to_owned(),
                RawModulator::Lfo(_) => "lfo".to_owned(),
                RawModulator::Glide(_) => "glide".to_owned(),
                RawModulator::Other { chunk_id } => format!("chunk 0x{chunk_id:02x}"),
            };
            let envelope = (matches!(params.modulator, RawModulator::Ahdsr(_)) && !volume)
                .then_some(out.envelopes.len());
            let mut primary_flex = false;
            let volume_env = match params.modulator {
                RawModulator::Ahdsr(env) if volume && out.volume_env.is_none() => {
                    // Typed 0x3f/v11 scalar law and source clock are proven for
                    // this exact untransformed primary geometry. Packed note-
                    // value records remain opaque; only their saved defaults
                    // are admitted, never inferred as synchronized timing.
                    let native_wrapper = modulator.0.private_data.ends_with(&2u32.to_le_bytes())
                        && modulator.0.find_first(7)
                            .and_then(|c| ni_file::kontakt::StructuredObject::try_from(c).ok())
                            .is_some_and(|w| w.version == 0x90 && w.private_data.is_empty()
                                && w.public_data == [0; 4] && w.children.len() == 1
                                && w.children[0].id == 0x3f);
                    let default_time = env.unknown_tail.len() == 52
                        && env.unknown_tail.chunks_exact(13).all(|r|
                            r == [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0]);
                    let plain = native_wrapper && params.targets.len() == 1 && params.targets.iter().all(|t|
                        t.param == "volume" && t.slot.is_none() && t.intensity == 1.
                        && t.unknown_i16 == -1 && t.unknown_flags == 0x10 && !t.invert && t.lag_ms == 0
                        && !t.shaper.as_ref().is_some_and(|s| s.enabled))
                        && params.unknown_flags[0] <= 1 && env.unknown_flag <= 1 && default_time;
                    out.native_volume_env = plain && params.unknown_flags[1..] == [0, 1, 0];
                    if plain && env.unknown_flag == 0 && params.unknown_flags[1..] == [0, 0, 0] {
                        shared_primary = u8::try_from(slot).ok();
                    }
                    if !out.native_volume_env {
                        primary_ahdsr_warning = Some(out.warnings.len());
                        out.warnings.push(format!("Primary AHDSR slot {slot}: native source kernel not applied; requires the admitted v90-kind0 wrapper/category2 and v11 source flag geometry, one unity uninverted volume target, zero lag, known flags, no enabled shaper and default opaque timing records"));
                    } else {
                        out.warnings.push(format!("Primary AHDSR slot {slot}: native finite-stage kernel and 32-frame amplitude interpolation admitted; held-voice retargeting remains unsupported; live primary bypass requires separately validated sampler ownership"));
                    }
                    out.volume_env = Some(env);
                    volume_env_slot = Some(slot);
                    true
                }
                RawModulator::Flex(env) if volume && out.flex_env.is_none() => {
                    primary_flex = true;
                    // v12 reads Loop and Oneshot separately before its timing
                    // record. Admit the independently traced mode only; the
                    // existing Flex startup/curve arithmetic remains separate.
                    let native_wrapper = modulator.0.private_data.ends_with(&2u32.to_le_bytes())
                        && modulator.0.find_first(7)
                            .and_then(|c| ni_file::kontakt::StructuredObject::try_from(c).ok())
                            .is_some_and(|w| w.version == 0x90 && w.private_data.is_empty()
                                && w.public_data == 1u32.to_le_bytes() && w.children.len() == 1
                                && w.children[0].id == 0x40);
                    let mode = env.loop_and_oneshot();
                    let admitted_source = native_wrapper
                        && mode.is_some()
                        && env.unknown_tail[2..] == [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0]
                        && env.points.last().is_some_and(|p| p.level == 0.)
                        && env.unknown_index <= env.sustain
                        && (env.sustain as usize) < env.points.len()
                        && params.targets.len() == 1 && params.targets.iter().all(|t|
                            t.param == "volume" && t.slot.is_none() && t.intensity == 1.
                            && t.unknown_i16 == -1 && t.unknown_flags == 0x10
                            && !t.invert && t.lag_ms == 0
                            && !t.shaper.as_ref().is_some_and(|s| s.enabled))
                        && params.unknown_flags[0] <= 1 && params.unknown_flags[1..] == [0, 1, 0];
                    out.native_flex_oneshot = admitted_source && mode == Some((false, true));
                    out.native_flex_loop = admitted_source && mode == Some((true, false))
                        && env.sustain - env.unknown_index >= 2;
                    out.warnings.push(if out.native_flex_oneshot {
                        format!("Primary Flex slot {slot}: saved v12 Oneshot/Loop-disabled mode admitted; existing Flex startup and curve arithmetic remain approximate")
                    } else if out.native_flex_loop {
                        format!("Primary Flex slot {slot}: saved v12 wider-section Loop/Oneshot-disabled mode admitted; loop return retains the current level and release enters the post-loop points; existing Flex startup and curve arithmetic remain approximate")
                    } else {
                        format!("Primary Flex slot {slot}: native Oneshot/Loop mode not applied; requires the admitted v90-kind1 wrapper/category2, v12 Loop-disabled Oneshot or wider-section Loop with Oneshot disabled, zero endpoint, default unsynchronized timing, active retriggered source and one uninverted unity volume target without lag or enabled shaper")
                    });
                    out.flex_env = Some(env);
                    false
                }
                RawModulator::Ahdsr(env) if !volume => {
                    let native_wrapper = modulator.0.private_data.ends_with(&2u32.to_le_bytes())
                        && modulator.0.find_first(7)
                            .and_then(|c| ni_file::kontakt::StructuredObject::try_from(c).ok())
                            .is_some_and(|w| w.version == 0x90 && w.private_data.is_empty()
                                && w.public_data == [0; 4] && w.children.len() == 1
                                && w.children[0].id == 0x3f);
                    let native_ahd_only = native_wrapper && env.unknown_flag == 1
                        && env.unknown_tail.len() == 52 && env.unknown_tail.chunks_exact(13).all(|r|
                            r == [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0])
                        && params.unknown_flags[0] <= 1 && params.unknown_flags[1] <= 1
                        && params.unknown_flags[2..] == [1, 0]
                        && !params.targets.is_empty() && params.targets.iter().all(|t|
                            matches!((t.param.as_str(), t.slot), ("pitch", None)
                                | ("filterCutoff" | "filterQ", Some(_)))
                            && t.intensity.is_finite() && t.unknown_i16 == -1 && t.unknown_flags == 0x10
                            && !t.invert && t.lag_ms == 0 && !t.shaper.as_ref().is_some_and(|s| s.enabled));
                    if env.unknown_flag != 0 {
                        out.warnings.push(if native_ahd_only {
                            format!("AHDSR slot {slot}: saved retriggered AHD Only mode admitted for pitch/filter routes; generic source timing and curves remain approximate")
                        } else {
                            format!("AHDSR slot {slot}: saved AHD mode not applied; requires the admitted v90-kind0/category2 wrapper, v11 AHD byte1, default timing records, retrigger1 and known uninverted pitch/filter targets without lag or enabled shaper; shared retrigger0 ownership remains unsupported")
                        });
                    } else if params.unknown_flags[2] == 0 {
                        out.warnings.push(format!("AHDSR slot {slot}: saved retrigger0 shared source ownership not applied; generic per-voice AHDSR retained"));
                    }
                    let targets: Vec<_> = params
                        .targets
                        .iter()
                        .filter_map(|t| {
                            Some(ModAssignment {
                                cc_default: None,
                                name: params.name.clone(),
                                source: ModSource::Unassigned,
                                target: match (t.param.as_str(), t.slot) {
                                    ("pitch", None) => ModTarget::Pitch,
                                    (_, Some(slot)) => ModTarget::Module {
                                        param: t.param.clone(),
                                        slot,
                                    },
                                    _ => ModTarget::Group(t.param.clone()),
                                },
                                intensity: target_depth(t),
                                invert: t.invert,
                                lag_ms: t.lag_ms,
                                shaper: t.shaper.clone().filter(|s| s.enabled).map(|s| s.curve),
                            })
                        })
                        .collect();
                    out.envelopes.push(ModEnvelope { env, targets, native_ahd_only });
                    false
                }
                RawModulator::Glide(glide) => {
                    let mode = match glide.mode { 1 => Some(GlideMode::Speed), 2 => Some(GlideMode::Time), _ => None };
                    // Native category5 directly selects BParGlide. The
                    // category2 envelope wrapper accepts only kinds0..2.
                    let native_source = modulator.0.private_data.ends_with(&5u32.to_le_bytes())
                        && modulator.0.public_data.is_empty() && modulator.0.children.len() == 1
                        && modulator.0.children[0].id == 0x0B;
                    let timing = match glide.version {
                        0x50 => glide.unknown_tail.is_empty(),
                        0x51 => glide.unknown_tail == [0, 0, 0, 0, 0, 0, 128, 63, 1],
                        _ => false,
                    };
                    let depth: f32 = params.targets.iter().map(target_depth).sum();
                    let admitted = native_source && timing && mode.is_some()
                        && glide.coefficient.is_finite() && glide.coefficient > 0.
                        && glide.sync_note_value.is_finite() && glide.sync_note_value <= 1e-9f32
                        && params.unknown_flags[0] <= 1 && params.unknown_flags[1] <= 1
                        && params.unknown_flags[2..] == [1, 0]
                        && depth.is_finite() && !params.targets.is_empty()
                        && params.targets.iter().all(|t| t.param == "pitch" && t.slot.is_none()
                            && t.intensity.is_finite() && t.intensity >= 0.
                            && t.unknown_i16 == -1 && matches!(t.unknown_flags, 0x10 | 0x12)
                            && !t.invert && t.lag_ms == 0 && !t.shaper.as_ref().is_some_and(|s| s.enabled));
                    if admitted {
                        out.pitch_glides.push(PitchGlide { slot: slot as u8, mode: mode.unwrap(),
                            time_ms: glide.coefficient, depth,
                            targets: params.targets.iter().enumerate().map(|(i, t)| (i as u32, target_depth(t))).collect(),
                            bypassed: params.unknown_flags[1] != 0 });
                        out.warnings.push(format!("Internal Glide slot {slot}: saved retriggered unsynchronized pitch is eligible for ordinary sampler playback; synchronized/shared sources and live timing controls remain unsupported; exponential conversion uses the engine's existing floating-point approximation"));
                    } else {
                        out.warnings.push(format!("Internal Glide slot {slot} not applied: requires the native category5/direct0B source, v50/51 Speed or Time, positive finite unsynchronized timing with default opaque fields, retrigger1 and plain signed pitch-only targets; shared sources, synchronization, zero timing, inversion, lag and shaping remain unsupported"));
                    }
                    false
                }
                RawModulator::Lfo(lfo) => {
                    // Retain strict diagnostics for every state outside the
                    // independently established saved-only source clock.
                    let weights = lfo.trailing_values.unwrap_or([0.; 5]);
                    let deterministic = weights.iter().all(|v| v.is_finite() && v.abs() <= 1.);
                    let digital = (lfo.waveform == 6 && deterministic && weights[4] == 0.
                        && lfo.initial_values[2].is_finite()).then(|| DigitalLfo {
                            rectangle: weights[1], triangle: weights[2], saw: weights[3],
                            normalize: lfo.records[0].flag, pulse_width: lfo.initial_values[2].clamp(0.05, 0.95),
                        });
                    let waveform_supported = (lfo.waveform == 5 && weights[0].is_finite()
                        && weights[0].abs() <= 1. && weights[1..].iter().all(|&v| v == 0.))
                        || digital.is_some();
                    let source_name = if digital.is_some() { "deterministic Multi Digital" } else { "sine-only Multi" };
                    let frequency_note = lfo.records[0].values[0];
                    let coefficient = lfo.initial_values[1];
                    // The native synchronized frequency converter floors its
                    // coefficient at one; retain the authored value in the source.
                    let native_count_floor = !lfo.structured
                        && params.unknown_flags[0] <= 1 && params.unknown_flags[1] <= 1
                        && params.unknown_flags[2..] == [1, 0]
                        && modulator.0.private_data.ends_with(&1u32.to_le_bytes())
                        && modulator.0.public_data.is_empty() && modulator.0.children.len() == 1
                        && modulator.0.children[0].id == 0x08 && !lfo.trailing_flag
                        && lfo.records.iter().all(|r| r.values[1..] == [0., 0.])
                        && !params.targets.is_empty() && params.targets.iter().all(|t|
                            matches!((t.param.as_str(), t.slot), ("pitch" | "volume", None))
                            && t.intensity.is_finite() && t.intensity >= 0. && t.unknown_i16 == -1
                            && matches!(t.unknown_flags, 0x10 | 0x12)
                            && !t.invert && !t.shaper.as_ref().is_some_and(|s| s.enabled)
                            && if t.param == "pitch" { t.lag_ms == 0 } else { (t.lag_ms as i16) >= 0 });
                    let shared = params.unknown_flags[2] == 0 && digital.is_some()
                        && (frequency_note <= 1e-9 || !lfo.structured)
                        && lfo.initial_values[0] == 0.
                        && params.unknown_flags[0] <= 1 && params.unknown_flags[1] <= 1
                        && params.unknown_flags[3] == 0
                        && modulator.0.private_data.ends_with(&1u32.to_le_bytes())
                        && modulator.0.public_data.is_empty() && modulator.0.children.len() == 1
                        && modulator.0.children[0].id == 0x08
                        && !lfo.trailing_flag && lfo.additional_flag.is_none()
                        && lfo.records.iter().all(|r| r.values[1..] == [0., 0.])
                        && !params.targets.is_empty() && params.targets.iter().all(|t|
                            matches!((t.param.as_str(), t.slot), ("pitch" | "volume", None))
                            && t.intensity.is_finite() && t.intensity >= 0. && t.unknown_i16 == -1
                            && matches!(t.unknown_flags, 0x10 | 0x12)
                            && !t.invert && !t.shaper.as_ref().is_some_and(|s| s.enabled)
                            && if t.param == "pitch" { t.lag_ms == 0 } else { (t.lag_ms as i16) >= 0 });
                    // The same converter runs before the native shared/reset
                    // branch. Its default count floor also applies to this
                    // already-validated shared zero-fade source geometry.
                    let frequency_supported = frequency_note.is_finite() && coefficient.is_finite()
                        && if frequency_note > 1e-9 { coefficient >= 1. || native_count_floor || shared } else {
                            (f32::from_bits(0x3c2c0831)..=f32::from_bits(0x4355199a)).contains(&coefficient)
                        };
                    let version_supported = match (lfo.version, lfo.additional_flag) {
                        (0x71, None) => true,
                        (0x73, Some(_)) => params.unknown_flags[0] <= 1
                            && params.unknown_flags[1] <= 1 && params.unknown_flags[2..] == [1, 0]
                            && modulator.0.private_data.ends_with(&1u32.to_le_bytes())
                            && modulator.0.public_data.is_empty() && modulator.0.children.len() == 1
                            && modulator.0.children[0].id == 0x08
                            && !lfo.trailing_flag && lfo.records.iter().all(|r| r.values[1..] == [0., 0.]),
                        _ => false,
                    };
                    let extended_fade = lfo.additional_flag == Some(true);
                    let fade_note = lfo.records[1].values[0];
                    let sync_fade = fade_note > 1e-9;
                    let fade_supported = fade_note.is_finite() && lfo.initial_values[0].is_finite()
                        && if sync_fade {
                            !shared
                                && params.unknown_flags[0] <= 1 && params.unknown_flags[1] <= 1
                                && params.unknown_flags[2..] == [1, 0]
                                && modulator.0.private_data.ends_with(&1u32.to_le_bytes())
                                && modulator.0.public_data.is_empty() && modulator.0.children.len() == 1
                                && modulator.0.children[0].id == 0x08 && !lfo.trailing_flag
                                && lfo.records.iter().all(|r| r.values[1..] == [0., 0.])
                        } else { (0. ..=5000.).contains(&lfo.initial_values[0]) };
                    let supported = version_supported && waveform_supported
                        && (params.unknown_flags[2] != 0 || shared)
                        && fade_supported
                        && lfo.initial_values[3].is_finite() && (0. ..=1.).contains(&lfo.initial_values[3])
                        && frequency_supported
                        && lfo.records[1].flag;
                    let pitch: Vec<_> = params.targets.iter().enumerate()
                        .filter(|(_, t)| t.param == "pitch" && t.slot.is_none()).collect();
                    let depth: f32 = pitch.iter().map(|(_, t)| target_depth(t)).sum();
                    let pitch_admitted = supported && depth.is_finite() && !pitch.is_empty() && pitch.iter().all(|(_, t)| !t.invert && t.lag_ms == 0
                        && !t.shaper.as_ref().is_some_and(|s| s.enabled) && target_depth(t).is_finite());
                    if pitch_admitted {
                        out.pitch_lfos.push(PitchLfo { shared, start_phase: lfo.initial_values[3], slot: slot as u8,
                            count: lfo.initial_values[1], note_value: lfo.records[0].values[0],
                            sine: weights[0], digital: digital.clone(), fade_ms: if sync_fade { 0. } else { lfo.initial_values[0] }, fade_sync: sync_fade.then_some(LfoFadeSync { note_value: fade_note, count: lfo.initial_values[0] }), extended_fade, depth,
                            targets: pitch.iter().map(|(i, t)| (*i as u32, target_depth(t))).collect(),
                            bypassed: params.unknown_flags[1] != 0 });
                        out.warnings.push(format!("Internal LFO slot {slot}: saved {} {source_name} pitch with synchronized count or unsynchronized Hz and saved fade-in is eligible for ordinary sampler playback; live source bypass is supported for this admitted pitch path; live LFO timing and unadmitted target configurations remain unsupported", if shared { "shared" } else { "retriggered" }));
                    } else if !pitch.is_empty() {
                        out.warnings.push(format!("Internal LFO slot {slot} pitch not applied: source clock or target depth/inversion/lag/shaper is unsupported"));
                    }
                    let volume: Vec<_> = params.targets.iter().enumerate()
                        .filter(|(_, t)| t.param == "volume" && t.slot.is_none()).collect();
                    // The native internal array has16 sources. One target per
                    // source is an explicit consumer subset, not a format cap.
                    let admitted = supported && volume.len() == 1
                        && volume.iter().all(|(_, t)| t.intensity.is_finite() && t.intensity >= 0.
                            && !t.invert && (t.lag_ms as i16) >= 0
                            && t.unknown_flags & !0x02 == 0x10
                            && !t.shaper.as_ref().is_some_and(|s| s.enabled));
                    if admitted {
                        let (target, t) = volume[0];
                        out.volume_lfos.push(VolumeLfo {
                            source: PitchLfo { shared, start_phase: lfo.initial_values[3], slot: slot as u8, count: lfo.initial_values[1],
                                note_value: lfo.records[0].values[0], sine: weights[0], digital: digital.clone(), fade_ms: if sync_fade { 0. } else { lfo.initial_values[0] }, fade_sync: sync_fade.then_some(LfoFadeSync { note_value: fade_note, count: lfo.initial_values[0] }), extended_fade,
                                depth: 0., targets: Vec::new(), bypassed: params.unknown_flags[1] != 0 },
                            target: target as u32, intensity: t.intensity,
                            negative: t.unknown_flags & 0x02 != 0, lag_ms: t.lag_ms as i16,
                        });
                        out.warnings.push(format!("Internal LFO slot {slot}: saved {} {source_name} volume with synchronized count or unsynchronized Hz, saved fade-in and zero/positive lag is eligible for ordinary sampler playback; live source bypass and target intensity are supported; live timing and unadmitted targets remain unsupported", if shared { "shared" } else { "retriggered" }));
                    } else if !volume.is_empty() {
                        out.warnings.push(format!("Internal LFO slot {slot} volume not applied: requires one saved admitted deterministic Multi target with finite cycle phase in 0..1, known v71/v73 fade timing and mode, nonnegative signed lag, known polarity flags, no separate inversion or enabled shaper"));
                    }
                    if !pitch_admitted && !admitted { skipped_lfos += 1; }
                    if params.targets.iter().any(|t| !matches!((t.param.as_str(), t.slot),
                        ("pitch" | "volume", None))) {
                        out.warnings.push(format!("Internal LFO slot {slot}: target assignments other than admitted pitch and volume are not applied"));
                    }
                    false
                }
                _ => {
                    skipped += 1;
                    false
                }
            };
            out.modulators.push(Modulator {
                name: params.name,
                targets: params.targets.into_iter().map(|t| t.name).collect(),
                assignments: None,
                volume_env,
                bypassed: params.unknown_flags[1] != 0,
                flex,
                primary_flex,
                envelope,
                kind,
            });
        }
        if skipped_lfos > 0 {
            out.warnings.push(format!("{skipped_lfos} internal LFO sources are identified; their timing, waveforms and target assignments are not applied"));
        }
        if skipped > 0 {
            out.warnings.push(
                "Internal modulators other than AHDSR and the first flex volume envelope are not applied".into(),
            );
        }
    }

    if let Some(chunk) = group.0.find_first(EXTERNAL_MODS_ID) {
        for (slot, assignment) in ExternalModArray32::try_from(chunk)?.slots()? {
            external_sources += 1;
            let params = match assignment.params() {
                Ok(params) => params,
                Err(error) => {
                    let Some(identity) = recover else { return Err(error.into()); };
                    complete_sources = false;
                    out.failed_slot(identity, "external", slot, assignment.0.version, &error);
                    continue;
                }
            };
            amplitude_targets += params.targets.iter().filter(|t| t.param == "volume").count();
            if !params.unknown_tail.is_empty() {
                out.warnings.push(format!("External modulation v0x{:X} footer fields are retained but not applied", assignment.0.version));
            }
            // Native BParExternalMod +0x558 is the first signed word after
            // the source address. The following source flags remain opaque.
            let cc_default = if matches!(params.source, ModSource::MidiCc(_)) {
                Some(i16::from_le_bytes(params.unknown_source_data[..2].try_into().unwrap()))
            } else { None };
            out.modulators.push(Modulator {
                name: params.name.clone(),
                targets: params.targets.iter().map(|t| t.name.clone()).collect(),
                assignments: Some(out.mods.len()),
                volume_env: false, bypassed: false,
                flex: false, primary_flex: false,
                envelope: None,
                kind: "external".into(),
            });
            for target in params.targets {
                let intensity = target_depth(&target);
                out.mods.push(ModAssignment {
                    name: params.name.clone(),
                    source: params.source,
                    cc_default,
                    target: match (target.param.as_str(), target.slot) {
                        ("volume", None) => ModTarget::Volume,
                        ("pitch", None) => ModTarget::Pitch,
                        ("playPos", None) => ModTarget::SampleStart,
                        ("ahdsr_attack", Some(slot)) if volume_env_slot == Some(slot.into()) => {
                            ModTarget::Attack
                        }
                        ("ahdsr_release", Some(slot)) if volume_env_slot == Some(slot.into()) => {
                            ModTarget::Release
                        }
                        (_, Some(slot)) => ModTarget::Module {
                            param: target.param,
                            slot,
                        },
                        (_, None) => ModTarget::Group(target.param),
                    },
                    intensity,
                    invert: target.invert,
                    lag_ms: target.lag_ms,
                    shaper: target
                        .shaper
                        .filter(|shaper| shaper.enabled)
                        .map(|shaper| shaper.curve),
                });
            }
        }
    }

    // External frequency/phase/weight assignments invalidate this narrow
    // saved-only clock; never silently substitute its initial state.
    out.pitch_lfos.retain(|lfo| {
        let driven = out.mods.iter().any(|m| match &m.target {
            // Effect and internal-source slots are separate namespaces.
            // A recognized insert target cannot alter the LFO at that number.
            ModTarget::Module { param, slot } if *slot == lfo.slot =>
                crate::engine::filter::Knob::parse(param).is_none()
                    && crate::engine::filter::stage_knob(param).is_none(),
            _ => false,
        });
        if driven { out.warnings.push(format!("Internal LFO slot {} pitch not applied: external source controls its parameters", lfo.slot)); }
        !driven
    });
    out.volume_lfos.retain(|lfo| {
        let driven = out.mods.iter().any(|m| match &m.target {
            ModTarget::Module { param, slot } if *slot == lfo.source.slot =>
                crate::engine::filter::Knob::parse(param).is_none()
                    && crate::engine::filter::stage_knob(param).is_none(),
            _ => false,
        });
        if driven { out.warnings.push(format!("Internal LFO slot {} volume not applied: external source controls its parameters", lfo.source.slot)); }
        !driven
    });
    out.pitch_glides.retain(|glide| {
        let driven = out.mods.iter().any(|m| match &m.target {
            ModTarget::Module { param, slot } if *slot == glide.slot =>
                crate::engine::filter::Knob::parse(param).is_none()
                    && crate::engine::filter::stage_knob(param).is_none(),
            _ => false,
        });
        if driven { out.warnings.push(format!("Internal Glide slot {} not applied: external source controls its parameters", glide.slot)); }
        !driven
    });
    // This proof uses the original occupied records, including records whose
    // decoded targets would otherwise be omitted. Sharing cannot infer an empty
    // source roster from filtered DSP vectors.
    if complete_sources && internal_sources == 1 && external_sources == 0
        && amplitude_targets == 1 && out.flex_env.is_none() {
        out.native_shared_volume_slot = shared_primary;
    }
    if let (Some(slot), Some(warning)) = (out.native_shared_volume_slot, primary_ahdsr_warning) {
        out.warnings[warning] = format!("Primary AHDSR slot {slot}: native shared source kernel and 32-frame amplitude interpolation admitted only through sealed script-free resident unlooped Sampler ranges with identity ingress; KSP, delayed/streamed/looped sources and release-trigger siblings remain unsupported");
    }
    // Count actual native parameter addresses, never authored target labels.
    // Active saved source admission already proves retrigger and target geometry.
    if complete_sources && amplitude_targets == 1 && out.mods.is_empty()
        && ((out.native_volume_env && out.flex_env.is_none())
            || ((out.native_flex_oneshot || out.native_flex_loop) && out.volume_env.is_none())) {
        out.native_primary_bypass_slot = out.modulators.iter().position(|m|
            m.volume_env || m.primary_flex).and_then(|slot| u8::try_from(slot).ok());
    }
    Ok(out)
}

fn target_depth(target: &ni_file::kontakt::objects::ModTarget) -> f32 {
    // The native signed-target setter writes abs(depth) and sets target bit 1
    // for negative values. The shared target reader/writer confirms this is
    // unknown_flags, separately from invert. Apply it only to the pitch and
    // filter-cutoff and loop target laws independently established by native records.
    let signed = matches!((target.param.as_str(), target.slot),
        ("pitch", None) | ("filterCutoff", Some(_)) | ("loopStart" | "loopLength", None));
    if signed && target.unknown_flags & 0x02 != 0 {
        -target.intensity
    } else {
        target.intensity
    }
}

impl GroupModulation {
    fn failed_slot(&mut self, (index, name): (usize, &str), source: &str, slot: usize, version: u16, error: &ni_file::Error) {
        // Occupied slots still count in find_mod's order. Never assign an
        // invented name/source to a record whose parameters could not be read.
        self.modulators.push(Modulator {
            name: String::new(), targets: Vec::new(), assignments: None,
            volume_env: false, bypassed: false, flex: false, primary_flex: false, envelope: None, kind: "undecoded".into(),
        });
        self.warnings.push(format!("Group {index} {name:?}: {source} modulation slot {slot}, version 0x{version:x}, not imported: {error}; independent readable slots are retained"));
    }
}

impl Group {
    /// First assignment of `source` to `target`.
    pub fn modulation(&self, source: ModSource, target: &ModTarget) -> Option<&ModAssignment> {
        self.mods
            .iter()
            .find(|m| m.source == source && &m.target == target)
    }

    /// Largest stored velocity-to-volume intensity; 0.0 means velocity does not
    /// change volume. Scripts may change intensities at runtime.
    pub fn velocity_to_volume(&self) -> f32 {
        self.mods
            .iter()
            .filter(|m| m.source == ModSource::Velocity && m.target == ModTarget::Volume)
            .map(|m| m.intensity)
            .fold(0.0, f32::max)
    }

    /// Pitch-bend range in semitones; `None` when pitch bend is not assigned to pitch.
    pub fn pitch_bend_range(&self) -> Option<f32> {
        self.modulation(ModSource::PitchBend, &ModTarget::Pitch)
            .map(|m| m.intensity * PITCH_SEMITONES_PER_INTENSITY)
    }

    /// First MIDI CC assigned to volume, with its controller number.
    pub fn cc_volume(&self) -> Option<(u8, &ModAssignment)> {
        self.mods.iter().find_map(|m| match m.source {
            ModSource::MidiCc(cc) if m.target == ModTarget::Volume => Some((cc, m)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment(source: ModSource, target: ModTarget, intensity: f32) -> ModAssignment {
        ModAssignment { cc_default: None,
            name: String::new(),
            source,
            target,
            intensity,
            invert: false,
            lag_ms: 0,
            shaper: None,
        }
    }

    #[test]
    fn accessors_find_engine_relevant_assignments() {
        let group = Group {
            mods: vec![
                assignment(ModSource::PitchBend, ModTarget::Pitch, 2.0 / 12.0),
                assignment(ModSource::MidiCc(11), ModTarget::Volume, 1.0),
                assignment(ModSource::Velocity, ModTarget::Volume, 0.5),
            ],
            ..Default::default()
        };
        assert_eq!(group.velocity_to_volume(), 0.5);
        assert!((group.pitch_bend_range().unwrap() - 2.0).abs() < 1e-6);
        assert_eq!(group.cc_volume().map(|(cc, _)| cc), Some(11));

        let silent = Group::default();
        assert_eq!(silent.velocity_to_volume(), 0.0);
        assert_eq!(silent.pitch_bend_range(), None);
        assert!(silent.cc_volume().is_none());
    }

    /// ANALOG STRINGS layouts: `pan`/`loopStart`/`loopLength` targets carry
    /// no slot byte, and an LFO stores category 1 with its 0x08 object bare.
    #[test]
    fn group_targets_and_bare_lfos_parse() {
        use ni_file::kontakt::{
            Chunk, StructuredObject,
            objects::{ExternalMod, InternalMod, Modulator},
        };
        fn name(out: &mut Vec<u8>, s: &str) {
            out.extend((s.len() as u32).to_le_bytes());
            out.extend(s.as_bytes());
        }
        let targets = |params: &[(&str, Option<u8>)], lag: u16, flags: u8, invert: bool| {
            let mut b = (params.len() as u32).to_le_bytes().to_vec();
            for (param, slot) in params {
                name(&mut b, param);
                b.extend(0.5f32.to_le_bytes());
                b.extend((-1i16).to_le_bytes());
                b.push(flags);
                b.extend(lag.to_le_bytes());
                name(&mut b, "<none>");
                b.extend(slot);
                b.push(u8::from(invert)); // separate inversion
            }
            b.extend(std::iter::repeat_n(0, params.len())); // no shapers
            b
        };
        let mut ext = targets(&[("pan", None), ("loopStart", None), ("loopLength", None), ("filterCutoff", Some(0))], 15, 0x10, false);
        name(&mut ext, "Loop_Start");
        ext.extend(2u32.to_le_bytes()); // unassigned
        ext.extend([0, 0]);
        ext.extend(7u32.to_le_bytes());
        let object = |version, private_data, children| StructuredObject { version, public_data: Vec::new(), private_data, children };
        // Retain the authored external sibling for the sparse internal-roster
        // check below; its prepared assignment offsets must remain independent.
        let mut external = vec![1]; external.extend(0x100u16.to_le_bytes());
        external.extend((ext.len() as u32).to_le_bytes()); external.extend(&ext);
        external.extend([0; 8]);
        let mut external_slots = vec![0; 5]; external_slots.push(1);
        Chunk { id: 0x0c, data: external }.write(&mut external_slots).unwrap(); external_slots.extend([0; 26]);
        let mut external_data = vec![1]; external_data.extend(0x10u16.to_le_bytes());
        external_data.extend(0u32.to_le_bytes()); external_data.extend((external_slots.len() as u32).to_le_bytes());
        external_data.extend(external_slots); external_data.extend(0u32.to_le_bytes());
        let external_chunk = Chunk { id: EXTERNAL_MODS_ID, data: external_data };
        let p = ExternalMod(object(0x100, ext, Vec::new())).params().unwrap();
        assert_eq!(p.targets.iter().map(|t| t.slot).collect::<Vec<_>>(), [None, None, None, Some(0)]);

        let mut int = targets(&[("pan", None)], 15, 0x10, false);
        int.extend([0, 1, 1, 0]);
        int.extend(187u32.to_le_bytes());
        name(&mut int, "LFO_P1");
        int.extend(1u32.to_le_bytes());
        // Authored structured-object fixture, independent of any library bytes.
        let mut data = vec![0]; // unstructured public data
        data.extend(0x71u16.to_le_bytes());
        data.extend(5u32.to_le_bytes());
        data.extend([0; 63]);
        let lfo = Chunk { id: 0x08, data };
        let p = InternalMod(object(0x80, int, vec![lfo])).params().unwrap();
        assert_eq!(p.name, "LFO_P1");
        assert!(matches!(p.modulator, Modulator::Lfo(_)));
        assert_eq!(p.targets[0].param, "pan");
        assert_eq!(p.targets[0].intensity, 0.5);
        let source = ni_file::kontakt::objects::Lfo { structured: false, version: 0x71,
            waveform: 5, initial_values: [0., 12., 0.5, 0.], records: [
                ni_file::kontakt::objects::LfoRecord { flag: true, values: [1. / 24., 0., 0.] },
                ni_file::kontakt::objects::LfoRecord { flag: true, values: [-1., 0., 0.] }],
            trailing_flag: false, trailing_values: Some([0.03, 0., 0., 0., 0.]), additional_flag: None };
        let saved_at = |slot: usize, rows: &[(&str, Option<u8>)], lag, flags, invert, fade, phase| {
            let mut source = source.clone(); source.initial_values[0] = fade; source.initial_values[3] = phase;
            let mut private = targets(rows, lag, flags, invert);
            private.extend([0, 0, 1, 0]); private.extend(0u32.to_le_bytes());
            name(&mut private, "Saved sine"); private.extend(1u32.to_le_bytes());
            let mut wrapper = vec![0; slot]; wrapper.push(1);
            let internal = object(0x80, private, vec![source.to_chunk().unwrap()]);
            let mut data = vec![1]; data.extend(internal.version.to_le_bytes());
            for part in [&internal.private_data[..], &[][..]] { data.extend((part.len() as u32).to_le_bytes()); data.extend(part); }
            let mut children = Vec::new(); internal.children[0].write(&mut children).unwrap();
            data.extend((children.len() as u32).to_le_bytes()); data.extend(children);
            Chunk { id: 0x0d, data }.write(&mut wrapper).unwrap(); wrapper.extend(std::iter::repeat_n(0, 15 - slot));
            RawGroup(StructuredObject { version: 0x95, public_data: vec![], private_data: vec![],
                children: vec![Chunk { id: INTERNAL_MODS_ID, data: {
                    let mut data = vec![1]; data.extend(0x10u16.to_le_bytes()); data.extend(0u32.to_le_bytes());
                    data.extend((wrapper.len() as u32).to_le_bytes()); data.extend(wrapper); data.extend(0u32.to_le_bytes()); data
                }}] })
        };
        let saved = |rows: &[(&str, Option<u8>)], lag, flags, invert, fade, phase|
            saved_at(0, rows, lag, flags, invert, fade, phase);
        let raw = saved(&[("pan", None), ("pitch", None), ("volume", None), ("pitch", None)], 0, 0x10, false, 0., 0.);
        let decoded = read_group(&raw).unwrap();
        assert_eq!(decoded.pitch_lfos.len(), 1);
        assert_eq!(decoded.pitch_lfos[0].slot, 0);
        assert_eq!(decoded.pitch_lfos[0].targets, [(1, 0.5), (3, 0.5)], "do not compress native target indices");
        assert_eq!(decoded.pitch_lfos[0].depth, 1.);
        assert_eq!(decoded.volume_lfos.len(), 1);
        assert_eq!((decoded.volume_lfos[0].source.slot, decoded.volume_lfos[0].target), (0, 2));
        assert_eq!(decoded.volume_lfos[0].intensity, 0.5);
        assert!(!decoded.volume_lfos[0].negative);
        assert_eq!(decoded.volume_lfos[0].lag_ms, 0);
        assert!(decoded.warnings.iter().any(|w| w.contains("other than admitted pitch and volume")));

        for phase in [0., 0.25, 0.4990234375, 0.5, 0.5009765625, 0.75, 1.] {
            let decoded = read_group(&saved(&[("pitch", None), ("volume", None)], 0, 0x10, false, 40., phase)).unwrap();
            assert_eq!(decoded.pitch_lfos[0].start_phase.to_bits(), phase.to_bits());
            assert_eq!(decoded.volume_lfos[0].source.start_phase.to_bits(), phase.to_bits());
            assert_eq!(decoded.pitch_lfos[0].fade_ms, 40.);
            assert_eq!(decoded.volume_lfos[0].source.fade_ms, 40.);
            assert_eq!((decoded.pitch_lfos[0].targets[0].0, decoded.volume_lfos[0].target), (0, 1));
        }
        for phase in [-0.001, 1.001, f32::INFINITY, f32::NAN] {
            let decoded = read_group(&saved(&[("pitch", None), ("volume", None)], 0, 0x10, false, 0., phase)).unwrap();
            assert!(decoded.pitch_lfos.is_empty() && decoded.volume_lfos.is_empty());
            assert!(decoded.warnings.iter().any(|w| w.contains("pitch not applied")));
            assert!(decoded.warnings.iter().any(|w| w.contains("volume not applied")));
        }
        let volume = [("volume", None)];
        let negative = read_group(&saved(&volume, 15, 0x12, false, 0., 0.)).unwrap();
        assert_eq!((negative.volume_lfos[0].negative, negative.volume_lfos[0].lag_ms), (true, 15));
        for (rows, lag, flags, invert, fade) in [
            (&volume[..], 32768, 0x10, false, 0.),
            (&volume[..], 15, 0x14, false, 0.),
            (&volume[..], 15, 0x10, true, 0.),
            (&volume[..], 15, 0x10, false, 5001.),
            (&[("volume", None), ("volume", None)][..], 15, 0x10, false, 0.),
        ] {
            let rejected = read_group(&saved(rows, lag, flags, invert, fade, 0.)).unwrap();
            assert!(rejected.volume_lfos.is_empty());
            assert!(rejected.warnings.iter().any(|w| w.contains("volume not applied")));
        }
        let rejected_pitch = read_group(&saved(&[("pitch", None)], 15, 0x10, false, 0., 0.)).unwrap();
        assert!(rejected_pitch.pitch_lfos.is_empty());
        assert!(rejected_pitch.warnings.iter().any(|w| w.contains("pitch not applied")),
            "eligible source waveform cannot silence invalid destination warnings");
        for slot in [1, 7, 15] {
            let mut sparse = saved_at(slot, &[("pitch", None)], 0, 0x10, false, 0., 0.25);
            sparse.0.children.push(Chunk { id: external_chunk.id, data: external_chunk.data.clone() });
            let decoded = read_group(&sparse).unwrap();
            assert_eq!(decoded.modulators.len(), slot + 2);
            assert!(decoded.modulators[..slot].iter().all(|m| m.kind == "undecoded" && m.name.is_empty()));
            assert_eq!(decoded.modulators[slot + 1].name, "Loop_Start");
            assert_eq!(decoded.modulators[slot + 1].assignments, Some(0));
            assert_eq!(decoded.mods.len(), 4, "internal holes cannot discard or retarget external assignments");
            let group = crate::import::Group { modulators: decoded.modulators,
                pitch_lfos: decoded.pitch_lfos, mods: decoded.mods, ..Default::default() };
            assert_sparse_lfo_live_pcm(group, slot);
        }
    }

    fn assert_sparse_lfo_live_pcm(group: crate::import::Group, slot: usize) {
        use crate::{audio::Sample, engine::{Bank, Engine, load_scripts}, import::{Instrument, Zone}};
        const FRAMES: usize = 1537;
        const STAGES: [(usize, f64, bool); 4] = [(0, 1., false), (229, 0.125, false),
            (617, 0.125, true), (941, 0.125, false)];
        let script = "on init
            declare $source := get_mod_idx(0,\"Saved sine\")
            set_engine_par($ENGINE_PAR_INTMOD_INTENSITY,1000000,0,$source,0)
            end on
            on controller
            if ($CC_NUM=16)
                set_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY,750000,0,$source,0)
            end if
            if ($CC_NUM=17)
                set_engine_par($ENGINE_PAR_INTMOD_BYPASS,%CC[17],0,$source,-1)
            end if
            end on";
        let instrument = Instrument { groups: vec![group.clone()], scripts: vec![script.into()], ..Default::default() };
        assert_eq!(crate::engine::params::find_mod(&instrument.groups, 0, &|name| name == "Saved sine"), Some(slot));
        for rate in [48_000, 96_000] {
            let create = |modulated| {
                let mut engine = Engine::default(); engine.reset(f64::from(rate)); engine.attack = 0.;
                engine.set_transport(false, 120., 0., (4, 4));
                let sample = Sample { rate, frames: (0..4096).map(|i|
                    [0.125 + if modulated { i as f32 / 8192. } else { 0. }; 2]).collect() };
                engine.set_bank(Some(Box::new(Bank::from_samples(vec![if modulated { group.clone() }
                    else { Default::default() }], vec![Zone::default()], vec![(Default::default(), sample)]).unwrap())));
                if modulated {
                    let (runtime, errors) = load_scripts(&instrument, vec![], f64::from(rate));
                    assert!(errors.is_empty(), "{errors:?}"); engine.set_script(runtime);
                    assert_eq!(engine.settings().unwrap()[0].pitch_lfos[0].depth, 1., "named depth must reach sparse native source");
                }
                engine.note_on(0, 60, 127); engine
            };
            let (mut whole, mut split, mut plain) = (create(true), create(true), create(false));
            let (mut a, mut b, mut baseline) = ([0.; FRAMES], [0.; FRAMES], [0.; FRAMES]);
            let mut exercise = || {
                for (stage, &(start, _, bypass)) in STAGES.iter().enumerate() {
                    let end = STAGES.get(stage + 1).map_or(FRAMES, |s| s.0);
                    for engine in [&mut whole, &mut split] {
                        if stage == 1 { engine.cc(0, 16, 127); }
                        if stage >= 2 { engine.cc(0, 17, u8::from(bypass)); }
                    }
                    for (engine, pcm, sizes) in [(&mut whole, &mut a, &[128][..]),
                        (&mut split, &mut b, &[1, 7, 31, 127][..]), (&mut plain, &mut baseline, &[128][..])] {
                        let (mut at, mut block) = (start, 0);
                        while at < end {
                            let n = sizes[block % sizes.len()].min(end - at); let mut right = [0.; 128];
                            engine.render(&mut pcm[at..at + n], &mut right[..n]); at += n; block += 1;
                        }
                    }
                }
            };
            #[cfg(feature = "plugin")]
            assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
            #[cfg(not(feature = "plugin"))]
            exercise();
            // Independent phase integral and native 32-frame interpolation;
            // the constant sample supplies only the ordinary amplifier law.
            let (mut phase, mut previous, mut current, mut position) = (0.25_f64, 0., 0., 0_u64);
            let mut error = 0_f64;
            for frame in 0..FRAMES {
                let &(_, depth, bypass) = STAGES.iter().rev().find(|s| s.0 <= frame).unwrap();
                let pitch = if bypass { 0. } else {
                    if frame % 32 == 0 {
                        previous = current; current = -(phase * std::f64::consts::TAU).sin() * 12. * depth * f64::from(0.03_f32);
                        if frame == 0 { previous = current; }
                    }
                    phase += 4. / f64::from(rate);
                    previous + (current - previous) * (frame % 32) as f64 / 32.
                };
                let source_position = position as f64 / 4294967296.;
                let mut sample = 0.125 + source_position / 8192.;
                // The first cubic window has a zero left tap outside the
                // sample, rather than the straight line's extrapolated value.
                // Its Catmull-Rom weight is -t(1-t)^2/2; retain this boundary
                // contribution so the oracle also checks the first two frames.
                if source_position < 1. {
                    sample += 0.5 * (0.125 - 1. / 8192.) * source_position * (1. - source_position).powi(2);
                }
                let expected = f64::from(baseline[frame]) / 0.125 * sample;
                error = error.max((f64::from(a[frame]) - expected).abs());
                assert!((a[frame] - b[frame]).abs() < 1e-7, "sparse source clock depends on fragment at {frame}");
                position += (2_f64.powf(pitch / 12.) * 4294967296.) as u64;
            }
            assert!(error < 2e-7, "slot={slot} rate={rate}: sparse named depth/bypass PCM error={error}");
        }
    }

    #[test]
    fn insert_modulation_does_not_disable_same_numbered_saved_pitch_lfo() {
        use ni_file::kontakt::{Chunk, StructuredObject, objects::{Lfo, LfoRecord}};
        fn name(out: &mut Vec<u8>, text: &str) {
            out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes());
        }
        fn object(id: u16, private: &[u8], public: &[u8], children: &[u8], version: u16) -> Chunk {
            let mut data = vec![1]; data.extend(version.to_le_bytes());
            for part in [private, public, children] { data.extend((part.len() as u32).to_le_bytes()); data.extend(part); }
            Chunk { id, data }
        }
        fn target(param: &str, slot: Option<u8>, depth: f32, lag: u16) -> Vec<u8> {
            let mut data = 1u32.to_le_bytes().to_vec(); name(&mut data, param);
            data.extend(depth.to_le_bytes()); data.extend((-1i16).to_le_bytes());
            data.push(0x10); data.extend(lag.to_le_bytes()); name(&mut data, param);
            data.extend(slot); data.extend([0, 0]); // not inverted, no shaper
            data
        }
        let raw = |param: &str, fade_ms, delay_note| {
            let source = Lfo { structured: false, version: 0x71, waveform: 5,
                initial_values: [fade_ms, 14., 0.5, 0.], records: [
                    LfoRecord { flag: true, values: [1. / 24., 0., 0.] },
                    LfoRecord { flag: true, values: [delay_note, 0., 0.] }],
                trailing_flag: false, trailing_values: Some([0.03, 0., 0., 0., 0.]), additional_flag: None };
            let mut child = Vec::new(); source.to_chunk().unwrap().write(&mut child).unwrap();
            let mut internals = Vec::new();
            for slot in 0..16 {
                internals.push(u8::from(slot < 8));
                if slot < 8 {
                    let mut private = target("pitch", None, 0.44428888, 0);
                    private.extend([0, 0, 1, 0]); private.extend(0u32.to_le_bytes());
                    name(&mut private, &format!("LFO{slot}")); private.extend(1u32.to_le_bytes());
                    object(0x0d, &private, &[], &child, 0x80).write(&mut internals).unwrap();
                }
            }
            // Actual-shaped CV_SATURATION: Constant, zero depth, 15 ms lag,
            // effect slot7 overlaps an independently retriggered LFO slot7.
            let mut private = target(param, Some(7), 0., 15);
            name(&mut private, "CV_SATURATION"); private.extend(1u32.to_le_bytes());
            private.extend(9u32.to_le_bytes()); private.extend([0; 4]); private.extend(0u32.to_le_bytes());
            let mut external = vec![1]; object(0x0c, &private, &[], &[], 0x102).write(&mut external).unwrap();
            external.extend([0; 31]);
            RawGroup(StructuredObject { version: 0x95, public_data: vec![], private_data: vec![],
                children: vec![object(INTERNAL_MODS_ID, &[], &internals, &[], 0x10),
                    object(EXTERNAL_MODS_ID, &[], &external, &[], 0x10)] })
        };
        let decoded = read_group(&raw("shaper", 0., -1.)).unwrap();
        assert!(decoded.pitch_lfos.iter().any(|l| l.slot == 7), "group effect slot7 cannot disable internal LFO slot7");
        assert_eq!(decoded.mods[0].target, ModTarget::Module { param: "shaper".into(), slot: 7 });
        assert_eq!((decoded.mods[0].intensity, decoded.mods[0].lag_ms), (0., 15), "legitimate effect assignment is retained");
        let with_fade = read_group(&raw("shaper", 2.3047996, -1.)).unwrap();
        assert_eq!(with_fade.pitch_lfos[0].fade_ms, 2.3047996);
        for count in [-3., 0.5, 2.] {
            let decoded = read_group(&raw("shaper", count, 1. / 24.)).unwrap();
            let source = &decoded.pitch_lfos[0];
            assert_eq!(source.fade_ms, 0.);
            assert_eq!(source.fade_sync, Some(LfoFadeSync { note_value: 1. / 24., count }));
        }
        for (fade, note) in [(2., f32::NAN), (-1., -1.), (f32::NAN, -1.)] {
            assert!(read_group(&raw("shaper", fade, note)).unwrap().pitch_lfos.is_empty(),
                "invalid fade state must retain unsupported diagnostics");
        }
        let unknown = read_group(&raw("unknownSourceControl", 0., -1.)).unwrap();
        assert!(!unknown.pitch_lfos.iter().any(|l| l.slot == 7), "unknown same-slot source controls remain unsupported");
        assert!(unknown.pitch_lfos.iter().any(|l| l.slot == 6), "other sources remain eligible");
        assert!(unknown.warnings.iter().any(|w| w.contains("external source controls its parameters")));
    }

    #[test]
    fn saved_target_sign_reaches_only_proven_pitch_cutoff_and_loop_routes() {
        use ni_file::kontakt::{Chunk, StructuredObject, objects::{ExternalMod, InternalMod}};
        fn name(out: &mut Vec<u8>, text: &str) {
            out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes());
        }
        fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
            let mut data = vec![1]; data.extend(version.to_le_bytes());
            for part in [private, public, children] {
                data.extend((part.len() as u32).to_le_bytes()); data.extend(part);
            }
            Chunk { id, data }
        }
        fn targets(flags: u8) -> Vec<u8> {
            let rows = [("pitch", None, 2.0f32 / 12.), ("filterCutoff", Some(3), 1.),
                ("filterReso", Some(3), 0.125), ("loopLength", None, 0.5)];
            let mut data = (rows.len() as u32).to_le_bytes().to_vec();
            for (param, slot, depth) in rows {
                name(&mut data, param); data.extend(depth.to_le_bytes());
                data.extend((-1i16).to_le_bytes()); data.push(flags); data.extend(15u16.to_le_bytes());
                name(&mut data, param); data.extend(slot); data.push(1); // independent invert
            }
            data.extend([0; 4]); // independent absent shapers
            data
        }
        fn slots(id: u16, count: usize, items: &[Chunk]) -> Chunk {
            let mut public = Vec::new();
            for i in 0..count {
                public.push(u8::from(i < items.len()));
                if let Some(item) = items.get(i) { item.write(&mut public).unwrap(); }
            }
            object(id, 0x10, &[], &public, &[])
        }
        let env = Ahdsr { attack_curve: 0., attack_ms: 0., decay_ms: 10., hold_ms: 0.,
            release_ms: 100., sustain: 1., unknown_flag: 0, unknown_tail: vec![0; 52] };
        let mut concrete = Vec::new(); env.write(&mut concrete).unwrap();
        let mut wrapped = Vec::new();
        object(7, 0x90, &[], &0u32.to_le_bytes(), &concrete).write(&mut wrapped).unwrap();
        let internal = |flags| {
            let mut private = targets(flags); private.extend([0; 4]); private.extend(0u32.to_le_bytes());
            name(&mut private, "Env"); private.extend(2u32.to_le_bytes());
            object(0x0d, 0x80, &private, &[], &wrapped)
        };
        let external = |flags| {
            let mut private = targets(flags); name(&mut private, "Constant");
            private.extend(1u32.to_le_bytes()); private.extend(9u32.to_le_bytes());
            private.extend([0; 4]); private.extend(0u32.to_le_bytes());
            object(0x0c, 0x102, &private, &[], &[])
        };
        let raw = RawGroup(StructuredObject { version: 0x95, public_data: vec![], private_data: vec![],
            children: vec![slots(INTERNAL_MODS_ID, 16, &[internal(0x10), internal(0x12)]),
                slots(EXTERNAL_MODS_ID, 32, &[external(0x10), external(0x12)])] });
        let decoded = read_group(&raw).unwrap();
        for (i, expected) in [(0, 1.), (1, -1.)] {
            for rows in [&decoded.envelopes[i].targets[..], &decoded.mods[i * 4..i * 4 + 4]] {
                assert_eq!(rows[0].target, ModTarget::Pitch);
                assert_eq!(rows[0].intensity, expected * (2. / 12.));
                assert_eq!(rows[1].target, ModTarget::Module { param: "filterCutoff".into(), slot: 3 });
                assert_eq!(rows[1].intensity, expected);
                assert_eq!(rows[2].intensity, 0.125, "unverified resonance law unchanged");
                assert_eq!(rows[3].intensity, expected * 0.5, "independently proved loop sign");
                assert!(rows.iter().all(|row| row.invert && row.lag_ms == 15 && row.shaper.is_none()));
            }
            let params = InternalMod::try_from(&internal(if i == 0 { 0x10 } else { 0x12 })).unwrap().params().unwrap();
            assert_eq!(params.targets[1].intensity, 1., "raw serialized magnitude unchanged");
            assert_eq!(params.targets[1].unknown_flags, if i == 0 { 0x10 } else { 0x12 });
            let ext = ExternalMod::try_from(&external(if i == 0 { 0x10 } else { 0x12 })).unwrap().params().unwrap();
            assert_eq!(ext.targets, params.targets, "both source layouts share the exact target record");
        }
        assert_eq!(decoded.modulators[2].assignments, Some(0));
        assert_eq!(decoded.modulators[3].assignments, Some(4));
        assert_eq!(decoded.modulators[1].envelope, Some(1));
    }

    #[test]
    fn ordinary_import_keeps_readable_modulation_slots_but_snapshots_remain_strict() {
        use ni_file::kontakt::{Chunk, StructuredObject};
        fn name(out: &mut Vec<u8>, text: &str) {
            out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes());
        }
        fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
            let mut data = vec![1]; data.extend(version.to_le_bytes());
            for part in [private, public, children] { data.extend((part.len() as u32).to_le_bytes()); data.extend(part); }
            Chunk { id, data }
        }
        fn targets(params: &[(&str, Option<u8>)], invert: u8, shaper: u8) -> Vec<u8> {
            let mut data = (params.len() as u32).to_le_bytes().to_vec();
            for (param, slot) in params {
                name(&mut data, param); data.extend(0.5f32.to_le_bytes());
                data.extend((-1i16).to_le_bytes()); data.push(0); data.extend(0u16.to_le_bytes());
                name(&mut data, "Target"); data.extend(slot); data.push(invert);
            }
            for _ in params { data.push(shaper); if shaper != 0 { data.push(0); } }
            data
        }
        fn slots(id: u16, count: usize, items: &[Chunk]) -> Chunk {
            let mut public = Vec::new();
            for i in 0..count {
                public.push(u8::from(i < items.len()));
                if let Some(item) = items.get(i) { item.write(&mut public).unwrap(); }
            }
            object(id, 0x10, &[], &public, &[])
        }
        let envelope = Ahdsr { attack_curve: 0., attack_ms: 10., decay_ms: 30., hold_ms: 20.,
            release_ms: 100., sustain: 0.5, unknown_flag: 0, unknown_tail: vec![0;52] };
        let mut concrete = Vec::new(); envelope.write(&mut concrete).unwrap();
        let mut wrapped = Vec::new(); object(7, 0x90, &[], &0u32.to_le_bytes(), &concrete).write(&mut wrapped).unwrap();
        let internal = |category: u32, label: &str, flags: [u8; 4]| {
            let mut private = targets(&[("volume", None)], 0, 0);
            private.extend(flags); private.extend(0u32.to_le_bytes()); name(&mut private, label);
            private.extend(category.to_le_bytes()); object(0x0d, 0x80, &private, &[], &wrapped)
        };
        let external = |invert, shaper, label: &str| {
            let mut private = targets(&[("volume", None), ("pitch", None), ("ahdsr_attack", Some(1))], invert, shaper);
            name(&mut private, label); private.extend(1u32.to_le_bytes());
            private.extend(6u32.to_le_bytes()); // existing decoded velocity source
            private.extend([0;4]); private.extend(17u32.to_le_bytes());
            object(0x0c, 0x102, &private, &[], &[])
        };
        let mut group = RawGroup(StructuredObject { version: 0x95, public_data: vec![], private_data: vec![], children: vec![
            slots(INTERNAL_MODS_ID, 16, &[internal(3, "UnknownEnv", [0; 4]), internal(2, "GoodEnv", [0; 4])]),
            slots(EXTERNAL_MODS_ID, 32, &[external(0, 8, "UnknownShaper"), external(2, 0, "UnknownFlag"), external(0, 0, "GoodVelocity")]),
        ] });
        assert!(read_group(&group).unwrap_err().to_string().contains("category 3"), "snapshot reader stays strict");
        let parsed = read_group_partial(&group, 7, "Authored").unwrap();
        assert_eq!(parsed.volume_env, Some(envelope));
        // Native XML/typed-reader proof: router-open is byte 0, bypass byte 1.
        // This readable fixture has depth0.5/flags0 and zeroed opaque records,
        // so the native primary kernel must remain unadmitted independently of
        // router state. Compare against the closed-router diagnostic baseline.
        let decoded_flags = |flags| read_group(&RawGroup(StructuredObject {
            version: 0x95, public_data: vec![], private_data: vec![],
            children: vec![slots(INTERNAL_MODS_ID, 16, &[internal(2, "Env", flags)])],
        })).unwrap();
        let baseline = decoded_flags([0, 0, 1, 0]);
        assert!(!baseline.native_volume_env);
        assert_eq!(baseline.warnings.len(), 1);
        assert!(baseline.warnings[0].starts_with("Primary AHDSR slot 0: native source kernel not applied;"));
        for flags in [[1, 0, 1, 0], [0, 1, 1, 0]] {
            let decoded = decoded_flags(flags);
            assert_eq!(decoded.modulators[0].bypassed, flags[1] != 0);
            assert!(!decoded.native_volume_env);
            assert_eq!(decoded.volume_env, baseline.volume_env);
            assert_eq!(decoded.warnings, baseline.warnings,
                "router-open is UI state and cannot add an unsupported audio-mode diagnostic");
        }
        assert_eq!(parsed.mods.len(), 3, "all targets of readable siblings survive");
        assert_eq!(parsed.mods[2].target, ModTarget::Attack, "native volume-envelope slot remains 1");
        assert_eq!(parsed.modulators.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["", "GoodEnv", "", "", "GoodVelocity"]);
        assert_eq!(parsed.modulators[4].assignments, Some(0));
        for i in [0, 2, 3] { assert_eq!(parsed.modulators[i].kind, "undecoded"); assert!(parsed.modulators[i].targets.is_empty()); }
        assert!(!parsed.native_volume_env);
        assert_eq!(parsed.warnings.len(), 4, "three failed-slot diagnostics plus the explicit primary fallback");
        assert_eq!(parsed.warnings.iter().filter(|w| w.starts_with("Group 7 \"Authored\": ")).count(), 3);
        assert_eq!(parsed.warnings.iter().filter(|w|
            w.starts_with("Primary AHDSR slot 1: native source kernel not applied;")).count(), 1);
        for error in ["category 3", "shaper kind 8", "Invalid boolean byte 2"] {
            assert!(parsed.warnings.iter().any(|w| w.starts_with("Group 7 \"Authored\"") && w.contains(error)));
        }
        group.0.children.remove(0);
        assert!(read_group(&group).unwrap_err().to_string().contains("shaper kind 8"));
        group.0.children[0].data.pop();
        assert!(read_group_partial(&group, 7, "Authored").is_err(), "bad array boundaries still reject decoding");
    }

    #[test]
    #[ignore = "requires an installed ANALOG STRINGS instrument"]
    fn analog_strings_lfo_sources_and_targets_are_identified() {
        let path = std::env::var_os("KONTRA_ANALOG_NKI").expect("set KONTRA_ANALOG_NKI");
        let program = crate::import::read(std::path::Path::new(&path)).unwrap();
        let lfos: Vec<_> = program.groups.iter().flat_map(|g| &g.modulators)
            .filter(|m| m.kind == "lfo").collect();
        assert_eq!(lfos.len(), 2_400);
        assert_eq!(lfos.iter().map(|m| m.targets.len()).sum::<usize>(), 19_680);
        assert_eq!(lfos.iter().filter(|m| m.targets.len() == 1).count(), 480);
        assert!(lfos.iter().all(|m| m.assignments.is_none() && m.envelope.is_none()));
    }

    #[test]
    fn shape_is_identity_without_shaper() {
        let mut m = assignment(ModSource::Velocity, ModTarget::Volume, 1.0);
        assert_eq!(m.shape(0.25), 0.25);
        m.shaper = Some(ShaperCurve::Table(vec![1.0, 0.0]));
        assert_eq!(m.shape(0.25), 0.75);
    }

    #[test]
    fn primary_ahdsr_admission_preserves_raw_metadata_and_rejects_unproven_transforms() {
        use ni_file::kontakt::{Chunk, StructuredObject};
        fn name(out: &mut Vec<u8>, text: &str) { out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes()); }
        fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
            let mut data = vec![1]; data.extend(version.to_le_bytes());
            for part in [private, public, children] { data.extend((part.len() as u32).to_le_bytes()); data.extend(part); }
            Chunk { id, data }
        }
        let raw = |depth: f32, flags: u8, lag: u16, invert: u8, source: [u8;4], ahd: u8, opaque: bool, shaper: bool, wrapper: (u16,u32,u32,bool)| {
            let mut tail = [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0].repeat(4);
            if opaque { tail[0] = 1; }
            let envelope = Ahdsr { attack_curve: 1., attack_ms: 125.012924, decay_ms: 0., hold_ms: 0.,
                release_ms: 25000.043, sustain: 1., unknown_flag: ahd, unknown_tail: tail };
            let mut concrete = Vec::new(); envelope.write(&mut concrete).unwrap();
            let mut wrapped = Vec::new(); object(7, wrapper.0, if wrapper.3 { &[99] } else { &[] }, &wrapper.1.to_le_bytes(), &concrete).write(&mut wrapped).unwrap();
            let mut private = 1u32.to_le_bytes().to_vec(); name(&mut private, "volume"); private.extend(depth.to_le_bytes());
            private.extend((-1i16).to_le_bytes()); private.push(flags); private.extend(lag.to_le_bytes());
            name(&mut private, "ENV_AHDSR_VOLUME"); private.push(invert);
            private.push(u8::from(shaper));
            if shaper { private.push(1); for _ in 0..128 { private.extend(0f32.to_le_bytes()); } }
            private.extend(source); private.extend(0u32.to_le_bytes()); name(&mut private, "ENV_AHDSR_VOLUME"); private.extend(wrapper.2.to_le_bytes());
            let internal = object(0x0d, 0x80, &private, &[], if wrapper.2 == 1 { &concrete } else { &wrapped });
            let mut slots = vec![1]; internal.write(&mut slots).unwrap(); slots.extend([0; 15]);
            RawGroup(StructuredObject { version: 0x95, public_data: vec![], private_data: vec![],
                children: vec![object(INTERNAL_MODS_ID, 0x10, &[], &slots, &[])] })
        };
        let known_wrapper = (0x90,0,2,false);
        for ahd in [0, 1] {
            let decoded = read_group(&raw(1., 0x10, 0, 0, [0,0,1,0], ahd, false, false, known_wrapper)).unwrap();
            assert!(decoded.native_volume_env);
            let group = crate::import::Group { native_volume_env: true, volume_env: decoded.volume_env, ..Default::default() };
            let saved = group.volume_env.as_ref().unwrap();
            let mut chunk = saved.to_chunk().unwrap();
            assert_eq!(&Ahdsr::try_from(&chunk).unwrap(), saved, "binary scalar/opaque fields remain lossless");
            chunk.data[1] = 0x12;
            assert!(Ahdsr::try_from(&chunk).is_err(), "unproved AHDSR versions cannot reach admission");
            let json = serde_json::to_value(&group).unwrap();
            let restored: crate::import::Group = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(restored.volume_env, group.volume_env, "cache retains raw AHD/timing metadata");
            assert!(restored.native_volume_env);
            let mut legacy = json; legacy.as_object_mut().unwrap().remove("native_volume_env");
            let legacy_env = legacy["volume_env"].as_object_mut().unwrap();
            legacy_env.remove("unknown_flag"); legacy_env.remove("unknown_tail");
            let legacy: crate::import::Group = serde_json::from_value(legacy).unwrap();
            assert!(!legacy.native_volume_env);
            assert!(legacy.volume_env.unwrap().unknown_tail.is_empty(), "older caches remain readable without inventing raw timing metadata");
        }
        assert!(read_group(&raw(1.,0x10,0,0,[1,0,1,0],0,false,false,known_wrapper)).unwrap().native_volume_env,
            "router-open UI state does not change the source law");
        for wrapper in [(0x91,0,2,false),(0x90,1,2,false),(0x90,0,1,false),(0x90,0,2,true)] {
            let decoded = read_group(&raw(1.,0x10,0,0,[0,0,1,0],0,false,false,wrapper)).unwrap();
            assert!(!decoded.native_volume_env, "unproved wrapper/category geometry stays outside the native source");
            assert!(decoded.volume_env.is_some());
            assert!(decoded.warnings.iter().any(|w| w.contains("native source kernel not applied")));
        }
        for (depth, flags, lag, invert, source, ahd, opaque, shaper) in [
            (0.5,0x10,0,0,[0,0,1,0],0,false,false), (1.,0x12,0,0,[0,0,1,0],0,false,false),
            (1.,0x10,15,0,[0,0,1,0],0,false,false), (1.,0x10,0,1,[0,0,1,0],0,false,false),
            (1.,0x10,0,0,[0,1,1,0],0,false,false), (1.,0x10,0,0,[0,0,0,0],0,false,false),
            (1.,0x10,0,0,[0,0,1,1],0,false,false), (1.,0x10,0,0,[2,0,1,0],0,false,false), (1.,0x10,0,0,[0,0,1,0],2,false,false),
            (1.,0x10,0,0,[0,0,1,0],0,true,false), (1.,0x10,0,0,[0,0,1,0],0,false,true),
        ] {
            let decoded = read_group(&raw(depth, flags, lag, invert, source, ahd, opaque, shaper, known_wrapper)).unwrap();
            assert!(!decoded.native_volume_env);
            assert_eq!(decoded.native_shared_volume_slot, (source == [0,0,0,0]).then_some(0));
            let diagnostic = if source == [0,0,0,0] { "native shared source kernel" }
                else { "native source kernel not applied" };
            assert!(decoded.warnings.iter().any(|w| w.contains(diagnostic)));
            assert!(decoded.volume_env.is_some(), "unadmitted metadata is retained for fallback and diagnostics");
        }
    }
}
