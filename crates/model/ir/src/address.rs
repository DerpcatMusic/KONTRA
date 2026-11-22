//! Atomic parameter addressing, unified modulation route descriptor, and
//! interned value-law specs (IR vNext Phase 1B).
//!
//! This module introduces the vendor-neutral addressing layer shared across
//! modulation routes, native `.kontra` serialization, and format script
//! adapters without forcing formats to maintain parallel target enums.

use crate::{Curve, EffectParam, ModKind, Source, TableValue};

/// Where a [`ModRoute`] or [`ParamAddr`] operates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Per-voice / per-note.
    Voice,
    /// Group-level shared state.
    Group(u16),
    /// Bus-level mixer channel.
    Bus(u16),
    /// Instrument-wide master/rack state.
    Instrument,
}

/// Interned index into an instrument's value-law table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LawId(pub u32);

/// How a modulation route combines into its target parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CombineOp {
    /// Additive offset (`target + value`).
    #[default]
    Add,
    /// Multiplicative scaling (`target * value`).
    Multiply,
    /// Bipolar attenuation/replacement (`lerp`).
    Replace,
}

/// Serializable value-law specification referenced by [`LawId`].
#[derive(Clone, Debug, PartialEq)]
pub enum LawSpec {
    /// Identity mapping (`x -> x`).
    Identity,
    /// Gain curve law (`Decibels`, `Depth`, `Squared`, `Falling`, `Signed`).
    Curve(Curve),
    /// Piecewise-linear lookup table over `0..=1` (with optional stepped hold).
    Table { values: Vec<f32>, stepped: bool },
    /// Exponential pitch/frequency scaling (`base * 2^(x * octaves)`).
    Octaves { base: f32, octaves: f32 },
}

/// Bitflags modifying how a [`ModRoute`] evaluates its source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RouteFlags(pub u8);

