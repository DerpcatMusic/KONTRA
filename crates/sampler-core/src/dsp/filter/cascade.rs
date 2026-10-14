//! Section cascade: the filter engine's core for filters built from first- and second-order
//! direct-form sections whose coefficients a native law derives from the stored knobs.
//!
//! Kontakt's legacy BFilter classes (saved types 2..=4, 6..=12) are all one design, recovered
//! from Kontakt 8.13.1 and checked here against the native classes run under `tools/re/kemu.py`
//! (prepare, setter, process; vectors in `cascade_native_vectors.json`):
//!
//! - prepare (vtable slot 1: `FUN_1408de6e0` 1-pole, `FUN_1408de730` 2-pole, `FUN_1408db380`
//!   4-pole, `FUN_1408db410` BR4, `FUN_1408de790` 6-pole) stores `44011.976 / rate` and a
//!   Butterworth prototype per section: `FUN_14082a0b0(k, N)` is section `k` of order `N`
//!   (bilinear at `tan(w/2) = 0.5463`, `Q_k = 1 / (2 cos((2k+1) pi / 2N))`); the one-pole
//!   prototype is the constant `b0 = b1 = 0.26276`, pole `0.29341`.
//! - the setter (vtable slot 36: `FUN_14095a5a0` LP1, `14095a200` HP1, `14095a900` BP2,
//!   `14095b2d0` LP2, `14095ae80` HP2, `14095c9c0` LP4, `14095c410` HP4, `14095b720` BP4,
//!   `14095bdd0` BR4, `14095cf60` LP6) maps the knob through the 8-term exp series to a warp
//!   angle, scales each prototype's feedback taps by `1 + f r'` (`r' = 1 - (1 - res)^2`),
//!   moves it with the first-order allpass substitution of `FUN_1408e4970` (LP: pole
//!   `sin(.5 - t) / sin(.5 + t)`; HP: `-cos(t + .5) / cos(t - .5)` and `z -> -z`), and
//!   normalises the passband with the class's resonance-dependent gain.
//! - process (vtable slot 33: `FUN_1409097d0` LP4, `14090b4e0` HP4, `140909bf0` BP4,
//!   `140909780` BR4, `140aec430` LP6, `140aec040` LP2, `140909390` BP2) runs the sections in
//!   direct form I; LP4/BP4 clip the first section's stored output into a knee (HP4 its input)
//!   (`clamp(y, -1, 1) + 0.6 (y - clamp)`), LP6 the second one's (`0.7`); BR4 sums its two
//!   sections in parallel, in double coefficients.
//!
//! The kernel runs the native coefficient law in f32 exactly as the binary evaluates it
//! (truncated series, operation order) and the sections in f32 as the native does.

use super::super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::super::hiir::{Hiir, HiirState};
use super::super::Planar;

/// Frames between control evaluations (the engine's native-knob clock).
const QUANTUM: usize = 32;
/// Cells of `[f64; 2]` owned per instance: 3 sections x (x1, x2, y1, y2) per channel (or a
/// stage law's 8 values), the knobs designed for, the coefficients, the clocked knobs.
pub(crate) const CELLS: usize = 12 + 2 + 9 + 2 + 13;
const KNOBS: usize = 12;
const COEFS: usize = 14;
/// The clocked knobs and the started flag.
const CLOCK: usize = 23;

/// The native legacy classes (saved type in the comment).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Legacy {
    Lp1,    // 2
    Hp1,    // 3
    Bp2,    // 4
    Lp2,    // 6
    Hp2,    // 7
    Lp4,    // 8
    Hp4,    // 9
    Bp4,    // 10
    Br4,    // 11
    Lp6,    // 12
    VowelA, // 14
    VowelB, // 15
    Pro52,  // 16
    Ladder, // 17
    Versatile(sampler_ir::fx::filter::Versatile), // 19
    Sv(Sv),                                       // 50..=65
    Uvi(Uvi),
}

impl Legacy {
    pub fn from_saved(id: i32) -> Option<Self> {
        Some(match id {
            2 => Self::Lp1,
            3 => Self::Hp1,
            4 => Self::Bp2,
            6 => Self::Lp2,
            7 => Self::Hp2,
            8 => Self::Lp4,
            9 => Self::Hp4,
            10 => Self::Bp4,
            11 => Self::Br4,
            12 => Self::Lp6,
            14 => Self::VowelA,
            15 => Self::VowelB,
            16 => Self::Pro52,
            17 => Self::Ladder,
            _ => return None,
        })
    }
}

/// One direct-form section: `y = b0 x + b1 x1 + b2 x2 + c1 y1 + c2 y2` (feedback taps added).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Section {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub c1: f32,
    pub c2: f32,
}

/// The coefficient set a setter produces.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Design {
    pub sections: [Section; 3],
    pub count: usize,
    /// Sum the sections' outputs from the common input (BR4) instead of chaining them.
    pub parallel: bool,
    /// Output gain.
    pub gain: f32,
    /// Knee clip `clamp(v, -1, 1) + slope (v - clamp)`: (where, slope). `where` 0 is the input
    /// (HP4), `s + 1` the stored output of section `s`.
    pub clip: Option<(usize, f32)>,
}

// ----------------------------------------------------------------- the native law

/// Truncated series the setters use (constants at 0x144f385cc..0x144737728).
const C3: f32 = 0.166_666_67;
const C4: f32 = 0.041_666_668;
const C5: f32 = 0.008_333_334;
const C6: f32 = 0.001_388_888_9;
const C7: f32 = 0.000_198_412_7;
const C8: f32 = 2.480_158_7e-5;
const TINY: f32 = 1e-4;

/// Prototypes from `FUN_14082a0b0(k, N)` as stored, `[b1, b2, c1, c2, b0]` (c taps are
/// feedback, added).
const P2_0: [u32; 5] = [0x3e93_9041, 0x3e13_9041, 0x3f2d_700b, 0xbe82_0097, 0x3e13_9041];
const P4_0: [u32; 5] = [0x3e84_6b75, 0x3e04_6b75, 0x3f1b_a37a, 0xbe00_3bba, 0x3e04_6b75];
const P4_1: [u32; 5] = [0x3eb2_08f2, 0x3e32_08f2, 0x3f51_4087, 0xbf03_4979, 0x3e32_08f2];
const P6_0: [u32; 5] = [0x3e81_d5d5, 0x3e01_d5d5, 0x3f18_99d8, 0xbdd3_7d69, 0x3e01_d5d5];
const P6_2: [u32; 5] = [0x3ec1_45cb, 0x3e41_45cb, 0x3f63_295c, 0xbf24_6f26, 0x3e41_45cb];
/// `FUN_14082a2f0(k, 4)` (the f64 design BR4 keeps).
const D4_0: [f64; 5] = [
    0.258_632_319_841_800_96,
    0.129_316_159_920_900_48,
    0.607_963_192_186_333_5,
    -0.125_227_831_869_935_36,
    0.129_316_159_920_900_48,
];
const D4_1: [f64; 5] = [
    0.347_724_494_085_089_8,
    0.173_862_247_042_544_9,
    0.817_390_856_466_274_3,
    -0.512_839_844_636_453_8,
    0.173_862_247_042_544_9,
];
/// One-pole prototype (`FUN_1408de6e0`): `b0 = b1`, pole.
const P1_B: u32 = 0x3e86_1406;
const P1_POLE: u32 = 0x3e96_3993;

fn bits(p: [u32; 5]) -> [f32; 5] {
    p.map(f32::from_bits)
}

fn clamp01(v: f32) -> f32 {
    if v >= 0.0 {
        if v >= 1.0 { 1.0 } else { v }
    } else {
        0.0
    }
}

/// The knob through the setters' 8-term exp series, clamps and rate factor (f32).
fn knob_position(cutoff: f32, rate_factor: f32) -> f32 {
    let x = (f64::from(clamp01(cutoff)) * 6.15) as f32;
    let e = if x >= TINY || x <= -TINY {
        let x3 = x * x * x;
        let x4 = x3 * x;
        let x5 = x4 * x;
        let x6 = x5 * x;
        let x7 = x6 * x;
        x7 * x * C8 + x7 * C7 + x6 * C6 + x4 * C4 + x5 * C5 + x * x * 0.5 + x + x3 * C3 + 1.0
    } else {
        1.0
    };
    let v = clamp01((e - 1.0) * 0.002_572_831_7);
    clamp01((v + 0.002) * rate_factor)
}

/// Warp angle: `v pi/2` times the class scale (0.99, 0.92 or 0.82), each step rounded to f32.
fn angle(v: f32, scale: f64) -> f32 {
    ((f64::from(v) * std::f64::consts::FRAC_PI_2) as f32 as f64 * scale) as f32
}

/// Low-pass allpass pole `sin(.5 - t) / sin(.5 + t)`, `t = w * 0.998780904966900`.
fn lp_pole(w: f32) -> f32 {
    let t = (f64::from(w) * 0.998_780_904_966_9) as f32;
    let x = 0.5 - t;
    let num = if x >= TINY || x <= -TINY {
        let x3 = x * x * x;
        let x5 = x3 * x * x;
        (x5 * C5 - x * x5 * x * C7) + (x - x3 * C3)
    } else {
        0.0
    };
    let y = t + 0.5;
    let y3 = y * y * y;
    let y5 = y3 * y * y;
    let den = (y5 * C5 - y * y5 * y * C7) + 0.5 + (t - y3 * C3);
    num / den
}

/// High-pass allpass pole `-cos(t + .5) / cos(t - .5)`, `t = w * 0.99713317384107`.
fn hp_pole(w: f32) -> f32 {
    let t = (f64::from(w) * 0.997_133_173_841_07) as f32;
    let y = t + 0.5;
    let num = if y >= TINY || y <= -TINY {
        let y4 = y * y * y * y;
        (y * y4 * y * C6 - y4 * C4) + (y * y * 0.5 - 1.0)
    } else {
        -1.0
    };
    let z = t - 0.5;
    let den = if z >= TINY || z <= -TINY {
        let z4 = z * z * z * z;
        (z4 * C4 - z * z4 * z * C6) + (1.0 - z * z * 0.5)
    } else {
        1.0
    };
    num / den
}

/// `FUN_1408e4970`: the allpass substitution `z^-1 -> (z^-1 - a) / (1 - a z^-1)` of
/// `(b0 + b1 z^-1 + b2 z^-2) / (1 - c1 z^-1 - c2 z^-2)`, normalised to a unit z^0 feedback term.
fn substitute(b0: f64, b1: f64, b2: f64, c1: f64, c2: f64, a: f64) -> Section {
    let a2 = a * a;
    let raw = [
        ((b1 - (b0 + b0) * a) + a2 * b1) - (b2 + b2) * a,
        (a2 * b0 - b1 * a) + b2,
        ((a2 + 1.0) * c1 + a + a) - (c2 + c2) * a,
        c2 - (c1 * a + a2),
    ];
    let d = 1.0 / ((c1 * a + 1.0) - a2 * c2);
    let b0_ = ((b0 - b1 * a) + a2 * b2) as f32;
    let s = |v: f64| (f64::from(v as f32) * d) as f32;
    Section {
        b1: s(raw[0]),
        b2: s(raw[1]),
        c1: s(raw[2]),
        c2: s(raw[3]),
        b0: (f64::from(b0_) * d) as f32,
    }
}

/// One prototype section, its feedback taps scaled by `1 + f r'` in f32, substituted.
fn warp(p: [f32; 5], f1: f32, f2: f32, r: f32, a: f32) -> Section {
    let [b1, b2, _, _, b0] = p.map(f64::from);
    substitute(
        b0,
        b1,
        b2,
        f64::from((r * f1 + 1.0) * p[2]),
        f64::from((r * f2 + 1.0) * p[3]),
        f64::from(a),
    )
}

impl Section {
    fn mirror(mut self) -> Self {
        self.b1 = -self.b1;
        self.c1 = -self.c1;
        self
    }

    fn scale(mut self, d: f64) -> Self {
        let s = |v: f32| (f64::from(v) * d) as f32;
        self.b0 = s(self.b0);
        self.b1 = s(self.b1);
        self.b2 = s(self.b2);
        self
    }
}

/// One-pole substitution (`FUN_14095a5a0` / `14095a200` / `14095a900`): numerator taps and pole.
fn one_pole(a: f64) -> (f32, f32, f32) {
    let b = f64::from(f32::from_bits(P1_B));
    let p = f64::from(f32::from_bits(P1_POLE));
    let d = 1.0 / (p * a + 1.0);
    let b0 = (f64::from((b - b * a) as f32) * d) as f32;
    let b1 = (f64::from((b - b * a) as f32) * d) as f32;
    let fb = (f64::from((p + a) as f32) * d) as f32;
    (b0, b1, fb)
}

fn one_pole_low(a: f64) -> Section {
    let (b0, b1, fb) = one_pole(a);
    let dv = (1.0 - f64::from(fb)) / f64::from(b1 + b0);
    Section { b0: (f64::from(b0) * dv) as f32, b1: (f64::from(b1) * dv) as f32, c1: fb, ..Section::default() }
}

fn one_pole_high(a: f64) -> Section {
    let (b0, b1, fb) = one_pole(a);
    let (b1, fb_neg) = (-b1, -fb);
    let dv = (1.0 - f64::from(fb)) / f64::from(b0 - b1);
    Section { b0: (f64::from(b0) * dv) as f32, b1: (f64::from(b1) * dv) as f32, c1: fb_neg, ..Section::default() }
}

