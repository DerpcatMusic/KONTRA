//! A four-stage resonant wah filter in physical units: a swept band-pass peak,
//! a low-mid contour stage, a mid vocal stage, and a high presence stage, with
//! optional mono summing and smooth per-sample parameter gliding.

use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

use kontra_dsp_core::flush;

/// Settings in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WahSettings {
    pub peak_hz: f32,
    pub peak_q: f32,
    pub peak_gain: f32,
    pub low_hz: f32,
    pub low_gain: f32,
    pub mid_hz: f32,
    pub mid_gain: f32,
    pub high_hz: f32,
    pub high_gain: f32,
    pub mono: bool,
}

/// One mono TPT state-variable stage (two integrator states).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Stage {
    ic1: f64,
    ic2: f64,
}

impl Stage {
    #[inline]
    fn tick(&mut self, x: f64, g: f64, k: f64) -> (f64, f64) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (k * v1, v2)
    }

    fn flush_denormals(&mut self) {
        let mut s = [self.ic1, self.ic2];
        flush(&mut s);
        [self.ic1, self.ic2] = s;
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// Smoothed scalar state for click-free pedal sweeps.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Smooth {
    cur: f64,
    target: f64,
}

impl Smooth {
    fn new(v: f64) -> Self {
        Self { cur: v, target: v }
    }

    #[inline]
    fn step(&mut self, alpha: f64) -> f64 {
        self.cur += alpha * (self.target - self.cur);
        self.cur
    }
}

/// Four-stage resonant wah processor. Zero allocations in `set`/`process`.
#[derive(Clone, Debug, PartialEq)]
pub struct Wah {
    rate: f64,
    alpha: f64,
    peak_hz: Smooth,
    peak_k: Smooth,
    peak_gain: Smooth,
    low_hz: Smooth,
    low_gain: Smooth,
    mid_hz: Smooth,
    mid_gain: Smooth,
    high_hz: Smooth,
    high_gain: Smooth,
    mono: bool,
    stages: [[Stage; 4]; 2],
}

impl Wah {
    /// Creates a new wah filter at `rate`.
    #[must_use]
    pub fn new(s: WahSettings, rate: u32) -> Self {
        let rate_f = f64::from(rate.max(1));
        // ~5 ms exponential smoother for click-free live pedal sweeps.
        let alpha = 1.0 - (-1.0 / (0.005 * rate_f)).exp();
        let (peak_hz, peak_k, peak_gain, low_hz, low_gain, mid_hz, mid_gain, high_hz, high_gain) =
            Self::clamp_params(s, rate_f);
        Self {
            rate: rate_f,
            alpha,
            peak_hz: Smooth::new(peak_hz),
            peak_k: Smooth::new(peak_k),
            peak_gain: Smooth::new(peak_gain),
            low_hz: Smooth::new(low_hz),
            low_gain: Smooth::new(low_gain),
            mid_hz: Smooth::new(mid_hz),
            mid_gain: Smooth::new(mid_gain),
            high_hz: Smooth::new(high_hz),
            high_gain: Smooth::new(high_gain),
            mono: s.mono,
            stages: [[Stage::default(); 4]; 2],
        }
    }

