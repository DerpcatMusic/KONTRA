//! One physical engine address space shared by every script instance and host.
use crate::{
    ControlId, ControlValue, ControlWrite, Error, ModTarget, ParamScope, PlanId, Prepared, Runtime,
    SlotKind, Text,
};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct EngineParameterAddress {
    pub parameter: u16,
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
}

/// One normalized player edit, ported from v1 engine::overrides. Group -1
/// applies to all bindings of this parameter/slot; a group edit adds to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineParameterOffset {
    pub address: EngineParameterAddress,
    pub offset: f32,
}

/// A native FX lane keeps its complete physical address, without hashing.
pub fn engine_parameter_control(address: EngineParameterAddress) -> ControlId {
    ControlId(
        (0x4658u128 << 112)
            | (u128::from(address.parameter) << 96)
            | (u128::from(address.group as u32) << 64)
            | (u128::from(address.slot as u32) << 32)
            | u128::from(address.generic as u32),
    )
}

/// Completion metadata for script service calls. Unsupported addresses stay
/// observable without stopping later authored writes; no fault text is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineParameterOutcome {
    pub plan: PlanId,
    pub program: usize,
    pub address: Option<EngineParameterAddress>,
    pub write: bool,
    pub result: Result<(), Error>,
}

pub(crate) const ENGINE_OUTCOME_CAPACITY: usize = 64;

/// Physical meter point. Channel is stereo 0/1; group/slot -1 means the
/// instrument/post-rack point. Bus is the authored bus number, not DSP packing.
/// Unsupported per-slot taps return InvalidInput rather than another level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineMeterAddress {
    pub group: i32,
    pub slot: i32,
    pub channel: u8,
    pub bus: Option<i32>,
}
/// Lowering asks per zone and processor lane; a scan of every name was a
/// third of lowering on large instruments. The first listed index wins.
pub fn engine_parameter_id(name: &str) -> Option<u16> {
    static IDS: std::sync::OnceLock<std::collections::HashMap<&'static str, u16>> =
        std::sync::OnceLock::new();
    let ids = IDS.get_or_init(|| {
        let mut ids = std::collections::HashMap::new();
        for (i, n) in crate::ENGINE_PARAMETER_NAMES.iter().enumerate() {
            if let Ok(i) = u16::try_from(i) {
                ids.entry(n.trim_start_matches('$')).or_insert(i);
            }
        }
        ids
    });
    ids.get(name.trim_start_matches('$')).copied()
}
pub fn engine_parameter_name(id: u16) -> Option<&'static str> {
    crate::ENGINE_PARAMETER_NAMES.get(usize::from(id)).copied()
}

/// Module owners bind the same controls the DSP reads. No private write mirror.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub enum EngineParameterLaw {
    /// Native WT phase-form ordinal. This is a discrete enum, not a 0..1M knob.
    /// NI Source Module reference; Kontakt phase dispatcher 0x140567aa0.
    WavetableForm,
    /// Native signed filter Gain: -1M..1M maps to -1..1 (±12 dB in the kernel).
    SignedNormalized,
    /// A native switch: zero is off, a nonzero write is on; readback is 0/1.
    Switch,
    Linear {
        low: f64,
        high: f64,
    },
    /// Geometric native range, for frequency and time controls. `high` below
    /// `low` falls (Kontakt's compressor RATIO, 1 down to 0.04).
    Exponential {
        low: f64,
        high: f64,
    },
    /// Geometric range minus an offset, including a native zero endpoint.
    ShiftedExponential {
        low: f64,
        high: f64,
        offset: f64,
    },
    /// Kontakt AHDSR normalized curvature to native exponential curvature.
    AhdsrCurve,
    /// A fixed decibel range whose DSP lane holds linear amplitude.
    DecibelGain {
        low_db: f64,
        high_db: f64,
    },
    /// Kontakt signed modulation depth, ported from v1 CubicDepth::Loop.
    SignedCubic,
    /// [`Self::SignedCubic`] in a route's depth units (12 per unit for pitch).
    ScaledSignedCubic {
        scale: f64,
    },
    CubicGain {
        unity: f64,
    },
    /// Measured stored values at evenly spaced writes over 0..=1e6, geometric
    /// between positive points (kontakt-bindings curves_dense.tsv).
    Table(&'static [f64]),
    /// Legacy EQ FREQn: `20 * 1000^(v/1e6)` Hz into the band's native knob.
    LegacyEqFrequency,
}
impl EngineParameterLaw {
    /// Shared authored/read/write conversion for native AHDSR stage values.
    /// Times are frames at `rate`; use 1000 for authored milliseconds.
    /// Sustain and the short Attack/Hold law port v1 `src/engine/params.rs`.
    pub fn envelope(stage: crate::EnvelopeStage, rate: u32) -> Self {
        match stage {
            crate::EnvelopeStage::Sustain => Self::Linear { low: 0., high: 1. },
            crate::EnvelopeStage::AttackCurve => Self::AhdsrCurve,
            _ => {
                let scale = f64::from(rate) / 1000.;
                Self::ShiftedExponential {
                    low: 2. * scale,
                    high: if matches!(
                        stage,
                        crate::EnvelopeStage::Attack | crate::EnvelopeStage::Hold
                    ) {
                        15002. * scale
                    } else {
                        25002. * scale
                    },
                    offset: 2. * scale,
                }
            }
        }
    }