/// The coefficients a class's setter derives from its knobs at `rate` hertz.
pub(crate) fn design(class: Legacy, cutoff: f32, resonance: f32, rate: f32) -> Design {
    let rate_factor = (44_011.976_047_904_19 / f64::from(rate)) as f32;
    let v = knob_position(cutoff, rate_factor);
    let r = clamp01(1.0 - (1.0 - resonance) * (1.0 - resonance));
    let q = 1.0 - r * r;
    let gain = q * 0.25 + 0.75;
    let mut d = Design { gain: 1.0, count: 1, ..Design::default() };
    let s = &mut d.sections;
    match class {
        Legacy::Lp1 => s[0] = one_pole_low(f64::from(lp_pole(angle(v, 0.99)))),
        Legacy::Hp1 => s[0] = one_pole_high(f64::from(hp_pole(angle(v, 0.82)))),
        Legacy::Bp2 => {
            let w = angle(v, 0.82);
            s[0] = one_pole_low(f64::from(lp_pole(w)));
            s[1] = one_pole_high(f64::from(hp_pole(w)) * 0.99);
            d.count = 2;
        }
        Legacy::Lp2 | Legacy::Hp2 => {
            let high = class == Legacy::Hp2;
            let a = if high { hp_pole(angle(v, 0.92)) } else { lp_pole(angle(v, 0.99)) };
            let n = (1.0 - r) * 0.55 + 0.45;
            let sec = warp(bits(P2_0), 0.0, 2.687, r, a);
            s[0] = if high {
                let m = sec.mirror();
                let dv = ((1.0 - f64::from(m.c2 - m.c1)) / f64::from((m.b0 - m.b1) + m.b2)) * f64::from(n);
                m.scale(dv)
            } else {
                let dv = ((1.0 - f64::from(sec.c2 + sec.c1)) / f64::from((sec.b1 + sec.b0) + sec.b2))
                    * f64::from(n);
                sec.scale(dv)
            };
        }
        Legacy::Lp4 | Legacy::Hp4 => {
            let high = class == Legacy::Hp4;
            let a = if high { hp_pole(angle(v, 0.82)) } else { lp_pole(angle(v, 0.99)) };
            let n = f64::from(q * 0.35 + 0.65);
            let mut x = warp(bits(P4_0), 0.647, 6.294, r, a);
            let mut y = warp(bits(P4_1), 0.3, 0.796, r, a);
            let dv = if high {
                x = x.mirror();
                y = y.mirror();
                ((2.0 - (f64::from(x.c2 - x.c1) + f64::from(y.c2 - y.c1)))
                    / (f64::from((x.b0 - x.b1) + x.b2) + f64::from((y.b0 - y.b1) + y.b2)))
                    * n
            } else {
                ((2.0 - (f64::from(x.c2 + x.c1) + f64::from(y.c2 + y.c1)))
                    / (f64::from((x.b1 + x.b0) + x.b2) + f64::from((y.b1 + y.b0) + y.b2)))
                    * n
            };
            *s = [x.scale(dv), y.scale(dv), Section::default()];
            d.count = 2;
            d.gain = gain;
            // LP4 (FUN_140909bf0) knees the first section's output, HP4 (FUN_14090b4e0) the input.
            d.clip = Some((if high { 0 } else { 1 }, 0.6));
        }
        Legacy::Bp4 => {
            let w = angle(v, 0.82);
            let n = f64::from(q * 0.35 + 0.65);
            let x = warp(bits(P4_0), 0.647, 6.294, r, lp_pole(w));
            let y = warp(bits(P4_1), 0.3, 0.796, r, hp_pole(w)).mirror();
            let dx = ((1.0 - f64::from(x.c2 + x.c1)) / f64::from((x.b1 + x.b0) + x.b2)) * n;
            let dy = ((1.0 - f64::from(y.c2 - y.c1)) / f64::from((y.b0 - y.b1) + y.b2)) * n;
            *s = [x.scale(dx), y.scale(dy), Section::default()];
            d.count = 2;
            d.gain = gain;
            d.clip = Some((1, 0.6));
        }
        Legacy::Br4 => {
            // f64 coefficients (FUN_1408e4af0); the section math is the same.
            let w = angle(v, 0.82);
            let n = f64::from((1.0 - r) * 0.2 + 0.8);
            let wide = |p: [f64; 5], f1: f32, f2: f32, a: f32| {
                let [b1, b2, c1, c2, b0] = p;
                substitute_wide(
                    b0,
                    b1,
                    b2,
                    f64::from(r * f1 + 1.0) * c1,
                    f64::from(r * f2 + 1.0) * c2,
                    f64::from(a),
                )
            };
            let x = wide(D4_0, 0.647, 6.294, lp_pole(w));
            let mut y = wide(D4_1, 0.3, 0.796, hp_pole(w));
            y[0] = -y[0];
            y[2] = -y[2];
            let dx = ((1.0 - (x[3] + x[2])) / ((x[0] + x[4]) + x[1])) * n;
            let dy = ((1.0 - (y[3] - y[2])) / ((y[4] - y[0]) + y[1])) * n;
            let narrow = |c: [f64; 5], k: f64| Section {
                b1: (c[0] * k) as f32,
                b2: (c[1] * k) as f32,
                c1: c[2] as f32,
                c2: c[3] as f32,
                b0: (c[4] * k) as f32,
            };
            *s = [narrow(x, dx), narrow(y, dy), Section::default()];
            d.count = 2;
            d.parallel = true;
        }
        Legacy::VowelA | Legacy::VowelB => {
            // FUN_14095dd00 / FUN_14095e250: no exp series; two formant sections in parallel,
            // A warped low (unit DC gain), B mirrored high (unit Nyquist gain), both x `level`.
            let a_kind = class == Legacy::VowelA;
            let (k8, mb, bb) = if a_kind { (0.18, 0.068, 0.071_24) } else { (0.22, 0.083, 0.052) };
            let r1 = (f64::from(r) * k8 + 0.85) as f32;
            let r2 = if a_kind { (f64::from(r * 0.5) + 0.2) as f32 } else { (f64::from(r) * 0.4 + 0.2) as f32 };
            let level = f64::from((1.0 - r2) * 0.68 + 0.32);
            let u = clamp01((clamp01(cutoff) + 0.002) * rate_factor);
            let t = f64::from((f64::from(u) * std::f64::consts::FRAC_PI_2) as f32);
            let wa = if a_kind { (0.035_62 - t * 0.011_33) as f32 } else { (t * 0.0101 + 0.018_37) as f32 };
            let wb = (t * mb + bb) as f32;
            let x = warp(bits(P4_0), 0.647, 6.294, r1, lp_pole(wa));
            let y = warp(bits(P4_1), 0.3, 0.796, r2, hp_pole(wb)).mirror();
            let dx = ((1.0 - f64::from(x.c2 + x.c1)) / f64::from((x.b1 + x.b0) + x.b2)) * level;
            let dy = ((1.0 - f64::from(y.c2 - y.c1)) / f64::from((y.b0 - y.b1) + y.b2)) * level;
            *s = [x.scale(dx), y.scale(dy), Section::default()];
            d.count = 2;
            d.parallel = true;
        }
        Legacy::Uvi(Uvi::Biquad { mode, hertz, q }) => s[0] = uvi_biquad(mode, hertz, q, rate),
        Legacy::Pro52 | Legacy::Ladder | Legacy::Versatile(_) | Legacy::Sv(_) | Legacy::Uvi(_) => {
            unreachable!("not a section class")
        }
        Legacy::Lp6 => {
            let a = lp_pole(angle(v, 0.99));
            let n = f64::from(q * 0.55 + 0.45);
            let x = warp(bits(P6_0), 0.771, 7.786, r, a);
            let y = warp(bits(P2_0), 0.572, 2.388, r, a);
            let z = warp(bits(P6_2), 0.149, 0.473, r, a);
            let sum_c = |s: Section| f64::from(s.c2 + s.c1);
            let sum_b = |s: Section| f64::from((s.b1 + s.b0) + s.b2);
            let dv = ((3.0 - (sum_c(x) + sum_c(y) + sum_c(z))) / (sum_b(x) + sum_b(y) + sum_b(z))) * n;
            *s = [x.scale(dv), y.scale(dv), z.scale(dv)];
            d.count = 3;
            d.gain = gain;
            d.clip = Some((2, 0.7));
        }
    }
    d
}

/// `FUN_1408e4af0`: [`substitute`] kept in f64, `[b1, b2, c1, c2, b0]`.
fn substitute_wide(b0: f64, b1: f64, b2: f64, c1: f64, c2: f64, a: f64) -> [f64; 5] {
    let a2 = a * a;
    let d = 1.0 / ((c1 * a + 1.0) - a2 * c2);
    [
        (((b1 - (b0 + b0) * a) + a2 * b1) - (b2 + b2) * a) * d,
        ((a2 * b0 - b1 * a) + b2) * d,
        (((a2 + 1.0) * c1 + a + a) - (c2 + c2) * a) * d,
        (c2 - (c1 * a + a2)) * d,
        ((b0 - b1 * a) + a2 * b2) * d,
    ]
}

// ----------------------------------------------------------------- explicit stages

/// One explicit (Chamberlin) state-variable stage: `hp = soft(b) + (k x - (b d + l))`,
/// `b += hp g`, `l += b g + eps`; its output `x + (mode - x) wet` with
/// `mode = wl l + wb b + wh hp`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Stage {
    pub g: f32,
    pub d: f32,
    /// Input scale.
    pub k: f32,
    pub eps: f32,
    /// Soft clip of the band state added to the high pass ([`soft`] coefficients).
    pub shaper: (f32, f32),
    /// Low, band and high pass weights.
    pub weights: [f32; 3],
    pub wet: f32,
}

/// Up to three stages in series, a soft-clipped feedback of the last low pass subtracted from
/// the input, and an output gain smoothed toward `gain` by `smooth` per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Chamberlin {
    pub stages: [Stage; 3],
    pub count: usize,
    pub feedback: (f32, f32),
    pub gain: f32,
    pub smooth: f32,
}

/// `(a - c |s|) s` with `s` the input clamped to +-4 (Pro-53 feedback `NL`, Versatile shaper).
#[inline(always)]
fn soft(v: f32, (a, c): (f32, f32)) -> f32 {
    let s = v.clamp(-4.0, 4.0);
    (a - s.abs() * c) * s
}

/// The 2401-entry table `2^(i/60 - 20)` at the MIDI-note index `((x 100 + 20) 5) + 1381.88`
/// (`FUN_14095d940` / `FUN_14095d630`): 25.96 Hz at 0, 8372 Hz at 1.
fn note_hertz(cutoff: f32) -> f32 {
    super::ladder_kernel::exponential((clamp01(cutoff) * 100.0 + 20.0) * 5.0 + 1381.881_6)
}

/// Pro-53 (`FUN_14095d940` setter, `FUN_1409099a0` process): two stages, the second fed the
/// first's low pass scaled by its damping, `NL(l2)` fed back.
fn pro52(cutoff: f32, resonance: f32, rate: f32) -> Chamberlin {
    let r = clamp01(resonance);
    let k = if r >= 0.5 { r * 0.48 + 0.46 } else { r * 1.16 + 0.12 };
    let limit = (r * 0.293) * (r + 1.3075) + 0.7352;
    let g = (note_hertz(cutoff) * (6.283 / rate)).clamp(0.0, limit);
    let k2 = k * 0.8;
    let d2 = 2.0 - (k2 + k2);
    let low = |d, k, eps| Stage { g, d, k, eps, weights: [1.0, 0.0, 0.0], wet: 1.0, ..Stage::default() };
    Chamberlin {
        stages: [low(2.0 - (k + k), 1.0, 1e-7), low(d2, d2, 0.0), Stage::default()],
        count: 2,
        feedback: (0.133_35, 0.016_668_75),
        gain: 1.0,
        smooth: 1.0,
    }
}

/// 3x2 Versatile (`FUN_14095db50` cutoff, `FUN_140966230`.. resonances, assignment
/// `0x14096490b`, normaliser `FUN_14096f840`, process `FUN_140aef6e0`): three stages with the
/// band-state shaper, morphing LP -> BP -> HP, each blended by its wet amount.
fn versatile(v: &sampler_ir::fx::filter::Versatile, cutoff: f32, resonance: f32, rate: f32) -> Chamberlin {
    let k = (1.0 / f64::from(rate) * 6.283) as f32;
    let note = clamp01(cutoff) * 111.0 + 20.0;
    let shift = v.shift.map(|x| clamp01(x) * 80.0);
    let n1 = shift[0] + note;
    let notes = [note, n1, n1 + shift[1]];
    let res = [clamp01(resonance), clamp01(v.resonance[0]), clamp01(v.resonance[1])];
    let wet = v.wet.map(clamp01);
    let mut stages = [Stage::default(); 3];
    let mut comp = [0f32; 3];
    for i in 0..3 {
        let r = res[i];
        let q = if r > 0.5 { (f64::from(r - 0.5) * 0.66 + 0.65) as f32 } else { (f64::from(r) * 1.3) as f32 };
        let limit = ((f64::from(q) + 1.3075) * f64::from(q) * 0.293 + 0.7352) as f32;
        let arg = (notes[i] - 69.0) * 0.083_333_336;
        let g = ((f64::from(arg).exp2() * 440.0) as f32 * k).min(limit);
        let t = clamp01(v.types[i]);
        let weights = if t < 0.5 {
            let a = 0.5 - t;
            [a + a, t + t, 0.0]
        } else {
            let a = 1.0 - t;
            [0.0, a + a, ((t - 0.5) + t) - 0.5]
        };
        comp[i] = (f64::from(wet[i]) / (f64::from(1.0 - r) * 1.4 + 0.05) + f64::from(1.0 - wet[i])) as f32;
        stages[i] = Stage { g, d: 2.0 - (q + q), k: 1.0, eps: 0.0, shaper: (0.06, 0.0075), weights, wet: wet[i] };
    }
    let [fa, fb, fc] = stages.map(|s| s.g);
    let [a1, b1, c1] = comp.map(|c| c - 1.0);
    let inv = 1.0 / fa;
    let t1 = (a1 * b1) / ((fb * inv) * 6.0 - 5.0);
    let t2 = (c1 * b1) / ((fc / fb) * 6.0 - 5.0);
    let t3 = (c1 * a1) / ((fc * inv) * 6.0 - 5.0);
    let norm = (3.0 / f64::from((((t1 + t2) + t3) + comp[1]) + (comp[2] + comp[0]))) as f32;
    let gain = 10f64.powf(f64::from((clamp01(v.gain) - 0.5) * 3.0)) as f32;
    Chamberlin { stages, count: 3, feedback: (0.0, 0.0), gain: gain * norm, smooth: (1.0 / f64::from(rate) * 20.0) as f32 }
}

