//! The generic dynamics kernel: the configurable textbook compressor, limiter,
//! expander and gate. Not a native law; formats that route a native effect
//! here must report it as generic.
//!
//! Per sample and channel: side-chain (optional first-order high-pass) ->
//! level (peak, 4x-oversampled true peak with the ITU-R BS.1770-4 Annex 2
//! interpolator, or RMS over a window) -> static curve in dB (quadratic knee)
//! -> smooth-branching one-pole of the gain change in dB (attack while it
//! grows, release while it falls; Giannoulis et al. 2012) -> gain on the
//! audio, delayed by the look-ahead. Level to dB and dB to gain use tables
//! (exponent bits plus a 1024-segment mantissa table), so the audio loop runs
//! no transcendental function.
// ponytail: the timing and curve laws are the textbook ones, not recovered
// from any original; confirm against a rendering before claiming parity.

use super::{Planar, ProcessorState, Shared, maximizer};
use sampler_ir::fx::dynamics::{Detector, Direction, Generic, Link, Topology};
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
fn table_db(level: f64) -> f64 {
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
fn table_gain(db: f64) -> f64 {
    let x = (db / DB_PER_BIT).clamp(-1000., 1000.);
    let whole = x.floor();
    let at = (x - whole) * SEGMENTS as f64;
    let i = (at as usize).min(SEGMENTS - 1);
    let t = &*EXP2;
    (t[i] + (t[i + 1] - t[i]) * (at - i as f64)) * f64::from_bits(((whole as i64 + 1023) as u64) << 52)
}
/// Boost ceilings. ponytail: upward compression and expansion of silence is
/// unbounded in theory; these bound it, raise if a real effect needs more.
const MAX_BOOST_DB: f64 = 60.;
const MAX_CUT_DB: f64 = 200.;

#[derive(Clone, Copy)]
pub(crate) struct Coefficients {
    direction: Direction,
    threshold_db: f64,
    threshold: f64,
    /// `1 - 1/ratio` (compression) or `ratio - 1` (expansion).
    slope: f64,
    knee_db: f64,
    /// Lowest level that reaches the knee (downward).
    knee_floor: f64,
    /// Largest reduction or boost in dB.
    range: f64,
    attack: f64,
    release: f64,
    rms: f64,
    highpass: f64,
}

fn pole(seconds: f64, rate: u32) -> f64 {
    let frames = seconds * f64::from(rate);
    if frames <= 0. { 0. } else { (-1. / frames).exp() }
}

impl Coefficients {
    /// `knobs` = [threshold dB, ratio, attack s, release s].
    pub(crate) fn new(g: &Generic, [threshold_db, ratio, attack, release]: [f64; 4], rate: u32) -> Self {
        let rate_f = f64::from(rate);
        let window = match g.detector {
            Detector::Rms { window } => window.seconds(),
            Detector::Peak | Detector::TruePeak => 0.,
        };
        Self {
            direction: g.direction,
            threshold_db,
            threshold: 10f64.powf(threshold_db / 20.),
            slope: match g.direction {
                Direction::Expand => ratio - 1.,
                _ => 1. - 1. / ratio,
            },
            knee_db: g.knee_db,
            knee_floor: 10f64.powf((threshold_db - g.knee_db / 2.) / 20.),
            range: if g.range_db > 0. { g.range_db } else { f64::INFINITY },
            attack: pole(attack, rate),
            release: pole(release, rate),
            rms: pole(window, rate),
            highpass: if g.sidechain_highpass_hz > 0. {
                (-std::f64::consts::TAU * g.sidechain_highpass_hz.min(rate_f / 2.) / rate_f).exp()
            } else {
                0.
            },
        }
    }

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

pub(super) struct Kernel {
    c: Coefficients,
    detector: Detector,
    feedback: bool,
    /// Look-ahead ring length in frames (0 = none; also none for feedback).
    ring: usize,
    /// True-peak history (12 cells) starts here.
    history: Option<usize>,
}

/// Cells the generic law needs besides the look-ahead ring.
pub(super) fn extra_cells(g: &Generic) -> usize {
    if g.detector == Detector::TruePeak { 12 } else { 0 }
}

impl Kernel {
    pub(super) fn new(g: &Generic, knobs: [f64; 4], rate: u32, cells: usize) -> Self {
        Self {
            c: Coefficients::new(g, knobs, rate),
            detector: g.detector,
            feedback: g.topology == Topology::Feedback,
            ring: if g.topology == Topology::Feedback { 0 } else { cells - extra_cells(g) },
            history: (g.detector == Detector::TruePeak).then(|| cells - 12),
        }
    }

    /// State: `z[c] = [gain change dB, mean square]`; `aux[0..4]` the
    /// high-pass memory (x1, y1 per channel); `aux[4..6]` the last gained
    /// sample (feedback); `aux[6]` the true-peak cursor; `delay_position`
    /// the look-ahead ring cursor.
    pub(super) fn process(
        &self,
        shared: &Shared,
        state: &mut ProcessorState,
        cells: &mut [[f64; 2]],
        block: &mut Planar,
        len: usize,
    ) {
        let c = &self.c;
        let linked = shared.link != Link::Off;
        let mut change = [state.z[0][0], state.z[1][0]];
        let mut mean_square = [state.z[0][1], state.z[1][1]];
        let mut hp = [state.aux[0], state.aux[1], state.aux[2], state.aux[3]];
        let mut last = [state.aux[4], state.aux[5]];
        let mut cursor = state.delay_position as usize;
        let upward = c.direction == Direction::Upward;
        let [left, right] = block;
        for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
            let audio = [*l, *r];
            let mut sc = if self.feedback { last } else { audio };
            if c.highpass > 0. {
                for ch in 0..2 {
                    let y = c.highpass * (hp[2 * ch + 1] + sc[ch] - hp[2 * ch]);
                    (hp[2 * ch], hp[2 * ch + 1]) = (sc[ch], y);
                    sc[ch] = y;
                }
            }
            if shared.link == Link::Mean {
                sc = [(sc[0] + sc[1]) * 0.5; 2];
            }
            let mut level = sc.map(f64::abs);
            if let Some(h) = self.history {
                let at = state.aux[6] as usize;
                cells[h + at] = sc;
                let at = (at + 1) % 12;
                state.aux[6] = at as f64;
                let history = &cells[h..h + 12];
                for ch in 0..2 {
                    let r = |j: usize| history[(at + j) % 12][ch] as f32;
                    level[ch] = maximizer::PHASES
                        .iter()
                        .fold(0f32, |m, p| m.max(maximizer::interpolate(p, r).abs()))
                        .into();
                }
            }
            if let Detector::Rms { .. } = self.detector {
                for ch in 0..2 {
                    mean_square[ch] = sc[ch] * sc[ch] + c.rms * (mean_square[ch] - sc[ch] * sc[ch]);
                    level[ch] = mean_square[ch].sqrt();
                }
            }
            if shared.link == Link::Max {
                level = [level[0].max(level[1]); 2];
            }
            let mut gain = [1.; 2];
            for ch in 0..if linked { 1 } else { 2 } {
                let target = c.target(level[ch]);
                let coefficient = if target > change[ch] { c.attack } else { c.release };
                let mut value = target + coefficient * (change[ch] - target);
                if value < 1e-12 {
                    value = 0.;
                }
                change[ch] = value;
                gain[ch] = if value == 0. {
                    1.
                } else if upward {
                    table_gain(value)
                } else {
                    table_gain(-value)
                };
            }
            if linked {
                (change[1], gain[1]) = (change[0], gain[0]);
            }
            let heard = if self.ring == 0 {
                audio
            } else {
                let heard = cells[cursor];
                cells[cursor] = audio;
                cursor += 1;
                if cursor == self.ring {
                    cursor = 0;
                }
                heard
            };
            last = [audio[0] * gain[0], audio[1] * gain[1]];
            *l = heard[0] * shared.factor(gain[0]);
            *r = heard[1] * shared.factor(gain[1]);
        }
        (state.z[0], state.z[1]) = ([change[0], mean_square[0]], [change[1], mean_square[1]]);
        state.aux[..4].copy_from_slice(&hp);
        state.aux[4..6].copy_from_slice(&last);
        state.delay_position = cursor as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::Time;

    fn generic(direction: Direction, knee_db: f64) -> Generic {
        Generic {
            threshold_db: -20.,
            ratio: 4.,
            knee_db,
            direction,
            detector: Detector::Peak,
            topology: Topology::FeedForward,
            lookahead: Time::ZERO,
            sidechain_highpass_hz: 0.,
            range_db: 0.,
        }
    }

    fn at(g: &Generic, ratio: f64) -> Coefficients {
        Coefficients::new(g, [g.threshold_db, ratio, 0., 0.], 48_000)
    }

    fn lin(db: f64) -> f64 {
        (db / DB_PER_NEPER).exp()
    }

    #[test]
    fn hard_knee_reduction_is_over_times_slope() {
        let g = generic(Direction::Downward, 0.);
        let c = at(&g, 4.);
        assert_eq!(c.target(lin(-30.)), 0.);
        assert!((c.target(lin(-8.)) - 12. * 0.75).abs() < 1e-5);
    }

    #[test]
    fn soft_knee_is_continuous_and_meets_the_hard_curve() {
        let g = generic(Direction::Downward, 12.);
        let c = at(&g, 4.);
        // Zero at the lower knee edge, the hard value at the upper one.
        assert!(c.target(lin(-26.)).abs() < 1e-5);
        assert!((c.target(lin(-14.)) - 6. * 0.75).abs() < 1e-5);
        // Centre: slope * (W/2)^2 / (2W) = slope * W / 8.
        assert!((c.target(lin(-20.)) - 0.75 * 12. / 8.).abs() < 1e-5);
        let mut previous = 0.;
        for step in 0..=240 {
            let value = c.target(lin(-30. + f64::from(step) * 0.05));
            assert!(value >= previous - 1e-12);
            previous = value;
        }
    }

    #[test]
    fn expander_lowers_the_level_below_the_threshold() {
        let g = generic(Direction::Expand, 0.);
        let c = at(&g, 3.);
        assert_eq!(c.target(lin(-10.)), 0.);
        assert!((c.target(lin(-30.)) - 10. * 2.).abs() < 1e-5);
    }

    #[test]
    fn upward_compression_raises_quiet_levels_to_a_ceiling() {
        let g = generic(Direction::Upward, 0.);
        let c = at(&g, 2.);
        assert!((c.target(lin(-40.)) - 20. * 0.5).abs() < 1e-5);
        assert_eq!(c.target(0.), MAX_BOOST_DB);
    }

    #[test]
    fn tables_track_libm() {
        for k in 0..2000 {
            let level = 1e-9 * 1.0137f64.powi(k);
            assert!((table_db(level) - 20. * level.log10()).abs() < 1e-5, "{level}");
            let db = -150. + k as f64 * 0.1;
            assert!((table_gain(db) / 10f64.powf(db / 20.) - 1.).abs() < 1e-6, "{db}");
        }
    }

    #[test]
    fn true_peak_sees_the_inter_sample_peak() {
        // fs/4 sine at 45 degrees: samples at 0.707, true peak 1 (+3 dB).
        let mut g = generic(Direction::Downward, 0.);
        g.threshold_db = -2.;
        let mut out = [0.; 2];
        for (i, detector) in [Detector::Peak, Detector::TruePeak].into_iter().enumerate() {
            g.detector = detector;
            let k = Kernel::new(&g, [-2., 100., 0., 0.], 48_000, extra_cells(&g));
            let mut cells = vec![[0.; 2]; extra_cells(&g)];
            let shared = Shared { link: Link::Off, makeup: 1., mix: 1. };
            let mut state = ProcessorState::default();
            let mut block = [[0.; super::super::super::BLOCK]; 2];
            for b in 0..4 {
                for (j, x) in block[0].iter_mut().enumerate() {
                    *x = (std::f64::consts::FRAC_PI_2 * (b * 64 + j) as f64 + std::f64::consts::FRAC_PI_4).sin();
                }
                block[1] = block[0];
                k.process(&shared, &mut state, &mut cells, &mut block, 64);
            }
            out[i] = block[0].iter().fold(0f64, |m, x| m.max(x.abs()));
        }
        assert!((out[0] - 0.7071).abs() < 1e-3, "{out:?}");
        assert!(out[1] < 0.84, "{out:?}");
    }
}
