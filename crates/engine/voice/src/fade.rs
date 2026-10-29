//! A voice's fade: a straight gain ramp over its envelope, which keeps
//! running underneath. A stopping fade ends the voice at silence; any other
//! holds where it arrives until the next.

/// The fade gain and where it is going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fade {
    level: f32,
    to: f32,
    step: f32,
    left: u32,
    stop: bool,
}

impl Fade {
    /// Full level, not moving.
    pub(crate) const NONE: Self = Self {
        level: 1.0,
        to: 1.0,
        step: 0.0,
        left: 0,
        stop: false,
    };

    /// Silent, not moving: where a voice that fades in from its start begins.
    pub(crate) const SILENT: Self = Self {
        level: 0.0,
        to: 0.0,
        ..Self::NONE
    };

    /// Ramps from where it is to `to` over `frames` (at least 2), then ends
    /// the voice if `stop`. A stopping fade is final: a later one may only
    /// end it sooner.
    pub(crate) fn start(&mut self, to: f32, frames: u32, stop: bool) {
        let frames = frames.max(2);
        if self.stop && !(stop && frames < self.left) {
            return;
        }
        let to = to.clamp(0.0, 1.0);
        *self = Self {
            to,
            step: (to - self.level) / frames as f32,
            left: frames,
            stop,
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
            self.level = if self.left == 0 {
                self.to
            } else {
                self.level + self.step
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
        fade.start(0.0, 4, true);
        assert_eq!(run(&mut fade, 4), [0.75, 0.5, 0.25, 0.0]);
        assert!(fade.ended());
    }

    #[test]
    fn a_held_fade_comes_back_with_a_fade_in() {
        let mut fade = Fade::NONE;
        fade.start(0.0, 2, false);
        run(&mut fade, 3);
        assert!(!fade.ended());
        assert_eq!(fade.level(), 0.0);
        fade.start(1.0, 4, false);
        assert_eq!(run(&mut fade, 4), [0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn a_stopping_fade_only_ever_ends_sooner() {
        let mut fade = Fade::NONE;
        fade.start(0.0, 10, true);
        fade.start(0.0, 100, true);
        fade.start(1.0, 2, false);
        assert!(run(&mut fade, 10).ends_with(&[0.0]));
        assert!(fade.ended());
        let mut fade = Fade::NONE;
        fade.start(0.0, 100, true);
        fade.start(0.0, 4, true);
        run(&mut fade, 4);
        assert!(fade.ended());
    }
}
