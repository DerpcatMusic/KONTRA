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
    /// Silent frames before the attack, even if the attack is instantaneous.
    pub delay: u32,
    /// The gain is `floor + depth·level` while the envelope runs.
    pub floor: f32,
    pub depth: f32,
    /// AHD-only one-shot mode: ignores key-up, decays to 0, and ends after decay.
    pub one_shot: bool,
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
        delay: 0,
        floor: 0.0,
        depth: 1.0,
        one_shot: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Delay(u32),
    Attack,
    Hold(u32),
    Decay,
    Sustain,
    SustainGlide,
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
    const IDLE: Self = Self {
        from: 0.0,
        to: 0.0,
        scale: 0.0,
        state: 1.0,
        step: 1.0,
        left: 0,
    };

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

/// Smooth sustain-change ramp length when sustain is edited mid-note (~1.3 ms at 48 kHz).
const SUSTAIN_GLIDE_FRAMES: u32 = 64;

impl Envelope {
    /// Starts an envelope at its attack. Times in `settings` are frames. When
    /// `attack > lag`, `lag` silent frames precede the curve so full level still
    /// lands on time; when `0 < attack <= lag`, the curve runs without lag so
    /// sub-millisecond attacks are not crushed into silence plus a 1-frame step.
    pub fn new(settings: Ahdsr) -> Self {
        let mut env = Self {
            settings,
            stage: Stage::Attack,
            level: 0.0,
            segment: Segment::IDLE,
            wait: 0,
        };
        if settings.delay > 0 {
            env.stage = Stage::Delay(settings.delay);
        } else {
            env.enter_attack();
        }
        env
    }

    /// Updates envelope parameters smoothly from the current level without clicks.
    pub fn update_params(&mut self, settings: Ahdsr) {
        let old = self.settings;
        self.settings = settings;
        match self.stage {
            Stage::Delay(left) => {
                let remaining = settings
                    .delay
                    .saturating_sub(old.delay.saturating_sub(left));
                if remaining == 0 {
                    self.enter_attack();
                } else {
                    self.stage = Stage::Delay(remaining);
                }
            }
            Stage::Attack => {
                if settings.attack == 0 {
                    self.wait = 0;
                    self.level = 1.0;
                    self.enter_after_attack();
                } else if self.wait > 0 {
                    let new_lag = if settings.attack > settings.lag {
                        settings.lag
                    } else {
                        0
                    };
                    self.wait = self.wait.min(new_lag);
                    let curve_frames = settings.attack.saturating_sub(new_lag).max(1);
                    self.segment =
                        Segment::new(self.level, 1.0, curve_frames, settings.attack_shape);
                } else {
                    let old_lag = if old.attack > old.lag { old.lag } else { 0 };
                    let old_total = old.attack.saturating_sub(old_lag).max(1);
                    let new_lag = if settings.attack > settings.lag {
                        settings.lag
                    } else {
                        0
                    };
                    let new_total = settings.attack.saturating_sub(new_lag).max(1);
                    let left = (f64::from(self.segment.left) * f64::from(new_total)
                        / f64::from(old_total))
                    .round() as u32;
                    self.segment =
                        Segment::new(self.level, 1.0, left.max(1), settings.attack_shape);
                }
            }
            Stage::Hold(left) => {
                let elapsed = old.hold.saturating_sub(left);
                let new_left = settings.hold.saturating_sub(elapsed);
                if new_left == 0 {
                    self.enter_decay();
                } else {
                    self.stage = Stage::Hold(new_left);
                }
            }
            Stage::Decay => {
                let target = if settings.one_shot {
                    0.0
                } else {
                    settings.sustain
                };
                if settings.decay == 0 {
                    self.level = target;
                    self.stage = if settings.one_shot {
                        Stage::Off
                    } else {
                        Stage::Sustain
                    };
                } else {
                    let left = (f64::from(self.segment.left) * f64::from(settings.decay)
                        / f64::from(old.decay.max(1)))
                    .round() as u32;
                    self.segment =
                        Segment::new(self.level, target, left.max(1), settings.fall_shape);
                }
            }
            Stage::Sustain => {
                if settings.one_shot {
                    self.enter_decay();
                } else if (settings.sustain - self.level).abs() > 1e-6 {
                    self.segment =
                        Segment::new(self.level, settings.sustain, SUSTAIN_GLIDE_FRAMES, 0.0);
                    self.stage = Stage::SustainGlide;
                }
            }
            Stage::SustainGlide => {
                if settings.one_shot {
                    self.enter_decay();
                } else if settings.sustain != old.sustain {
                    self.segment =
                        Segment::new(self.level, settings.sustain, SUSTAIN_GLIDE_FRAMES, 0.0);
                }
            }
            Stage::Release => {
                if settings.release != old.release || settings.fall_shape != old.fall_shape {
                    let left = (f64::from(self.segment.left) * f64::from(settings.release.max(1))
                        / f64::from(old.release.max(1)))
                    .round() as u32;
                    self.segment = Segment::new(self.level, 0.0, left.max(1), settings.fall_shape);
                }
            }
            Stage::Off => {}
        }
    }