impl Chamberlin {
    /// One frame. `state = [b, l] x 3, feedback, smoothed gain`.
    #[inline(always)]
    fn tick(&self, state: &mut [f32; 8], input: f32) -> f32 {
        let mut y = input - state[6];
        let mut low = 0.0;
        for (i, st) in self.stages[..self.count].iter().enumerate() {
            let (b, l) = (state[2 * i], state[2 * i + 1]);
            let hp = soft(b, st.shaper) + (y * st.k - (b * st.d + l));
            let b = hp * st.g + b;
            let l = (b * st.g + st.eps) + l;
            state[2 * i] = b;
            state[2 * i + 1] = l;
            let mode = (b * st.weights[1] + hp * st.weights[2]) + l * st.weights[0];
            y += (mode - y) * st.wet;
            low = l;
        }
        state[6] = soft(low, self.feedback);
        state[7] += (self.gain - state[7]) * self.smooth;
        state[7] * y
    }
}

/// Legacy Ladder (`FUN_14095d630` setter, `FUN_140aec760` / `FUN_140aef200` process): four
/// one-pole stages with a zero at 0.3, a `v (1 - |v| / 100)` clip between stages and in the
/// feedback, run twice per frame (linear midpoint, then the frame). Output: the last state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Ladder2x {
    pub g: f32,
    pub r4: f32,
}

fn legacy_ladder(cutoff: f32, resonance: f32, rate: f32) -> Ladder2x {
    let g = (note_hertz(cutoff) * (6.283 / rate)).max(0.0).min(1.57);
    Ladder2x { g: g * 0.5, r4: clamp01(resonance) * 4.0 }
}

impl Ladder2x {
    /// One frame. `state = [x1, s1, s2, s3, s4, feedback]`.
    #[inline(always)]
    fn tick(&self, s: &mut [f32; 8], v: f32) -> f32 {
        let clip = |v: f32| if v.abs() <= 50.0 { v * (1.0 - v.abs() * 0.01) } else { 25f32.copysign(v) };
        let big_g = self.g * 0.769_230_8;
        let k = self.g - 1.0;
        // Each pass feeds the four stages and returns the last stage's output.
        let pass = |s: &mut [f32; 8], u: f32| {
            let mut t = u + s[1];
            s[1] = ((u * 0.3) - t * k) + 1e-12;
            for j in 2..5 {
                let a = clip(t) * big_g;
                t = a + s[j];
                s[j] = ((a * 0.3) - t * k) + 1e-12;
            }
            t
        };
        let t = pass(s, (((v + s[0]) * 0.5) - s[5]) * big_g);
        let t = pass(s, (v - (clip(t) * self.r4 + 1e-12)) * big_g);
        s[5] = clip(t) * self.r4 + 1e-12;
        s[0] = v;
        s[4]
    }
}

// ----------------------------------------------------------------- SV family

/// Kontakt's SV filters (`NI::KONTAKTFX::FilterSV1/SV2/SV4/SV6/MultiSV`, saved types 50..=65):
/// zero-delay state-variable stages whose high pass also sees the previous high pass, with
/// band and low states saturated near self-oscillation, and coefficient-domain controls ramped
/// per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sv {
    /// One-pole TPT (vtable 0x144fcf968, set 0x140b05d80, process 0x140b02280): low or high.
    One { high: bool },
    /// One stage (0x144fcfa30, set 0x140b05ee0, process 0x140b02400); mode 0 LP, 1 BP, 2 HP.
    Two { mode: u8 },
    /// Two stages (0x144fcfaf8, set 0x140b05fa0, process 0x140b02790); mode 0 LP, 1 BP, 2 HP,
    /// 3 notch.
    Four { mode: u8 },
    /// Three stages (0x144fcfbc0, set 0x140b06070, process 0x140b02cf0); modes as `Four`.
    Six { mode: u8 },
    /// Two stages a bandwidth apart (0x144fcf8a0, set 0x140b05d50, process 0x140b01d00):
    /// mode 0 parallel LP/HP, 1 parallel BP/BP, 2 serial LP/HP; `bandwidth` normalized.
    Multi { mode: u8, bandwidth: f32 },
}

/// `tan(pi f T)` by the truncated series of 0x140b04f60, in its order.
fn sv_g(f: f32, t: f32) -> f32 {
    let a = ((t * t) * 12.987_881) * (f * f) + 3.289_868_4;
    let a = (a * f) * ((f * t) * t) + 1.0;
    a * ((f * std::f32::consts::PI) * t)
}

fn sv_hertz(note: f32) -> f32 {
    super::ladder_kernel::exponential(note * 5.0 + 1381.881_6).max(0.0)
}

/// `1 - (1 - v)^3`.
fn sv_r(v: f32) -> f32 {
    let q = 1.0 - v;
    1.0 - (q * q) * q
}

impl Sv {
    /// Ramp duration (seconds): 8 ms for SV4, else 2 ms.
    fn ramp(self) -> f32 {
        if matches!(self, Self::Four { .. }) { 0.008 } else { 0.002 }
    }

    /// The control targets the setters derive (slot layout per class in `tick`).
    fn targets(self, u: f32, res: f32, t: f32) -> [f32; 8] {
        let mut out = [0f32; 8];
        match self {
            Self::One { .. } => out[0] = sv_g(sv_hertz((u * 115.0 + 20.0).min(160.0)), t),
            Self::Two { .. } => {
                let note = (u * 115.0 + 20.0).min(160.0);
                let r = sv_r(res);
                let n = (note * 0.007_462_686_4).clamp(0.0, 1.0);
                out[..5].copy_from_slice(&[
                    1.0,
                    sv_g(sv_hertz(note), t),
                    r,
                    600.0 - n * 576.0,
                    1.0 - ((((r * r) * r) * r) * r) * 0.5,
                ]);
            }
            Self::Four { mode } => {
                let note = u * 115.0 + 20.0;
                let n = note.min(134.0) * 0.007_462_686_4;
                let r = sv_r(res);
                let c = [0.77, 0.9334, 0.77, 0.88][usize::from(mode)];
                out[..4].copy_from_slice(&[
                    sv_g(sv_hertz(note), t),
                    r,
                    1.0 - r * c,
                    1600.0 - (2346.0 - n.abs() * 782.0) * n,
                ]);
            }
            Self::Six { mode } => {
                let note = (u * 115.0 + 20.0).min(160.0);
                let n = note.min(128.0) * 0.0078125;
                let r = sv_r(res * 0.9);
                let s = match mode {
                    0 | 2 => (1.212_500_1 - r.abs() * 0.2425) * r,
                    1 => ((2.0 - r) * r) * 0.991,
                    _ => (1.482 - r.abs() * 0.494) * r,
                };
                out[..4].copy_from_slice(&[
                    sv_g(sv_hertz(note), t),
                    r,
                    1.0 - s,
                    1600.0 - ((2.0 - n) * n) * 1576.0,
                ]);
            }
            Self::Multi { mode, bandwidth } => {
                let centre = u * 180.0 - 20.0;
                let spread = bandwidth * 31.5 + 0.5;
                let notes = [(centre - spread).min(160.0), (spread + centre).min(160.0)];
                let g = notes.map(|n| sv_g(sv_hertz(n).max(10.0), t));
                let l = notes.map(|n| 400.0 - (n.clamp(0.0, 134.0) * 0.007_462_686_4) * 376.0);
                let mut r = sv_r(res);
                let gain = if mode == 2 {
                    r = (((spread.clamp(0.0, 6.0) * 0.166_666_67) * 0.009_999_99) + 0.99) * r;
                    let r5 = ((r * r) * r) * (r * r);
                    let o = 1.0 - r5 * 0.5;
                    o * o
                } else {
                    0.7 - (((r * r) * r) * (r * r)) * 0.49
                };
                out[..7].copy_from_slice(&[g[0], g[1], r, l[0], l[1], spread, gain]);
            }
        }
        out
    }
}

/// The SV control ramp (`Ramp` of the model): every 32-frame block, a changed target ramps
/// linearly over `blocks` blocks; a step whose square is under 1e-15 snaps.
#[derive(Clone, Copy, Debug, Default)]
struct SvRamp {
    cur: [f32; 8],
    step: [f32; 8],
    target: [f32; 8],
    count: u32,
    dirty: bool,
}

const RAMP: usize = 25;

impl SvRamp {
    fn load(cells: &[[f64; 2]]) -> Self {
        let v = |i: usize| cells[RAMP + i / 2][i % 2] as f32;
        Self {
            cur: std::array::from_fn(|i| v(i)),
            step: std::array::from_fn(|i| v(8 + i)),
            target: std::array::from_fn(|i| v(16 + i)),
            count: v(24) as u32,
            dirty: v(25) != 0.0,
        }
    }

    fn store(&self, cells: &mut [[f64; 2]]) {
        let values = self.cur.iter().chain(&self.step).chain(&self.target).copied()
            .chain([self.count as f32, if self.dirty { 1.0 } else { 0.0 }]);
        for (i, v) in values.enumerate() {
            cells[RAMP + i / 2][i % 2] = f64::from(v);
        }
    }

    fn block(&mut self, blocks: u32, inverse: f32) {
        if self.dirty {
            self.dirty = false;
            let mut moving = false;
            for i in 0..8 {
                let d = (self.target[i] - self.cur[i]) * inverse;
                if f64::from(d * d) < 1e-15 {
                    self.step[i] = 0.0;
                    self.cur[i] = self.target[i];
                } else {
                    self.step[i] = d;
                    moving = true;
                }
            }
            if moving {
                self.count = blocks;
            }
        } else if self.count != 0 {
            self.count -= 1;
            if self.count == 0 {
                self.cur = self.target;
                self.step = [0.0; 8];
            }
        }
    }
}

/// `clamp(v, +-2L)` stored as `c (1 - |c| w)`.
#[inline(always)]
fn sv_sat(v: f32, l2: f32, w: impl Fn(f32) -> f32) -> f32 {
    let c = v.clamp(-l2, l2);
    (1.0 - w(c.abs())) * c
}

/// One zero-delay stage in the SV2/MultiSV order. `s = [h, b, l]`; returns (hp, bp, lp).
#[inline(always)]
fn sv_stage(s: &mut [f32], x: f32, g: f32, k: f32, l: f32) -> (f32, f32, f32) {
    let (l2, inv) = (l + l, 1.0 / l);
    let a = k + g;
    let gk = a * g;
    let [h, b, lo] = [s[0], s[1], s[2]];
    let hp = (x - ((((a + g) * b) + lo) + gk * h)) / (gk + 1.0);
    let bp = (h + hp) * g + b;
    let lp = (bp + b) * g + lo;
    s[0] = hp;
    s[1] = sv_sat(bp, l2, |c| (c * 0.25) * inv);
    s[2] = sv_sat(lp, l2, |c| (c * 0.25) * inv);
    (hp, bp, lp)
}