    fn clamp_params(s: WahSettings, rate: f64) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64) {
        let nyq = 0.45 * rate;
        let peak_hz = f64::from(s.peak_hz).clamp(20.0, nyq);
        let peak_q = f64::from(s.peak_q).clamp(0.5, 25.0);
        let peak_k = 1.0 / peak_q;
        let peak_gain = f64::from(s.peak_gain).clamp(-2.0, 12.0);
        let low_hz = f64::from(s.low_hz).clamp(15.0, nyq);
        let low_gain = f64::from(s.low_gain).clamp(-1.0, 2.0);
        let mid_hz = f64::from(s.mid_hz).clamp(20.0, nyq);
        let mid_gain = f64::from(s.mid_gain).clamp(0.0, 2.0);
        let high_hz = f64::from(s.high_hz).clamp(20.0, nyq);
        let high_gain = f64::from(s.high_gain).clamp(0.0, 16.0);
        (
            peak_hz, peak_k, peak_gain, low_hz, low_gain, mid_hz, mid_gain, high_hz, high_gain,
        )
    }

    /// Updates target filter parameters without allocating.
    pub fn set(&mut self, s: WahSettings) {
        let (peak_hz, peak_k, peak_gain, low_hz, low_gain, mid_hz, mid_gain, high_hz, high_gain) =
            Self::clamp_params(s, self.rate);
        self.peak_hz.target = peak_hz;
        self.peak_k.target = peak_k;
        self.peak_gain.target = peak_gain;
        self.low_hz.target = low_hz;
        self.low_gain.target = low_gain;
        self.mid_hz.target = mid_hz;
        self.mid_gain.target = mid_gain;
        self.high_hz.target = high_hz;
        self.high_gain.target = high_gain;
        self.mono = s.mono;
    }

    /// Resets integrator state and snaps smoothers to their current targets.
    pub fn reset(&mut self) {
        for ch in &mut self.stages {
            for st in ch {
                st.reset();
            }
        }
        self.peak_hz.cur = self.peak_hz.target;
        self.peak_k.cur = self.peak_k.target;
        self.peak_gain.cur = self.peak_gain.target;
        self.low_hz.cur = self.low_hz.target;
        self.low_gain.cur = self.low_gain.target;
        self.mid_hz.cur = self.mid_hz.target;
        self.mid_gain.cur = self.mid_gain.target;
        self.high_hz.cur = self.high_hz.target;
        self.high_gain.cur = self.high_gain.target;
    }

    /// Processes stereo audio in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let inv_rate = PI / self.rate;
        let alpha = self.alpha;
        for frame in io {
            let peak_hz = self.peak_hz.step(alpha);
            let peak_k = self.peak_k.step(alpha);
            let peak_gain = self.peak_gain.step(alpha);
            let low_hz = self.low_hz.step(alpha);
            let low_gain = self.low_gain.step(alpha);
            let mid_hz = self.mid_hz.step(alpha);
            let mid_gain = self.mid_gain.step(alpha);
            let high_hz = self.high_hz.step(alpha);
            let high_gain = self.high_gain.step(alpha);

            let g_peak = (peak_hz * inv_rate).tan();
            let g_low = (low_hz * inv_rate).tan();
            let g_mid = (mid_hz * inv_rate).tan();
            let g_high = (high_hz * inv_rate).tan();

            let input = if self.mono {
                let m = 0.5 * (f64::from(frame[0]) + f64::from(frame[1]));
                [m, m]
            } else {
                [f64::from(frame[0]), f64::from(frame[1])]
            };

            let peak_weight = 1.0 + 0.15 * peak_gain;
            for (out, (&x, [s0, s1, s2, s3])) in
                frame.iter_mut().zip(input.iter().zip(&mut self.stages))
            {
                let (bp, _) = s0.tick(x, g_peak, peak_k);
                let (_, lp) = s1.tick(x, g_low, SQRT_2);
                let (mid, _) = s2.tick(x, g_mid, FRAC_1_SQRT_2);
                let (high, _) = s3.tick(x, g_high, 1.0);
                let y = mid_gain
                    * (peak_weight * 4.0 * bp
                        + 0.5 * low_gain * lp
                        + mid
                        + 0.05 * high_gain * high);
                *out = if y.is_finite() { y as f32 } else { 0.0 };
            }
        }
        for ch in &mut self.stages {
            for st in ch {
                st.flush_denormals();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wah(peak_hz: f32, peak_q: f32, mono: bool) -> WahSettings {
        WahSettings {
            peak_hz,
            peak_q,
            peak_gain: 2.0,
            low_hz: 80.0,
            low_gain: 0.2,
            mid_hz: 2000.0,
            mid_gain: 0.2,
            high_hz: 3428.0,
            high_gain: 6.0,
            mono,
        }
    }

    fn tone_rms(s: WahSettings, hz: f32) -> f32 {
        let mut w = Wah::new(s, 48_000);
        let mut io: Vec<[f32; 2]> = (0..16_384)
            .map(|n| [(std::f32::consts::TAU * hz * n as f32 / 48_000.0).sin() * 0.25; 2])
            .collect();
        w.process(&mut io);
        assert!(io.iter().flatten().all(|v| v.is_finite()));
        let sum: f32 = io[8192..].iter().map(|f| f[0] * f[0]).sum();
        (sum / 8192.0).sqrt()
    }

    #[test]
    fn peak_frequency_boosts_on_resonance_and_attenuates_far_off() {
        let heel = wah(350.0, 9.0, false);
        let toe = wah(1600.0, 3.2, false);
        assert!(tone_rms(heel, 350.0) > 3.0 * tone_rms(heel, 1600.0));
        assert!(tone_rms(toe, 1600.0) > 2.0 * tone_rms(toe, 350.0));
    }

    #[test]
    fn mono_mode_sums_channels_and_block_size_is_invariant() {
        let mut w = Wah::new(wah(800.0, 5.0, true), 48_000);
        let mut io: Vec<[f32; 2]> = (0..512).map(|n| [(n as f32 * 0.1).sin(), 0.0]).collect();
        w.process(&mut io);
        for [l, r] in &io {
            assert_eq!(l, r, "mono mode must produce identical L/R");
        }

        let input: Vec<[f32; 2]> = (0..512)
            .map(|n| [(n as f32 * 0.07).sin(), -(n as f32 * 0.05).cos()])
            .collect();
        let mut a = input.clone();
        let mut w1 = Wah::new(wah(900.0, 6.0, false), 48_000);
        w1.process(&mut a);
        let mut b = input;
        let mut w2 = Wah::new(wah(900.0, 6.0, false), 48_000);
        for chunk in b.chunks_mut(13) {
            w2.process(chunk);
        }
        assert_eq!(a, b);
    }
}
