//! Group modulation decoded from Kontakt presets, in engine-facing form.
//!
//! Field meanings and confidence are documented in `audits/MODULATION.md`.
//! Nothing here is applied to playback by itself.

use anyhow::{Result, bail};
use ni_file::kontakt::objects::{
    ExternalModArray32, Group as RawGroup, InternalModArray16, Modulator as RawModulator,
};
use serde::Serialize;

use crate::import::Group;

pub use ni_file::kontakt::objects::{
    Breakpoint, EnvelopeAhdsr as Ahdsr, EnvelopeFlex as FlexEnvelope, FlexPoint, ModShaper,
    ModSource, ShaperCurve,
};

const INTERNAL_MODS_ID: u16 = 0x3B;
const EXTERNAL_MODS_ID: u16 = 0x3C;
/// Pitch modulation at intensity 1.0 spans one octave (PB_PITCH stores 2/12).
const PITCH_SEMITONES_PER_INTENSITY: f32 = 12.0;

/// Parameter driven by a modulation assignment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ModTarget {
    /// Group amplitude (`volume`).
    Volume,
    /// Group pitch (`pitch`).
    Pitch,
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
}

/// Modulation read from one group, plus notes about what was left out.
#[derive(Debug, Default)]
pub(crate) struct GroupModulation {
    pub volume_env: Option<Ahdsr>,
    pub flex_env: Option<FlexEnvelope>,
    pub mods: Vec<ModAssignment>,
    pub modulators: Vec<Modulator>,
    pub warnings: Vec<String>,
}

/// Read internal and external modulators from a group's child chunks.
pub(crate) fn read_group(group: &RawGroup) -> Result<GroupModulation> {
    let mut out = GroupModulation::default();

    // Internal-modulator slot of the volume AHDSR, which module targets address.
    let mut volume_env_slot = None;
    if let Some(chunk) = group.0.find_first(INTERNAL_MODS_ID) {
        let mut skipped = 0;
        for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
            let params = modulator.params()?;
            if std::env::var("DBG_INT").is_ok() { for t in &params.targets { eprintln!("INT {} flags={:02x?} {} i={:.3} inv={} tflags={:02x} shaper={}", params.name, params.unknown_flags, t.param, t.intensity, t.invert, t.unknown_flags, t.shaper.as_ref().is_some_and(|s| s.enabled)); } }
            // The first volume envelope of each kind; the voice multiplies them.
            let volume = params.targets.iter().any(|t| t.param == "volume");
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
            });
        }
        if skipped > 0 {
            out.warnings.push(
                "Internal modulators other than the first volume AHDSR and flex envelopes are not applied".into(),
            );
        }
    }

    if let Some(chunk) = group.0.find_first(EXTERNAL_MODS_ID) {
        for (_, assignment) in ExternalModArray32::try_from(chunk)?.slots()? {
            let params = assignment.params()?;
            out.modulators.push(Modulator {
                name: params.name.clone(),
                targets: params.targets.iter().map(|t| t.name.clone()).collect(),
                assignments: Some(out.mods.len()),
                volume_env: false,
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
                        (param, None) => bail!("Group-level modulation target {param} is unknown"),
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

    #[test]
    fn shape_is_identity_without_shaper() {
        let mut m = assignment(ModSource::Velocity, ModTarget::Volume, 1.0);
        assert_eq!(m.shape(0.25), 0.25);
        m.shaper = Some(ShaperCurve::Table(vec![1.0, 0.0]));
        assert_eq!(m.shape(0.25), 0.75);
    }
}
