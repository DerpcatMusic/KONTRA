//! Port from v1 0cb7a8a0:src/engine/filter.rs: a live TPT bell, not native EQ.
use super::{
    Parameter, Planar, ProcessorState,
    control::{ControlRamp, ControlRange, PreparedParameter},
};
use crate::Error;

/// A peaking band with a real, voice-local gain lane. Static RBJ bands keep
/// their existing path; this owner preserves v1's flat-band state suspension.
#[derive(Clone, Copy, Debug)]
pub struct PeakingEq {
    pub frequency_hz: f64,
    pub bandwidth_octaves: f64,
    pub gain_db: Parameter,
}
impl PeakingEq {
    pub(super) fn valid(self) -> bool {
        [self.frequency_hz, self.bandwidth_octaves]
            .iter()
            .all(|v| v.is_finite() && (*v as f32).is_finite() && (*v as f32) > 0.)
            && self.gain_db.valid()
            && !matches!(self.gain_db, Parameter::Expression { .. })
    }
    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Result<Eq, Error> {
        if rate == 0 || !self.valid() {
            return Err(Error::InvalidInput);
        }
        let frequency = (self.frequency_hz as f32).min(0.49 * rate as f32);
        let bandwidth = self.bandwidth_octaves as f32;
        // v1 Proto::bell; frequency and width are immutable in this slice.
        let g = (std::f32::consts::PI * frequency / rate as f32).tan();
        let q = 1. / (2. * (std::f32::consts::LN_2 * 0.5 * bandwidth).sinh());
        for gain in self.gain_db.bounds() {
            if !coefficients(g, q, gain as f32)
                .iter()
                .all(|v| v.is_finite())
            {
                return Err(Error::InvalidInput);
            }
        }
        Ok(Eq {
            g,
            q,
            frequency,
            bandwidth,
            gain: self.gain_db.compile(bindings),
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Eq {
    g: f32,
    q: f32,
    frequency: f32,
    bandwidth: f32,
    gain: PreparedParameter,
}
fn coefficients(g: f32, q: f32, gain_db: f32) -> [f32; 6] {
    let a = 10f32.powf(gain_db / 40.);
    let k = 1. / (q * a);
    let a1 = 1. / (1. + g * (g + k));
    let a2 = g * a1;
    [a1, a2, g * a2, 1., k * (a * a - 1.), 0.]
}
impl Eq {
    pub(crate) fn trace_parameters(self) -> [(&'static str, PreparedParameter); 3] {
        [
            (
                "frequency_hz",
                PreparedParameter::Constant(f64::from(self.frequency)),
            ),
            (
                "bandwidth_octaves",
                PreparedParameter::Constant(f64::from(self.bandwidth)),
            ),
            ("gain_db", self.gain),
        ]
    }
    pub(super) fn is_flat(self, parameters: &[ControlRamp], at: u64, len: usize) -> bool {
        len == 0
            || ((self.gain.value(parameters, at, None) as f32).abs() < 0.01
                && (self.gain.value(parameters, at + len as u64 - 1, None) as f32).abs() < 0.01)
    }
    pub(super) fn process(
        self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
    ) {
        // A monotone base ramp plus held voice modulation cannot leave this flat interval.
        if self.is_flat(parameters, at, len) {
            return;
        }
        let mut histories = state.z.map(|channel| channel.map(|v| v as f32));
        let mut c = std::array::from_fn(|i| state.aux[i + 1] as f32);
        let [left, right] = block;
        for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
            let gain = self.gain.value(parameters, at + i as u64, None) as f32;
            if gain.abs() < 0.01 {
                continue;
            }
            if state.aux[7] == 0. || state.aux[0] != f64::from(gain) {
                c = coefficients(self.g, self.q, gain);
                state.aux[0] = f64::from(gain);
                for (dst, value) in state.aux[1..7].iter_mut().zip(c) {
                    *dst = f64::from(value);
                }
                state.aux[7] = 1.;
            }
            let [a1, a2, a3, m0, m1, m2] = c;
            let (b1, b2, b3) = (2. * a1 - 1., 2. * a2, 2. * a3);
            // v1 Section::process_body scalar recurrence, adapted to planar f64 I/O.
            for (sample, history) in [l, r].into_iter().zip(&mut histories) {
                let [s1, s2] = *history;
                let x = *sample as f32;
                let v3 = x - s2;
                let v1 = a1 * s1 + a2 * v3;
                let v2 = s2 + a2 * s1 + a3 * v3;
                *history = [b1 * s1 + b2 * v3, s2 + b2 * s1 + b3 * v3];
                *sample = f64::from(m0 * x + m1 * v1 + m2 * v2);
            }
        }
        state.z = histories
            .map(|channel| channel.map(|v| f64::from(if v.abs() < 1e-20 { 0. } else { v })));
    }
}
