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

/// What the level detector reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detector {
    /// The input (feed-forward).
    Input,
    /// The input at the gain it last got (feedback).
    Output,
    /// The input past the threshold triggers a hold that the gain follows
    /// to a fixed depth, `threshold·(1 − 1/ratio)` dB, by the attack; the
    /// hold falls by the release while nothing retriggers it. Linked, the
    /// channels' mean triggers both.
    Trigger,
}

#[derive(Clone)]
pub struct Compressor {
    s: Settings,
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
    /// [`Settings::sidechain_hz`]: its pole (0 passes all) and, per
    /// channel, the last input and output.
    key: f32,
    keyed: [[f32; 2]; 2],
}

impl Compressor {
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let mut c = Self {
            s,
            attack: 1.0,
            release: 1.0,
            makeup: 1.0,
            reduction: [0.0; 2],
            hold: [0.0; 2],
            envelope: [0.0; 2],
            threshold: 1.0,
            depth: 1.0,
            key: 0.0,
            keyed: [[0.0; 2]; 2],
        };
        c.set(s, rate);
        c
    }

    /// Changes the settings while it plays; the reduction carries over.
    pub fn set(&mut self, s: Settings, rate: u32) {
        let rate = rate as f32;
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
        if self.s.detector == Detector::Trigger {
            self.trigger_inner(io, sidechain);
            return;
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
