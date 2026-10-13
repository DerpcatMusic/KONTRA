//! The one EQ kernel: a matched-response second-order design realised as a TPT state-variable
//! section. Every band of every format is `design(kind, Hz, Q, dB)` (or a native law's own
//! coefficients) followed by [`Svf::from_biquad`] and [`Svf::run`].
//!
//! Design: after Vicanek, "Matched Second Order Digital Filters" (2016). The poles are the exact
//! exponential map `z = exp(pT)` of the analogue prototype's poles. The numerator is then solved so
//! that the digital magnitude equals the analogue prototype's at DC, at the band frequency and at
//! Nyquist: no bilinear cramping of bells or shelves near Nyquist. First-order kinds use the same
//! construction with one pole and a two-term numerator (DC and Nyquist matched).
//!
//! Realisation: the Cytomic/Zavalishin TPT SVF (state `s1`, `s2`, output `m0 x + m1 v1 + m2 v2`).
//! Any biquad maps onto it exactly, its states are well behaved when coefficients change at
//! control cadence, and it costs one 3-state update per sample and channel (about 14 flops).
use sampler_ir::fx::eq::Kind;
use std::f64::consts::{PI, TAU};

/// `y = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Biquad {
    pub b: [f64; 3],
    pub a: [f64; 2],
}

/// Analogue prototype `(n2 s^2 + n1 s + n0) / (s^2 + d1 s + d0)` in units of the band frequency
/// (`s` = Laplace variable divided by the band's radian frequency).
struct Prototype {
    n: [f64; 3],
    d: [f64; 2],
}

fn prototype(kind: Kind, q: f64, boost_db: f64) -> Option<Prototype> {
    let a = 10f64.powf(boost_db / 40.);
    let r = a.sqrt();
    // n = [n2, n1, n0], d = [d1, d0].
    let (n, d) = match kind {
        Kind::Peak => ([1., a / q, 1.], [1. / (a * q), 1.]),
        // RBJ analogue shelves, monic denominators: DC gain A^2 (low) / HF gain A^2 (high).
        Kind::LowShelf => ([1., r / q, a], [1. / (q * r), 1. / a]),
        Kind::HighShelf => ([a * a, a * r / q, a], [r / q, a]),
        Kind::LowPass => ([0., 0., 1.], [1. / q, 1.]),
        Kind::HighPass => ([1., 0., 0.], [1. / q, 1.]),
        Kind::BandPass => ([0., 1. / q, 0.], [1. / q, 1.]),
        Kind::Notch => ([1., 0., 1.], [1. / q, 1.]),
        _ => return None,
    };
    Some(Prototype { n, d })
}

impl Prototype {
    /// Squared magnitude at normalised radian frequency `w` (1 = band frequency).
    fn mag2(&self, w: f64) -> f64 {
        let w2 = w * w;
        let [n2, n1, n0] = self.n;
        let (nr, ni) = (n0 - n2 * w2, n1 * w);
        let (dr, di) = (self.d[1] - w2, self.d[0] * w);
        (nr * nr + ni * ni) / (dr * dr + di * di)
    }
}

/// `z^2 + c1 z + c2` with the roots `exp(r wt)` of `s^2 + d1 s + d0` (`s` in units of the band
/// frequency, `wt` the band's radian frequency per sample).
fn map_roots(d1: f64, d0: f64, wt: f64) -> (f64, f64) {
    let disc = d1 * d1 - 4. * d0;
    if disc < 0. {
        let (sigma, wd) = (-d1 / 2. * wt, (-disc).sqrt() / 2. * wt);
        (-2. * sigma.exp() * wd.cos(), (2. * sigma).exp())
    } else {
        let r = disc.sqrt();
        let (z1, z2) = (((-d1 + r) / 2. * wt).exp(), ((-d1 - r) / 2. * wt).exp());
        (-(z1 + z2), z1 * z2)
    }
}

