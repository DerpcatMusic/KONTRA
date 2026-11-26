//! A four-lane staggered bilinear resonant wah filter in physical units, with
//! optional mono summing and smooth per-sample parameter gliding.
//!
//! Lane 0 forms the high-pass first-difference stage prewarped at `high_hz`;
//! lane 1 carries the pass-through feed; lanes 2 and 3 form the resonant
//! second-order wah sections tuned by `low_hz`/`high_hz` and `peak_hz`/`peak_q`,
//! staggered by one sample and blended by `[peak_gain, mid_gain, low_gain, high_gain]`.

use std::f64::consts::{PI, TAU};

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

    #[inline]
    fn snap(&mut self) {
        self.cur = self.target;
    }
}

/// One stereo channel's four-lane staggered bilinear state (`y1`, `y2`, `u`, `v`, `d`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ChannelState {
    y1: [f64; 4],
    y2: [f64; 4],
    u: [f64; 4],
    v: [f64; 4],
    d: [f64; 4],
}

impl ChannelState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn flush_denormals(&mut self) {
        flush(&mut self.y1);
        flush(&mut self.y2);
        flush(&mut self.u);
        flush(&mut self.v);
        flush(&mut self.d);
    }
}

/// Precomputed per-sample coefficients for the four bilinear lanes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Coeffs {
    p: [f64; 4],
    c: [f64; 4],
    fb1: [f64; 4],
    fb2: [f64; 4],
    norm: [f64; 4],
}

/// Four-lane staggered bilinear resonant wah processor. Zero allocations in `set`/`process`.
#[derive(Clone, Debug, PartialEq)]
pub struct Wah {
    rate: f64,
    alpha: f64,
    peak_hz: Smooth,
    peak_q: Smooth,
    peak_gain: Smooth,
    low_hz: Smooth,
    low_gain: Smooth,
    mid_gain: Smooth,
    high_hz: Smooth,
    high_gain: Smooth,
    mono: bool,
    channels: [ChannelState; 2],
}

impl Wah {
    /// Creates a new wah filter at `rate`.
    #[must_use]
    pub fn new(s: WahSettings, rate: u32) -> Self {
        let rate_f = f64::from(rate.max(1));
        // ~2 ms smoother for click-free live pedal sweeps while settling fast.
        let alpha = 1.0 - (-1.0 / (0.002 * rate_f)).exp();
        let (peak_hz, peak_q, peak_gain, low_hz, low_gain, mid_gain, high_hz, high_gain) =
            Self::clamp_params(s, rate_f);
        Self {
            rate: rate_f,
            alpha,
            peak_hz: Smooth::new(peak_hz),
            peak_q: Smooth::new(peak_q),
            peak_gain: Smooth::new(peak_gain),
            low_hz: Smooth::new(low_hz),
            low_gain: Smooth::new(low_gain),
            mid_gain: Smooth::new(mid_gain),
            high_hz: Smooth::new(high_hz),
            high_gain: Smooth::new(high_gain),
            mono: s.mono,
            channels: [ChannelState::default(); 2],
        }
    }

