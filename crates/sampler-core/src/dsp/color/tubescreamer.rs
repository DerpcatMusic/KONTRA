//! Kontakt Tubescreamer (`BEffectSkreamerDlx`, module 0x78; SkreamerDlx core ctor 0x141996f20,
//! process 0x141999240) as the color chain's shaper: mono (L + R) through a soft input
//! limiter, a 15 kHz TPT low-pass, the op-amp feedback one-poles, the clipping diode pair
//! (Newton solve 0x141998cf0, f64), then the tone stage: a 4-state DK tone stack (0x14199a540)
//! for circuits 0/1, or a fixed stack plus the 8-node MNA pot network (0x141999030 /
//! 0x141998930) for circuit 2; a 10 Hz TPT high-pass and the level. Laws: prepare
//! 0x141998280, mode table 0x141997b30, diode 0x14199aab0, MNA 0x14199a820 (model
//! kcolor/ts.py + tslaws.py, checked against the emulated core).
// ponytail: knobs are constants; matrix inverses are Gauss-Jordan (native 0x141999b30
// differs at rounding level).
use super::super::kaiser::{self, Wrapper, WrapperState};
use super::super::Planar;
pub use sampler_ir::fx::color::Tubescreamer;

/// Per circuit: R1 R2 R3 C1 C2 | diode Is n Cd | tone-stack components (0x141997b30).
const TABLES: [[f32; 15]; 3] = [
    [4490.0, 50300.0, 513100.0, 5.17e-8, 5.26e-11, 1.924, 0.026, 1.87e-9, 1000.0, 220.0, 1000.0, 2.2e-7, 2.2e-7, 18980.0, 1.0],
    [4700.0, 51000.0, 897000.0, 4.7e-8, 5.1e-11, 5.010_856_6, 0.026, 2.458_512_06e-9, 1000.0, 330.0, 1000.0, 4.7e-8, 4.7e-7, 19400.0, 0.31],
    [4490.0, 10000.0, 1_000_000.0, 5.17e-8, 5.26e-11, 1.924, 0.026, 1.87e-9, 1000.0, 220.0, 1000.0, 2.2e-7, 2.2e-7, 18980.0, 1.0],
];
/// The circuit-2 fixed stack's components (ctor).
const FIXED_STACK: [f32; 7] = [1000.0, 330.0, 1000.0, 4.7e-8, 4.7e-7, 26100.0, 0.5];
/// Core defaults by index: drive, pot a, pot b, filter, tone, level, mode.
pub(crate) const DEFAULTS: [f32; 7] = [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0];

fn invert<const N: usize>(m: [[f64; N]; N]) -> [[f64; N]; N] {
    let mut a = m;
    let mut inv: [[f64; N]; N] = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 }));
    for c in 0..N {
        let p = (c..N).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs())).unwrap_or(c);
        a.swap(c, p);
        inv.swap(c, p);
        let d = a[c][c];
        for j in 0..N {
            a[c][j] /= d;
            inv[c][j] /= d;
        }
        for r in (0..N).filter(|&r| r != c) {
            let k = a[r][c];
            for j in 0..N {
                a[r][j] -= k * a[c][j];
                inv[r][j] -= k * inv[c][j];
            }
        }
    }
    inv
}

fn clamp6(x: f64) -> f64 {
    x.clamp(1e-6, 0.999_999)
}

/// A TPT state-variable filter (f32): g, d, k.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Svf {
    g: f32,
    d: f32,
    k: f32,
}

impl Svf {
    #[allow(clippy::excessive_precision)]
    fn new(x: f32, q: f32, c3: f32, c5: f32) -> Self {
        let g = (((q * q) * x) * c5 + (q * x) * c3) + x;
        let k = 1.414_213_54;
        Self { g, d: 1.0 / ((g + k) * g + 1.0), k }
    }

    /// (low-pass, high-pass).
    #[inline(always)]
    fn tick(&self, s: &mut [f32; 2], x: f32) -> (f32, f32) {
        let s2 = s[1];
        let h = ((x - s2) * self.g + s[0]) * self.d;
        let s2n = (h + h) * self.g + s2;
        s[0] = (h + h) - s[0];
        s[1] = s2n;
        let lp = (s2 + s2n) * 0.5;
        (lp, (x - h * self.k) - lp)
    }
}

/// A 4-state DK tone stack: `m1`/`m2` as `[j][i]`, input gain `p4`, output gain `p1`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stack {
    m1: [[f64; 4]; 4],
    m2: [[f64; 4]; 4],
    p4: f64,
    p1: f64,
}