impl RouteFlags {
    pub const NONE: Self = Self(0);
    /// Route is disabled (may still be toggled by scripts/automation).
    pub const BYPASS: Self = Self(1 << 0);
    /// Read source as unipolar `0..=1` even when natively bipolar.
    pub const UNIPOLAR: Self = Self(1 << 1);
    /// Sample source once at note-on rather than continuously.
    pub const LATCH_NOTE_ON: Self = Self(1 << 2);
    /// Sample source once at note-off (release).
    pub const LATCH_NOTE_OFF: Self = Self(1 << 3);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Envelope stage addressed by [`ParamAddr::EnvelopeStage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvStage {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// LFO parameter addressed by [`ParamAddr::LfoParam`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LfoParam {
    Phase,
    Rate,
    Hz,
    Division,
}

/// One canonical, vendor-neutral address for any modulatable or scriptable
/// engine parameter across voice, group, bus, effect, and modulator scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamAddr {
    /// Voice or group amplitude gain.
    Volume,
    /// Voice or group pitch in semitones.
    Pitch,
    /// Voice or group stereo pan (`-1..=1`).
    Pan,
    /// Voice sample start offset within `start_mod`.
    SampleStart,
    /// Voice loop start (`length = false`) or loop length (`length = true`).
    LoopShift { length: bool },
    /// Amplitude envelope (`envelope: None`) or breakpoint envelope (`Some(idx)`) stage.
    EnvelopeStage {
        envelope: Option<u8>,
        stage: EnvStage,
    },
    /// LFO parameter on slot `lfo`.
    LfoParam { lfo: u8, param: LfoParam },
    /// Filter cutoff in octaves (`hz_range = false`) or Hz (`hz_range = true`).
    FilterCutoff { insert: u8, hz_range: bool },
    /// Filter resonance on insert slot `insert`.
    FilterResonance { insert: u8 },
    /// Parametric EQ band gain on insert slot `insert`.
    EqBandGain { insert: u8, band: u8 },
    /// Generic effect parameter on insert slot `insert`.
    EffectParam { insert: u8, param: EffectParam },
    /// Wavetable reader parameter.
    Wavetable(TableValue),
    /// Per-zone channel mix matrix cell.
    ZoneMix { zone: u16, row: u8, side: u8 },
    /// Modulation depth scaling of another timed source (`of`).
    RouteIntensity { of: Source },
    /// Bus send level on bus send index `send`.
    BusSend { send: u8 },
}

impl ParamAddr {
    /// Maps an existing [`ModKind`] into its canonical [`ParamAddr`].
    /// Returns `None` only for the structural [`ModKind::Envelope`] marker.
    #[must_use]
    pub const fn from_mod_kind(kind: &ModKind) -> Option<Self> {
        Some(match *kind {
            ModKind::Envelope => return None,
            ModKind::Volume { .. } => Self::Volume,
            ModKind::Pitch { .. } => Self::Pitch,
            ModKind::Pan { .. } => Self::Pan,
            ModKind::Cutoff { insert, .. } => Self::FilterCutoff {
                insert,
                hz_range: false,
            },
            ModKind::CutoffHz { insert, .. } => Self::FilterCutoff {
                insert,
                hz_range: true,
            },
            ModKind::Resonance { insert, .. } => Self::FilterResonance { insert },
            ModKind::EqGain { insert, band, .. } => Self::EqBandGain { insert, band },
            ModKind::SampleStart { .. } => Self::SampleStart,
            ModKind::Attack { envelope, .. } => Self::EnvelopeStage {
                envelope,
                stage: EnvStage::Attack,
            },
            ModKind::Hold { envelope, .. } => Self::EnvelopeStage {
                envelope,
                stage: EnvStage::Hold,
            },
            ModKind::Decay { envelope, .. } => Self::EnvelopeStage {
                envelope,
                stage: EnvStage::Decay,
            },
            ModKind::Sustain { envelope, .. } => Self::EnvelopeStage {
                envelope,
                stage: EnvStage::Sustain,
            },
            ModKind::Release { envelope, .. } => Self::EnvelopeStage {
                envelope,
                stage: EnvStage::Release,
            },
            ModKind::LfoPhase { lfo, .. } => Self::LfoParam {
                lfo,
                param: LfoParam::Phase,
            },
            ModKind::LfoRate { lfo, .. } => Self::LfoParam {
                lfo,
                param: LfoParam::Rate,
            },
            ModKind::LfoHz { lfo, .. } => Self::LfoParam {
                lfo,
                param: LfoParam::Hz,
            },
            ModKind::LfoDivision { lfo, .. } => Self::LfoParam {
                lfo,
                param: LfoParam::Division,
            },
            ModKind::Intensity { of, .. } => Self::RouteIntensity { of },
            ModKind::LoopShift { length, .. } => Self::LoopShift { length },
            ModKind::Wavetable { value, .. } => Self::Wavetable(value),
            ModKind::Mix {
                zone, row, side, ..
            } => Self::ZoneMix { zone, row, side },
            ModKind::EffectParam { insert, param, .. } => Self::EffectParam { insert, param },
            ModKind::Send { send, .. } => Self::BusSend { send },
        })
    }
}

/// One atomic modulation route covering voice, group, bus, FX, mod-of-mod,
/// and per-note/poly-mod connections.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModRoute {
    pub scope: Scope,
    pub source: Source,
    pub transform: LawId,
    pub target: ParamAddr,
    pub depth: f32,
    pub span: f32,
    pub combine: CombineOp,
    pub smooth_ms: f32,
    pub flags: RouteFlags,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_addr_covers_every_mod_kind_variant() {
        assert_eq!(ParamAddr::from_mod_kind(&ModKind::Envelope), None);
        assert_eq!(
            ParamAddr::from_mod_kind(&ModKind::Volume {
                source: Source::Velocity,
                intensity: 1.0,
                curve: Curve::Squared,
                tau_ms: 0.0,
            }),
            Some(ParamAddr::Volume)
        );
        assert_eq!(
            ParamAddr::from_mod_kind(&ModKind::Pitch {
                source: Source::PitchBend,
                semitones: 2.0,
                depth: 1.0,
                tau_ms: 0.0,
            }),
            Some(ParamAddr::Pitch)
        );
        assert_eq!(
            ParamAddr::from_mod_kind(&ModKind::Cutoff {
                source: Source::NoteExpr(crate::NoteExpr::Brightness),
                insert: 2,
                depth: 1.0,
            }),
            Some(ParamAddr::FilterCutoff {
                insert: 2,
                hz_range: false,
            })
        );
        assert_eq!(
            ParamAddr::from_mod_kind(&ModKind::Release {
                source: Source::ReleaseVelocity,
                intensity: 0.5,
                scale: crate::TimeScale::Linear { power: 1 },
                envelope: None,
            }),
            Some(ParamAddr::EnvelopeStage {
                envelope: None,
                stage: EnvStage::Release,
            })
        );
        assert_eq!(
            ParamAddr::from_mod_kind(&ModKind::Send {
                source: Source::PerNoteCc {
                    registered: true,
                    index: 74,
                },
                send: 1,
            }),
            Some(ParamAddr::BusSend { send: 1 })
        );
    }

    #[test]
    fn mod_route_stays_compact_and_copy() {
        let route = ModRoute {
            scope: Scope::Voice,
            source: Source::NoteExpr(crate::NoteExpr::Vibrato),
            transform: LawId(0),
            target: ParamAddr::Pitch,
            depth: 1.0,
            span: 2.0,
            combine: CombineOp::Add,
            smooth_ms: 5.0,
            flags: RouteFlags::NONE,
        };
        let copy = route;
        assert_eq!(route, copy);
        assert!(
            std::mem::size_of::<ModRoute>() <= 48,
            "ModRoute grew to {} bytes",
            std::mem::size_of::<ModRoute>()
        );
    }
}
