//! Amplitude envelope: attack, hold, decay, sustain, release.

/// Envelope settings in frames and linear gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ahdsr {
    pub attack: u32,
    pub hold: u32,
    /// Frames for the decay to fall about 60 dB toward sustain.
    pub decay: u32,
    pub sustain: f32,
    /// Frames for the release to fall about 60 dB.
    pub release: u32,
}

impl Ahdsr {
    /// No shaping: full level at once, sustain at 1, a 1 ms-ish click guard on release.
    pub const GATE: Self = Self {
        attack: 0,
        hold: 0,
        decay: 0,
        sustain: 1.0,
        release: 64,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Attack(u32),
    Hold(u32),
    Decay,
    Sustain,
    Release,
    Off,
}

/// Below this a released envelope is silent (−120 dB).
const FLOOR: f32 = 1e-6;

// ponytail: exponential decay/release toward a 60 dB fall; Kontakt's curve
// shapes come from the law table when a measurement shows the difference.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Envelope {
    settings: Ahdsr,
    stage: Stage,
    level: f32,
}

/// Per-frame multiplier for a 60 dB fall over `frames`.
fn coefficient(frames: u32) -> f32 {
    if frames == 0 {
        0.0
    } else {
        (1e-3f32).powf(1.0 / frames as f32)
    }
}

impl Envelope {
    pub(crate) fn new(settings: Ahdsr) -> Self {
        let stage = if settings.attack > 0 {
            Stage::Attack(0)
        } else {
            Stage::Hold(0)
        };
        let level = if settings.attack > 0 { 0.0 } else { 1.0 };
        Self {
            settings,
            stage,
            level,
        }
    }

    pub(crate) fn release(&mut self) {
        if self.stage != Stage::Off {
            self.stage = Stage::Release;
        }
    }

    /// Releases over `frames` (60 dB), whatever the release setting.
    pub(crate) fn fade(&mut self, frames: u32) {
        self.settings.release = frames.max(1);
        self.release();
    }

    pub(crate) fn is_off(&self) -> bool {
        self.stage == Stage::Off
    }

    /// The gain for the next frame.
    pub(crate) fn next(&mut self) -> f32 {
        let s = self.settings;
        match self.stage {
            Stage::Attack(n) => {
                self.level = (n + 1) as f32 / s.attack as f32;
                self.stage = if n + 1 >= s.attack {
                    Stage::Hold(0)
                } else {
                    Stage::Attack(n + 1)
                };
            }
            Stage::Hold(n) => {
                self.level = 1.0;
                if n >= s.hold {
                    self.stage = Stage::Decay;
                } else {
                    self.stage = Stage::Hold(n + 1);
                }
            }
            Stage::Decay => {
                let k = coefficient(s.decay);
                self.level = s.sustain + (self.level - s.sustain) * k;
                if (self.level - s.sustain).abs() < FLOOR {
                    self.level = s.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = s.sustain,
            Stage::Release => {
                self.level *= coefficient(s.release);
                if self.level < FLOOR {
                    self.level = 0.0;
                    self.stage = Stage::Off;
                }
            }
            Stage::Off => self.level = 0.0,
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_is_linear_and_reaches_full_level() {
        let mut env = Envelope::new(Ahdsr {
            attack: 4,
            ..Ahdsr::GATE
        });
        let levels: Vec<f32> = (0..5).map(|_| env.next()).collect();
        assert_eq!(levels, [0.25, 0.5, 0.75, 1.0, 1.0]);
    }

    #[test]
    fn decay_settles_on_sustain() {
        let mut env = Envelope::new(Ahdsr {
            decay: 100,
            sustain: 0.5,
            ..Ahdsr::GATE
        });
        for _ in 0..10_000 {
            env.next();
        }
        assert_eq!(env.next(), 0.5);
    }

    #[test]
    fn release_falls_60_db_over_its_time_and_then_ends() {
        let mut env = Envelope::new(Ahdsr {
            release: 1000,
            ..Ahdsr::GATE
        });
        env.next();
        env.release();
        let mut level = 1.0;
        for _ in 0..1000 {
            level = env.next();
        }
        assert!((level - 1e-3).abs() < 1e-4, "{level}");
        while !env.is_off() {
            env.next();
        }
        assert_eq!(env.next(), 0.0);
    }
}
