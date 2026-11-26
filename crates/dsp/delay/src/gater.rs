//! Tempo-synced or free rhythmic amplitude gater with edge shaping, stutter
//! subdivision, internal white-noise injection, and input muting.
//!
//! Real-time safe: zero allocations on [`Gater::process`], per-sample parameter
//! smoothing, and deterministic LCG noise generation.

use crate::slice_buf::{BeatClock, Smoother, flush};

/// Settings for [`Gater`] in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GaterSettings {
    /// Step rate in Hz (already resolved from BPM or tempo sync).
    pub rate_hz: f32,
    /// Gate duty cycle and edge smoothness, `0..=1`.
    pub shape: f32,
    /// Internal white-noise level injected before gating, `0..=1`.
    pub noise: f32,
    /// Wet share of the output, `0..=1`.
    pub mix: f32,
    /// Doubles the step rate for stutter accents.
    pub stutter: bool,
    /// Mutes the audio input into the gate so only gated noise passes.
    pub mute_input: bool,
    /// Engages rhythmic gating (when `false`, gate stays open).
    pub active: bool,
}

const SMOOTH_S: f32 = 0.012;
const NOISE_SCALE: f32 = 4.656_613e-10; // 2^-31

/// Real-time rhythmic stereo gater.
#[derive(Debug)]
pub struct Gater {
    s: GaterSettings,
    rate: f32,
    clock: BeatClock,
    shape: Smoother,
    noise: Smoother,
    mix: Smoother,
    input_gain: Smoother,
    active_mix: Smoother,
    env: f32,
    lcg: u32,
    written: f32,
}

