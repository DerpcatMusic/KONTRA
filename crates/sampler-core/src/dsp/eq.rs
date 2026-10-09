//! Port from v1 0cb7a8a0:src/engine/filter.rs: a live TPT bell, not native EQ.
use super::{
    Parameter, Planar, ProcessorState,
    control::{ControlRamp, ControlRange, PreparedParameter},
};
use crate::Error;

/// A peaking band with real, voice-local knob lanes. Static RBJ bands keep
/// their existing path; this owner preserves v1's flat-band state suspension.
#[derive(Clone, Copy, Debug)]
pub struct PeakingEq {
    /// Normalized v1 knob: 20 Hz * 10^(3*x).
    pub frequency: Parameter,
    /// Normalized v1 knob: 0.3 + 2.7*x octaves.
    pub bandwidth: Parameter,
    pub gain_db: Parameter,
}
impl PeakingEq {
    /// The same normalized frequency conversion and Nyquist guard as playback.
    pub fn frequency_hz(normalized: f32, rate: u32) -> f32 {
        (20. * 10f32.powf(3. * normalized.clamp(0., 1.))).min(0.49 * rate as f32)
    }

    /// The v1 normalized EQ bandwidth law, in octaves.
    pub fn bandwidth_octaves(normalized: f32) -> f32 {
        0.3 + 2.7 * normalized.clamp(0., 1.)
    }

    /// The TPT bell's Q, derived from its bandwidth knob, not a filter resonance knob.
    pub fn bandwidth_q(normalized: f32) -> f32 {
        1. / (2. * (std::f32::consts::LN_2 * 0.5 * Self::bandwidth_octaves(normalized)).sinh())
    }

    pub(super) fn valid(self) -> bool {
        [self.frequency, self.bandwidth].iter().all(|p| {
            p.valid()
                && !matches!(p, Parameter::Expression { .. })
                && p.bounds().iter().all(|v| (0. ..=1.).contains(v))
        }) && self.gain_db.valid()
            && !matches!(self.gain_db, Parameter::Expression { .. })
    }
    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Result<Eq, Error> {
        if rate == 0 || !self.valid() {
            return Err(Error::InvalidInput);
        }
        for frequency in self.frequency.bounds() {
            for bandwidth in self.bandwidth.bounds() {
                for gain in self.gain_db.bounds() {
                    if !coefficients(rate, frequency as f32, bandwidth as f32, gain as f32)
                        .iter()
                        .all(|v| v.is_finite())
                    {
                        return Err(Error::InvalidInput);
                    }
                }
            }
        }
        Ok(Eq {
            rate,
            frequency: self.frequency.compile(bindings),
            bandwidth: self.bandwidth.compile(bindings),
            gain: self.gain_db.compile(bindings),
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Eq {
    rate: u32,
    frequency: PreparedParameter,
    bandwidth: PreparedParameter,
    gain: PreparedParameter,
}
fn coefficients(rate: u32, frequency: f32, bandwidth: f32, gain_db: f32) -> [f32; 6] {
    // v1 band_settings and Proto::bell; clamp only after summing knob modulation.
    let hz = PeakingEq::frequency_hz(frequency, rate);
    let g = (std::f32::consts::PI * hz / rate as f32).tan();
    let q = PeakingEq::bandwidth_q(bandwidth);
    let a = 10f32.powf(gain_db / 40.);
    let k = 1. / (q * a);
    let a1 = 1. / (1. + g * (g + k));
    let a2 = g * a1;
    [a1, a2, g * a2, 1., k * (a * a - 1.), 0.]
}
impl Eq {
    pub(crate) fn trace_parameters(self) -> [(&'static str, PreparedParameter); 3] {
        [
            ("frequency_knob", self.frequency),
            ("bandwidth_knob", self.bandwidth),
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
            let frequency = self.frequency.value(parameters, at + i as u64, None) as f32;
            let bandwidth = self.bandwidth.value(parameters, at + i as u64, None) as f32;
            if state.aux[7] == 0.
                || state.aux[0] != f64::from(gain)
                || state.aux[8] != f64::from(frequency)
                || state.aux[9] != f64::from(bandwidth)
            {
                c = coefficients(self.rate, frequency, bandwidth, gain);
                state.aux[0] = f64::from(gain);
                state.aux[8] = f64::from(frequency);
                state.aux[9] = f64::from(bandwidth);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_preview_adapters_share_the_playback_knob_laws() {
        assert_eq!(PeakingEq::frequency_hz(0., 48000), 20.);
        assert_eq!(PeakingEq::frequency_hz(1., 48000), 20000.);
        assert_eq!(PeakingEq::frequency_hz(1., 8000), 3920.);
        assert_eq!(PeakingEq::bandwidth_octaves(0.), 0.3);
        assert_eq!(PeakingEq::bandwidth_octaves(1.), 3.);
        let q = PeakingEq::bandwidth_q(0.5);
        assert!((2. * (0.5 / f64::from(q)).asinh() / std::f64::consts::LN_2 - 1.65).abs() < 5e-7);
    }

    #[test]
    fn eq_knobs_reject_invalid_domains_before_rendering() {
        for value in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
            for frequency in [false, true] {
                let mut eq = PeakingEq {
                    frequency: Parameter::Constant(0.5),
                    bandwidth: Parameter::Constant(0.5),
                    gain_db: Parameter::Constant(12.),
                };
                if frequency {
                    eq.frequency = Parameter::Constant(value);
                } else {
                    eq.bandwidth = Parameter::Constant(value);
                }
                assert!(eq.compile(48000, &mut Vec::new()).is_err());
            }
        }
    }
}
