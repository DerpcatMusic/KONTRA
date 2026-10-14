//! UVI DigitalEq band design (Workstation 4.0.9, 0x1412840f0), checked against the native routine
//! under emulation over every type, order, Q and bandwidth (same response to float32 storage).
//!
//! An order-`n` low-pass prototype is a product of `s^2 + B1 s + B0` factors (plus `s + 1` for odd
//! orders). `Q` spreads the poles: above 0.5 every pole pair turns by `acos(1 / 2Q) / (n / 2)` (Q
//! 0.707 is Butterworth at any order), below it the pairs split along the real axis by
//! `r = (x + sqrt(x^2 - 1))^(2 / n)`, `x = 1 / 2Q`. Order 3 has its own Q law. Each factor becomes a
//! section by type: LP, HP, shelves (`G = gain^(1 / 2n)` per section), or, for band pass, notch and
//! peak, the low-pass to band-pass transform `s -> (s^2 + 1) / (b s)` with `b = 2^(bw/2) - 2^(-bw/2)`
//! (`bw` the band's octaves), split into two sections. Sections are bilinear at `k = 1/tan(w0/2)`.
use super::kernel::Biquad;
use realfft::num_complex::Complex64 as C;
use sampler_ir::fx::eq::Kind;
use std::f64::consts::PI;

/// Slope index (0..=7) to filter order: 6, 12, 18, 24, 36, 48, 72, 96 dB/octave.
pub const ORDERS: [u8; 8] = [1, 2, 3, 4, 6, 8, 12, 16];

/// Most sections one band yields: two per prototype pair for the band-pass family.
pub(super) const MAX_SECTIONS: usize = 17;

fn band_pass_family(kind: Kind) -> bool {
    matches!(kind, Kind::BandPass | Kind::Notch | Kind::Peak)
}

/// How many biquads (first-order sections count as one) a band of `kind` and order `n` runs.
pub(super) fn sections(kind: Kind, n: u8) -> usize {
    let pairs = usize::from(n / 2);
    let odd = usize::from(n % 2);
    if band_pass_family(kind) { 2 * pairs + odd } else { pairs + odd }
}

/// `(B0, B1)` of each second-order prototype factor; `count` of them are valid.
fn prototype(n: u8, q: f64) -> ([(f64, f64); 8], usize) {
    let mut out = [(0., 0.); 8];
    let x = 0.5 / q;
    let nf = f64::from(n);
    let half = n / 2;
    let (mut phi, mut r) = (0., 1.);
    if x < 1. {
        if half > 0 {
            phi = x.acos() / f64::from(half);
        }
    } else if x > 1. {
        r = ((x * x - 1.).sqrt() + x).powf(2. / nf);
    }
    // Pole pairs turned by +-phi about the angles (1 - (i + first) / n) pi, i = 0, 2, ...
    let (first, end) = match n % 4 {
        2 => (2., half - 1),
        0 => (1., half),
        _ => (0., 0),
    };
    let mut count = 0;
    for i in (0..end).step_by(2) {
        let th = (1. - (f64::from(i) + first) / nf) * PI;
        out[count] = (r * r, -2. * r * (th + phi).cos());
        out[count + 1] = (1. / (r * r), -2. / r * (th - phi).cos());
        count += 2;
    }
    if n % 4 == 2 {
        out[count] = (1., if x >= 1. { r + 1. / r } else { -2. * (PI - phi).cos() });
        count += 1;
    } else if n == 3 {
        let x2 = x * x;
        let c1 = C::new((x2 - 2.) * x2, 0.).sqrt();
        let c2 = C::new(x2 - c1.re - 1., -c1.im).sqrt();
        let c4 = C::new(x2 + c1.re - 1., c1.im).sqrt();
        out[0] = (c2.re * c4.re - c2.im * c4.im, c2.re + c4.re);
        count = 1;
    }
    (out, count)
}

