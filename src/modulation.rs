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
    /// Another group parameter (`pan`, `loopLength`); playback does not model it.
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
    /// Modulated parameter.
    pub target: ModTarget,
    /// Stored depth, 0..=1 in every local preset.
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
    /// A flex envelope: it has no AHDSR stages, so Kontakt ignores
    /// `$ENGINE_PAR_ATTACK` and friends addressed to it.
    #[serde(default)]
    pub flex: bool,
    /// Index into `Group::envelopes` of an AHDSR driving pitch or module parameters.
    #[serde(default)]
    pub envelope: Option<usize>,
    /// Modulator kind, for audits: `ahdsr`, `flex`, `lfo`, `chunk 0xNN` (an
    /// undecoded internal modulator), `external`, or `undecoded` (an occupied
    /// slot whose parameters could not be read; its empty name is not usable).
    #[serde(default)]
    pub kind: String,
}

/// Modulation read from one group, plus notes about what was left out.
#[derive(Debug, Default)]
pub(crate) struct GroupModulation {
    pub volume_env: Option<Ahdsr>,
    pub flex_env: Option<FlexEnvelope>,
    pub mods: Vec<ModAssignment>,
    pub modulators: Vec<Modulator>,
    pub envelopes: Vec<ModEnvelope>,
    pub warnings: Vec<String>,
}

