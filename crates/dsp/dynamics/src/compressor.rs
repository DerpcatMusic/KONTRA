//! A compressor: peak level in dB (of the input, or of its own output for a
//! feedback design), a soft-knee static curve, attack and release on the
//! gain reduction, make-up and a dry mix.

use kontra_dsp_core::math::{db_to_gain_f32, gain_to_db_f32};

use crate::{gain, pole};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub threshold_db: f32,
    /// Input dB over the threshold per output dB over it; infinite limits.
    pub ratio: f32,
    /// Width of the soft knee around the threshold; 0 is a hard knee.
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    /// Both channels follow the louder one.
    pub link: bool,
    /// 0..=1: the compressed share of the output, the rest is the input.
    pub mix: f32,
    pub detector: Detector,
    /// A one-pole high-pass on what the detector reads (not on the audio),
    /// its corner in Hz.
    pub sidechain_hz: Option<f32>,
}

/// What the level detector reads and how gain reduction is computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detector {
    /// The input (feed-forward peak with soft knee).
    Input,
    /// The input at the gain it last got (feedback).
    Output,
    /// The input past the threshold triggers a hold that the gain follows
    /// to a fixed depth, `threshold·(1 − 1/ratio)` dB, by the attack; the
    /// hold falls by the release while nothing retriggers it. Linked, the
    /// channels' mean triggers both.
    Trigger,
    /// 8-segment piecewise linear transfer curve, 2-state attack recurrence,
    /// decaying peak hold, and signed-mean stereo link.
    Enhanced,
    /// Bus VCA hybrid RMS/peak detector with `ln(0.8)` time constants,
    /// signed-mean stereo link, 2 dB soft knee, and optional dual-stage
    /// program-dependent auto-release.
    Bus { auto_release: bool },
}

/// Bus-compressor one-pole decay coefficient `min(1, exp(ln(0.8) / (ms * 0.001 * rate)))`.
#[must_use]
pub fn bus_coeff(ms: f32, rate: u32) -> f32 {
    let denom = f64::from(ms.max(1e-4)) * 0.001 * f64::from(rate.max(1));
    (0.8_f64.ln() / denom).exp().min(1.0) as f32
}

/// 8-segment piecewise linear transfer curve (`(upper_bound, slope, intercept)`).
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
struct EnhancedState {
    v: f32,
    y: f32,
    hold: f32,
}

impl EnhancedState {
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

#[derive(Clone, Copy, Debug, Default)]
struct BusState {
    rms_sq: f32,
    env_db: f32,
    slow_db: f32,
}

#[derive(Clone)]
pub struct Compressor {
    s: Settings,
    rate: u32,
    attack: f32,
    release: f32,
    makeup: f32,
    /// Gain reduction per channel, dB (≥ 0).
    reduction: [f32; 2],
    /// [`Detector::Trigger`]: per channel, the hold and the envelope the
    /// gain follows (0..=1), and the linear threshold and depth.
    hold: [f32; 2],
    envelope: [f32; 2],
    threshold: f32,
    depth: f32,
    /// [`Detector::Enhanced`] parameters & per-channel state.
    enh_excess_scale: f32,
    enh_attack_k: f32,
    enh_release_mul: f32,
    enh: [EnhancedState; 2],
    /// [`Detector::Bus`] coefficients & per-channel state.
    bus_attack: f32,
    bus_release: f32,
    bus_rms: f32,
    bus_auto_fast_rel: f32,
    bus_auto_slow_att: f32,
    bus_auto_slow_rel: f32,
    bus: [BusState; 2],
    /// [`Settings::sidechain_hz`]: its pole (0 passes all) and, per
    /// channel, the last input and output.
    key: f32,
    keyed: [[f32; 2]; 2],
}

impl Compressor {
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let rate = rate.max(1);
        let mut c = Self {
            s,
            rate,
            attack: 1.0,
            release: 1.0,
            makeup: 1.0,
            reduction: [0.0; 2],
            hold: [0.0; 2],
            envelope: [0.0; 2],
            threshold: 1.0,
            depth: 1.0,
            enh_excess_scale: 0.0,
            enh_attack_k: 0.0,
            enh_release_mul: 1.0,
            enh: [EnhancedState::default(); 2],
            bus_attack: 0.0,
            bus_release: 0.0,
            bus_rms: bus_coeff(2.5, rate),
            bus_auto_fast_rel: bus_coeff(25.0, rate),
            bus_auto_slow_att: bus_coeff(80.0, rate),
            bus_auto_slow_rel: bus_coeff(280.0, rate),
            bus: [BusState::default(); 2],
            key: 0.0,
            keyed: [[0.0; 2]; 2],
        };
        c.set(s, rate);
        c
    }

