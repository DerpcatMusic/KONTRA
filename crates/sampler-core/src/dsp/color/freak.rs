//! Kontakt Freak (effektrig core: ctor 0x141963480, setters 0x141964260.., prepare 0x141966110,
//! process 0x141966ad0): an envelope-gated drive whose signal is mixed with a quadrature
//! oscillator or a side-chain follower (8-stage all-pass Hilbert pair), fed through an implicit
//! feedback stage (fixed point, at most 4 passes), an ADAA `sqrt(x^2 + 1) - 1` shaper on the
//! sum and difference branches, and in mode 0 the fuzz chain (MINSTD noise, band-pass, ADAA
//! rectifier, smoothing SVFs, sine ring-mod, level follower). Model kcolor/freak.py: the core
//! runs on its native object layout (offsets 0x940..0x11e0), so the setters port as written.
// ponytail: the Hilbert coefficient design 0x141968890 is not ported; its output is dumped for
// 44.1/48/88.2/96 kHz and other rates use the nearest (upgrade: port the designer).
use super::super::Planar;
use super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};
use sampler_ir::fx::color::Freak;

const BASE: usize = 0x940;
const SIZE: usize = 0x11e0 - BASE;

/// A byte image of the core object from offset `B`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mem<const N: usize, const B: usize>([u8; N]);

impl<const N: usize, const B: usize> Default for Mem<N, B> {
    fn default() -> Self {
        Self([0; N])
    }
}

