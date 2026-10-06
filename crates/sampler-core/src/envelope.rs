//! Native DAHDSR/AHD envelopes. Durations are frames at the prepared sample rate.
use crate::Error;

/// Normalized exponential segment: expm1(curvature * t) / expm1(curvature).
/// Zero is linear. Positive curvature starts slowly; negative starts quickly.
/// This native unit is not a Kontakt or Falcon parameter value.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnvelopeCurve(f64);

impl EnvelopeCurve {
    pub fn exponential(curvature: f64) -> Result<Self, Error> {
        if !curvature.is_finite() || !(-32.0..=32.0).contains(&curvature) {
            return Err(Error::InvalidInput);
        }
        Ok(Self(curvature))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Curve {
    curvature: f64,
    denominator: f64,
    multiplier: f64,
    delta: f64,
}

impl Curve {
    fn exprel(x: f64) -> f64 {
        if x.abs() < 1e-5 {
            1.0 + x * (0.5 + x * (1.0 / 6.0 + x / 24.0))
        } else {
            x.exp_m1() / x
        }
    }

    fn new(curve: EnvelopeCurve, duration: u32) -> Self {
        if curve.0 == 0.0 || duration == 0 {
            return Self::default();
        }
        let denominator = Self::exprel(curve.0);
        let step = curve.0 / f64::from(duration);
        Self {
            curvature: curve.0,
            denominator,
            multiplier: step.exp(),
            delta: Self::exprel(step) / denominator / f64::from(duration),
        }
    }

    fn at(self, age: u32, duration: u32) -> f64 {
        let t = f64::from(age) / f64::from(duration);
        t * Self::exprel(self.curvature * t) / self.denominator
    }
}

/// Validated immutable envelope parameters. Zero-length stages are skipped.
/// Attack starts at zero, decay at one, release at the next held sample's level.
/// Each ramp reaches its endpoint at the exclusive end of its duration.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    delay: u32,
    attack: u32,
    hold: u32,
    decay: u32,
    sustain: f32,
    release: u32,
    one_shot: bool,
    curves: [Curve; 3],
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            delay: 0,
            attack: 0,
            hold: 0,
            decay: 0,
            sustain: 1.0,
            release: 0,
            one_shot: false,
            curves: [Curve::default(); 3],
        }
    }
}

