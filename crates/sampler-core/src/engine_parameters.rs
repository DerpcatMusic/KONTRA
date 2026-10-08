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
    Linear {
        low: f64,
        high: f64,
    },
    /// Geometric native range, for frequency and time controls.
    Exponential {
        low: f64,
        high: f64,
    },
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
            Self::Linear { low, high } => low.is_finite() && high.is_finite() && high >= low,
            Self::Exponential { low, high } => {
                low.is_finite() && high.is_finite() && low > 0. && high >= low
            }
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
        let (low, high) = (self.decode(0), self.decode(1_000_000));
        if !low.is_finite() || !high.is_finite() || value < low || value > high {
            return Err(Error::InvalidInput);
        }
        Ok(self.encode(value))
    }

    /// Convert the normalized service range to a native DSP value. Use a law
    /// admitted by Prepared::with_engine_parameters; normalized inputs clamp.
    pub fn decode(self, value: i32) -> f64 {
        let v = f64::from(value.clamp(0, 1_000_000));
        match self {
            Self::Linear { low, high } => low + (high - low) * v / 1e6,
            Self::Exponential { low, high } => (low.ln() + (high.ln() - low.ln()) * v / 1e6).exp(),
            Self::DecibelGain { low_db, high_db } => {
                10f64.powf((low_db + (high_db - low_db) * v / 1e6) / 20.)
            }
            Self::CubicGain { unity } => (v / unity).powi(3),
        }
    }
    /// Convert a finite native DSP value to the normalized service range.
    /// Frontends should use normalized_value to validate authored input.
    pub fn encode(self, value: f64) -> i32 {
        (match self {
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

impl Prepared {
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
            assert_eq!(law.normalized_value(f64::INFINITY), Err(Error::InvalidInput));
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
            if let ControlValue::Real(v) = self.control_value(plan, b.control)? {
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