/// Matched second-order design. `hz` must be inside (0, rate/2). `None` for kinds that are not
/// filters (`Level`, first-order handled by [`first_order`]) or an unstable result.
pub(super) fn design(kind: Kind, rate: u32, hz: f64, q: f64, boost_db: f64) -> Option<Biquad> {
    let p = prototype(kind, q, boost_db)?;
    let t = 1. / f64::from(rate);
    let w0 = TAU * hz;
    // Poles: roots of s^2 + d1 s + d0 (units of w0) mapped through exp(pT).
    let (a1, a2) = map_roots(p.d[0], p.d[1], w0 * t);
    let a_poly = |phi: [f64; 3]| (1. + a1 + a2).powi(2) * phi[0] + (1. - a1 + a2).powi(2) * phi[1] - 4. * a2 * phi[2];
    let s = (PI * hz * t).sin().powi(2);
    let phi = [1. - s, s, 4. * (1. - s) * s];
    let nyquist = PI / t / w0;
    let b0sq = p.mag2(0.) * a_poly([1., 0., 0.]);
    let b1sq = p.mag2(nyquist) * a_poly([0., 1., 0.]);
    let mid = p.mag2(1.) * a_poly(phi);
    let b2sq = if phi[2] > 1e-12 { (mid - b0sq * phi[0] - b1sq * phi[1]) / phi[2] } else { 0. };
    let (r0, r1) = (b0sq.max(0.).sqrt(), b1sq.max(0.).sqrt());
    let w = (r0 + r1) / 2.;
    let b0 = (w + (w * w + b2sq).max(0.).sqrt()) / 2.;
    let b1 = (r0 - r1) / 2.;
    let b2 = if b0 > 0. { -b2sq / (4. * b0) } else { 0. };
    // Kinds whose analogue zeros are fixed keep them as digital zeros (a three-point match alone
    // would lose the stop-band slope): HP double zero at DC, notch zero pair on the band
    // frequency; only the gain is matched (at f0, or at the nearer end).
    let c = (TAU * hz * t).cos();
    let b = match kind {
        Kind::HighPass => {
            let k = (mid / (16. * s * s)).max(0.).sqrt();
            [k, -2. * k, k]
        }
        Kind::Notch if hz < 0.25 / t => {
            let k = r0 / (2. - 2. * c);
            [k, -2. * k * c, k]
        }
        Kind::Notch => {
            let k = r1 / (2. + 2. * c);
            [k, -2. * k * c, k]
        }
        // The three-point match has no real solution (a deep cut with a wide band): fall back to
        // exact pole-zero mapping of the numerator, gain matched at the band frequency.
        _ if w * w + b2sq < 0. && p.n[0] != 0. => {
            let (c1, c2) = map_roots(p.n[1] / p.n[0], p.n[2] / p.n[0], w0 * t);
            let w1 = TAU * hz * t;
            let (re, im) = (1. + c1 * w1.cos() + c2 * (2. * w1).cos(), c1 * w1.sin() + c2 * (2. * w1).sin());
            let k = (mid / (re * re + im * im)).max(0.).sqrt();
            [k, k * c1, k * c2]
        }
        _ => [b0, b1, b2],
    };
    let f = Biquad { b, a: [a1, a2] };
    (f.b.iter().chain(&f.a).all(|v| v.is_finite()) && a2.abs() < 1. && 1. + a1 + a2 > 0. && 1. - a1 + a2 > 0.).then_some(f)
}

/// First-order matched design (one real pole; DC and Nyquist gains matched). `kind` is a
/// first-order low/high pass or shelf; `boost_db` is the shelf boost.
pub(super) fn first_order(kind: Kind, rate: u32, hz: f64, boost_db: f64) -> Option<Biquad> {
    let a = 10f64.powf(boost_db / 40.);
    // Analogue |H(jw)| with the band frequency as unit; pole at -`pole`.
    let (mag, pole): (fn(f64, f64) -> f64, f64) = match kind {
        Kind::LowPass => (|_, w| 1. / (1. + w * w).sqrt(), 1.),
        Kind::HighPass => (|_, w| w / (1. + w * w).sqrt(), 1.),
        // (s + A) / (s + 1/A) and A^2 (s + 1/A) / (s + A).
        Kind::FirstOrderLowShelf => (|a, w| ((w * w + a * a) / (w * w + 1. / (a * a))).sqrt(), 1. / a),
        Kind::FirstOrderHighShelf => (|a, w| a * a * ((w * w + 1. / (a * a)) / (w * w + a * a)).sqrt(), a),
        _ => return None,
    };
    let nyquist = PI * f64::from(rate) / (TAU * hz);
    let z = (-pole * TAU * hz / f64::from(rate)).exp();
    let (m0, m1) = (mag(a, 0.), mag(a, nyquist));
    let (dc, ny) = (m0 * (1. - z), m1 * (1. + z));
    let f = Biquad { b: [(dc + ny) / 2., (dc - ny) / 2., 0.], a: [-z, 0.] };
    f.b.iter().all(|v| v.is_finite()).then_some(f)
}

