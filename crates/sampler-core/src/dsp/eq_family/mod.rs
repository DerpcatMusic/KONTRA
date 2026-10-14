//! Universal `eq` DSP (wave 4): one band stage covers every equaliser.
//!
//! There is ONE kernel (`kernel.rs`): a matched-response second-order design (Vicanek 2016) run
//! as a TPT state-variable section. A band is [`EqBand`]: a response [`Kind`] under a knob
//! [`Law`] that only decides which (Hz, Q, dB) or which native coefficients feed that kernel.
//! - [`Law::KontaktLegacy`]: the native one-band knob laws, coefficient generator 0x1409591c0 and
//!   32-frame control cadence (`legacy.rs`); its coefficients are realised by the kernel.
//! - [`Law::Solid`]: the native Solid G-EQ coefficient law (`solid.rs`), one stage per native
//!   biquad (`poles` carries the biquad index), realised by the kernel.
//! - [`Law::Rbj`]: physical Hertz/Q/dB (`generic.rs`), also the home of scripted UVI EQ inserts.
//! - [`Law::Cookbook`]: Hertz/Q/dB through the bilinear RBJ cookbook (`kernel::cookbook`), the
//!   native law of UVI's TwoBandEq/ThreeBandEq/ParametricEQ, realised by the kernel.
//! A [`sampler_ir::fx::eq::Config`] is a serial cascade of bands, lowered here into one stage per
//! band (plus one per extra filter section and an output gain); per-voice state stays in the
//! stage's `ProcessorState`, so nothing allocates while rendering.
//!
mod generic;
mod kernel;
pub(super) mod legacy;
pub(crate) mod solid;

pub use legacy::KontaktLegacyEqSettings;
pub use sampler_ir::fx::eq::{Kind, Law};

use super::{
    ControlRamp, ControlRange, Parameter, Planar, Processor, ProcessorState,
    control::PreparedParameter,
};
use crate::Error;

/// One EQ band's live settings. Meaning of the three value lanes follows `law`:
/// `Rbj`: Hertz, Q, decibels. `KontaktLegacy`: native frequency and width knobs (0..=1) and
/// displayed gain (-18..=18). `Solid`: the section's saved knobs (0..=1). `reference_db` is
/// subtracted from the gain (relative shelves); `enabled` below 0.5 passes the signal.
#[derive(Clone, Copy, Debug)]
pub struct EqBand {
    pub kind: Kind,
    pub law: Law,
    pub frequency: Parameter,
    pub q: Parameter,
    pub gain_db: Parameter,
    pub reference_db: Parameter,
    pub enabled: Parameter,
    /// Low/high pass order for this stage: 1 is first order, anything else a 2-pole section.
    pub poles: u8,
    /// Effective Q is `q * (1 + proportional_q * |boost| / 12)` (generic laws only).
    pub proportional_q: f32,
}

impl EqBand {
    /// A generic Hertz/Q/decibel band with constant controls.
    pub fn rbj(kind: Kind, frequency_hz: f64, q: f64, gain_db: f64) -> Self {
        let c = Parameter::Constant;
        Self {
            kind,
            law: Law::Rbj,
            frequency: c(frequency_hz),
            q: c(q),
            gain_db: c(gain_db),
            reference_db: c(0.),
            enabled: c(1.),
            poles: 2,
            proportional_q: 0.,
        }
    }

    pub(super) fn valid(self) -> bool {
        let live = |p: Parameter| p.valid() && !matches!(p, Parameter::Expression { .. });
        let within = |p: Parameter, lo: f64, hi: f64| live(p) && p.bounds().iter().all(|v| (lo..=hi).contains(v));
        let common = live(self.reference_db) && live(self.enabled) && self.proportional_q.is_finite();
        common
            && match self.law {
                Law::Rbj | Law::Cookbook => live(self.frequency) && live(self.q) && live(self.gain_db),
                Law::KontaktLegacy => {
                    self.kind == Kind::Peak
                        && within(self.frequency, 0., 1.)
                        && within(self.q, 0., 1.)
                        && within(self.gain_db, -18., 18.)
                }
                Law::Solid(_) => {
                    within(self.frequency, 0., 1.) && within(self.q, 0., 1.) && within(self.gain_db, 0., 1.)
                }
            }
    }

    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Result<Stage, Error> {
        if rate == 0 || !self.valid() {
            return Err(Error::InvalidInput);
        }
        Ok(match self.law {
            Law::KontaktLegacy => Stage::Native {
                eq: KontaktLegacyEqSettings {
                    frequency: self.frequency,
                    bandwidth: self.q,
                    gain_db: self.gain_db,
                }
                .compile(rate, bindings)?,
                enabled: self.enabled.compile(bindings),
            },
            _ => Stage::Generic(generic::Generic {
                rate,
                band: self,
                frequency: self.frequency.compile(bindings),
                q: self.q.compile(bindings),
                gain: self.gain_db.compile(bindings),
                reference: self.reference_db.compile(bindings),
                enabled: self.enabled.compile(bindings),
            }),
        })
    }
}

