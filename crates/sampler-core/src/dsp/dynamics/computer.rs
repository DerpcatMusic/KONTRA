//! Gain-computer stages: the static curve from detector level to the amount
//! the ballistics follow (or straight to a gain).

use super::pipeline::{each, Frame, Stage};
use sampler_ir::fx::dynamics::Direction;
use std::sync::LazyLock;

pub(super) const DB_PER_NEPER: f64 = 8.685_889_638_065_037;
/// `20 log10 2`.
const DB_PER_BIT: f64 = 6.020_599_913_279_624;
const SEGMENTS: usize = 1024;
/// `log2(1 + i / 1024)` and `2^(i / 1024)`, i = 0..=1024.
static LOG2: LazyLock<[f64; SEGMENTS + 1]> =
    LazyLock::new(|| std::array::from_fn(|i| (1. + i as f64 / SEGMENTS as f64).log2()));
static EXP2: LazyLock<[f64; SEGMENTS + 1]> =
    LazyLock::new(|| std::array::from_fn(|i| (i as f64 / SEGMENTS as f64).exp2()));

/// dB of a positive normal level, table-interpolated (error under 1e-6 dB).
#[inline(always)]
pub(super) fn table_db(level: f64) -> f64 {
    let bits = level.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as f64 - 1023.;
    let at = (bits & ((1 << 52) - 1)) as f64 * (SEGMENTS as f64 / (1u64 << 52) as f64);
    let i = at as usize;
    let t = &*LOG2;
    DB_PER_BIT * (exponent + t[i] + (t[i + 1] - t[i]) * (at - i as f64))
}

/// `10^(db / 20)`, table-interpolated (relative error under 2e-7), for
/// |db| up to 6000.
#[inline(always)]
pub(super) fn table_gain(db: f64) -> f64 {
    let x = (db / DB_PER_BIT).clamp(-1000., 1000.);
    // floor without a libm call (baseline x86-64 has no roundsd); |x| <= 1000.
    let t = x as i64 as f64;
    let whole = if t > x { t - 1. } else { t };
    let at = (x - whole) * SEGMENTS as f64;
    let i = (at as usize).min(SEGMENTS - 1);
    let t = &*EXP2;
    (t[i] + (t[i + 1] - t[i]) * (at - i as f64)) * f64::from_bits(((whole as i64 + 1023) as u64) << 52)
}

/// Boost ceilings. ponytail: upward compression and expansion of silence is
/// unbounded in theory; these bound it, raise if a real effect needs more.
pub(super) const MAX_BOOST_DB: f64 = 60.;
const MAX_CUT_DB: f64 = 200.;

/// Generic: ratio and quadratic knee in dB, downward, upward or expanding
/// (a gate is a large expansion ratio), capped by the range.
#[derive(Clone, Copy)]
pub(crate) struct Ratio {
    pub direction: Direction,
    pub threshold_db: f64,
    pub threshold: f64,
    /// `1 - 1/ratio` (compression) or `ratio - 1` (expansion).
    pub slope: f64,
    pub knee_db: f64,
    /// Lowest level that reaches the knee (downward).
    pub knee_floor: f64,
    /// Largest reduction or boost in dB.
    pub range: f64,
    /// Channels with their own detector: 1 linked, else 2.
    pub channels: usize,
}

impl Ratio {
    /// Gain change in dB for a steady linear `level`, nonnegative; its sign is
    /// the direction's (it lowers, or for upward compression raises).
    #[inline(always)]
    pub(crate) fn target(&self, level: f64) -> f64 {
        match self.direction {
            Direction::Downward => {
                // Linear gate: skip the log while the level cannot reach the knee.
                if self.knee_db == 0. {
                    if level > self.threshold {
                        ((table_db(level) - self.threshold_db) * self.slope).min(self.range)
                    } else {
                        0.
                    }
                } else if level <= self.knee_floor {
                    0.
                } else {
                    self.curve(table_db(level) - self.threshold_db, MAX_CUT_DB.min(self.range))
                }
            }
            Direction::Upward => self.curve(self.threshold_db - db(level), MAX_BOOST_DB.min(self.range)),
            Direction::Expand => self.curve(self.threshold_db - db(level), MAX_CUT_DB.min(self.range)),
        }
    }

