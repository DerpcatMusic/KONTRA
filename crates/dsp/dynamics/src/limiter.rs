//! A peak limiter that works on the dB over its ceiling:
//!
//! 1. the louder channel after input gain, in dB over the ceiling (soft
//!    knee optional), never below 0;
//! 2. a held peak that rises at once and recovers at most 6 dB per release
//!    time (the slew limit);
//! 3. an optional smoother: rising to two thirds in the attack time, falling
//!    to a third in the smoothing time, blended with a fall 100× slower;
//! 4. the largest reduction over the lookahead window, applied to the audio
//!    delayed by that window, so the gain falls before a peak arrives;
//! 5. an optional hard clip at the ceiling for what is left.
//!
//! With no smoothing, lookahead or knee it is a plain peak follower whose
//! held peak halves in the release time.

use std::collections::VecDeque;

use crate::gain;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub input_db: f32,
    pub ceiling_db: f32,
    /// Time for the held peak to halve (6 dB of recovery); 0 = no limit.
    pub release_ms: f32,
    /// Width of the soft knee around the ceiling, dB.
    pub knee_db: f32,
    /// Time a rising reduction takes to cover two thirds of the step.
    pub attack_ms: f32,
    /// Time a falling reduction takes to fall to a third.
    pub smooth_ms: f32,
    /// Share, 0..=1, of a fall 100× slower than `smooth_ms`.
    pub slow: f32,
    /// How far ahead the gain looks; the audio is delayed by this much.
    pub lookahead_ms: f32,
    /// Clip the output at the ceiling.
    pub clip: bool,
}

pub struct Limiter {
    input: f32,
    ceiling_db: f32,
    ceiling: f32,
    knee: f32,
    clip: bool,
    /// Largest fall of the held peak per frame, dB.
    step: f32,
    /// One-pole coefficients: rising, falling, falling slowly.
    attack: f32,
    fast: f32,
    slow: f32,
    blend: f32,
    held: f32,
    smooth: [f32; 3],
    /// The audio waiting for its gain, and the window's falling maxima.
    delay: Vec<[f32; 2]>,
    maxima: VecDeque<(usize, f32)>,
    frame: usize,
}

impl Limiter {
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let mut l = Self {
            input: 1.0,
            ceiling_db: 0.0,
            ceiling: 1.0,
            knee: 0.0,
            clip: true,
            step: f32::INFINITY,
            attack: 0.0,
            fast: 0.0,
            slow: 0.0,
            blend: 0.0,
            held: 0.0,
            smooth: [0.0; 3],
            delay: Vec::new(),
            maxima: VecDeque::with_capacity(2),
            frame: 0,
        };
        l.set(s, rate);
        l
    }

    /// Changes the settings while it plays. Only a new lookahead allocates.
    pub fn set(&mut self, s: Settings, rate: u32) {
        let frames = |ms: f32| ms.max(0.0) * 0.001 * rate as f32;
        // Covers all but a third of a step in `n` frames; 0 is at once.
        let third = |n: f32| {
            if n > 0.0 {
                (-3f32.ln() / n).exp()
            } else {
                0.0
            }
        };
        self.input = gain(s.input_db);
        // A NaN ceiling would make `clamp` panic: full scale instead.
        self.ceiling_db = if s.ceiling_db.is_nan() {
            0.0
        } else {
            s.ceiling_db
        };
        self.ceiling = gain(self.ceiling_db);
        self.knee = s.knee_db.max(0.0);
        self.clip = s.clip;
        let release = frames(s.release_ms);
        self.step = if release > 0.0 {
            20.0 * std::f32::consts::LOG10_2 / release.max(1.0)
        } else {
            f32::INFINITY
        };
        self.attack = third(frames(s.attack_ms));
        self.fast = third(frames(s.smooth_ms));
        self.slow = third(100.0 * frames(s.smooth_ms));
        self.blend = s.slow.clamp(0.0, 1.0);
        // Even, as the UVI Maximizer rounds it.
        let window = 2 * (frames(s.lookahead_ms) * 0.5) as usize;
        if window != self.delay.len() {
            self.delay = vec![[0.0; 2]; window];
            self.maxima = VecDeque::with_capacity(window + 2);
            self.frame = 0;
        }
    }

    /// dB over the ceiling of a level, through the knee.
    fn over(&self, level: f32) -> f32 {
        let x = 20.0 * level.max(1e-10).log10() - self.ceiling_db;
        let k = self.knee;
        if x <= -k {
            0.0
        } else if x < k {
            (x + k) * (x + k) * 0.25 / k
        } else {
            x
        }
    }

    /// The reduction in dB after the held peak and the smoother.
    fn reduction(&mut self, over: f32) -> f32 {
        self.held = over.max(self.held - self.step);
        let [fast, slow, out] = &mut self.smooth;
        let x = self.held;
        let (cf, cs) = if x >= *out {
            (self.attack, self.attack)
        } else {
            (self.fast, self.slow)
        };
        *fast = cf * *fast + (1.0 - cf) * x;
        *slow = cs * *slow + (1.0 - cs) * x;
        // Released to silence: no subnormal tail.
        for v in [&mut *fast, &mut *slow] {
            if *v < 1e-6 {
                *v = 0.0;
            }
        }
        *out = *fast + (*slow - *fast) * self.blend;
        *out
    }

    /// Limits `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let c = self.ceiling;
        let window = self.delay.len();
        for frame in io {
            let x = frame.map(|v| v * self.input);
            let r = self.reduction(self.over(x[0].abs().max(x[1].abs())));
            // The largest reduction over this frame and the `window` before.
            while self.maxima.back().is_some_and(|&(_, v)| v <= r) {
                self.maxima.pop_back();
            }
            self.maxima.push_back((self.frame, r));
            if self.maxima.front().is_some_and(|&(t, _)| t + window < self.frame) {
                self.maxima.pop_front();
            }
            let r = self.maxima.front().map_or(0.0, |&(_, v)| v);
            let x = match self.delay.get_mut(self.frame % window.max(1)) {
                Some(slot) => std::mem::replace(slot, x),
                None => x,
            };
            self.frame += 1;
            let g = if r > 0.0 { gain(-r) } else { 1.0 };
            *frame = x.map(|v| v * g);
            if self.clip {
                *frame = frame.map(|v| v.clamp(-c, c));
            }
        }
    }
}
