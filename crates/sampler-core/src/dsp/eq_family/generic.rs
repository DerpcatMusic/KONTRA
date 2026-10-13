//! The RBJ-biquad band of the universal EQ. Hertz/Q/decibel bands (UVI, scripted inserts, the
//! generic fallback of unproved Kontakt multiband kinds) and the Solid G-EQ bands, whose knobs
//! pass through the proved spline tables ([`super::solid`]) before the same (generic) biquad.
//! Controls are read once per block (the control ramp still applies at the block edge); the
//! biquad keeps its state across coefficient changes. No allocation, no per-sample trig.
use super::super::{
    Biquad, FilterKind, Planar, ProcessorState, flush,
    control::{ControlRamp, PreparedParameter},
};
use super::{EqBand, Kind, Law, solid};

#[derive(Clone, Copy)]
pub(crate) struct Generic {
    pub(super) rate: u32,
    pub(super) band: EqBand,
    pub(super) frequency: PreparedParameter,
    pub(super) q: PreparedParameter,
    pub(super) gain: PreparedParameter,
    pub(super) reference: PreparedParameter,
    pub(super) enabled: PreparedParameter,
}

/// 6 dB/octave low or high pass as a degenerate biquad.
fn first_order(rate: u32, hz: f64, high: bool) -> Biquad {
    let g = (std::f64::consts::PI * hz / f64::from(rate)).tan();
    let d = 1. + g;
    let (b0, b1) = if high { (1. / d, -1. / d) } else { (g / d, g / d) };
    Biquad { rate, b: [b0, b1, 0.], a: [(g - 1.) / d, 0.] }
}

impl Generic {
    pub(crate) fn trace_parameters(self) -> Vec<(&'static str, PreparedParameter)> {
        vec![
            ("frequency", self.frequency),
            ("q", self.q),
            ("gain_db", self.gain),
            ("reference_db", self.reference),
            ("enabled", self.enabled),
        ]
    }

    /// Hertz, Q and boost in dB for this block, by law.
    pub(super) fn targets(self, parameters: &[ControlRamp], at: u64) -> (f64, f64, f64) {
        let read = |p: PreparedParameter| p.value(parameters, at, None);
        let (hz, q, boost) = match self.band.law {
            Law::Solid(section) => {
                let (hz, q, db) = solid::physical(section, read(self.frequency), read(self.q), read(self.gain));
                (hz, q, db - read(self.reference))
            }
            _ => (read(self.frequency), read(self.q), read(self.gain) - read(self.reference)),
        };
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
        let (frequency, q, boost) = self.targets(parameters, at);
        let kind = match self.band.kind {
            Kind::Level => {
                if boost.abs() > 1e-9 {
                    let gain = 10f64.powf(boost / 20.);
                    for channel in block.iter_mut() {
                        channel[..len].iter_mut().for_each(|v| *v *= gain);
                    }
                }
                return;
            }
            Kind::LowShelf => FilterKind::LowShelf { gain_db: boost },
            Kind::HighShelf => FilterKind::HighShelf { gain_db: boost },
            Kind::FirstOrderLowShelf => FilterKind::FirstOrderLowShelf { gain_db: boost },
            Kind::FirstOrderHighShelf => FilterKind::FirstOrderHighShelf { gain_db: boost },
            Kind::Peak => FilterKind::Peak { gain_db: boost },
            Kind::LowPass => FilterKind::LowPass,
            Kind::HighPass => FilterKind::HighPass,
            Kind::BandPass => FilterKind::BandPass,
            Kind::Notch => FilterKind::Notch,
        };
        if matches!(
            self.band.kind,
            Kind::LowShelf | Kind::HighShelf | Kind::FirstOrderLowShelf | Kind::FirstOrderHighShelf | Kind::Peak
        ) && boost.abs() < 0.01
        {
            return;
        }
        // aux: frequency, q, boost, valid flag, then b0 b1 b2 a1 a2.
        let key = [frequency, q, boost];
        if state.aux[3] == 0. || state.aux[..3] != key {
            state.aux[3] = 0.;
            let design = if self.band.poles == 1 && matches!(self.band.kind, Kind::LowPass | Kind::HighPass) {
                Ok(first_order(self.rate, frequency, self.band.kind == Kind::HighPass))
            } else {
                Biquad::new(self.rate, kind, frequency, q)
            };
            if let Ok(f) = design {
                state.aux[..3].copy_from_slice(&key);
                state.aux[3] = 1.;
                state.aux[4..7].copy_from_slice(&f.b);
                state.aux[7..9].copy_from_slice(&f.a);
            }
        }
        if state.aux[3] == 0. {
            return;
        }
        let filter = Biquad {
            rate: self.rate,
            b: [state.aux[4], state.aux[5], state.aux[6]],
            a: [state.aux[7], state.aux[8]],
        };
        let [mut zl, mut zr] = state.z;
        let [left, right] = block;
        for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
            (*l, zl) = filter.sample(*l, zl);
            (*r, zr) = filter.sample(*r, zr);
        }
        state.z = [zl.map(flush), zr.map(flush)];
    }
}
