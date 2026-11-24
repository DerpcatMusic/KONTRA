//! Kontakt 8 binary-verified L4 DSP cores:
//! - [`EnhancedCompressor`]: Kontakt Compressor mode 1 (8-segment piecewise curve,
//!   signed-mean stereo link, 2-state attack recurrence, decaying peak hold).
//! - [`SolidBusComp`]: Solid Bus Comp (RMS/peak detector, signed-mean link,
//!   exact `ln(0.8)` time-constant law, dual-stage program-dependent auto-release).
//! - [`SolidPassFilters`]: Solid G-EQ v0x11 3rd-order (18 dB/oct) HP and
//!   2nd-order (12 dB/oct) LP pass stages with verified frequency curves.
//! - [`StereoModeller`]: Kontakt Stereo Modeller width/pan smoothers and
//!   pseudo-stereo 1024-sample right-channel delay ring.
//!
//! All processors are allocation-free in their `process` / `set` paths.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use kontra_ir as ir;

use crate::processors::Wet;

// ---------------------------------------------------------------------------
// 1. Kontakt Enhanced Compressor (mode 1)
// ---------------------------------------------------------------------------

/// Settings for [`EnhancedCompressor`] (Kontakt Compressor mode 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnhancedCompressorSettings {
    /// Threshold in dB (converted via native `2^(db / 6)` law).
    pub threshold_db: f32,
    /// Saved Kontakt ratio parameter (`0..=1`; values `< 0.04` use scale `5.0`).
    pub saved_ratio: f32,
    /// Attack time in milliseconds (`>= 1.0`).
    pub attack_ms: f32,
    /// Release time in milliseconds.
    pub release_ms: f32,
    /// Output makeup gain in dB.
    pub makeup_db: f32,
    /// Stereo link: `true` detects signed channel mean `0.5 * (L + R)`.
    pub link: bool,
    /// Wet/dry mix (`0.0` dry, `1.0` wet).
    pub mix: f32,
}

impl Default for EnhancedCompressorSettings {
    fn default() -> Self {
        Self {
            threshold_db: 0.0,
            saved_ratio: 0.5,
            attack_ms: 1.0,
            release_ms: 100.0,
            makeup_db: 0.0,
            link: true,
            mix: 1.0,
        }
    }
}

/// Native 8-segment piecewise linear curve (`(upper_bound, slope, intercept)`).
const ENHANCED_CURVE: [(f32, f32, f32); 8] = [
    (0.5, 0.46, 0.0),
    (1.207, 0.381_895, 0.039_052_4),
    (2.207, 0.34, 0.089_62),
    (3.621, 0.297_03, 0.184_456),
    (5.621, 0.26, 0.318_54),
    (8.449, 0.233_38, 0.468_168),
    (12.449, 0.215, 0.623_465),
    (f32::INFINITY, 0.198_195, 0.832_671),
];

#[derive(Clone, Copy, Debug, Default)]
struct EnhancedChannel {
    v: f32,
    y: f32,
    hold: f32,
}

impl EnhancedChannel {
    #[inline]
    fn step(&mut self, abs_sample: f32, thr: f32, excess_scale: f32, k: f32, rel: f32) -> f32 {
        let excess = (abs_sample - thr).max(0.0_f32);
        let curve = if excess > 0.0_f32 {
            let scaled = excess * excess_scale;
            let mut out = 0.0_f32;
            for &(upper, slope, intercept) in &ENHANCED_CURVE {
                if scaled <= upper {
                    out = slope * scaled + intercept;
                    break;
                }
            }
            out
        } else {
            0.0_f32
        };
        self.v += (curve - (1.8_f32 * self.v + self.y)) * k;
        self.y += self.v * k + 1e-12_f32;
        self.hold = (self.hold * rel).max(self.y);
        1.0_f32 / (1.0_f32 + self.hold)
    }
}

/// Kontakt 8 Enhanced Compressor (mode 1).
#[derive(Clone, Debug)]
pub struct EnhancedCompressor {
    rate: u32,
    thr: f32,
    excess_scale: f32,
    attack_k: f32,
    release_mul: f32,
    makeup: f32,
    mix: f32,
    link: bool,
    channels: [EnhancedChannel; 2],
}

impl EnhancedCompressor {
    #[must_use]
    pub fn new(settings: EnhancedCompressorSettings, rate: u32) -> Self {
        let mut comp = Self {
            rate: rate.max(1),
            thr: 1.0,
            excess_scale: 0.0,
            attack_k: 0.0,
            release_mul: 1.0,
            makeup: 1.0,
            mix: 1.0,
            link: true,
            channels: [EnhancedChannel::default(); 2],
        };
        comp.configure(settings);
        comp
    }

    /// Resets detector states (`v`, `y`, `hold`) to zero.
    pub fn reset(&mut self) {
        self.channels = [EnhancedChannel::default(); 2];
    }