    pub(crate) fn valid(self) -> bool {
        match self {
            Self::WavetableForm | Self::SignedNormalized | Self::Switch | Self::SignedCubic => true,
            Self::ScaledSignedCubic { scale } => scale.is_finite() && scale > 0.,
            Self::Linear { low, high } => low.is_finite() && high.is_finite() && high >= low,
            Self::Exponential { low, high } => {
                low.is_finite() && high.is_finite() && low > 0. && high > 0.
            }
            Self::Table(points) => points.len() >= 2 && points.iter().all(|p| p.is_finite() && *p >= 0.),
            Self::LegacyEqFrequency => true,
            Self::ShiftedExponential { low, high, offset } => {
                low.is_finite()
                    && high.is_finite()
                    && offset.is_finite()
                    && low > 0.
                    && high >= low
                    && low >= offset
            }
            Self::AhdsrCurve => true,
            Self::DecibelGain { low_db, high_db } => {
                low_db.is_finite()
                    && high_db.is_finite()
                    && high_db >= low_db
                    && 10f64.powf(high_db / 20.).is_finite()
                    && 10f64.powf(low_db / 20.) > 0.
            }
            Self::CubicGain { unity } => unity.is_finite() && unity > 0.,
        }
    }
    /// Validate a native authored value before converting it. This is the same
    /// law the addressed runtime binding uses; frontends do not duplicate it.
    pub fn normalized_value(self, value: f64) -> Result<i32, Error> {
        if !self.valid() || !value.is_finite() {
            return Err(Error::InvalidInput);
        }
        if self == Self::WavetableForm {
            // The measured phase dispatcher takes an integer ordinal. A real
            // host control must not round an unproved or fractional mode into
            // another sound (NI Source Module; native switch 0x140567aa0).
            return if value.fract() == 0.
                && (0. ..=33.).contains(&value)
                && sampler_ir::Wavetable::supports_form(value as u8)
            {
                Ok(value as i32)
            } else {
                Err(Error::InvalidInput)
            };
        }
        let (a, b) = (
            self.decode(if self == Self::SignedNormalized {
                -1_000_000
            } else {
                0
            }),
            self.decode(1_000_000),
        );
        let (low, high) = (a.min(b), a.max(b));
        if !low.is_finite() || !high.is_finite() || value < low || value > high {
            return Err(Error::InvalidInput);
        }
        Ok(self.encode(value))
    }

