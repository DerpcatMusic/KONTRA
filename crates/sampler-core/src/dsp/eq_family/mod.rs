//! Universal `eq` DSP (wave 4): one band stage covers every equaliser.
//!
//! A band is [`EqBand`]: a response [`Kind`] under a coefficient [`Law`].
//! - [`Law::KontaktLegacy`] runs the exact native one-band recurrence (`legacy.rs`, coefficient
//!   generator 0x1409591c0) with its knob laws and 32-frame control cadence.
//! - [`Law::Solid`] maps the saved knobs through the proved Solid G-EQ splines (`solid.rs`) into a
//!   generic RBJ biquad (the native topology is unproved).
//! - [`Law::Rbj`] is the generic Hertz/Q/dB biquad (`generic.rs`), also the home of scripted
//!   UVI EQ inserts.
//! A [`sampler_ir::fx::eq::Config`] is a serial cascade of bands, lowered here into one stage per
//! band (plus one per extra filter section and an output gain); per-voice state stays in the
//! stage's `ProcessorState`, so nothing allocates while rendering.
//!
//! `peaking.rs` holds [`PeakingEq`], the v1 TPT bell: a GENERIC FALLBACK, not native and not a
//! band of the superset; it is kept only for the v1-parity bands of legacy EQ kinds 23/24.
pub(super) mod generic;
pub(super) mod legacy;
pub(super) mod peaking;
mod solid;

pub use legacy::KontaktLegacyEqSettings;
pub use peaking::PeakingEq;
pub use sampler_ir::fx::eq::{Kind, Law, SolidSection};

use super::{
    Biquad, ControlRamp, ControlRange, FilterKind, Parameter, Planar, Processor, ProcessorState, StateVariableFilter, SvfMode,
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
                Law::Rbj => live(self.frequency) && live(self.q) && live(self.gain_db),
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

/// A band as a control-driven stage (constants seeded), for live-bound owners.
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

/// Static lowering: fixed bands become plain biquads and state-variable filters (which batch
/// across voices, like every other fixed filter), the native legacy law stays a stateful stage. A 3-pole filter is first order plus a pair; an output gain
/// follows when it is not 0 dB. Band order is signal order; disabled and flat bands add nothing.
pub(crate) fn lower(config: &sampler_ir::fx::eq::Config, rate: u32) -> Result<Vec<Processor>, Error> {
    let mut stages = Vec::new();
    for b in config.bands().iter().filter(|b| b.enabled) {
        if b.law == Law::KontaktLegacy {
            stages.push(Processor::Eq(live_band(b)));
            continue;
        }
        let (hz, q, boost) = match b.law {
            Law::Solid(section) => {
                solid::physical(section, f64::from(b.frequency), f64::from(b.q), f64::from(b.gain_db))
            }
            _ => (f64::from(b.frequency), f64::from(b.q), f64::from(b.gain_db)),
        };
        let q = (q * (1. + f64::from(b.proportional_q) * boost.abs() / 12.)).clamp(0.05, 50.);
        let hz = hz.clamp(1., 0.49 * f64::from(rate));
        let biquad = |kind, q| Biquad::new(rate, kind, hz, q).map(Processor::Biquad);
        let svf = |mode, q| {
            Processor::StateVariable(StateVariableFilter { mode, cutoff_hz: Parameter::Constant(hz), q: Parameter::Constant(q) })
        };
        let gain_kind = |make: fn(f64) -> FilterKind| (boost.abs() > 1e-9).then(|| biquad(make(boost), q));
        match b.kind {
            Kind::Level => {
                if boost.abs() > 1e-9 {
                    stages.push(Processor::Gain(10f64.powf(boost / 20.)));
                }
            }
            Kind::Peak => stages.extend(gain_kind(|g| FilterKind::Peak { gain_db: g }).transpose()?),
            Kind::LowShelf => stages.extend(gain_kind(|g| FilterKind::LowShelf { gain_db: g }).transpose()?),
            Kind::HighShelf => stages.extend(gain_kind(|g| FilterKind::HighShelf { gain_db: g }).transpose()?),
            Kind::FirstOrderLowShelf => {
                stages.extend(gain_kind(|g| FilterKind::FirstOrderLowShelf { gain_db: g }).transpose()?)
            }
            Kind::FirstOrderHighShelf => {
                stages.extend(gain_kind(|g| FilterKind::FirstOrderHighShelf { gain_db: g }).transpose()?)
            }
            Kind::BandPass => stages.push(svf(SvfMode::BandPass, q)),
            Kind::Notch => stages.push(svf(SvfMode::Notch, q)),
            Kind::LowPass | Kind::HighPass => {
                let high = b.kind == Kind::HighPass;
                // Solid HP/LP sections are one 2-pole Butterworth (their spline has no Q).
                let n = if matches!(b.law, Law::Solid(_)) { 2 } else { b.poles.clamp(1, 4) };
                let (one, two) = if high {
                    (SvfMode::OnePoleHighPass, SvfMode::HighPass)
                } else {
                    (SvfMode::OnePoleLowPass, SvfMode::LowPass)
                };
                if n % 2 == 1 {
                    stages.push(svf(one, q));
                }
                for k in 1..=n / 2 {
                    // The band's own Q only shapes a 2-pole filter.
                    stages.push(svf(two, if n == 2 { q } else { butterworth_q(n, k) }));
                }
            }
        }
    }
    if config.output_gain_db != 0. {
        stages.push(Processor::Gain(10f64.powf(f64::from(config.output_gain_db) / 20.)));
    }
    Ok(stages)
}

#[cfg(test)]
mod tests;
