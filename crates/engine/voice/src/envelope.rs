//! Amplitude envelope: attack, hold, decay, sustain, release.
//!
//! Every moving stage is one segment of a fixed length shaped
//! `expm1(k·t)/expm1(k)` over t in 0..=1, one multiply per frame, reaching its
//! target exactly at the end. Formats choose k (0 is linear), and a lag: an
//! attack stays silent that many frames and then runs in the frames left, so
//! full level still comes on time. Without an attack there is no lag.

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
    /// Silent frames at the start of an attack, taken out of its length.
    pub lag: u32,
    /// The gain is `floor + depth·level` while the envelope runs.
    pub floor: f32,
    pub depth: f32,
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
        lag: 0,
        floor: 0.0,
        depth: 1.0,
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

/// A running amplitude envelope: one gain per frame.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    settings: Ahdsr,
    stage: Stage,
    level: f32,
    segment: Segment,
    /// Silent frames before the attack moves.
    wait: u32,
}

impl Envelope {
    /// Starts an envelope at its attack. Times in `settings` are frames. The
    /// lag is capped at `attack − 1` frames, so the attack always moves on its
    /// last frame; with no attack there is no lag and the first frame is full.
    pub fn new(settings: Ahdsr) -> Self {
        let wait = settings.lag.min(settings.attack.saturating_sub(1));
        let mut env = Self {
            settings,
            stage: Stage::Attack,
            level: 0.0,
            segment: Segment::new(0.0, 1.0, settings.attack - wait, settings.attack_shape),
            wait,
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

    /// The key-up: the release starts from the current level.
    pub fn release(&mut self) {
        if self.stage == Stage::Off || self.stage == Stage::Release {
            return;
        }
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

    /// The gain for the next frame: `floor + depth·level`, silent once off.
    pub fn tick(&mut self) -> f32 {
        let s = self.settings;
        let level = self.step();
        if self.stage == Stage::Off {
            return 0.0;
        }
        s.floor + s.depth * level
    }

    /// The next frame's envelope level.
    fn step(&mut self) -> f32 {
        let s = self.settings;
        if self.wait > 0 && self.stage != Stage::Release {
            self.wait -= 1;
            return self.level;
        }
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
        let levels: Vec<f32> = (0..5).map(|_| env.tick()).collect();
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
        env.tick();
        env.release();
        let levels: Vec<f32> = (0..1000).map(|_| env.tick()).collect();
        let half = 1.075 * (3.0f32 / 43.0).sqrt() - 0.075;
        assert!((levels[499] - half).abs() < 1e-4, "{}", levels[499]);
        assert_eq!(levels[999], 0.0);
        assert!(env.is_off());
    }

    /// Measured on Kontakt 8.13 (lag 33): an attack is silent 33 frames and
    /// still full on time; without an attack the first frame is full; a
    /// key-up releases at once.
    #[test]
    fn a_lag_squeezes_the_attack_but_not_full_level_or_the_release() {
        let lagged = |attack| {
            let mut env = Envelope::new(Ahdsr {
                attack,
                release: 4,
                lag: 3,
                ..Ahdsr::GATE
            });
            let mut out: Vec<f32> = (0..8).map(|_| env.tick()).collect();
            env.release();
            out.extend((0..8).map(|_| env.tick()));
            out
        };
        let want = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.75, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0];
        let got = lagged(4);
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5), "{got:?}");
        assert_eq!(lagged(0)[0], 1.0);
    }

    /// A volume route at half intensity: scaled, the envelope tops out at
    /// 0.5; floored, it starts at 0.5. Both are silent once released.
    #[test]
    fn depth_and_floor_shape_the_gain() {
        let run = |floor, depth| {
            let mut env = Envelope::new(Ahdsr {
                attack: 2,
                release: 1,
                floor,
                depth,
                ..Ahdsr::GATE
            });
            let mut out: Vec<f32> = (0..3).map(|_| env.tick()).collect();
            env.release();
            out.extend((0..2).map(|_| env.tick()));
            out
        };
        let got = run(0.0, 0.5);
        assert!(got.iter().zip([0.25, 0.5, 0.5, 0.0, 0.0]).all(|(g, w)| (g - w).abs() < 1e-5), "{got:?}");
        let got = run(0.5, 0.5);
        assert!(got.iter().zip([0.75, 1.0, 1.0, 0.0, 0.0]).all(|(g, w)| (g - w).abs() < 1e-5), "{got:?}");
    }

    /// A key-up during the lag releases at once: silent, and off after the
    /// release time, without waiting out the rest of the attack.
    #[test]
    fn a_release_in_the_lag_ends_after_the_release_time() {
        let mut env = Envelope::new(Ahdsr {
            attack: 100,
            lag: 50,
            release: 4,
            ..Ahdsr::GATE
        });
        assert_eq!(env.tick(), 0.0);
        env.release();
        let levels: Vec<f32> = (0..4).map(|_| env.tick()).collect();
        assert_eq!(levels, [0.0; 4]);
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
            env.tick();
        }
        assert_eq!(env.tick(), 0.5);
    }
}