/// An internal AHDSR driving pitch or module parameters (filter cutoff, EQ gain...),
/// not volume. Targets use `ModSource::Unassigned`; the envelope is the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModEnvelope {
    pub env: Ahdsr,
    pub targets: Vec<ModAssignment>,
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

    // Internal-modulator slot of the volume AHDSR, which module targets address.
    let mut volume_env_slot = None;
    if let Some(chunk) = group.0.find_first(INTERNAL_MODS_ID) {
        let mut skipped = 0;
        let mut skipped_lfos = 0;
        for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
            let params = match modulator.params() {
                Ok(params) => params,
                Err(error) => {
                    let Some(identity) = recover else { return Err(error.into()); };
                    out.failed_slot(identity, "internal", slot, modulator.0.version, &error);
                    continue;
                }
            };
            if params.unknown_flags[0] != 0 {
                out.warnings.push(format!("Internal modulator {} has an undecoded mode/bypass flag; preset bypass is not applied", params.name));
            }
            match &params.modulator {
                RawModulator::Ahdsr(env) if env.unknown_flag != 0 => out.warnings.push(
                    "AHDSR mode switches are not decoded; AHD-only/retrigger behavior may differ".into()),
                RawModulator::Flex(_) => out.warnings.push(
                    "Flex one-shot/loop switches are not decoded; playback uses sustain and key-release behavior".into()),
                _ => {}
            }
            // The first volume envelope of each kind; the voice multiplies them.
            let volume = params.targets.iter().any(|t| t.param == "volume");
            let flex = matches!(params.modulator, RawModulator::Flex(_));
            let kind = match params.modulator {
                RawModulator::Ahdsr(_) => "ahdsr".to_owned(),
                RawModulator::Flex(_) => "flex".to_owned(),
                RawModulator::Lfo(_) => "lfo".to_owned(),
                RawModulator::Other { chunk_id } => format!("chunk 0x{chunk_id:02x}"),
            };
            let envelope = (matches!(params.modulator, RawModulator::Ahdsr(_)) && !volume)
                .then_some(out.envelopes.len());
            let volume_env = match params.modulator {
                RawModulator::Ahdsr(env) if volume && out.volume_env.is_none() => {
                    out.volume_env = Some(env);
                    volume_env_slot = Some(slot);
                    true
                }
                RawModulator::Flex(env) if volume && out.flex_env.is_none() => {
                    out.flex_env = Some(env);
                    false
                }
                RawModulator::Ahdsr(env) if !volume => {
                    let targets: Vec<_> = params
                        .targets
                        .iter()
                        .filter_map(|t| {
                            Some(ModAssignment {
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
                                intensity: t.intensity,
                                invert: t.invert,
                                lag_ms: t.lag_ms,
                                shaper: t.shaper.clone().filter(|s| s.enabled).map(|s| s.curve),
                            })
                        })
                        .collect();
                    out.envelopes.push(ModEnvelope { env, targets });
                    false
                }
                RawModulator::Lfo(_) => {
                    skipped_lfos += 1;
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
                flex,
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
            let params = match assignment.params() {
                Ok(params) => params,
                Err(error) => {
                    let Some(identity) = recover else { return Err(error.into()); };
                    out.failed_slot(identity, "external", slot, assignment.0.version, &error);
                    continue;
                }
            };
            out.modulators.push(Modulator {
                name: params.name.clone(),
                targets: params.targets.iter().map(|t| t.name.clone()).collect(),
                assignments: Some(out.mods.len()),
                volume_env: false,
                flex: false,
                envelope: None,
                kind: "external".into(),
            });
            for target in params.targets {
                out.mods.push(ModAssignment {
                    name: params.name.clone(),
                    source: params.source,
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
                    intensity: target.intensity,
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

    Ok(out)
}

impl GroupModulation {
    fn failed_slot(&mut self, (index, name): (usize, &str), source: &str, slot: usize, version: u16, error: &ni_file::Error) {
        // Occupied slots still count in find_mod's order. Never assign an
        // invented name/source to a record whose parameters could not be read.
        self.modulators.push(Modulator {
            name: String::new(), targets: Vec::new(), assignments: None,
            volume_env: false, flex: false, envelope: None, kind: "undecoded".into(),
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
        ModAssignment {
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
        let targets = |params: &[(&str, Option<u8>)]| {
            let mut b = (params.len() as u32).to_le_bytes().to_vec();
            for (param, slot) in params {
                name(&mut b, param);
                b.extend(0.5f32.to_le_bytes());
                b.extend((-1i16).to_le_bytes());
                b.push(0x10);
                b.extend(15u16.to_le_bytes());
                name(&mut b, "<none>");
                b.extend(slot);
                b.push(0); // invert
            }
            b.extend(std::iter::repeat_n(0, params.len())); // no shapers
            b
        };
        let mut ext = targets(&[("pan", None), ("loopStart", None), ("loopLength", None), ("filterCutoff", Some(0))]);
        name(&mut ext, "Loop_Start");
        ext.extend(2u32.to_le_bytes()); // unassigned
        ext.extend([0, 0]);
        ext.extend(7u32.to_le_bytes());
        let object = |version, private_data, children| StructuredObject { version, public_data: Vec::new(), private_data, children };
        let p = ExternalMod(object(0x100, ext, Vec::new())).params().unwrap();
        assert_eq!(p.targets.iter().map(|t| t.slot).collect::<Vec<_>>(), [None, None, None, Some(0)]);

        let mut int = targets(&[("pan", None)]);
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
        let internal = |category: u32, label: &str| {
            let mut private = targets(&[("volume", None)], 0, 0);
            private.extend([0;4]); private.extend(0u32.to_le_bytes()); name(&mut private, label);
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
            slots(INTERNAL_MODS_ID, 16, &[internal(3, "UnknownEnv"), internal(2, "GoodEnv")]),
            slots(EXTERNAL_MODS_ID, 32, &[external(0, 8, "UnknownShaper"), external(2, 0, "UnknownFlag"), external(0, 0, "GoodVelocity")]),
        ] });
        assert!(read_group(&group).unwrap_err().to_string().contains("category 3"), "snapshot reader stays strict");
        let parsed = read_group_partial(&group, 7, "Authored").unwrap();
        assert_eq!(parsed.volume_env, Some(envelope));
        assert_eq!(parsed.mods.len(), 3, "all targets of readable siblings survive");
        assert_eq!(parsed.mods[2].target, ModTarget::Attack, "native volume-envelope slot remains 1");
        assert_eq!(parsed.modulators.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["", "GoodEnv", "", "", "GoodVelocity"]);
        assert_eq!(parsed.modulators[4].assignments, Some(0));
        for i in [0, 2, 3] { assert_eq!(parsed.modulators[i].kind, "undecoded"); assert!(parsed.modulators[i].targets.is_empty()); }
        assert_eq!(parsed.warnings.len(), 3);
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
}