    fn enter_attack(&mut self) {
        let s = self.settings;
        if s.attack == 0 {
            self.level = 1.0;
            self.enter_after_attack();
        } else {
            self.wait = if s.attack > s.lag { s.lag } else { 0 };
            self.segment = Segment::new(0.0, 1.0, s.attack - self.wait, s.attack_shape);
            self.stage = Stage::Attack;
        }
    }

    fn enter_after_attack(&mut self) {
        if self.settings.hold > 0 {
            self.stage = Stage::Hold(self.settings.hold);
        } else {
            self.enter_decay();
        }
    }

    fn enter_decay(&mut self) {
        let s = self.settings;
        let target = if s.one_shot { 0.0 } else { s.sustain };
        if s.decay == 0 || (!s.one_shot && s.sustain >= 1.0) {
            self.level = target;
            self.stage = if s.one_shot {
                Stage::Off
            } else {
                Stage::Sustain
            };
        } else {
            self.segment = Segment::new(self.level, target, s.decay, s.fall_shape);
            self.stage = Stage::Decay;
        }
    }

    pub(crate) fn stage_name(&self) -> &'static str {
        match self.stage {
            Stage::Delay(_) => "delay",
            Stage::Attack => "attack",
            Stage::Hold(_) => "hold",
            Stage::Decay => "decay",
            Stage::Sustain | Stage::SustainGlide => "sustain",
            Stage::Release => "release",
            Stage::Off => "off",
        }
    }

    /// The key-up: the release starts smoothly from the current output gain
    /// (`floor + depth·level`), avoiding any cliff click when `floor > 0`.
    /// Ignored in AHD one-shot mode.
    pub fn release(&mut self) {
        if !self.settings.one_shot {
            self.force_release();
        }
    }

    pub(crate) fn force_release(&mut self) {
        if matches!(self.stage, Stage::Off | Stage::Release) {
            return;
        }
        let s = self.settings;
        let current_gain = self.output_gain();
        self.wait = 0;
        self.level = current_gain;
        self.segment = Segment::new(current_gain, 0.0, s.release.max(1), s.fall_shape);
        self.stage = Stage::Release;
    }

    pub(crate) fn level(&self) -> f32 {
        self.level
    }

    /// Current effective output gain, including `floor` and `depth`.
    pub(crate) fn output_gain(&self) -> f32 {
        match self.stage {
            Stage::Delay(_) | Stage::Off => 0.0,
            Stage::Release => self.level,
            _ => self.settings.floor + self.settings.depth * self.level,
        }
    }

    pub(crate) fn is_off(&self) -> bool {
        self.stage == Stage::Off
    }

    /// The gain for the next frame: `floor + depth·level` while held/decaying,
    /// smooth direct ramp to `0.0` during release, silent once off.
    pub fn tick(&mut self) -> f32 {
        if let Stage::Delay(left) = self.stage {
            if left == 1 {
                self.enter_attack();
            } else {
                self.stage = Stage::Delay(left - 1);
            }
            return 0.0;
        }
        let s = self.settings;
        let was_release = self.stage == Stage::Release;
        let level = self.step();
        if self.stage == Stage::Off {
            return 0.0;
        }
        if was_release {
            level
        } else {
            s.floor + s.depth * level
        }
    }

    /// The next frame's envelope level.
    fn step(&mut self) -> f32 {
        let s = self.settings;
        if self.wait > 0 && self.stage != Stage::Release {
            self.wait -= 1;
            return self.level;
        }
        match self.stage {
            Stage::Attack | Stage::Decay | Stage::SustainGlide | Stage::Release => {
                let (level, ended) = self.segment.next();
                self.level = level;
                if ended {
                    match self.stage {
                        Stage::Attack => self.enter_after_attack(),
                        Stage::Decay => {
                            self.stage = if s.one_shot {
                                Stage::Off
                            } else {
                                Stage::Sustain
                            };
                        }
                        Stage::SustainGlide => self.stage = Stage::Sustain,
                        _ => self.stage = Stage::Off,
                    }
                }
            }
            Stage::Hold(left) => {
                self.level = 1.0;
                if left > 1 {
                    self.stage = Stage::Hold(left - 1);
                } else {
                    self.enter_decay();
                }
            }
            Stage::Sustain => self.level = s.sustain,
            Stage::Off | Stage::Delay(_) => self.level = 0.0,
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
        let want = [
            0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.75, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let got = lagged(4);
        assert!(
            got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5),
            "{got:?}"
        );
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
        assert!(
            got.iter()
                .zip([0.25, 0.5, 0.5, 0.0, 0.0])
                .all(|(g, w)| (g - w).abs() < 1e-5),
            "{got:?}"
        );
        let got = run(0.5, 0.5);
        assert!(
            got.iter()
                .zip([0.75, 1.0, 1.0, 0.0, 0.0])
                .all(|(g, w)| (g - w).abs() < 1e-5),
            "{got:?}"
        );
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

    #[test]
    fn ahd_one_shot_ignores_release_and_sustain_and_ends_after_decay() {
        let mut env = Envelope::new(Ahdsr {
            attack: 2,
            hold: 2,
            decay: 4,
            sustain: 0.8,
            release: 100,
            one_shot: true,
            ..Ahdsr::GATE
        });
        let mut out = vec![env.tick(), env.tick(), env.tick()];
        // Key-up during Hold must be ignored in AHD one-shot mode.
        env.release();
        out.extend((0..6).map(|_| env.tick()));
        let want = [0.5, 1.0, 1.0, 1.0, 0.75, 0.5, 0.25, 0.0, 0.0];
        assert!(
            out.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5),
            "{out:?}"
        );
        assert!(env.is_off());
    }

    #[test]
    fn hold_duration_is_exact_including_zero() {
        let run = |hold| {
            let mut env = Envelope::new(Ahdsr {
                attack: 2,
                hold,
                decay: 4,
                sustain: 0.0,
                ..Ahdsr::GATE
            });
            (0..(6 + hold as usize))
                .map(|_| env.tick())
                .collect::<Vec<_>>()
        };
        let zero = run(0);
        let want_zero = [0.5, 1.0, 0.75, 0.5, 0.25, 0.0];
        assert!(
            zero.iter()
                .zip(want_zero)
                .all(|(g, w)| (g - w).abs() < 1e-5),
            "hold=0: {zero:?}"
        );
        let three = run(3);
        let want_three = [0.5, 1.0, 1.0, 1.0, 1.0, 0.75, 0.5, 0.25, 0.0];
        assert!(
            three
                .iter()
                .zip(want_three)
                .all(|(g, w)| (g - w).abs() < 1e-5),
            "hold=3: {three:?}"
        );
    }

    #[test]
    fn floor_above_zero_releases_smoothly_to_zero_without_click() {
        let mut env = Envelope::new(Ahdsr {
            attack: 2,
            release: 20,
            floor: 0.5,
            depth: 0.5,
            ..Ahdsr::GATE
        });
        assert!((env.tick() - 0.75).abs() < 1e-5);
        assert!((env.tick() - 1.0).abs() < 1e-5);
        env.release();
        let rel: Vec<f32> = (0..20).map(|_| env.tick()).collect();
        assert!(env.is_off());
        assert_eq!(*rel.last().unwrap(), 0.0);
        let mut prev = 1.0f32;
        for (i, &v) in rel.iter().enumerate() {
            assert!(v <= prev + 1e-6, "non-monotonic at {i}: {prev} -> {v}");
            assert!(
                (prev - v) <= 0.06,
                "release cliff at {i}: {prev} -> {v} ({rel:?})"
            );
            prev = v;
        }
    }

    #[test]
    fn short_attack_below_lag_ramps_smoothly_instead_of_crushing_to_step() {
        let mut env = Envelope::new(Ahdsr {
            attack: 10,
            lag: 33,
            ..Ahdsr::GATE
        });
        let out: Vec<f32> = (0..10).map(|_| env.tick()).collect();
        assert!(
            out[0] > 0.05,
            "first frame must ramp, not sit silent: {out:?}"
        );
        assert!((out[9] - 1.0).abs() < 1e-5, "{out:?}");
        assert!(out.windows(2).all(|w| w[1] > w[0] && (w[1] - w[0]) < 0.15));
    }

    #[test]
    fn explicit_delay_is_silent_even_with_instant_attack() {
        let mut env = Envelope::new(Ahdsr {
            delay: 4800,
            floor: 0.25,
            depth: 0.75,
            ..Ahdsr::GATE
        });
        assert!((0..4800).all(|_| env.tick() == 0.0));
        assert_eq!(env.tick(), 1.0);
    }

    #[test]
    fn release_edit_does_not_interrupt_sustain_glide() {
        let base = Ahdsr {
            sustain: 0.8,
            ..Ahdsr::GATE
        };
        let mut env = Envelope::new(base);
        let new = Ahdsr {
            sustain: 0.2,
            ..base
        };
        env.update_params(new);
        let mut reference = env;
        for _ in 0..32 {
            env.tick();
            reference.tick();
        }
        env.update_params(Ahdsr {
            release: 128,
            ..new
        });
        for _ in 0..32 {
            assert_eq!(env.tick(), reference.tick());
        }
        assert_eq!(env.tick(), 0.2);
    }

    #[test]
    fn enabling_one_shot_in_sustain_decays_from_current_level_without_jumping() {
        let base = Ahdsr {
            decay: 4,
            sustain: 0.5,
            ..Ahdsr::GATE
        };
        let mut env = Envelope::new(base);
        for _ in 0..5 {
            env.tick();
        }
        assert!((env.tick() - 0.5).abs() < 1e-5);
        env.update_params(Ahdsr {
            one_shot: true,
            ..base
        });
        let out: Vec<f32> = (0..4).map(|_| env.tick()).collect();
        let want = [0.375, 0.25, 0.125, 0.0];
        assert!(
            out.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5),
            "{out:?}"
        );
        assert!(env.is_off());
    }

    #[test]
    fn update_params_transitions_sustain_and_stages_smoothly() {
        let base = Ahdsr {
            attack: 0,
            decay: 0,
            sustain: 0.8,
            release: 100,
            ..Ahdsr::GATE
        };
        let mut env = Envelope::new(base);
        assert!((env.tick() - 0.8).abs() < 1e-5);
        env.update_params(Ahdsr {
            sustain: 0.2,
            ..base
        });
        let glide: Vec<f32> = (0..64).map(|_| env.tick()).collect();
        let mut prev = 0.8f32;
        for &v in &glide {
            assert!(
                (prev - v).abs() < 0.02,
                "step click in sustain glide: {prev} -> {v}"
            );
            prev = v;
        }
        assert!((glide[63] - 0.2).abs() < 1e-5, "{glide:?}");

        // Live attack duration change mid-attack continues from current level.
        let mut env = Envelope::new(Ahdsr {
            attack: 100,
            ..Ahdsr::GATE
        });
        for _ in 0..50 {
            env.tick();
        }
        let mid = env.level();
        env.update_params(Ahdsr {
            attack: 200,
            ..Ahdsr::GATE
        });
        let next = env.tick();
        assert!(
            next > mid && (next - mid) < 0.02,
            "mid-attack jump: {mid} -> {next}"
        );
    }
}
