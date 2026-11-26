//! Building blocks the DSP families share (03-DSP.md).

mod glide;
mod halfband;
pub mod math;
mod section;

pub use glide::{Fader, Levels};
pub use halfband::Halfband;
pub use kontra_law::{Section, Sections, Stage};
pub use moose_dsp::biquad::Side;
use moose_dsp::biquad::{Coeffs, cookbook};
pub use section::{Biquad32, bilinear};

/// One stereo biquad section, transposed direct form II, f64 state so
/// low bands stay clean at high rates.
#[derive(Clone, Debug, PartialEq)]
pub struct Biquad {
    c: Coeffs<f64>,
    state: [[f64; 2]; 2],
}

impl From<Coeffs<f64>> for Biquad {
    fn from(c: Coeffs<f64>) -> Self {
        Self {
            c,
            state: [[0.0; 2]; 2],
        }
    }
}

impl Biquad {
    /// A section from `b0, b1, b2, a0, a1, a2`; `a0` must not be 0.
    #[must_use]
    pub fn new(c: [f64; 6]) -> Self {
        Coeffs::normalize(c).into()
    }

    /// Filters both channels of `io` in place.
    #[inline]
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let Coeffs { b0, b1, b2, a1, a2 } = self.c;
        let [mut s0l, mut s1l] = self.state[0];
        let [mut s0r, mut s1r] = self.state[1];
        for frame in io {
            let xl = f64::from(frame[0]);
            let xr = f64::from(frame[1]);
            let yl = b0 * xl + s0l;
            let yr = b0 * xr + s0r;
            s0l = b1 * xl - a1 * yl + s1l;
            s0r = b1 * xr - a1 * yr + s1r;
            s1l = b2 * xl - a2 * yl;
            s1r = b2 * xr - a2 * yr;
            *frame = [yl as f32, yr as f32];
        }
        self.state = [[s0l, s1l], [s0r, s1r]];
        flush(self.state.as_flattened_mut());
    }

    /// Filters a single stereo `f64` frame without flushing subnormals yet.
    /// Call [`Self::settle`] once at the end of the block.
    #[inline]
    pub fn tick_f64(&mut self, xl: f64, xr: f64) -> (f64, f64) {
        let Coeffs { b0, b1, b2, a1, a2 } = self.c;
        let [s0l, s1l] = self.state[0];
        let [s0r, s1r] = self.state[1];
        let yl = b0 * xl + s0l;
        let yr = b0 * xr + s0r;
        self.state[0] = [b1 * xl - a1 * yl + s1l, b2 * xl - a2 * yl];
        self.state[1] = [b1 * xr - a1 * yr + s1r, b2 * xr - a2 * yr];
        (yl, yr)
    }

    /// Zeroes subnormal state after a run of [`Self::tick_f64`] calls.
    #[inline]
    pub fn settle(&mut self) {
        flush(self.state.as_flattened_mut());
    }

    /// Whether the state has rung out (below −200 dB).
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.state.iter().flatten().all(|s| s.abs() < 1e-10)
    }

    /// Takes `from`'s coefficients and keeps this section's state, so a band
    /// that moves mid-note carries on from where it was.
    pub fn retune(&mut self, from: &Self) {
        self.c = from.c;
    }

    /// Multiplies the section's output by `gain`.
    pub fn scale(&mut self, gain: f64) {
        for b in [&mut self.c.b0, &mut self.c.b1, &mut self.c.b2] {
            *b *= gain;
        }
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 2]; 2];
    }
}

/// A shelf: `gain_db` on `side` of `hz`, its knee set by `slope` (the
/// cookbook's S, held to 0.01..=1 so the knee never peaks).
#[must_use]
pub fn shelf(hz: f32, gain_db: f32, slope: f32, side: Side, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let hz = f64::from(hz).max(1.0).min(0.49 * rate);
    let w = std::f64::consts::TAU * hz / rate;
    let a = 10f64.powf(f64::from(gain_db) / 40.0);
    let s = f64::from(slope).clamp(0.01, 1.0);
    let alpha = w.sin() / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
    cookbook::shelf(w.cos(), alpha, a, side).into()
}

/// The cookbook second-order high-pass at `hz`, resonance `q` (½ two
/// coincident poles, no peak).
#[must_use]
pub fn high_pass(hz: f32, q: f32, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let w = std::f64::consts::TAU * f64::from(hz).max(1.0).min(0.49 * rate) / rate;
    cookbook::high_pass(w.cos(), w.sin() / (2.0 * f64::from(q).max(1e-3))).into()
}

/// Zeroes state that has decayed below −400 dB, so a rung-out filter never
/// feeds subnormals to what follows (no FTZ on the audio thread).
pub fn flush(state: &mut [f64]) {
    for s in state {
        moose_dsp::flush(s, 1e-20);
    }
}