impl Cascade {
    /// The SV classes: their own ramp in place of the knob clock. State: cells `0..9` per
    /// channel (`[h, b, l]` per stage), the ramp from `RAMP`.
    fn sv(&self, sv: Sv, cells: &mut [[f64; 2]], block: &mut Planar, len: usize, knobs: impl Fn(usize) -> [f32; 2]) {
        let t = 1.0 / self.rate;
        let blocks = ((self.rate * sv.ramp() / 32.0 + 0.5) as u32).max(1);
        let inverse = 1.0 / (blocks * 32) as f32;
        let mut ramp = SvRamp::load(cells);
        if cells[CLOCK + 1][0] == 0.0 {
            let [u, r] = knobs(0);
            ramp = SvRamp { target: sv.targets(u, r, t), ..SvRamp::default() };
            ramp.cur = ramp.target;
            ramp.dirty = true;
            cells[CLOCK + 1][0] = 1.0;
        }
        let mut position = 0;
        while position < len {
            let frames = position..(position + QUANTUM).min(len);
            let [u, r] = knobs(position);
            let target = sv.targets(u, r, t);
            if target != ramp.target {
                ramp.target = target;
                ramp.dirty = true;
            }
            ramp.block(blocks, inverse);
            let start = ramp;
            for ch in 0..2 {
                ramp = start;
                let mut s: [f32; 9] = std::array::from_fn(|i| cells[i][ch] as f32);
                let mut last_r = -1.0;
                let mut sections = [(0f32, 0f32, 0f32); 2];
                for v in &mut block[ch][frames.clone()] {
                    for i in 0..8 {
                        ramp.cur[i] += ramp.step[i];
                    }
                    let c = ramp.cur;
                    let x = *v as f32;
                    let y = match sv {
                        Sv::One { high } => {
                            let g = c[0];
                            let lp = ((s[0] + s[0]) + g * x) / (g + 1.0);
                            let hp = x - lp;
                            s[0] = hp * g + s[0];
                            if high { hp } else { lp }
                        }
                        Sv::Two { mode } => {
                            let [gin, g, r, l, gain] = [c[0], c[1], c[2], c[3], c[4]];
                            let l = if l > 1e-20 { l } else { 9.999_999_7e-21 };
                            let k = (1.0 - r) + (1.0 - r);
                            let (hp, bp, lp) = sv_stage(&mut s[..3], x * gin, g, k, l);
                            let y = match mode { 0 => lp, 1 => bp + bp, _ => hp };
                            y * gain
                        }
                        Sv::Four { mode } | Sv::Six { mode } => {
                            let [g, r, gain, l] = [c[0], c[1], c[2], c[3]];
                            let (l2, w) = (l + l, 0.25 / l);
                            let four = matches!(sv, Sv::Four { .. });
                            let coefs = |k: f32| {
                                let a = k + g;
                                let gk = g * a;
                                (g + a, gk, 1.0 / (gk + 1.0))
                            };
                            if four {
                                // SV4 re-derives its sections only when R moves (and per block).
                                if r != last_r {
                                    let k2 = 1.0 - r * 0.666;
                                    sections = [coefs((1.0 - r) + (1.0 - r)), coefs(k2 + k2)];
                                    last_r = r;
                                }
                            } else {
                                sections = [coefs((1.0 - r) + (1.0 - r)); 2];
                            }
                            let mut x = x;
                            for j in 0..if four { 2 } else { 3 } {
                                let (a2, gk, d) = sections[j.min(1)];
                                let st = &mut s[3 * j..3 * j + 3];
                                let [h, b, lo] = [st[0], st[1], st[2]];
                                let hp = (x - (((b * a2) + lo) + h * gk)) * d;
                                let bp = (hp + h) * g + b;
                                let lp = (bp + b) * g + lo;
                                st[0] = hp;
                                st[1] = sv_sat(bp, l2, |c| c * w);
                                st[2] = sv_sat(lp, l2, |c| c * w);
                                x = match mode { 0 => lp, 1 => bp + bp, 2 => hp, _ => x - (bp + bp) };
                            }
                            x * gain
                        }
                        Sv::Multi { mode, .. } => {
                            let [g1, g2, r, l1, l2, spread, gain] = [c[0], c[1], c[2], c[3], c[4], c[5], c[6]];
                            let k = (1.0 - r) + (1.0 - r);
                            let (hp, bp, lp) = sv_stage(&mut s[..3], x, g1, k, l1);
                            let bb1 = bp + bp;
                            let o1 = match mode { 0 => lp, 1 => bb1, _ => hp };
                            // Serial: (o1 - x) * 1 + x, in f32 as the native mixes it.
                            let x2 = if mode == 2 { (o1 - x) + x } else { x };
                            let (hp2, bp2, lp2) = sv_stage(&mut s[3..6], x2, g2, k, l2);
                            let bb2 = bp2 + bp2;
                            let y = match mode {
                                0 => {
                                    let w = 0.5 - spread.min(3.0) * 0.166_666_67;
                                    (bb2 + bb1) * w + (hp2 + o1)
                                }
                                1 => bb2 + o1,
                                _ => lp2,
                            };
                            y * gain
                        }
                    };
                    *v = f64::from(y);
                }
                for (i, v) in s.iter().enumerate() {
                    cells[i][ch] = flushed(*v);
                }
            }
            position = frames.end;
        }
        ramp.store(cells);
    }
}

// ----------------------------------------------------------------- UVI laws

/// UVI Workstation 4.0.9 filter elements whose controls are constants here (hertz already key
/// tracked by the translator).
pub use sampler_ir::fx::filter::Uvi;

/// `BiquadFilter` coefficients (0x141102e30): RBJ cookbook in f32, Q scaled per mode
/// (`alpha = sin w / (s Q + 1)`, s = 24 HP, 12 LP, 8 BP / notch), cutoff under `0.499 rate`.
fn uvi_biquad(mode: u8, hertz: f32, q: f32, rate: f32) -> Section {
    let fc = hertz.min(rate * 0.499);
    let w = (fc / rate) * std::f32::consts::TAU;
    let (sn, cs) = w.sin_cos();
    let scale = [24.0, 12.0, 8.0, 8.0][usize::from(mode.min(3))];
    let alpha = sn / (q * scale + 1.0);
    let n = 1.0 / (alpha + 1.0);
    let a1 = (cs * -2.0) * n;
    let a2 = (1.0 - alpha) * n;
    let (b0, b1, b2) = match mode {
        0 => {
            let b0 = ((cs + 1.0) * 0.5) * n;
            (b0, -((cs + 1.0) * n), b0)
        }
        1 => {
            let b1 = (1.0 - cs) * n;
            (b1 * 0.5, b1, b1 * 0.5)
        }
        2 => (n * alpha, 0.0, -(n * alpha)),
        _ => (n, a1, n),
    };
    Section { b0, b1, b2, c1: -a1, c2: -a2 }
}

/// `MoogFilter` (coefficients 0x1412a9810, process 0x1412aa220): four one-pole stages with a
/// zero at -1 and global feedback, no saturation. `state = [x1, y1, y2, y3, y4]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Moog {
    r: f32,
    p: f32,
    k: f32,
}

impl Moog {
    fn new(hertz: f32, q: f32, rate: f32) -> Self {
        let x = ((hertz + hertz) / rate).min(1.0);
        let q1 = 1.0 - x;
        let p = ((x * 0.8) * q1) + x;
        let t = (((q1 * 5.6) * q1) + x) * q1 * 0.5;
        Self { r: (t + 1.0) * q.min(0.9), p, k: (p + p) - 1.0 }
    }

    #[inline(always)]
    fn tick(&self, s: &mut [f32; 8], v: f32) -> f32 {
        let xi = v - s[4] * self.r;
        let n1 = (xi + s[0]) * self.p - s[1] * self.k;
        let n2 = (s[1] + n1) * self.p - s[2] * self.k;
        let n3 = (s[2] + n2) * self.p - s[3] * self.k;
        let n4 = (s[3] + n3) * self.p - s[4] * self.k;
        s[..5].copy_from_slice(&[xi, n1, n2, n3, n4]);
        n4
    }
}

/// `Rez` (update 0x141100fd0, process 0x1411020e0): a Chamberlin state-variable filter in f64,
/// input scaled by the damping `a = 0.49 - 0.41 Q`, output by `1 / a`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rez {
    f: f64,
    a: f64,
    mode: u8,
}

impl Rez {
    fn new(mode: u8, hertz: f32, q: f32, rate: f32) -> Self {
        let x = hertz / rate;
        let x = if x >= 0.0 { x.min(0.5) } else { 0.0 };
        let f = 2.0 * (f64::from(x) * std::f64::consts::PI).sin();
        Self { f: f.clamp(0.0, 1.55), a: f64::from(f32::from_bits(0x3efa_e148) - q * f32::from_bits(0x3ed1_eb86)), mode }
    }

    /// `state = [lp, bp]`.
    #[inline(always)]
    fn tick(&self, s: &mut [f64; 2], v: f64) -> f64 {
        s[0] += s[1] * self.f;
        let hp = (self.a * v - s[0]) - s[1] * self.a;
        s[1] += hp * self.f;
        let y = match self.mode { 0 => s[0], 1 => hp, _ => s[1] };
        f64::from((y * (1.0 / self.a)) as f32)
    }
}

// ----------------------------------------------------------------- UVI Xpander

/// `XpanderFilter` output mixes per Mode (`[input, y1, y2, y3, y4]`, dumped from the native
/// update 0x1412cbc30; row 0 is the static LP1 row).
const XPANDER_MIX: [[f32; 5]; 37] = [
    [0., 1., 0., 0., 0.],
    [0., 0., 1., 0., 0.],
    [0., 0., 0., 1., 0.],
    [0., 0., 0., 0., 1.],
    [1., -1., 0., 0., 0.],
    [1., -2., 1., 0., 0.],
    [1., -3., 3., -1., 0.],
    [1., -4., 6., -4., 1.],
    [0., 2., -2., 0., 0.],
    [0., 0., 4., -8., 4.],
    [1., -2., 2., 0., 0.],
    [1., -4., 8., -8., 4.],
    [1., -3., 6., -4., 0.],
    [0., 1., -3., 6., -4.],
    [-1., 2., 0., 0., 0.],
    [1., -4., 4., 0., 0.],
    [0., 0., 0., 1., -1.],
    [0., 1., -3., 3., -1.],
    [0., 1., -2., 1., 0.],
    [0., 0., 1., -1., 0.],
    [0., -1., 2., 0., 0.],
    [0., 0., 1., -2., 2.],
    [1., -4., 7., -6., 2.],
    [0., 4., -12., 16., -8.],
    [1., -1., 1., 0., 0.],
    [1., 0., -1., 2., -1.],
    [-1., 4., -5.25, 2.5, 0.],
    [0., 0., 4., -8., 5.],
    [1., -4., 6.25, -4.5, 1.25],
    [0., 2., -6., 6.5, -2.5],
    [-0.25, 1., -2.25, 2.5, 0.],
    [0., 0., 0.25, -0.5, 1.25],
    [1., -4., 10., -12., 5.],
    [0., 0.5, -1.5, 3.5, -2.5],
    [1., -4., 10.25, -12.5, 6.25],
    [0.25, -1., 2.75, -3.5, 2.5],
    [1., -4., 7.25, -6.5, 2.5],
];

/// Cells: the kernel's 5 states, R and g of the last call (0 fresh), the drive-glide flag,
/// then the HIIR stage-0 memories (up then down).
pub(crate) const XPANDER_CELLS: usize = 8 + 64;
const XP_PREVIOUS: usize = 5;
const XP_GLIDED: usize = 7;
const XP_HIIR: usize = 8;

/// `XpanderFilter` (process 0x1412bf1c0, update 0x1412cbc30; kernels 0x1412c14e0 /
/// 0x1412bf450 / 0x1412c03f0 for Algorithm 0 and 0x1412c4cb0 / 0x1412c2580 / 0x1412c3870 for
/// Algorithm 1, one per distortion): a four-pole ladder with the input saturator inside the
/// feedback loop and a five-tap output mix, at 2x through the shared HIIR stage (edge
/// `min(20000 / rate, 0.48) rate`, 80 dB) when Algorithm 0 runs at or under 48 kHz or
/// Oversampling is on. Chain (wrapper 0x1412ca0a0 / 0x1412c8520 / 0x1412c9260): input times
/// the drive (a fresh unit's first frame at 1), up, kernel, down, times `1 / NL(drive)`. R
/// and g ramp across each call from the previous call's (0 on a fresh unit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Xpander {
    r: f32,
    g: f32,
    fat: f32,
    mix: [f32; 5],
    distortion: u8,
    zdf: bool,
    os2: bool,
    drive: f32,
    hiir: Hiir,
}

/// The ladder's input nonlinearity: 0 Padé `u (27 + u^2) / (27 + 9 u^2)`, 1 hard clip, 2 the
/// Padé of `1e-6 u^2` (near linear).
#[inline(always)]
fn xp_sat<const D: u8>(u: f32) -> f32 {
    match D {
        0 => (((u * u) + 27.0) * u) / (((u * u) * 9.0) + 27.0),
        1 => u.clamp(-1.0, 1.0),
        _ => {
            let t = (u * u) * f32::from_bits(0x3586_37be);
            ((t + 27.0) * u) / ((t * 9.0) + 27.0)
        }
    }
}

impl Xpander {
    pub(crate) fn new(u: Uvi, rate: f32) -> Option<Self> {
        let Uvi::Xpander { mode, hertz, q, distortion, drive, fat, algorithm, oversampling } = u else {
            return None;
        };
        let zdf = algorithm == 1;
        let os2 = (!zdf && rate <= 48_000.0) || oversampling;
        let rate2 = if os2 { rate + rate } else { rate };
        let fb = f32::from_bits;
        let (mut r, g) = if zdf {
            let x = (hertz / rate2).min(fb(0x3eff_7cee));
            let t = (x * fb(0x4049_0fdb)).tan();
            (q, t + t)
        } else {
            let x = (hertz / rate2).min(0.5);
            let x2 = x * x;
            let w = x * fb(0x40c9_0fdb);
            let t = x2 * fb(0x421d_e9e7);
            let r = ((((w * fb(0x3d57_7319)) + fb(0x3f80_5f07)) - (t * fb(0x3dbd_a512))) + ((t * w) * fb(0x3cb2_95ea)))
                * q;
            let g = (((w * fb(0x3f7d_3c36)) - (t * fb(0x3ed9_30be))) + ((t * w) * fb(0x3e0d_6a16)))
                - ((x2 * x2) * fb(0x41fb_dc68));
            (r, g)
        };
        if distortion == 2 && fb(0x469c_4000) < hertz {
            let h = rate2 * 0.5;
            r *= 1.0 - (hertz.min(h) - fb(0x469c_4000)) / h;
        }
        Some(Self {
            r,
            g,
            fat,
            mix: XPANDER_MIX[usize::from(mode.min(36))],
            distortion: distortion.min(2),
            zdf,
            os2,
            drive: 10f32.powf(drive * fb(0x3d4c_cccd)),
            hiir: Hiir::new(rate, f64::from((20_000.0 / rate).min(fb(0x3ef5_c28f))) * f64::from(rate), 80.0),
        })
    }

    fn makeup(&self, drive: f32) -> f32 {
        1.0 / match self.distortion {
            0 => xp_sat::<0>(drive),
            1 => xp_sat::<1>(drive),
            _ => xp_sat::<2>(drive),
        }
    }

    fn kernel(&self, s: &mut [f32; 5], x: &mut [f32], previous: [f32; 2]) {
        match (self.zdf, self.distortion) {
            (false, 0) => self.ladder::<0>(s, x, previous),
            (false, 1) => self.ladder::<1>(s, x, previous),
            (false, _) => self.ladder::<2>(s, x, previous),
            (true, 0) => self.tpt::<0>(s, x, previous),
            (true, 1) => self.tpt::<1>(s, x, previous),
            (true, _) => self.tpt::<2>(s, x, previous),
        }
    }

