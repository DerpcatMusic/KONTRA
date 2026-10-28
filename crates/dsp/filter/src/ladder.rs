//! A four-pole resonant ladder: four trapezoidal one-poles in series, the
//! last fed back through a soft clip, its output a weighted mix of the taps
//! (the clipped input and each pole). Its passband falls `1/(1 + 4.16 q)`
//! as resonance rises, and each selector's compensation curve lifts it back.
//!
//! The law is Kontakt 8's Ladder core FUN_140b00be2: feedback scale 4.16
//! (0x144fcfdd0), the tan series `((40.80263 w² + 10.335426) w² + π) w`
//! for the pole gain clamped at 8, the ±24 clamp then `v (1 − |v|/48)`
//! clip, tap weights 0x144fcebc0 and compensation knots/values
//! 0x144fcecb0/0x144fced70 (all twelve rows read from the binary).
//!
//! The feedback also runs through a one-pole at `80/R` Hz scaled by
//! `R² − 1`, `R = 2^(1196.6814/60 − 20)` (prepare FUN_140b06180): it takes a
//! little feedback out of the lows and damps the loop near self-oscillation.
//!
//! ponytail: the native 32/4-frame control clock is left out; the engine's
//! spans move the cutoff instead.

use kontra_dsp_core::flush;

const FEEDBACK: f64 = 4.16;

/// The correction's fixed gain `R`, its pole at `80/R` Hz.
fn correction_r() -> f64 {
    2f64.powf(1196.681_396_5 / 60.0 - 20.0)
}

/// Per selector 1..=12: the weights of the input and the four pole taps.
#[rustfmt::skip]
const WEIGHTS: [[f64; 5]; 12] = [
    [0., 1., 0., 0., 0.], [0., 0., 1., 0., 0.], [0., 0., 0., 1., 0.], [0., 0., 0., 0., 1.],
    [1., -1., 0., 0., 0.], [1., -2., 1., 0., 0.], [1., -3., 3., -1., 0.], [1., -4., 6., -4., 1.],
    [0., -2., 2., 0., 0.], [0., 0., 6., -12., 6.], [1., 0., 0., 0., 0.], [1., -4., 12., -16., 8.],
];

/// Per selector, the compensation curve over `q²`: its knots, and the half
/// gains it eases between.
#[rustfmt::skip]
const COMPENSATION: [([f64; 4], [f64; 4]); 12] = [
    ([0., 0.25, 0.75, 1.], [0.49, 0.83, 0.83, 0.4]),
    ([0., 0.26, 0.73, 1.], [0.5, 0.97, 0.97, 0.5]),
    ([0., 0.34, 0.79, 1.], [0.5, 1., 1., 0.52]),
    ([0., 0.34, 0.83, 1.], [0.51, 1., 1., 0.57]),
    ([0., 0.46, 0.79, 1.], [0.5, 0.5, 0.5, 0.2]),
    ([0., 0.46, 0.75, 1.], [0.5, 0.5, 0.5, 0.2]),
    ([0., 0.46, 0.75, 1.], [0.5, 0.5, 0.5, 0.25]),
    ([0., 0.46, 0.76, 1.], [0.5, 0.5, 0.5, 0.28]),
    ([0., 0.51, 0.63, 1.], [0.5, 0.5, 0.5, 0.25]),
    ([0., 0.56, 0.63, 1.], [0.5, 0.5, 0.5, 0.19]),
    ([0., 0.54, 0.80, 1.], [0.5, 0.5, 0.5, 0.2]),
    ([0., 0.25, 0.60, 1.], [0.5, 0.5, 0.49, 0.19]),
];

/// The output gain of selector `row` (0-based) at feedback `q`.
fn compensation(q: f64, row: usize) -> f64 {
    let Some(&(knots, values)) = COMPENSATION.get(row) else {
        return 1.0;
    };
    let p = q * q;
    let i = if p < knots[1] {
        0
    } else if p < knots[2] {
        1
    } else {
        2
    };
    let t = (p - knots[i]) / (knots[i + 1] - knots[i]);
    // Between a smoothstep and an ease-out, by how far the knot sits from 0.
    let s = (1.0 - t) * (1.0 - t);
    let smooth = 1.0 - (2.0 - s) * s;
    let ease = smooth + values[i].abs() * ((2.0 - t) * t - smooth);
    2.0 * (values[i] + (values[i + 1] - values[i]) * ease)
}

/// The pole gain for a cutoff at `w` cycles per sample.
fn pole(w: f64) -> f64 {
    let w2 = w * w;
    (((40.802_63 * w2 + 10.335_426) * w2 + std::f64::consts::PI) * w).min(8.0)
}

