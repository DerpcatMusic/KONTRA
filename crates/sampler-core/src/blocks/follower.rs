//! Envelope follower: smooths an input level with separate attack and release
//! times. Control-rate; exact for an input held across the step.
//! Kontakt's stored follower (0x0A) has no decoded reader or proven law yet.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follower {
    pub attack_ms: f64,
    pub release_ms: f64,
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FollowerState {
    level: f32,
}

impl FollowerState {
    /// Advance `frames` at `rate` Hz towards `input` (magnitude taken).
    pub fn advance(&mut self, f: &Follower, input: f32, frames: u32, rate: f64) -> f32 {
        let target = input.abs() * f.gain;
        let ms = if target > self.level { f.attack_ms } else { f.release_ms };
        let tau = ms * 0.001 * rate;
        let coef = if tau <= 0.0 { 1.0 } else { 1.0 - (-f64::from(frames) / tau).exp() };
        self.level += (target - self.level) * coef as f32;
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rises_with_attack_and_falls_with_release_time_constants() {
        let f = Follower { attack_ms: 10.0, release_ms: 100.0, gain: 1.0 };
        let mut s = FollowerState::default();
        // One attack time constant reaches 1 - 1/e of the step.
        let up = s.advance(&f, -1.0, 480, 48000.0);
        assert!((up - (1.0 - (-1f32).exp())).abs() < 1e-6, "{up}");
        // Splitting the step does not change the result.
        let mut a = FollowerState::default();
        let mut b = FollowerState::default();
        a.advance(&f, 1.0, 480, 48000.0);
        b.advance(&f, 1.0, 240, 48000.0);
        b.advance(&f, 1.0, 240, 48000.0);
        assert!((a.level - b.level).abs() < 1e-6);
        let down = s.advance(&f, 0.0, 4800, 48000.0);
        assert!((down - up * (-1f32).exp()).abs() < 1e-6);
    }

    #[test]
    fn zero_time_follows_instantly_with_gain() {
        let f = Follower { attack_ms: 0.0, release_ms: 0.0, gain: 0.5 };
        let mut s = FollowerState::default();
        assert_eq!(s.advance(&f, 0.8, 32, 48000.0), 0.4);
        assert_eq!(s.advance(&f, 0.0, 32, 48000.0), 0.0);
    }
}
