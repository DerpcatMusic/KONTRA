//! One physical engine address space shared by every script instance and host.
use crate::{
    ControlId, ControlValue, ControlWrite, Error, ModTarget, ParamScope, PlanId, Prepared, Runtime,
    SlotKind, Text,
};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    ControlId((0x4658u128 << 112) | (u128::from(address.parameter) << 96)
        | (u128::from(address.group as u32) << 64)
        | (u128::from(address.slot as u32) << 32) | u128::from(address.generic as u32))
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
pub fn engine_parameter_id(name: &str) -> Option<u16> {
    crate::ENGINE_PARAMETER_NAMES
        .iter()
        .position(|n| n.trim_start_matches('$') == name.trim_start_matches('$'))
        .and_then(|i| u16::try_from(i).ok())
}
pub fn engine_parameter_name(id: u16) -> Option<&'static str> {
    crate::ENGINE_PARAMETER_NAMES.get(usize::from(id)).copied()
}

/// Module owners bind the same controls the DSP reads. No private write mirror.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineParameterLaw {
    /// Native signed filter Gain: -1M..1M maps to -1..1 (±12 dB in the kernel).
    SignedNormalized,
    Linear {
        low: f64,
        high: f64,
    },
    /// Geometric native range, for frequency and time controls.
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
    CubicGain {
        unity: f64,
    },
}
impl EngineParameterLaw {
    fn valid(self) -> bool {
        match self {
            Self::SignedNormalized => true,
            Self::Linear { low, high } => low.is_finite() && high.is_finite() && high >= low,
            Self::Exponential { low, high } => {
                low.is_finite() && high.is_finite() && low > 0. && high >= low
            }
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
        let (a, b) = (self.decode(if self == Self::SignedNormalized { -1_000_000 } else { 0 }), self.decode(1_000_000));
        let (low, high) = (a.min(b), a.max(b));
        if !low.is_finite() || !high.is_finite() || value < low || value > high {
            return Err(Error::InvalidInput);
        }
        Ok(self.encode(value))
    }

    /// Convert the normalized service range to a native DSP value. Use a law
    /// admitted by Prepared::with_engine_parameters; normalized inputs clamp.
    pub fn decode(self, value: i32) -> f64 {
        if self == Self::SignedNormalized { return f64::from(value.clamp(-1_000_000, 1_000_000)) / 1e6; }
        let v = f64::from(value.clamp(0, 1_000_000));
        match self {
            Self::SignedNormalized => unreachable!(),
            Self::Linear { low, high } => low + (high - low) * v / 1e6,
            Self::Exponential { low, high } => (low.ln() + (high.ln() - low.ln()) * v / 1e6).exp(),
            Self::ShiftedExponential { low, high, offset } => {
                (low.ln() + (high.ln() - low.ln()) * v / 1e6).exp().max(low) - offset
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
        }
    }
    /// Convert a finite native DSP value to the normalized service range.
    /// Frontends should use normalized_value to validate authored input.
    pub fn encode(self, value: f64) -> i32 {
        if self == Self::SignedNormalized { return (value * 1e6).round().clamp(-1e6, 1e6) as i32; }
        (match self {
            Self::SignedNormalized => unreachable!(),
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
                    (value.max(low).ln() - low.ln()) / (high.ln() - low.ln()) * 1e6
                }
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
        mut self,
        group: u32,
        physical_group: i32,
        slot: i32,
        authored: crate::Envelope,
    ) -> Result<Self, Error> {
        if group >= self.group_count || physical_group < 0 || slot < 0 {
            return Err(Error::InvalidInput);
        }
        let mut controls = self.controls.to_vec();
        let mut bindings = self.engine_parameters.to_vec();
        let mut lanes = self.envelope_controls.to_vec();
        lanes.resize(self.group_count as usize, [None; 6]);
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
            let law = match stage {
                crate::EnvelopeStage::Sustain => EngineParameterLaw::CubicGain { unity: 1000000. },
                crate::EnvelopeStage::AttackCurve => EngineParameterLaw::AhdsrCurve,
                _ => {
                    let scale = self.rate as f64 / 1000.;
                    EngineParameterLaw::ShiftedExponential {
                        low: 2. * scale,
                        high: if stage == crate::EnvelopeStage::Attack {
                            15002. * scale
                        } else {
                            25002. * scale
                        },
                        offset: 2. * scale,
                    }
                }
            };
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
fn slot_parameter(name: &str) -> Option<SlotKind> {
    match name {
        "$ENGINE_PAR_EFFECT_BYPASS" | "$ENGINE_PAR_SEND_EFFECT_BYPASS" => Some(SlotKind::Bypass),
        "$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN" | "$ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN" => {
            Some(SlotKind::Output)
        }
        "$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL" => Some(SlotKind::Dry),
        _ => None,
    }
}

#[cfg(test)]
mod law_tests {
    use super::*;
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
    /// Replace the player's complete offset layer without allocating. Scripts
    /// continue to read/write base values; DSP consumes the base plus offsets.
    pub fn set_engine_offsets(&mut self, offsets: &[EngineParameterOffset]) -> Result<(), Error> {
        if offsets.len() > 256 || offsets.iter().any(|o| !o.offset.is_finite()) {
            return Err(Error::InvalidInput);
        }
        let generation = self.plans.get_mut(self.active_plan.0).ok_or(Error::StaleHandle)?;
        let prepared = &generation.prepared;
        let matches = |a: EngineParameterAddress, b: EngineParameterAddress| {
            a.parameter == b.parameter && a.slot == b.slot && a.generic == b.generic
                && (a.group == -1 || a.group == b.group)
        };
        // Missing controls are ignored, like v1 offsets for a group without that parameter.
        let mut changed = false;
        for binding in &prepared.engine_parameters {
            let index = prepared.control_index(binding.control).unwrap();
            let offset: f32 = offsets.iter().filter(|o| matches(o.address, binding.address)).map(|o| o.offset).sum();
            if !offset.is_finite() { return Err(Error::InvalidInput); }
            changed |= generation.controls.offsets[index] != offset;
        }
        if !changed { return Ok(()); }
        let revision = generation.controls.revision.checked_add(1)
            .filter(|r| r.checked_add(generation.controls.pending as u64).is_some()).ok_or(Error::Capacity)?;
        for binding in &prepared.engine_parameters {
            let index = prepared.control_index(binding.control).unwrap();
            let offset: f32 = offsets.iter().filter(|o| matches(o.address, binding.address)).map(|o| o.offset).sum();
            if generation.controls.offsets[index] != offset {
                generation.controls.offsets[index] = offset;
                let value = generation.controls.playing(prepared, index);
                generation.controls.values[index] = value;
                generation.dsp.edit_control(prepared, index, value, self.now);
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
        if let Some(kind) = slot_parameter(name) {
            let id = crate::slot_control(kind, address.group, address.slot, address.generic);
            if self.control_definition(plan, id).is_ok() {
                let value = if kind == SlotKind::Bypass {
                    f64::from(value != 0)
                } else {
                    EngineParameterLaw::CubicGain { unity: 396851. }
                        .decode(value)
                        .min(kind.max())
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
        if let Some(kind) = slot_parameter(name) {
            if let Ok(ControlValue::Real(v)) = self.control_value(
                plan,
                crate::slot_control(kind, address.group, address.slot, address.generic),
            ) {
                return Ok(if kind == SlotKind::Bypass {
                    i32::from(v != 0.)
                } else {
                    EngineParameterLaw::CubicGain { unity: 396851. }.encode(v)
                });
            }
        }
        Err(Error::InvalidInput)
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
                let _ = text.write_str("-inf dB");
            } else {
                let _ = write!(
                    text,
                    "{:.1} dB",
                    layer_decode(ModTarget::Decibels, value) as f64 / 1000.
                );
            }
        }
        "$ENGINE_PAR_TUNE" => {
            let _ = write!(
                text,
                "{:.2} st",
                layer_decode(ModTarget::Pitch, value) as f64 / 100000.
            );
        }
        _ => {
            let _ = write!(text, "{value}");
        }
    }
}