impl Stack {
    /// 0x141999da0 components then the 0x14199a540 law at `tone`.
    fn new(a: [f32; 7], sr: f64, tone: f64) -> Self {
        let a = a.map(f64::from);
        let (p4, p3, p5, p8, p9, p10, p1) = (1.0 / a[0], a[5], 1.0 / a[1], 1.0 / a[2], a[3] + a[3], a[4] + a[4], a[6]);
        let (d3, d4) = (sr * p9, sr * p10);
        let x = clamp6((tone * 5.0204 - 2.5732).tanh() * 0.5054 + 0.502);
        let d5 = 1.0 / (p3 * x);
        let c0 = 1.0 / ((1.0 - x) * p3);
        let d3b = c0 + d5;
        let s = [d5 + p4 + d3, d3b, 0.0, -(c0 + p8), -d5, -(d4 + d3b), d4, c0, 0.0, d4, -(d4 + p5), 0.0, 0.0, 0.0, 0.0, p8];
        // Column-major: entry i + 4 j is row i, column j.
        let inv = invert::<4>(std::array::from_fn(|i| std::array::from_fn(|j| s[i + 4 * j])));
        let mut m1 = [[0.0; 4]; 4];
        m1[0][0] = d3 - (d5 + p4);
        m1[1][0] = d5;
        m1[0][1] = -d3b;
        m1[1][1] = d3b - d4;
        m1[2][1] = d4;
        m1[1][2] = d4;
        m1[2][2] = p5 - d4;
        m1[0][3] = p8 + c0;
        m1[1][3] = -c0;
        m1[3][3] = -p8;
        Self { m1, m2: std::array::from_fn(|j| std::array::from_fn(|i| inv[i][j])), p4, p1 }
    }

