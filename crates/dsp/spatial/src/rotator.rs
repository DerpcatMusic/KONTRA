//! Two-rotor rotary speaker: Linkwitz-Riley-style crossover into horn and drum
//! rotors with independent spin-up/spin-down acceleration inertia, stereo
//! Doppler delay modulation, and directional amplitude modulation.
//!
//! Real-time safe: pre-allocated power-of-two delay lines, zero heap
//! allocations on [`Rotary::process`], and per-sample parameter smoothing.

use std::f32::consts::{PI, TAU};

use moose_dsp::svf::{Svf, SvfCoeffs};

/// Settings for [`Rotary`] in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotarySettings {
    pub fast: bool,
    pub horn_slow_hz: f32,
    pub horn_fast_hz: f32,
    pub drum_slow_hz: f32,
    pub drum_fast_hz: f32,
    pub accel_hi: f32,
    pub accel_lo: f32,
    pub crossover_hz: f32,
    pub balance: f32,
    pub distance: f32,
    pub mix: f32,
}

/// Maximum Doppler delay excursion in milliseconds (covers horn + drum at any rate).
const MAX_DELAY_MS: f32 = 8.0;
/// Base delay centre in milliseconds so Doppler excursion never underflows zero.
const BASE_DELAY_MS: f32 = 3.0;
/// Horn maximum Doppler excursion (ms) at close microphone distance.
const HORN_DOPPLER_MS: f32 = 1.6;
/// Drum maximum Doppler excursion (ms) at close microphone distance.
const DRUM_DOPPLER_MS: f32 = 0.7;
/// Parameter smoothing time constant (seconds).
const SMOOTH_S: f32 = 0.015;

/// Fixed power-of-two mono delay line with linear interpolation and panic-free access.
#[derive(Debug)]
struct DelayLine {
    buf: Box<[f32]>,
    write: usize,
}

impl DelayLine {
    fn new(max_frames: usize) -> Self {
        let len = max_frames.max(16).next_power_of_two();
        Self {
            buf: vec![0.0; len].into_boxed_slice(),
            write: 0,
        }
    }

    #[inline]
    fn push(&mut self, sample: f32) {
        let mask = self.buf.len().saturating_sub(1);
        self.write = (self.write + 1) & mask;
        if let Some(slot) = self.buf.get_mut(self.write) {
            *slot = flush(sample);
        }
    }

    #[inline]
    fn read(&self, delay_frames: f32) -> f32 {
        let len = self.buf.len();
        if len == 0 {
            return 0.0;
        }
        let max_d = (len.saturating_sub(2)) as f32;
        let d = delay_frames.clamp(0.0, max_d);
        let whole = d.floor() as usize;
        let frac = d - whole as f32;
        let mask = len - 1;
        let i0 = self.write.wrapping_sub(whole) & mask;
        let i1 = self.write.wrapping_sub(whole + 1) & mask;
        let s0 = self.buf.get(i0).copied().unwrap_or(0.0);
        let s1 = self.buf.get(i1).copied().unwrap_or(0.0);
        s0 + (s1 - s0) * frac
    }
}

/// Real-time stereo rotary speaker processor.
#[derive(Debug)]
pub struct Rotary {
    s: RotarySettings,
    rate: f32,
    /// Two cascaded Butterworth SVFs form a phase-coherent crossover.
    xover_a: Svf<f32>,
    xover_b: Svf<f32>,
    xover_coeffs: SvfCoeffs<f32>,
    /// Subtle directional horn air low-pass per stereo side.
    horn_lp: [f32; 2],
    /// Delay lines for horn and drum signals.
    horn_line: DelayLine,
    drum_line: DelayLine,
    /// Current angular speeds (Hz) evolving with acceleration inertia.
    horn_hz: f32,
    drum_hz: f32,
    /// Rotor phases in turns `[0, 1)`.
    horn_phase: f64,
    drum_phase: f64,
    /// Per-sample smoothed parameters.
    balance: f32,
    distance: f32,
    mix: f32,
    smooth_coeff: f32,
    written: f32,
}

