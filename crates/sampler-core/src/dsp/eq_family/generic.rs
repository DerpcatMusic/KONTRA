//! The Hertz/Q/decibel band stage of the universal EQ: Rbj-law bands (UVI, scripted inserts, any
//! format that already knows physical units) and Solid G-EQ bands, whose saved knobs give the
//! native G_EQ biquads ([`super::solid`]). Both feed the one kernel
//! ([`super::kernel`]). Controls are read once per block; the TPT sections keep their state across
//! coefficient changes, and coefficients are redesigned only when a control actually moved.
use super::super::{
    Planar, ProcessorState, flush,
    control::{ControlRamp, PreparedParameter},
};
use super::{EqBand, Kind, Law, digital, kernel, solid};
use crate::dsp::biquad::{self, Design};

#[derive(Clone, Copy)]
pub(crate) struct Generic {
    pub(super) rate: u32,
    pub(super) band: EqBand,
    pub(super) frequency: PreparedParameter,
    pub(super) q: PreparedParameter,
    pub(super) gain: PreparedParameter,
    pub(super) reference: PreparedParameter,
    pub(super) enabled: PreparedParameter,
    pub(super) bandwidth: PreparedParameter,
}

impl Generic {
    pub(crate) fn trace_parameters(self) -> Vec<(&'static str, PreparedParameter)> {
        vec![
            ("frequency", self.frequency),
            ("q", self.q),
            ("gain_db", self.gain),
            ("reference_db", self.reference),
            ("enabled", self.enabled),
            ("bandwidth", self.bandwidth),
        ]
    }

    /// Hertz, Q and boost in dB for this block, by law.
    pub(super) fn targets(self, parameters: &[ControlRamp], at: u64) -> (f64, f64, f64) {
        let read = |p: PreparedParameter| p.value(parameters, at, None);
        let (hz, q, boost) = (read(self.frequency), read(self.q), read(self.gain) - read(self.reference));
        let q = q * (1. + f64::from(self.band.proportional_q) * boost.abs() / 12.);
        (hz.clamp(1., 0.49 * f64::from(self.rate)), q.clamp(0.05, 50.), boost)
    }