/// The two quadratic factors `s^2 + d1 s + d0` of `s^4 + a s^3 + b s^2 + a s + 1`, grouped as the
/// native root finder (0x1412881a0) groups them.
fn split(a: f64, b: f64) -> [(f64, f64); 2] {
    let disc = a * a - 4. * b + 8.;
    let root = C::new(disc, 0.).sqrt();
    let (d, h) = (root * 0.25, root * (-a * 0.5));
    let c = a * a * 0.5 - b - 2.;
    let (e1, e2) = ((C::new(c, 0.) - h).sqrt(), (C::new(c, 0.) + h).sqrt());
    let q = C::new(-0.25 * a, 0.);
    let r0 = -d - e1 * 0.5 + q;
    let r1 = -d + e1 * 0.5 + q;
    let r2 = d - e2 * 0.5 + q;
    let r3 = d + e2 * 0.5 + q;
    let pair = |u: C, v: C| (-(u.re + v.re), (u * v).re);
    if disc <= 0. { [pair(r0, r2), pair(r1, r3)] } else { [pair(r0, r1), pair(r2, r3)] }
}

/// Bilinear transform of `(n2 s^2 + n1 s + n0) / (d2 s^2 + d1 s + d0)` at `s = k (1 - z^-1) / (1 + z^-1)`.
fn bilinear(n: [f64; 3], d: [f64; 3], k: f64) -> Biquad {
    let [n2, n1, n0] = n;
    let [d2, d1, d0] = d;
    let kk = k * k;
    let a0 = kk * d2 + k * d1 + d0;
    Biquad {
        b: [(kk * n2 + k * n1 + n0) / a0, 2. * (n0 - kk * n2) / a0, (kk * n2 - k * n1 + n0) / a0],
        a: [2. * (d0 - kk * d2) / a0, (kk * d2 - k * d1 + d0) / a0],
    }
}

/// Section `index` of a band: `fnorm` is frequency / rate (native clamp 0.499), `gain_db` the band
/// gain (GainScale applied), `bandwidth` octaves. `None` past the last section.
pub(super) fn section(kind: Kind, n: u8, fnorm: f64, gain_db: f64, q: f64, bandwidth: f64, index: usize) -> Option<Biquad> {
    let k = 1. / (PI * fnorm.min(0.499)).tan();
    let big = (gain_db * 0.115_129_254_758_358).exp().powf(1. / (2. * f64::from(n)));
    let g2 = big * big;
    let beta = 2f64.powf(bandwidth * 0.5) - 2f64.powf(-bandwidth * 0.5);
    let (proto, count) = prototype(n, q);
    let family = band_pass_family(kind);
    let pairs = if family { 2 * count } else { count };
    if index < pairs {
        let (b0, b1) = proto[if family { index / 2 } else { index }];
        let d = |d1, d0| [1., d1, d0];
        return Some(match kind {
            Kind::LowPass => bilinear([0., 0., 1.], d(b1, b0), k),
            Kind::HighPass => bilinear([1., 0., 0.], d(b1, b0), k),
            Kind::LowShelf => bilinear([b0, b1 * big, g2], d(b1 / big, b0 / g2), k),
            Kind::HighShelf => bilinear([b0 * g2 * g2, b1 * g2 * big, g2], d(b1 * big, b0 * g2), k),
            _ => {
                let j = index % 2;
                let (p1, p0) = if kind == Kind::Peak {
                    split(beta * b1 / big, beta * beta * b0 / g2 + 2.)[j]
                } else {
                    split(b1 * beta, b0 * beta * beta + 2.)[j]
                };
                let num = match kind {
                    Kind::BandPass => [0., beta, 0.],
                    Kind::Notch => [1., 0., 1.],
                    _ => {
                        let (z1, z0) = split(beta * big * b1 / b0, g2 * beta * beta / b0 + 2.)[j];
                        [1., z1, z0]
                    }
                };
                bilinear(num, d(p1, p0), k)
            }
        });
    }
    if index > pairs || n % 2 == 0 {
        return None;
    }
    // Odd order: one first-order factor (s + 1), a second-order section for the band-pass family.
    let first = |n1: f64, n0: f64, d0: f64| Biquad {
        b: [(k * n1 + n0) / (k + d0), (n0 - k * n1) / (k + d0), 0.],
        a: [(d0 - k) / (k + d0), 0.],
    };
    Some(match kind {
        Kind::LowPass => first(0., 1., 1.),
        Kind::HighPass => first(1., 0., 1.),
        Kind::LowShelf => first(1., big, 1. / big),
        Kind::HighShelf => first(g2, big, big),
        Kind::BandPass => bilinear([0., beta, 0.], [1., beta, 1.], k),
        Kind::Notch => bilinear([1., 0., 1.], [1., beta, 1.], k),
        _ => bilinear([1., beta * big, 1.], [1., beta / big, 1.], k),
    })
}
