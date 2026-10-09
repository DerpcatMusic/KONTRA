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
    fn tune(
        self,
        state: &mut ProcessorState,
        gain: f32,
        frequency: f32,
        bandwidth: f32,
    ) -> [f32; 6] {
        if state.aux[7] == 0.
            || state.aux[0] != f64::from(gain)
            || state.aux[8] != f64::from(frequency)
            || state.aux[9] != f64::from(bandwidth)
        {
            let c = coefficients(self.rate, frequency, bandwidth, gain);
            state.aux[0] = f64::from(gain);
            state.aux[8] = f64::from(frequency);
            state.aux[9] = f64::from(bandwidth);
            for (dst, value) in state.aux[1..7].iter_mut().zip(c) {
                *dst = f64::from(value);
            }
            state.aux[7] = 1.;
            c
        } else {
            std::array::from_fn(|i| state.aux[i + 1] as f32)
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
        if self.is_flat(parameters, at, len) {
            return;
        }
        let mut histories = state.z.map(|channel| channel.map(|v| v as f32));
        let held = self
            .gain
            .held(parameters, at, len)
            .zip(self.frequency.held(parameters, at, len))
            .zip(self.bandwidth.held(parameters, at, len))
            .map(|((gain, frequency), bandwidth)| {
                [gain as f32, frequency as f32, bandwidth as f32]
            });
        let [left, right] = block;
        if let Some([gain, frequency, bandwidth]) = held {
            let c = self.tune(state, gain, frequency, bandwidth);
            let [[l1, l2], [r1, r2]] = histories;
            let mut history = [l1, l2, r1, r2];
            sampler_simd::filter_section_v1(c, &mut history, &mut left[..len], &mut right[..len]);
            histories = [[history[0], history[1]], [history[2], history[3]]];
        } else {
            for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
                let gain = self.gain.value(parameters, at + i as u64, None) as f32;
                if gain.abs() < 0.01 {
                    continue;
                }
                let frequency = self.frequency.value(parameters, at + i as u64, None) as f32;
                let bandwidth = self.bandwidth.value(parameters, at + i as u64, None) as f32;
                let c = self.tune(state, gain, frequency, bandwidth);
                for (sample, history) in [l, r].into_iter().zip(&mut histories) {
                    *sample = f64::from(section(c, history, *sample as f32));
                }
            }
        }
        state.z = histories
            .map(|channel| channel.map(|v| f64::from(if v.abs() < 1e-20 { 0. } else { v })));
    }
}

// v1 0cb7a8a0:filter.rs Section::process_body; same f32 operation order.
#[inline(always)]
fn section([a1, a2, a3, m0, m1, m2]: [f32; 6], history: &mut [f32; 2], x: f32) -> f32 {
    let (b1, b2, b3) = (2. * a1 - 1., 2. * a2, 2. * a3);
    let [s1, s2] = *history;
    let v3 = x - s2;
    let v1 = a1 * s1 + a2 * v3;
    let v2 = s2 + a2 * s1 + a3 * v3;
    *history = [b1 * s1 + b2 * v3, s2 + b2 * s1 + b3 * v3];
    m0 * x + m1 * v1 + m2 * v2
}

#[cfg(test)]
mod tests {
    use super::*;