    /// Algorithm 1's output mix (the mono kernels' order).
    #[inline(always)]
    fn out(&self, t: f32, [y1, y2, y3, y4]: [f32; 4]) -> f32 {
        let m = self.mix;
        (((y4 * m[4]) + (y3 * m[3])) + (y2 * m[2])) + ((t * m[0]) + (y1 * m[1]))
    }

    /// Algorithm 0: one-sample feedback delay, `s = [s1, s2, s3, s4, y4]`. The engine is stereo, so
    /// this is the stereo SIMD kernel's per-channel order (0x1412c78d0 / 0x1412c60b0 / 0x1412c6cf0;
    /// the native takes it for 2 channels when bit 2 of the block size is clear).
    fn ladder<const D: u8>(&self, s: &mut [f32; 5], x: &mut [f32], [rc, gc]: [f32; 2]) {
        let (ka, kb) = (f32::from_bits(0x3f44_ec4f), f32::from_bits(0x3e6c_4ec6));
        let inv = 1.0 / x.len() as f32;
        let fat = self.fat * 4.0;
        let mut k = rc * 4.0;
        let mut gain = (rc * fat) + 1.0;
        let dgain = (((self.r * fat) + 1.0) - gain) * inv;
        let dk = ((self.r * 4.0) - k) * inv;
        let (mut a, mut b, mut c) = (gc * ka, gc * kb, 1.0 - gc);
        let da = ((self.g * ka) - a) * inv;
        let db = ((self.g * kb) - b) * inv;
        let dc = ((1.0 - self.g) - c) * inv;
        let [mut s1, mut s2, mut s3, mut s4, mut y4] = *s;
        for v in x {
            let feedback = k * y4;
            k += dk;
            let u = (gain * *v) - feedback;
            let t = if D == 2 {
                // The stereo kernels' form: 1000 P(u / 1000).
                let w = u / 1000.0;
                let ww = w * w;
                1000.0 * (((ww + 27.0) * w) / ((ww * 9.0) + 27.0))
            } else {
                xp_sat::<D>(u)
            };
            let y1 = (t * a) + s1;
            s1 = (t * b) + (c * y1);
            let y2 = (a * y1) + s2;
            let y3 = (a * y2) + s3;
            s2 = (c * y2) + (b * y1);
            y4 = (a * y3) + s4;
            s3 = (c * y3) + (b * y2);
            s4 = (c * y4) + (b * y3);
            gain += dgain;
            let m = self.mix;
            *v = (y4 * m[4]) + ((y3 * m[3]) + ((y2 * m[2]) + ((y1 * m[1]) + (t * m[0]))));
            (a, b, c) = (a + da, b + db, c + dc);
        }
        *s = [s1, s2, s3, s4, y4];
    }

    /// Algorithm 1: the zero-delay (TPT) ladder, coefficients re-derived at 64-frame sub-block
    /// edges and ramped inside; `s[..4]` are the `(1 + G)`-scaled stage states.
    fn tpt<const D: u8>(&self, s: &mut [f32; 5], x: &mut [f32], [rc, gc]: [f32; 2]) {
        let inv = 1.0 / x.len() as f32;
        let (dg, dr) = (self.g - gc, self.r - rc);
        let scale = 1.0 / ((gc * 0.5) + 1.0);
        let [mut s1, mut s2, mut s3, mut s4] = [s[0], s[1], s[2], s[3]].map(|v| scale * v);
        let mut g1p = None;
        for (index, chunk) in x.chunks_mut(64).enumerate() {
            let (i0, l) = (index * 64, chunk.len());
            let (r0, r1) = (i0 as f32 * inv, (i0 + l) as f32 * inv);
            let big0 = ((r0 * dg) + gc) * 0.5;
            let big1 = ((r1 * dg) + gc) * 0.5;
            let k0 = ((r0 * dr) + rc) * 4.0;
            let k1 = ((r1 * dr) + rc) * 4.0;
            let n0 = 1.0 / (big0 + 1.0);
            let mut gain = (k0 * self.fat) + 1.0;
            let mut c = (1.0 - big0) * n0;
            let mut h = n0 * big0;
            let mut norm = 1.0 / ((((h * h) * (h * h)) * k0) + 1.0);
            let p = big1 + 1.0;
            g1p = Some(p);
            let ip = 1.0 / p;
            let h1 = big1 * ip;
            let il = 1.0 / l as f32;
            let dh = (h1 - h) * il;
            let dc = (((1.0 - big1) * ip) - c) * il;
            let dn = (il / ((((h1 * h1) * (h1 * h1)) * k1) + 1.0)) - (norm / l as f32);
            let dk = (k1 - k0) * il;
            let dgain = (((k1 * self.fat) + 1.0) - gain) * il;
            let mut k = k0;
            for v in chunk {
                let est = ((((((h * s1) + s2) * h) + s3) * h) + s4) * k;
                k += dk;
                let u = (gain * *v) - est;
                let t = if D == 2 {
                    let tt = (u * u) * f32::from_bits(0x3586_37be);
                    (norm / ((tt * 9.0) + 27.0)) * ((tt + 27.0) * u)
                } else {
                    xp_sat::<D>(u) * norm
                };
                let v1 = t * h;
                let y1 = v1 + s1;
                let y2 = (y1 * h) + s2;
                s1 = (y1 * c) + v1;
                let v2 = y2 * h;
                let y3 = v2 + s3;
                s2 = (y2 * c) + (y1 * h);
                let v3 = y3 * h;
                h += dh;
                let y4 = v3 + s4;
                s3 = (y3 * c) + v2;
                s4 = (y4 * c) + v3;
                c += dc;
                gain += dgain;
                *v = self.out(t, [y1, y2, y3, y4]);
                norm += dn;
            }
        }
        if let Some(p) = g1p {
            *s = [p * s1, p * s2, p * s3, p * s4, 0.0];
        }
    }

    /// Drive, 2x up, kernel, down, makeup; state in `cells`.
    fn process(&self, cells: &mut [[f64; 2]], block: &mut Planar, len: usize) {
        const BLOCK: usize = super::super::BLOCK;
        let previous = [cells[XP_PREVIOUS][0] as f32, cells[XP_PREVIOUS + 1][0] as f32];
        // A fresh unit's drive glide starts at 1 and reaches the target after one frame.
        let glide = cells[XP_GLIDED][0] == 0.0 && (self.drive - 1.0).abs() >= f32::from_bits(0x3586_37bd);
        let makeup = self.makeup(self.drive);
        for ch in 0..2 {
            let mut s: [f32; 5] = std::array::from_fn(|i| cells[i][ch] as f32);
            let mut out: [f32; BLOCK] = std::array::from_fn(|i| block[ch][i] as f32 * self.drive);
            if glide && len > 0 {
                out[0] = block[ch][0] as f32;
            }
            if self.os2 {
                let mut h = HiirState::default();
                for (m, c) in h.first().into_iter().flatten().flatten().zip(&cells[XP_HIIR..]) {
                    *m = c[ch] as f32;
                }
                let (mut up, mut scratch) = ([0f32; 2 * BLOCK], [0f32; 2 * BLOCK]);
                self.hiir.up(&mut h, &out[..len], 1, &mut up, &mut scratch);
                self.kernel(&mut s, &mut up[..2 * len], previous);
                self.hiir.down(&mut h, &mut up[..2 * len], 1, &mut out);
                for (m, c) in h.first().into_iter().flatten().flatten().zip(&mut cells[XP_HIIR..]) {
                    c[ch] = flushed(*m);
                }
            } else {
                self.kernel(&mut s, &mut out[..len], previous);
            }
            for (i, (o, v)) in block[ch][..len].iter_mut().zip(&out).enumerate() {
                let makeup = if glide && i == 0 { self.makeup(1.0) } else { makeup };
                *o = f64::from(*v * makeup);
            }
            for (i, v) in s.iter().enumerate() {
                cells[i][ch] = flushed(*v);
            }
        }
        cells[XP_PREVIOUS] = [f64::from(self.r); 2];
        cells[XP_PREVIOUS + 1] = [f64::from(self.g); 2];
        cells[XP_GLIDED] = [1.0; 2];
    }
}

// ----------------------------------------------------------------- UVI section banks + comb

/// Sections a [`Bank`] holds: DigitalFilter's 16th-order band shapes give 16 plus one.
const BANK: usize = 17;
/// Cells: 2 transposed-direct-form-II states per section, then the first-order state.
const BANK_FIRST: usize = 2 * BANK;
const _: () = assert!(BANK_FIRST < CELLS);

/// Fixed sections in transposed direct form II, chained or summed, per channel. `wide` runs
/// DigitalFilter's f64 sections with an f32 store after each (0x141646fb0) and its f32
/// first-order stage (0x141648c00); otherwise the coefficients and states are f32
/// (0x141646fa0) and the sections are summed in order and scaled (VowelFilter 0x141345ef0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bank {
    sections: [[[f64; 5]; BANK]; 2],
    count: usize,
    first: [Option<[f32; 3]>; 2],
    wide: bool,
    sum: Option<f32>,
}

/// `VowelFilter` formant table (0x141ea1f30): 11 vowels x 4 formants, Hz.
const VOWELS: [[f64; 4]; 11] = [
    [700.0, 1100.0, 2450.0, 2900.0],
    [500.0, 1350.0, 1700.0, 2900.0],
    [250.0, 1700.0, 2500.0, 2700.0],
    [570.0, 840.0, 2400.0, 3500.0],
    [300.0, 870.0, 2240.0, 2650.0],
    [440.0, 1020.0, 2240.0, 2900.0],
    [520.0, 1190.0, 2390.0, 2900.0],
    [660.0, 1720.0, 2410.0, 2900.0],
    [530.0, 1840.0, 2480.0, 2900.0],
    [390.0, 1990.0, 2550.0, 2900.0],
    [270.0, 2290.0, 3010.0, 3500.0],
];

/// RBJ sections at sample rate 1 in f64 (0x14163fd30 LP, 0x14163ff50 BP at 0 dB peak,
/// 0x14163fe30 HP); above Nyquist LP and BP pass and HP is silent.
pub(crate) fn rbj(mode: u8, fn_: f64, q: f64) -> [f64; 5] {
    if fn_ > 0.5 {
        return [if mode == 2 { 0.0 } else { 1.0 }, 0.0, 0.0, 0.0, 0.0];
    }
    let (sn, cs) = (fn_ * std::f64::consts::TAU).sin_cos();
    let al = sn * (0.5 / q);
    let a0 = al + 1.0;
    let (a1, a2) = ((cs * -2.0) / a0, (1.0 - al) / a0);
    match mode {
        0 => {
            let t = 1.0 - cs;
            let b0 = (t * 0.5) / a0;
            [b0, t / a0, b0, a1, a2]
        }
        1 => [al / a0, 0.0 / a0, -(al / a0), a1, a2],
        _ => {
            let b0 = ((cs + 1.0) * 0.5) / a0;
            [b0, (-1.0 - cs) / a0, b0, a1, a2]
        }
    }
}

/// `(re, im)` square root, C99 `csqrt` branch cuts (the sign of a zero imaginary part kept).
fn csqrt((x, y): (f64, f64)) -> (f64, f64) {
    if x == 0.0 && y == 0.0 {
        return (0.0, y);
    }
    let t = ((x.abs() + x.hypot(y)) * 0.5).sqrt();
    if x >= 0.0 { (t, y / (t + t)) } else { (y.abs() / (t + t), t.copysign(y)) }
}

/// 0x141643f60: analog `(p2 s^2 + p3 s + p4) / (s^2 + p5 s + p6)` through the bilinear map.
fn bilinear(p2: f64, p3: f64, p4: f64, p5: f64, p6: f64, w: f64) -> [f64; 5] {
    let k = 1.0 / (w * 0.5).tan();
    let k2 = k * k;
    let n2 = k2 * p2;
    let n = 1.0 / (k * p5 + p6 + k2);
    [
        (k * p3 + p4 + n2) * n,
        ((p4 - n2) + (p4 - n2)) * n,
        ((p4 - k * p3) + n2) * n,
        ((p6 - k2) + (p6 - k2)) * n,
        ((p6 - k * p5) + k2) * n,
    ]
}

/// 0x1416440a0: analog `(p2 s^2 + p3 s + p4) / (p5 s^2 + p6 s + p7)`.
fn bilinear2(p2: f64, p3: f64, p4: f64, p5: f64, p6: f64, p7: f64, w: f64) -> [f64; 5] {
    let k = 1.0 / (w * 0.5).tan();
    let p5 = k * k * p5;
    let p2 = k * k * p2;
    let n = 1.0 / (k * p6 + p5 + p7);
    [(k * p3 + p2 + p4) * n, ((p4 - p2) + (p4 - p2)) * n, ((p2 - k * p3) + p4) * n, ((p7 - p5) + (p7 - p5)) * n, ((p5 - k * p6) + p7) * n]
}

/// 0x1412881a0: the four roots of `s^2 + B s + C` under the band transform.
fn band_roots(b: f64, c: f64) -> [(f64, f64); 4] {
    let d4 = (b * b - c * 4.0) + 8.0;
    let (sre, sim) = csqrt((d4, 0.0));
    let (q8, q10) = (sre * 0.25, sim * 0.25);
    let d3 = sre * -b * 0.5;
    let d2 = sim * -b * 0.5;
    let d7 = (b * b * 0.5 - c) - 2.0;
    let w1 = csqrt((d7 - d3, -d2));
    let pb = b * -0.25;
    let w2 = csqrt((d3 + d7, d2));
    let z0 = ((-q8 - w1.0 * 0.5) + pb, -q10 - w1.1 * 0.5);
    let za = ((-q8 + w1.0 * 0.5) + pb, w1.1 * 0.5 + -q10);
    let zb = ((q8 - w2.0 * 0.5) + pb, q10 - w2.1 * 0.5);
    let z3 = (w2.0 * 0.5 + q8 + pb, w2.1 * 0.5 + q10);
    if d4 <= 0.0 { [z0, zb, za, z3] } else { [z0, za, zb, z3] }
}

