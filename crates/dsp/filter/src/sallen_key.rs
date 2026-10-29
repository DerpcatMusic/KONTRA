//! A two-pole Sallen-Key filter with its feedback through a saturating
//! curve: two trapezoidal integrators `y1`, `y2` (outputs `v = S + y/2`)
//! solved together by Newton each half sample.
//!
//! `y1 = T(a1 − nl − v1)`, `y2 = T(nl − a2 − v2 + v1)`, the feedback
//! `nl = m + a P(b m)` on `m = K(a2 + v2)`, `P(u) = u(u² + 27)/(9u² + 27)`;
//! `a1 = g·morph·x` and `a2 = g(morph − 1)x` the low and high inputs, the
//! output `(a2 + v2)/g` (morph 1 a low-pass, 0 a high-pass), `T = πf/rate`
//! and `K = 0.549 q`. The law is UVIWorkstation's MS20 (process
//! 0x1412afb10, step 0x1412b0b60, setup 0x1412ae600): two substeps on the
//! input's midpoint then its end, at most 20 iterations to 1e-6.

use kontra_dsp_core::flush;

/// The feedback resistor divider `10 kΩ/(10 kΩ + 8.2 kΩ)` on `q`.
const DIVIDER: f64 = 10_000.0 / 18_200.0;

#[derive(Clone, Debug, PartialEq)]
pub struct SallenKey {
    morph: f64,
    curve: [f64; 2],
    t: f64,
    k: f64,
    /// Per channel: the last `y1`, `y2`, the integrators `S1`, `S2` and the
    /// last inputs `a1`, `a2`.
    state: [[f64; 6]; 2],
}

impl SallenKey {
    /// A filter mixing low (`morph` 1) to high (0), its feedback curve `[a, b]`.
    #[must_use]
    pub fn new(morph: f32, curve: [f32; 2]) -> Self {
        Self {
            morph: f64::from(morph.clamp(0.0, 1.0)),
            curve: curve.map(f64::from),
            t: 0.0,
            k: 0.0,
            state: [[0.0; 6]; 2],
        }
    }

    /// Moves the cutoff to `hz` at `rate` and the resonance to `q` (0..=1).
    pub fn tune(&mut self, hz: f64, rate: f64, q: f64) {
        self.t = std::f64::consts::PI * hz.min(rate / 2.0) / rate.max(1.0);
        self.k = DIVIDER * q.clamp(0.0, 1.0);
    }

    /// Filters `io` in place, driving the circuit with `gain` (made up after).
    pub fn process(&mut self, io: &mut [[f32; 2]], gain: f64) {
        let gain = gain.max(1e-6);
        let (t, k, [a, b]) = (self.t, self.k, self.curve);
        for frame in io {
            for (io, s) in frame.iter_mut().zip(&mut self.state) {
                let [y1, y2, s1, s2, p1, p2] = s;
                let x = gain * f64::from(*io);
                let (in1, in2) = (self.morph * x, (self.morph - 1.0) * x);
                let mut out = 0.0;
                for _ in 0..2 {
                    let (u1, u2) = (0.5 * (*p1 + in1), 0.5 * (*p2 + in2));
                    for _ in 0..20 {
                        let (v1, v2) = (0.5 * *y1 + *s1, 0.5 * *y2 + *s2);
                        let m = k * (u2 + v2);
                        let nl = m + a * shape(b * m);
                        let dnl = 0.5 * k * (1.0 + a * b * slope(b * m));
                        let f1 = *y1 - t * (u1 - nl - v1);
                        let f2 = *y2 - t * (nl - u2 - v2 + v1);
                        let (j11, j12) = (1.0 + 0.5 * t, t * dnl);
                        let (j21, j22) = (-0.5 * t, 1.0 + 0.5 * t - t * dnl);
                        let det = j11 * j22 - j12 * j21;
                        let d1 = (f1 * j22 - f2 * j12) / det;
                        let d2 = (f2 * j11 - f1 * j21) / det;
                        *y1 -= d1;
                        *y2 -= d2;
                        if d1.abs().max(d2.abs()) < 1e-6 {
                            break;
                        }
                    }
                    out += u2 + *s2 + 0.5 * *y2;
                    *s1 += *y1;
                    *s2 += *y2;
                    (*p1, *p2) = (in1, in2);
                }
                *io = (out / (2.0 * gain)) as f32;
            }
        }
        flush(self.state.as_flattened_mut());
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.state.iter().flatten().all(|s| s.abs() < 1e-10)
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 6]; 2];
    }
}

fn shape(u: f64) -> f64 {
    let u2 = u * u;
    u * (u2 + 27.0) / (9.0 * u2 + 27.0)
}

/// `shape`'s derivative.
fn slope(u: f64) -> f64 {
    let u2 = u * u;
    ((u2 - 18.0) * u2 + 81.0) / ((9.0 * u2 + 54.0) * u2 + 81.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_db(morph: f32, q: f64, hz: f64, level: f32) -> f64 {
        let mut f = SallenKey::new(morph, [2.065_561, 1.749_079]);
        f.tune(1000.0, 48_000.0, q);
        let sine: Vec<[f32; 2]> = (0..24_000)
            .map(|n| {
                [level * (std::f64::consts::TAU * hz * f64::from(n) / 48_000.0).sin() as f32; 2]
            })
            .collect();
        let mut io = sine.clone();
        f.process(&mut io, 1.0);
        let power = |v: &[[f32; 2]]| {
            v[12_000..]
                .iter()
                .map(|x| f64::from(x[0]).powi(2))
                .sum::<f64>()
        };
        10.0 * (power(&io) / power(&sine)).log10()
    }

    #[test]
    fn without_resonance_it_is_two_poles_low_or_high() {
        // Two coincident one-poles: −6 dB at the cutoff, −12 dB an octave per octave beyond.
        assert!(gain_db(1.0, 0.0, 50.0, 0.01).abs() < 0.1);
        assert!((gain_db(1.0, 0.0, 1000.0, 0.01) + 6.0).abs() < 0.3);
        assert!((gain_db(1.0, 0.0, 8000.0, 0.01) + 36.0).abs() < 2.0);
        // The half-sample input interpolation shades the top: −3 dB at 15 kHz.
        assert!(gain_db(0.0, 0.0, 4000.0, 0.01).abs() < 0.5);
        assert!((gain_db(0.0, 0.0, 15_000.0, 0.01) + 3.1).abs() < 0.3);
        assert!(gain_db(0.0, 0.0, 100.0, 0.01) < -20.0);
    }

    #[test]
    fn resonance_peaks_at_the_cutoff_and_loud_input_stays_bounded() {
        assert!(gain_db(1.0, 0.7, 1000.0, 0.01) > gain_db(1.0, 0.0, 1000.0, 0.01) + 6.0);
        let mut f = SallenKey::new(1.0, [2.087_344, 2.568_901]);
        f.tune(1000.0, 48_000.0, 1.0);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| [if n % 100 < 50 { 4.0 } else { -4.0 }; 2])
            .collect();
        f.process(&mut io, 1.0);
        let peak = io.iter().map(|x| x[0].abs()).fold(0.0, f32::max);
        assert!(peak.is_finite() && peak < 40.0, "{peak}");
    }
}
