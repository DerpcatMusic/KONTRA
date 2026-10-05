//! Native linear AHDSR. Durations are frames at the prepared sample rate.
use crate::Error;

/// Validated immutable envelope parameters. Zero-length stages are skipped.
/// Attack starts at zero, decay at one, release at the next held sample's level.
/// Each ramp reaches its endpoint at the exclusive end of its duration.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    attack: u32,
    hold: u32,
    decay: u32,
    sustain: f32,
    release: u32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            attack: 0,
            hold: 0,
            decay: 0,
            sustain: 1.0,
            release: 0,
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
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EnvelopeState {
    shape: Envelope,
    age: u64,
    releasing: Option<(f32, u32)>,
}

impl EnvelopeState {
    pub(super) fn new(shape: Envelope) -> Self {
        Self {
            shape,
            age: 0,
            releasing: None,
        }
    }

    fn held(&self) -> f32 {
        let e = self.shape;
        let mut age = self.age;
        if age < u64::from(e.attack) {
            return (age as f64 / f64::from(e.attack)) as f32;
        }
        age -= u64::from(e.attack);
        if age < u64::from(e.hold) {
            return 1.0;
        }
        age -= u64::from(e.hold);
        if age < u64::from(e.decay) {
            return (1.0 + (f64::from(e.sustain) - 1.0) * age as f64 / f64::from(e.decay)) as f32;
        }
        e.sustain
    }

    /// Repeated cleanup must never restart an already progressing release.
    pub(super) fn release(&mut self) {
        if self.releasing.is_none() {
            self.releasing = Some((self.held(), 0));
        }
    }

    /// Fade from the next sample's level, without extending an existing tail.
    pub(super) fn choke(&mut self, frames: u32) {
        if self.releasing.is_some() && frames as usize >= self.remaining() {
            return;
        }
        let level = match self.releasing {
            Some((level, age)) => {
                (f64::from(level) * (1.0 - f64::from(age) / f64::from(self.shape.release))) as f32
            }
            None => self.held(),
        };
        self.shape.release = frames;
        self.releasing = Some((level, 0));
    }

    pub(super) fn done(&self) -> bool {
        self.releasing
            .is_some_and(|(_, age)| age == self.shape.release)
    }

    pub(super) fn remaining(&self) -> usize {
        self.releasing
            .map_or(usize::MAX, |(_, age)| (self.shape.release - age) as usize)
    }

    pub(super) fn constant_level(&self) -> Option<f32> {
        if self.releasing.is_some() {
            return None;
        }
        if self.shape.attack == 0 && self.shape.sustain == 1.0 {
            return Some(1.0);
        }
        let sustain_at =
            u64::from(self.shape.attack) + u64::from(self.shape.hold) + u64::from(self.shape.decay);
        (self.age >= sustain_at).then_some(self.shape.sustain)
    }

    pub(super) fn next(&mut self) -> f32 {
        if let Some((level, age)) = &mut self.releasing {
            if *age == self.shape.release {
                return 0.0;
            }
            let value = (f64::from(*level)
                * (1.0 - f64::from(*age) / f64::from(self.shape.release)))
                as f32;
            *age += 1; // Strictly below the u32 duration, cannot overflow.
            value
        } else {
            let value = self.held();
            // Keep held notes at a bounded age after reaching sustain.
            let end = u64::from(self.shape.attack)
                + u64::from(self.shape.hold)
                + u64::from(self.shape.decay);
            self.age = (self.age + 1).min(end);
            value
        }
    }
}