/// A prepared band.
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Native { eq: legacy::NativeEq, enabled: PreparedParameter },
    Generic(generic::Generic),
}

impl Stage {
    pub(crate) fn trace_parameters(self) -> Vec<(&'static str, PreparedParameter)> {
        match self {
            Stage::Native { eq, enabled } => {
                let mut all = eq.trace_parameters().to_vec();
                all.push(("enabled", enabled));
                all
            }
            Stage::Generic(g) => g.trace_parameters(),
        }
    }

    pub(super) fn process(
        self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
    ) {
        match self {
            Stage::Native { eq, enabled } => {
                if enabled.value(parameters, at, None) >= 0.5 {
                    eq.process(state, parameters, block, len, at);
                }
            }
            Stage::Generic(g) => g.process(state, parameters, block, len, at),
        }
    }
}

/// Butterworth section Q for pole pair `k` (1-based) of an `n`-pole filter.
fn butterworth_q(n: u8, k: u8) -> f64 {
    0.5 / (std::f64::consts::PI * f64::from(2 * k - 1) / f64::from(2 * n)).sin()
}

/// A band as an `EqBand` with constant controls (live-bound owners rebind them).
pub(crate) fn live_band(b: &sampler_ir::fx::eq::Band) -> EqBand {
    let c = Parameter::Constant;
    EqBand {
        kind: b.kind,
        law: b.law,
        frequency: c(f64::from(b.frequency)),
        q: c(f64::from(b.q)),
        gain_db: c(f64::from(b.gain_db)),
        reference_db: c(0.),
        enabled: c(f64::from(u8::from(b.enabled))),
        poles: b.poles,
        proportional_q: b.proportional_q,
    }
}

/// Lowering: one stage per filter section of every enabled band (a 3-pole filter is a first-order
/// section plus a pair), then an output gain when it is not 0 dB. Band order is signal order.
/// Every stage is the one kernel; only the knob law differs. Exactly-flat generic gain bands
/// add nothing.
pub(crate) fn lower(config: &sampler_ir::fx::eq::Config) -> Vec<Processor> {
    let mut stages = Vec::new();
    for b in config.bands().iter().filter(|b| b.enabled) {
        let band = live_band(b);
        let gainful = matches!(
            b.kind,
            Kind::Peak | Kind::LowShelf | Kind::HighShelf | Kind::FirstOrderLowShelf | Kind::FirstOrderHighShelf
        );
        if gainful && matches!(b.law, Law::Rbj | Law::Cookbook) && b.gain_db == 0. {
            continue;
        }
        if let Law::Solid(section) = b.law {
            for index in 0..solid::sections(section, b.kind) {
                stages.push(Processor::Eq(EqBand { poles: index as u8, ..band }));
            }
            continue;
        }
        let n = b.poles.clamp(1, 4);
        if !matches!(b.kind, Kind::LowPass | Kind::HighPass) || n == 2 {
            stages.push(Processor::Eq(EqBand { poles: 2, ..band }));
            continue;
        }
        // Other orders: Butterworth sections (the band's own Q only shapes a 2-pole filter).
        if n % 2 == 1 {
            stages.push(Processor::Eq(EqBand { poles: 1, ..band }));
        }
        for k in 1..=n / 2 {
            stages.push(Processor::Eq(EqBand { poles: 2, q: Parameter::Constant(butterworth_q(n, k)), ..band }));
        }
    }
    if config.output_gain_db != 0. {
        stages.push(Processor::Gain(10f64.powf(f64::from(config.output_gain_db) / 20.)));
    }
    stages
}

#[cfg(test)]
mod tests;