    /// `slope` times the amount past the threshold, with the quadratic knee.
    #[inline(always)]
    fn curve(&self, over: f64, cap: f64) -> f64 {
        let w = self.knee_db;
        let amount = if 2. * over < -w {
            0.
        } else if w > 0. && 2. * over.abs() <= w {
            let t = over + w / 2.;
            t * t / (2. * w)
        } else {
            over
        };
        (amount * self.slope).min(cap)
    }
}

#[inline(always)]
fn db(level: f64) -> f64 {
    if level > 1e-20 { table_db(level) } else { -400. }
}

impl Stage for Ratio {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.level[0] = self.target(f.level[0]);
        if self.channels == 2 {
            f.level[1] = self.target(f.level[1]);
        }
    }
}

/// Kontakt Classic: a trigger, 1 while the level is over the threshold.
pub(super) struct Trigger(pub f32);

impl Stage for Trigger {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.level = each(f.level, |v| if v as f32 <= self.0 { 0. } else { 1. });
    }
}

/// Kontakt Enhanced: the excess over the threshold, scaled, through eight
/// native linear segments (constants `0x144fc3330..0x144fc4600`).
pub(super) struct Segments {
    pub threshold: f32,
    pub excess_scale: f32,
}

const CURVE: [[f32; 3]; 8] = [
    [0.5, 0.46, 0.],
    [1.207, 0.381895, 0.0390524],
    [2.207, 0.34, 0.08962],
    [3.621, 0.29703, 0.184456],
    [5.621, 0.26, 0.31854],
    [8.449, 0.23338, 0.468168],
    [12.449, 0.215, 0.623465],
    [f32::INFINITY, 0.198195, 0.832671],
];

impl Stage for Segments {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.level = each(f.level, |v| {
            let excess = v as f32 - self.threshold;
            f64::from(if excess >= 0. {
                let x = excess * self.excess_scale;
                let [_, slope, intercept] = CURVE.iter().find(|row| x <= row[0]).copied().unwrap_or(CURVE[7]);
                x * slope + intercept
            } else {
                0.
            })
        });
    }
}

/// Kontakt Pro: dB by a quadratic mantissa (6.02 dB per exponent) less the
/// threshold, through a 12 dB quadratic soft knee. f32.
pub(super) struct ProKnee(pub f32);

const DB_PER_OCTAVE: f32 = f32::from_bits(0x40c0_a82f); // 0x144f38bd4
const LOG_A: f32 = f32::from_bits(0x4101_aea6); // 0x144fc4484
const LOG_B: f32 = f32::from_bits(0x4005_69ee); // 0x144fc406c
const KNEE_DB: f32 = 6.; // 0x144f38bd0: half the 12 dB knee, and the dB per knee unit
const KNEE_UNIT: f32 = f32::from_bits(0x3e2a_aac1); // 0x144f38720

impl Stage for ProKnee {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let bits = (f.level[0] as f32).to_bits();
        let mantissa = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000) - 1.;
        let exponent = ((bits & 0x7f80_0000) >> 23) as i32 - 127;
        let level = (LOG_A - mantissa * LOG_B) * mantissa + exponent as f32 * DB_PER_OCTAVE - self.0;
        let t = (level + KNEE_DB).max(0.) * KNEE_UNIT;
        let k = t.min(2.);
        f.level = [f64::from((t - (k - k * k * 0.25)) * KNEE_DB); 2];
    }
}

/// Kontakt Limiter: `1 / (1 + y^2/4)` up to `y = 2`, `1 / y` beyond. f32.
pub(super) struct SoftClip;

impl Stage for SoftClip {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let y = f.level[0] as f32;
        let m = y.min(2.);
        f.gain = [f64::from(1. / ((y - (1. - m * 0.25) * m) + 1.)); 2];
    }
}

/// Kontakt Transparent Limiter, before its ballistics: level in dB. f32.
pub(super) struct Decibel;

