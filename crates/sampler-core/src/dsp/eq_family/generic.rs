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
            // The native G_EQ biquad `poles` of this section, from the saved knobs.
            let read = |p: PreparedParameter| p.value(parameters, at, None);
            let key = [read(self.frequency), read(self.q), read(self.gain)];
            if state.aux[3] == 0. || state.aux[..3] != key {
                let [b0, b1, b2, a1, a2] = solid::coefficients(
                    section,
                    kind,
                    usize::from(self.band.poles),
                    f64::from(self.rate),
                    key[0],
                    key[1],
                    key[2],
                );
                let svf = kernel::Svf::from_biquad(&kernel::Biquad { b: [b0, b1, b2], a: [a1, a2] })
                    .unwrap_or(kernel::Svf::PASS);
                state.aux[..3].copy_from_slice(&key);
                state.aux[3] = 1.;
                state.aux[4..10].copy_from_slice(&svf.to_array());
            }
            run(kernel::Svf::from_array(std::array::from_fn(|i| state.aux[4 + i])), state, block, len);
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
    let f = if first {
        kernel::first_order(kind, rate, hz, boost)
    } else if law == Law::Cookbook {
        kernel::cookbook(kind, rate, hz, q, boost)
    } else {
        kernel::design(kind, rate, hz, q, boost)
    };
    kernel::Svf::from_biquad(&f?)
}

/// Both channels through one section, then flush the states.
pub(super) fn run(svf: kernel::Svf, state: &mut ProcessorState, block: &mut Planar, len: usize) {
    let [left, right] = block;
    svf.run_stereo(&mut state.z, [&mut left[..len], &mut right[..len]]);
    state.z = state.z.map(|s| s.map(flush));
}
