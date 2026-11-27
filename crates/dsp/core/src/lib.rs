//! Building blocks the DSP families share (03-DSP.md).

pub mod contract;
pub mod delay;
pub mod eq;
mod glide;
mod halfband;
pub mod math;
mod section;
pub mod smooth;

pub use contract::DspCore;
pub use delay::{DelayLine, FixedRing, hermite};
pub use glide::{Fader, Levels};
pub use halfband::Halfband;
pub use kontra_law::{Section, Sections, Stage};
pub use moose_dsp::biquad::Side;
use moose_dsp::biquad::{Coeffs, cookbook};
pub use section::{Biquad32, bilinear};
pub use smooth::Smoother;

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

/// The cookbook peaking (bell) biquad at `hz`, resonance `q`, and `gain_db`.
#[must_use]
pub fn peak(hz: f32, q: f32, gain_db: f32, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let f = f64::from(hz).max(1.0).min(0.49 * rate);
    let w = std::f64::consts::TAU * f / rate;
    let a = 10f64.powf(f64::from(gain_db) / 40.0);
    let alpha = w.sin() / (2.0 * f64::from(q).max(1e-3));
    cookbook::bell(w.cos(), alpha, a).into()
}

/// The cookbook second-order low-pass at `hz`, resonance `q`.
#[must_use]
pub fn low_pass(hz: f32, q: f32, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let f = f64::from(hz).max(1.0).min(0.49 * rate);
    let w = std::f64::consts::TAU * f / rate;
    cookbook::low_pass(w.cos(), w.sin() / (2.0 * f64::from(q).max(1e-3))).into()
}

/// The cookbook second-order high-pass at `hz`, resonance `q` (½ two
/// coincident poles, no peak).
#[must_use]
pub fn high_pass(hz: f32, q: f32, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let w = std::f64::consts::TAU * f64::from(hz).max(1.0).min(0.49 * rate) / rate;
    cookbook::high_pass(w.cos(), w.sin() / (2.0 * f64::from(q).max(1e-3))).into()
}

/// Bilinear first-order high-pass at `hz`, pre-warped at the corner.
#[must_use]
pub fn first_order_hp(hz: f32, rate: u32) -> Biquad {
    let fs = f64::from(rate.max(1));
    let fc = f64::from(hz).clamp(1.0_f64.min(0.49 * fs), 0.49 * fs);
    let k = (std::f64::consts::PI * fc / fs).tan();
    Biquad::new([1.0, -1.0, 0.0, 1.0 + k, k - 1.0, 0.0])
}

/// Bilinear second-order high-pass at `hz` with resonance `q`.
#[must_use]
pub fn second_order_hp(hz: f32, q: f64, rate: u32) -> Biquad {
    let fs = f64::from(rate.max(1));
    let fc = f64::from(hz).clamp(1.0_f64.min(0.49 * fs), 0.49 * fs);
    let k = (std::f64::consts::PI * fc / fs).tan();
    let k2 = k * k;
    let q = q.max(1e-3);
    Biquad::new([
        1.0,
        -2.0,
        1.0,
        1.0 + k / q + k2,
        2.0 * (k2 - 1.0),
        1.0 - k / q + k2,
    ])
}

/// Analog-matched second-order low-pass (Vicanek magnitude match at DC,
/// cutoff, and Nyquist) avoiding bilinear Nyquist cramping.
#[must_use]
pub fn vicanek_lp(hz: f32, q: f64, rate: u32) -> Biquad {
    let fs = f64::from(rate.max(1));
    let fc = f64::from(hz).clamp(1.0_f64.min(0.49 * fs), 0.49 * fs);
    let q = q.max(1e-3);
    let w0 = std::f64::consts::TAU * fc / fs;
    let inv_2q = 0.5 / q;
    let r = (-inv_2q * w0).exp();
    let a1 = if inv_2q <= 1.0 {
        -2.0 * r * ((1.0 - inv_2q * inv_2q).sqrt() * w0).cos()
    } else {
        -2.0 * r * ((inv_2q * inv_2q - 1.0).sqrt() * w0).cosh()
    };
    let a2 = r * r;

    let big_a0 = (1.0 + a1 + a2).powi(2);
    let big_a1 = (1.0 - a1 + a2).powi(2);
    let big_a2 = -4.0 * a2;

    let phi1 = (0.5 * w0).sin().powi(2);
    let phi0 = 1.0 - phi1;
    let phi2 = 4.0 * phi0 * phi1;

    let nyq_ratio = std::f64::consts::PI / w0;
    let nyq_mag2 = 1.0 / ((1.0 - nyq_ratio * nyq_ratio).powi(2) + (nyq_ratio / q).powi(2));
    let cut_mag2 = q * q;

    let big_b0 = big_a0;
    let big_b1 = big_a1 * nyq_mag2;
    let d_w0 = big_a0 * phi0 + big_a1 * phi1 + big_a2 * phi2;
    let big_b2 = if phi2 > 1e-12 {
        (cut_mag2 * d_w0 - big_b0 * phi0 - big_b1 * phi1) / phi2
    } else {
        0.0
    };

    let sqrt_b0 = big_b0.max(0.0).sqrt();
    let sqrt_b1 = big_b1.max(0.0).sqrt();
    let s = 0.5 * (sqrt_b0 + sqrt_b1);
    let b1 = 0.5 * (sqrt_b0 - sqrt_b1);
    let disc = (s * s + big_b2).max(0.0).sqrt();
    let b0 = 0.5 * (s + disc);
    let b2 = s - b0;
    Biquad::new([b0, b1, b2, 1.0, a1, a2])
}

/// Zeroes state that has decayed below −400 dB, so a rung-out filter never
/// feeds subnormals to what follows (no FTZ on the audio thread).
pub fn flush(state: &mut [f64]) {
    for s in state {
        moose_dsp::flush(s, 1e-20);
    }
}

/// Zeroes subnormal or non-finite `f32` values (`|x| < 1e-20` or `!is_finite()`).
#[inline]
#[must_use]
pub fn flush_f32(x: f32) -> f32 {
    if !x.is_finite() || x.abs() < 1e-20 {
        0.0
    } else {
        x
    }
}

/// Zeroes subnormal or non-finite values in an `f32` slice.
#[inline]
pub fn flush_f32_slice(state: &mut [f32]) {
    for s in state {
        *s = flush_f32(*s);
    }
}