/// TPT state-variable realisation of a biquad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Svf {
    a: [f64; 3],
    m: [f64; 3],
}

impl Svf {
    pub const PASS: Self = Self { a: [1., 0., 0.], m: [1., 0., 0.] };

    /// Same poles and numerator as `f` (denominator sums must be positive: a stable section).
    pub fn from_biquad(f: &Biquad) -> Option<Self> {
        let ([b0, b1, b2], [a1, a2]) = (f.b, f.a);
        let (plus, minus) = (1. + a1 + a2, 1. - a1 + a2);
        if !(plus > 0. && minus > 0.) {
            return None;
        }
        let g = (plus / minus).sqrt();
        let k = 2. * (1. - a2) / (g * minus);
        let d = 4. / minus; // 1 + g k + g^2
        let m0 = d * (b0 + b2 - b1) / 4.;
        let m2 = (d * b1 - 2. * m0 * (g * g - 1.)) / (2. * g * g);
        let m1 = (d * (b0 - b2) - 2. * g * k * m0) / (2. * g);
        let a_1 = 1. / (1. + g * (g + k));
        let c = Self { a: [a_1, g * a_1, g * g * a_1], m: [m0, m1, m2] };
        c.a.iter().chain(&c.m).all(|v| v.is_finite()).then_some(c)
    }

    pub fn to_array(self) -> [f64; 6] {
        [self.a[0], self.a[1], self.a[2], self.m[0], self.m[1], self.m[2]]
    }

    pub fn from_array([a0, a1, a2, m0, m1, m2]: [f64; 6]) -> Self {
        Self { a: [a0, a1, a2], m: [m0, m1, m2] }
    }

    /// Filter `x` in place; `s` is the section's `[s1, s2]` state.
    #[inline]
    pub fn run(&self, s: &mut [f64; 2], x: &mut [f64]) {
        let ([a1, a2, a3], [m0, m1, m2]) = (self.a, self.m);
        let [mut s1, mut s2] = *s;
        for sample in x {
            let v3 = *sample - s2;
            let v1 = a1 * s1 + a2 * v3;
            let v2 = s2 + a2 * s1 + a3 * v3;
            s1 = 2. * v1 - s1;
            s2 = 2. * v2 - s2;
            *sample = m0 * *sample + m1 * v1 + m2 * v2;
        }
        *s = [s1, s2];
    }
}

/// Complex response of a biquad at `hz` (tests, error tables).
#[cfg(test)]
pub(super) fn response(f: &Biquad, rate: u32, hz: f64) -> (f64, f64) {
    let w = TAU * hz / f64::from(rate);
    let poly = |c: [f64; 3]| {
        (c[0] + c[1] * w.cos() + c[2] * (2. * w).cos(), -c[1] * w.sin() - c[2] * (2. * w).sin())
    };
    let ((nr, ni), (dr, di)) = (poly(f.b), poly([1., f.a[0], f.a[1]]));
    let d = dr * dr + di * di;
    ((nr * dr + ni * di) / d, (ni * dr - nr * di) / d)
}

/// Magnitude of a biquad at `hz`.
#[cfg(test)]
pub(super) fn magnitude(f: &Biquad, rate: u32, hz: f64) -> f64 {
    let (re, im) = response(f, rate, hz);
    re.hypot(im)
}

/// Analogue prototype magnitude in dB at `f` Hz (tests): the target every kind's design matches.
#[cfg(test)]
pub(super) fn analog_db(kind: Kind, hz: f64, q: f64, boost_db: f64, f: f64) -> f64 {
    let a = 10f64.powf(boost_db / 40.);
    let w = f / hz;
    let m2 = match kind {
        Kind::FirstOrderLowShelf => (w * w + a * a) / (w * w + 1. / (a * a)),
        Kind::FirstOrderHighShelf => a.powi(4) * (w * w + 1. / (a * a)) / (w * w + a * a),
        _ => prototype(kind, q, boost_db).expect("filter kind").mag2(w),
    };
    10. * m2.log10()
}
