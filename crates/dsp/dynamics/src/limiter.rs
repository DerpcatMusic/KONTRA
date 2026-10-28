//! A peak limiter: input gain, an instant-attack peak follower with a
//! release, and a hard clip at the ceiling for what the follower misses.

use crate::gain;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub input_db: f32,
    pub ceiling_db: f32,
    pub release_ms: f32,
}

pub struct Limiter {
    input: f32,
    ceiling: f32,
    /// Per frame, the factor the held peak falls by.
    fall: f32,
    peak: f32,
}

impl Limiter {
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let mut l = Self {
            input: 1.0,
            ceiling: 1.0,
            fall: 0.0,
            peak: 0.0,
        };
        l.set(s, rate);
        l
    }

    /// Changes the settings while it plays.
    pub fn set(&mut self, s: Settings, rate: u32) {
        self.input = gain(s.input_db);
        self.ceiling = gain(s.ceiling_db);
        // Halves in the release time.
        let frames = (s.release_ms * 0.001 * rate as f32).max(1.0);
        self.fall = (-std::f32::consts::LN_2 / frames).exp();
    }

    /// Limits `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let c = self.ceiling;
        for frame in io {
            let x = frame.map(|v| v * self.input);
            let level = x[0].abs().max(x[1].abs());
            self.peak = level.max(self.peak * self.fall);
            let g = if self.peak > c { c / self.peak } else { 1.0 };
            *frame = x.map(|v| (v * g).clamp(-c, c));
        }
    }
}
