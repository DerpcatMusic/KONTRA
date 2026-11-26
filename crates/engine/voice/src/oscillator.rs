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
        const INV_PERIOD: f64 = 1.0 / OSC_PERIOD;
        let mut phase = self.phase;
        match self.wave {
            OscWave::Sine => {
                for (i, frame) in out.iter_mut().enumerate() {
                    let dt = (step_at(i as u64) * INV_PERIOD).clamp(0.0, 0.49);
                    let sample = fast_sin_unit(phase as f32);
                    *frame = [sample, sample];
                    phase += dt;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
            }
            OscWave::Ramp => {
                for (i, frame) in out.iter_mut().enumerate() {
                    let dt = (step_at(i as u64) * INV_PERIOD).clamp(0.0, 0.49);
                    let t = phase as f32;
                    let sample = (2.0 * t - 1.0) - poly_blep(t, dt as f32);
                    *frame = [sample, sample];
                    phase += dt;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
            }
            OscWave::Saw => {
                for (i, frame) in out.iter_mut().enumerate() {
                    let dt = (step_at(i as u64) * INV_PERIOD).clamp(0.0, 0.49);
                    let t = phase as f32;
                    let sample = (1.0 - 2.0 * t) + poly_blep(t, dt as f32);
                    *frame = [sample, sample];
                    phase += dt;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
            }
            OscWave::Square { width } => {
                let w = width.clamp(0.01, 0.99);
                for (i, frame) in out.iter_mut().enumerate() {
                    let dt = (step_at(i as u64) * INV_PERIOD).clamp(0.0, 0.49);
                    let dt32 = dt as f32;
                    let t = phase as f32;
                    let mut v = if t < w { 1.0 } else { -1.0 };
                    v += poly_blep(t, dt32);
                    let tw = if t >= w { t - w } else { t - w + 1.0 };
                    v -= poly_blep(tw, dt32);
                    *frame = [v, v];
                    phase += dt;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
            }
            OscWave::Triangle => {
                let mut tri = self.tri_state;
                for (i, frame) in out.iter_mut().enumerate() {
                    let dt = (step_at(i as u64) * INV_PERIOD).clamp(0.0, 0.49);
                    let dt32 = dt as f32;
                    let t = phase as f32;
                    let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                    sq += poly_blep(t, dt32);
                    let tw = if t >= 0.5 { t - 0.5 } else { t + 0.5 };
                    sq -= poly_blep(tw, dt32);
                    let leak = (1.0 - dt32).clamp(0.95, 0.9999);
                    tri = (tri * leak + 4.0 * dt32 * sq).clamp(-1.0, 1.0);
                    *frame = [tri, tri];
                    phase += dt;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
                self.tri_state = tri;
            }
            OscWave::Noise => {
                for (i, frame) in out.iter_mut().enumerate() {
                    let dt = (step_at(i as u64) * INV_PERIOD).clamp(0.0, 0.49);
                    let sample = self.next_noise();
                    *frame = [sample, sample];
                    phase += dt;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
            }
        }
        self.phase = phase;
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

/// Quarter-wave Estrin polynomial for `sin(2*pi*t)` with `t in [0, 1)`.
/// Max absolute error `< 3e-7` (`> 130 dB` SFDR).
#[inline(always)]
fn fast_sin_unit(t: f32) -> f32 {
    let mut u = if t >= 0.5 { t - 1.0 } else { t };
    if u > 0.25 {
        u = 0.5 - u;
    } else if u < -0.25 {
        u = -0.5 - u;
    }
    let x = u * TAU;
    let x2 = x * x;
    let x4 = x2 * x2;
    let lo = (-1.0 / 6.0) * x2 + 1.0;
    let mid = (-1.0 / 5_040.0) * x2 + (1.0 / 120.0);
    let hi = (-1.0 / 39_916_800.0) * x2 + (1.0 / 362_880.0);
    x * (lo + x4 * (mid + x4 * hi))
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