    /// Convert the normalized service range to a native DSP value. Use a law
    /// admitted by Prepared::with_engine_parameters; normalized inputs clamp.
    pub fn decode(self, value: i32) -> f64 {
        if self == Self::WavetableForm {
            return f64::from(value.clamp(0, 33));
        }
        if self == Self::SignedNormalized {
            return f64::from(value.clamp(-1_000_000, 1_000_000)) / 1e6;
        }
        let v = f64::from(value.clamp(0, 1_000_000));
        match self {
            Self::WavetableForm | Self::SignedNormalized => unreachable!(),
            Self::Switch => f64::from(value != 0),
            Self::SignedCubic => f64::from((2. * (v as f32 / 1_000_000.) - 1.).powi(3)),
            Self::ScaledSignedCubic { scale } => scale * Self::SignedCubic.decode(value),
            Self::Linear { low, high } => low + (high - low) * v / 1e6,
            Self::Exponential { low, high } => {
                // Declared endpoints must not shrink or widen through log/exp roundoff.
                if v == 0. {
                    low
                } else if v == 1e6 {
                    high
                } else {
                    (low.ln() + (high.ln() - low.ln()) * v / 1e6).exp()
                }
            }
            Self::ShiftedExponential { low, high, offset } => {
                Self::Exponential { low, high }.decode(value).max(low) - offset
            }
            Self::AhdsrCurve => {
                let c = v / 500000. - 1.;
                let b = f64::from(((1. - c.abs()) * 500000f64.ln() - 20000f64.ln()).exp() as f32);
                if c > 0. {
                    (b / (1. + b)).ln()
                } else {
                    ((1. + b) / b).ln()
                }
            }
            Self::DecibelGain { low_db, high_db } => {
                10f64.powf((low_db + (high_db - low_db) * v / 1e6) / 20.)
            }
            Self::CubicGain { unity } => (v / unity).powi(3),
            Self::Table(points) => {
                let x = v / 1e6 * (points.len() - 1) as f64;
                let i = (x as usize).min(points.len() - 2);
                let (a, b) = (points[i], points[i + 1]);
                let t = x - i as f64;
                // Measured points are exact (an authored 10 ms must stay inside the
                // lane's domain); a segment from zero (RV_PREDELAY) is linear.
                if t == 0. {
                    a
                } else if t == 1. {
                    b
                } else if a > 0. {
                    (a.ln() + (b.ln() - a.ln()) * t).exp()
                } else {
                    a + (b - a) * t
                }
            }
            Self::LegacyEqFrequency => f64::from(crate::KontaktLegacyEqSettings::frequency_knob(
                (20. * 1000f64.powf(v / 1e6)) as f32,
            )),
        }
    }
    /// Convert a finite native DSP value to the normalized service range.
    /// Frontends should use normalized_value to validate authored input.
    pub fn encode(self, value: f64) -> i32 {
        if self == Self::WavetableForm {
            return value.round().clamp(0., 33.) as i32;
        }
        if self == Self::SignedNormalized {
            return (value * 1e6).round().clamp(-1e6, 1e6) as i32;
        }
        (match self {
            Self::WavetableForm | Self::SignedNormalized => unreachable!(),
            Self::Switch => f64::from(value != 0.),
            Self::SignedCubic => 500_000. * (value.cbrt() + 1.),
            Self::ScaledSignedCubic { scale } => 500_000. * ((value / scale).cbrt() + 1.),
            Self::Linear { low, high } => {
                if low == high {
                    0.
                } else {
                    (value - low) / (high - low) * 1e6
                }
            }
            Self::CubicGain { unity } => value.max(0.).cbrt() * unity,
            Self::Exponential { low, high } => {
                if low == high {
                    0.
                } else {
                    (value.clamp(low.min(high), low.max(high)).ln() - low.ln()) / (high.ln() - low.ln()) * 1e6
                }
            }
            // Monotone measured curves: the write whose decode is nearest.
            Self::Table(_) | Self::LegacyEqFrequency => {
                let (mut lo, mut hi) = (0, 1_000_000);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if self.decode(mid) < value { lo = mid + 1 } else { hi = mid }
                }
                f64::from(lo)
            }
            Self::ShiftedExponential { low, high, offset } => {
                ((value + offset).max(low) / low).ln() / (high / low).ln() * 1e6
            }
            Self::AhdsrCurve => {
                if value == 0. {
                    500000.
                } else {
                    let b = 1. / value.abs().exp_m1();
                    let c = (1. - (b.ln() + 20000f64.ln()) / 500000f64.ln()).clamp(0., 1.);
                    500000. * (1. - value.signum() * c)
                }
            }
            Self::DecibelGain { low_db, high_db } => {
                if low_db == high_db {
                    0.
                } else {
                    (20. * value.max(f64::MIN_POSITIVE).log10() - low_db) / (high_db - low_db) * 1e6
                }
            }
        })
        .round()
        .clamp(0., 1e6) as i32
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineParameterBinding {
    pub address: EngineParameterAddress,
    pub control: ControlId,
    pub law: EngineParameterLaw,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineLookup {
    pub group: i32,
    pub owner: i32,
    pub target: bool,
    pub name: String,
    pub index: i32,
}

pub(crate) const ENVELOPE_STAGES: [crate::EnvelopeStage; 6] = [
    crate::EnvelopeStage::Attack,
    crate::EnvelopeStage::Hold,
    crate::EnvelopeStage::Decay,
    crate::EnvelopeStage::Sustain,
    crate::EnvelopeStage::Release,
    crate::EnvelopeStage::AttackCurve,
];

impl Prepared {
    pub(crate) fn with_authored_engine_parameters(
        mut self,
        mut values: Vec<(EngineParameterAddress, i32)>,
    ) -> Result<Self, Error> {
        values.sort_by_key(|(address, _)| *address);
        if values.windows(2).any(|v| v[0].0 == v[1].0)
            || values
                .iter()
                .any(|(address, _)| engine_parameter_name(address.parameter).is_none())
        {
            return Err(Error::InvalidInput);
        }
        self.authored_engine_parameters = values.into_boxed_slice();
        Ok(self)
    }

    pub fn engine_parameter_bindings(&self) -> &[EngineParameterBinding] {
        &self.engine_parameters
    }
    pub fn engine_lookups(&self) -> &[EngineLookup] {
        &self.engine_lookups
    }

    /// Install real native amplitude-envelope control lanes at an inventoried
    /// physical modulator slot. Defaults are authored native values, never a
    /// script-write mirror. Values are consumed when a voice starts.
    pub fn with_group_envelope_parameters(
        self,
        group: u32,
        physical_group: i32,
        slot: i32,
        authored: crate::Envelope,
    ) -> Result<Self, Error> {
        self.with_group_envelope_parameters_batch([(group, physical_group, slot, authored)])
    }

    /// Port v1 bank::Builder's single control-thread group construction pass;
    /// validate the completed schema once rather than once per envelope.
    pub fn with_group_envelope_parameters_batch(
        mut self,
        envelopes: impl IntoIterator<Item = (u32, i32, i32, crate::Envelope)>,
    ) -> Result<Self, Error> {
        let mut envelopes = envelopes.into_iter().peekable();
        if envelopes.peek().is_none() {
            return Ok(self);
        }
        let mut controls = self.controls.to_vec();
        let mut bindings = self.engine_parameters.to_vec();
        let mut lanes = self.envelope_controls.to_vec();
        lanes.resize(self.group_count as usize, [None; 6]);
        for (group, physical_group, slot, authored) in envelopes {
            if group >= self.group_count || physical_group < 0 || slot < 0 {
                return Err(Error::InvalidInput);
            }
            for (index, stage) in ENVELOPE_STAGES.into_iter().enumerate() {
                if lanes[group as usize][index].is_some() {
                    return Err(Error::InvalidInput);
                }
                let parameter = engine_parameter_id(
                    [
                        "ENGINE_PAR_ATTACK",
                        "ENGINE_PAR_HOLD",
                        "ENGINE_PAR_DECAY",
                        "ENGINE_PAR_SUSTAIN",
                        "ENGINE_PAR_RELEASE",
                        "ENGINE_PAR_ATK_CURVE",
                    ][index],
                )
                .unwrap();
                let id = ControlId(
                    (0x454e56u128 << 104)
                        | (u128::from(index as u8) << 96)
                        | (u128::from(physical_group as u32) << 64)
                        | (u128::from(slot as u32) << 32),
                );
                let law = EngineParameterLaw::envelope(stage, self.rate);
                let (min, max) = match stage {
                    crate::EnvelopeStage::Sustain => (0., 1.),
                    crate::EnvelopeStage::AttackCurve => (-32., 32.),
                    _ => (0., u32::MAX as f64),
                };
                controls.push(crate::ControlDefinition {
                    id,
                    domain: crate::ControlDomain::Real { min, max },
                    default: ControlValue::Real(authored.control_value(stage)),
                });
                lanes[group as usize][index] = Some(id);
                bindings.push(EngineParameterBinding {
                    address: EngineParameterAddress {
                        parameter,
                        group: physical_group,
                        slot,
                        generic: -1,
                    },
                    control: id,
                    law,
                });
            }
        }
        let lookups = self.engine_lookups.to_vec();
        self = self.with_controls(controls)?;
        self.envelope_controls = lanes.into_boxed_slice();
        self.with_engine_parameters(bindings, lookups)
    }

    pub fn with_engine_parameters(
        mut self,
        mut bindings: Vec<EngineParameterBinding>,
        lookups: Vec<EngineLookup>,
    ) -> Result<Self, Error> {
        bindings.sort_by_key(|b| b.address);
        if bindings.windows(2).any(|b| b[0].address == b[1].address) {
            return Err(Error::InvalidInput);
        }
        for b in &bindings {
            self.control_index(b.control)?;
            if engine_parameter_name(b.address.parameter).is_none() || !b.law.valid() {
                return Err(Error::InvalidInput);
            }
        }
        self.engine_parameters = bindings.into_boxed_slice();
        self.engine_lookups = lookups.into_boxed_slice();
        Ok(self)
    }
}

fn layer_parameter(name: &str) -> Option<ModTarget> {
    match name {
        "$ENGINE_PAR_VOLUME" => Some(ModTarget::Decibels),
        "$ENGINE_PAR_PAN" => Some(ModTarget::Pan),
        "$ENGINE_PAR_TUNE" => Some(ModTarget::Pitch),
        _ => None,
    }
}
fn layer_decode(target: ModTarget, value: i32) -> i64 {
    let v = f64::from(value.clamp(0, 1_000_000));
    (match target {
        ModTarget::Decibels => {
            if v == 0. {
                -1_000_000.
            } else {
                18000. * v.log2() - 346_768.234_247_835_1
            }
        }
        ModTarget::Pan => (v - 500000.) / 500.,
        _ => (v - 500000.) * 7.2,
    })
    .round() as i64
}
fn layer_encode(target: ModTarget, value: i64) -> i32 {
    let v = value as f64;
    (match target {
        ModTarget::Decibels => 2f64.powf((v + 346_768.234_247_835_1) / 18000.),
        ModTarget::Pan => v * 500. + 500000.,
        _ => v / 7.2 + 500000.,
    })
    .round()
    .clamp(0., 1e6) as i32
}
/// An instrument bus fader: +12 dB at 1e6, cubic like the group volume.
pub const BUS_VOLUME_LAW: EngineParameterLaw = EngineParameterLaw::CubicGain {
    unity: 629_960.524_947_436_1,
};
/// The slot control behind a slot parameter write, and its law.
/// `$ENGINE_PAR_VOLUME` at (-1, -1, `$NI_BUS_OFFSET` + bus) is an instrument
/// bus volume: the bus's Mix block at [`crate::BUS_VOLUME_SLOT`], on Kontakt's
/// volume law (18000·log2(v) − 346768 mB, 2^(dB/6)) = (v/unity)^3 with +12 dB at 1e6.
fn slot_parameter(
    name: &str,
    a: EngineParameterAddress,
) -> Option<(SlotKind, ControlId, EngineParameterLaw)> {
    let effect = EngineParameterLaw::CubicGain { unity: 396851. };
    let (kind, slot, law) = match name {
        "$ENGINE_PAR_EFFECT_BYPASS" | "$ENGINE_PAR_SEND_EFFECT_BYPASS" => {
            (SlotKind::Bypass, a.slot, effect)
        }
        "$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN" | "$ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN" => {
            (SlotKind::Output, a.slot, effect)
        }
        "$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL" => (SlotKind::Dry, a.slot, effect),
        "$ENGINE_PAR_VOLUME" if a.group == -1 && a.slot == -1 && a.generic >= 1000 => (
            SlotKind::Output,
            crate::BUS_VOLUME_SLOT,
            BUS_VOLUME_LAW,
        ),
        _ => return None,
    };
    Some((kind, crate::slot_control(kind, a.group, slot, a.generic), law))
}

#[cfg(test)]
mod law_tests {
    use super::*;
    #[test]
    fn measured_tables_decode_and_encode() {
        // Evenly spaced measured points (curves_dense.tsv), geometric between them.
        let law = EngineParameterLaw::Table(&[5., 10., 40.]);
        assert!(law.valid());
        assert!((law.decode(0) - 5.).abs() < 1e-9 && (law.decode(1_000_000) - 40.).abs() < 1e-9);
        assert!((law.decode(250_000) - 50f64.sqrt()).abs() < 1e-9);
        assert!((law.encode(20.) - 750_000).abs() <= 1);
        // Endpoints are exact: ANALOG STRINGS' authored 10 ms limiter release sat
        // 2e-15 below the lane's decoded minimum and failed the whole load.
        let release = EngineParameterLaw::Table(&sampler_ir::fx::tables::LIM_RELEASE);
        assert_eq!((release.decode(0), release.decode(1_000_000)), (10., 1000.));
        // RV_PREDELAY starts at 0 ms: that segment is linear.
        let law = EngineParameterLaw::Table(&[0., 2., 8.]);
        assert!(law.valid() && (law.decode(250_000) - 1.).abs() < 1e-9);
        // COMP RATIO falls from 1 to 0.04.
        let ratio = EngineParameterLaw::Exponential { low: 1., high: 0.04 };
        assert!(ratio.valid() && (ratio.decode(500_000) - 0.2).abs() < 1e-9);
        // INTMOD on a 12-semitone pitch route (v1: 1e6 is depth 1, 750000 is 0.125).
        let pitch = EngineParameterLaw::ScaledSignedCubic { scale: 12. };
        assert_eq!((pitch.decode(1_000_000), pitch.decode(750_000), pitch.decode(0)), (12., 1.5, -12.));
        assert_eq!(pitch.encode(1.5), 750_000);
    }

    #[test]
    fn bus_volume_reaches_the_bus_mix_on_the_kontakt_volume_law() {
        let id = engine_parameter_id("ENGINE_PAR_VOLUME").unwrap();
        let at = |generic| EngineParameterAddress { parameter: id, group: -1, slot: -1, generic };
        assert!(slot_parameter("$ENGINE_PAR_VOLUME", at(-1)).is_none());
        let (kind, control, law) = slot_parameter("$ENGINE_PAR_VOLUME", at(1002)).unwrap();
        assert_eq!(kind, SlotKind::Output);
        assert_eq!(control, crate::slot_control(kind, -1, crate::BUS_VOLUME_SLOT, 1002));
        for v in [1000, 250_000, 629_961, 1_000_000] {
            let db = layer_decode(ModTarget::Decibels, v) as f64 / 1000.;
            assert!((law.decode(v) - (db / 6.).exp2()).abs() < 1e-4 * law.decode(v).max(1e-3), "{v}");
        }
    }
    #[test]
    fn display_is_the_bare_number_because_scripts_append_the_unit() {
        let show = |name: &str, value| engine_parameter_display(engine_parameter_id(name).unwrap(), value);
        assert_eq!(show("ENGINE_PAR_ATTACK", 0), "0.0");
        assert_eq!(show("ENGINE_PAR_SUSTAIN", 0), "-inf");
        assert_eq!(show("ENGINE_PAR_SUSTAIN", 1_000_000), "0.0");
        assert_eq!(show("ENGINE_PAR_VOLUME", 0), "-inf");
        assert!(!show("ENGINE_PAR_VOLUME", 630_859).contains("dB"));
        assert!(!show("ENGINE_PAR_RELEASE", 500_000).is_empty());
    }
    #[test]
    fn signed_cubic_loop_depth_matches_v1_authored_controls() {
        let law = EngineParameterLaw::SignedCubic;
        for (raw, depth) in [
            (751086, 0.1266360729932785),
            (14233, -0.9170099496841431),
            (500000, 0.),
            (0, -1.),
            (1000000, 1.),
        ] {
            assert!((law.decode(raw) - depth).abs() < 1e-7);
            assert!((law.encode(depth) - raw).abs() <= 1);
            assert!(law.normalized_value(depth).is_ok());
        }
        assert!(law.normalized_value(f64::NAN).is_err());
        assert!(law.normalized_value(1.1).is_err());
    }

    #[test]
    fn exponential_parameter_endpoints_are_exact_and_reject_exterior_neighbors() {
        for (law, low, high) in [
            (
                EngineParameterLaw::Exponential {
                    low: 20.,
                    high: 20_000.,
                },
                20.,
                20_000.,
            ),
            (
                EngineParameterLaw::Exponential {
                    low: 20.,
                    high: 20.,
                },
                20.,
                20.,
            ),
            (
                EngineParameterLaw::ShiftedExponential {
                    low: 2.,
                    high: 15_002.,
                    offset: 2.,
                },
                0.,
                15_000.,
            ),
            (
                EngineParameterLaw::ShiftedExponential {
                    low: 2.,
                    high: 25_002.,
                    offset: 2.,
                },
                0.,
                25_000.,
            ),
        ] {
            assert_eq!(law.decode(0), low, "{law:?}");
            assert_eq!(law.decode(1_000_000), high, "{law:?}");
            assert_eq!(law.decode(i32::MIN), low, "{law:?}");
            assert_eq!(law.decode(i32::MAX), high, "{law:?}");
            assert_eq!(law.normalized_value(low), Ok(0), "{law:?}");
            assert_eq!(
                law.normalized_value(high),
                Ok(if low == high { 0 } else { 1_000_000 }),
                "{law:?}"
            );
            for value in [
                low.next_down(),
                high.next_up(),
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ] {
                assert_eq!(
                    law.normalized_value(value),
                    Err(Error::InvalidInput),
                    "{law:?}: {value}"
                );
            }
            if low != high {
                for value in [low.next_up(), high.next_down()] {
                    assert!(law.normalized_value(value).is_ok(), "{law:?}: {value}");
                }
                assert_eq!(
                    law.normalized_value(law.decode(500_000)),
                    Ok(500_000),
                    "{law:?}"
                );
            }
        }
    }

    #[test]
    fn native_parameter_laws_are_invertible_and_reject_invalid_ranges() {
        let laws = [
            EngineParameterLaw::Linear { low: -2., high: 4. },
            EngineParameterLaw::Exponential {
                low: 20.,
                high: 20_000.,
            },
            EngineParameterLaw::DecibelGain {
                low_db: -24.,
                high_db: 24.,
            },
            EngineParameterLaw::CubicGain { unity: 396851. },
        ];
        for law in laws {
            assert!(law.valid());
            assert_eq!(law.normalized_value(f64::NAN), Err(Error::InvalidInput));
            assert_eq!(
                law.normalized_value(f64::INFINITY),
                Err(Error::InvalidInput)
            );
            assert_eq!(law.normalized_value(law.decode(500000)), Ok(500000));
            for value in [0, 123456, 500000, 999999, 1000000] {
                assert!((law.encode(law.decode(value)) - value).abs() <= 1);
            }
        }
        assert!((laws[1].decode(500000) - (20f64 * 20_000.).sqrt()).abs() < 1e-8);
        assert_eq!(laws[2].decode(500000), 1.);
        for law in [
            EngineParameterLaw::Exponential {
                low: 0.,
                high: 20_000.,
            },
            EngineParameterLaw::Linear { low: 2., high: 1. },
            EngineParameterLaw::CubicGain { unity: f64::NAN },
        ] {
            assert!(!law.valid());
        }
    }
}

impl Runtime {
    pub fn engine_meter(&self, plan: PlanId, address: EngineMeterAddress) -> Result<f32, Error> {
        if address.channel > 1 || address.slot != -1 {
            return Err(Error::InvalidInput);
        }
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let bus = if let Some(bus) = address.bus {
            generation
                .prepared
                .bus_addresses
                .iter()
                .find(|(physical, _)| *physical == bus)
                .map(|(_, runtime)| *runtime)
        } else {
            generation
                .script
                .bus(u32::try_from(address.group).ok(), None)
        }
        .ok_or(Error::InvalidInput)?;
        generation
            .dsp
            .buses
            .peaks
            .get(bus)
            .map(|peak| peak[address.channel as usize])
            .ok_or(Error::InvalidInput)
    }
    pub fn engine_parameter_bindings(
        &self,
        plan: PlanId,
    ) -> Result<&[EngineParameterBinding], Error> {
        Ok(&self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .engine_parameters)
    }
    /// Replace the player's complete offset layer without allocating. Scripts
    /// continue to read/write base values; DSP consumes the base plus offsets.
    pub fn set_engine_offsets(&mut self, offsets: &[EngineParameterOffset]) -> Result<(), Error> {
        if offsets.len() > 256 || offsets.iter().any(|o| !o.offset.is_finite()) {
            return Err(Error::InvalidInput);
        }
        let generation = self
            .plans
            .get_mut(self.active_plan.0)
            .ok_or(Error::StaleHandle)?;
        let prepared = &generation.prepared;
        let matches = |a: EngineParameterAddress, binding: &EngineParameterBinding| {
            let b = binding.address;
            // User-layer amplitude selector across different physical modulator slots.
            // -2 is never admitted as a script address; match only actual amplitude lanes.
            a.parameter == b.parameter
                && (a.slot == b.slot || a.slot == -2 && binding.control.0 >> 104 == 0x454e56)
                && a.generic == b.generic
                && (a.group == -1 || a.group == b.group)
        };
        // Missing controls are ignored, like v1 offsets for a group without that parameter.
        let mut changed = false;
        for binding in &prepared.engine_parameters {
            let index = prepared.control_index(binding.control).unwrap();
            let offset: f32 = offsets
                .iter()
                .filter(|o| matches(o.address, binding))
                .map(|o| o.offset)
                .sum();
            if !offset.is_finite() {
                return Err(Error::InvalidInput);
            }
            // Phase mode uses raw ordinals; no continuous normalized player
            // offset law has been admitted for this discrete selector.
            if binding.law == EngineParameterLaw::WavetableForm && offset != 0. {
                return Err(Error::InvalidInput);
            }
            changed |= generation.controls.offsets[index] != offset;
        }
        if !changed {
            return Ok(());
        }
        let revision = generation
            .controls
            .revision
            .checked_add(1)
            .filter(|r| r.checked_add(generation.controls.pending as u64).is_some())
            .ok_or(Error::Capacity)?;
        for binding in &prepared.engine_parameters {
            let index = prepared.control_index(binding.control).unwrap();
            let offset: f32 = offsets
                .iter()
                .filter(|o| matches(o.address, binding))
                .map(|o| o.offset)
                .sum();
            if generation.controls.offsets[index] != offset {
                generation.controls.offsets[index] = offset;
                let value = generation.controls.playing(prepared, index);
                generation.controls.values[index] = value;
                generation
                    .dsp
                    .edit_control(prepared, index, value, self.now);
            }
        }
        generation.controls.revision = revision;
        Ok(())
    }

    pub fn set_engine_parameter(
        &mut self,
        address: EngineParameterAddress,
        value: i32,
    ) -> Result<(), Error> {
        self.set_engine_parameter_in(self.active_plan(), address, value)
    }
    pub fn engine_parameter(&mut self, address: EngineParameterAddress) -> Result<i32, Error> {
        self.engine_parameter_in(self.active_plan(), address)
    }
    pub(crate) fn set_engine_parameter_in(
        &mut self,
        plan: PlanId,
        address: EngineParameterAddress,
        value: i32,
    ) -> Result<(), Error> {
        let name = engine_parameter_name(address.parameter).ok_or(Error::InvalidInput)?;
        if address.slot == -1 && address.generic == -1 {
            if let Some(target) = layer_parameter(name) {
                return self.write_param(
                    plan,
                    ParamScope::Group,
                    address.group.into(),
                    target,
                    layer_decode(target, value),
                    false,
                );
            }
            if name == "$ENGINE_PAR_OUTPUT_CHANNEL" {
                if let Ok(g) = usize::try_from(address.group) {
                    self.plans
                        .get_mut(plan.0)
                        .ok_or(Error::StaleHandle)?
                        .script
                        .set_route(g, value.into());
                }
                return Ok(());
            }
        }
        let binding = self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .engine_parameters
            .iter()
            .find(|b| b.address == address)
            .copied();
        if let Some(b) = binding {
            if b.law == EngineParameterLaw::WavetableForm
                && !u8::try_from(value).is_ok_and(sampler_ir::Wavetable::supports_form)
            {
                return Err(Error::InvalidInput);
            }
            return self
                .edit_controls_now(
                    plan,
                    None,
                    &[ControlWrite {
                        id: b.control,
                        value: ControlValue::Real(b.law.decode(value)),
                    }],
                )
                .map(|_| ());
        }
        if let Some((kind, id, law)) = slot_parameter(name, address) {
            if self.control_definition(plan, id).is_ok() {
                let value = if kind == SlotKind::Bypass {
                    f64::from(value != 0)
                } else {
                    law.decode(value).min(kind.max())
                };
                return self
                    .edit_controls_now(
                        plan,
                        None,
                        &[ControlWrite {
                            id,
                            value: ControlValue::Real(value),
                        }],
                    )
                    .map(|_| ());
            }
        }
        // An unbound module is explicit, never an apparently successful mirror write.
        Err(Error::InvalidInput)
    }
    pub(crate) fn engine_parameter_in(
        &mut self,
        plan: PlanId,
        address: EngineParameterAddress,
    ) -> Result<i32, Error> {
        let name = engine_parameter_name(address.parameter).ok_or(Error::InvalidInput)?;
        if address.slot == -1 && address.generic == -1 {
            if let Some(target) = layer_parameter(name) {
                let value =
                    self.read_param(plan, ParamScope::Group, address.group.into(), target)?;
                return Ok(layer_encode(target, value));
            }
        }
        let b = self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .engine_parameters
            .iter()
            .find(|b| b.address == address)
            .copied();
        if let Some(b) = b {
            let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
            let index = generation.prepared.control_index(b.control)?;
            if let ControlValue::Real(v) = generation.controls.base[index] {
                return Ok(b.law.encode(v));
            }
        }
        if let Some((kind, id, law)) = slot_parameter(name, address) {
            if let Ok(ControlValue::Real(v)) = self.control_value(plan, id) {
                return Ok(if kind == SlotKind::Bypass {
                    i32::from(v != 0.)
                } else {
                    law.encode(v)
                });
            }
        }
        // Port v1's live-then-authored getter for inactive native modulators.
        let values = &self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .authored_engine_parameters;
        let index = values
            .binary_search_by_key(&address, |(address, _)| *address)
            .map_err(|_| Error::InvalidInput)?;
        Ok(values[index].1)
    }
}
pub(crate) fn display(parameter: u16, value: i32, text: &mut Text) {
    match engine_parameter_name(parameter).unwrap_or("") {
        "$ENGINE_PAR_PAN" => {
            if value == 500000 {
                let _ = text.write_str("center");
            } else {
                let _ = write!(
                    text,
                    "{} {}",
                    (value - 500000).abs() / 5000,
                    if value < 500000 { "L" } else { "R" }
                );
            }
        }
        "$ENGINE_PAR_VOLUME" => {
            if value == 0 {
                let _ = text.write_str("-inf");
            } else {
                let _ = write!(
                    text,
                    "{:.1}",
                    layer_decode(ModTarget::Decibels, value) as f64 / 1000.
                );
            }
        }
        "$ENGINE_PAR_TUNE" => {
            let _ = write!(
                text,
                "{:.2}",
                layer_decode(ModTarget::Pitch, value) as f64 / 100000.
            );
        }
        // The bare number: scripts append the unit themselves (`& " ms"`, `& " dB"`).
        name @ ("$ENGINE_PAR_ATTACK" | "$ENGINE_PAR_HOLD" | "$ENGINE_PAR_DECAY"
        | "$ENGINE_PAR_RELEASE") => {
            let stage = match name {
                "$ENGINE_PAR_ATTACK" => crate::EnvelopeStage::Attack,
                "$ENGINE_PAR_HOLD" => crate::EnvelopeStage::Hold,
                "$ENGINE_PAR_DECAY" => crate::EnvelopeStage::Decay,
                _ => crate::EnvelopeStage::Release,
            };
            let _ = write!(text, "{:.1}", EngineParameterLaw::envelope(stage, 1000).decode(value));
        }
        "$ENGINE_PAR_SUSTAIN" => {
            let gain = f64::from(value.clamp(0, 1_000_000)) / 1e6;
            if gain <= 1e-6 {
                let _ = text.write_str("-inf");
            } else {
                // + 0.0 turns -0.0 into 0.0, as Kontakt shows.
                let _ = write!(text, "{:.1}", (200. * gain.log10()).round() / 10. + 0.0);
            }
        }
        _ => {
            let _ = write!(text, "{value}");
        }
    }
}
/// The text `get_engine_par_disp` shows for a native `value`, for hosts that
/// evaluate scripts without a running engine.
pub fn engine_parameter_display(parameter: u16, value: i32) -> String {
    let mut text = Text::default();
    display(parameter, value, &mut text);
    text.as_str().to_owned()
}

#[cfg(test)]
mod id_tests {
    #[test]
    fn indexed_ids_match_the_first_listed_name() {
        for name in crate::ENGINE_PARAMETER_NAMES {
            let first = crate::ENGINE_PARAMETER_NAMES
                .iter()
                .position(|n| n.trim_start_matches('$') == name.trim_start_matches('$'))
                .map(|i| i as u16);
            assert_eq!(super::engine_parameter_id(name), first);
            assert_eq!(super::engine_parameter_id(&format!("${name}")), first);
        }
        assert_eq!(super::engine_parameter_id("NOT_AN_ENGINE_PARAMETER"), None);
    }
}