    // Literal v1 Section::process_body two-frame SSE arithmetic, expressed as scalars.
    #[cfg(target_arch = "x86_64")]
    fn v1_section(c: [f32; 6], s: &mut [f32; 4], left: &mut [f64], right: &mut [f64]) {
        let [a1, a2, a3, m0, m1, m2] = c;
        let (b1, b2, b3) = (2. * a1 - 1., 2. * a2, 2. * a3);
        let (e11, e12, e21, e22) = (b1 - 1., -b2, b2, -b3);
        let f11 = e11 * e11 + e12 * e21 + 2. * e11;
        let f12 = e11 * e12 + e12 * e22 + 2. * e12;
        let f21 = e21 * e11 + e22 * e21 + 2. * e21;
        let f22 = e21 * e12 + e22 * e22 + 2. * e22;
        let (g1, g2) = (b2 + e11 * b2 + e12 * b3, b3 + e21 * b2 + e22 * b3);
        let (c1, c2, d) = (
            m1 * a1 + m2 * a2,
            m2 * (1. - a3) - m1 * a2,
            m0 + m1 * a2 + m2 * a3,
        );
        let n = left.len();
        for i in (0..n / 2 * 2).step_by(2) {
            for ch in 0..2 {
                let frames = if ch == 0 { &mut *left } else { &mut *right };
                let (x0, x1) = (frames[i] as f32, frames[i + 1] as f32);
                let (s1, s2) = (s[2 * ch], s[2 * ch + 1]);
                let (u1, u2) = (b2 * x0 + 0. * x1, b3 * x0 + 0. * x1);
                let (t1, t2) = (
                    (e11 * s1 + u1) + (e12 * s2 + s1),
                    (e21 * s1 + u2) + (e22 * s2 + s2),
                );
                frames[i] = f64::from((c1 * s1 + c2 * s2) + d * x0);
                frames[i + 1] = f64::from((c1 * t1 + c2 * t2) + d * x1);
                let (u1, u2) = (g1 * x0 + b2 * x1, g2 * x0 + b3 * x1);
                s[2 * ch] = (f11 * s1 + u1) + (f12 * s2 + s1);
                s[2 * ch + 1] = (f21 * s1 + u2) + (f22 * s2 + s2);
            }
        }
        if n % 2 == 1 {
            for ch in 0..2 {
                let frames = if ch == 0 { &mut *left } else { &mut *right };
                let x = frames[n - 1] as f32;
                let (s1, s2) = (s[2 * ch], s[2 * ch + 1]);
                frames[n - 1] = f64::from((c1 * s1 + c2 * s2) + d * x);
                s[2 * ch] = (e11 * s1 + b2 * x) + (e12 * s2 + s1);
                s[2 * ch + 1] = (e21 * s1 + b3 * x) + (e22 * s2 + s2);
            }
        }
        *s = s.map(|v| if v.abs() < 1e-20 { 0. } else { v });
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn held_eq_nulls_against_literal_v1_two_frame_section() {
        use super::super::control::PreparedParameter;
        let eq = Eq {
            rate: 48000,
            frequency: PreparedParameter::Constant(0.6),
            bandwidth: PreparedParameter::Constant(0.4),
            gain: PreparedParameter::Constant(6.),
        };
        // Evaluate the oracle's transcendental coefficients at runtime, like playback.
        let [frequency, bandwidth, gain] = std::hint::black_box([0.6, 0.4, 6.]);
        let c = coefficients(48000, frequency, bandwidth, gain);
        for len in [1, 2, 3, 7, 31, 32, 33, 63, 64] {
            let mut state = ProcessorState::default();
            state.z = [[0.11, -0.02], [0.21, -0.03]];
            let mut oracle = [0.11, -0.02, 0.21, -0.03];
            for block in 0..3 {
                let mut actual: Planar = std::array::from_fn(|ch| {
                    std::array::from_fn(|i| {
                        f64::from(((i + ch * 17 + block * 3) as f32 * 0.13).sin() * 0.25)
                    })
                });
                let mut expected = actual;
                let [l, r] = &mut expected;
                v1_section(c, &mut oracle, &mut l[..len], &mut r[..len]);
                eq.process(&mut state, &[], &mut actual, len, (block * len) as u64);
                assert_eq!(
                    actual.map(|c| c.map(f64::to_bits)),
                    expected.map(|c| c.map(f64::to_bits)),
                    "v1 two-frame output for len={len} block={block}"
                );
                assert_eq!(
                    state.z.map(|c| c.map(|v| (v as f32).to_bits())),
                    [
                        [oracle[0].to_bits(), oracle[1].to_bits()],
                        [oracle[2].to_bits(), oracle[3].to_bits()]
                    ]
                );
            }
        }
    }

    #[test]
    fn held_eq_reads_controls_once_and_reuses_coefficients() {
        use super::super::control::{PARAMETER_READS, PreparedParameter};
        let eq = Eq {
            rate: 48000,
            frequency: PreparedParameter::Control(0),
            bandwidth: PreparedParameter::Control(1),
            gain: PreparedParameter::Control(2),
        };
        let parameters = [
            ControlRamp::test_ramp(0.2, 0.6, 0, 0),
            ControlRamp::test_ramp(0.1, 0.4, 0, 0),
            ControlRamp::test_ramp(0., 6., 0, 0),
        ];
        let mut state = ProcessorState::default();
        for at in [0, 32, 64] {
            let mut block = [[0.125; super::super::BLOCK]; 2];
            PARAMETER_READS.with(|n| n.set(0));
            eq.process(&mut state, &parameters, &mut block, 32, at);
            let reads = PARAMETER_READS.with(|n| n.get());
            eprintln!("held EQ reads={reads} frames=32 at={at}");
            assert!(
                reads <= 8,
                "held EQ performed {reads} target reads for 32 frames"
            );
        }
    }

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