    /// Updates compressor parameters without allocating or resetting detector state.
    pub fn configure(&mut self, s: EnhancedCompressorSettings) {
        let rate = self.rate as f32;
        let threshold_db = if s.threshold_db.is_finite() {
            s.threshold_db
        } else {
            0.0
        };
        let saved_ratio = if s.saved_ratio.is_finite() {
            s.saved_ratio.max(0.0)
        } else {
            0.5
        };
        let attack_ms = if s.attack_ms.is_finite() {
            s.attack_ms.max(1.0)
        } else {
            1.0
        };
        let release_ms = if s.release_ms.is_finite() {
            s.release_ms.max(0.0)
        } else {
            100.0
        };

        // Native threshold: exp2(saved_dB * f32(1/6)).
        let thr = (threshold_db * (1.0_f32 / 6.0_f32)).exp2();
        let ratio_scale = if saved_ratio < 0.04_f32 {
            5.0_f32
        } else {
            1.0_f32 / (saved_ratio + 0.13_f32) - 0.884_f32
        };
        let attack_mult =
            (1.0_f64 + 0.289_529_450_721_538_1_f64 * f64::from(attack_ms).ln()) as f32;
        let excess_scale = (ratio_scale / thr.max(0.0004_f32)) * attack_mult;
        let attack_k = 2513.2_f32 / (rate * attack_ms);
        let release_mul = if release_ms < 50.0_f32 {
            1.0_f32 - 35.566_f32 / rate
        } else {
            1.0_f32 - 1778.3_f32 / (rate * release_ms)
        }
        .clamp(0.0_f32, 1.0_f32);

        self.thr = thr;
        self.excess_scale = excess_scale;
        self.attack_k = attack_k;
        self.release_mul = release_mul;
        self.makeup = if s.makeup_db.is_finite() && s.makeup_db != 0.0 {
            10.0_f32.powf(s.makeup_db / 20.0_f32)
        } else {
            1.0_f32
        };
        self.mix = if s.mix.is_finite() {
            s.mix.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.link = s.link;
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io.iter_mut() {
            let [l, r] = *frame;
            let [gl, gr] = self.gains(l, r);
            let wl = l * gl * self.makeup;
            let wr = r * gr * self.makeup;
            if self.mix >= 1.0_f32 {
                *frame = [wl, wr];
            } else {
                let dry = 1.0_f32 - self.mix;
                *frame = [l * dry + wl * self.mix, r * dry + wr * self.mix];
            }
        }
    }

    pub fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        for (frame, &[kl, kr]) in io.iter_mut().zip(key.iter()) {
            let [l, r] = *frame;
            let [gl, gr] = self.gains(kl, kr);
            let wl = l * gl * self.makeup;
            let wr = r * gr * self.makeup;
            if self.mix >= 1.0_f32 {
                *frame = [wl, wr];
            } else {
                let dry = 1.0_f32 - self.mix;
                *frame = [l * dry + wl * self.mix, r * dry + wr * self.mix];
            }
        }
    }

    #[inline]
    fn gains(&mut self, kl: f32, kr: f32) -> [f32; 2] {
        if self.link {
            let det = (0.5_f32 * (kl + kr)).abs();
            let g = self.channels[0].step(
                det,
                self.thr,
                self.excess_scale,
                self.attack_k,
                self.release_mul,
            );
            [g, g]
        } else {
            let gl = self.channels[0].step(
                kl.abs(),
                self.thr,
                self.excess_scale,
                self.attack_k,
                self.release_mul,
            );
            let gr = self.channels[1].step(
                kr.abs(),
                self.thr,
                self.excess_scale,
                self.attack_k,
                self.release_mul,
            );
            [gl, gr]
        }
    }
}

