//! Building blocks the DSP families share (03-DSP.md).

mod glide;
mod halfband;

pub use glide::{Fader, Levels};
pub use halfband::Halfband;

/// One stereo biquad section, transposed direct form II, f64 state so
/// low bands stay clean at high rates.
#[derive(Clone, Debug, PartialEq)]
pub struct Biquad {
    /// b0, b1, b2, a1, a2 with a0 = 1.
    c: [f64; 5],
    state: [[f64; 2]; 2],
}

impl Biquad {
    /// A section from `b0, b1, b2, a0, a1, a2`; `a0` must not be 0.
    #[must_use]
    pub fn new([b0, b1, b2, a0, a1, a2]: [f64; 6]) -> Self {
        Self {
            c: [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0],
            state: [[0.0; 2]; 2],
        }
    }

    /// Filters both channels of `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let [b0, b1, b2, a1, a2] = self.c;
        for frame in io {
            for (x, s) in frame.iter_mut().zip(&mut self.state) {
                let input = f64::from(*x);
                let y = b0 * input + s[0];
                s[0] = b1 * input - a1 * y + s[1];
                s[1] = b2 * input - a2 * y;
                *x = y as f32;
            }
        }
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
        for b in self.c.iter_mut().take(3) {
            *b *= gain;
        }
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 2]; 2];
    }
}

/// Which side of its corner a shelf moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Low,
    High,
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
    let (cos, root) = (w.cos(), 2.0 * a.sqrt() * alpha);
    // The high shelf is the low one with cos negated and b1, a1 flipped.
    let (c, sign) = if side == Side::High {
        (-cos, -1.0)
    } else {
        (cos, 1.0)
    };
    Biquad::new([
        a * ((a + 1.0) - (a - 1.0) * c + root),
        sign * 2.0 * a * ((a - 1.0) - (a + 1.0) * c),
        a * ((a + 1.0) - (a - 1.0) * c - root),
        (a + 1.0) + (a - 1.0) * c + root,
        sign * -2.0 * ((a - 1.0) + (a + 1.0) * c),
        (a + 1.0) + (a - 1.0) * c - root,
    ])
}

/// The cookbook second-order high-pass at `hz`, resonance `q` (½ two
/// coincident poles, no peak).
#[must_use]
pub fn high_pass(hz: f32, q: f32, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let w = std::f64::consts::TAU * f64::from(hz).max(1.0).min(0.49 * rate) / rate;
    let (cos, alpha) = (w.cos(), w.sin() / (2.0 * f64::from(q).max(1e-3)));
    Biquad::new([
        (1.0 + cos) / 2.0,
        -(1.0 + cos),
        (1.0 + cos) / 2.0,
        1.0 + alpha,
        -2.0 * cos,
        1.0 - alpha,
    ])
}

/// Zeroes state that has decayed below −400 dB, so a rung-out filter never
/// feeds subnormals to what follows (no FTZ on the audio thread).
pub fn flush(state: &mut [f64]) {
    for s in state {
        if s.abs() < 1e-20 {
            *s = 0.0;
        }
    }
}
