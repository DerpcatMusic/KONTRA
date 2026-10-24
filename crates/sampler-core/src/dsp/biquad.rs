//! Shared biquad coefficient designs. Each design only produces `b`/`a` coefficients; the
//! realisation (TDF2 in [`super::Biquad`], the TPT section of the EQ kernel, the Kontakt cascade
//! bank) stays with its owner so every structure keeps its native precision and operation order.
//!
//! - [`Design::Cookbook`]: RBJ cookbook by the bilinear transform. The polynomials ([`rbj`]) are
//!   shared by UVI's EQ routines, the native sampler filters and the Kontakt cascade; each caller
//!   forms the angle and `alpha` in its own rounding order.
//! - [`Design::Matched`]: after Vicanek, "Matched Second Order Digital Filters" (2016). The poles
//!   are the exact exponential map `z = exp(pT)` of the analogue prototype's poles. The numerator
//!   is then solved so that the digital magnitude equals the analogue prototype's at DC, at the
//!   band frequency and at Nyquist: no bilinear cramping of bells or shelves near Nyquist.
//! - [`Design::FirstOrder`]: the same construction with one pole and a two-term numerator (DC and
//!   Nyquist matched).
use sampler_ir::fx::eq::Kind;
use super::{Planar, flush};
use std::f64::consts::{PI, TAU};

/// How a Hertz/Q/dB band becomes coefficients.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Design {
    Cookbook,
    Matched,
    FirstOrder,
}

/// One band's coefficients by `design`; `q` is ignored by [`Design::FirstOrder`].
pub(crate) fn design(design: Design, kind: Kind, rate: u32, hz: f64, q: f64, boost_db: f64) -> Option<Coefficients> {
    match design {
        Design::Cookbook => cookbook(kind, rate, hz, q, boost_db),
        Design::Matched => matched(kind, rate, hz, q, boost_db),
        Design::FirstOrder => first_order(kind, rate, hz, boost_db),
    }
}

/// `y = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Coefficients {
    pub b: [f64; 3],
    pub a: [f64; 2],
}

impl Coefficients {
    /// `b / d[0]` over `1 + (d[1] / d[0]) z^-1 + (d[2] / d[0]) z^-2`.
    pub fn normalised(b: [f64; 3], d: [f64; 3]) -> Self {
        Self { b: b.map(|v| v / d[0]), a: [d[1] / d[0], d[2] / d[0]] }
    }

    /// Finite, with both poles strictly inside the unit circle.
    pub fn is_stable(&self) -> bool {
        let [a1, a2] = self.a;
        self.b.iter().chain(&self.a).all(|v| v.is_finite()) && a2.abs() < 1. && 1. + a1 + a2 > 0. && 1. - a1 + a2 > 0.
    }

    pub fn to_array(self) -> [f64; 5] {
        [self.b[0], self.b[1], self.b[2], self.a[0], self.a[1]]
    }
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
pub(crate) fn matched(kind: Kind, rate: u32, hz: f64, q: f64, boost_db: f64) -> Option<Coefficients> {
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
    let f = Coefficients { b, a: [a1, a2] };
    f.is_stable().then_some(f)
}

/// RBJ cookbook biquad by the bilinear transform: `A = 10^(dB/40)`, `alpha = sin(w0) / 2q`. The
/// coefficients of UVI's TwoBandEq/ThreeBandEq/ParametricEQ routines (0x141296b30, 0x141291cf0,
/// 0x1416403e0, 0x141640060, 0x141640220, 0x14163fd30, 0x14163fe30), checked under emulation.
pub(crate) fn cookbook(kind: Kind, rate: u32, hz: f64, q: f64, boost_db: f64) -> Option<Coefficients> {
    let a = 10f64.powf(boost_db / 40.);
    let (sin, cos) = (TAU * hz / f64::from(rate)).sin_cos();
    let shape = match kind {
        Kind::Peak => Shape::Peak,
        Kind::LowShelf => Shape::LowShelf,
        Kind::HighShelf => Shape::HighShelf,
        Kind::LowPass => Shape::LowPass,
        Kind::HighPass => Shape::HighPass,
        Kind::BandPass => Shape::BandPass,
        Kind::Notch => Shape::Notch,
        _ => return None,
    };
    let (b, d) = rbj(shape, cos, sin / (2. * q), a);
    Some(Coefficients::normalised(b, d))
}

/// The RBJ cookbook responses. Band-pass has unity peak gain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    AllPass,
    Peak,
    LowShelf,
    HighShelf,
}

