//! Amplitude envelope: attack, hold, decay, sustain, release.
//!
//! Every moving stage is one segment of a fixed length shaped
//! `expm1(k·t)/expm1(k)` over t in 0..=1, one multiply per frame, reaching its
//! target exactly at the end. Formats choose k (0 is linear).

/// Envelope settings in frames and linear gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ahdsr {
    pub attack: u32,
    pub hold: u32,
    /// Frames from full level to sustain.
    pub decay: u32,
    pub sustain: f32,
    /// Frames from the level at key-up to silence.
    pub release: u32,
    /// Shape of the attack: 0 is linear, positive starts slowly.
    pub attack_shape: f32,
    /// Shape of decay and release.
    pub fall_shape: f32,
}

impl Ahdsr {
    /// No shaping: full level at once, sustain at 1, a short click guard on release.
    pub const GATE: Self = Self {
        attack: 0,
        hold: 0,
        decay: 0,
        sustain: 1.0,
        release: 64,
        attack_shape: 0.0,
        fall_shape: 0.0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Attack,
    Hold(u32),
    Decay,
    Sustain,
    Release,
    Off,
}

/// A moving stage: `level = from + scale·(state − 1)`, `state *= step` per frame.
#[derive(Clone, Copy, Debug)]
struct Segment {
    from: f32,
    to: f32,
    scale: f64,
    state: f64,
    step: f64,
    left: u32,
}

impl Segment {
    fn new(from: f32, to: f32, frames: u32, shape: f32) -> Self {
        // Below this the shape is a straight line to f64 precision.
        let k = if shape.abs() < 1e-6 {
            1e-6
        } else {
            f64::from(shape)
        };
        Self {
            from,
            to,
            scale: f64::from(to - from) / k.exp_m1(),
            state: 1.0,
            step: (k / f64::from(frames.max(1))).exp(),
            left: frames,
        }
    }

    /// The next frame's level, and whether the segment ended on it.
    fn next(&mut self) -> (f32, bool) {
        self.left = self.left.saturating_sub(1);
        if self.left == 0 {
            return (self.to, true);
        }
        self.state *= self.step;
        let level = f64::from(self.from) + self.scale * (self.state - 1.0);
        (level as f32, false)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Envelope {
    settings: Ahdsr,
    stage: Stage,
    level: f32,
    segment: Segment,
}

impl Envelope {
    pub(crate) fn new(settings: Ahdsr) -> Self {
        let mut env = Self {
            settings,
            stage: Stage::Attack,
            level: 0.0,
            segment: Segment::new(0.0, 1.0, settings.attack, settings.attack_shape),
        };
        if settings.attack == 0 {
            env.level = 1.0;
            env.stage = Stage::Hold(0);
        }
        env
    }

    pub(crate) fn stage_name(&self) -> &'static str {
        match self.stage {
            Stage::Attack => "attack",
            Stage::Hold(_) => "hold",
            Stage::Decay => "decay",
            Stage::Sustain => "sustain",
            Stage::Release => "release",
            Stage::Off => "off",
        }
    }

    pub(crate) fn release(&mut self) {
        if self.stage != Stage::Off && self.stage != Stage::Release {
            self.start_release();
        }
    }

    fn start_release(&mut self) {
        let s = self.settings;
        self.segment = Segment::new(self.level, 0.0, s.release.max(1), s.fall_shape);
        self.stage = Stage::Release;
    }

    pub(crate) fn level(&self) -> f32 {
        self.level
    }

    pub(crate) fn is_off(&self) -> bool {
        self.stage == Stage::Off
    }

    /// The gain for the next frame.
    pub(crate) fn next(&mut self) -> f32 {
        let s = self.settings;
        match self.stage {
            Stage::Attack | Stage::Decay | Stage::Release => {
                let (level, ended) = self.segment.next();
                self.level = level;
                if ended {
                    self.stage = match self.stage {
                        Stage::Attack => Stage::Hold(0),
                        Stage::Decay => Stage::Sustain,
                        _ => Stage::Off,
                    };
                }
            }
            Stage::Hold(n) => {
                self.level = 1.0;
                if n < s.hold {
                    self.stage = Stage::Hold(n + 1);
                } else if s.decay == 0 || s.sustain >= 1.0 {
                    self.level = s.sustain;
                    self.stage = Stage::Sustain;
                } else {
                    self.segment = Segment::new(1.0, s.sustain, s.decay, s.fall_shape);
                    self.stage = Stage::Decay;
                }
            }
            Stage::Sustain => self.level = s.sustain,
            Stage::Off => self.level = 0.0,
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linear_attack_reaches_full_level_on_its_last_frame() {
        let mut env = Envelope::new(Ahdsr {
            attack: 4,
            ..Ahdsr::GATE
        });
        let levels: Vec<f32> = (0..5).map(|_| env.next()).collect();
        for (got, want) in levels.iter().zip([0.25, 0.5, 0.75, 1.0, 1.0]) {
            assert!((got - want).abs() < 1e-5, "{levels:?}");
        }
    }

    /// A release shaped k = ln(3/43) is `1.075·(3/43)^t − 0.075` of the key-up
    /// level, silent exactly at the release time.
    #[test]
    fn a_shaped_release_follows_its_curve_and_ends_on_time() {
        let mut env = Envelope::new(Ahdsr {
            release: 1000,
            fall_shape: (3.0f32 / 43.0).ln(),
            ..Ahdsr::GATE
        });
        env.next();
        env.release();
        let levels: Vec<f32> = (0..1000).map(|_| env.next()).collect();
        let half = 1.075 * (3.0f32 / 43.0).sqrt() - 0.075;
        assert!((levels[499] - half).abs() < 1e-4, "{}", levels[499]);
        assert_eq!(levels[999], 0.0);
        assert!(env.is_off());
    }

    #[test]
    fn decay_settles_on_sustain() {
        let mut env = Envelope::new(Ahdsr {
            decay: 100,
            sustain: 0.5,
            ..Ahdsr::GATE
        });
        for _ in 0..101 {
            env.next();
        }
        assert_eq!(env.next(), 0.5);
    }
}
