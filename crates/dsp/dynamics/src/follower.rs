//! Adaptive audio-rate envelope follower for modulation sources.
//!
//! Tracks rectified stereo peak amplitude with asymmetric attack/release
//! smoothing, optional adaptive tracking sensitivity, and input gain boost.
//! Zero allocations, fixed-size state, denormal-safe.

use crate::{gain, pole};

/// Physical settings for [`EnvelopeFollower`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Attack time constant in milliseconds (`0..=2000`).
    pub attack_ms: f32,
    /// Release time constant in milliseconds (`0..=5000`).
    pub release_ms: f32,
    /// Adaptive tracking smoothing window in milliseconds (`0` disables adaption).
    pub adaption_ms: f32,
    /// Input gain boost in dB (`-24..=24`).
    pub gain_db: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            attack_ms: 5.0,
            release_ms: 100.0,
            adaption_ms: 0.0,
            gain_db: 0.0,
        }
    }
}

/// Zero-allocation adaptive envelope follower producing a unipolar `0..=1` level.
#[derive(Clone, Copy, Debug)]
pub struct EnvelopeFollower {
    rate: f32,
    attack_c: f32,
    release_c: f32,
    adapt_c: f32,
    input_gain: f32,
    env: f32,
    adapt: f32,
}

#[inline]
fn flush32(x: f32) -> f32 {
    if x.abs() < 1e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

impl EnvelopeFollower {
    /// Builds a new envelope follower for sample rate `rate`.
    #[must_use]
    pub fn new(settings: Settings, rate: u32) -> Self {
        let mut this = Self {
            rate: (rate.max(1)) as f32,
            attack_c: 1.0,
            release_c: 1.0,
            adapt_c: 0.0,
            input_gain: 1.0,
            env: 0.0,
            adapt: 0.0,
        };
        this.set(settings);
        this
    }

    /// Updates physical parameters without allocating or resetting envelope state.
    pub fn set(&mut self, settings: Settings) {
        self.attack_c = pole(settings.attack_ms.clamp(0.0, 2_000.0), self.rate);
        self.release_c = pole(settings.release_ms.clamp(0.0, 5_000.0), self.rate);
        self.adapt_c = if settings.adaption_ms > 0.01 {
            pole(settings.adaption_ms.clamp(0.01, 5_000.0), self.rate)
        } else {
            0.0
        };
        self.input_gain = gain(settings.gain_db.clamp(-24.0, 24.0));
    }

    /// Resets detector state to silence.
    pub fn reset(&mut self) {
        self.env = 0.0;
        self.adapt = 0.0;
    }

    /// Current smoothed envelope value in `0..=1`.
    #[must_use]
    pub fn value(&self) -> f32 {
        self.env.clamp(0.0, 1.0)
    }

    /// Feeds one stereo frame and returns the updated unipolar `0..=1` envelope.
    #[inline]
    pub fn tick(&mut self, frame: [f32; 2]) -> f32 {
        let peak = frame[0].abs().max(frame[1].abs()) * self.input_gain;
        let peak = if peak.is_finite() { peak } else { 0.0 };
        // Adaptive sensitivity: when adaption is enabled, track a slow baseline
        // and boost transients relative to sustained level while keeping 0..=1 bounds.
        let target = if self.adapt_c > 0.0 {
            self.adapt = flush32(self.adapt + (peak - self.adapt) * self.adapt_c);
            let denom = (self.adapt + 0.15).max(0.15);
            (peak / denom).min(1.0)
        } else {
            peak.min(1.0)
        };
        let c = if target > self.env {
            self.attack_c
        } else {
            self.release_c
        };
        self.env = flush32(self.env + (target - self.env) * c).clamp(0.0, 1.0);
        self.env
    }

    /// Feeds a stereo audio block and returns the resulting envelope value.
    pub fn process(&mut self, frames: &[[f32; 2]]) -> f32 {
        for &f in frames {
            self.tick(f);
        }
        self.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_release_and_gain_boost_behave_physically() {
        let mut ef = EnvelopeFollower::new(
            Settings {
                attack_ms: 5.0,
                release_ms: 50.0,
                adaption_ms: 0.0,
                gain_db: 0.0,
            },
            48_000,
        );
        // Silence stays at zero.
        assert_eq!(ef.process(&[[0.0, 0.0]; 256]), 0.0);

        // Step to 0.5 rises monotonically toward 0.5.
        let v1 = ef.process(&[[0.5, 0.5]; 240]); // 5 ms -> ~63% of 0.5
        assert!(v1 > 0.25 && v1 < 0.42, "v1={v1}");
        let v2 = ef.process(&[[0.5, 0.5]; 2400]);
        assert!((v2 - 0.5).abs() < 0.01, "v2={v2}");

        // Release decays smoothly back toward 0.
        let rel = ef.process(&[[0.0, 0.0]; 2400]); // 50 ms -> ~37% of 0.5
        assert!(rel < v2 * 0.5 && rel > 0.1, "rel={rel}");

        // +6 dB gain boost doubles detected level on a 0.25 input.
        let mut boosted = EnvelopeFollower::new(
            Settings {
                attack_ms: 1.0,
                release_ms: 20.0,
                adaption_ms: 0.0,
                gain_db: 6.0206,
            },
            48_000,
        );
        let b = boosted.process(&[[0.25, 0.25]; 1024]);
        assert!((b - 0.5).abs() < 0.02, "boosted={b}");
    }

    #[test]
    fn adaptive_mode_stays_finite_and_tracks_transients() {
        let mut ef = EnvelopeFollower::new(
            Settings {
                attack_ms: 2.0,
                release_ms: 40.0,
                adaption_ms: 80.0,
                gain_db: 0.0,
            },
            48_000,
        );
        let hit = ef.process(&[[0.2, 0.2]; 480]);
        assert!(hit.is_finite() && hit > 0.4 && hit <= 1.0, "hit={hit}");
        let quiet = ef.process(&[[0.0, 0.0]; 12_000]);
        assert!(quiet.is_finite() && quiet < 0.05, "quiet={quiet}");
    }
}
