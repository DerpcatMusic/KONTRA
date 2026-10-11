//! Control-driven stages for scripted insert effects: a biquad whose frequency,
//! Q, gain and enable follow controls, and a 2x2 matrix whose cells do. Values
//! are read once per 64-frame block (the control ramp still applies at the
//! block edge); the biquad keeps its state across coefficient changes.
use super::{
    Biquad, FilterKind, Parameter, Planar, ProcessorState, flush, kernels,
    control::{ControlRamp, ControlRange, PreparedParameter},
};
use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveShape {
    /// A flat gain in decibels (`gain_db - reference_db`), no filtering.
    Level,
    LowShelf,
    HighShelf,
    /// 6 dB/octave shelves; the Q has no effect.
    FirstOrderLowShelf,
    FirstOrderHighShelf,
    Peak,
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// `gain_db - reference_db` is the boost; shelves and bells with no boost, and
/// every shape while `enabled` is below 0.5, pass the signal untouched.
#[derive(Clone, Copy, Debug)]
pub struct LiveFilter {
    pub shape: LiveShape,
    pub frequency_hz: Parameter,
    pub q: Parameter,
    pub gain_db: Parameter,
    pub reference_db: Parameter,
    pub enabled: Parameter,
}

impl LiveFilter {
    pub(super) fn valid(self) -> bool {
        [
            self.frequency_hz,
            self.q,
            self.gain_db,
            self.reference_db,
            self.enabled,
        ]
        .iter()
        .all(|p| p.valid() && !matches!(p, Parameter::Expression { .. }))
    }

    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Result<Live, Error> {
        if rate == 0 || !self.valid() {
            return Err(Error::InvalidInput);
        }
        Ok(Live {
            rate,
            shape: self.shape,
            frequency: self.frequency_hz.compile(bindings),
            q: self.q.compile(bindings),
            gain: self.gain_db.compile(bindings),
            reference: self.reference_db.compile(bindings),
            enabled: self.enabled.compile(bindings),
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Live {
    rate: u32,
    shape: LiveShape,
    frequency: PreparedParameter,
    q: PreparedParameter,
    gain: PreparedParameter,
    reference: PreparedParameter,
    enabled: PreparedParameter,
}

impl Live {
    pub(crate) fn trace_parameters(self) -> [(&'static str, PreparedParameter); 5] {
        [
            ("frequency_hz", self.frequency),
            ("q", self.q),
            ("gain_db", self.gain),
            ("reference_db", self.reference),
            ("enabled", self.enabled),
        ]
    }

    pub(super) fn process(
        self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
    ) {
        let read = |p: PreparedParameter| p.value(parameters, at, None);
        if read(self.enabled) < 0.5 {
            return;
        }
        let boost = read(self.gain) - read(self.reference);
        let kind = match self.shape {
            LiveShape::Level => {
                if boost.abs() > 1e-9 {
                    let gain = 10f64.powf(boost / 20.);
                    for channel in block.iter_mut() {
                        channel[..len].iter_mut().for_each(|v| *v *= gain);
                    }
                }
                return;
            }
            LiveShape::LowShelf => FilterKind::LowShelf { gain_db: boost },
            LiveShape::HighShelf => FilterKind::HighShelf { gain_db: boost },
            LiveShape::FirstOrderLowShelf => FilterKind::FirstOrderLowShelf { gain_db: boost },
            LiveShape::FirstOrderHighShelf => FilterKind::FirstOrderHighShelf { gain_db: boost },
            LiveShape::Peak => FilterKind::Peak { gain_db: boost },
            LiveShape::LowPass => FilterKind::LowPass,
            LiveShape::HighPass => FilterKind::HighPass,
            LiveShape::BandPass => FilterKind::BandPass,
            LiveShape::Notch => FilterKind::Notch,
        };
        if matches!(
            self.shape,
            LiveShape::LowShelf
                | LiveShape::HighShelf
                | LiveShape::FirstOrderLowShelf
                | LiveShape::FirstOrderHighShelf
                | LiveShape::Peak
        ) && boost.abs() < 0.01
        {
            return;
        }
        let frequency = read(self.frequency).clamp(1., 0.49 * f64::from(self.rate));
        let q = read(self.q).clamp(0.05, 50.);
        // aux: frequency, q, boost, valid flag, then b0 b1 b2 a1 a2.
        let key = [frequency, q, boost];
        if state.aux[3] == 0. || state.aux[..3] != key {
            state.aux[3] = 0.;
            if let Ok(f) = Biquad::new(self.rate, kind, frequency, q) {
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

/// A 2x2 matrix (rows are output L/R, columns input L/R) with control cells.
pub(super) fn matrix(
    cells: &[PreparedParameter; 4],
    parameters: &[ControlRamp],
    block: &mut Planar,
    len: usize,
    at: u64,
) {
    let [left, right] = block;
    for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
        let c = cells.map(|cell| cell.value(parameters, at + i as u64, None));
        [*l, *r] = kernels::matrix([*l, *r], [[c[0], c[1]], [c[2], c[3]]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(shape: LiveShape, gain: f64, reference: f64, enabled: f64) -> f64 {
        let c = Parameter::Constant;
        let live = LiveFilter {
            shape,
            frequency_hz: c(1000.),
            q: c(0.707),
            gain_db: c(gain),
            reference_db: c(reference),
            enabled: c(enabled),
        }
        .compile(48000, &mut Vec::new())
        .unwrap();
        let mut state = ProcessorState::default();
        let mut out = 0.;
        for block in 0..40 {
            let mut x = [[1.; super::super::BLOCK]; 2];
            live.process(&mut state, &[], &mut x, super::super::BLOCK, block * 64);
            out = x[0][63];
        }
        out
    }

    #[test]
    fn shelf_level_and_enable_follow_their_parameters() {
        assert!((run(LiveShape::LowShelf, 12., 0., 1.) - 10f64.powf(0.6)).abs() < 1e-3);
        // Relative gain: 12 dB above a 12 dB reference is flat.
        assert!((run(LiveShape::LowShelf, 12., 12., 1.) - 1.).abs() < 1e-9);
        assert!((run(LiveShape::Level, 6.0206, 0., 1.) - 2.).abs() < 1e-3);
        assert_eq!(run(LiveShape::HighShelf, 12., 0., 0.), 1.);
    }
}