impl Stage for Decibel {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.level = [f64::from(super::logarithm::log10(f.level[0] as f32) * 20.); 2];
    }
}

/// Kontakt Transparent Limiter, after its ballistics: gain `-max(level, 0)`
/// dB, capped so the window's driven peak stays under 0 dBFS. f32.
pub(super) struct Ceiling;

impl Stage for Ceiling {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let mut gain_db = (-(f.level[0] as f32)).min(0.);
        let window_db = super::logarithm::log10(f.peak as f32) * 20.;
        if window_db + gain_db > 0. {
            gain_db = -window_db;
        }
        f.gain = [f64::from(10f32.powf(gain_db * f32::from_bits(0x3d4c_cccd))); 2];
    }
}

/// UVI Maximizer: level in dB, lifted to the threshold below it (soft knee
/// `(v - t + k)^2 / 4k + t` within ±k). f32.
pub(super) struct MaxKnee {
    pub threshold: f32,
    pub knee: f32,
}

impl Stage for MaxKnee {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let (t, k) = (self.threshold, self.knee);
        let v = 20. * super::logarithm::log10(f.level[0] as f32);
        let v = if k > 0. {
            if v < t - k {
                t
            } else if v < k + t {
                let u = (v - t) + k;
                u * u * (0.25 / k) + t
            } else {
                v
            }
        } else {
            v.max(t)
        };
        f.level = [f64::from(v); 2];
    }
}

/// Solid Bus Comp: the request `(threshold - level) * (1 - R)` in 0..=1.
pub(super) struct BusRatio {
    pub threshold: f64,
    pub slope: f64,
}

impl BusRatio {
    /// The 0..=1 reduction depth for a tap value (run by `BusEnvelope`).
    #[inline(always)]
    pub fn depth(&self, tap: f64) -> f64 {
        ((self.threshold - super::detector::bus_level(tap.abs().min(1.))) * self.slope).clamp(0., 1.)
    }
}

/// Kontakt Transient Master: `attack * A + sustain * (S - 0.33 A) + gain`
/// dB through the native exponent table, from `level = [attack, sustain]`.
pub(super) struct TransientMix {
    pub attack: f32,
    pub sustain: f32,
    pub gain: f32,
}

impl Stage for TransientMix {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let [a, sustain] = each(f.level, |v| v as f32);
        let db = (sustain * self.sustain - self.sustain * f32::from_bits(0x3ea8_f5c3) * a) + a * self.attack + self.gain;
        let gain = super::super::filter::ar_kernel::exponential(db * f32::from_bits(0x411f_73da) + 1200.);
        f.gain = [f64::from(gain); 2];
    }
}

/// Kontakt Feedback Compressor's curve on the averaged detector `a`: 1 up to
/// the threshold `T`, `exp(c4 ln(a/T)^4)` up to the knee top, then the
/// power law `g0 (a / top)^(1 - s)` (`0x141a91d10`), through the native
/// exponent table.
#[derive(Clone, Copy)]
pub(super) struct FeedbackCurve {
    pub threshold: f32,
    pub knee_top: f32,
    pub slope: f32,
    pub g0: f32,
    pub quartic: f32,
}

/// `60 / ln 2`: table units per neper (0x1479ab5b0).
pub(super) const TABLE_PER_NEPER: f32 = f32::from_bits(0x42ad_1f97);

pub(super) fn table_exp(nepers_times_k: f32) -> f32 {
    super::super::filter::ar_kernel::exponential(nepers_times_k + 1200.)
}

impl FeedbackCurve {
    #[inline(always)]
    pub fn gain(&self, a: f64) -> f64 {
        if a > f64::from(self.threshold) {
            if a > f64::from(self.knee_top) {
                let y = (a / f64::from(self.knee_top)).ln() as f32;
                f64::from(table_exp(y * (1. - self.slope) * TABLE_PER_NEPER)) * f64::from(self.g0)
            } else {
                let y = (a / f64::from(self.threshold)).ln();
                let y2 = y * y;
                f64::from(table_exp((f64::from(self.quartic) * y2 * y2) as f32 * TABLE_PER_NEPER))
            }
        } else {
            1.
        }
    }
}
