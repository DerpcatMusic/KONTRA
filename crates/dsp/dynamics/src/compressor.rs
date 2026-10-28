//! A feed-forward compressor: peak level in dB, a soft-knee static curve,
//! attack and release on the gain reduction, make-up and a dry mix.

use crate::{gain, pole};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub threshold_db: f32,
    /// Input dB over the threshold per output dB over it; infinite limits.
    pub ratio: f32,
    /// Width of the soft knee around the threshold; 0 is a hard knee.
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    /// Both channels follow the louder one.
    pub link: bool,
    /// 0..=1: the compressed share of the output, the rest is the input.
    pub mix: f32,
}

pub struct Compressor {
    s: Settings,
    attack: f32,
    release: f32,
    makeup: f32,
    /// Gain reduction per channel, dB (≥ 0).
    reduction: [f32; 2],
}

impl Compressor {
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let mut c = Self {
            s,
            attack: 1.0,
            release: 1.0,
            makeup: 1.0,
            reduction: [0.0; 2],
        };
        c.set(s, rate);
        c
    }

    /// Changes the settings while it plays; the reduction carries over.
    pub fn set(&mut self, s: Settings, rate: u32) {
        let rate = rate as f32;
        self.s = s;
        self.attack = pole(s.attack_ms, rate);
        self.release = pole(s.release_ms, rate);
        self.makeup = gain(s.makeup_db);
    }

    /// Compresses `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let mix = self.s.mix.clamp(0.0, 1.0);
        for frame in io {
            let [l, r] = frame.map(f32::abs);
            let levels = if self.s.link { [l.max(r); 2] } else { [l, r] };
            for ((x, level), reduction) in frame.iter_mut().zip(levels).zip(&mut self.reduction) {
                let db = 20.0 * level.max(1e-9).log10();
                let target = curve(&self.s, db - self.s.threshold_db);
                let k = if target > *reduction {
                    self.attack
                } else {
                    self.release
                };
                *reduction += (target - *reduction) * k;
                // Released to silence: no subnormal tail.
                if *reduction < 1e-6 {
                    *reduction = 0.0;
                }
                let wet = *x * gain(-*reduction) * self.makeup;
                *x += (wet - *x) * mix;
            }
        }
    }

    /// Current gain reduction, dB, per channel.
    #[must_use]
    pub fn reduction(&self) -> [f32; 2] {
        self.reduction
    }
}

/// The static curve: dB of reduction for a level `over` dB past the
/// threshold.
fn curve(s: &Settings, over: f32) -> f32 {
    let slope = 1.0 - 1.0 / s.ratio.max(1.0);
    let w = s.knee_db.max(0.0);
    let into = if 2.0 * over <= -w {
        0.0
    } else if 2.0 * over < w {
        let t = over + w / 2.0;
        t * t / (2.0 * w)
    } else {
        over
    };
    into * slope
}
