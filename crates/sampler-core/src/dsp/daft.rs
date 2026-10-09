//! Kontakt's Daft filter (a 2-pole low or high pass with soft saturation,
//! 2x oversampled). DSP_SYSTEM_INVENTORY "Daft parameter laws and scheduling"
//! fixes the parameter laws, the 32-frame control cadence and the ramp length;
//! it does not recover the audio kernel.
// ponytail: unverified - the kernel is a saturating trapezoidal state-variable
// filter at twice the rate, with linear-interpolation up- and two-tap
// down-sampling, not the original's resampler or feedback nonlinearity. The
// cadence follows the absolute sample clock, not a per-instance countdown.
use super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::{Planar, ProcessorState};
use std::sync::OnceLock;

const TABLE: usize = 2401;
/// Frames between control updates.
const QUANTUM: u64 = 32;

/// `T[i] = 2^(i/60 - 20)`, filled once while a plan is prepared.
fn table() -> &'static [f32; TABLE] {
    static T: OnceLock<Box<[f32; TABLE]>> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0f32; TABLE]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = (i as f64 / 60. - 20.).exp2() as f32;
        }
        t
    })
}

/// Linear interpolation between adjacent table entries.
fn lookup(position: f32) -> f64 {
    let t = table();
    let position = position.clamp(0., (TABLE - 1) as f32);
    let i = (position as usize).min(TABLE - 2);
    let fraction = position - i as f32;
    f64::from(t[i] + fraction * (t[i + 1] - t[i]))
}

/// Normalized controls (0..=1): leading gain (12 dB at 1), cutoff, resonance
/// and response (below 0.5 low pass, from 0.5 high pass).
#[derive(Clone, Copy, Debug)]
pub struct DaftSettings {
    pub gain: Parameter,
    pub cutoff: Parameter,
    pub resonance: Parameter,
    pub response: Parameter,
}

impl DaftSettings {
    fn parameters(&self) -> [Parameter; 4] {
        [self.gain, self.cutoff, self.resonance, self.response]
    }