impl Rotary {
    /// Allocates delay lines and initializes the rotors at their target speed.
    /// Not real-time.
    #[must_use]
    pub fn new(s: RotarySettings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        let max_frames = ((rate_f * 0.001 * MAX_DELAY_MS).ceil() as usize).max(64);
        let horn_hz = if s.fast {
            s.horn_fast_hz
        } else {
            s.horn_slow_hz
        };
        let drum_hz = if s.fast {
            s.drum_fast_hz
        } else {
            s.drum_slow_hz
        };
        let xover_coeffs = xover_svf(s.crossover_hz, rate_f);
        Self {
            s,
            rate: rate_f,
            xover_a: Svf::default(),
            xover_b: Svf::default(),
            xover_coeffs,
            horn_lp: [0.0; 2],
            horn_line: DelayLine::new(max_frames),
            drum_line: DelayLine::new(max_frames),
            horn_hz: horn_hz.max(0.05),
            drum_hz: drum_hz.max(0.05),
            horn_phase: 0.0,
            drum_phase: 0.25,
            balance: s.balance.clamp(0.0, 1.0),
            distance: s.distance.clamp(0.0, 1.0),
            mix: s.mix.clamp(0.0, 1.0),
            smooth_coeff: one_pole_time(SMOOTH_S, rate_f),
            written: 0.0,
        }
    }

    /// Updates target settings without allocation. Real-time.
    pub fn set(&mut self, s: RotarySettings) {
        if (s.crossover_hz - self.s.crossover_hz).abs() > 0.5 {
            self.xover_coeffs = xover_svf(s.crossover_hz, self.rate);
        }
        self.s = s;
    }

    /// Current instantaneous horn rotation frequency (Hz), for telemetry and tests.
    #[must_use]
    pub fn horn_hz(&self) -> f32 {
        self.horn_hz
    }

    /// Current instantaneous drum rotation frequency (Hz), for telemetry and tests.
    #[must_use]
    pub fn drum_hz(&self) -> f32 {
        self.drum_hz
    }

