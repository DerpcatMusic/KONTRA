//! Zero-allocation real-time polyBLEP synth oscillator for [`kontra_voice::Voice`]
//! (IR vNext Phase 3).
//!
//! Uses the same [`OSC_PERIOD`] convention as [`crate::wavetable::PERIOD`] so the
//! engine's standard pitch step (`step = freq_hz / sample_rate * OSC_PERIOD`) drives
//! keytracking, pitch bend, glide, vibrato LFOs, and MIDI 2.0 per-note tuning
//! identically to samples and wavetables.

use std::f32::consts::TAU;

/// Cursor units per oscillator cycle (`step / OSC_PERIOD == freq_hz / output_rate`).
pub const OSC_PERIOD: f64 = 64.0;

/// Real-time oscillator waveform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OscWave {
    Sine,
    /// Band-limited falling sawtooth (`1 -> -1`).
    Saw,
    /// Band-limited rising sawtooth (`-1 -> 1`).
    Ramp,
    /// Band-limited pulse wave with duty cycle `width` in `(0.01..=0.99)`.
    Square {
        width: f32,
    },
    /// Integrated band-limited triangle wave.
    Triangle,
    /// Deterministic white noise generator (xorshift64).
    Noise,
}

/// Per-voice inline synth oscillator state (zero heap allocations).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oscillator {
    pub wave: OscWave,
    /// Current phase in `[0.0, 1.0)`.
    pub phase: f64,
    /// Leaky integrator state for band-limited triangle synthesis.
    tri_state: f32,
    /// Deterministic PRNG state for [`OscWave::Noise`].
    seed: u64,
}

impl Oscillator {
    /// Creates an oscillator starting at `phase` cycles (wrapped to `[0, 1)`).
    #[must_use]
    pub fn new(wave: OscWave, phase: f32) -> Self {
        let p = if phase.is_finite() {
            f64::from(phase).rem_euclid(1.0)
        } else {
            0.0
        };
        // Seed mixes the starting phase so voices don't start identically if phase varies,
        // while staying nonzero for xorshift64.
        let seed = 0x9E37_79B9_7F4A_7C15_u64 ^ p.to_bits().rotate_left(17) | 1;
        Self {
            wave,
            phase: p,
            tri_state: 0.0,
            seed,
        }
    }

    /// Renders `out.len()` stereo frames using per-frame voice step `step_at(i)`.
    pub fn render(&mut self, step_at: impl Fn(u64) -> f64, out: &mut [[f32; 2]]) {
        for (i, frame) in out.iter_mut().enumerate() {
            let dt = (step_at(i as u64) / OSC_PERIOD).clamp(0.0, 0.49) as f32;
            let t = self.phase as f32;
            let sample = match self.wave {
                OscWave::Sine => (t * TAU).sin(),
                OscWave::Ramp => {
                    let raw = 2.0 * t - 1.0;
                    raw - poly_blep(t, dt)
                }
                OscWave::Saw => {
                    let raw = 1.0 - 2.0 * t;
                    raw + poly_blep(t, dt)
                }
                OscWave::Square { width } => {
                    let w = width.clamp(0.01, 0.99);
                    let mut v = if t < w { 1.0 } else { -1.0 };
                    v += poly_blep(t, dt);
                    v -= poly_blep((t - w).rem_euclid(1.0), dt);
                    v
                }
                OscWave::Triangle => {
                    // Integrate a 50% band-limited square wave with gentle leak to avoid DC drift.
                    let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                    sq += poly_blep(t, dt);
                    sq -= poly_blep((t - 0.5).rem_euclid(1.0), dt);
                    let leak = (1.0 - dt).clamp(0.95, 0.9999);
                    self.tri_state = (self.tri_state * leak + 4.0 * dt * sq).clamp(-1.0, 1.0);
                    self.tri_state
                }
                OscWave::Noise => self.next_noise(),
            };
            *frame = [sample, sample];
            self.phase = (self.phase + f64::from(dt)).rem_euclid(1.0);
        }
    }

    #[inline]
    fn next_noise(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        // Map top 24 bits to [-1.0, 1.0].
        let unit = ((x >> 40) as f32) * (1.0 / ((1u32 << 24) as f32));
        unit * 2.0 - 1.0
    }
}

/// Second-order polynomial band-limited step residual around a discontinuity at `t = 0`.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_oscillator_hits_target_frequency_and_unit_amplitude() {
        let rate = 48_000.0;
        let hz = 440.0;
        let step = hz / rate * OSC_PERIOD;
        let mut osc = Oscillator::new(OscWave::Sine, 0.0);
        let mut buf = [[0.0; 2]; 4800];
        osc.render(|_| step, &mut buf);

        let peak = buf.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max);
        assert!((peak - 1.0).abs() < 1e-3, "peak {peak}");

        // Count rising zero crossings over 0.1 s (expected 44 cycles).
        let crossings = buf
            .windows(2)
            .filter(|w| w[0][0] <= 0.0 && w[1][0] > 0.0)
            .count();
        assert!((43..=45).contains(&crossings), "crossings {crossings}");
    }

    #[test]
    fn polyblep_waves_and_noise_produce_bounded_non_silent_audio() {
        let step = 440.0 / 48_000.0 * OSC_PERIOD;
        for wave in [
            OscWave::Saw,
            OscWave::Ramp,
            OscWave::Square { width: 0.25 },
            OscWave::Triangle,
            OscWave::Noise,
        ] {
            let mut osc = Oscillator::new(wave, 0.0);
            let mut buf = [[0.0; 2]; 512];
            osc.render(|_| step, &mut buf);
            let peak = buf.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max);
            assert!(peak > 0.2 && peak <= 1.05, "{wave:?}: peak {peak}");
            assert!(buf.iter().all(|f| f[0].is_finite() && f[0] == f[1]));
        }
    }
}