fn re_mul(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.0 - a.1 * b.1
}

/// `DigitalFilter` design (0x1412840f0): kind 0 LP, 1 HP, 2 BP, 3 notch, 4 low shelf, 5 high
/// shelf, 6 peak; order `n`, `fn_ = fc / rate`. Sections plus the odd orders' first-order stage.
fn digital_design(kind: u8, n: usize, fn_: f64, gain_db: f32, q: f64, bw: f64) -> (Vec<[f64; 5]>, Option<[f32; 3]>) {
    use std::f64::consts::PI;
    let nf = n as f64;
    let w = fn_ * (2.0 * PI);
    let r = 0.5 / q;
    let mut phi = 0.0;
    let mut k = 1.0;
    if r >= 1.0 {
        if r > 1.0 {
            k = ((r * r - 1.0).sqrt() + r).powf(1.0 / nf + 1.0 / nf);
        }
    } else if n / 2 > 0 {
        phi = r.acos() / (n / 2) as f64;
    }
    let r2 = r * r;
    let g = f64::from((gain_db * 0.115_129_25).exp());
    let a = g.powf(1.0 / (nf + nf));
    let a2 = a * a;
    let d = 2f64.powf(bw * 0.5) - 2f64.powf(bw * -0.5);
    let (kk, ikk) = (k * k, 1.0 / (k * k));
    let (mut bs, mut cs) = ([0f64; 16], [0f64; 16]);
    let pairs = match n % 4 {
        2 => {
            let last = n / 2 - 1;
            let mut i = 0;
            while i < last {
                let th = (1.0 - (i + 2) as f64 * (1.0 / nf)) * PI;
                (bs[i], cs[i]) = ((th + phi).cos() * (k * -2.0), kk);
                (bs[i + 1], cs[i + 1]) = ((th - phi).cos() * (-2.0 / k), ikk);
                i += 2;
            }
            cs[last] = 1.0;
            bs[last] = if r >= 1.0 { 1.0 / k + k } else { (PI - phi).cos() * -2.0 };
            n / 2
        }
        0 => {
            let mut i = 0;
            while i < n / 2 {
                let th = (1.0 - (i + 1) as f64 * (1.0 / nf)) * PI;
                (bs[i], cs[i]) = ((th + phi).cos() * (k * -2.0), kk);
                (bs[i + 1], cs[i + 1]) = ((th - phi).cos() * (-2.0 / k), ikk);
                i += 2;
            }
            n / 2
        }
        _ if n == 3 => {
            let t = (r2 - 2.0) * r2;
            let z1 = csqrt((t, 0.0));
            let zb = csqrt(((r2 - z1.0) - 1.0, -z1.1));
            let zc = csqrt(((r2 + z1.0) - 1.0, z1.1));
            bs[0] = zb.0 + zc.0;
            cs[0] = zb.0 * zc.0 - zb.1 * zc.1;
            1
        }
        _ => n / 2,
    };
    let mut sections = Vec::with_capacity(BANK);
    for j in 0..pairs {
        let (b, c) = (bs[j], cs[j]);
        match kind {
            0 => sections.push(bilinear(0.0, 0.0, 1.0, b, c, w)),
            1 => sections.push(bilinear(1.0, 0.0, 0.0, b, c, w)),
            4 => sections.push(bilinear(c, b * a, a2, b / a, c / a2, w)),
            5 => sections.push(bilinear(c * a2 * a2, b * a2 * a, a2, b * a, c * a2, w)),
            2 | 3 => {
                let z = band_roots(b * d, c * d * d + 2.0);
                for (u, v) in [(z[0], z[1]), (z[2], z[3])] {
                    let (p5, p6) = (-u.0 - v.0, re_mul(u, v));
                    sections.push(if kind == 2 { bilinear(0.0, d, 0.0, p5, p6, w) } else { bilinear(1.0, 0.0, 1.0, p5, p6, w) });
                }
            }
            _ => {
                let ic = 1.0 / c;
                let nz = band_roots(d * a * (b * ic), (d * ic) * (a2 * d) + 2.0);
                let pz = band_roots((b / a) * d, d * d * (c / a2) + 2.0);
                for (n0, n1, q0, q1) in [(nz[0], nz[1], pz[0], pz[1]), (nz[2], nz[3], pz[2], pz[3])] {
                    sections.push(bilinear(1.0, -n0.0 - n1.0, re_mul(n0, n1), -q0.0 - q1.0, re_mul(q0, q1), w));
                }
            }
        }
    }
    let mut first = None;
    if n & 1 == 1 {
        let k = 1.0 / (w * 0.5).tan();
        let f3 = |v: [f64; 3]| Some(v.map(|v| v as f32));
        match kind {
            0 => {
                let n = 1.0 / (k + 1.0);
                first = f3([n, n, (1.0 - k) * n]);
            }
            1 => {
                let n = 1.0 / (k + 1.0);
                first = f3([k * n, -(k * n), (1.0 - k) * n]);
            }
            4 => {
                let ia = 1.0 / a;
                let n = 1.0 / (k + ia);
                first = f3([(k + a) * n, (a - k) * n, (ia - k) * n]);
            }
            5 => {
                let n = 1.0 / (k + a);
                first = f3([(k * a2 + a) * n, (a - k * a2) * n, (a - k) * n]);
            }
            2 => sections.push(bilinear2(0.0, d, 0.0, 1.0, d, 1.0, w)),
            3 => sections.push(bilinear2(1.0, 0.0, 1.0, 1.0, d, 1.0, w)),
            _ => sections.push(bilinear2(1.0, d * a, 1.0, 1.0, d / a, 1.0, w)),
        }
    }
    (sections, first)
}

impl Bank {
    fn new(u: Uvi, rate: f32) -> Option<Self> {
        let mut bank = Self { sections: [[[0.0; 5]; BANK]; 2], count: 0, first: [None; 2], wide: false, sum: None };
        match u {
            Uvi::Digital { kind, order, hertz, track, spread, q, gain, bandwidth } => {
                bank.wide = true;
                for ch in 0..2 {
                    // 0x141287010: stereo spread is -/+ half the semitones per channel.
                    let sign = if ch == 0 { -1.0 } else { 1.0 };
                    let e = ((spread * (1.0 / 24.0)) * sign) + (track * (1.0 / 12.0));
                    // ponytail: the clamp statics (0x14258bff0/4) are encrypted .data; the Freq
                    // range 10..22000 stands in until a runtime dump.
                    let fc = (e.exp2() * hertz).clamp(10.0, 22_000.0);
                    let (sections, first) = digital_design(
                        kind,
                        usize::from(order),
                        f64::from(fc / rate),
                        gain,
                        f64::from(q),
                        f64::from(bandwidth),
                    );
                    bank.count = sections.len().min(BANK);
                    bank.sections[ch][..bank.count].copy_from_slice(&sections[..bank.count]);
                    bank.first[ch] = first;
                }
            }
            Uvi::Vowel { morph, q, formant, a, b, mode, count } => {
                let count = usize::from(count.clamp(1, 4));
                let m = f64::from(morph);
                for i in 0..count {
                    let hz = m * VOWELS[usize::from(b.min(10))][i] + (1.0 - m) * VOWELS[usize::from(a.min(10))][i];
                    let fn_ = (2f64.powf(f64::from(formant)) * hz) / f64::from(rate);
                    let s = rbj(mode.min(2), fn_, f64::from(q)).map(|c| f64::from(c as f32));
                    bank.sections[0][i] = s;
                    bank.sections[1][i] = s;
                }
                bank.count = count;
                bank.sum = Some(if mode == 1 { 1.0 } else { 1.0 / count as f32 });
            }
            _ => return None,
        }
        Some(bank)
    }

    fn process(&self, cells: &mut [[f64; 2]], block: &mut Planar, len: usize) {
        for ch in 0..2 {
            let sections = &self.sections[ch][..self.count];
            let mut s: [[f64; 2]; BANK] = std::array::from_fn(|i| [cells[2 * i][ch], cells[2 * i + 1][ch]]);
            let x = &mut block[ch][..len];
            if self.wide {
                for v in x.iter_mut() {
                    let mut y = f64::from(*v as f32);
                    for (c, s) in sections.iter().zip(&mut s) {
                        let o = y * c[0] + s[0];
                        s[0] = (y * c[1] + s[1]) - o * c[3];
                        s[1] = y * c[2] - o * c[4];
                        y = f64::from(o as f32);
                    }
                    *v = y;
                }
                if let Some([b0, b1, a1]) = self.first[ch] {
                    let mut z = cells[BANK_FIRST][ch] as f32;
                    for v in x.iter_mut() {
                        let u = *v as f32;
                        let y = (u * b0) + z;
                        z = (u * b1) - (y * a1);
                        *v = f64::from(y);
                    }
                    cells[BANK_FIRST][ch] = flushed(z);
                }
            } else {
                let gain = self.sum.unwrap_or(1.0);
                for v in x.iter_mut() {
                    let u = *v as f32;
                    let mut acc = 0f32;
                    for (c, s) in sections.iter().zip(&mut s) {
                        let [b0, b1, b2, a1, a2] = c.map(|c| c as f32);
                        let y = (u * b0) + s[0] as f32;
                        s[0] = f64::from(((u * b1) + s[1] as f32) - (y * a1));
                        s[1] = f64::from((u * b2) - (y * a2));
                        acc += y;
                    }
                    *v = f64::from(if gain == 1.0 { acc } else { acc * gain });
                }
            }
            for (i, s) in s[..self.count].iter().enumerate() {
                cells[2 * i][ch] = super::super::flush(s[0]);
                cells[2 * i + 1][ch] = super::super::flush(s[1]);
            }
        }
    }
}

/// `CombFilter` (delay 0x141105240, process 0x141106490): `y = x + g (d0 + (d1 - d0) frac)`
/// with `d0 = y[n - D]`, `d1 = y[n - D - 1]`, the delay `rate / fc` split into integer and
/// fraction, `g = Q` (Mode 0) or `-Q` (Mode 1). Cells: the write index, then a ring of
/// `D + 2` outputs (the native ring is larger; only these taps are read).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Comb {
    delay: usize,
    frac: f32,
    g: f32,
}

impl Comb {
    fn new(u: Uvi, rate: f32) -> Option<Self> {
        let Uvi::Comb { hertz, q, negative } = u else { return None };
        let half = rate * 0.5;
        let fc = if hertz <= half { hertz } else { half };
        let p = 1.0 / ((fc + fc) / rate);
        let dl = (p + p).abs();
        let delay = dl as usize;
        Some(Self { delay, frac: dl - delay as f32, g: if negative { -q } else { q } })
    }

    fn cells(&self) -> usize {
        1 + self.delay + 2
    }

    fn process(&self, cells: &mut [[f64; 2]], block: &mut Planar, len: usize) {
        let size = self.delay + 2;
        let start = cells[0][0] as usize % size;
        let ring = &mut cells[1..1 + size];
        for ch in 0..2 {
            let mut w = start;
            for v in &mut block[ch][..len] {
                let i0 = (w + size - self.delay) % size;
                let d0 = ring[i0][ch] as f32;
                let d1 = ring[(i0 + size - 1) % size][ch] as f32;
                let y = ((((d1 - d0) * self.frac) + d0) * self.g) + *v as f32;
                ring[w][ch] = flushed(y);
                *v = f64::from(y);
                w = (w + 1) % size;
            }
        }
        cells[0] = [((start + len) % size) as f64; 2];
    }
}

/// The UVI laws designed once at prepare (their controls are constants).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fixed {
    Xpander(Xpander),
    Bank(Bank),
    Comb(Comb),
}

impl Fixed {
    fn new(class: Legacy, rate: f32) -> Option<Self> {
        let Legacy::Uvi(u) = class else { return None };
        Xpander::new(u, rate)
            .map(Self::Xpander)
            .or_else(|| Bank::new(u, rate).map(Self::Bank))
            .or_else(|| Comb::new(u, rate).map(Self::Comb))
    }

    fn cells(&self) -> usize {
        match self {
            Self::Xpander(_) => XPANDER_CELLS,
            Self::Bank(_) => CELLS,
            Self::Comb(c) => c.cells(),
        }
    }

    fn process(&self, cells: &mut [[f64; 2]], block: &mut Planar, len: usize) {
        match self {
            Self::Xpander(x) => x.process(cells, block, len),
            Self::Bank(b) => b.process(cells, block, len),
            Self::Comb(c) => c.process(cells, block, len),
        }
    }
}

// ----------------------------------------------------------------- runtime

/// Normalized cutoff and resonance (the native knobs).
#[derive(Clone, Copy, Debug)]
pub struct CascadeSettings {
    pub class: Legacy,
    pub cutoff: Parameter,
    pub resonance: Parameter,
}

impl CascadeSettings {
    pub(crate) fn valid(self) -> bool {
        [self.cutoff, self.resonance].iter().all(|p| {
            p.valid()
                && !matches!(p, Parameter::Expression { .. })
                && p.bounds().iter().all(|v| (0.0..=1.0).contains(v))
        })
    }

    pub(crate) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Cascade {
        super::ladder_kernel::prepare();
        Cascade {
            class: self.class,
            parameters: [self.cutoff, self.resonance].map(|p| p.compile(bindings)),
            rate: rate as f32,
            fixed: Fixed::new(self.class, rate as f32),
        }
    }
}