impl Wet for EnhancedCompressor {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        EnhancedCompressor::process(self, io);
    }

    fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        EnhancedCompressor::process_sidechain(self, io, key);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Compressor(c) = kind {
            // Invert the mode-1 translator's `ratio = 1 / saved_ratio` mapping.
            let saved_ratio = (1.0 / c.ratio.max(1.0)).clamp(0.0, 1.0);
            self.configure(EnhancedCompressorSettings {
                threshold_db: c.threshold_db,
                saved_ratio,
                attack_ms: c.attack_ms,
                release_ms: c.release_ms,
                makeup_db: c.makeup(),
                link: c.link,
                mix: c.mix,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Kontakt Solid Bus Comp (detector + dual-stage auto-release)
// ---------------------------------------------------------------------------

/// Verified Solid Bus Comp attack step times in milliseconds (`enum 0..=5`).
pub const SOLID_BUS_ATTACK_MS: [f32; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];

/// Verified Solid Bus Comp release step times in milliseconds (`enum 0..=5`);
/// index 5 selects program-dependent auto-release.
pub const SOLID_BUS_RELEASE_MS: [f32; 6] = [12.5, 18.75, 37.5, 50.0, 100.0, 75.0];

/// Verified Solid Bus Comp one-pole decay coefficient `min(1, exp(ln(0.8) / (ms * 0.001 * rate)))`.
#[must_use]
pub fn solid_bus_coeff(ms: f32, rate: u32) -> f32 {
    let denom = f64::from(ms.max(1e-4)) * 0.001 * f64::from(rate.max(1));
    ((0.8_f64.ln()) / denom).exp().min(1.0) as f32
}

/// Settings for [`SolidBusComp`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidBusCompSettings {
    /// Threshold in dB (`-20..=+20` typical).
    pub threshold_db: f32,
    /// Compression ratio (`>= 1.0`, e.g. `2.0`, `4.0`, `10.0`).
    pub ratio: f32,
    /// Attack time in milliseconds (e.g. from [`SOLID_BUS_ATTACK_MS`]).
    pub attack_ms: f32,
    /// Manual release time in milliseconds (e.g. from [`SOLID_BUS_RELEASE_MS`]).
    pub release_ms: f32,
    /// Enables dual-stage program-dependent auto-release (fast transient + slow sustained).
    pub auto_release: bool,
    /// Output makeup gain in dB.
    pub makeup_db: f32,
    /// Stereo link: `true` detects signed mean `0.5 * (L + R)`.
    pub link: bool,
    /// Wet/dry mix (`0.0..=1.0`).
    pub mix: f32,
}

impl Default for SolidBusCompSettings {
    fn default() -> Self {
        Self {
            threshold_db: 0.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            auto_release: false,
            makeup_db: 0.0,
            link: true,
            mix: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BusDetector {
    /// Short-window RMS energy state.
    rms_sq: f32,
    /// Primary (or fast auto-release) gain-reduction envelope in dB (`>= 0`).
    env_db: f32,
    /// Slow program-dependent gain-reduction envelope in dB (`>= 0`, auto mode).
    slow_db: f32,
}

/// Kontakt Solid Bus Comp with RMS/peak detector and dual-stage auto-release.
#[derive(Clone, Debug)]
pub struct SolidBusComp {
    rate: u32,
    threshold_db: f32,
    slope: f32,
    attack_coef: f32,
    release_coef: f32,
    rms_coef: f32,
    auto_fast_rel: f32,
    auto_slow_att: f32,
    auto_slow_rel: f32,
    auto_release: bool,
    makeup: f32,
    mix: f32,
    link: bool,
    det: [BusDetector; 2],
}

impl SolidBusComp {
    #[must_use]
    pub fn new(settings: SolidBusCompSettings, rate: u32) -> Self {
        let rate = rate.max(1);
        let mut comp = Self {
            rate,
            threshold_db: 0.0,
            slope: 0.75,
            attack_coef: 0.0,
            release_coef: 0.0,
            rms_coef: solid_bus_coeff(2.5, rate),
            // `solid_bus_coeff` uses `ln(0.8)` (~4.48x slower than `e^-1`), so
            // 25 / 80 / 280 ms map to ~110 ms fast release, ~360 ms slow charge,
            // and ~1.25 s slow program-dependent release.
            auto_fast_rel: solid_bus_coeff(25.0, rate),
            auto_slow_att: solid_bus_coeff(80.0, rate),
            auto_slow_rel: solid_bus_coeff(280.0, rate),
            auto_release: false,
            makeup: 1.0,
            mix: 1.0,
            link: true,
            det: [BusDetector::default(); 2],
        };
        comp.configure(settings);
        comp
    }

    pub fn reset(&mut self) {
        self.det = [BusDetector::default(); 2];
    }

    pub fn configure(&mut self, s: SolidBusCompSettings) {
        let ratio = if s.ratio.is_finite() {
            s.ratio.max(1.0)
        } else {
            1.0
        };
        self.threshold_db = if s.threshold_db.is_finite() {
            s.threshold_db
        } else {
            0.0
        };
        self.slope = 1.0 - 1.0 / ratio;
        let att_ms = if s.attack_ms.is_finite() {
            s.attack_ms.max(0.05)
        } else {
            10.0
        };
        let rel_ms = if s.release_ms.is_finite() {
            s.release_ms.max(1.0)
        } else {
            100.0
        };
        self.attack_coef = solid_bus_coeff(att_ms, self.rate);
        self.release_coef = solid_bus_coeff(rel_ms, self.rate);
        if self.auto_release != s.auto_release {
            for d in &mut self.det {
                d.slow_db = 0.0;
            }
            self.auto_release = s.auto_release;
        }
        self.makeup = if s.makeup_db.is_finite() && s.makeup_db != 0.0 {
            10.0_f32.powf(s.makeup_db / 20.0_f32)
        } else {
            1.0
        };
        self.mix = if s.mix.is_finite() {
            s.mix.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.link = s.link;
    }

    /// Current effective gain reduction in dB for channel `ch` (`0` or `1`).
    #[must_use]
    pub fn gain_reduction_db(&self, ch: usize) -> f32 {
        if ch >= self.det.len() {
            return 0.0;
        }
        let idx = if self.link { 0 } else { ch };
        let Some(d) = self.det.get(idx) else {
            return 0.0;
        };
        if self.auto_release {
            d.env_db.max(d.slow_db)
        } else {
            d.env_db
        }
    }

    #[inline]
    fn step_channel(
        d: &mut BusDetector,
        x: f32,
        params: &BusStepParams,
        auto: Option<(f32, f32, f32)>,
    ) -> f32 {
        // Hybrid RMS/peak level detection: tracks RMS energy with instant peak sensitivity.
        let x2 = x * x;
        d.rms_sq = params.rms_coef * d.rms_sq + (1.0 - params.rms_coef) * x2;
        let rms = d.rms_sq.max(0.0).sqrt();
        let peak = x.abs();
        let level = (0.5 * peak + 0.5 * rms).max(1e-12);
        let level_db = 20.0 * level.log10();

        // Soft 2 dB knee typical of bus VCA dynamics.
        const KNEE_DB: f32 = 2.0;
        let over = level_db - params.threshold_db;
        let target_gr = if over <= -KNEE_DB * 0.5 {
            0.0
        } else if over >= KNEE_DB * 0.5 {
            over * params.slope
        } else {
            let t = over + KNEE_DB * 0.5;
            params.slope * (t * t) / (2.0 * KNEE_DB)
        };

        let gr_db = if let Some((fast_rel, slow_att, slow_rel)) = auto {
            let c_fast = if target_gr > d.env_db {
                params.attack_coef
            } else {
                fast_rel
            };
            d.env_db = c_fast * d.env_db + (1.0 - c_fast) * target_gr;

            let c_slow = if target_gr > d.slow_db {
                slow_att
            } else {
                slow_rel
            };
            d.slow_db = c_slow * d.slow_db + (1.0 - c_slow) * target_gr;
            d.env_db.max(d.slow_db)
        } else {
            let c = if target_gr > d.env_db {
                params.attack_coef
            } else {
                params.release_coef
            };
            d.env_db = c * d.env_db + (1.0 - c) * target_gr;
            d.env_db
        };

        10.0_f32.powf(-gr_db / 20.0_f32)
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let auto = self.auto_release.then_some((
            self.auto_fast_rel,
            self.auto_slow_att,
            self.auto_slow_rel,
        ));
        for frame in io.iter_mut() {
            let [l, r] = *frame;
            let [gl, gr] = self.gains(l, r, auto);
            let wl = l * gl * self.makeup;
            let wr = r * gr * self.makeup;
            if self.mix >= 1.0 {
                *frame = [wl, wr];
            } else {
                let dry = 1.0 - self.mix;
                *frame = [l * dry + wl * self.mix, r * dry + wr * self.mix];
            }
        }
    }

    pub fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        let auto = self.auto_release.then_some((
            self.auto_fast_rel,
            self.auto_slow_att,
            self.auto_slow_rel,
        ));
        for (frame, &[kl, kr]) in io.iter_mut().zip(key.iter()) {
            let [l, r] = *frame;
            let [gl, gr] = self.gains(kl, kr, auto);
            let wl = l * gl * self.makeup;
            let wr = r * gr * self.makeup;
            if self.mix >= 1.0 {
                *frame = [wl, wr];
            } else {
                let dry = 1.0 - self.mix;
                *frame = [l * dry + wl * self.mix, r * dry + wr * self.mix];
            }
        }
    }

    #[inline]
    fn gains(&mut self, kl: f32, kr: f32, auto: Option<(f32, f32, f32)>) -> [f32; 2] {
        let params = BusStepParams {
            threshold_db: self.threshold_db,
            slope: self.slope,
            rms_coef: self.rms_coef,
            attack_coef: self.attack_coef,
            release_coef: self.release_coef,
        };
        if self.link {
            let g = Self::step_channel(&mut self.det[0], 0.5 * (kl + kr), &params, auto);
            [g, g]
        } else {
            let gl = Self::step_channel(&mut self.det[0], kl, &params, auto);
            let gr = Self::step_channel(&mut self.det[1], kr, &params, auto);
            [gl, gr]
        }
    }
}

#[derive(Clone, Copy)]
struct BusStepParams {
    threshold_db: f32,
    slope: f32,
    rms_coef: f32,
    attack_coef: f32,
    release_coef: f32,
}

impl Wet for SolidBusComp {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        SolidBusComp::process(self, io);
    }

    fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        SolidBusComp::process_sidechain(self, io, key);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Compressor(c) = kind {
            self.configure(SolidBusCompSettings {
                threshold_db: c.threshold_db,
                ratio: c.ratio,
                attack_ms: c.attack_ms,
                release_ms: c.release_ms,
                auto_release: self.auto_release,
                makeup_db: c.makeup(),
                link: c.link,
                mix: c.mix,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Solid G-EQ v0x11 HP (18 dB/oct, 3rd-order) & LP (12 dB/oct, 2nd-order)
// ---------------------------------------------------------------------------

/// Verified Solid G-EQ v0x11 HP corner frequencies (Hz) at normalized knob
/// positions `[0.0, 0.2, 0.4, 0.6, 0.8, 1.0]`, derived from `solid-geq-hplp.tsv`.
const SOLID_HP_HZ_KNOTS: [f64; 6] = [16.5, 24.0, 59.5, 103.5, 286.0, 462.0];

/// Verified Solid G-EQ v0x11 LP corner frequencies (Hz) at normalized knob
/// positions `[0.0, 0.2, 0.4, 0.6, 0.8, 1.0]`, derived from `solid-geq-hplp.tsv`.
const SOLID_LP_HZ_KNOTS: [f64; 6] = [21_800.0, 19_200.0, 15_200.0, 10_300.0, 4_820.0, 3_270.0];

fn interp_log_knots(knots: &[f64; 6], norm: f32) -> f64 {
    let x = if norm.is_finite() {
        f64::from(norm.clamp(0.0, 1.0)) * 5.0
    } else {
        0.0
    };
    let i = (x.floor() as usize).min(4);
    let frac = (x - i as f64).clamp(0.0, 1.0);
    let lo = knots.get(i).copied().unwrap_or(1000.0);
    let hi = knots.get(i + 1).copied().unwrap_or(lo);
    (lo.ln() + (hi.ln() - lo.ln()) * frac).exp()
}

/// Solid G-EQ v0x11 HP cutoff frequency in Hz from normalized `0..=1` knob.
#[must_use]
pub fn solid_geq_hp_hz(norm: f32) -> f32 {
    interp_log_knots(&SOLID_HP_HZ_KNOTS, norm) as f32
}

/// Solid G-EQ v0x11 LP cutoff frequency in Hz from normalized `0..=1` knob.
#[must_use]
pub fn solid_geq_lp_hz(norm: f32) -> f32 {
    interp_log_knots(&SOLID_LP_HZ_KNOTS, norm) as f32
}

/// Settings for [`SolidPassFilters`] (Solid G-EQ v0x11 HP/LP stages).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidPassSettings {
    pub hp_on: bool,
    pub hp_hz: f32,
    pub lp_on: bool,
    pub lp_hz: f32,
}

impl SolidPassSettings {
    /// Builds pass filter settings from Kontakt Solid G-EQ v0x11 normalized knobs.
    #[must_use]
    pub fn from_normalized(hp_on: bool, hp_norm: f32, lp_on: bool, lp_norm: f32) -> Self {
        Self {
            hp_on,
            hp_hz: solid_geq_hp_hz(hp_norm),
            lp_on,
            lp_hz: solid_geq_lp_hz(lp_norm),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct StereoBiquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    s1: [f64; 2],
    s2: [f64; 2],
}

impl StereoBiquad {
    const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
        s1: [0.0; 2],
        s2: [0.0; 2],
    };

    fn clamp_cutoff(hz: f32, fs: f64) -> f64 {
        let hi = 0.49 * fs;
        let lo = 1.0_f64.min(hi);
        f64::from(hz).clamp(lo, hi)
    }

    fn set_first_order_hp(&mut self, hz: f32, rate: u32) {
        let fs = f64::from(rate.max(1));
        let fc = Self::clamp_cutoff(hz, fs);
        let k = (PI * fc / fs).tan();
        let norm = 1.0 / (1.0 + k);
        self.b0 = norm;
        self.b1 = -norm;
        self.b2 = 0.0;
        self.a1 = (k - 1.0) * norm;
        self.a2 = 0.0;
    }

    fn set_second_order_hp(&mut self, hz: f32, q: f64, rate: u32) {
        let fs = f64::from(rate.max(1));
        let fc = Self::clamp_cutoff(hz, fs);
        let k = (PI * fc / fs).tan();
        let k2 = k * k;
        let norm = 1.0 / (1.0 + k / q + k2);
        self.b0 = norm;
        self.b1 = -2.0 * norm;
        self.b2 = norm;
        self.a1 = 2.0 * (k2 - 1.0) * norm;
        self.a2 = (1.0 - k / q + k2) * norm;
    }

    /// Analog-matched 2nd-order Butterworth low-pass (Vicanek magnitude match at
    /// DC, cutoff, and Nyquist) so high-frequency rolloff avoids bilinear Nyquist
    /// cramping and tracks Kontakt Solid G-EQ's measured `solid-geq-hplp.tsv`.
    fn set_second_order_lp(&mut self, hz: f32, q: f64, rate: u32) {
        let fs = f64::from(rate.max(1));
        let fc = Self::clamp_cutoff(hz, fs);
        let w0 = 2.0 * PI * fc / fs;
        let inv_2q = 0.5 / q;
        let r = (-inv_2q * w0).exp();
        let a1 = if inv_2q <= 1.0 {
            -2.0 * r * ((1.0 - inv_2q * inv_2q).sqrt() * w0).cos()
        } else {
            -2.0 * r * ((inv_2q * inv_2q - 1.0).sqrt() * w0).cosh()
        };
        let a2 = r * r;

        let big_a0 = (1.0 + a1 + a2).powi(2);
        let big_a1 = (1.0 - a1 + a2).powi(2);
        let big_a2 = -4.0 * a2;

        let phi1 = (0.5 * w0).sin().powi(2);
        let phi0 = 1.0 - phi1;
        let phi2 = 4.0 * phi0 * phi1;

        // Analog 2nd-order lowpass squared magnitude: 1 / ((1 - r^2)^2 + (r/Q)^2).
        let nyq_ratio = PI / w0;
        let nyq_mag2 = 1.0 / ((1.0 - nyq_ratio * nyq_ratio).powi(2) + (nyq_ratio / q).powi(2));
        let cut_mag2 = q * q; // At w = w0, |H(jw0)|^2 = Q^2 (= 0.5 for Butterworth).

        let big_b0 = big_a0;
        let big_b1 = big_a1 * nyq_mag2;
        let d_w0 = big_a0 * phi0 + big_a1 * phi1 + big_a2 * phi2;
        let big_b2 = if phi2 > 1e-12 {
            (cut_mag2 * d_w0 - big_b0 * phi0 - big_b1 * phi1) / phi2
        } else {
            0.0
        };

        let sqrt_b0 = big_b0.max(0.0).sqrt();
        let sqrt_b1 = big_b1.max(0.0).sqrt();
        let s = 0.5 * (sqrt_b0 + sqrt_b1);
        let b1 = 0.5 * (sqrt_b0 - sqrt_b1);
        let disc = (s * s + big_b2).max(0.0).sqrt();
        let b0 = 0.5 * (s + disc);
        let b2 = s - b0;

        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }

    fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io.iter_mut() {
            for ((sample, s1), s2) in frame
                .iter_mut()
                .zip(self.s1.iter_mut())
                .zip(self.s2.iter_mut())
            {
                let x = f64::from(*sample);
                let y = self.b0 * x + *s1;
                *s1 = self.b1 * x - self.a1 * y + *s2;
                *s2 = self.b2 * x - self.a2 * y;
                *sample = y as f32;
            }
        }
    }

    fn is_silent(&self) -> bool {
        self.s1.iter().chain(&self.s2).all(|v| v.abs() < 1e-12)
    }

    fn reset(&mut self) {
        self.s1 = [0.0; 2];
        self.s2 = [0.0; 2];
    }
}

/// Solid G-EQ v0x11 pass filter bank: 3rd-order (18 dB/oct Butterworth) HP
/// cascaded with 2nd-order (12 dB/oct Butterworth) LP.
#[derive(Clone, Debug)]
pub struct SolidPassFilters {
    rate: u32,
    hp_on: bool,
    lp_on: bool,
    hp1: StereoBiquad,
    hp2: StereoBiquad,
    lp: StereoBiquad,
}

impl SolidPassFilters {
    #[must_use]
    pub fn new(settings: SolidPassSettings, rate: u32) -> Self {
        let mut f = Self {
            rate: rate.max(1),
            hp_on: false,
            lp_on: false,
            hp1: StereoBiquad::IDENTITY,
            hp2: StereoBiquad::IDENTITY,
            lp: StereoBiquad::IDENTITY,
        };
        f.configure(settings);
        f
    }

    pub fn reset(&mut self) {
        self.hp1.reset();
        self.hp2.reset();
        self.lp.reset();
    }

    pub fn configure(&mut self, s: SolidPassSettings) {
        self.hp_on = s.hp_on;
        self.lp_on = s.lp_on;
        if s.hp_on && s.hp_hz.is_finite() {
            // 3rd-order Butterworth high-pass = 1st-order + 2nd-order (Q = 1.0).
            self.hp1.set_first_order_hp(s.hp_hz, self.rate);
            self.hp2.set_second_order_hp(s.hp_hz, 1.0, self.rate);
        }
        if s.lp_on && s.lp_hz.is_finite() {
            // 2nd-order Butterworth low-pass (Q = 1 / sqrt(2)).
            self.lp
                .set_second_order_lp(s.lp_hz, FRAC_1_SQRT_2, self.rate);
        }
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if self.hp_on {
            self.hp1.process(io);
            self.hp2.process(io);
        }
        if self.lp_on {
            self.lp.process(io);
        }
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        (!self.hp_on || (self.hp1.is_silent() && self.hp2.is_silent()))
            && (!self.lp_on || self.lp.is_silent())
    }
}

impl Wet for SolidPassFilters {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        SolidPassFilters::process(self, io);
    }
}

// ---------------------------------------------------------------------------
// 4. Kontakt Stereo Modeller (width/pan smoothers + pseudo-stereo delay ring)
// ---------------------------------------------------------------------------

/// Ring buffer length for Kontakt Stereo Modeller pseudo-stereo delay (1024 floats).
const PSEUDO_RING_LEN: usize = 1024;
const PSEUDO_RING_MASK: usize = PSEUDO_RING_LEN - 1;
const WIDTH_SMOOTH_K: f32 = 1.0 / 180.0;
const PAN_SMOOTH_K: f32 = 1.0 / 1800.0;

/// Settings for [`StereoModeller`] (Kontakt module `0x1f`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoModellerSettings {
    /// Width / spread (`0.0` mono, `0.5` identity, `1.0` full spread).
    pub width: f32,
    /// Balance pan (`-1.0` left, `0.0` center, `1.0` right).
    pub pan: f32,
    /// Pseudo-stereo mode: delays right channel by `min(1023, trunc(sr * 0.01 * w^3))`.
    pub pseudo: bool,
}

impl Default for StereoModellerSettings {
    fn default() -> Self {
        Self {
            width: 0.5,
            pan: 0.0,
            pseudo: false,
        }
    }
}

/// Computes Kontakt's exact pseudo-stereo right-channel delay in frames:
/// `min(1023, trunc(rate * 0.01 * width^3))`.
#[must_use]
pub fn pseudo_delay_frames(width: f32, rate: u32) -> usize {
    if !width.is_finite() || width <= 0.0 {
        return 0;
    }
    let w = width.clamp(0.0, 1.0);
    let raw = (rate as f32 * 0.01_f32 * w * w * w).trunc();
    (raw.max(0.0) as usize).min(PSEUDO_RING_MASK)
}

/// Kontakt Stereo Modeller with width/pan one-pole smoothers and pseudo-stereo
/// 1024-sample zero-initialized right-channel delay ring.
#[derive(Clone, Debug)]
pub struct StereoModeller {
    rate: u32,
    width_cur: f32,
    width_target: f32,
    pan_cur: f32,
    pan_target: f32,
    pseudo: bool,
    ring: Box<[f32; PSEUDO_RING_LEN]>,
    write: usize,
    written: f32,
}

impl StereoModeller {
    #[must_use]
    pub fn new(settings: StereoModellerSettings, rate: u32) -> Self {
        let w = if settings.width.is_finite() {
            settings.width.clamp(0.0, 1.0)
        } else {
            0.5
        };
        let p = if settings.pan.is_finite() {
            settings.pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        Self {
            rate: rate.max(1),
            width_cur: w,
            width_target: w,
            pan_cur: p,
            pan_target: p,
            pseudo: settings.pseudo,
            ring: Box::new([0.0_f32; PSEUDO_RING_LEN]),
            write: 0,
            written: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.write = 0;
        self.written = 0.0;
        self.width_cur = self.width_target;
        self.pan_cur = self.pan_target;
    }

    pub fn configure(&mut self, settings: StereoModellerSettings) {
        if settings.width.is_finite() {
            self.width_target = settings.width.clamp(0.0, 1.0);
        }
        if settings.pan.is_finite() {
            self.pan_target = settings.pan.clamp(-1.0, 1.0);
        }
        if !self.pseudo && settings.pseudo {
            self.ring.fill(0.0);
            self.write = 0;
            self.written = 0.0;
        }
        self.pseudo = settings.pseudo;
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let mut peak = 0.0_f32;
        for frame in io.iter_mut() {
            self.width_cur += (self.width_target - self.width_cur) * WIDTH_SMOOTH_K;
            self.pan_cur += (self.pan_target - self.pan_cur) * PAN_SMOOTH_K;
            let [l_in, r_in] = *frame;
            let (l, r) = if self.pseudo {
                if let Some(slot) = self.ring.get_mut(self.write) {
                    *slot = r_in;
                }
                peak = peak.max(r_in.abs());
                let delay = pseudo_delay_frames(self.width_cur, self.rate);
                let read = self.write.wrapping_sub(delay) & PSEUDO_RING_MASK;
                let r_del = self.ring.get(read).copied().unwrap_or(0.0);
                self.write = (self.write + 1) & PSEUDO_RING_MASK;
                (l_in, r_del)
            } else if self.width_cur >= 0.5_f32 {
                let s = 2.0_f32 * self.width_cur - 1.0_f32;
                (
                    (1.0_f32 + s) * l_in - s * r_in,
                    (1.0_f32 + s) * r_in - s * l_in,
                )
            } else {
                let a = 0.5_f32 - self.width_cur;
                (l_in + a * (r_in - l_in), r_in + a * (l_in - r_in))
            };
            let lg = 1.0_f32 - self.pan_cur.max(0.0_f32);
            let rg = 1.0_f32 + self.pan_cur.min(0.0_f32);
            *frame = [l * lg, r * rg];
        }
        self.written = peak;
    }
}

impl Wet for StereoModeller {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        StereoModeller::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Stereo { width, pan } = kind {
            self.configure(StereoModellerSettings {
                width: *width,
                pan: *pan,
                pseudo: self.pseudo,
            });
        }
    }

    fn memory(&self) -> Option<usize> {
        Some(PSEUDO_RING_LEN)
    }

    fn written(&self) -> f32 {
        self.written
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        for _ in 0..silence.len() {
            self.width_cur += (self.width_target - self.width_cur) * WIDTH_SMOOTH_K;
            self.pan_cur += (self.pan_target - self.pan_cur) * PAN_SMOOTH_K;
            if let Some(slot) = self.ring.get_mut(self.write) {
                *slot = 0.0;
            }
            self.write = (self.write + 1) & PSEUDO_RING_MASK;
        }
        self.written = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact golden vectors from `docs/re/kontakt-compressor.md`.
    #[test]
    fn enhanced_compressor_matches_native_golden_vectors_and_link_cancellation() {
        let mut comp = EnhancedCompressor::new(
            EnhancedCompressorSettings {
                threshold_db: -6.0,
                saved_ratio: 0.5,
                attack_ms: 1.0,
                release_ms: 100.0,
                makeup_db: 0.0,
                link: false,
                mix: 1.0,
            },
            48_000,
        );
        let mut frames = [[1.0_f32, -0.5_f32]; 4];
        comp.process(&mut frames);
        let want_l = [0.999_157_3_f32, 0.997_557_64, 0.995_283_8, 0.992_414_53];
        for (i, (&got, &want)) in frames.iter().zip(&want_l).enumerate() {
            assert!(
                (got[0] - want).abs() < 1e-6,
                "frame {i} L mismatch: got {:.9}, want {:.9}",
                got[0],
                want
            );
            assert_eq!(got[1], -0.5_f32, "frame {i} R should stay untouched");
        }

        // With link enabled, `[1.0, -1.0]` cancels to 0 in the signed mean and passes unchanged.
        let mut linked = EnhancedCompressor::new(
            EnhancedCompressorSettings {
                threshold_db: -6.0,
                saved_ratio: 0.5,
                attack_ms: 1.0,
                release_ms: 100.0,
                makeup_db: 0.0,
                link: true,
                mix: 1.0,
            },
            48_000,
        );
        let mut cancel = [[1.0_f32, -1.0_f32]; 16];
        linked.process(&mut cancel);
        for (i, &[l, r]) in cancel.iter().enumerate() {
            assert!(
                (l - 1.0).abs() < 1e-6 && (r + 1.0).abs() < 1e-6,
                "frame {i}: [{l}, {r}]"
            );
        }
    }

    #[test]
    fn solid_bus_comp_auto_release_recovers_faster_on_transient_than_sustained() {
        let rate = 48_000;
        let settings = SolidBusCompSettings {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: SOLID_BUS_ATTACK_MS[2], // 1.0 ms
            release_ms: SOLID_BUS_RELEASE_MS[5],
            auto_release: true,
            makeup_db: 0.0,
            link: true,
            mix: 1.0,
        };
        // Short 10 ms transient burst followed by 150 ms silence.
        let mut short_comp = SolidBusComp::new(settings, rate);
        let mut burst = vec![[0.8_f32; 2]; 480];
        short_comp.process(&mut burst);
        let short_peak_gr = short_comp.gain_reduction_db(0);
        let mut silence = vec![[0.0_f32; 2]; 7_200];
        short_comp.process(&mut silence);
        let short_after = short_comp.gain_reduction_db(0);

        // Sustained 1000 ms signal followed by 150 ms silence.
        let mut long_comp = SolidBusComp::new(settings, rate);
        let mut sustain = vec![[0.8_f32; 2]; 48_000];
        long_comp.process(&mut sustain);
        let long_peak_gr = long_comp.gain_reduction_db(0);
        let mut silence2 = vec![[0.0_f32; 2]; 7_200];
        long_comp.process(&mut silence2);
        let long_after = long_comp.gain_reduction_db(0);

        assert!(short_peak_gr > 5.0 && long_peak_gr > 10.0);
        let short_frac = short_after / short_peak_gr;
        let long_frac = long_after / long_peak_gr;
        assert!(
            short_frac < long_frac * 0.7,
            "auto-release must recover faster after brief transient than sustained signal: \
             short_frac={short_frac:.3} long_frac={long_frac:.3}"
        );

        // Signed-mean link cancellation.
        let mut link_comp = SolidBusComp::new(settings, rate);
        let mut cancel = vec![[1.0_f32, -1.0_f32]; 256];
        link_comp.process(&mut cancel);
        assert!(link_comp.gain_reduction_db(0) < 1e-4);
    }

    fn sine_gain_db(filter: &mut SolidPassFilters, hz: f64, rate: u32) -> f64 {
        filter.reset();
        let n = 4096usize;
        let settle = 2048usize;
        let mut buf: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let s = (2.0 * PI * hz * i as f64 / f64::from(rate)).sin() as f32;
                [s, 0.0]
            })
            .collect();
        filter.process(&mut buf);
        // Verify zero L->R stereo crosstalk.
        assert!(buf.iter().all(|f| f[1] == 0.0));
        let rms_out = (buf
            .iter()
            .skip(settle)
            .map(|f| f64::from(f[0]) * f64::from(f[0]))
            .sum::<f64>()
            / (n - settle) as f64)
            .sqrt();
        let rms_in = std::f64::consts::FRAC_1_SQRT_2;
        20.0 * (rms_out / rms_in).max(1e-9).log10()
    }

    #[test]
    fn solid_pass_filters_match_18db_hp_and_12db_lp_slopes_and_tsv() {
        let rate = 48_000;
        // HP at norm=0.8 (~286 Hz): TSV measures -40.23 dB @ 60 Hz, -27.03 dB @ 100 Hz, -10.07 dB @ 200 Hz.
        let mut hp = SolidPassFilters::new(
            SolidPassSettings::from_normalized(true, 0.8, false, 0.0),
            rate,
        );
        let db60 = sine_gain_db(&mut hp, 60.0, rate);
        let db120 = sine_gain_db(&mut hp, 120.0, rate);
        let db200 = sine_gain_db(&mut hp, 200.0, rate);
        let db1000 = sine_gain_db(&mut hp, 1000.0, rate);
        let hp_slope = db120 - db60;
        assert!(
            (hp_slope - 18.0).abs() < 1.0,
            "HP slope expected ~18 dB/oct, got {hp_slope:.2} dB/oct"
        );
        assert!(
            (db200 - (-10.07)).abs() < 1.0,
            "HP norm=0.8 @200 Hz expected ~-10.07 dB, got {db200:.2} dB"
        );
        assert!(
            db1000.abs() < 0.25,
            "HP passband should be flat: {db1000:.2} dB"
        );

        // LP at norm=1.0 (~3270 Hz): TSV measures -8.40 dB @ 5 kHz, -19.56 dB @ 10 kHz.
        let mut lp = SolidPassFilters::new(
            SolidPassSettings::from_normalized(false, 0.0, true, 1.0),
            rate,
        );
        let db200_lp = sine_gain_db(&mut lp, 200.0, rate);
        let db5k = sine_gain_db(&mut lp, 5_000.0, rate);
        let db10k = sine_gain_db(&mut lp, 10_000.0, rate);
        assert!(
            db200_lp.abs() < 0.1,
            "LP passband should be flat: {db200_lp:.2} dB"
        );
        assert!(
            (db5k - (-8.40)).abs() < 1.0,
            "LP norm=1.0 @5 kHz expected ~-8.40 dB, got {db5k:.2} dB"
        );
        assert!(
            (db10k - (-19.56)).abs() < 1.5,
            "LP norm=1.0 @10 kHz expected ~-19.56 dB, got {db10k:.2} dB"
        );
        let lp_slope = db5k - db10k;
        assert!(
            (lp_slope - 11.5).abs() < 2.0,
            "LP 2nd-order rolloff 5k->10k expected ~11-12 dB, got {lp_slope:.2} dB"
        );
    }

    #[test]
    fn stereo_modeller_pseudo_delay_and_settled_matrix_match_spec() {
        assert_eq!(pseudo_delay_frames(0.0, 48_000), 0);
        assert_eq!(pseudo_delay_frames(0.5, 48_000), 60);
        assert_eq!(pseudo_delay_frames(1.0, 48_000), 480);
        assert_eq!(pseudo_delay_frames(1.0, 192_000), 1023);

        // Pseudo-stereo impulse: L appears at frame 0, R delayed by 60 frames at w=0.5, 48 kHz.
        let mut pseudo = StereoModeller::new(
            StereoModellerSettings {
                width: 0.5,
                pan: 0.0,
                pseudo: true,
            },
            48_000,
        );
        let mut buf = vec![[0.0_f32; 2]; 128];
        if let Some(first) = buf.first_mut() {
            *first = [1.0, 1.0];
        }
        pseudo.process(&mut buf);
        assert_eq!(buf.first().map(|f| f[0]), Some(1.0));
        assert_eq!(buf.first().map(|f| f[1]), Some(0.0));
        assert_eq!(buf.get(60).map(|f| f[1]), Some(1.0));
        assert!(buf.iter().enumerate().all(|(i, f)| i == 60 || f[1] == 0.0));

        // Non-pseudo settled matrix matches `crate::matrix::matrix` for width + pan.
        for &(width, pan) in &[(0.0_f32, 0.0_f32), (0.5, -0.5), (1.0, 0.25)] {
            let mut sm = StereoModeller::new(
                StereoModellerSettings {
                    width,
                    pan,
                    pseudo: false,
                },
                48_000,
            );
            let mut io = [[0.7_f32, -0.3_f32]];
            sm.process(&mut io);
            let m = crate::matrix::matrix(&ir::Effect {
                kind: ir::EffectKind::Stereo { width, pan },
                bypass: false,
                output: 1.0,
                dry: 0.0,
            });
            let want = [
                m[0][0] * 0.7 + m[0][1] * -0.3,
                m[1][0] * 0.7 + m[1][1] * -0.3,
            ];
            assert!((io[0][0] - want[0]).abs() < 1e-5 && (io[0][1] - want[1]).abs() < 1e-5);
        }
    }
}