impl Envelope {
    pub fn new(
        attack: u32,
        hold: u32,
        decay: u32,
        sustain: f32,
        release: u32,
    ) -> Result<Self, Error> {
        if !sustain.is_finite() || !(0.0..=1.0).contains(&sustain) {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            attack,
            hold,
            decay,
            sustain,
            release,
            ..Self::default()
        })
    }

    /// AHD completes independently of gate closure, then retires its source.
    /// Explicit stops, panic and family chokes still apply. Source-loop release
    /// policy is independent of this envelope gate policy.
    pub fn one_shot(attack: u32, hold: u32, decay: u32) -> Self {
        Self {
            attack,
            hold,
            decay,
            sustain: 0.0,
            one_shot: true,
            ..Self::default()
        }
    }

    /// Replace one stage's frames (or Sustain's 0..=1000 level), keeping
    /// the stage's curvature. Script engine parameters; see `script_params`.
    pub(crate) fn with_stage(mut self, stage: crate::EnvelopeStage, value: u32) -> Self {
        use crate::EnvelopeStage as S;
        let recurve = |curve: Curve, frames| Curve::new(EnvelopeCurve(curve.curvature), frames);
        match stage {
            S::Attack => {
                self.attack = value;
                self.curves[0] = recurve(self.curves[0], value);
            }
            S::Hold => self.hold = value,
            S::Decay => {
                self.decay = value;
                self.curves[1] = recurve(self.curves[1], value);
            }
            S::Sustain => self.sustain = (value.min(1000) as f32) / 1000.0,
            S::Release => {
                self.release = value;
                self.curves[2] = recurve(self.curves[2], value);
            }
        }
        self
    }

    pub fn with_delay(mut self, frames: u32) -> Self {
        self.delay = frames;
        self
    }

    /// Coefficients are compiled here, off audio. No per-sample power function.
    pub fn with_curves(
        mut self,
        attack: EnvelopeCurve,
        decay: EnvelopeCurve,
        release: EnvelopeCurve,
    ) -> Self {
        self.curves = [
            Curve::new(attack, self.attack),
            Curve::new(decay, self.decay),
            Curve::new(release, self.release),
        ];
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EnvelopeState {
    shape: Envelope,
    phase: Phase,
    age: u32,
    release_level: f32,
    progress: f64,
    delta: f64,
}

impl EnvelopeState {
    pub(super) fn new(shape: Envelope) -> Self {
        let mut state = Self {
            shape,
            phase: Phase::Delay,
            age: 0,
            release_level: 0.0,
            progress: 0.0,
            delta: 0.0,
        };
        state.enter(Phase::Delay);
        state
    }

    fn duration(&self) -> u32 {
        match self.phase {
            Phase::Delay => self.shape.delay,
            Phase::Attack => self.shape.attack,
            Phase::Hold => self.shape.hold,
            Phase::Decay => self.shape.decay,
            Phase::Release => self.shape.release,
            Phase::Sustain | Phase::Done => 0,
        }
    }

    fn curve(&self) -> Curve {
        match self.phase {
            Phase::Attack => self.shape.curves[0],
            Phase::Decay => self.shape.curves[1],
            Phase::Release => self.shape.curves[2],
            _ => Curve::default(),
        }
    }

    fn following(&self) -> Phase {
        match self.phase {
            Phase::Delay => Phase::Attack,
            Phase::Attack => Phase::Hold,
            Phase::Hold => Phase::Decay,
            Phase::Decay if !self.shape.one_shot => Phase::Sustain,
            _ => Phase::Done,
        }
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        // At most the four finite onset stages; no cyclic zero-time transitions.
        while !matches!(self.phase, Phase::Sustain | Phase::Done) && self.duration() == 0 {
            self.phase = self.following();
        }
        self.age = 0;
        self.progress = 0.0;
        self.delta = self.curve().delta;
    }

    fn level(&self) -> f32 {
        let e = self.shape;
        let curved = self.curve().curvature != 0.0;
        let position = self.progress.clamp(0.0, 1.0);
        match self.phase {
            Phase::Delay | Phase::Done => 0.0,
            Phase::Attack if curved => position as f32,
            Phase::Attack => (f64::from(self.age) / f64::from(e.attack)) as f32,
            Phase::Hold => 1.0,
            Phase::Decay if curved => (1.0 + (f64::from(e.sustain) - 1.0) * position) as f32,
            Phase::Decay => {
                (1.0 + (f64::from(e.sustain) - 1.0) * f64::from(self.age) / f64::from(e.decay))
                    as f32
            }
            Phase::Sustain => e.sustain,
            Phase::Release => {
                let position = if curved {
                    position
                } else {
                    f64::from(self.age) / f64::from(e.release)
                };
                (f64::from(self.release_level) * (1.0 - position)) as f32
            }
        }
    }

    /// Repeated cleanup must never restart an already progressing release.
    pub(super) fn release(&mut self) {
        if !self.shape.one_shot && !matches!(self.phase, Phase::Release | Phase::Done) {
            self.release_level = self.level();
            self.enter(Phase::Release);
        }
    }

    /// Capture any curved stage's next level, then apply a non-extending linear fade.
    pub(super) fn choke(&mut self, frames: u32) {
        if (self.shape.one_shot || matches!(self.phase, Phase::Release | Phase::Done))
            && frames as usize >= self.remaining()
        {
            return;
        }
        self.release_level = self.level();
        self.shape.release = frames;
        self.shape.curves[2] = Curve::default();
        self.enter(Phase::Release);
    }

    pub(super) fn releasing(&self) -> bool {
        matches!(self.phase, Phase::Release | Phase::Done)
    }

    /// The level the next frame starts from.
    pub(super) fn current(&self) -> f32 {
        self.level()
    }

    pub(super) fn done(&self) -> bool {
        self.phase == Phase::Done
    }

    pub(super) fn remaining(&self) -> usize {
        let frames = match self.phase {
            Phase::Done => 0,
            Phase::Release => u64::from(self.shape.release - self.age),
            _ if self.shape.one_shot => {
                let e = self.shape;
                let later = match self.phase {
                    Phase::Delay => u64::from(e.attack) + u64::from(e.hold) + u64::from(e.decay),
                    Phase::Attack => u64::from(e.hold) + u64::from(e.decay),
                    Phase::Hold => u64::from(e.decay),
                    _ => 0,
                };
                u64::from(self.duration() - self.age) + later
            }
            _ => return usize::MAX,
        };
        usize::try_from(frames).unwrap_or(usize::MAX)
    }

    /// Only return levels that remain constant indefinitely: callers may skip clocks.
    pub(super) fn constant_level(&self) -> Option<f32> {
        if self.phase == Phase::Sustain {
            return Some(self.shape.sustain);
        }
        if !self.shape.one_shot
            && self.shape.attack == 0
            && self.shape.sustain == 1.0
            && matches!(self.phase, Phase::Hold | Phase::Decay)
        {
            return Some(1.0);
        }
        None
    }

    /// Level after `frames` more frames, as if `next` ran that many times.
    /// Linear stages jump; curved stages still step per frame.
    // ponytail: curved stages step per frame; jump with Curve::at if modulation envelopes get hot.
    pub(super) fn advance(&mut self, mut frames: u32) -> f32 {
        while frames > 0 && !matches!(self.phase, Phase::Sustain | Phase::Done) {
            if self.curve().curvature != 0.0 {
                self.next();
                frames -= 1;
                continue;
            }
            let step = frames.min(self.duration() - self.age);
            self.age += step;
            frames -= step;
            if self.age == self.duration() {
                self.enter(self.following());
            }
        }
        self.level()
    }

    pub(super) fn next(&mut self) -> f32 {
        let value = self.level();
        if matches!(self.phase, Phase::Sustain | Phase::Done) {
            return value;
        }
        self.age += 1; // Strictly below the u32 duration.
        if self.age == self.duration() {
            self.enter(self.following());
        } else {
            let curve = self.curve();
            if curve.curvature != 0.0 {
                if self.age.is_multiple_of(64) {
                    // Anchor to the source clock, never the host block. Bounded
                    // recurrence spans avoid drift even for u32::MAX durations.
                    self.progress = curve.at(self.age, self.duration());
                    self.delta = curve.delta
                        * (curve.curvature * (f64::from(self.age) / f64::from(self.duration())))
                            .exp();
                } else {
                    self.progress += self.delta;
                    self.delta *= curve.multiplier;
                }
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_curves_keep_finite_monotone_endpoints_without_counter_overflow() {
        for k in [-32., -1e-320, 1e-320, 32.] {
            let curve = EnvelopeCurve::exponential(k).unwrap();
            let shape = Envelope::new(u32::MAX, 0, 0, 1., u32::MAX)
                .unwrap()
                .with_curves(curve, curve, curve);
            let mut state = EnvelopeState::new(shape);
            // Independently initialize a far-advanced valid clock/state. Iterating
            // four billion samples is unnecessary to exercise the final transitions.
            state.age = u32::MAX - 127;
            let t = f64::from(state.age) / f64::from(u32::MAX);
            state.progress = if k.abs() < 1e-8 {
                t
            } else {
                (k * t).exp_m1() / k.exp_m1()
            };
            state.delta = if k.abs() < 1e-8 {
                1. / f64::from(u32::MAX)
            } else {
                (k / f64::from(u32::MAX)).exp_m1() * (k * t).exp() / k.exp_m1()
            };
            let mut previous = 0.;
            for _ in 0..128 {
                let level = state.next();
                assert!(level.is_finite() && level >= previous && level <= 1.);
                previous = level;
            }
            assert_eq!(state.phase, Phase::Sustain);
            assert_eq!(state.next(), 1.);
            state.release();
            state.age = u32::MAX - 1;
            state.progress = 1.;
            assert_eq!(state.next(), 0.);
            assert!(state.done());
            assert_eq!(state.remaining(), 0);
            state.choke(100);
            state.release();
            assert!(state.done());
        }
    }
}
