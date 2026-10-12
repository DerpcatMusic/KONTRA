//! Port from v1 0cb7a8a0:src/fx/blocks.rs.
//! Disconnected source preparation; this is v1's documented six-stage proxy,
//! not an admission of the unresolved native legacy Phaser law.
use std::f32::consts::{PI, TAU};
type Fields = [f32; 12];
const ANTI_DENORMAL: f32 = 1e-20;
fn lfo_hz(x: f32) -> f32 {
    0.01 * 1000f32.powf(x.clamp(0.0, 1.0))
}

/// Legacy Phaser: six first-order all-passes per channel swept from
/// 200 Hz up `depth · 5` octaves by a sine LFO, with feedback. Wet only:
/// the notches come from the slot's dry mix.
pub(super) struct Phaser {
    inc: f32,
    phase: f32,
    offset: f32,
    octaves: f32,
    feedback: f32,
    rate: f32,
    s: [[f32; 6]; 2],
    last: [f32; 2],
}

/// Frames between all-pass coefficient updates.
const PHASER_STEP: usize = 16;

impl Phaser {
    pub(super) fn tune(&mut self, f: &Fields, rate: f32) {
        let x = |i: usize| f[i].clamp(0.0, 1.0);
        self.octaves = 5.0 * x(0);
        self.feedback = 0.9 * x(1);
        self.inc = lfo_hz(x(2)) / rate * PHASER_STEP as f32;
        self.offset = 0.5 * x(3);
        self.rate = rate;
    }

    fn coefficient(&self, p: f32) -> f32 {
        let hz = 200.0 * (self.octaves * (0.5 + 0.5 * (TAU * p).sin())).exp2();
        let t = (PI * hz.min(0.45 * self.rate) / self.rate).tan();
        (t - 1.0) / (t + 1.0)
    }

    pub(super) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        for start in (0..n).step_by(PHASER_STEP) {
            let end = (start + PHASER_STEP).min(n);
            self.phase = (self.phase + self.inc).fract();
            let coeffs = [
                self.coefficient(self.phase),
                self.coefficient(self.phase + self.offset),
            ];
            for (ch, buf) in [&mut left[start..end], &mut right[start..end]]
                .into_iter()
                .enumerate()
            {
                let a = coeffs[ch];
                for x in buf.iter_mut() {
                    let mut y = *x + self.feedback * self.last[ch];
                    for s in &mut self.s[ch] {
                        let out = a * y + *s;
                        *s = y - a * out + ANTI_DENORMAL;
                        y = out;
                    }
                    self.last[ch] = y;
                    *x = y;
                }
            }
        }
    }
}
