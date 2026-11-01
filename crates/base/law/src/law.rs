use std::sync::Arc;

use crate::FormulaId;
use crate::real::Real;

/// Curve from a control position to a parameter value.
///
/// [`Law::eval`] clamps `x` to `0..=1`. It never allocates or panics, because it runs on
/// the audio thread. [`Law::eval_unclamped`] accepts source-specific units. The `_f64`
/// forms evaluate the same law in f64, for the engine's parameter round trips.
#[derive(Clone, Debug, PartialEq)]
pub enum Law {
    /// Straight line from `lo` to `hi`.
    Linear { lo: f32, hi: f32 },
    /// Geometric interpolation from `lo` to `hi` (frequency-style tapers).
    Exp { lo: f32, hi: f32 },
    /// `scale · (base^((x + offset) · factor) + shift)`, in that order.
    ExpAffine {
        base: f32,
        offset: f32,
        factor: f32,
        shift: f32,
        scale: f32,
    },
    /// `scale · 2^((x - origin) / divisor + offset)` for source-unit input.
    Exp2Index {
        origin: f32,
        divisor: f32,
        offset: f32,
        scale: f32,
    },
    /// `scale · base^((x - origin) / divisor)` in source units.
    PowIndex {
        base: f32,
        origin: f32,
        divisor: f32,
        scale: f32,
    },
    /// Linear in decibels from `lo` to `hi`, evaluated as linear gain.
    Db { lo: f32, hi: f32 },
    /// `lo + (hi − lo) · x^exponent`: a power taper (a reverb damping knob: `x^¼`).
    Power { lo: f32, hi: f32, exponent: f32 },
    /// An integer power of the input, preserving `powi` rounding; `bipolar` raises `2x − 1`
    /// instead.
    IntegerPower { exponent: i32, bipolar: bool },
    /// A quarter-sine crossfade raised to `power`.
    Sine { power: f32 },
    /// Evenly spaced values over `0..=1`, linearly interpolated.
    Table(Arc<[f32]>),
    /// `(x, y, curve)` points with `x` strictly increasing. `curve` bends the segment that
    /// starts at its point; `0` is straight.
    Breakpoints(Arc<[(f32, f32, f32)]>),
    /// A named closed form, for a curve no point list reproduces exactly.
    Formula(FormulaId),
}

/// Curve magnitudes beyond this overflow `f32::exp`, so [`Law::eval`] clamps to it.
const CURVE_LIMIT: f32 = 16.0;

/// Curves this close to zero are evaluated as straight lines.
const CURVE_EPSILON: f32 = 1e-3;

