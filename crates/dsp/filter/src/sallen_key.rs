//! A two-pole Sallen-Key filter with its feedback through a saturating
//! curve: two trapezoidal integrators `y1`, `y2` (outputs `v = S + y/2`)
//! solved together by Newton each half sample.
//!
//! `y1 = T(a1 − nl − v1)`, `y2 = T(nl − a2 − v2 + v1)`, the feedback
//! `nl = m + a P(b m)` on `m = K(a2 + v2)`, `P(u) = u(u² + 27)/(9u² + 27)`;
//! `a1 = g·morph·x` and `a2 = g(morph − 1)x` the low and high inputs, the
//! output `(a2 + v2)/g` (morph 1 a low-pass, 0 a high-pass), `T = πf/rate`
//! and `K = 0.549 q`. The law is UVIWorkstation's MS20 (process
//! [ref], step [ref], setup [ref]): two substeps on the
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
        if io.is_empty() {
            return;
        }
        if self.state.iter().flatten().all(|&s| s == 0.0)
            && io.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
        {
            return;
        }
        let gain = gain.max(1e-6);
        let (t, k, [a, b]) = (self.t, self.k, self.curve);
        let m_lo = self.morph * gain;
        let m_hi = (self.morph - 1.0) * gain;
        let inv_2gain = 0.5 / gain;
        let c = 1.0 + 0.5 * t;
        if k == 0.0 {
            let inv_c = 1.0 / c;
            let tc = t * inv_c;
            for frame in io {
                for (sample, s) in frame.iter_mut().zip(&mut self.state) {
                    let [y1, y2, s1, s2, p1, p2] = s;
                    let x = f64::from(*sample);
                    let (in1, in2) = (m_lo * x, m_hi * x);
                    // Substep 1: midpoint between previous and current input.
                    let (u1, u2) = (0.5 * (*p1 + in1), 0.5 * (*p2 + in2));
                    let ny1 = tc * (u1 - *s1);
                    let v1 = *s1 + 0.5 * ny1;
                    let ny2 = tc * (v1 - u2 - *s2);
                    let mut out = u2 + *s2 + 0.5 * ny2;
                    *s1 += ny1;
                    *s2 += ny2;
                    // Substep 2: current input.
                    let ny1 = tc * (in1 - *s1);
                    let v1 = *s1 + 0.5 * ny1;
                    let ny2 = tc * (v1 - in2 - *s2);
                    out += in2 + *s2 + 0.5 * ny2;
                    *s1 += ny1;
                    *s2 += ny2;
                    *y1 = ny1;
                    *y2 = ny2;
                    *p1 = in1;
                    *p2 = in2;
                    *sample = (out * inv_2gain) as f32;
                }
            }
            flush(self.state.as_flattened_mut());
            return;
        }
        let half_k = 0.5 * k;
        let ab = a * b;
        let c2 = c * c;
        let neg_ht = -0.5 * t;
        for frame in io {
            for (sample, s) in frame.iter_mut().zip(&mut self.state) {
                let [y1, y2, s1, s2, p1, p2] = s;
                let x = f64::from(*sample);
                let (in1, in2) = (m_lo * x, m_hi * x);
                let substeps = [(0.5 * (*p1 + in1), 0.5 * (*p2 + in2)), (in1, in2)];
                let mut out = 0.0;
                for (u1, u2) in substeps {
                    let (mut ly1, mut ly2) = (*y1, *y2);
                    let (ls1, ls2) = (*s1, *s2);
                    for _ in 0..20 {
                        let (v1, v2) = (0.5 * ly1 + ls1, 0.5 * ly2 + ls2);
                        let m = k * (u2 + v2);
                        let (sh, sl) = shape_and_slope(b * m);
                        let nl = m + a * sh;
                        let dnl = half_k * (1.0 + ab * sl);
                        let f1 = ly1 - t * (u1 - nl - v1);
                        let f2 = ly2 - t * (nl - u2 - v2 + v1);
                        let j12 = t * dnl;
                        let j22 = c - j12;
                        // det = c * (c - j12) - j12 * (-0.5 * t) = c^2 - j12
                        let inv_det = 1.0 / (c2 - j12);
                        let d1 = (f1 * j22 - f2 * j12) * inv_det;
                        let d2 = (f2 * c - f1 * neg_ht) * inv_det;
                        ly1 -= d1;
                        ly2 -= d2;
                        if d1.abs().max(d2.abs()) < 1e-6 {
                            break;
                        }
                    }
                    *y1 = ly1;
                    *y2 = ly2;
                    out += u2 + ls2 + 0.5 * ly2;
                    *s1 = ls1 + ly1;
                    *s2 = ls2 + ly2;
                }
                *p1 = in1;
                *p2 = in2;
                *sample = (out * inv_2gain) as f32;
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

/// `P(u) = u(u^2 + 27)/(9u^2 + 27)` and its derivative `((u^2 - 9)/(3u^2 + 9))^2`
/// computed from a single shared reciprocal.
#[inline(always)]
fn shape_and_slope(u: f64) -> (f64, f64) {
    let u2 = u * u;
    let inv = 1.0 / (3.0 * u2 + 9.0);
    let shape = (1.0 / 3.0) * u * (u2 + 27.0) * inv;
    let t = (u2 - 9.0) * inv;
    (shape, t * t)
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