    /// Changes the settings while it plays; the reduction carries over.
    pub fn set(&mut self, s: Settings, rate: u32) {
        let rate_u32 = rate.max(1);
        let rate = rate_u32 as f32;
        if self.rate != rate_u32 {
            self.rate = rate_u32;
            self.bus_rms = bus_coeff(2.5, rate_u32);
            self.bus_auto_fast_rel = bus_coeff(25.0, rate_u32);
            self.bus_auto_slow_att = bus_coeff(80.0, rate_u32);
            self.bus_auto_slow_rel = bus_coeff(280.0, rate_u32);
        }
        if matches!(s.detector, Detector::Bus { auto_release: b } if !matches!(self.s.detector, Detector::Bus { auto_release: a } if a == b))
        {
            for d in &mut self.bus {
                d.slow_db = 0.0;
            }
        }
        self.s = s;
        self.attack = pole(s.attack_ms, rate);
        self.release = pole(s.release_ms, rate);
        self.makeup = gain(s.makeup_db);
        self.threshold = gain(s.threshold_db);
        self.depth = gain(s.threshold_db * (1.0 - 1.0 / s.ratio.max(1.0)));
        self.key = s
            .sidechain_hz
            .filter(|hz| *hz > 0.0)
            .map_or(0.0, |hz| (-std::f32::consts::TAU * hz / rate).exp());

        if s.detector == Detector::Enhanced {
            let saved_ratio = (1.0_f32 / s.ratio.max(1.0)).clamp(0.0, 1.0);
            let ratio_scale = if saved_ratio < 0.04_f32 {
                5.0_f32
            } else {
                1.0_f32 / (saved_ratio + 0.13_f32) - 0.884_f32
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
            let attack_mult =
                (1.0_f64 + 0.289_529_450_721_538_1_f64 * f64::from(attack_ms).ln()) as f32;
            self.enh_excess_scale = (ratio_scale / self.threshold.max(0.0004_f32)) * attack_mult;
            self.enh_attack_k = 2513.2_f32 / (rate * attack_ms);
            self.enh_release_mul = if release_ms < 50.0_f32 {
                1.0_f32 - 35.566_f32 / rate
            } else {
                1.0_f32 - 1778.3_f32 / (rate * release_ms)
            }
            .clamp(0.0_f32, 1.0_f32);
        } else if matches!(s.detector, Detector::Bus { .. }) {
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
            self.bus_attack = bus_coeff(att_ms, rate_u32);
            self.bus_release = bus_coeff(rel_ms, rate_u32);
        }
    }

    /// Compresses `io` in place using `io` as the detector signal.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        self.process_inner(io, None);
    }