pub(crate) struct Cascade {
    class: Legacy,
    parameters: [PreparedParameter; 2],
    rate: f32,
    /// UVI laws designed at prepare.
    fixed: Option<Fixed>,
}

impl Cascade {
    pub(crate) fn cells(&self) -> usize {
        self.fixed.map_or(CELLS, |f| f.cells())
    }

    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 2] {
        [("cutoff", self.parameters[0]), ("resonance", self.parameters[1])]
    }

    /// Filter `len` frames. `cells` holds the stage states, the clocked knobs, the knobs last
    /// designed for and the design; `modulation` adds to the knobs (native knob-domain
    /// modulation).
    ///
    /// The control clock (`0x140ae9f90`, types 2..=17): the first call snaps to the target;
    /// after every 32-frame chunk the cutoff moves toward it by at most
    /// `clamp(1 / (31 r + 4), 1/35, 1/4)` (`r` the resonance at the call's start; downward
    /// moves snap while modulated) and the resonance by `0.015` of the distance when that is
    /// over `1e-4`. Versatile's clock (`0x140aea690`) moves both by `0.015`.
    // ponytail: "modulated" is a nonzero cutoff delta, not the native's modulation-buffer
    // pointer; the clock's alternative branch (global 0x144fd4c54 = 1) and its offset slew
    // (+0x1f4 -> +0x1f8) are taken as unused, both unwritten in the binary.
    pub(crate) fn process(
        &self,
        cells: &mut [[f64; 2]],
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) -> bool {
        let target = |position: usize| -> [f32; 2] {
            std::array::from_fn(|i| {
                (self.parameters[i].value(parameters, at + position as u64, None) as f32
                    + modulation[i] as f32)
                    .clamp(0., 1.)
            })
        };
        if let Some(f) = &self.fixed {
            f.process(cells, block, len);
            return !cells.iter().flatten().all(|v| v.is_finite());
        }
        if let Legacy::Sv(sv) = self.class {
            self.sv(sv, cells, block, len, target);
            return !cells[..KNOBS].iter().flatten().all(|v| v.is_finite());
        }
        let started = cells[CLOCK + 1][0] != 0.0;
        let mut current = if started { cells[CLOCK].map(|v| v as f32) } else { target(0) };
        let step = (1.0 / (31.0 * current[1] + 4.0)).clamp(0.028_571_429, 0.25);
        let mut position = 0;
        while position < len {
            let goal = target(position);
            let frames = position..position + QUANTUM.min(len - position);
            match self.law(cells, current) {
                Law::Sections(d) => {
                    for ch in 0..2 {
                        let mut state: [[f32; 4]; 3] = std::array::from_fn(|s| {
                            std::array::from_fn(|i| cells[s * 4 + i][ch] as f32)
                        });
                        for v in &mut block[ch][frames.clone()] {
                            *v = f64::from(tick(&d, &mut state, *v as f32));
                        }
                        for (s, section) in state.iter().enumerate() {
                            for (i, v) in section.iter().enumerate() {
                                cells[s * 4 + i][ch] = flushed(*v);
                            }
                        }
                    }
                }
                Law::Chamberlin(c) => {
                    if !started && position == 0 {
                        // Reset sets the smoothed output gain to its target.
                        cells[7] = [f64::from(c.gain); 2];
                    }
                    run(cells, block, frames.clone(), |s, v| c.tick(s, v));
                }
                Law::Ladder(l) => run(cells, block, frames.clone(), |s, v| l.tick(s, v)),
                Law::Moog(m) => run(cells, block, frames.clone(), |s, v| m.tick(s, v)),
                Law::Rez(r) => {
                    for ch in 0..2 {
                        let mut s = [cells[0][ch], cells[1][ch]];
                        for v in &mut block[ch][frames.clone()] {
                            *v = r.tick(&mut s, f64::from(*v as f32));
                        }
                        cells[0][ch] = super::super::flush(s[0]);
                        cells[1][ch] = super::super::flush(s[1]);
                    }
                }
            }
            position = frames.end;
            current = self.clock(current, goal, step, modulation[0] != 0.0);
        }
        cells[CLOCK] = current.map(f64::from);
        cells[CLOCK + 1][0] = 1.0;
        !cells[..KNOBS].iter().flatten().all(|v| v.is_finite())
    }

    /// One control tick toward `goal`.
    fn clock(&self, [cut, res]: [f32; 2], goal: [f32; 2], step: f32, modulated: bool) -> [f32; 2] {
        if matches!(self.class, Legacy::Versatile(_)) {
            return [cut + (goal[0] - cut) * 0.015, res + (goal[1] - res) * 0.015];
        }
        let delta = goal[0] - cut;
        let cut = if modulated && delta < 0.0 { goal[0] } else { cut + delta.clamp(-step, step) };
        let res = if (goal[1] - res).abs() > 1e-4 { res + (goal[1] - res) * 0.015 } else { res };
        [cut, res]
    }

    /// The law for `knobs`; section designs come from the cells when designed already.
    fn law(&self, cells: &mut [[f64; 2]], knobs: [f32; 2]) -> Law {
        match self.class {
            Legacy::Pro52 => return Law::Chamberlin(pro52(knobs[0], knobs[1], self.rate)),
            Legacy::Versatile(v) => {
                return Law::Chamberlin(versatile(&v, knobs[0], knobs[1], self.rate));
            }
            Legacy::Ladder => return Law::Ladder(legacy_ladder(knobs[0], knobs[1], self.rate)),
            Legacy::Uvi(Uvi::Moog { hertz, q }) => return Law::Moog(Moog::new(hertz, q, self.rate)),
            Legacy::Uvi(Uvi::Rez { mode, hertz, q }) => return Law::Rez(Rez::new(mode, hertz, q, self.rate)),
            Legacy::Uvi(Uvi::Xpander { .. } | Uvi::Digital { .. } | Uvi::Vowel { .. } | Uvi::Comb { .. }) => {
                unreachable!("fixed UVI laws run before the clocked laws")
            }
            _ => {}
        }
        let seen = cells[KNOBS];
        let valid = cells[KNOBS + 1][0] != 0.0;
        if valid && seen == [f64::from(knobs[0]), f64::from(knobs[1])] {
            return Law::Sections(unpack(&cells[COEFS..]));
        }
        let design = design(self.class, knobs[0], knobs[1], self.rate);
        cells[KNOBS] = [f64::from(knobs[0]), f64::from(knobs[1])];
        cells[KNOBS + 1][0] = 1.0;
        pack(&design, &mut cells[COEFS..]);
        Law::Sections(design)
    }
}

enum Law {
    Sections(Design),
    Chamberlin(Chamberlin),
    Ladder(Ladder2x),
    Moog(Moog),
    Rez(Rez),
}

/// Run a stage law over `frames` of both channels; its state is cells `0..8`.
fn run(
    cells: &mut [[f64; 2]],
    block: &mut Planar,
    frames: std::ops::Range<usize>,
    tick: impl Fn(&mut [f32; 8], f32) -> f32,
) {
    for ch in 0..2 {
        let mut state: [f32; 8] = std::array::from_fn(|i| cells[i][ch] as f32);
        for v in &mut block[ch][frames.clone()] {
            *v = f64::from(tick(&mut state, *v as f32));
        }
        for (i, v) in state.iter().enumerate() {
            cells[i][ch] = flushed(*v);
        }
    }
}

fn flushed(v: f32) -> f64 {
    f64::from(super::super::flush(f64::from(v)) as f32)
}

fn pack(d: &Design, cells: &mut [[f64; 2]]) {
    let mut flat = [0f64; 18];
    for (s, sec) in d.sections.iter().enumerate() {
        flat[s * 5..s * 5 + 5]
            .copy_from_slice(&[sec.b0, sec.b1, sec.b2, sec.c1, sec.c2].map(f64::from));
    }
    flat[15] = f64::from(d.gain);
    flat[16] = d.count as f64 + if d.parallel { 8.0 } else { 0.0 };
    flat[17] = d.clip.map_or(-1.0, |(s, slope)| s as f64 * 4.0 + f64::from(slope));
    for (i, v) in flat.iter().enumerate() {
        cells[i / 2][i % 2] = *v;
    }
}

fn unpack(cells: &[[f64; 2]]) -> Design {
    let flat: [f64; 18] = std::array::from_fn(|i| cells[i / 2][i % 2]);
    let sections = std::array::from_fn(|s| {
        let c = |i: usize| flat[s * 5 + i] as f32;
        Section { b0: c(0), b1: c(1), b2: c(2), c1: c(3), c2: c(4) }
    });
    let shape = flat[16] as usize;
    Design {
        sections,
        count: shape & 7,
        parallel: shape & 8 != 0,
        gain: flat[15] as f32,
        clip: (flat[17] >= 0.0).then(|| {
            let s = (flat[17] / 4.0).floor();
            (s as usize, (flat[17] - s * 4.0) as f32)
        }),
    }
}