fn clip(v: f64) -> f64 {
    let v = v.clamp(-24.0, 24.0);
    v * (1.0 - v.abs() / 48.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ladder {
    row: usize,
    g: f64,
    q: f64,
    /// The correction one-pole's gain.
    gc: f64,
    /// Per channel, the four poles' states, then the correction's.
    state: [[f64; 5]; 2],
}

impl Ladder {
    /// A ladder with output selector `taps` (1..=12, held in range).
    #[must_use]
    pub fn new(taps: u8) -> Self {
        Self {
            row: usize::from(taps.clamp(1, 12) - 1),
            g: 0.0,
            q: 0.0,
            gc: 0.0,
            state: [[0.0; 5]; 2],
        }
    }

    /// Moves the cutoff to `hz` at `rate` and the feedback to `q` (0..=1).
    pub fn tune(&mut self, hz: f64, rate: f64, q: f64) {
        self.g = pole(hz / rate.max(1.0));
        self.q = q.clamp(0.0, 1.0);
        self.gc = pole(80.0 / correction_r() / rate.max(1.0));
    }

    /// Filters `io` in place after the input `gain`.
    pub fn process(&mut self, io: &mut [[f32; 2]], gain: f64) {
        let (g, k) = (self.g, self.q * FEEDBACK);
        let a = g / (1.0 + g);
        let weights = WEIGHTS.get(self.row).copied().unwrap_or_default();
        let out = compensation(self.q, self.row);
        let (gc, r) = (self.gc, correction_r());
        let a4 = a * a * a * a;
        for frame in io {
            for (x, s) in frame.iter_mut().zip(&mut self.state) {
                let (poles, held_c) = s.split_at_mut(4);
                // Each pole is `a x + s/(1 + g)`; the four in series give
                // the last as `a⁴ u + held`, solved with the feedback.
                let held = poles.iter().fold(0.0, |y, si| a * y + si / (1.0 + g));
                let input = gain * f64::from(*x);
                let feedback = k * (a4 * input + held) / (1.0 + k * a4);
                let mut fb = feedback;
                if let Some(c) = held_c.first_mut() {
                    let low = (feedback - *c * (gc - 1.0)) / (1.0 + gc);
                    fb += (*c + low) * gc * (r * r - 1.0);
                    *c = low;
                }
                let mut v = clip(input - fb);
                let mut mix = weights[0] * v;
                for (si, w) in poles.iter_mut().zip(&weights[1..]) {
                    let step = a * (v - *si);
                    v = step + *si;
                    *si = v + step;
                    mix += w * v;
                }
                *x = (mix * out) as f32;
            }
        }
        flush(self.state.as_flattened_mut());
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.state.iter().flatten().all(|s| s.abs() < 1e-10)
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 5]; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kontakt's Ladder (cutoff knob 0.4997, about 717 Hz, gain 0) in dB
    /// against bypass at 200, 500, 1000, 2000 and 5000 Hz, per selector and
    /// resonance knob (docs/re captures, v2 ladder_kernel.rs).
    const MEASURED: [(u8, f64, [f64; 5]); 6] = [
        (1, 0.5039, [-6.2, 3.6, 2.2, -5.2, -12.9]),
        (2, 0.5039, [-5.2, 3.2, -1.1, -13.3, -28.8]),
        (3, 0.5039, [-5.3, 1.7, -5.5, -22.5, -45.8]),
        (1, 0.8031, [-13.4, -4.3, -2.5, -10.6, -18.3]),
        (2, 0.8031, [-12.0, -4.5, -5.6, -18.5, -33.9]),
        (3, 0.8031, [-11.7, -5.5, -9.6, -27.2, -50.5]),
    ];

    fn gain_db(taps: u8, r: f64, hz: f64) -> f64 {
        let mut f = Ladder::new(taps);
        let cutoff = 440.0 * 2f64.powf((115.0 * 0.4997 + 20.0 - 69.0) / 12.0);
        f.tune(cutoff, 48_000.0, 1.0 - (1.0 - r) * (1.0 - r));
        let sine: Vec<[f32; 2]> = (0..24_000)
            .map(|n| {
                [(0.01 * (std::f64::consts::TAU * hz * f64::from(n) / 48_000.0).sin()) as f32; 2]
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
    fn the_taps_match_kontakts_measured_responses() {
        for (taps, r, expected) in MEASURED {
            for (hz, want) in [200.0, 500.0, 1000.0, 2000.0, 5000.0]
                .into_iter()
                .zip(expected)
            {
                let got = gain_db(taps, r, hz);
                assert!(
                    (got - want).abs() < 0.7,
                    "LP{taps} r={r} {hz} Hz: {got:.2} vs {want}"
                );
            }
        }
    }

    #[test]
    fn full_feedback_rings_but_stays_bounded() {
        let mut f = Ladder::new(4);
        f.tune(1000.0, 48_000.0, 1.0);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| [if n == 0 { 1.0 } else { 0.0 }; 2])
            .collect();
        f.process(&mut io, 4.0);
        let tail = io[24_000..].iter().map(|x| x[0].abs()).fold(0.0, f32::max);
        assert!(tail.is_finite() && tail < 30.0, "{tail}");
    }
}