    pub(super) fn process(
        self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
    ) {
        if self.enabled.value(parameters, at, None) < 0.5 {
            return;
        }
        let kind = self.band.kind;
        if let Law::Solid(section) = self.band.law {
            // The native G_EQ ramp (eq-kontakt.md sec. 5): the knob values move linearly from where
            // they were to the new target over the ramp, and the section is redesigned from the
            // moving values (the native per-frame recompute, here once per block). aux: target knobs
            // (3), valid flag, ramp start knobs (3), frames elapsed in the ramp, TPT coefficients (6).
            let read = |p: PreparedParameter| p.value(parameters, at, None);
            let target = [read(self.frequency), read(self.q), read(self.gain)];
            let ramp = self.band.ramp * f64::from(self.rate);
            let fraction = |elapsed: f64| if ramp > 0. { (elapsed / ramp).min(1.) } else { 1. };
            let first = state.aux[3] == 0.;
            let changed = first || state.aux[..3] != target;
            let start: [f64; 3] = if first {
                target
            } else if changed {
                std::array::from_fn(|k| state.aux[4 + k] + (state.aux[k] - state.aux[4 + k]) * fraction(state.aux[7]))
            } else {
                std::array::from_fn(|k| state.aux[4 + k])
            };
            let elapsed = if changed { 0. } else { state.aux[7] };
            let end = (elapsed + len as f64).min(ramp);
            // A ramp still moving this block, or a new target, needs a redesign.
            if changed || end > elapsed {
                let now: [f64; 3] = std::array::from_fn(|k| start[k] + (target[k] - start[k]) * fraction(end));
                let [b0, b1, b2, a1, a2] = solid::coefficients(
                    section,
                    kind,
                    usize::from(self.band.poles),
                    f64::from(self.rate),
                    now[0],
                    now[1],
                    now[2],
                );
                let svf = kernel::Svf::from_biquad(&kernel::Biquad { b: [b0, b1, b2], a: [a1, a2] })
                    .unwrap_or(kernel::Svf::PASS);
                state.aux[10..16].copy_from_slice(&svf.to_array());
            }
            state.aux[..3].copy_from_slice(&target);
            state.aux[3] = 1.;
            state.aux[4..7].copy_from_slice(&start);
            state.aux[7] = end;
            run(kernel::Svf::from_array(std::array::from_fn(|i| state.aux[10 + i])), state, block, len);
            return;
        }
        if self.band.law == Law::UviDigital {
            // aux: hertz, q, gain, octaves, valid flag, then the six TPT coefficients.
            let read = |p: PreparedParameter| p.value(parameters, at, None);
            let key = [read(self.frequency), read(self.q), read(self.gain) - read(self.reference), read(self.bandwidth)];
            if state.aux[4] == 0. || state.aux[..4] != key {
                let rate = f64::from(self.rate);
                let section = digital::section(kind, self.band.poles, key[0] / rate, key[2], key[1], key[3], usize::from(self.band.section));
                let svf = section.and_then(|b| kernel::Svf::from_biquad(&b)).unwrap_or(kernel::Svf::PASS);
                state.aux[..4].copy_from_slice(&key);
                state.aux[4] = 1.;
                state.aux[5..11].copy_from_slice(&svf.to_array());
            }
            run(kernel::Svf::from_array(std::array::from_fn(|i| state.aux[5 + i])), state, block, len);
            return;
        }
        let (hz, q, boost) = self.targets(parameters, at);
        if kind == Kind::Level {
            if boost.abs() > 1e-9 {
                let gain = 10f64.powf(boost / 20.);
                for channel in block.iter_mut() {
                    channel[..len].iter_mut().for_each(|v| *v *= gain);
                }
            }
            return;
        }
        let gainful = matches!(
            kind,
            Kind::Peak | Kind::LowShelf | Kind::HighShelf | Kind::FirstOrderLowShelf | Kind::FirstOrderHighShelf
        );
        if gainful && boost.abs() < 0.01 {
            return;
        }
        // aux: hertz, q, boost, valid flag, then the six TPT coefficients.
        let key = [hz, q, boost];
        if state.aux[3] == 0. || state.aux[..3] != key {
            state.aux[3] = 0.;
            if let Some(svf) = design(self.rate, self.band.law, kind, self.band.poles, hz, q, boost) {
                state.aux[..3].copy_from_slice(&key);
                state.aux[3] = 1.;
                state.aux[4..10].copy_from_slice(&svf.to_array());
            }
        }
        if state.aux[3] == 0. {
            return;
        }
        run(kernel::Svf::from_array(std::array::from_fn(|i| state.aux[4 + i])), state, block, len);
    }
}

/// Design one band and realise it as the kernel's section.
pub(super) fn design(rate: u32, law: Law, kind: Kind, poles: u8, hz: f64, q: f64, boost: f64) -> Option<kernel::Svf> {
    let first = matches!(kind, Kind::FirstOrderLowShelf | Kind::FirstOrderHighShelf)
        || (poles == 1 && matches!(kind, Kind::LowPass | Kind::HighPass));
    let design = if first {
        Design::FirstOrder
    } else if law == Law::Cookbook {
        Design::Cookbook
    } else {
        Design::Matched
    };
    kernel::Svf::from_biquad(&biquad::design(design, kind, rate, hz, q, boost)?)
}

/// Both channels through one section, then flush the states.
pub(super) fn run(svf: kernel::Svf, state: &mut ProcessorState, block: &mut Planar, len: usize) {
    let [left, right] = block;
    svf.run_stereo(&mut state.z, [&mut left[..len], &mut right[..len]]);
    state.z = state.z.map(|s| s.map(flush));
}
