//! Canonical per-sample one-pole parameter smoother built on `moose-dsp`.

use moose_dsp::smooth::OnePole;
use moose_dsp::time;

use crate::flush_f32;

/// Per-sample one-pole parameter smoother (zero zipper noise, RT-safe).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smoother {
    pole: OnePole<f32>,
    coeff: f32,
}

impl Smoother {
    /// Initializes a smoother at `initial` with time constant `tau_s` seconds.
    #[must_use]
    pub fn new(initial: f32, tau_s: f32, rate: f32) -> Self {
        let coeff = if tau_s <= 1e-6 || rate <= 1.0 {
            1.0
        } else {
            time::coeff_secs(tau_s, rate)
        };
        Self {
            pole: OnePole::new(initial),
            coeff,
        }
    }

    /// Advances one sample toward `target` and returns the smoothed value.
    #[inline]
    pub fn tick(&mut self, target: f32) -> f32 {
        let v = self.pole.step(target, self.coeff);
        self.pole.value = flush_f32(v);
        self.pole.value
    }

    /// Returns the current smoothed value.
    #[inline]
    #[must_use]
    pub fn get(self) -> f32 {
        self.pole.value
    }

    /// Snaps immediately to `value` (use on construction or reset).
    #[inline]
    pub fn snap(&mut self, value: f32) {
        self.pole.value = flush_f32(value);
    }

    /// Updates the time constant in seconds at `rate`.
    pub fn set_tau(&mut self, tau_s: f32, rate: f32) {
        self.coeff = if tau_s <= 1e-6 || rate <= 1.0 {
            1.0
        } else {
            time::coeff_secs(tau_s, rate)
        };
    }
}