    pub(super) fn valid(&self) -> bool {
        self.parameters()
            .iter()
            .all(|p| p.valid() && !matches!(p, Parameter::Expression { .. }))
    }

    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Daft {
        table();
        Daft {
            rate: f64::from(rate) * 2.,
            lanes: self.parameters().map(|p| p.compile(bindings)),
            // About 2 ms of input frames, in whole control quanta.
            ramp_quanta: ((0.002 * f64::from(rate) / QUANTUM as f64).ceil() as u32).max(1),
            modulation_index: usize::MAX,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Daft {
    /// The doubled processing rate.
    rate: f64,
    lanes: [PreparedParameter; 4],
    ramp_quanta: u32,
    pub(crate) modulation_index: usize,
}

// Layout of `ProcessorState::aux`.
const CURRENT: usize = 0;
const DELTA: usize = 4;
const TARGET: usize = 8;
const REMAINING: usize = 12;
const STARTED: usize = 13;
const PREVIOUS: usize = 14;

struct Coefficients {
    g: f64,
    inverse: f64,
    damping: f64,
    amplitude: f64,
    high: f64,
}

impl Daft {
    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 4] { std::array::from_fn(|i| (["gain","cutoff","resonance","response"][i],self.lanes[i])) }
    /// The four ramped quantities for the normalized controls.
    fn targets(x: [f64; 4]) -> [f64; 4] {
        let x = x.map(|v| if v.is_nan() { 0. } else { v.clamp(0., 1.) });
        [
            lookup(x[0] as f32 * 119.589_41 + 1_200.),
            lookup(x[1] as f32 * 625. + 1_481.881_6).clamp(1., 30_000.),
            1. - (1. - x[2]) * (1. - x[2]),
            f64::from((2. * x[3]).trunc().clamp(0., 1.) as u8),
        ]
    }

    fn coefficients(&self, now: &[f64]) -> Coefficients {
        let hz = now[1].min(self.rate * 0.45);
        let g = (std::f64::consts::PI * hz / self.rate).tan();
        let feedback = 1.6 - (hz * 0.5).min(14_000.) * 5.714_285_725_844_093e-5;
        let damping = 2. - now[2] * feedback;
        Coefficients {
            g,
            inverse: 1. / (1. + damping * g + g * g),
            damping,
            amplitude: now[0],
            high: now[3],
        }
    }

    /// One 2x-rate sample; `s` is the filter state.
    fn tick(c: &Coefficients, s: &mut [f64; 2], x: f64) -> f64 {
        let high = (x - (c.damping + c.g) * s[0] - s[1]) * c.inverse;
        let band = c.g * high + s[0];
        let low = c.g * band + s[1];
        s[0] = soft(c.g * high + band);
        s[1] = c.g * band + low;
        c.amplitude * ((1. - c.high) * low + c.high * high)
    }

    fn control(&self, state: &mut ProcessorState, parameters: &[ControlRamp], at: u64, modulation: [f64; 4]) {
        let mut x = self.lanes.map(|lane| lane.value(parameters, at, None));
        x[0] += modulation[2];
        x[1] += modulation[0];
        x[2] += modulation[1];
        let target = Self::targets(x);
        let a = &mut state.aux;
        if a[STARTED] == 0. {
            a[CURRENT..CURRENT + 4].copy_from_slice(&target);
            a[TARGET..TARGET + 4].copy_from_slice(&target);
            a[STARTED] = 1.;
            return;
        }
        if a[REMAINING] > 0. {
            a[REMAINING] -= 1.;
            if a[REMAINING] == 0. {
                a.copy_within(TARGET..TARGET + 4, CURRENT);
                a[DELTA..DELTA + 4].fill(0.);
            }
        }
        if a[TARGET..TARGET + 4] != target {
            let samples = f64::from(self.ramp_quanta) * QUANTUM as f64 * 2.;
            for k in 0..4 {
                let delta = (target[k] - a[CURRENT + k]) / samples;
                if delta * delta < 1e-15 {
                    a[CURRENT + k] = target[k];
                    a[DELTA + k] = 0.;
                } else {
                    a[DELTA + k] = delta;
                }
            }
            a[TARGET..TARGET + 4].copy_from_slice(&target);
            a[REMAINING] = f64::from(self.ramp_quanta);
        }
    }

    /// Filter `len` planar frames in place from absolute frame `at`.
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) {
        let mut i = 0;
        while i < len {
            let t = at + i as u64;
            if t % QUANTUM == 0 || state.aux[STARTED] == 0. {
                self.control(state, parameters, t, modulation);
            }
            let run = ((QUANTUM - t % QUANTUM) as usize).min(len - i);
            let ramping = state.aux[DELTA..DELTA + 4].iter().any(|d| *d != 0.);
            let mut coefficients = self.coefficients(&state.aux[CURRENT..CURRENT + 4]);
            for n in i..i + run {
                let x = [block[0][n], block[1][n]];
                let inputs = [0, 1].map(|c| [0.5 * (state.aux[PREVIOUS + c] + x[c]), x[c]]);
                state.aux[PREVIOUS..PREVIOUS + 2].copy_from_slice(&x);
                let mut out = [0.; 2];
                for half in 0..2 {
                    if ramping {
                        for k in 0..4 {
                            state.aux[CURRENT + k] += state.aux[DELTA + k];
                        }
                        coefficients = self.coefficients(&state.aux[CURRENT..CURRENT + 4]);
                    }
                    for c in 0..2 {
                        out[c] += Self::tick(&coefficients, &mut state.z[c], inputs[c][half]);
                    }
                }
                block[0][n] = 0.5 * out[0];
                block[1][n] = 0.5 * out[1];
            }
            i += run;
        }
        for v in &mut state.z.iter_mut().flatten() {
            *v = super::flush(*v);
        }
    }
}

/// Unity-slope soft limit to +-1 (a rational tanh).
fn soft(x: f64) -> f64 {
    if x.abs() >= 3. { x.signum() } else { x * (27. + x * x) / (27. + 9. * x * x) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_laws_match_the_specified_table_points() {
        // DSP_SYSTEM_INVENTORY: normalized input, interpolated amplitude, cutoff Hz.
        for (x, amplitude, hz) in [
            (0., 1., 25.956_726),
            (0.25, 1.412_546, 157.827_438),
            (0.5, 1.995_283, 959.662_048),
            (0.75, 2.818_422, 5_835.129_395),
            (1., 3.981_133, 30_000.),
        ] {
            let t = Daft::targets([x, x, 0., 0.]);
            assert!((t[0] / amplitude - 1.).abs() < 1e-4, "{x}: {}", t[0]);
            assert!((t[1] / hz - 1.).abs() < 1e-4, "{x}: {}", t[1]);
        }
        // Signed leading gain: -0.25 reads 0.707950, and is clamped by the outer path.
        assert!((lookup(-0.25 * 119.589_41 + 1_200.) - 0.707_950).abs() < 1e-4);
        let t = Daft::targets([0., 0., 0.5, 0.49]);
        assert_eq!((t[2], t[3]), (0.75, 0.));
        assert_eq!(Daft::targets([0., 0., 1., 0.5])[3], 1.);
    }

    #[test]
    fn ramp_length_follows_the_sample_rate() {
        // Ramp countdown at a 32-frame quantum, from the same table.
        for (rate, quanta) in [(8_000, 1), (44_100, 3), (48_000, 3), (96_000, 6), (192_000, 12)] {
            let s = DaftSettings {
                gain: Parameter::Constant(0.),
                cutoff: Parameter::Constant(0.),
                resonance: Parameter::Constant(0.),
                response: Parameter::Constant(0.),
            };
            assert_eq!(s.compile(rate, &mut Vec::new()).ramp_quanta, quanta, "{rate}");
        }
    }
}