impl Gater {
    /// Initializes the gater. Real-time safe (no heap allocations).
    #[must_use]
    pub fn new(s: GaterSettings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        Self {
            s,
            rate: rate_f,
            clock: BeatClock::new(),
            shape: Smoother::new(s.shape.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            noise: Smoother::new(s.noise.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            mix: Smoother::new(s.mix.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            input_gain: Smoother::new(if s.mute_input { 0.0 } else { 1.0 }, SMOOTH_S, rate_f),
            active_mix: Smoother::new(if s.active { 1.0 } else { 0.0 }, SMOOTH_S, rate_f),
            env: 1.0,
            lcg: 0x1234_5678,
            written: 0.0,
        }
    }

    /// Updates settings without allocation. Real-time.
    pub fn set(&mut self, s: GaterSettings) {
        self.s = s;
    }

    /// Maximum magnitude processed during the last block.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Advances the step clock during silence. Real-time.
    pub fn idle(&mut self, frames: usize) {
        let hz = self.effective_hz();
        self.clock.advance(frames, hz, 1.0 / f64::from(self.rate));
        self.written = 0.0;
    }

    #[inline]
    fn effective_hz(&self) -> f64 {
        let base = f64::from(self.s.rate_hz.clamp(0.1, 100.0));
        if self.s.stutter { base * 2.0 } else { base }
    }

    #[inline]
    fn next_noise(&mut self) -> f32 {
        // Classic 32-bit LCG (x = x * 1103515245 + 12345).
        self.lcg = self.lcg.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        (self.lcg as i32) as f32 * NOISE_SCALE
    }

    /// Processes stereo audio in place. Zero allocations, real-time safe.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let inv_rate = 1.0 / f64::from(self.rate);
        let step_hz = self.effective_hz();
        let shape_t = self.s.shape.clamp(0.0, 1.0);
        let noise_t = self.s.noise.clamp(0.0, 1.0);
        let mix_t = self.s.mix.clamp(0.0, 1.0);
        let in_t = if self.s.mute_input { 0.0 } else { 1.0 };
        let act_t = if self.s.active { 1.0 } else { 0.0 };

        let mut peak = 0.0f32;

        for frame in io {
            let [dry_l, dry_r] = *frame;
            let (phase_f64, _) = self.clock.tick(step_hz, inv_rate);
            let phase = phase_f64 as f32;

            let shape = self.shape.tick(shape_t);
            let noise_amt = self.noise.tick(noise_t);
            let mix = self.mix.tick(mix_t);
            let in_gain = self.input_gain.tick(in_t);
            let act = self.active_mix.tick(act_t);

            // Duty cycle spans 0.15 (sharp staccato) to 0.85 (wide); ramp is
            // placed symmetrically so full-width half-maximum equals `duty`.
            let duty = 0.15 + 0.70 * shape;
            let ramp = (0.03 + 0.12 * shape)
                .min(0.45 * duty)
                .min(0.45 * (1.0 - duty));
            let target_gate = gate_trapezoid(phase, duty, ramp);

            // Slight de-clicking follower on the gate envelope.
            let env_coeff = 1.0 - (-1.0 / (0.002 * self.rate)).exp();
            self.env = flush(self.env + (target_gate - self.env) * env_coeff);
            let gate = 1.0 + (self.env - 1.0) * act;

            let n_l = if noise_amt > 1e-6 {
                self.next_noise() * noise_amt * 0.25
            } else {
                0.0
            };
            let n_r = if noise_amt > 1e-6 {
                self.next_noise() * noise_amt * 0.25
            } else {
                0.0
            };

            let wet_l = (dry_l * in_gain + n_l) * gate;
            let wet_r = (dry_r * in_gain + n_r) * gate;

            let out_l = flush(dry_l + (wet_l - dry_l) * mix);
            let out_r = flush(dry_r + (wet_r - dry_r) * mix);
            peak = peak.max(out_l.abs()).max(out_r.abs());
            *frame = [out_l, out_r];
        }
        self.written = peak;
    }
}

/// Smooth raised-cosine trapezoidal gate on `phase` in `[0, 1)` whose
/// half-amplitude width equals `duty`.
#[inline]
fn gate_trapezoid(phase: f32, duty: f32, ramp: f32) -> f32 {
    let r = ramp.max(1e-4);
    let fall_end = (duty + r).min(1.0);
    if phase < r {
        0.5 * (1.0 - (std::f32::consts::PI * phase / r).cos())
    } else if phase < duty {
        1.0
    } else if phase < fall_end {
        let u = (phase - duty) / (fall_end - duty).max(1e-4);
        0.5 * (1.0 + (std::f32::consts::PI * u).cos())
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gater_enforces_exact_tempo_locked_duty_cycle() {
        // 120 BPM quarter-note steps -> 2 Hz (0.5s period = 24,000 frames at 48 kHz).
        // shape = 0.5 -> duty = 0.50 (12,000 frames open, 12,000 frames closed).
        let mut g = Gater::new(
            GaterSettings {
                rate_hz: 2.0,
                shape: 0.5,
                noise: 0.0,
                mix: 1.0,
                stutter: false,
                mute_input: false,
                active: true,
            },
            48_000,
        );
        let mut io = vec![[1.0f32; 2]; 48_000];
        g.process(&mut io);
        // Second cycle (frames 24_000..48_000): middle of open half (~30_000) is ~1.0,
        // middle of closed half (~42_000) is ~0.0.
        assert!(io[30_000][0] > 0.95, "open {}", io[30_000][0]);
        assert!(io[42_000][0] < 0.02, "closed {}", io[42_000][0]);
        let open_count = io[24_000..48_000].iter().filter(|f| f[0] > 0.5).count();
        let ratio = open_count as f32 / 24_000.0;
        assert!((ratio - 0.5).abs() < 0.05, "duty ratio {ratio}");
    }

    #[test]
    fn stutter_doubles_gater_pulse_rate() {
        let mut g = Gater::new(
            GaterSettings {
                rate_hz: 2.0,
                shape: 0.5,
                noise: 0.0,
                mix: 1.0,
                stutter: true,
                mute_input: false,
                active: true,
            },
            48_000,
        );
        let mut io = vec![[1.0f32; 2]; 48_000];
        g.process(&mut io);
        let rising_edges = io
            .windows(2)
            .filter(|w| w[0][0] <= 0.5 && w[1][0] > 0.5)
            .count();
        // 4 Hz over 1 s -> 3 or 4 rising edges (initial starts near 0).
        assert!((3..=4).contains(&rising_edges), "edges {rising_edges}");
    }
}