/// Unnormalised RBJ cookbook numerator and denominator at angle cosine `cos`, `alpha` (sin / 2Q)
/// and amplitude `a = 10^(dB/40)` (only peaks and shelves read it). Low and high pass take the
/// bilinear `(1 -/+ cos) / 2` numerators.
pub(crate) fn rbj(shape: Shape, cos: f64, alpha: f64, a: f64) -> ([f64; 3], [f64; 3]) {
    let r = 2. * a.sqrt() * alpha;
    let (p, m) = (a + 1., a - 1.);
    let pole = [1. + alpha, -2. * cos, 1. - alpha];
    match shape {
        Shape::Peak => ([1. + alpha * a, -2. * cos, 1. - alpha * a], [1. + alpha / a, -2. * cos, 1. - alpha / a]),
        Shape::LowShelf => (
            [a * (p - m * cos + r), 2. * a * (m - p * cos), a * (p - m * cos - r)],
            [p + m * cos + r, -2. * (m + p * cos), p + m * cos - r],
        ),
        Shape::HighShelf => (
            [a * (p + m * cos + r), -2. * a * (m + p * cos), a * (p + m * cos - r)],
            [p - m * cos + r, 2. * (m - p * cos), p - m * cos - r],
        ),
        Shape::LowPass => ([(1. - cos) / 2., 1. - cos, (1. - cos) / 2.], pole),
        Shape::HighPass => ([(1. + cos) / 2., -(1. + cos), (1. + cos) / 2.], pole),
        Shape::BandPass => ([alpha, 0., -alpha], pole),
        Shape::Notch => ([1., -2. * cos, 1.], pole),
        Shape::AllPass => ([1. - alpha, -2. * cos, 1. + alpha], pole),
    }
}

/// First-order matched design (one real pole; DC and Nyquist gains matched). `kind` is a
/// first-order low/high pass or shelf; `boost_db` is the shelf boost.
pub(crate) fn first_order(kind: Kind, rate: u32, hz: f64, boost_db: f64) -> Option<Coefficients> {
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
    let f = Coefficients { b: [(dc + ny) / 2., (dc - ny) / 2., 0.], a: [-z, 0.] };
    f.b.iter().all(|v| v.is_finite()).then_some(f)
}

/// Frames `range` of both channels through one direct-form-I section, the native G_EQ cascade
/// record (`y = b0 x + b1 x1 + b2 x2 - a1 y1 - a2 y2`, process 0x141a595d0). `hist` holds x1, x2,
/// y1 and y2 for each channel.
pub(crate) fn run_df1(c: [f64; 5], hist: &mut [f64], block: &mut Planar, range: std::ops::Range<usize>) {
    for (channel, samples) in block.iter_mut().enumerate() {
        let h = &mut hist[4 * channel..4 * channel + 4];
        for x in &mut samples[range.clone()] {
            let input = *x;
            let y = flush(c[0] * input + c[1] * h[0] + c[2] * h[1] - c[3] * h[2] - c[4] * h[3]);
            *x = y;
            h[1] = h[0];
            h[0] = input;
            h[3] = h[2];
            h[2] = y;
        }
    }
}

/// Analogue prototype magnitude in dB at `f` Hz (tests): the target every kind's design matches.
#[cfg(test)]
pub(crate) fn analog_db(kind: Kind, hz: f64, q: f64, boost_db: f64, f: f64) -> f64 {
    let a = 10f64.powf(boost_db / 40.);
    let w = f / hz;
    let m2 = match kind {
        Kind::FirstOrderLowShelf => (w * w + a * a) / (w * w + 1. / (a * a)),
        Kind::FirstOrderHighShelf => a.powi(4) * (w * w + 1. / (a * a)) / (w * w + a * a),
        _ => prototype(kind, q, boost_db).expect("filter kind").mag2(w),
    };
    10. * m2.log10()
}

/// Bit fingerprints of every biquad coefficient design over a parameter grid, pinned before the
/// designs moved here: any numerical change to a design fails here.
#[cfg(test)]
mod fingerprints {
    use super::super::{biquad, filter::cascade, Biquad, FilterKind};
    use sampler_ir::fx::eq::Kind;

    const KINDS: [Kind; 10] = [
        Kind::Level,
        Kind::Peak,
        Kind::LowShelf,
        Kind::HighShelf,
        Kind::FirstOrderLowShelf,
        Kind::FirstOrderHighShelf,
        Kind::LowPass,
        Kind::HighPass,
        Kind::BandPass,
        Kind::Notch,
    ];