    /// Maximum sample magnitude written into delay state during the last block.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Tail memory in frames.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.horn_line.buf.len()
    }

    /// Advances rotor clocks while silent. Real-time.
    pub fn idle(&mut self, frames: usize) {
        let dt = frames as f32 / self.rate;
        let horn_target = if self.s.fast {
            self.s.horn_fast_hz
        } else {
            self.s.horn_slow_hz
        }
        .max(0.05);
        let drum_target = if self.s.fast {
            self.s.drum_fast_hz
        } else {
            self.s.drum_slow_hz
        }
        .max(0.05);
        let horn_tau = accel_tau(self.s.accel_hi, 0.15, 1.2, self.s.fast);
        let drum_tau = accel_tau(self.s.accel_lo, 0.45, 2.4, self.s.fast);
        self.horn_hz += (horn_target - self.horn_hz) * (1.0 - (-dt / horn_tau).exp());
        self.drum_hz += (drum_target - self.drum_hz) * (1.0 - (-dt / drum_tau).exp());
        self.horn_phase = (self.horn_phase + f64::from(self.horn_hz * dt)).rem_euclid(1.0);
        self.drum_phase = (self.drum_phase + f64::from(self.drum_hz * dt)).rem_euclid(1.0);
        self.written = 0.0;
    }

    /// Processes stereo audio in place. Zero allocations, real-time safe.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let inv_rate = 1.0 / self.rate;
        let inv_rate_f64 = f64::from(inv_rate);
        let ms_to_frames = self.rate * 0.001;
        let base_frames = BASE_DELAY_MS * ms_to_frames;

        let horn_target = if self.s.fast {
            self.s.horn_fast_hz
        } else {
            self.s.horn_slow_hz
        }
        .max(0.05);
        let drum_target = if self.s.fast {
            self.s.drum_fast_hz
        } else {
            self.s.drum_slow_hz
        }
        .max(0.05);

        let horn_tau = accel_tau(self.s.accel_hi, 0.15, 1.2, self.s.fast);
        let drum_tau = accel_tau(self.s.accel_lo, 0.45, 2.4, self.s.fast);
        let horn_alpha = one_pole_time(horn_tau, self.rate);
        let drum_alpha = one_pole_time(drum_tau, self.rate);

        let bal_target = self.s.balance.clamp(0.0, 1.0);
        let dist_target = self.s.distance.clamp(0.0, 1.0);
        let mix_target = self.s.mix.clamp(0.0, 1.0);
        let sm = self.smooth_coeff;

        let mut peak = 0.0f32;

        for frame in io {
            let [dry_l, dry_r] = *frame;
            let mono = 0.5 * (dry_l + dry_r);
            peak = peak.max(mono.abs());

            // Smooth parameters per sample.
            self.balance += (bal_target - self.balance) * sm;
            self.distance += (dist_target - self.distance) * sm;
            self.mix += (mix_target - self.mix) * sm;

            // Rotor angular velocity with physical spin-up/down inertia.
            self.horn_hz += (horn_target - self.horn_hz) * horn_alpha;
            self.drum_hz += (drum_target - self.drum_hz) * drum_alpha;

            self.horn_phase += f64::from(self.horn_hz) * inv_rate_f64;
            if self.horn_phase >= 1.0 {
                self.horn_phase -= 1.0;
            }
            self.drum_phase += f64::from(self.drum_hz) * inv_rate_f64;
            if self.drum_phase >= 1.0 {
                self.drum_phase -= 1.0;
            }

            // 4th-order Linkwitz-Riley crossover (two cascaded Butterworth SVFs).
            let lp1 = self.xover_a.tick(mono, &self.xover_coeffs).low;
            let low = self.xover_b.tick(lp1, &self.xover_coeffs).low;
            let high = mono - low;

            self.horn_line.push(high);
            self.drum_line.push(low);

            // Stereo mic angle narrows as distance grows: close = 90 deg, far = 25 deg.
            let closeness = 1.0 - 0.65 * self.distance;
            let stereo_rad = (0.5 - 0.36 * self.distance) * PI;

            let h_ang = (self.horn_phase as f32) * TAU;
            let d_ang = (self.drum_phase as f32) * TAU;

            // Left/Right mic angles symmetrically placed around 0.
            let h_l = h_ang - 0.5 * stereo_rad;
            let h_r = h_ang + 0.5 * stereo_rad;
            let d_l = d_ang + 0.5 * stereo_rad;
            let d_r = d_ang - 0.5 * stereo_rad;

            // Doppler delay modulation (cosine of angle -> derivative gives sine FM).
            let h_depth = HORN_DOPPLER_MS * closeness * ms_to_frames;
            let d_depth = DRUM_DOPPLER_MS * closeness * ms_to_frames;

            let horn_raw_l = self.horn_line.read(base_frames + h_depth * h_l.cos());
            let horn_raw_r = self.horn_line.read(base_frames + h_depth * h_r.cos());
            let drum_raw_l = self.drum_line.read(base_frames + d_depth * d_l.cos());
            let drum_raw_r = self.drum_line.read(base_frames + d_depth * d_r.cos());

            // Directional AM + mild horn off-axis HF damping.
            let h_front_l = 0.5 + 0.5 * h_l.sin();
            let h_front_r = 0.5 + 0.5 * h_r.sin();
            let h_am_l = 1.0 - closeness * 0.45 * (1.0 - h_front_l);
            let h_am_r = 1.0 - closeness * 0.45 * (1.0 - h_front_r);

            let d_am_l = 1.0 - closeness * 0.28 * (0.5 - 0.5 * d_l.sin());
            let d_am_r = 1.0 - closeness * 0.28 * (0.5 - 0.5 * d_r.sin());

            // Off-axis horn dulling via lightweight 1-pole low-pass modulated by angle.
            let lp_g_l = (0.25 + 0.72 * h_front_l).clamp(0.1, 0.99);
            let lp_g_r = (0.25 + 0.72 * h_front_r).clamp(0.1, 0.99);
            self.horn_lp[0] = flush(self.horn_lp[0] + (horn_raw_l - self.horn_lp[0]) * lp_g_l);
            self.horn_lp[1] = flush(self.horn_lp[1] + (horn_raw_r - self.horn_lp[1]) * lp_g_r);

            // Equal-power-like drum/horn balance (0.5 = 1.0 each).
            let horn_gain = (2.0 * self.balance).min(1.0) * (0.75 + 0.25 * self.balance);
            let drum_gain = (2.0 * (1.0 - self.balance)).min(1.0) * (1.0 - 0.25 * self.balance);

            let wet_l = self.horn_lp[0] * h_am_l * horn_gain + drum_raw_l * d_am_l * drum_gain;
            let wet_r = self.horn_lp[1] * h_am_r * horn_gain + drum_raw_r * d_am_r * drum_gain;

            let m = self.mix;
            *frame = [
                flush(dry_l + (wet_l - dry_l) * m),
                flush(dry_r + (wet_r - dry_r) * m),
            ];
        }
        self.xover_a.flush(1e-20);
        self.xover_b.flush(1e-20);
        self.written = peak;
    }
}