    /// Sums in the native lane order (rows 0, 1 start at j = 1).
    #[inline(always)]
    fn apply(m: &[[f64; 4]; 4], x: &[f64; 4]) -> [f64; 4] {
        std::array::from_fn(|i| {
            if i < 2 {
                ((x[1] * m[1][i] + x[0] * m[0][i]) + x[2] * m[2][i]) + x[3] * m[3][i]
            } else {
                ((x[0] * m[0][i] + x[1] * m[1][i]) + x[2] * m[2][i]) + x[3] * m[3][i]
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Diode {
    k10: f64,
    k38: f64,
    k60: f64,
    k70: f64,
    k78: f64,
    k80: f64,
    k88: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    mode: usize,
    level: f32,
    hi: Svf,
    lo: Svf,
    c744: f32,
    c750: f32,
    diode: Diode,
    stack: Stack,
    /// Circuit 2: the MNA inverse `[i][j]` and the pot resistances p3..p6.
    mna: [[f64; 8]; 8],
    pots: [f64; 4],
    sr: f64,
    /// The wrapper runs the core at 2^round(log2(4 x 44100 / rate)) x the host rate.
    oversampler: Wrapper,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    hi: [f32; 2],
    lo: [f32; 2],
    z74c: f32,
    z758: f32,
    /// x, d3, d2, v.
    diode: [f64; 4],
    stack: [f64; 4],
    previous: f64,
    /// x, then the 8 node voltages.
    mna: [f64; 9],
    up: [WrapperState; 2],
    down: WrapperState,
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.hi.iter().chain(&self.lo).all(|v| v.is_finite())
            && self.diode.iter().chain(&self.stack).chain(&self.mna).all(|v| v.is_finite())
            && self.up.iter().chain([&self.down]).all(WrapperState::finite)
    }
}

impl Engine {
    #[allow(clippy::excessive_precision)]
    pub(super) fn new(ts: Tubescreamer, rate: u32) -> Self {
        let p = ts.knobs;
        let oversampler = Wrapper::new(2, rate);
        let core = rate * oversampler.factor() as u32;
        let sr = f64::from(core);
        let mode = ((p[6] * 3.0) as i32).clamp(0, 2) as usize;
        let t = TABLES[mode].map(f64::from);
        let tt = 1.0 / sr;
        // Diode block (0x7a8): T, drive, R1 R2 R3 C1 C2, k38, .., Is n Cd, 1/(n Is).
        let k60 = 1.0 / (t[6] * t[5]);
        let k38 = (tt * 0.5) / (t[3] * t[0]);
        let drive = f64::from(p[0]);
        let th = (((drive * 6.6076 - 6.0052).exp() - 1.0) * 1.4213 + 1.4389).tanh();
        let d1 = tt / t[4];
        let d2 = (tt * 0.5) / ((th * t[2] + t[1]) * t[4]);
        let d4 = 1.0 / (d2 + 1.0);
        let k80 = (d1 * t[7]) * d4;
        let diode = Diode { k10: t[0], k38, k60, k70: d4 * d1, k78: (1.0 - d2) * d4, k80, k88: k80 * k60 };
        let tone: [f32; 7] = std::array::from_fn(|i| TABLES[mode][8 + i]);
        let stack = if mode == 2 {
            Stack::new(FIXED_STACK, sr, f64::from(p[3]))
        } else {
            Stack::new(tone, sr, f64::from(p[4]))
        };
        // MNA (0x14199a820): the apply and prepare both remap pot a below 0.5 (native quirk).
        let mut x1 = f64::from(p[1]);
        for _ in 0..2 {
            if x1 < 0.5 {
                x1 = x1 * 0.4528 + 0.2736;
            }
        }
        let x2 = f64::from(p[2]);
        let a = clamp6((((x1 - 0.5) * 6.0).tanh() + 1.0) * 0.5);
        let c = clamp6((((x2 - 0.5) * 6.0).tanh() + 1.0) * 0.5);
        let pots = [a * 1e4, (1.0 - a) * 1e4, c * 1e4, (1.0 - c) * 1e4];
        let (d7, i3, i5, d8) = (0.5 / pots[1], 1.0 / pots[0], 1.0 / pots[2], 0.5 / pots[3]);
        let mut g = [[0f64; 8]; 8];
        let mut set = |k: usize, v: f64| g[k % 8][k / 8] = v;
        for (k, m, c) in [
            (0x11, 1.2e-6, 0.0),
            (0x0a, 1.2e-6, 0.0),
            (0x12, -1.247e-6, -0.001_515_151_515_151_515_2),
            (0x1a, 4.7e-8, 0.001_515_151_515_151_515_2),
            (0x13, 4.7e-8, 0.0),
            (0x1b, -4.7e-8, -6.097_560_975_609_756_4e-6),
            (0x2c, -2.7e-7, 0.0),
            (0x25, 2.7e-7, 0.0),
            (0x2d, -2.782e-7, -0.001_515_151_515_151_515_2),
            (0x35, 8.2e-9, 0.001_515_151_515_151_515_2),
            (0x2e, 8.2e-9, 0.0),
            (0x36, -8.2e-9, -6.097_560_975_609_756_4e-6),
        ] {
            set(k, sr * m + c);
        }
        set(0x3f, f64::from_bits(0x3f23_dc01_3dc0_13dc));
        set(0x00, (i5 + i3) * 1650.0 + 0.5);
        set(0x08, i3 * -1650.0);
        set(0x20, i5 * -1650.0);
        set(0x01, i3 * 0.5 + d7);
        set(0x09, (i3 * -0.5 - d7) - sr * 1.2e-6);
        set(0x04, i5 * -0.5 - d8);
        set(0x24, (i5 * 0.5 + d8) + sr * 2.7e-7);
        set(0x07, -0.000_151_515_151_515_151_52 - (d8 + d7));
        set(0x0f, d7);
        set(0x27, d8);
        let one = 1.0f32;
        let t32 = one / core as f32;
        let x0 = t32 * 3.141_592_74;
        let q = t32 * t32;
        let (a9, a10) = (t32 * 15.915, t32 * 20.941);
        let g8 = ((q * a9) * 84.429_077_1 + a9) + ((q * q) * a9) * 8_553.921_88;
        let g9 = ((q * a10) * 146.175_171 + a10) + ((q * q) * a10) * 25_640.613_3;
        Self {
            mode,
            level: p[5],
            hi: Svf::new(x0 * 15_000.0, x0 * x0, 75_000_000.0, 6.750_000_13e15),
            lo: Svf::new(x0 * 10.0, x0 * x0, 33.333_335_9, 1_333.333_37),
            c744: g9 / (g9 + one),
            c750: g8 / (g8 + one),
            diode,
            stack,
            mna: invert(g),
            pots,
            sr,
            oversampler,
        }
    }

    /// 0x141998cf0.
    #[allow(clippy::excessive_precision)]
    fn diode(&self, s: &mut [f64; 4], x: f64) -> f64 {
        let k = &self.diode;
        let mut v = s[3];
        let d3 = ((1.0 - k.k38) * s[1] + (x - s[0])) / (k.k38 + 1.0);
        let d2 = ((d3 / k.k10) * k.k70 + k.k78 * s[2]) - k.k80 * v;
        for _ in 0..20 {
            let sg = if v < 0.0 { -1.0 } else { 1.0 };
            let e = (sg + sg) * v + 1.0;
            let r = 1.0 / e;
            let bits = e.to_bits();
            let mm = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
            let ex = (((bits >> 52) & 0x7ff) as i64 - 0x3ff) as f64;
            let d10 = (r * r) * -4.0;
            let d9 = 1.0 / ((r + r) + k.k88);
            let lg = ((((mm * 0.164_042_561_333_445_2 - 1.098_865_286_222_744) * mm + 3.148_297_929_334_117) * mm + ex)
                - 2.213_475_204_444_817)
                * 0.693_147_180_559_945;
            let mut dl = -(((lg * sg - d2 * k.k60) + k.k88 * v) * d9);
            let d4 = (((d10 * d9) * dl) * 0.5) + 1.0;
            let h = 1.0 / ((((((dl * dl) * d9) * r) * 2.666_666_666_666_666_5) * r) * r + ((d9 * d10) * dl) + 1.0);
            let h = if h <= 0.0 { 1.0 / d4 } else { h * d4 };
            let c = if h > 8.0 { 8.0 } else if 0.5 <= h { h } else { 0.5 };
            dl *= c;
            v += dl;
            if !(1e-4 <= dl.abs()) {
                break;
            }
        }
        s[0] = x;
        s[1] = d3;
        let d2 = d2 - k.k80 * v;
        s[3] = v;
        s[2] = d2;
        x + d2
    }

    /// 0x141999030 then 0x141998930.
    #[allow(clippy::excessive_precision)]
    fn circuit2(&self, s: &mut State, x: f64) -> f64 {
        let st = &self.stack;
        let u = x + s.previous;
        let t: [f64; 4] = std::array::from_fn(|i| {
            let sum = (((s.stack[0] * st.m1[0][i]) + s.stack[1] * st.m1[1][i]) + s.stack[2] * st.m1[2][i]) + s.stack[3] * st.m1[3][i];
            sum + (u * st.p4) * if i == 0 { 1.0 } else { 0.0 }
        });
        s.stack = std::array::from_fn(|i| (((t[0] * st.m2[0][i]) + t[1] * st.m2[1][i]) + t[2] * st.m2[2][i]) + t[3] * st.m2[3][i]);
        s.previous = x;
        let x = s.stack[3] * st.p1;
        const K: f64 = 1650.0;
        const H: f64 = 0.5;
        const C3: f64 = 0.001_515_151_515_151_515_2;
        const C4: f64 = 0.000_151_515_151_515_151_52;
        const C6: f64 = 6.097_560_975_609_756_4e-6;
        let [p3, p4, p5, p6] = self.pots;
        let n = &s.mna;
        let (d1, d2, d3, d4) = (n[1], n[2], n[5], self.sr);
        let (d8, d6) = (H / p4, H / p6);
        let r0 = (((x + n[0]) * H - ((1.0 / p5 + 1.0 / p3) * K + H) * d1) + ((1.0 / p3) * K) * d2) + ((1.0 / p5) * K) * d3;
        let (d9, d7) = (H / p3 + d8, H / p5 + d6);
        let d5 = n[3];
        let d11 = (d4 * 1.2e-6) * d2;
        let r1 = ((d2 * d9 - d1 * d9) - d11) + (d5 * d4) * 1.2e-6;
        let d9 = n[4];
        let d10 = (d4 * 4.7e-8) * d9;
        let r3 = ((d4 * 4.7e-8) * d5 - d10) + d9 * C6;
        let r2 = (((d11 - (d4 * d5) * 1.247e-6) + d5 * C3) + d10) - d9 * C3;
        let d10 = (d4 * 2.7e-7) * d3;
        let d5 = n[6];
        let r4 = ((d1 * d7 - d3 * d7) + d10) - (d5 * d4) * 2.7e-7;
        let d9 = n[7];
        let r5 = (((d10 - (d4 * 2.782e-7) * d5) + d5 * C3) + (d9 * d4) * 8.2e-9) - d9 * C3;
        let r6 = ((d4 * 8.2e-9) * d5 - (d4 * 8.2e-9) * d9) + d9 * C6;
        let r7 = ((((d6 + d8) + C4) * d1 - d2 * d8) - d6 * d3) - n[8] * C4;
        let r = [r0, r1, r2, r3, r4, r5, r6, r7];
        let sol: [f64; 8] = std::array::from_fn(|i| (0..8).fold(0.0, |acc, j| acc + self.mna[i][j] * r[j]));
        s.mna[0] = x;
        s.mna[1..].copy_from_slice(&sol);
        sol[7]
    }

    #[allow(clippy::excessive_precision)]
    #[inline(always)]
    fn tick(&self, s: &mut State, l: f32, r: f32) -> f32 {
        let x = r + l;
        let x = (0.75 / ((x * x) * 0.001_406_250_06 + 1.0).sqrt()) * x;
        let (lp, _) = self.hi.tick(&mut s.hi, x);
        let t = (lp - s.z74c) * self.c744;
        let y = t + s.z74c;
        s.z74c = t + y;
        let u = (lp - y) * 0.235_294_119 - 0.179_284_289;
        let v = (u / (u * u + 1.0).sqrt()) * 4.25 + 0.75;
        let t = (v - s.z758) * self.c750;
        let y2 = t + s.z758;
        s.z758 = y2 + t;
        let d = self.diode(&mut s.diode, f64::from(v - y2));
        let o = if self.mode == 2 {
            self.circuit2(s, d) as f32
        } else {
            let st = &self.stack;
            let mut t = Stack::apply(&st.m1, &s.stack);
            t[0] += st.p4 * (d + s.previous);
            s.stack = Stack::apply(&st.m2, &t);
            s.previous = d;
            (s.stack[3] * st.p1) as f32
        };
        let (_, hp) = self.lo.tick(&mut s.lo, o);
        (hp * self.level) * 0.529_999_971
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        let n = self.oversampler.factor();
        for i in 0..len {
            let mut up = [[0f32; kaiser::MAX]; 2];
            for c in 0..2 {
                self.oversampler.up(&mut s.up[c], block[c][i] as f32, &mut up[c][..n]);
            }
            let frame: [f32; kaiser::MAX] = std::array::from_fn(|p| if p < n { self.tick(s, up[0][p], up[1][p]) } else { 0.0 });
            // Both channels carry the same signal; one decimator serves them.
            let y = f64::from(self.oversampler.down(&mut s.down, &frame[..n]));
            block[0][i] = y;
            block[1][i] = y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emulated native core (kcolor/vec_tubescreamer.json, 48 kHz): an impulse of 0.5
    /// (128 frames) and a 0.5 sine at 200 Hz, frames 1920..2400, all three circuits.
    #[test]
    fn matches_the_native_core() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("tables/kontakt_tubescreamer_vectors.json")).unwrap();
        const BLOCK: usize = super::super::super::BLOCK;
        for case in cases.as_array().unwrap() {
            let mut knobs = DEFAULTS;
            for (k, v) in case["params"].as_object().unwrap() {
                knobs[k.parse::<usize>().unwrap()] = v.as_f64().unwrap() as f32;
            }
            let unit = Engine::new(Tubescreamer { knobs }, 48_000);
            let run = |input: &dyn Fn(usize) -> f32, frames: usize| {
                let mut s = State::default();
                // The vectors follow 4800 frames of silence (the bias settles).
                for _ in 0..4800 / BLOCK {
                    unit.process(&mut s, &mut [[0.0; BLOCK]; 2], BLOCK);
                }
                let mut out = Vec::new();
                for start in (0..frames).step_by(BLOCK) {
                    let mut block: Planar = [[0.0; BLOCK]; 2];
                    for i in 0..BLOCK {
                        let x = f64::from(input(start + i));
                        block[0][i] = x;
                        block[1][i] = x;
                    }
                    unit.process(&mut s, &mut block, BLOCK);
                    out.extend(block[0].iter().map(|&v| v as f32));
                }
                out
            };
            let check = |name: &str, out: &[f32], native: &serde_json::Value, at: usize| {
                for (i, n) in native.as_array().unwrap().iter().enumerate() {
                    let n = n.as_f64().unwrap() as f32;
                    assert!((out[at + i] - n).abs() < 2e-6, "{knobs:?} {name} {i}: {} vs {n}", out[at + i]);
                }
            };
            check("impulse", &run(&|i| if i == 0 { 0.5 } else { 0.0 }, 128), &case["impulse_L"], 0);
            let sine = |i: usize| (0.5 * (std::f64::consts::TAU * 200.0 * i as f64 / 48_000.0).sin()) as f32;
            check("sine", &run(&sine, 2400), &case["sine_L_settled_1920_2400"], 1920);
        }
    }
}
