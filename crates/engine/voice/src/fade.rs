//! A voice's fade: a gain ramp along a [`FadeCurve`] over its envelope,
//! which keeps running underneath. A stopping fade ends the voice at
//! silence; any other holds where it arrives until the next.

/// The shape `c(t)` of a fade, `t` 0..=1 of its length. Rising, the gain is
/// `from + (to − from)·c(t)`; falling, `to + (from − to)·c(1 − t)`, so a
/// fade out mirrors the fade in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FadeCurve {
    /// `t`.
    #[default]
    Linear,
    /// `sin(πt/2)`: equal power.
    QuarterSine,
    /// `(1 − cos πt)/2`.
    HalfCosine,
    /// `t²`.
    Square,
    /// `1 − (1 − t)²`.
    InverseSquare,
}

impl FadeCurve {
    #[must_use]
    pub fn at(self, t: f32) -> f32 {
        use std::f32::consts::{FRAC_PI_2, PI};
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::QuarterSine => (FRAC_PI_2 * t).sin(),
            Self::HalfCosine => 0.5 * (1.0 - (PI * t).cos()),
            Self::Square => t * t,
            Self::InverseSquare => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// Where a fade goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FadeTarget {
    /// To silence, where the voice ends.
    End,
    /// To this gain (0 silent, 1 full), where the voice plays on.
    Hold(f32),
}

/// The fade gain and where it is going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fade {
    level: f32,
    from: f32,
    to: f32,
    frames: u32,
    left: u32,
    stop: bool,
    curve: FadeCurve,
}

impl Fade {
    /// Full level, not moving.
    pub(crate) const NONE: Self = Self {
        level: 1.0,
        from: 1.0,
        to: 1.0,
        frames: 0,
        left: 0,
        stop: false,
        curve: FadeCurve::Linear,
    };

    /// Silent, not moving: where a voice that fades in from its start begins.
    pub(crate) const SILENT: Self = Self {
        level: 0.0,
        to: 0.0,
        ..Self::NONE
    };

    /// Ramps from where it is to `target` over `frames` (at least 2) along
    /// `curve`. An ending fade is final: a later one may only end it sooner.
    pub(crate) fn start(&mut self, target: FadeTarget, frames: u32, curve: FadeCurve) {
        let frames = frames.max(2);
        let (to, stop) = match target {
            FadeTarget::End => (0.0, true),
            FadeTarget::Hold(level) if level.is_nan() => return,
            FadeTarget::Hold(level) => (level.clamp(0.0, 1.0), false),
        };
        if self.stop && !(stop && frames < self.left) {
            return;
        }
        *self = Self {
            from: self.level,
            to,
            frames,
            left: frames,
            stop,
            curve,
            ..*self
        };
    }

    pub(crate) fn level(&self) -> f32 {
        self.level
    }

    /// The gain for the next frame.
    pub(crate) fn next(&mut self) -> f32 {
        if self.left > 0 {
            self.left -= 1;
            let t = (self.frames - self.left) as f32 / self.frames as f32;
            self.level = match self.left {
                0 => self.to,
                _ if self.to >= self.from => self.from + (self.to - self.from) * self.curve.at(t),
                _ => self.to + (self.from - self.to) * self.curve.at(1.0 - t),
            };
        }
        self.level
    }

    /// Whether a stopping fade has reached silence.
    pub(crate) fn ended(&self) -> bool {
        self.stop && self.left == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fade: &mut Fade, frames: usize) -> Vec<f32> {
        (0..frames).map(|_| fade.next()).collect()
    }

    #[test]
    fn a_fade_is_a_straight_line_to_its_end() {
        let mut fade = Fade::NONE;
        fade.start(FadeTarget::Hold(f32::NAN), 4, FadeCurve::Linear);
        fade.start(FadeTarget::End, 4, FadeCurve::Linear);
        assert_eq!(run(&mut fade, 4), [0.75, 0.5, 0.25, 0.0]);
        assert!(fade.ended());
    }

    /// Kontakt's curve tables (built at 0x1407f4b4d): an equal-power fade
    /// out is the fade in read backwards, `cos(πt/2)`; an S-curve fade in
    /// is `(1 − cos πt)/2`.
    #[test]
    fn a_curved_fade_follows_its_table() {
        let mut fade = Fade::NONE;
        fade.start(FadeTarget::End, 4, FadeCurve::QuarterSine);
        let out = run(&mut fade, 4);
        for (got, want) in out
            .iter()
            .zip([0.9239, std::f32::consts::FRAC_1_SQRT_2, 0.3827, 0.0])
        {
            assert!((got - want).abs() < 1e-4, "{out:?}");
        }
        fade = Fade::SILENT;
        fade.start(FadeTarget::Hold(1.0), 4, FadeCurve::HalfCosine);
        let out = run(&mut fade, 4);
        for (got, want) in out.iter().zip([0.1464, 0.5, 0.8536, 1.0]) {
            assert!((got - want).abs() < 1e-4, "{out:?}");
        }
    }

    #[test]
    fn a_held_fade_comes_back_with_a_fade_in() {
        let mut fade = Fade::NONE;
        fade.start(FadeTarget::Hold(0.0), 2, FadeCurve::Linear);
        run(&mut fade, 3);
        assert!(!fade.ended());
        assert_eq!(fade.level(), 0.0);
        fade.start(FadeTarget::Hold(1.0), 4, FadeCurve::Linear);
        assert_eq!(run(&mut fade, 4), [0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn a_stopping_fade_only_ever_ends_sooner() {
        let mut fade = Fade::NONE;
        fade.start(FadeTarget::End, 10, FadeCurve::Linear);
        fade.start(FadeTarget::End, 100, FadeCurve::Linear);
        fade.start(FadeTarget::Hold(1.0), 2, FadeCurve::Linear);
        assert!(run(&mut fade, 10).ends_with(&[0.0]));
        assert!(fade.ended());
        let mut fade = Fade::NONE;
        fade.start(FadeTarget::End, 100, FadeCurve::Linear);
        fade.start(FadeTarget::End, 4, FadeCurve::Linear);
        run(&mut fade, 4);
        assert!(fade.ended());
    }
}