impl Law {
    /// The 0..=1 position this law maps to `y`, by bisection (for a
    /// monotonic law).
    #[must_use]
    pub fn invert(&self, y: f32) -> f32 {
        let rising = self.eval(1.0) >= self.eval(0.0);
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..32 {
            let mid = 0.5 * (lo + hi);
            if (self.eval(mid) < y) == rising {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// Maps a normalized position to the parameter value.
    #[must_use]
    pub fn eval(&self, x: f32) -> f32 {
        self.eval_unclamped(x.clamp(0.0, 1.0))
    }

    /// Maps an input without clamping; callers must supply the law's intended source units.
    ///
    /// Out-of-domain inputs can extrapolate or produce non-finite values depending on the shape.
    #[must_use]
    pub fn eval_unclamped(&self, x: f32) -> f32 {
        self.at(x)
    }

    /// [`Law::eval`] in f64.
    #[must_use]
    pub fn eval_f64(&self, x: f64) -> f64 {
        self.at(x.clamp(0.0, 1.0))
    }

    /// [`Law::eval_unclamped`] in f64.
    #[must_use]
    pub fn eval_unclamped_f64(&self, x: f64) -> f64 {
        self.at(x)
    }

    /// The input this law maps to `y`, in closed form; `None` for shapes without one.
    #[must_use]
    pub fn inverse(&self, y: f32) -> Option<f32> {
        self.inverse_at(y)
    }

    /// [`Law::inverse`] in f64.
    #[must_use]
    pub fn inverse_f64(&self, y: f64) -> Option<f64> {
        self.inverse_at(y)
    }

    fn inverse_at<T: Real>(&self, y: T) -> Option<T> {
        let of = T::of;
        match self {
            Self::Linear { lo, hi } => Some((y - of(*lo)) / (of(*hi) - of(*lo))),
            Self::ExpAffine {
                base,
                offset,
                factor,
                shift,
                scale,
            } => Some(
                (y / of(*scale) - of(*shift)).ln() / of(*base).ln() / of(*factor) - of(*offset),
            ),
            Self::PowIndex {
                base,
                origin,
                divisor,
                scale,
            } => {
                // Exact logs for the bases in use, so `20·log10(g)` keeps its rounding.
                let r = y / of(*scale);
                let log = match *base {
                    10.0 => r.log10(),
                    2.0 => r.log2(),
                    b => r.ln() / of(b).ln(),
                };
                Some(of(*divisor) * log + of(*origin))
            }
            Self::IntegerPower { exponent, bipolar } => {
                // An even power folds a bipolar input, so only its unipolar form inverts.
                let root = match (exponent, bipolar) {
                    (1, _) => y,
                    (2, false) => y.sqrt(),
                    (3, _) => y.cbrt(),
                    _ => return None,
                };
                Some(if *bipolar {
                    (root + of(1.0)) / of(2.0)
                } else {
                    root
                })
            }
            Self::Formula(id) => id.inverse(y.to_f64()).map(T::of_f64),
            Self::Exp { .. }
            | Self::Exp2Index { .. }
            | Self::Db { .. }
            | Self::Power { .. }
            | Self::Sine { .. }
            | Self::Table(_)
            | Self::Breakpoints(_) => None,
        }
    }

    fn at<T: Real>(&self, x: T) -> T {
        let of = T::of;
        let one = of(1.0);
        match self {
            Self::Linear { lo, hi } => of(*lo) * (one - x) + of(*hi) * x,
            Self::Exp { lo, hi } => of(*lo).powf(one - x) * of(*hi).powf(x),
            Self::ExpAffine {
                base,
                offset,
                factor,
                shift,
                scale,
            } => of(*scale) * (of(*base).powf((x + of(*offset)) * of(*factor)) + of(*shift)),
            Self::Exp2Index {
                origin,
                divisor,
                offset,
                scale,
            } => of(*scale) * ((x - of(*origin)) / of(*divisor) + of(*offset)).exp2(),
            Self::PowIndex {
                base,
                origin,
                divisor,
                scale,
            } => of(*scale) * of(*base).powf((x - of(*origin)) / of(*divisor)),
            Self::Db { lo, hi } => of(10.0).powf((of(*lo) * (one - x) + of(*hi) * x) / of(20.0)),
            Self::Power { lo, hi, exponent } => {
                of(*lo) + (of(*hi) - of(*lo)) * x.powf(of(*exponent))
            }
            Self::IntegerPower { exponent, bipolar } => {
                let x = if *bipolar { of(2.0) * x - one } else { x };
                x.powi(*exponent)
            }
            Self::Sine { power } => (T::FRAC_PI_2 * x).sin().powf(of(*power)),
            Self::Table(values) => table_at(values, x),
            Self::Breakpoints(points) => breakpoints_at(points, x),
            Self::Formula(id) => id.at(x),
        }
    }
}

/// The laws of one module: a law per parameter, shared tables, and module flags.
///
/// Keys are generic because the parameter and table ids belong to higher layers.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleLaw<P, T> {
    /// Law per parameter.
    pub params: Vec<(P, Law)>,
    /// Tables referenced by the laws.
    pub tables: Vec<(T, Arc<[f32]>)>,
    /// Bits owned by the module.
    pub flags: u32,
}

fn table_at<T: Real>(values: &[f32], x: T) -> T {
    match values {
        [] => T::of(0.0),
        [only] => T::of(*only),
        _ => {
            let pos = x * T::of_count(values.len() - 1);
            let i = pos.to_index().min(values.len() - 2);
            let frac = pos - T::of_count(i);
            let (Some(&a), Some(&b)) = (values.get(i), values.get(i + 1)) else {
                return T::of(0.0);
            };
            // Written as a weighted sum so both endpoints come out exact.
            T::of(a) * (T::of(1.0) - frac) + T::of(b) * frac
        }
    }
}

fn breakpoints_at<T: Real>(points: &[(f32, f32, f32)], x: T) -> T {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return T::of(0.0);
    };
    if x <= T::of(first.0) {
        return T::of(first.1);
    }
    if x >= T::of(last.0) {
        return T::of(last.1);
    }
    for pair in points.windows(2) {
        let [a, b] = pair else { continue };
        if x <= T::of(b.0) {
            // A zero-width segment (invalid input) snaps to its end point.
            let span = T::of(b.0) - T::of(a.0);
            let t = if span > T::of(0.0) {
                (x - T::of(a.0)) / span
            } else {
                T::of(1.0)
            };
            let s = curve_shape(t, T::of(a.2));
            return T::of(a.1) * (T::of(1.0) - s) + T::of(b.1) * s;
        }
    }
    T::of(last.1)
}

fn curve_shape<T: Real>(t: T, curve: T) -> T {
    let curve = curve.clamp(T::of(-CURVE_LIMIT), T::of(CURVE_LIMIT));
    if curve.abs() < T::of(CURVE_EPSILON) {
        t
    } else {
        ((curve * t).exp() - T::of(1.0)) / (curve.exp() - T::of(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEPS: u32 = 10_000;

    fn sweep() -> impl Iterator<Item = f32> {
        (0..=STEPS).map(|i| i as f32 / STEPS as f32)
    }

    /// One law of each kind, each non-decreasing over `0..=1`.
    fn increasing_laws() -> Vec<Law> {
        vec![
            Law::Linear { lo: -3.0, hi: 9.0 },
            Law::Exp {
                lo: 20.0,
                hi: 20_000.0,
            },
            Law::Db { lo: -60.0, hi: 6.0 },
            Law::Table(Arc::from([0.0, 0.1, 0.5, 0.6, 1.0])),
            Law::Breakpoints(Arc::from([
                (0.0, 0.0, 3.0),
                (0.3, 0.8, -2.0),
                (1.0, 1.0, 0.0),
            ])),
            Law::Breakpoints(Arc::from([(0.0, 0.0, 14.0), (1.0, 1.0, 0.0)])),
        ]
    }

    #[test]
    fn tapers_stay_monotonic() {
        for law in increasing_laws() {
            let mut prev = law.eval(0.0);
            for x in sweep() {
                let y = law.eval(x);
                // Allow float noise of a few ulps; a real reversal is far larger.
                assert!(
                    y >= prev - 1e-5 * prev.abs().max(1.0),
                    "{law:?} falls at x={x}: {prev} -> {y}"
                );
                prev = y;
            }
        }
    }

    #[test]
    fn endpoints_are_exact() {
        let table = Law::Table(Arc::from([0.25, 0.5, 2.0]));
        assert_eq!(table.eval(0.0), 0.25);
        assert_eq!(table.eval(1.0), 2.0);

        let linear = Law::Linear { lo: -3.0, hi: 9.0 };
        assert_eq!(linear.eval(0.0), -3.0);
        assert_eq!(linear.eval(1.0), 9.0);

        let exp = Law::Exp {
            lo: 20.0,
            hi: 20_000.0,
        };
        assert_eq!(exp.eval(0.0), 20.0);
        assert_eq!(exp.eval(1.0), 20_000.0);

        let points = Law::Breakpoints(Arc::from([(0.0, 0.1, 2.0), (1.0, 0.9, 0.0)]));
        assert_eq!(points.eval(0.0), 0.1);
        assert_eq!(points.eval(1.0), 0.9);
    }

    #[test]
    fn source_unit_evaluation_does_not_change_normalized_clamping() {
        let law = Law::Exp2Index {
            origin: 0.0,
            divisor: 60.0,
            offset: -20.0,
            scale: 1.0,
        };
        assert_eq!(law.eval(60.0), law.eval_unclamped(1.0));
        assert_eq!(law.eval_unclamped(60.0), (1.0f32 - 20.0).exp2());
    }

    #[test]
    fn power_taper_is_x_to_the_exponent() {
        let law = Law::Power {
            lo: 0.0,
            hi: 1.0,
            exponent: 0.25,
        };
        assert_eq!(law.eval(0.0), 0.0);
        assert!((law.eval(0.0625) - 0.5).abs() < 1e-6);
        assert_eq!(law.eval(1.0), 1.0);
    }

    #[test]
    fn table_interpolates_between_evenly_spaced_values() {
        let table = Law::Table(Arc::from([0.0, 1.0, 4.0]));
        assert!((table.eval(0.25) - 0.5).abs() < 1e-6);
        assert!((table.eval(0.75) - 2.5).abs() < 1e-6);
    }

    #[test]
    fn breakpoints_straight_segments_interpolate_linearly() {
        let law = Law::Breakpoints(Arc::from([
            (0.0, 0.0, 0.0),
            (0.5, 1.0, 0.0),
            (1.0, 1.0, 0.0),
        ]));
        assert!((law.eval(0.25) - 0.5).abs() < 1e-6);
        assert!((law.eval(0.75) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn breakpoints_curve_bends_the_segment() {
        let law = Law::Breakpoints(Arc::from([(0.0, 0.0, 2.0), (1.0, 1.0, 0.0)]));
        // (e^(2 * 0.5) - 1) / (e^2 - 1)
        let want = (1.0_f32.exp() - 1.0) / (2.0_f32.exp() - 1.0);
        assert!((law.eval(0.5) - want).abs() < 1e-5, "got {}", law.eval(0.5));
    }

    #[test]
    fn degenerate_laws_evaluate_without_panicking() {
        let laws = [
            Law::Table(Arc::from([])),
            Law::Table(Arc::from([3.0])),
            Law::Breakpoints(Arc::from([])),
            Law::Breakpoints(Arc::from([(0.5, 1.0, 0.0), (0.5, 2.0, 0.0)])),
            Law::Breakpoints(Arc::from([(0.0, 0.0, f32::MAX), (1.0, 1.0, f32::MIN)])),
            Law::Exp { lo: 0.0, hi: 1.0 },
        ];
        for law in &laws {
            for x in sweep() {
                std::hint::black_box(law.eval(x));
            }
        }
    }
}