impl<const N: usize, const B: usize> Mem<N, B> {
    fn bytes<const K: usize>(&self, o: usize) -> [u8; K] {
        self.0[o - B..o - B + K].try_into().unwrap()
    }
    fn f(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.bytes(o))
    }
    fn d(&self, o: usize) -> f64 {
        f64::from_le_bytes(self.bytes(o))
    }
    fn i(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.bytes(o))
    }
    fn f4(&self, o: usize) -> [f32; 4] {
        std::array::from_fn(|k| self.f(o + 4 * k))
    }
    fn d2(&self, o: usize) -> [f64; 2] {
        [self.d(o), self.d(o + 8)]
    }
    fn wf(&mut self, o: usize, v: f32) {
        self.0[o - B..o - B + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn wd(&mut self, o: usize, v: f64) {
        self.0[o - B..o - B + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn wi(&mut self, o: usize, v: u32) {
        self.0[o - B..o - B + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn wf4(&mut self, o: usize, v: [f32; 4]) {
        for (k, x) in v.into_iter().enumerate() {
            self.wf(o + 4 * k, x);
        }
    }
    fn wd2(&mut self, o: usize, v: [f64; 2]) {
        self.wd(o, v[0]);
        self.wd(o + 8, v[1]);
    }
    fn pair(&mut self, o: usize, v: f64) {
        self.wd2(o, [v, v]);
    }
}

type Image = Mem<0x1400, 0>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct State {
    m: Mem<SIZE, BASE>,
    ready: bool,
}

impl Default for State {
    fn default() -> Self {
        Self { m: Mem::default(), ready: false }
    }
}

impl State {
    pub(super) fn finite(&self) -> bool {
        [0x1014, 0xd54, 0x1030].iter().all(|&o| self.m.f(o).is_finite())
            && [0xce0, 0x1130, 0x1160, 0x1170, 0xfb0].iter().all(|&o| self.m.d2(o).iter().all(|v| v.is_finite()))
    }
}

pub(super) struct Engine {
    image: Box<Mem<SIZE, BASE>>,
    d00: f32,
}

fn u(b: u32) -> f32 {
    f32::from_bits(b)
}
const PI_F: f32 = std::f32::consts::PI;
const C3: f64 = 1.0 / 3.0;
const C5: f64 = 0.133_333_333_333_333_33;
const TANMAX: f64 = f64::from_bits(0x3ff8229c779a6b51);
const K_FC: f32 = f32::from_bits(0x44acbc36);
const K_D30: f32 = f32::from_bits(0x44eb3c36);
const K_SVF: f32 = f32::from_bits(0x44c1fc36);
/// Constructor 0x141963480 image: nonzero words in 0x940..0x11f0 (std::function words dropped).
#[rustfmt::skip]
const CTOR: [(usize, u32); 55] = [
    (0x940, 0x36be37c6), (0x960, 0x3f800000), (0x964, 0x3f800000), (0x968, 0x3f800000), (0x96c, 0x3f800000), (0xd10, 0x3e99999a),
    (0xd20, 0x40000000), (0xd24, 0x3fd33333), (0xd28, 0x40000000), (0xd2c, 0x3fd33333), (0xd34, 0x459c4000), (0xd50, 0x472c4400),
    (0xd58, 0x3ba3d70a), (0xd5c, 0x3c23d70a), (0xd60, 0x3ca95120), (0xd64, 0x3c2a3380), (0xd68, 0xc0935d8e), (0xda4, 0x3ff00000),
    (0xdac, 0x3ff00000), (0xe14, 0x3e500000), (0xe1c, 0x3e500000), (0xe74, 0x3ff00000), (0xe7c, 0x3ff00000), (0xed0, 0x240b7803),
    (0xed4, 0x3fd87e28), (0xed8, 0x240b7803), (0xedc, 0x3fd87e28), (0xee0, 0xbb98c7e3), (0xee4, 0x3fed9096), (0xee8, 0xbb98c7e3),
    (0xeec, 0x3fed9096), (0xf14, 0x3ff00000), (0xf1c, 0x3ff00000), (0x1010, 0x472c4400), (0x1018, 0x3ba3d70a), (0x101c, 0x3dcccccd),
    (0x1020, 0x3ca95120), (0x1024, 0x3a88ce00), (0x1028, 0xc0935d8e), (0x102c, 0x472c4400), (0x1034, 0x3b03126f), (0x1038, 0x3e4ccccd),
    (0x103c, 0x3d506060), (0x1040, 0x3a08d800), (0x1044, 0xc0935d8e), (0x104c, 0x00003039), (0x1050, 0x00003039), (0x1070, 0x3f800000),
    (0x1074, 0x3f800000), (0x10c0, 0xbf800000), (0x10c4, 0x3f800000), (0x1190, 0x39b504f3), (0x1194, 0x39b504f3), (0x1198, 0x39b504f3),
    (0x119c, 0x39b504f3),
];

/// Hilbert stage tables by rate (0x141968890 output, dumped at 44.1/48/88.2/96 kHz): gain 0x960, all-pass 0x9c0, FIR 0xc40.
#[rustfmt::skip]
const HILBERT: [(u32, [f32; 4], [[f32; 4]; 8], [[f32; 4]; 8]); 4] = [
    (44100, [f32::from_bits(0x3d2e15ec), f32::from_bits(0x3d2e15ec), f32::from_bits(0x3c935548), f32::from_bits(0x3c935548)], [[f32::from_bits(0x3f3ade56), f32::from_bits(0x3f3ade56), f32::from_bits(0xbf7f3a1a), f32::from_bits(0xbf7f3a1a)], [f32::from_bits(0x3e6af385), f32::from_bits(0x3e6af385), f32::from_bits(0xbf7ce90d), f32::from_bits(0xbf7ce90d)], [f32::from_bits(0xbebd86ec), f32::from_bits(0xbebd86ec), f32::from_bits(0xbf756eee), f32::from_bits(0xbf756eee)], [f32::from_bits(0xbf438687), f32::from_bits(0xbf438687), f32::from_bits(0xbf5d77b0), f32::from_bits(0xbf5d77b0)], [f32::from_bits(0xbf6cc1f2), f32::from_bits(0xbf6cc1f2), f32::from_bits(0xbf1a3d06), f32::from_bits(0xbf1a3d06)], [f32::from_bits(0xbf7a411d), f32::from_bits(0xbf7a411d), f32::from_bits(0xbda1e0ec), f32::from_bits(0xbda1e0ec)], [f32::from_bits(0xbf7e61bb), f32::from_bits(0xbf7e61bb), f32::from_bits(0x3f01bd71), f32::from_bits(0x3f01bd71)], [f32::from_bits(0xbf7fc597), f32::from_bits(0xbf7fc597), f32::from_bits(0x3f69859d), f32::from_bits(0x3f69859d)]], [[f32::from_bits(0x3c80ce16), f32::from_bits(0x3c80ce16), f32::from_bits(0xbd138e67), f32::from_bits(0xbd138e67)], [f32::from_bits(0x3e483b53), f32::from_bits(0x3e483b53), f32::from_bits(0x3d14ae9b), f32::from_bits(0x3d14ae9b)], [f32::from_bits(0xbf7075ac), f32::from_bits(0xbf7075ac), f32::from_bits(0xbd18ce04), f32::from_bits(0xbd18ce04)], [f32::from_bits(0x3fcaa2d3), f32::from_bits(0x3fcaa2d3), f32::from_bits(0x3d28305d), f32::from_bits(0x3d28305d)], [f32::from_bits(0xbfef1d03), f32::from_bits(0xbfef1d03), f32::from_bits(0xbd6fd946), f32::from_bits(0xbd6fd946)], [f32::from_bits(0x3ffb4d8b), f32::from_bits(0x3ffb4d8b), f32::from_bits(0x3eff67c8), f32::from_bits(0x3eff67c8)], [f32::from_bits(0xbffef6a4), f32::from_bits(0xbffef6a4), f32::from_bits(0x3ee658f4), f32::from_bits(0x3ee658f4)], [f32::from_bits(0x3fffe2cc), f32::from_bits(0x3fffe2cc), f32::from_bits(0x3db3d318), f32::from_bits(0x3db3d318)]]),
    (48000, [f32::from_bits(0xbb00577b), f32::from_bits(0xbb00577b), f32::from_bits(0x3d3b18e1), f32::from_bits(0x3d3b18e1)], [[f32::from_bits(0x3f147a57), f32::from_bits(0x3f147a57), f32::from_bits(0xbf7f5601), f32::from_bits(0xbf7f5601)], [f32::from_bits(0xbc06fe6a), f32::from_bits(0xbc06fe6a), f32::from_bits(0xbf7d7844), f32::from_bits(0xbf7d7844)], [f32::from_bits(0xbf09705e), f32::from_bits(0xbf09705e), f32::from_bits(0xbf77d507), f32::from_bits(0xbf77d507)], [f32::from_bits(0xbf542dbe), f32::from_bits(0xbf542dbe), f32::from_bits(0xbf669a49), f32::from_bits(0xbf669a49)], [f32::from_bits(0xbf7185c4), f32::from_bits(0xbf7185c4), f32::from_bits(0xbf366671), f32::from_bits(0xbf366671)], [f32::from_bits(0xbf7b6db0), f32::from_bits(0xbf7b6db0), f32::from_bits(0xbe97f6ce), f32::from_bits(0xbe97f6ce)], [f32::from_bits(0xbf7ea399), f32::from_bits(0xbf7ea399), f32::from_bits(0x3e968dae), f32::from_bits(0x3e968dae)], [f32::from_bits(0xbf7fcd38), f32::from_bits(0xbf7fcd38), f32::from_bits(0x3f5a5bc7), f32::from_bits(0x3f5a5bc7)]], [[f32::from_bits(0xbab9e166), f32::from_bits(0xbab9e166), f32::from_bits(0xbdbb572b), f32::from_bits(0xbdbb572b)], [f32::from_bits(0x3ed38be5), f32::from_bits(0x3ed38be5), f32::from_bits(0x3dbc8522), f32::from_bits(0x3dbc8522)], [f32::from_bits(0xbf96288e), f32::from_bits(0xbf96288e), f32::from_bits(0xbdc0940d), f32::from_bits(0xbdc0940d)], [f32::from_bits(0x3fd79639), f32::from_bits(0x3fd79639), f32::from_bits(0x3dce7a1a), f32::from_bits(0x3dce7a1a)], [f32::from_bits(0xbff2d4f4), f32::from_bits(0xbff2d4f4), f32::from_bits(0xbe028aed), f32::from_bits(0xbe028aed)], [f32::from_bits(0x3ffc2b7e), f32::from_bits(0x3ffc2b7e), f32::from_bits(0x3ea687f9), f32::from_bits(0x3ea687f9)], [f32::from_bits(0xbfff1f27), f32::from_bits(0xbfff1f27), f32::from_bits(0x3f1a2677), f32::from_bits(0x3f1a2677)], [f32::from_bits(0x3fffe69c), f32::from_bits(0x3fffe69c), f32::from_bits(0x3e1690e4), f32::from_bits(0x3e1690e4)]]),
    (88200, [f32::from_bits(0x3b9552bc), f32::from_bits(0x3b9552bc), f32::from_bits(0xbddfbe18), f32::from_bits(0xbddfbe18)], [[f32::from_bits(0xbc2f4ebd), f32::from_bits(0xbc2f4ebd), f32::from_bits(0xbf7fadac), f32::from_bits(0xbf7fadac)], [f32::from_bits(0xbf11103c), f32::from_bits(0xbf11103c), f32::from_bits(0xbf7edd1a), f32::from_bits(0xbf7edd1a)], [f32::from_bits(0xbf5423f9), f32::from_bits(0xbf5423f9), f32::from_bits(0xbf7ca8da), f32::from_bits(0xbf7ca8da)], [f32::from_bits(0xbf6ff54d), f32::from_bits(0xbf6ff54d), f32::from_bits(0xbf76714e), f32::from_bits(0xbf76714e)], [f32::from_bits(0xbf7a5661), f32::from_bits(0xbf7a5661), f32::from_bits(0xbf654d04), f32::from_bits(0xbf654d04)], [f32::from_bits(0xbf7e0a4c), f32::from_bits(0xbf7e0a4c), f32::from_bits(0xbf3954e4), f32::from_bits(0xbf3954e4)], [f32::from_bits(0xbf7f5cc7), f32::from_bits(0xbf7f5cc7), f32::from_bits(0xbeada968), f32::from_bits(0xbeada968)], [f32::from_bits(0xbf7fe6f4), f32::from_bits(0xbf7fe6f4), f32::from_bits(0x3f069a6e), f32::from_bits(0x3f069a6e)]], [[f32::from_bits(0xbedc636e), f32::from_bits(0xbedc636e), f32::from_bits(0x3e5fe21e), f32::from_bits(0x3e5fe21e)], [f32::from_bits(0x3f96b782), f32::from_bits(0x3f96b782), f32::from_bits(0xbe6085fd), f32::from_bits(0xbe6085fd)], [f32::from_bits(0xbfd44bb3), f32::from_bits(0xbfd44bb3), f32::from_bits(0x3e6282a3), f32::from_bits(0x3e6282a3)], [f32::from_bits(0x3feff24d), f32::from_bits(0x3feff24d), f32::from_bits(0xbe686b3b), f32::from_bits(0xbe686b3b)], [f32::from_bits(0xbffa8254), f32::from_bits(0xbffa8254), f32::from_bits(0x3e7aa0e0), f32::from_bits(0x3e7aa0e0)], [f32::from_bits(0x3ffe49a9), f32::from_bits(0x3ffe49a9), f32::from_bits(0xbe9d6a1d), f32::from_bits(0xbe9d6a1d)], [f32::from_bits(0xbfff9560), f32::from_bits(0xbfff9560), f32::from_bits(0x3f344225), f32::from_bits(0x3f344225)], [f32::from_bits(0x3ffff37a), f32::from_bits(0x3ffff37a), f32::from_bits(0x3ef2cb24), f32::from_bits(0x3ef2cb24)]]),
    (96000, [f32::from_bits(0x3d008399), f32::from_bits(0x3d008399), f32::from_bits(0xbdf733f8), f32::from_bits(0xbdf733f8)], [[f32::from_bits(0xbd89cd6a), f32::from_bits(0xbd89cd6a), f32::from_bits(0xbf7fb4b7), f32::from_bits(0xbf7fb4b7)], [f32::from_bits(0xbf1a5ee7), f32::from_bits(0xbf1a5ee7), f32::from_bits(0xbf7ef6bd), f32::from_bits(0xbf7ef6bd)], [f32::from_bits(0xbf583bea), f32::from_bits(0xbf583bea), f32::from_bits(0xbf7cf6ed), f32::from_bits(0xbf7cf6ed)], [f32::from_bits(0xbf717a39), f32::from_bits(0xbf717a39), f32::from_bits(0xbf77573a), f32::from_bits(0xbf77573a)], [f32::from_bits(0xbf7adcdb), f32::from_bits(0xbf7adcdb), f32::from_bits(0xbf67d3f6), f32::from_bits(0xbf67d3f6)], [f32::from_bits(0xbf7e374e), f32::from_bits(0xbf7e374e), f32::from_bits(0xbf3fb20f), f32::from_bits(0xbf3fb20f)], [f32::from_bits(0xbf7f6aec), f32::from_bits(0xbf7f6aec), f32::from_bits(0xbec69601), f32::from_bits(0xbec69601)], [f32::from_bits(0xbf7fe915), f32::from_bits(0xbf7fe915), f32::from_bits(0x3ef766ce), f32::from_bits(0x3ef766ce)]], [[f32::from_bits(0xbefecf1b), f32::from_bits(0xbefecf1b), f32::from_bits(0x3e77585c), f32::from_bits(0x3e77585c)], [f32::from_bits(0x3f9eaa9f), f32::from_bits(0x3f9eaa9f), f32::from_bits(0xbe77fd7d), f32::from_bits(0xbe77fd7d)], [f32::from_bits(0xbfd829d1), f32::from_bits(0xbfd829d1), f32::from_bits(0x3e79fbe6), f32::from_bits(0x3e79fbe6)], [f32::from_bits(0x3ff169dc), f32::from_bits(0x3ff169dc), f32::from_bits(0xbe7fe07c), f32::from_bits(0xbe7fe07c)], [f32::from_bits(0xbffb0148), f32::from_bits(0xbffb0148), f32::from_bits(0x3e88ec90), f32::from_bits(0x3e88ec90)], [f32::from_bits(0x3ffe704f), f32::from_bits(0x3ffe704f), f32::from_bits(0xbea7cff4), f32::from_bits(0xbea7cff4)], [f32::from_bits(0xbfff9e92), f32::from_bits(0xbfff9e92), f32::from_bits(0x3f2bae05), f32::from_bits(0x3f2bae05)], [f32::from_bits(0x3ffff48a), f32::from_bits(0x3ffff48a), f32::from_bits(0x3f044c99), f32::from_bits(0x3f044c99)]]),
];

/// Init slot 1 defaults, core values p0..p18.
pub(super) const DEFAULTS: [f32; 19] =
    [0.0, 0.5, 1.0, 0.5, 0.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.5, 0.0, 1.0, 0.5, 0.5, 0.0, 0.5, 1.0, 1.0];

/// One-pole `g = P / (P + 1)`, `P` the tan series (0x1419661cb order).
fn tanpoly(x: f64) -> f64 {
    let d = x * x;
    let p = (d * d) * (x * C5) + ((x * C3) * d + x);
    p / (p + 1.0)
}

/// Clamped tan series of the TPT SVF blocks (0x1419658f4 order).
fn tanc(x: f64) -> f64 {
    let x = x.clamp(0.0, TANMAX);
    let d = x * x;
    ((C3 * x) * d + x) + (C5 * x) * (d * d)
}

/// musl `logf` as inlined in 0x141968a40.
fn logf(t: f32) -> f32 {
    if t == 0.0 {
        return f32::NEG_INFINITY;
    }
    if t < 0.0 || t.is_nan() {
        return f32::NAN;
    }
    if t == f32::INFINITY {
        return t;
    }
    let (t, eadj) = if t < 1.175_494_35e-38 { (t * 8_388_608.0, -23) } else { (t, 0) };
    let ix = (t.to_bits() as i32).wrapping_add(0x004a_fb0d);
    let m = f32::from_bits(((ix & 0x007f_ffff) + 0x3f35_04f3) as u32) - 1.0;
    let k = ((ix >> 23) - 0x7f + eadj) as f32;
    let s = m / (2.0 + m);
    let z = s * s;
    let w = z * z;
    let hf = (0.5 * m) * m;
    let t1 = (0.242_790_788 * w + 0.400_009_722) * w;
    let t2 = (0.284_987_867 * w + 0.666_666_627) * z;
    ((((t1 + t2) + hf) * s + -0.000_212_194_442 * k) - hf) + m + 0.693_359_375 * k
}

/// `F(q) = 0.5 (asinh q + q sqrt(q^2 + 1)) - q`, the antiderivative of `sqrt(q^2 + 1) - 1`.
fn adaa_f(q: f32) -> f32 {
    let r = (q * q + 1.0).sqrt();
    (logf(q + r) + q * r) * 0.5 - q
}

/// The fixed four-lane sine of the oscillators.
fn sine(t: f32) -> f32 {
    let t2 = t * t;
    (1.570_796_37 * t - (0.641_592_681 * t) * t2) + (0.070_796_325_8 * t) * (t2 * t2)
}

fn fuzz_hz(m: &Image) -> f32 {
    let (p12, p13) = (m.f(0xa0), m.f(0xa4));
    let a = p12 * 40.0 + 100.0;
    let b = 24.0 - p12 * 12.0;
    let dd = a - b;
    table((((a + b) - dd) * p13 + dd) * 5.0 + K_FC)
}

fn fuzz_filters(m: &mut Image, x: f32) {
    let xd = f64::from(x);
    m.pair(0xe20, xd);
    m.pair(0xe40, xd);
    let g = tanc(xd * m.d(0xe70));
    m.pair(0xe50, g);
    m.pair(0xe90, g);
    let k = m.d(0xe60);
    m.pair(0xea0, 1.0 / (((g * k + g * k) + 1.0) + g * g));
    m.pair(0xe80, k + k);
    m.pair(0xef0, 0.5 * xd);
    svf2(m);
    let r = m.f(0x1064);
    if x >= 0.0 && (r == 0.0 || x <= 1.0 / r) {
        m.wf(0x1068, x);
        m.wf(0x1060, r * x);
    }
}

fn svf2(m: &mut Image) {
    let g = tanc(m.d(0xef0) * m.d(0xf10));
    m.pair(0xf00, g);
    m.pair(0xf30, g);
    m.pair(0xf80, g);
    let (a, b) = (m.d(0xed0), m.d(0xee0));
    m.pair(0xf40, 1.0 / (((g * a + g * a) + 1.0) + g * g));
    m.pair(0xf20, a + a);
    m.pair(0xf90, 1.0 / (((b * g + b * g) + 1.0) + g * g));
    m.pair(0xf70, b + b);
}

fn cutoff(m: &mut Image) {
    let lg = (m.f(0xd30).abs() + 200.0).ln();
    let cf = table(((lg * u(0x418a7fac) - u(0x4211815a)) * m.f(0x70)) * 5.0 + K_FC);
    m.wf(0xcf0, cf);
    m.pair(0xcc0, tanpoly(f64(m.f(0x940) * PI_F * cf)));
}

fn f64(v: f32) -> f64 {
    f64::from(v)
}

fn rate(m: &mut Image) {
    if m.i(0x11e0) == 1 {
        let t = (m.f(0x74) + m.f(0x74)) - 1.0;
        m.wf(0xd30, (t * m.f(0xd34)) * (t * t));
    } else {
        m.wf(0xd30, table(m.f(0xa0) * 200.0 + K_D30));
        let hz = fuzz_hz(m);
        fuzz_filters(m, hz);
    }
    m.wf(0xd14, m.f(0x940) * m.f(0xd30));
    cutoff(m);
}

fn mode_targets(m: &mut Image) -> f32 {
    let (d00, d38, d44, e00) = [(0.0, 0.0, 1.0, 1.0), (1.0, 0.0, 1.0, 0.0), (0.0, 1.0, -1.0, 0.0)][m.i(0x11e0) as usize];
    m.wf(0xd44, d44);
    m.wf(0xd38, d38);
    m.wf(0x954, d44 * m.f(0x94c));
    m.pair(0xd00, f64(d00));
    m.wf(0xe00, e00);
    d00
}

fn env_coefs(m: &mut Image, b: usize) {
    let sr = m.f(b);
    for k in [2, 3] {
        let t = m.f(b + 4 * k);
        let v = if t > 0.0 { 1.0 - (f64(m.f(b + 24) / (t * sr))).exp() as f32 } else { 1.0 };
        m.wf(b + 4 * k + 8, v);
    }
}

fn svf_d70(m: &mut Image) {
    let v = f64(m.f(0xd48));
    m.pair(0xd70, v);
    let g = tanc(v * m.d(0xda0));
    m.pair(0xd80, g);
    m.pair(0xdc0, g);
    let k = m.d(0xd90);
    m.pair(0xdd0, 1.0 / (((k * g + k * g) + 1.0) + g * g));
    m.pair(0xdb0, k + k);
}

/// Setter `i` (0x141964260..); returns the new `d00` when the mode changes.
fn set(m: &mut Image, i: usize, d00: &mut f32) {
    let p = m.f(0x70 + 4 * i);
    match i {
        0 => cutoff(m),
        1 | 12 => rate(m),
        2 => {
            m.wf(0xd34, if m.f(0x590) < 0.5 { 200.0 } else { 5000.0 });
            rate(m);
        }
        3 => {
            let t = ((p - 0.5) + p) - 0.5;
            let s = t * t;
            let (a, b) = if t < 0.0 { (0.0, s) } else { (s, 0.0) };
            m.wf(0x94c, a);
            m.wf(0x954, a * m.f(0xd44));
            m.wf(0x950, b);
        }
        4 => {
            let e = p * 3.0 + 1.0;
            let k = p * 13.5 + 0.5;
            m.wf(0xe04, e);
            m.wf(0x1180, k);
            m.wf(0x1184, (k * k * 0.5 + 1.0).sqrt() * (0.5 / (k * k)));
            m.wf(0xe08, 1.0 / e.sqrt());
        }
        5 => m.wf(0xd40, p),
        6 => {
            let c = p * p * p;
            m.wf(0xd58, c * u(0x3e47ae15) + u(0x3ba3d70a));
            m.wf(0xd5c, c * u(0x411fd70a) + u(0x3c23d70a));
            env_coefs(m, 0xd50);
        }
        7 => {
            m.wf(0xd48, table(p * 455.0 + K_SVF));
            svf_d70(m);
        }
        8 => m.wf(0xd3c, p),
        9 => m.wf(0xd4c, p),
        10 => m.wf(0xd18, ((p - 0.5) + p) - 0.5),
        11 => {
            let a = p * 0.9 * p;
            m.wf(0x1100, a);
            m.wf(0x1104, (1.0 - a * a).sqrt());
            m.wf(0x1048, a * a * u(0x3e991688) + u(0x3a83126f));
        }
        13 => {
            let hz = fuzz_hz(m);
            fuzz_filters(m, hz);
        }
        14 => {
            let v = p * 1.9 + 0.1;
            let mk = if v <= 1.0 { 1.0 / (v * u(0x3f0e38e3) + u(0x3ee38e39)) } else { 1.0 };
            let o = 2f64.powf(f64(v * 0.5));
            let k = (o * o - 1.0) / (o * 2.0);
            m.pair(0xe60, k);
            let g = m.d(0xe50);
            m.pair(0xe90, g);
            m.pair(0xea0, 1.0 / (((g * k + g * k) + 1.0) + g * g));
            m.pair(0xe80, k + k);
            m.pair(0xe30, f64(mk));
        }
        15 => m.wf(0x1054, p),
        16 => {
            m.wi(0x11e0, ((p * 3.0) as i32).clamp(0, 2) as u32);
            *d00 = mode_targets(m);
            if m.i(0x11e0) < 2 {
                rate(m);
            } else {
                m.wf(0xd40, m.f(0x84));
            }
        }
        17 => m.wf(0x958, p),
        18 => {
            m.wf(0x948, p.sqrt());
            m.wf(0x944, (1.0 - p).sqrt());
        }
        _ => {}
    }
}

fn prepare(m: &mut Image, sr: u32) {
    let srf = sr as f32;
    let inv = 1.0 / srf;
    m.wf(0x940, inv);
    let w = inv * PI_F;
    m.wf(0xd14, inv * m.f(0xd30));
    m.pair(0xcc0, tanpoly(f64(w * m.f(0xcf0))));
    m.pair(0x1110, tanpoly(f64(w * 20000.0 * 0.75)));
    m.pair(0x1140, tanpoly(f64(w * 20.0)));
    let (_, gain, allpass, fir) = HILBERT.iter().min_by_key(|h| h.0.abs_diff(sr)).expect("tables");
    m.wf4(0x960, *gain);
    for j in 0..8 {
        m.wf4(0x9c0 + 16 * j, allpass[j]);
        m.wf4(0xc40 + 16 * j, fir[j]);
    }
    for b in [0xd50, 0x1010, 0x102c] {
        m.wf(b, srf);
        env_coefs(m, b);
    }
    let pi_rate = std::f64::consts::PI / f64(srf);
    m.pair(0xda0, pi_rate);
    svf_d70(m);
    let o = 2f64.powf(0.5 * 3.5);
    m.pair(0xd90, (o * o - 1.0) / (o * 2.0));
    svf_d70(m);
    m.pair(0xe70, pi_rate);
    let g = tanc(m.d(0xe40) * m.d(0xe70));
    m.pair(0xe50, g);
    m.pair(0xe90, g);
    let k = m.d(0xe60);
    m.pair(0xea0, 1.0 / (((g * k + g * k) + 1.0) + g * g));
    m.pair(0xe80, k + k);
    m.pair(0xf10, pi_rate);
    svf2(m);
    m.pair(0xfc0, tanpoly(f64(w * 50.0)));
    let r = 1.0 / srf;
    m.wf(0x1064, r);
    m.wf(0x1060, r * m.f(0x1068));
    m.wf(0x1074, r * 0.5);
    let ph = m.f(0x106c) * m.f(0x1070) + m.f(0x105c);
    let ph = if 1.0 < ph { ph - 1.0 } else if ph < 0.0 { ph + 1.0 } else { ph };
    m.wf(0x105c, ph);
    m.wf(0x106c, 0.0);
    m.wf(0x1070, 1.0);
    m.pair(0x10d0, tanpoly(f64(w * 5000.0)));
}

/// Per-lane helpers.
fn map4(a: [f32; 4], f: impl Fn(f32) -> f32) -> [f32; 4] {
    a.map(f)
}
fn zip4(a: [f32; 4], b: [f32; 4], f: impl Fn(f32, f32) -> f32) -> [f32; 4] {
    std::array::from_fn(|k| f(a[k], b[k]))
}
fn zip2(a: [f64; 2], b: [f64; 2], f: impl Fn(f64, f64) -> f64) -> [f64; 2] {
    [f(a[0], b[0]), f(a[1], b[1])]
}

impl Engine {
    pub(super) fn new(freak: Freak, rate: u32) -> Self {
        prepare_table();
        let mut m = Image::default();
        for (o, v) in CTOR {
            m.wi(o, v);
        }
        m.wi(0x104c, 12345);
        m.wi(0x1050, 12345);
        let mut d00 = 0.0;
        let write = |m: &mut Image, i: usize, v: f32| {
            m.wf(0x70 + 4 * i, v);
            m.wf(0x588 + 4 * i, v);
        };
        for (i, v) in DEFAULTS.into_iter().enumerate() {
            write(&mut m, i, v);
        }
        for i in 0..19 {
            set(&mut m, i, &mut d00);
        }
        // ponytail: the record's setParam calls in index order for the changed knobs; the
        // native apply (0x140aba860) runs field order, the final pass below overwrites both
        for (i, v) in freak.knobs.into_iter().enumerate() {
            if v != DEFAULTS[i] {
                write(&mut m, i, v);
                set(&mut m, i, &mut d00);
            }
        }
        for i in 0..19 {
            set(&mut m, i, &mut d00);
        }
        prepare(&mut m, rate);
        let mut image = Box::new(Mem::<SIZE, BASE>::default());
        image.0.copy_from_slice(&m.0[BASE..BASE + SIZE]);
        Self { image, d00 }
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        if !s.ready {
            s.m = *self.image;
            s.ready = true;
        }
        for i in 0..len {
            let (l, r) = self.sample(&mut s.m, block[0][i] as f32, block[1][i] as f32);
            block[0][i] = f64::from(l);
            block[1][i] = f64::from(r);
        }
    }

    #[inline(always)]
    fn sample(&self, m: &mut Mem<SIZE, BASE>, l: f32, r: f32) -> (f32, f32) {
        let (dl, dr) = (f64(l), f64(r));
        // 1. input envelope and gate
        let mx = l.abs().max(r.abs());
        let e = m.f(0x1014);
        let e = (mx - e) * (if mx <= e { m.f(0x1024) } else { m.f(0x1020) }) + e;
        m.wf(0x1014, e);
        let g = if e < 9.999_999_75e-6 { e * 100_000.0 } else { 1.0 };
        let gate = (g - 1.0) * m.f(0x958) + 1.0;
        let e00 = m.f(0xe00);
        let k95 = f64(e00 * m.f(0x950));
        let drv = f64((m.f(0xe04) - 1.0) * e00 + 1.0);
        let off = f64((e - 1.0) * m.f(0x1054) + 1.0) * k95;
        let kk = 1.0 - k95 * 0.5;
        let pre = [(drv * dl + off) * kk, (drv * dr + off) * kk];
        let mut sc0 = (0.0 - dl) * f64(m.f(0xd4c)) + dl;
        let mut sc1 = (0.0 - dr) * f64(m.f(0xd4c)) + dr;
        // 2. quadrature oscillator
        let o = f64(m.f(0xd18)) * 0.125;
        let wrap = |x: f64| if x >= 1.0 { x - 1.0 } else if x < 0.0 { x + 1.0 } else { x };
        let p0 = wrap(o + m.d(0xd20));
        let p1 = wrap(-o + m.d(0xd28));
        let inc = f64(m.f(0xd14));
        m.wd(0xd20, wrap(m.d(0xd20) + inc));
        m.wd(0xd28, wrap(m.d(0xd28) + inc));
        let a = [(0.5 + p0) as f32, (0.5 + p1) as f32, (0.25 + p0) as f32, (0.25 + p1) as f32];
        let mut osc = map4(a, |a| {
            let u = if a >= 1.0 { a - 1.0 } else if a < 0.0 { a + 1.0 } else { a };
            sine(1.0 - (2.0 - u * 4.0).abs())
        });
        // 3. side-chain follower
        let d38 = m.f(0xd38);
        let mut big_e = 0.0f32;
        if d38 > 0.0 {
            if m.f(0xd3c) > 0.0 {
                let (dc0, dd0, de0, df0, d90) = (m.d2(0xdc0), m.d2(0xdd0), m.d2(0xde0), m.d2(0xdf0), m.d2(0xd90));
                let hs = [(sc0 - df0[0]) * dc0[0], (sc1 - df0[1]) * dc0[1]];
                let v = [(hs[0] + de0[0]) * dd0[0], (hs[1] + de0[1]) * dd0[1]];
                let v2 = [v[0] + v[0], v[1] + v[1]];
                m.wd2(0xde0, zip2(v2, de0, |a, b| a - b));
                m.wd2(0xdf0, [v2[0] * dc0[0] + df0[0], v2[1] * dc0[1] + df0[1]]);
                let k3 = f64(m.f(0xd3c));
                sc0 += ((d90[0] + d90[0]) * v[0] - sc0) * k3;
                sc1 += ((d90[1] + d90[1]) * v[1] - sc1) * k3;
            }
            let mut v = [sc0 as f32, sc1 as f32, sc0 as f32, sc1 as f32];
            for i in 0..8 {
                let s = m.f4(0xb40 + 16 * i);
                let c = m.f4(0x9c0 + 16 * i);
                let h = zip4(zip4(v, s, |a, b| a - b), c, |a, b| a * b);
                m.wf4(0xb40 + 16 * i, zip4(h, v, |a, b| a + b));
                v = zip4(h, s, |a, b| a + b);
            }
            let amp = (((v[2] + v[3]) * (v[2] + v[3]) + (v[0] + v[1]) * (v[0] + v[1])) * 0.25).sqrt();
            let e = m.f(0xd54);
            big_e = (amp - e) * (if amp <= e { m.f(0xd64) } else { m.f(0xd60) }) + e;
            m.wf(0xd54, big_e);
            let d40 = m.f(0xd40);
            osc = zip4(osc, v, |o, v| o + (((big_e - v) * d40 + v) - o) * d38);
        }
        let f73 = (1.0 - e00) * m.f(0x950);
        let bias = ((big_e - 1.0) * d38 + 1.0) * f73;
        let sc = 1.0 - f73 * 0.5;
        let modv = map4(osc, |o| (bias + o) * sc);
        // 4. implicit stage
        let (cc0, ce0) = (m.d2(0xcc0), m.d2(0xce0));
        let ca = [1.0 - cc0[0], 1.0 - cc0[1]];
        let cb = [-ca[0] * ce0[0], -ca[1] * ce0[1]];
        let (g1, s1, g2, s2) = (m.d2(0x1110), m.d2(0x1130), m.d2(0x1140), m.d2(0x1160));
        let dv = [(1.0 - g1[0]) * s1[0], (1.0 - g1[1]) * s1[1]];
        let cc = [1.0 - g2[0], 1.0 - g2[1]];
        let cd = [-cc[0] * s2[0], -cc[1] * s2[1]];
        let fb = f64(m.f(0x1100));
        let k954 = m.f(0x954);
        let l92 = [1.0, 1.0, k954, k954];
        let w1170 = m.d2(0x1170);
        let mut y: [f64; 2] = std::array::from_fn(|c| pre[c] - fb * ((((g1[c] * w1170[c]) + dv[c]) * cc[c]) + cd[c]));
        let (c960, c970, k1180, k1184) = (m.f4(0x960), m.f4(0x970), m.f(0x1180), m.f(0x1184));
        let d00v = m.d2(0xd00);
        for _ in 0..4 {
            let mut yy = y;
            if self.d00 > 0.0 {
                yy = std::array::from_fn(|c| y[c] + (((ca[c] * y[c] + cb[c]) - y[c]) * d00v[c]));
            }
            let av: [f32; 4] = std::array::from_fn(|k| (yy[k & 1] as f32) * c960[k] + c970[k]);
            let tk: [f32; 4] = std::array::from_fn(|k| {
                let p = (modv[k] - av[k]) * k1180;
                let q = (av[k] + modv[k]) * k1180;
                let (r1, r2) = ((q * q + 1.0).sqrt(), (p * p + 1.0).sqrt());
                (((r1 - 1.0) - (r2 - 1.0)) * k1184) * l92[k]
            });
            let dd = [((f64(tk[2]) + f64(tk[0])) * g1[0] + dv[0]) * cc[0], ((f64(tk[3]) + f64(tk[1])) * g1[1] + dv[1]) * cc[1]];
            let yn = [pre[0] - (cd[0] + dd[0]) * fb, pre[1] - (cd[1] + dd[1]) * fb];
            let conv = (y[0] - yn[0]).abs() < 1e-4 && (y[1] - yn[1]).abs() < 1e-4;
            y = yn;
            if conv {
                break;
            }
        }
        let mut out4 = [y[0] as f32, y[1] as f32, y[0] as f32, y[1] as f32];
        if self.d00 > 0.0 {
            let cc8 = [(y[0] - ce0[0]) * cc0[0], (y[1] - ce0[1]) * cc0[1]];
            let cb8 = [cc8[0] + ce0[0], cc8[1] + ce0[1]];
            m.wd2(0xce0, [cb8[0] + cc8[0], cb8[1] + cc8[1]]);
            let yo = [y[0] + d00v[0] * ((y[0] - cb8[0]) - y[0]), y[1] + d00v[1] * ((y[1] - cb8[1]) - y[1])];
            out4 = [yo[0] as f32, yo[1] as f32, yo[0] as f32, yo[1] as f32];
        }
        let mut pre4 = [pre[0] as f32, pre[1] as f32, pre[0] as f32, pre[1] as f32];
        let mut acc = [0f32; 4];
        for i in 0..8 {
            let c = m.f4(0x9c0 + 16 * i);
            let s = m.f4(0xa40 + 16 * i);
            let h = zip4(zip4(out4, s, |a, b| a - b), c, |a, b| a * b);
            let sn = zip4(h, out4, |a, b| a + b);
            m.wf4(0xa40 + 16 * i, sn);
            out4 = zip4(s, h, |a, b| a + b);
            let s2 = m.f4(0xac0 + 16 * i);
            let h2 = zip4(zip4(pre4, s2, |a, b| a - b), c, |a, b| a * b);
            m.wf4(0xac0 + 16 * i, zip4(h2, pre4, |a, b| a + b));
            pre4 = zip4(s2, h2, |a, b| a + b);
            let fir = m.f4(0xc40 + 16 * i);
            acc = std::array::from_fn(|k| sn[k] * fir[k] + acc[k]);
        }
        m.wf4(0x970, acc);
        // 5. ADAA on the sum and difference branches
        let q = zip4(modv, out4, |a, b| k1180 * (a + b));
        let pq = zip4(modv, out4, |a, b| k1180 * (a - b));
        let thr = m.f4(0x1190);
        let adaa = |m: &mut Mem<SIZE, BASE>, x: [f32; 4], so: usize| -> [f32; 4] {
            let (xp, fp) = (m.f4(so), m.f4(so + 0x10));
            let fx = map4(x, adaa_f);
            let y = std::array::from_fn(|k| {
                let dx = x[k] - xp[k];
                if dx.abs() < ((x[k].abs() + xp[k].abs()) + 10.0) * thr[k] {
                    let hm = (x[k] + xp[k]) * 0.5;
                    (hm * hm + 1.0).sqrt() - 1.0
                } else {
                    (fx[k] - fp[k]) / dx
                }
            });
            m.wf4(so + 0x10, fx);
            m.wf4(so, x);
            y
        };
        let y1 = adaa(m, q, 0x11a0);
        let y2 = adaa(m, pq, 0x11c0);
        let c08 = zip4(y1, y2, |a, b| (a - b) * k1184);
        let lv = map4(c08, |v| v * k954);
        let mut wet = [f64(lv[2]) + f64(c08[0]), f64(lv[3]) + f64(c08[1])];
        m.wd2(0x1170, wet);
        let t298 = [(wet[0] - s1[0]) * g1[0], (wet[1] - s1[1]) * g1[1]];
        let v74 = [t298[0] + s1[0], t298[1] + s1[1]];
        m.wd2(0x1130, [v74[0] + t298[0], v74[1] + t298[1]]);
        let t288 = [(v74[0] - s2[0]) * g2[0], (v74[1] - s2[1]) * g2[1]];
        m.wd2(0x1160, [(t288[0] + s2[0]) + t288[0], (t288[1] + s2[1]) + t288[1]]);
        if e00 > 0.0 {
            wet = fuzz(m, wet, e00);
        }
        let f83 = (1.0 - e00) * m.f(0x94c);
        let gain = f64(((f83 * m.f(0x1104) + (1.0 - f83)) * gate) * m.f(0x948));
        let dry = f64(m.f(0x944));
        ((dry * dl + gain * wet[0]) as f32, (dry * dr + gain * wet[1]) as f32)
    }
}

/// MINSTD (48271, 2^31 - 1) uniform in `[+0x10c0, +0x10c4]` (0x141962370).
fn rand(m: &mut Mem<SIZE, BASE>, so: usize) -> f32 {
    let sd = ((u64::from(m.i(so)) * 48271) % 2_147_483_647) as u32;
    m.wi(so, sd);
    let (lo, hi) = (m.f(0x10c0), m.f(0x10c4));
    ((hi - lo) * (sd as f32 - 1.0)) * 4.656_612_87e-10 + lo
}

/// Mode 0 fuzz chain (0x141964b20 and the ring-mod std::function 0x141943a20).
fn fuzz(m: &mut Mem<SIZE, BASE>, d28: [f64; 2], e00: f32) -> [f64; 2] {
    let nz = [f64(rand(m, 0x104c)), f64(rand(m, 0x1050))];
    let (lp0, k10d0) = (m.d2(0x10f0), m.d2(0x10d0));
    let t = zip2(zip2(nz, lp0, |a, b| a - b), k10d0, |a, b| a * b);
    let lp = zip2(t, lp0, |a, b| a + b);
    m.wd2(0x10f0, zip2(lp, t, |a, b| a + b));
    let k1048 = f64(m.f(0x1048));
    let x = zip2(d28, lp, |a, b| a + k1048 * b);
    let (ec0, e90, eb0, ea0, e60) = (m.d2(0xec0), m.d2(0xe90), m.d2(0xeb0), m.d2(0xea0), m.d2(0xe60));
    let a: [f64; 2] = std::array::from_fn(|c| ((x[c] - ec0[c]) * e90[c] + eb0[c]) * ea0[c]);
    let v2 = [a[0] + a[0], a[1] + a[1]];
    let b: [f64; 2] = std::array::from_fn(|c| (e60[c] + e60[c]) * a[c]);
    m.wd2(0xeb0, zip2(v2, eb0, |a, b| a - b));
    m.wd2(0xec0, std::array::from_fn(|c| v2[c] * e90[c] + ec0[c]));
    let fx = b.map(|b| ((1f64.copysign(b) * b) * 0.5) * b);
    let (bp, fp) = (m.d2(0xff0), m.d2(0x1000));
    m.wd2(0xff0, b);
    m.wd2(0x1000, fx);
    let e10 = m.d2(0xe10);
    let y: [f64; 2] = std::array::from_fn(|c| {
        let dx = b[c] - bp[c];
        if dx.abs() < ((b[c].abs() + bp[c].abs()) + 0.2) * e10[c] { ((bp[c] + b[c]) * 0.5).abs() } else { (fx[c] - fp[c]) / dx }
    });
    let (fe0, fc0) = (m.d2(0xfe0), m.d2(0xfc0));
    let t7 = std::array::from_fn::<f64, 2, _>(|c| (y[c] - fe0[c]) * fc0[c]);
    let lp1 = zip2(t7, fe0, |a, b| a + b);
    m.wd2(0xfe0, zip2(t7, lp1, |a, b| a + b));
    let (f60, f30, f50, f40) = (m.d2(0xf60), m.d2(0xf30), m.d2(0xf50), m.d2(0xf40));
    let uu: [f64; 2] = std::array::from_fn(|c| {
        let u = (((y[c] - lp1[c]) - f60[c]) * f30[c] + f50[c]) * f40[c];
        u + u
    });
    m.wd2(0xf50, zip2(uu, f50, |a, b| a - b));
    let f60n: [f64; 2] = std::array::from_fn(|c| uu[c] * f30[c] + f60[c]);
    m.wd2(0xf60, f60n);
    let (fb0, f80, fa0, f90) = (m.d2(0xfb0), m.d2(0xf80), m.d2(0xfa0), m.d2(0xf90));
    let w: [f64; 2] = std::array::from_fn(|c| {
        let w = (((f60[c] + f60n[c]) * 0.5 - fb0[c]) * f80[c] + fa0[c]) * f90[c];
        w + w
    });
    m.wd2(0xfa0, zip2(w, fa0, |a, b| a - b));
    let fb0n: [f64; 2] = std::array::from_fn(|c| w[c] * f80[c] + fb0[c]);
    m.wd2(0xfb0, fb0n);
    let mut out = zip2(fb0, fb0n, |a, b| (a + b) * 0.5);
    let k1054 = m.f(0x1054);
    if k1054 > 0.0 {
        let a = m.f(0x1078) + m.f(0x105c);
        let a = if 1.0 <= a { a - 1.0 } else { a };
        let lfo = f64(sine(1.0 - (2.0 - a * 4.0).abs()));
        let v = m.f(0x106c);
        let st = if m.f(0x1074) < v { m.f(0x1074) } else { v };
        m.wf(0x106c, v - st);
        let ph = st * m.f(0x1070) + m.f(0x1060) + m.f(0x105c);
        let ph = if ph > 1.0 { ph - 1.0 } else if ph < 0.0 { ph + 1.0 } else { ph };
        m.wf(0x105c, ph);
        out = std::array::from_fn(|c| out[c] + (lfo * b[c] - out[c]) * f64(k1054));
    }
    let e30 = m.d2(0xe30);
    let out = zip2(out, e30, |a, b| a * b);
    let k = f64(m.f(0xe08)) * 2.0;
    let be8 = out.map(|v| k * v);
    let mx = (be8[0].abs().max(be8[1].abs())) as f32;
    let e = m.f(0x1030);
    let e = (mx - e) * (if mx <= e { m.f(0x1040) } else { m.f(0x103c) }) + e;
    m.wf(0x1030, e);
    let gn = f64(0.707_106_769 / (e + 0.400_000_006));
    std::array::from_fn(|c| d28[c] + f64(e00) * (be8[c] * gn - d28[c]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::BLOCK;

    /// fxemu vectors of the native core (kcolor/vec_freak.json, 48 kHz, left of a mono input
    /// after a 4800-frame silent pre-roll): a 0.5 impulse (128 frames) and a 0.5 sine at
    /// 200 Hz, frames 1920..2400.
    #[test]
    fn matches_the_native_core() {
        let v: serde_json::Value = serde_json::from_str(include_str!("tables/kontakt_freak_vectors.json")).unwrap();
        let f = |x: &serde_json::Value| x.as_f64().unwrap() as f32;
        for case in v["cases"].as_array().unwrap() {
            let mut knobs = DEFAULTS;
            for (k, x) in case["params"].as_object().unwrap() {
                knobs[k.parse::<usize>().unwrap()] = f(x);
            }
            let unit = Engine::new(Freak { knobs }, 48_000);
            let run = |input: &dyn Fn(usize) -> f32, frames: usize| {
                let mut s = State::default();
                let mut out = Vec::new();
                for start in (0..4800 + frames).step_by(BLOCK) {
                    let mut block: Planar = [[0.0; BLOCK]; 2];
                    for i in 0..BLOCK {
                        let n = start + i;
                        let x = if n < 4800 { 0.0 } else { f64::from(input(n - 4800)) };
                        block[0][i] = x;
                        block[1][i] = x;
                    }
                    unit.process(&mut s, &mut block, BLOCK);
                    out.extend(block[0].iter().map(|&v| v as f32));
                }
                out.split_off(4800)
            };
            let check = |name: &str, out: &[f32], native: &[f32]| {
                for (i, (a, b)) in out.iter().zip(native).enumerate() {
                    assert!((a - b).abs() <= 1e-6, "{:?} {name}[{i}]: {a} vs {b}", case["params"]);
                }
            };
            let imp: Vec<f32> = case["impulse_L"].as_array().unwrap().iter().map(f).collect();
            check("impulse", &run(&|n| if n == 0 { 0.5 } else { 0.0 }, 256)[..imp.len()], &imp);
            let sine: Vec<f32> = case["sine_L_settled_1920_2400"].as_array().unwrap().iter().map(f).collect();
            let rate = 48_000.0;
            let out = run(&|n| (0.5 * (2.0 * std::f64::consts::PI * 200.0 * n as f64 / rate).sin()) as f32, 2400);
            check("sine", &out[1920..2400], &sine);
        }
    }
}