/// Time constant in seconds for rotor spin-up / spin-down inertia.
/// Spin-down is naturally ~1.4x slower than spin-up, matching mechanical friction.
#[inline]
fn accel_tau(accel: f32, fast_s: f32, slow_s: f32, speeding_up: bool) -> f32 {
    let a = accel.clamp(0.0, 1.0);
    let base = slow_s + (fast_s - slow_s) * a;
    if speeding_up { base } else { base * 1.4 }
}

#[inline]
fn one_pole_time(tau_s: f32, rate: f32) -> f32 {
    if tau_s <= 1e-5 {
        1.0
    } else {
        1.0 - (-1.0 / (tau_s * rate)).exp()
    }
}

#[inline]
fn xover_svf(hz: f32, rate: f32) -> SvfCoeffs<f32> {
    SvfCoeffs::new(hz.clamp(80.0, 0.45 * rate), std::f32::consts::SQRT_2, rate)
}

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_settings(fast: bool) -> RotarySettings {
        RotarySettings {
            fast,
            horn_slow_hz: 0.81,
            horn_fast_hz: 6.09,
            drum_slow_hz: 0.68,
            drum_fast_hz: 5.40,
            accel_hi: 0.5,
            accel_lo: 0.5,
            crossover_hz: 800.0,
            balance: 0.5,
            distance: 0.3,
            mix: 1.0,
        }
    }

    #[test]
    fn rotor_inertia_accelerates_and_decelerates_smoothly() {
        let mut r = Rotary::new(default_settings(false), 48_000);
        assert!((r.horn_hz() - 0.81).abs() < 1e-3);
        // Switch to fast and process 0.5s: horn should be mid-ramp and faster than drum.
        r.set(default_settings(true));
        let mut buf = vec![[0.0; 2]; 24_000];
        r.process(&mut buf);
        assert!(
            r.horn_hz() > 1.5 && r.horn_hz() < 5.9,
            "horn mid-ramp: {}",
            r.horn_hz()
        );
        assert!(
            r.drum_hz() > 0.8 && r.drum_hz() < r.horn_hz(),
            "drum heavier than horn: drum={} horn={}",
            r.drum_hz(),
            r.horn_hz()
        );
        // Process 10 more seconds: converges to fast target.
        let mut long = vec![[0.0; 2]; 480_000];
        r.process(&mut long);
        assert!((r.horn_hz() - 6.09).abs() < 0.05, "{}", r.horn_hz());
        assert!((r.drum_hz() - 5.40).abs() < 0.05, "{}", r.drum_hz());
    }

    #[test]
    fn fast_horn_produces_am_and_doppler_modulation_and_stereo_spread() {
        let mut r = Rotary::new(default_settings(true), 48_000);
        let n = 48_000;
        let mut io: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let s = (TAU * 1500.0 * i as f32 / 48_000.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        r.process(&mut io);
        // Measure stereo difference and envelope peak-to-trough modulation.
        let mut diff_energy = 0.0f32;
        let mut env_min = f32::MAX;
        let mut env_max = 0.0f32;
        for chunk in io[4800..].chunks(480) {
            let rms_l =
                (chunk.iter().map(|f| f[0] * f[0]).sum::<f32>() / chunk.len() as f32).sqrt();
            env_min = env_min.min(rms_l);
            env_max = env_max.max(rms_l);
            diff_energy += chunk.iter().map(|f| (f[0] - f[1]).powi(2)).sum::<f32>();
        }
        assert!(
            env_max > env_min * 1.15,
            "AM modulation: min={env_min} max={env_max}"
        );
        assert!(diff_energy > 1.0, "stereo spread: {diff_energy}");
        assert!(io.iter().all(|f| f[0].is_finite() && f[1].is_finite()));
    }
}