    struct Fnv(u64);
    impl Fnv {
        fn eat(&mut self, v: u64) {
            for byte in v.to_le_bytes() {
                self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
        fn coefficients(&mut self, c: Option<[f64; 5]>) {
            match c {
                Some(c) => c.iter().for_each(|v| self.eat(v.to_bits())),
                None => self.eat(u64::MAX),
            }
        }
    }

    fn grid(mut f: impl FnMut(u32, f64, f64, f64)) {
        for rate in [8_000, 44_100, 48_000, 96_000, 192_000] {
            let mut hz = vec![1e-3, 0.4999 * f64::from(rate), 0.5 * f64::from(rate), 0.6 * f64::from(rate)];
            let mut h = 10.;
            while h < 0.5 * f64::from(rate) {
                hz.push(h);
                h *= 1.13;
            }
            for &hz in &hz {
                for q in [0.05, 0.3, std::f64::consts::FRAC_1_SQRT_2, 1., 2.5, 10., 40.] {
                    for db in [-30., -6.5, 0., 0.1, 6., 24.] {
                        f(rate, hz, q, db);
                    }
                }
            }
        }
    }

    fn fingerprint(design: impl Fn(u32, f64, f64, f64, &mut Fnv)) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        grid(|rate, hz, q, db| design(rate, hz, q, db, &mut h));
        h.0
    }

    #[test]
    fn coefficient_designs_are_bit_identical() {
        let native = fingerprint(|rate, hz, q, db, h| {
            for kind in [
                FilterKind::LowPass,
                FilterKind::HighPass,
                FilterKind::BandPass,
                FilterKind::Notch,
                FilterKind::AllPass,
                FilterKind::Peak { gain_db: db },
                FilterKind::LowShelf { gain_db: db },
                FilterKind::HighShelf { gain_db: db },
                FilterKind::FirstOrderLowShelf { gain_db: db },
                FilterKind::FirstOrderHighShelf { gain_db: db },
            ] {
                h.coefficients(Biquad::new(rate, kind, hz, q).ok().map(|b| b.trace_coefficients()));
            }
        });
        let cookbook = fingerprint(|rate, hz, q, db, h| {
            KINDS.iter().for_each(|&k| h.coefficients(biquad::cookbook(k, rate, hz, q, db).map(biquad::Coefficients::to_array)));
        });
        let matched = fingerprint(|rate, hz, q, db, h| {
            KINDS.iter().for_each(|&k| h.coefficients(biquad::matched(k, rate, hz, q, db).map(biquad::Coefficients::to_array)));
        });
        let first_order = fingerprint(|rate, hz, _, db, h| {
            KINDS.iter().for_each(|&k| h.coefficients(biquad::first_order(k, rate, hz, db).map(biquad::Coefficients::to_array)));
        });
        let kontakt = fingerprint(|rate, hz, q, _, h| {
            (0..4).for_each(|mode| h.coefficients(Some(cascade::rbj(mode, hz / f64::from(rate), q))));
        });
        let got = [native, cookbook, matched, first_order, kontakt];
        assert_eq!(got, [0x8eb3_3122_67b6_322b, 0x573e_e65e_842e_3e4b, 0x4bfc_94dc_3f48_85dc, 0x3b58_838f_c2ba_3d5d, 0x396e_3b26_2f76_4695]);
    }

    /// The G_EQ direct-form-I record written out per sample (the form `run_df1` replaces), and
    /// the same block fed in split ranges: the history must carry across the splits bit-exactly.
    #[test]
    fn run_df1_matches_the_written_recurrence_across_splits() {
        let c = [0.3, -0.2, 0.1, 0.4, -0.05];
        let input = |ch: usize| -> [f64; 64] { std::array::from_fn(|i| ((i * 37 % 19) as f64 - 9.) * 0.07 * (1. + ch as f64)) };
        let mut reference: super::super::Planar = [input(0), input(1)];
        for samples in reference.iter_mut() {
            let (mut x1, mut x2, mut y1, mut y2) = (0., 0., 0., 0.);
            for x in samples.iter_mut() {
                let y = super::super::flush(c[0] * *x + c[1] * x1 + c[2] * x2 - c[3] * y1 - c[4] * y2);
                (x2, x1, y2, y1) = (x1, *x, y1, y);
                *x = y;
            }
        }
        for cut in [0usize, 1, 31, 32, 63, 64] {
            let mut block: super::super::Planar = [input(0), input(1)];
            let mut hist = [0.; 8];
            super::run_df1(c, &mut hist, &mut block, 0..cut);
            super::run_df1(c, &mut hist, &mut block, cut..64);
            assert_eq!(block.map(|c| c.map(f64::to_bits)), reference.map(|c| c.map(f64::to_bits)), "cut {cut}");
        }
    }
}