/// One frame through the design. `state[s] = [x1, x2, y1, y2]`.
#[inline(always)]
fn tick(d: &Design, state: &mut [[f32; 4]; 3], input: f32) -> f32 {
    let knee = |v: f32, slope: f32| {
        let k = v.clamp(-1.0, 1.0);
        (v - k) * slope + k
    };
    let input = match d.clip {
        Some((0, slope)) => knee(input, slope),
        _ => input,
    };
    let mut x = input;
    let mut sum = 0.0;
    for s in 0..d.count {
        let c = d.sections[s];
        let [x1, x2, y1, y2] = state[s];
        let section_in = if d.parallel { input } else { x };
        let mut y = c.c1 * y1 + c.c2 * y2 + c.b2 * x2 + c.b1 * x1 + c.b0 * section_in;
        if let Some((at, slope)) = d.clip
            && at == s + 1
        {
            y = knee(y, slope);
        }
        state[s] = [section_in, x1, y, y1];
        sum += y;
        x = y;
    }
    if d.parallel { sum * d.gain } else { x * d.gain }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse_response(class: Legacy, cutoff: f32, resonance: f32, input: &[f32]) -> Vec<f32> {
        let d = design(class, cutoff, resonance, 48_000.0);
        let mut state = [[0f32; 4]; 3];
        input.iter().map(|&x| tick(&d, &mut state, x)).collect()
    }

    /// Native outputs of the BFilter classes (prepare, setter, process) run under
    /// tools/re/kemu.py from the Kontakt 8.13.1 binary at 48 kHz: 64 frames each of an impulse at
    /// three knob settings and of a +-4 square (period 40) that drives the in-loop clip.
    #[test]
    fn legacy_classes_match_the_native_vectors() {
        let vectors: std::collections::BTreeMap<String, Vec<f32>> =
            serde_json::from_str(include_str!("cascade_native_vectors.json")).unwrap();
        let classes = [
            ("LP1", Legacy::Lp1), ("HP1", Legacy::Hp1), ("BP2", Legacy::Bp2), ("LP2", Legacy::Lp2),
            ("HP2", Legacy::Hp2), ("LP4", Legacy::Lp4), ("HP4", Legacy::Hp4), ("BP4", Legacy::Bp4),
            ("BR4", Legacy::Br4), ("LP6", Legacy::Lp6),
        ];
        let mut failures = Vec::new();
        for (key, native) in &vectors {
            let mut words = key.split(' ');
            let name = words.next().unwrap();
            let cutoff: f32 = words.next().unwrap().parse().unwrap();
            let resonance: f32 = words.next().unwrap().parse().unwrap();
            let loud = words.next() == Some("loud");
            let class = classes.iter().find(|c| c.0 == name).unwrap().1;
            let input: Vec<f32> = (0..native.len())
                .map(|i| if loud { if (i / 20) % 2 == 0 { 4.0 } else { -4.0 } } else if i == 0 { 1.0 } else { 0.0 })
                .collect();
            let ours = impulse_response(class, cutoff, resonance, &input);
            let scale = native.iter().fold(1e-3f32, |m, v| m.max(v.abs()));
            let (i, err) = ours.iter().zip(native).map(|(a, b)| (a - b).abs() / scale).enumerate()
                .fold((0, 0f32), |m, (i, e)| if e > m.1 { (i, e) } else { m });
            if err >= 2e-5 {
                failures.push(format!("{key}: frame {i} err {err}: {:?} vs {:?}", &ours[..4], &native[..4]));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Pro-53, Legacy Ladder, Vowel A/B and Versatile: 64 frames of the extracted models (each
    /// matched the native setters and process under kemu with error 0), impulse and +-4 square.
    #[test]
    fn stage_classes_match_the_native_models() {
        super::super::ladder_kernel::prepare();
        let vectors: std::collections::BTreeMap<String, Vec<f32>> =
            serde_json::from_str(include_str!("cascade_stage_vectors.json")).unwrap();
        let vers = |mixed: bool, res: f32| sampler_ir::fx::filter::Versatile {
            shift: if mixed { [0.2, 0.1] } else { [0.0; 2] },
            resonance: if mixed { [0.6, 0.4] } else { [res; 2] },
            types: if mixed { [0.0, 0.5, 1.0] } else { [0.0; 3] },
            wet: if mixed { [1.0, 0.7, 0.2] } else { [1.0; 3] },
            gain: 0.5,
        };
        let mut failures = Vec::new();
        for (key, native) in &vectors {
            let mut words = key.split(' ');
            let name = words.next().unwrap();
            let cut: f32 = words.next().unwrap().parse().unwrap();
            let res: f32 = words.next().unwrap().parse().unwrap();
            let loud = words.next() == Some("loud");
            let input: Vec<f32> = (0..native.len())
                .map(|i| if loud { if (i / 20) % 2 == 0 { 4.0 } else { -4.0 } } else if i == 0 { 1.0 } else { 0.0 })
                .collect();
            let ours: Vec<f32> = match name {
                "PRO52" | "VERS-lp" | "VERS-mixed" => {
                    let c = match name {
                        "PRO52" => pro52(cut, res, 48_000.0),
                        n => versatile(&vers(n == "VERS-mixed", res), cut, res, 48_000.0),
                    };
                    let mut s = [0f32; 8];
                    s[7] = c.gain;
                    input.iter().map(|&x| c.tick(&mut s, x)).collect()
                }
                "LADDER" => {
                    let l = legacy_ladder(cut, res, 48_000.0);
                    let mut s = [0f32; 8];
                    input.iter().map(|&x| l.tick(&mut s, x)).collect()
                }
                n => impulse_response(if n == "VOWELA" { Legacy::VowelA } else { Legacy::VowelB }, cut, res, &input),
            };
            let scale = native.iter().fold(1e-3f32, |m, v| m.max(v.abs()));
            let err = ours.iter().zip(native).map(|(a, b)| (a - b).abs() / scale).fold(0f32, f32::max);
            if !(err < 2e-5) {
                failures.push(format!("{key}: err {err}: {:?} vs {:?}", &ours[..4], &native[..4]));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The SV family against the native classes run under kemu (sv/vectors.py: setters,
    /// 32-frame block update, process; 64 frames of an impulse and a +-4 square).
    #[test]
    fn sv_classes_match_the_native_vectors() {
        super::super::ladder_kernel::prepare();
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("cascade_sv_vectors.json")).unwrap();
        let mut failures = Vec::new();
        for case in &cases {
            let set = &case["setting"];
            let name = set["type"].as_str().unwrap();
            let num = |k: &str| set[k].as_f64().unwrap_or(0.0);
            let mode = |n: &str| ["LP", "BP", "HP", "NOTCH"].iter().position(|m| n.starts_with(m)).unwrap() as u8;
            let sv = match case["class"].as_str().unwrap() {
                "SV1" => Sv::One { high: name == "HP1" },
                "SV2" => Sv::Two { mode: mode(name) },
                "SV4" => Sv::Four { mode: mode(name) },
                "SV6" => Sv::Six { mode: mode(name) },
                _ => Sv::Multi {
                    mode: ["PAR_LPHP", "PAR_BPBP", "SER_LPHP"].iter().position(|m| *m == name).unwrap() as u8,
                    bandwidth: num("bandwidth") as f32,
                },
            };
            let unit = CascadeSettings {
                class: Legacy::Sv(sv),
                cutoff: Parameter::Constant(num("cutoff")),
                resonance: Parameter::Constant(num("res")),
            }
            .compile(48_000, &mut Vec::new());
            for input in ["impulse", "square4_p40"] {
                let native: Vec<f32> =
                    case[input].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
                let mut cells = vec![[0f64; 2]; CELLS];
                let mut block = [[0f64; super::super::super::BLOCK]; 2];
                for (i, v) in block[0].iter_mut().enumerate() {
                    *v = if input == "impulse" { if i == 0 { 1.0 } else { 0.0 } } else if (i % 40) < 20 { 4.0 } else { -4.0 };
                }
                unit.process(&mut cells, &[], &mut block, native.len(), 0, [0.0; 4]);
                let scale = native.iter().fold(1e-3f32, |m, v| m.max(v.abs()));
                let err = block[0].iter().zip(&native).map(|(a, b)| (*a as f32 - b).abs() / scale).fold(0f32, f32::max);
                if !(err < 1e-6) {
                    failures.push(format!("{} {set} {input}: err {err}: {:?} vs {:?}", case["class"], &block[0][..3], &native[..3]));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// UVI Workstation 4.0.9 BiquadFilter, MoogFilter and Rez run under kemu (uvifilt): a
    /// 64-frame impulse response at 48 kHz per setting.
    #[test]
    fn uvi_laws_match_the_native_vectors() {
        let vectors: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
            serde_json::from_str(include_str!("cascade_uvi_vectors.json")).unwrap();
        let mut failures = Vec::new();
        for (name, cases) in &vectors {
            for case in cases {
                let p = &case["params"];
                let num = |k: &str| p[k].as_f64().unwrap_or(0.0) as f32;
                let uvi = match name.as_str() {
                    "BiquadFilter" => Uvi::Biquad { mode: num("Mode") as u8, hertz: num("Freq"), q: num("Q") },
                    "MoogFilter" => Uvi::Moog { hertz: num("Freq"), q: num("Q") },
                    _ => Uvi::Rez { mode: num("Mode") as u8, hertz: num("Freq"), q: num("Q") },
                };
                let unit = CascadeSettings {
                    class: Legacy::Uvi(uvi),
                    cutoff: Parameter::Constant(0.0),
                    resonance: Parameter::Constant(0.0),
                }
                .compile(48_000, &mut Vec::new());
                let native: Vec<f32> =
                    case["out"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
                let mut cells = vec![[0f64; 2]; CELLS];
                let mut block = [[0f64; super::super::super::BLOCK]; 2];
                block[0][0] = 1.0;
                unit.process(&mut cells, &[], &mut block, native.len(), 0, [0.0; 4]);
                let scale = native.iter().fold(1e-3f32, |m, v| m.max(v.abs()));
                let err = block[0].iter().zip(&native).map(|(a, b)| (*a as f32 - b).abs() / scale).fold(0f32, f32::max);
                // BiquadFilter's coefficients come from the native fast sincosf (0x14199de20,
                // ~1.6e-7 absolute); ours use libm.
                if !(err < 3e-5) {
                    failures.push(format!("{name} {p}: err {err}: {:?} vs {:?}", &block[0][..3], &native[..3]));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// UVI XpanderFilter end to end (drive, HIIR 2x, kernel with its R/g ramp, makeup) under
    /// kemu: a fresh unit (R and g ramp from 0) and a settled one, 64-frame impulse at 48 kHz.
    #[test]
    fn xpander_matches_the_native_chain() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("cascade_xpander_vectors.json")).unwrap();
        let mut failures = Vec::new();
        for case in vectors["cases"].as_array().unwrap() {
            let p = &case["params"];
            let num = |k: &str| p[k].as_f64().unwrap() as f32;
            let uvi = Uvi::Xpander {
                mode: num("Mode") as u8,
                hertz: num("Freq"),
                q: num("Q"),
                distortion: num("DistortionType") as u8,
                drive: num("Drive_dB"),
                fat: num("Fat"),
                algorithm: num("Algorithm") as u8,
                oversampling: num("Oversampling") != 0.0,
            };
            let settings =
                CascadeSettings { class: Legacy::Uvi(uvi), cutoff: Parameter::Constant(0.0), resonance: Parameter::Constant(0.0) };
            let unit = settings.compile(48_000, &mut Vec::new());
            let Some(Fixed::Xpander(x)) = unit.fixed else { panic!() };
            let (r, g) = (case["R"].as_f64().unwrap() as f32, case["g"].as_f64().unwrap() as f32);
            if (x.r - r).abs() > 1e-6 || (x.g - g).abs() > 1e-6 {
                failures.push(format!("{p}: R/g {} {} vs {r} {g}", x.r, x.g));
            }
            let mut cells = vec![[0f64; 2]; unit.cells()];
            if !case["fresh"].as_bool().unwrap() {
                cells[XP_PREVIOUS] = [f64::from(x.r); 2];
                // Settled R and g only: the drive still glides from 1 on this first call.
                cells[XP_PREVIOUS + 1] = [f64::from(x.g); 2];
            }
            let mut block = [[0f64; super::super::super::BLOCK]; 2];
            block[0][0] = 1.0;
            block[1][0] = 0.5;
            unit.process(&mut cells, &[], &mut block, 64, 0, [0.0; 4]);
            for (ch, key) in ["out", "right"].into_iter().enumerate() {
                let Some(native) = case[key].as_array() else { continue };
                let native: Vec<f32> = native.iter().map(|v| v.as_f64().unwrap() as f32).collect();
                let scale = native.iter().fold(1e-3f32, |m, v| m.max(v.abs()));
                let err = block[ch].iter().zip(&native).map(|(a, b)| (*a as f32 - b).abs() / scale).fold(0f32, f32::max);
                // Mono alg-0 vectors differ from our stereo-kernel order by rounding.
                if !(err < 2e-5) {
                    failures.push(format!("{p} fresh {} {key}: err {err}: {:?} vs {:?}", case["fresh"], &block[ch][..4], &native[..4]));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// UVI DigitalFilter (f64 TDF-II cascade + first-order stage, stereo spread), VowelFilter
    /// (parallel f32 bank) and CombFilter (fractional feedback delay) under kemu.
    #[test]
    fn uvi_banks_and_comb_match_the_native_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("cascade_uvi_bank_vectors.json")).unwrap();
        let mut failures = Vec::new();
        for name in ["DigitalFilter", "VowelFilter", "CombFilter"] {
            for case in vectors[name].as_array().unwrap() {
                let p = &case["params"];
                let num = |k: &str| p[k].as_f64().unwrap() as f32;
                let uvi = match name {
                    "DigitalFilter" => Uvi::Digital {
                        kind: num("Type") as u8,
                        order: num("order") as u8,
                        hertz: num("Freq"),
                        track: 0.0,
                        spread: num("Spread"),
                        q: num("Q"),
                        gain: num("Gain"),
                        bandwidth: num("Bandwidth"),
                    },
                    "VowelFilter" => Uvi::Vowel {
                        morph: num("Morph"),
                        q: num("Q"),
                        formant: num("Formant"),
                        a: num("FilterA") as u8,
                        b: num("FilterB") as u8,
                        mode: num("Mode") as u8,
                        count: num("NumFormants") as u8,
                    },
                    _ => Uvi::Comb { hertz: num("Freq"), q: num("Q"), negative: num("Mode") == 1.0 },
                };
                let unit = CascadeSettings {
                    class: Legacy::Uvi(uvi),
                    cutoff: Parameter::Constant(0.0),
                    resonance: Parameter::Constant(0.0),
                }
                .compile(48_000, &mut Vec::new());
                let mut cells = vec![[0f64; 2]; unit.cells()];
                let mut block = [[0f64; super::super::super::BLOCK]; 2];
                block[0][0] = 1.0;
                block[1][0] = 1.0;
                // Two calls of 32 frames: the comb ring and bank states carry across.
                unit.process(&mut cells, &[], &mut block, 32, 0, [0.0; 4]);
                let mut rest = [[0f64; super::super::super::BLOCK]; 2];
                unit.process(&mut cells, &[], &mut rest, 32, 32, [0.0; 4]);
                for ch in 0..2 {
                    block[ch][32..64].copy_from_slice(&rest[ch][..32]);
                }
                for (ch, key) in ["out", "right"].into_iter().enumerate() {
                    let Some(native) = case[key].as_array() else { continue };
                    let native: Vec<f32> = native.iter().map(|v| v.as_f64().unwrap() as f32).collect();
                    let scale = native.iter().fold(1e-3f32, |m, v| m.max(v.abs()));
                    let err = block[ch].iter().zip(&native).map(|(a, b)| (*a as f32 - b).abs() / scale).fold(0f32, f32::max);
                    if !(err < 1e-5) {
                        failures.push(format!("{name} {p} {key}: err {err}: {:?} vs {:?}", &block[ch][..3], &native[..3]));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn lp4_matches_the_kontakt_measurement() {
        // filter-responses.tsv rows 73, 76, 77 (48 kHz, knob 0.5): -3 dB at 1192 Hz at zero
        // resonance; passband -5.23 / -9.88 dB and peak +20.88 / +37.26 dB over it at 0.5 / 0.9.
        let response = |d: &Design, hz: f64| {
            let w = std::f64::consts::TAU * hz / 48_000.0;
            let z = num_complex(-w);
            d.sections[..d.count].iter().fold(f64::from(d.gain), |h, s| {
                let num = cadd(cadd(cscale(f64::from(s.b0), (1.0, 0.0)), cscale(f64::from(s.b1), z)), cscale(f64::from(s.b2), cmul(z, z)));
                let den = csub(csub((1.0, 0.0), cscale(f64::from(s.c1), z)), cscale(f64::from(s.c2), cmul(z, z)));
                h * cabs(num) / cabs(den)
            })
        };
        let db = |v: f64| 20.0 * v.log10();
        let flat = design(Legacy::Lp4, 0.5, 0.0, 48_000.0);
        let corner = (100..20_000).find(|&f| db(response(&flat, f64::from(f))) < -3.0).unwrap();
        assert!((corner as f64 / 1192.0 - 1.0).abs() < 0.01, "{corner}");
        for (res, passband, peak) in [(0.5, -5.23, 20.88), (0.9, -9.88, 37.26)] {
            let d = design(Legacy::Lp4, 0.5, res, 48_000.0);
            let pb = db(response(&d, 30.0));
            let top = (100..5_000).map(|f| db(response(&d, f64::from(f)))).fold(f64::MIN, f64::max);
            assert!((pb - passband).abs() < 0.05, "{res}: passband {pb}");
            assert!((top - pb - peak).abs() < 0.2, "{res}: peak {}", top - pb);
        }
        // LP6 rows 80, 83, 84.
        for (res, passband) in [(0.5, -11.17), (0.9, -22.92)] {
            let pb = db(response(&design(Legacy::Lp6, 0.5, res, 48_000.0), 30.0));
            assert!((pb - passband).abs() < 0.05, "LP6 {res}: {pb}");
        }
    }

    fn num_complex(w: f64) -> (f64, f64) {
        (w.cos(), w.sin())
    }
    fn cmul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
        (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
    }
    fn cadd(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
        (a.0 + b.0, a.1 + b.1)
    }
    fn csub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
        (a.0 - b.0, a.1 - b.1)
    }
    fn cscale(k: f64, a: (f64, f64)) -> (f64, f64) {
        (k * a.0, k * a.1)
    }
    fn cabs(a: (f64, f64)) -> f64 {
        a.0.hypot(a.1)
    }
}