    /// Compresses `io` in place using an external sidechain `key` signal for
    /// level detection (falls back to `io` past `key.len()`).
    pub fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        self.process_inner(io, Some(key));
    }

    fn process_inner(&mut self, io: &mut [[f32; 2]], sidechain: Option<&[[f32; 2]]>) {
        if io.is_empty() {
            return;
        }
        match self.s.detector {
            Detector::Trigger => {
                self.trigger_inner(io, sidechain);
                return;
            }
            Detector::Enhanced => {
                self.enhanced_inner(io, sidechain);
                return;
            }
            Detector::Bus { auto_release } => {
                self.bus_inner(io, sidechain, auto_release);
                return;
            }
            Detector::Input | Detector::Output => {}
        }
        if self.reduction == [0.0; 2]
            && self.keyed == [[0.0; 2]; 2]
            && io.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
            && sidechain.is_none_or(|sc| sc.iter().all(|f| f[0] == 0.0 && f[1] == 0.0))
        {
            return;
        }
        let mix = self.s.mix.clamp(0.0, 1.0);
        let slope = 1.0 - 1.0 / self.s.ratio.max(1.0);
        let w = self.s.knee_db.max(0.0);
        let half_w = 0.5 * w;
        let inv_2w = if w > 0.0 { 0.5 / w } else { 0.0 };
        let thresh_db = self.s.threshold_db;
        let lin_knee_lo = db_to_gain_f32(thresh_db - half_w);
        let attack = self.attack;
        let release = self.release;
        let makeup = self.makeup;
        let has_hp = self.key != 0.0;
        let is_fb = self.s.detector == Detector::Output && sidechain.is_none();

        #[inline(always)]
        fn target_red(
            level: f32,
            lin_knee_lo: f32,
            thresh_db: f32,
            half_w: f32,
            inv_2w: f32,
            slope: f32,
        ) -> f32 {
            if level <= lin_knee_lo {
                return 0.0;
            }
            let db = gain_to_db_f32(level.max(1e-9));
            let over = db - thresh_db;
            let into = if over <= -half_w {
                0.0
            } else if over < half_w {
                let t = over + half_w;
                t * t * inv_2w
            } else {
                over
            };
            into * slope
        }

        // Fast path: stereo-linked feed-forward without external sidechain.
        // Evaluates one detector and one dB-to-gain conversion per stereo frame.
        if self.s.link && !is_fb && sidechain.is_none() {
            let mut red = self.reduction[0].max(self.reduction[1]);
            let same_red = self.reduction[0] == self.reduction[1];
            if same_red {
                let dry = 1.0 - mix;
                let wet_makeup = mix * makeup;
                for frame in io.iter_mut() {
                    let key = if has_hp {
                        self.highpass(*frame)
                    } else {
                        *frame
                    };
                    let level = key[0].abs().max(key[1].abs());
                    let target = target_red(level, lin_knee_lo, thresh_db, half_w, inv_2w, slope);
                    let k = if target > red { attack } else { release };
                    red += (target - red) * k;
                    if red < 1e-6 {
                        red = 0.0;
                    }
                    let gr = if red == 0.0 {
                        1.0
                    } else {
                        db_to_gain_f32(-red)
                    };
                    let total_g = dry + wet_makeup * gr;
                    frame[0] *= total_g;
                    frame[1] *= total_g;
                }
                self.reduction = [red, red];
                return;
            }
        }

        let dry = 1.0 - mix;
        let wet_makeup = mix * makeup;
        for (i, frame) in io.iter_mut().enumerate() {
            let det_in = sidechain.and_then(|k| k.get(i).copied()).unwrap_or(*frame);
            let key = if has_hp {
                self.highpass(det_in)
            } else {
                det_in
            };
            let [l, r] = if is_fb {
                let ([a, b], [ra, rb]) = (key, self.reduction);
                [
                    (a * if ra == 0.0 { 1.0 } else { db_to_gain_f32(-ra) }).abs(),
                    (b * if rb == 0.0 { 1.0 } else { db_to_gain_f32(-rb) }).abs(),
                ]
            } else {
                [key[0].abs(), key[1].abs()]
            };
            let levels = if self.s.link { [l.max(r); 2] } else { [l, r] };
            for ((x, level), reduction) in frame.iter_mut().zip(levels).zip(&mut self.reduction) {
                let target = target_red(level, lin_knee_lo, thresh_db, half_w, inv_2w, slope);
                let k = if target > *reduction { attack } else { release };
                *reduction += (target - *reduction) * k;
                // Released to silence: no subnormal tail.
                if *reduction < 1e-6 {
                    *reduction = 0.0;
                }
                let gr = if *reduction == 0.0 {
                    1.0
                } else {
                    db_to_gain_f32(-*reduction)
                };
                *x *= dry + wet_makeup * gr;
            }
        }
    }

    /// What the detector reads of `frame`: [`Settings::sidechain_hz`]'s
    /// high-pass, `y = (1 + p)/2·(x − x₁) + p·y₁`.
    fn highpass(&mut self, frame: [f32; 2]) -> [f32; 2] {
        if self.key == 0.0 {
            return frame;
        }
        let p = self.key;
        let mut out = frame;
        for (x, [x1, y1]) in out.iter_mut().zip(&mut self.keyed) {
            let y = 0.5 * (1.0 + p) * (*x - *x1) + p * *y1;
            // Silence settles to 0, never to a subnormal tail.
            *y1 = if y.abs() < 1e-20 { 0.0 } else { y };
            *x1 = *x;
            *x = *y1;
        }
        out
    }

    /// [`Detector::Trigger`]: a level past the threshold sets the hold to 1
    /// once it has fallen below [`REARM`]; else the hold falls by the
    /// release. The envelope drops to the hold at once and rises toward 1
    /// by the attack, never above the hold. While it lags well behind a
    /// fresh trigger, the hold restarts just above it ([`CATCH`]). The gain
    /// is `1 − (1 − depth)·envelope`.
    fn trigger_inner(&mut self, io: &mut [[f32; 2]], sidechain: Option<&[[f32; 2]]>) {
        let mix = self.s.mix.clamp(0.0, 1.0);
        let (rise, fall) = (1.0 - self.attack, 1.0 - self.release);
        for (i, frame) in io.iter_mut().enumerate() {
            let det_in = sidechain.and_then(|k| k.get(i).copied()).unwrap_or(*frame);
            let [l, r] = self.highpass(det_in);
            let levels = if self.s.link {
                [((l + r) * 0.5).abs(); 2]
            } else {
                [l.abs(), r.abs()]
            };
            let states = self.hold.iter_mut().zip(&mut self.envelope);
            for ((x, level), (hold, env)) in frame.iter_mut().zip(levels).zip(states) {
                let v = if level <= self.threshold || *hold >= REARM {
                    *hold *= fall;
                    *hold
                } else {
                    *hold = 1.0;
                    1.0
                };
                *env = if v <= *env {
                    v
                } else {
                    (1.0 - (1.0 - *env) * rise).min(v)
                };
                if v < CATCH.0 && (*env + 1.0) / (v + 1.0) < REARM {
                    *hold = *env * CATCH.1 + (CATCH.1 - 1.0);
                }
                let wet = *x * (1.0 - (1.0 - self.depth) * *env) * self.makeup;
                *x += (wet - *x) * mix;
            }
        }
        for (reduction, env) in self.reduction.iter_mut().zip(self.envelope) {
            *reduction = -20.0 * (1.0 - (1.0 - self.depth) * env).max(1e-9).log10();
        }
    }

    /// [`Detector::Enhanced`]: 8-segment piecewise linear transfer curve,
    /// 2-state attack recurrence, decaying peak hold, signed-mean link.
    fn enhanced_inner(&mut self, io: &mut [[f32; 2]], sidechain: Option<&[[f32; 2]]>) {
        let mix = self.s.mix.clamp(0.0, 1.0);
        let dry = 1.0 - mix;
        let makeup = self.makeup;
        let thr = self.threshold;
        let scale = self.enh_excess_scale;
        let k = self.enh_attack_k;
        let rel = self.enh_release_mul;
        let link = self.s.link;
        let has_hp = self.key != 0.0;
        let mut last_g = [1.0_f32; 2];
        for (i, frame) in io.iter_mut().enumerate() {
            let det_in = sidechain.and_then(|s| s.get(i).copied()).unwrap_or(*frame);
            let [kl, kr] = if has_hp {
                self.highpass(det_in)
            } else {
                det_in
            };
            let [gl, gr] = if link {
                let g = self.enh[0].step((0.5_f32 * (kl + kr)).abs(), thr, scale, k, rel);
                [g, g]
            } else {
                let gl = self.enh[0].step(kl.abs(), thr, scale, k, rel);
                let gr = self.enh[1].step(kr.abs(), thr, scale, k, rel);
                [gl, gr]
            };
            last_g = [gl, gr];
            let [l, r] = *frame;
            let wl = l * gl * makeup;
            let wr = r * gr * makeup;
            if mix >= 1.0_f32 {
                *frame = [wl, wr];
            } else {
                *frame = [l * dry + wl * mix, r * dry + wr * mix];
            }
        }
        self.reduction = [
            -gain_to_db_f32(last_g[0].max(1e-9)).min(0.0),
            -gain_to_db_f32(last_g[1].max(1e-9)).min(0.0),
        ];
    }

    /// [`Detector::Bus`]: hybrid RMS/peak detector, 2 dB soft knee, `ln(0.8)`
    /// ballistics, signed-mean link, and optional dual-stage auto-release.
    fn bus_inner(
        &mut self,
        io: &mut [[f32; 2]],
        sidechain: Option<&[[f32; 2]]>,
        auto_release: bool,
    ) {
        let mix = self.s.mix.clamp(0.0, 1.0);
        let dry = 1.0 - mix;
        let makeup = self.makeup;
        let thr_db = self.s.threshold_db;
        let slope = 1.0 - 1.0 / self.s.ratio.max(1.0);
        let rms_c = self.bus_rms;
        let att_c = self.bus_attack;
        let rel_c = self.bus_release;
        let auto = auto_release.then_some((
            self.bus_auto_fast_rel,
            self.bus_auto_slow_att,
            self.bus_auto_slow_rel,
        ));
        let link = self.s.link;
        let has_hp = self.key != 0.0;

        let step_bus = |d: &mut BusState, x: f32| -> (f32, f32) {
            const KNEE_DB: f32 = 2.0;
            d.rms_sq = rms_c * d.rms_sq + (1.0 - rms_c) * (x * x);
            let level = (0.5 * x.abs() + 0.5 * d.rms_sq.max(0.0).sqrt()).max(1e-12);
            let level_db = gain_to_db_f32(level);
            let over = level_db - thr_db;
            let target_gr = if over <= -KNEE_DB * 0.5 {
                0.0
            } else if over >= KNEE_DB * 0.5 {
                over * slope
            } else {
                let t = over + KNEE_DB * 0.5;
                slope * (t * t) / (2.0 * KNEE_DB)
            };
            let gr_db = if let Some((fast_rel, slow_att, slow_rel)) = auto {
                let c_fast = if target_gr > d.env_db {
                    att_c
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
                let c = if target_gr > d.env_db { att_c } else { rel_c };
                d.env_db = c * d.env_db + (1.0 - c) * target_gr;
                d.env_db
            };
            (db_to_gain_f32(-gr_db), gr_db)
        };

        for (i, frame) in io.iter_mut().enumerate() {
            let det_in = sidechain.and_then(|s| s.get(i).copied()).unwrap_or(*frame);
            let [kl, kr] = if has_hp {
                self.highpass(det_in)
            } else {
                det_in
            };
            let ([gl, gr], red) = if link {
                let (g, db) = step_bus(&mut self.bus[0], 0.5 * (kl + kr));
                ([g, g], [db, db])
            } else {
                let (gl, dbl) = step_bus(&mut self.bus[0], kl);
                let (gr, dbr) = step_bus(&mut self.bus[1], kr);
                ([gl, gr], [dbl, dbr])
            };
            self.reduction = red;
            let [l, r] = *frame;
            let wl = l * gl * makeup;
            let wr = r * gr * makeup;
            if mix >= 1.0_f32 {
                *frame = [wl, wr];
            } else {
                *frame = [l * dry + wl * mix, r * dry + wr * mix];
            }
        }
    }

    /// Current gain reduction, dB, per channel.
    #[must_use]
    pub fn reduction(&self) -> [f32; 2] {
        self.reduction
    }
}

/// A trigger rearms once the hold falls below this.
const REARM: f32 = 0.985;
/// Below the first, a hold that has outrun its envelope restarts at
/// `envelope·second + (second − 1)`, `second = 1/0.985`.
const CATCH: (f32, f32) = (0.97, 1.015_228_4);
