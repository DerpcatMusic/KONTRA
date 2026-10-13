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
use super::super::Planar;

/// Frames between control evaluations (the engine's native-knob clock).
const QUANTUM: usize = 32;
/// Cells of `[f64; 2]` owned per instance: 3 sections x (x1, x2, y1, y2) per channel, the
/// cached knobs and the coefficients.
pub(crate) const CELLS: usize = 12 + 2 + 9;
const KNOBS: usize = 12;
const COEFS: usize = 14;

/// The native legacy classes (saved type in the comment).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legacy {
    Lp1, // 2
    Hp1, // 3
    Bp2, // 4
    Lp2, // 6
    Hp2, // 7
    Lp4, // 8
    Hp4, // 9
    Bp4, // 10
    Br4, // 11
    Lp6, // 12
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
        Cascade {
            class: self.class,
            parameters: [self.cutoff, self.resonance].map(|p| p.compile(bindings)),
            rate: rate as f32,
        }
    }
}

pub(crate) struct Cascade {
    class: Legacy,
    parameters: [PreparedParameter; 2],
    rate: f32,
}

impl Cascade {
    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 2] {
        [("cutoff", self.parameters[0]), ("resonance", self.parameters[1])]
    }

    /// Filter `len` frames. `cells` holds the section states, the knobs last designed for and
    /// the design; `modulation` adds to the knobs (native knob-domain modulation).
    pub(crate) fn process(
        &self,
        cells: &mut [[f64; 2]],
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) -> bool {
        let mut position = 0;
        while position < len {
            let knobs: [f32; 2] = std::array::from_fn(|i| {
                (self.parameters[i].value(parameters, at + position as u64, None) as f32
                    + modulation[i] as f32)
                    .clamp(0., 1.)
            });
            let design = self.cached(cells, knobs);
            let run = QUANTUM.min(len - position);
            for ch in 0..2 {
                let mut state: [[f32; 4]; 3] =
                    std::array::from_fn(|s| std::array::from_fn(|i| cells[s * 4 + i][ch] as f32));
                for v in &mut block[ch][position..position + run] {
                    *v = f64::from(tick(&design, &mut state, *v as f32));
                }
                for (s, section) in state.iter().enumerate() {
                    for (i, v) in section.iter().enumerate() {
                        cells[s * 4 + i][ch] = f64::from(super::super::flush(f64::from(*v)) as f32);
                    }
                }
            }
            position += run;
        }
        !cells[..KNOBS].iter().flatten().all(|v| v.is_finite())
    }

    /// The design for `knobs`, from the cells when they were designed already.
    fn cached(&self, cells: &mut [[f64; 2]], knobs: [f32; 2]) -> Design {
        let seen = cells[KNOBS];
        let valid = cells[KNOBS + 1][0] != 0.0;
        if valid && seen == [f64::from(knobs[0]), f64::from(knobs[1])] {
            return unpack(&cells[COEFS..]);
        }
        let design = design(self.class, knobs[0], knobs[1], self.rate);
        cells[KNOBS] = [f64::from(knobs[0]), f64::from(knobs[1])];
        cells[KNOBS + 1][0] = 1.0;
        pack(&design, &mut cells[COEFS..]);
        design
    }
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
