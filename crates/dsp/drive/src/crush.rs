//! The reducer: a sample hold, a quantizer that truncates toward zero, then
//! coloured LCG noise (ported from v2 sampler-core dsp/color/lofi.rs; the
//! format translators carry the provenance and the knob laws); or, given a
//! [`Converter`], that modelled converter pair in place of hold and quantizer.

use crate::converter::{Converter, Unit};

/// What it reduces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reduction {
    /// Frames between held samples (1 holds nothing; fractions alternate).
    pub hold: f32,
    /// Quantizer resolution, `2^bits` steps per unit (none: off).
    pub bits: Option<f32>,
    /// Noise level, linear.
    pub noise: f32,
    /// The noise's one-pole smoothing, 0..=1 (1 white).
    pub noise_tone: f32,
    /// Replaces the hold and quantizer.
    pub converter: Option<Converter>,
    /// The rate the converter runs at.
    pub rate: u32,
}

/// A stereo reducer with its hold, quantizer and noise state.
#[derive(Clone, Debug, PartialEq)]
pub struct Crush {
    hold: f32,
    /// `(2^bits, 2^−bits)`.
    step: Option<(f32, f32)>,
    noise: f32,
    tone: f32,
    phase: f32,
    held: [f32; 2],
    seed: u32,
    /// The seed [`Crush::reset`] restarts from.
    origin: u32,
    colour: [f32; 2],
    converter: Option<Unit>,
}

impl Crush {
    /// `seed` starts its noise; voices given different seeds sum as noise.
    #[must_use]
    pub fn new(r: Reduction, seed: u32) -> Self {
        let mut c = Self {
            hold: 1.0,
            step: None,
            noise: 0.0,
            tone: 1.0,
            phase: 0.0,
            held: [0.0; 2],
            seed: seed.max(1),
            origin: seed.max(1),
            colour: [0.0; 2],
            converter: None,
        };
        c.set(r);
        c
    }

    /// Moves the settings; the hold phase, converter and noise carry on.
    pub fn set(&mut self, r: Reduction) {
        match (&mut self.converter, r.converter) {
            (Some(unit), Some(c)) => unit.set(c, r.rate),
            (unit, c) => *unit = c.map(|c| Unit::new(c, r.rate)),
        }
        self.hold = if r.hold.is_nan() {
            1.0
        } else {
            r.hold.max(1.0)
        };
        self.step = r.bits.filter(|b| b.is_finite()).map(|b| {
            let q = b.clamp(1.0, 24.0).exp2();
            (q, 1.0 / q)
        });
        self.noise = if r.noise.is_nan() {
            0.0
        } else {
            r.noise.max(0.0)
        };
        self.tone = if r.noise_tone.is_nan() {
            1.0
        } else {
            r.noise_tone.clamp(0.0, 1.0)
        };
    }

    /// Reduces `io` (frames of `[left, right]`) in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if let Some(unit) = &mut self.converter {
            unit.process(io);
        } else {
            self.hold_and_quantize(io);
        }
        self.add_noise(io);
    }

    fn hold_and_quantize(&mut self, io: &mut [[f32; 2]]) {
        for frame in io.iter_mut() {
            self.phase += 1.0;
            if self.phase >= self.hold {
                self.phase -= self.hold;
                self.held = *frame;
            }
            *frame = self.held;
        }
        if let Some((q, inverse)) = self.step {
            for x in io.iter_mut().flatten() {
                *x = ((*x * q) as i32) as f32 * inverse;
            }
        }
    }

    fn add_noise(&mut self, io: &mut [[f32; 2]]) {
        if self.noise > 0.0 {
            for frame in io.iter_mut() {
                for (x, h) in frame.iter_mut().zip(&mut self.colour) {
                    self.seed = self
                        .seed
                        .wrapping_mul(0x0bb3_8435)
                        .wrapping_add(0x3619_636b);
                    // −1..1 from the mantissa of a float in 2..4.
                    let u = f32::from_bits((self.seed & 0x007f_ffff) | 0x4000_0000) - 3.0;
                    *h += self.tone * (u - *h);
                    *x += *h * self.noise;
                }
            }
        }
    }

    /// Restarts its noise from `seed`, also on later resets.
    pub fn reseed(&mut self, seed: u32) {
        (self.seed, self.origin) = (seed.max(1), seed.max(1));
    }

    /// Clears the hold and noise state and restarts the noise from its seed;
    /// the settings stay.
    pub fn reset(&mut self) {
        (self.phase, self.held, self.colour) = (0.0, [0.0; 2], [0.0; 2]);
        if let Some(unit) = &mut self.converter {
            unit.reset();
        }
        self.seed = self.origin;
    }
}
