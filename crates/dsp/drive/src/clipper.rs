//! A diode clipper circuit, solved per frame by backward Euler: the input
//! soft-limited by `limit·tanh(drive·x/limit)`, a coupling capacitor (a
//! high-pass at `coupling_hz`) into an RC low-pass at `tone_hz` whose
//! capacitor an antiparallel diode pair shunts, then a one-pole DC blocker.
//! Each frame solves, for the capacitor voltage `v` (w = 2π·hz/rate):
//!
//! `(1 + w_t + w_c)·v + k(1 + w_c)(e^(b·v) − e^(−a·b·v)) = w_t(x − s) + (1 + w_c)·v₋₁`
//!
//! with `k = saturation·w_t`, `b = slope`, `a = asymmetry`, by Newton steps
//! held within ±0.5 and `v` within ±1.5, at most ten, until a step is under
//! 0.001; the coupling state follows `s ← (s + w_c(x − v))/(1 + w_c)`. It is
//! its own class because the circuit has memory around the curve, which
//! [`crate::Shaper`] cannot. Runs at the base rate in `f64`.

use std::f64::consts::TAU;

/// The circuit in physical terms (as [`kontra_ir::Clipper`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circuit {
    pub drive: f32,
    pub limit: f32,
    pub tone_hz: f32,
    pub coupling_hz: f32,
    pub saturation: f32,
    pub slope: f32,
    pub asymmetry: f32,
    pub dc_hz: f32,
}

/// A stereo diode clipper and its state.
#[derive(Clone, Debug, PartialEq)]
pub struct Clipper {
    drive: f64,
    limit: f64,
    tone: f64,
    coupling: f64,
    k: f64,
    slope: f64,
    asymmetry: f64,
    /// The DC blocker's pole.
    pole: f64,
    /// Per channel: the coupling state, `v`, and the DC blocker's low-pass.
    state: [[f64; 3]; 2],
}

impl Clipper {
    #[must_use]
    pub fn new(c: Circuit, rate: u32) -> Self {
        let w = |hz: f32| TAU * f64::from(hz.max(0.0)) / f64::from(rate.max(1));
        Self {
            drive: f64::from(c.drive),
            limit: f64::from(c.limit).max(f64::MIN_POSITIVE),
            tone: w(c.tone_hz),
            coupling: w(c.coupling_hz),
            k: f64::from(c.saturation) * w(c.tone_hz),
            slope: f64::from(c.slope),
            asymmetry: f64::from(c.asymmetry),
            pole: (-w(c.dc_hz)).exp(),
            state: [[0.0; 3]; 2],
        }
    }

    /// Clips `io` (frames of `[left, right]`) in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let (wt, wc, b) = (self.tone, self.coupling, self.slope);
        let a = self.asymmetry * b;
        let total = 1.0 + wt + wc;
        let k = self.k * (1.0 + wc);
        for frame in io.iter_mut() {
            for (x, [s, v, dc]) in frame.iter_mut().zip(self.state.iter_mut()) {
                let input = self.limit * (f64::from(*x) * self.drive / self.limit).tanh();
                let target = wt * (input - *s) + (1.0 + wc) * *v;
                let mut coupled = *s;
                for _ in 0..10 {
                    let (up, down) = ((b * *v).exp(), (-a * *v).exp());
                    let slope = total + k * (b * up + a * down);
                    if slope.abs() < 1e-10 {
                        break;
                    }
                    let step = (-(total * *v + k * (up - down) - target) / slope).clamp(-0.5, 0.5);
                    *v = (*v + step).clamp(-1.5, 1.5);
                    coupled = (*s + wc * (input - *v)) / (1.0 + wc);
                    if step.abs() < 0.001 {
                        break;
                    }
                }
                *s = coupled;
                *dc += (*v - *dc) * (1.0 - self.pole);
                *x = (*v - *dc) as f32;
            }
        }
        for c in &mut self.state {
            kontra_dsp_core::flush(c);
        }
    }

    /// Clears the circuit's state.
    pub fn reset(&mut self) {
        self.state = [[0.0; 3]; 2];
    }
}
