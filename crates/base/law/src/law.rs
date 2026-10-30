use std::sync::Arc;

/// Curve from a normalized knob position `x` in `0..=1` to a parameter value.
///
/// [`Law::eval`] clamps `x` to `0..=1`. It never allocates or panics, because it runs on
/// the audio thread.
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
    /// Linear in decibels from `lo` to `hi`, evaluated as linear gain.
    Db { lo: f32, hi: f32 },
    /// `lo + (hi − lo) · x^exponent`: a power taper (a reverb damping knob: `x^¼`).
    Power { lo: f32, hi: f32, exponent: f32 },
    /// An integer power of the normalized input, preserving `powi` rounding.
    IntegerPower { exponent: i32 },
    /// A quarter-sine crossfade raised to `power`.
    Sine { power: f32 },
    /// Evenly spaced values over `0..=1`, linearly interpolated.
    Table(Arc<[f32]>),
    /// `(x, y, curve)` points with `x` strictly increasing. `curve` bends the segment that
    /// starts at its point; `0` is straight.
    Breakpoints(Arc<[(f32, f32, f32)]>),
    /// A named closed form. None are registered yet, so this variant cannot be built.
    Formula(FormulaId),
}

/// Names a closed form. Add a variant, its arm in [`Law::eval`] and a test when a law needs one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaId {}

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
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear { lo, hi } => lo * (1.0 - x) + hi * x,
            Self::Exp { lo, hi } => lo.powf(1.0 - x) * hi.powf(x),
            Self::ExpAffine {
                base,
                offset,
                factor,
                shift,
                scale,
            } => scale * (base.powf((x + offset) * factor) + shift),
            Self::Db { lo, hi } => 10f32.powf((lo * (1.0 - x) + hi * x) / 20.0),
            Self::Power { lo, hi, exponent } => lo + (hi - lo) * x.powf(*exponent),
            Self::IntegerPower { exponent } => x.powi(*exponent),
            Self::Sine { power } => (std::f32::consts::FRAC_PI_2 * x).sin().powf(*power),
            Self::Table(values) => table_at(values, x),
            Self::Breakpoints(points) => breakpoints_at(points, x),
            Self::Formula(id) => match *id {},
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

fn table_at(values: &[f32], x: f32) -> f32 {
    match values {
        [] => 0.0,
        [only] => *only,
        _ => {
            let pos = x * (values.len() - 1) as f32;
            let i = (pos as usize).min(values.len() - 2);
            let frac = pos - i as f32;
            let (Some(&a), Some(&b)) = (values.get(i), values.get(i + 1)) else {
                return 0.0;
            };
            // Written as a weighted sum so both endpoints come out exact.
            a * (1.0 - frac) + b * frac
        }
    }
}

fn breakpoints_at(points: &[(f32, f32, f32)], x: f32) -> f32 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 0.0;
    };
    if x <= first.0 {
        return first.1;
    }
    if x >= last.0 {
        return last.1;
    }
    for pair in points.windows(2) {
        let [a, b] = pair else { continue };
        if x <= b.0 {
            // A zero-width segment (invalid input) snaps to its end point.
            let span = b.0 - a.0;
            let t = if span > 0.0 { (x - a.0) / span } else { 1.0 };
            let s = curve_shape(t, a.2);
            return a.1 * (1.0 - s) + b.1 * s;
        }
    }
    last.1
}

fn curve_shape(t: f32, curve: f32) -> f32 {
    let curve = curve.clamp(-CURVE_LIMIT, CURVE_LIMIT);
    if curve.abs() < CURVE_EPSILON {
        t
    } else {
        ((curve * t).exp() - 1.0) / (curve.exp() - 1.0)
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