    fn clamp_params(s: WahSettings, rate: f64) -> (f64, f64, f64, f64, f64, f64, f64, f64) {
        let nyq = 0.45 * rate;
        let peak_hz = f64::from(s.peak_hz).clamp(20.0, nyq);
        let peak_q = f64::from(s.peak_q).clamp(0.5, 25.0);
        let peak_gain = f64::from(s.peak_gain).clamp(-2.0, 12.0);
        let low_hz = f64::from(s.low_hz).clamp(0.01, nyq);
        let low_gain = f64::from(s.low_gain).clamp(-2.0, 2.0);
        let mid_gain = f64::from(s.mid_gain).clamp(0.0, 2.0);
        let high_hz = f64::from(s.high_hz).clamp(20.0, nyq);
        let high_gain = f64::from(s.high_gain).clamp(0.0, 16.0);
        (
            peak_hz, peak_q, peak_gain, low_hz, low_gain, mid_gain, high_hz, high_gain,
        )
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn build_coeffs(
        rate: f64,
        peak_hz: f64,
        peak_q: f64,
        peak_gain: f64,
        low_hz: f64,
        low_gain: f64,
        mid_gain: f64,
        high_hz: f64,
        high_gain: f64,
    ) -> Coeffs {
        let w = (high_hz * TAU).clamp(1.0, 0.9 * PI * rate);
        let half_w_t = (0.5 / rate) * w;
        let (sin_w, cos_w) = half_w_t.sin_cos();
        let k = (cos_w / sin_w.max(1e-9)) * w;
        let c0 = k / (k + w);
        let fb1_0 = (k - w) / (k + w);
        let wk = w * k;
        let kk = k * k;
        let ww = w * w;

        let q0 = peak_hz * TAU;
        let q1 = peak_q;
        let q2 = low_hz;
        let q0k = q0 * k;
        let q0_2 = q0 * q0;

        let a0 = (ww + kk) * q2;
        let a1 = (kk + q0_2) * q1;
        let a2 = (ww - kk) * q2;
        let a3 = (q0_2 - kk) * q1;

        Coeffs {
            p: [peak_gain, mid_gain, low_gain, high_gain],
            c: [c0, 0.0, wk, q0k],
            fb1: [fb1_0, 0.0, -2.0 * a2, -2.0 * a3],
            fb2: [0.0, 0.0, wk - a0, q0k - a1],
            norm: [
                1.0,
                1.0,
                1.0 / (a0 + wk).max(1e-12),
                1.0 / (a1 + q0k).max(1e-12),
            ],
        }
    }

    #[inline]
    fn tick_channel(st: &mut ChannelState, cf: &Coeffs, x: f64) -> f64 {
        let [_, u1, u2, u3] = st.u;
        let [_, v1, v2, v3] = st.v;
        let [d0, d1, d2, d3] = st.d;
        let [y10, y11, y12, y13] = st.y1;
        let [y20, y21, y22, y23] = st.y2;
        let [c0, c1, c2, c3] = cf.c;
        let [f10, f11, f12, f13] = cf.fb1;
        let [f20, f21, f22, f23] = cf.fb2;
        let [n0, n1, n2, n3] = cf.norm;
        let [p0, p1, p2, p3] = cf.p;

        let u = [x, u1, u2, u3];
        let v = [x, v1, v2, v3];
        let y = [
            ((x - d0) * c0 + y10 * f10 + y20 * f20) * n0,
            ((u1 - d1) * c1 + y11 * f11 + y21 * f21) * n1,
            ((u2 - d2) * c2 + y12 * f12 + y22 * f22) * n2,
            ((u3 - d3) * c3 + y13 * f13 + y23 * f23) * n3,
        ];
        st.y2 = st.y1;
        st.y1 = y;
        st.d = v;
        st.v = u;
        let u3_scaled = u3 * p1;
        let t0 = p0 * y[0] + x;
        let t1 = p1 * y[1] + u1;
        let t2 = p2 * y[2] + u2;
        let t3 = p3 * y[3] + u3_scaled;
        st.u = [t3, t1, t0, t2];
        t3
    }

    /// Updates target filter parameters without allocating.
    pub fn set(&mut self, s: WahSettings) {
        let (peak_hz, peak_q, peak_gain, low_hz, low_gain, mid_gain, high_hz, high_gain) =
            Self::clamp_params(s, self.rate);
        self.peak_hz.target = peak_hz;
        self.peak_q.target = peak_q;
        self.peak_gain.target = peak_gain;
        self.low_hz.target = low_hz;
        self.low_gain.target = low_gain;
        self.mid_gain.target = mid_gain;
        self.high_hz.target = high_hz;
        self.high_gain.target = high_gain;
        self.mono = s.mono;
    }

    /// Resets filter state and snaps smoothers to their current targets.
    pub fn reset(&mut self) {
        for ch in &mut self.channels {
            ch.reset();
        }
        self.peak_hz.snap();
        self.peak_q.snap();
        self.peak_gain.snap();
        self.low_hz.snap();
        self.low_gain.snap();
        self.mid_gain.snap();
        self.high_hz.snap();
        self.high_gain.snap();
    }

    /// Processes stereo audio in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let rate = self.rate;
        let alpha = self.alpha;
        for frame in io {
            let cf = Self::build_coeffs(
                rate,
                self.peak_hz.step(alpha),
                self.peak_q.step(alpha),
                self.peak_gain.step(alpha),
                self.low_hz.step(alpha),
                self.low_gain.step(alpha),
                self.mid_gain.step(alpha),
                self.high_hz.step(alpha),
                self.high_gain.step(alpha),
            );
            if self.mono {
                let m = 0.5 * (f64::from(frame[0]) + f64::from(frame[1]));
                let y = Self::tick_channel(&mut self.channels[0], &cf, m);
                let out = if y.is_finite() { y as f32 } else { 0.0 };
                *frame = [out, out];
            } else {
                let yl = Self::tick_channel(&mut self.channels[0], &cf, f64::from(frame[0]));
                let yr = Self::tick_channel(&mut self.channels[1], &cf, f64::from(frame[1]));
                *frame = [
                    if yl.is_finite() { yl as f32 } else { 0.0 },
                    if yr.is_finite() { yr as f32 } else { 0.0 },
                ];
            }
        }
        for ch in &mut self.channels {
            ch.flush_denormals();
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
            low_hz: 0.25,
            low_gain: -0.9,
            mid_hz: 2000.0,
            mid_gain: 0.22,
            high_hz: 21_536.875 / std::f32::consts::TAU,
            high_gain: 6.3,
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
        let heel = wah(462.0, 9.78, false);
        let toe = wah(2217.0, 3.19, false);
        assert!(tone_rms(heel, 462.0) > 3.0 * tone_rms(heel, 2217.0));
        assert!(tone_rms(toe, 2217.0) > 2.0 * tone_rms(toe, 462.0));
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
