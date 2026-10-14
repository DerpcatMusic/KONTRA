//! Kontakt DStortion (`BEffectDStortion`, vtable 0x144fc0b70, ctor 0x140a85ea0): init 0x140a9b040
//! (DStortionWaveTable built by 0x140adda10, Catmull-Rom 4x phase matrices from 0x140a98da0),
//! SR setup 0x140abf080, apply 0x140ab96f0 (+ bass 0x140acf440), kernel 0x140aaa6c0: a
//! staggered-lane one-pole tone network around a 4x-oversampled table shaper (interpolate
//! by the phase matrices, shape, 16-tap decimate).
// ponytail: the table, its bounds and the phase matrices do not depend on the rate or the
// knobs; they are the native init's output (dumped under kemu), not re-derived here.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};

type V = [f32; 4];

const LO: f32 = -127.0;
const HI: f32 = 128.0;
const BIAS: f32 = f32::from_bits(0xbd9a_0400);
const PHASE: [V; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-0.070_312_5, 0.867_187_5, 0.226_562_5, -0.023_437_5],
    [-0.062_5, 0.562_5, 0.562_5, -0.062_5],
    [-0.023_437_5, 0.226_562_5, 0.867_187_5, -0.070_312_5],
];
const DECIMATE: [V; 4] = [
    [-0.005_859_375, -0.015_625, -0.017_578_125, 0.0],
    [0.056_640_625, 0.140_625, 0.216_796_875, 0.25],
    [0.216_796_875, 0.140_625, 0.056_640_625, 0.0],
    [-0.017_578_125, -0.015_625, -0.005_859_375, 0.0],
];

fn shaper() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| include_bytes!("dstortion.f32le").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn ld(s: &[f32; 32], o: isize) -> V {
    let i = (o + 24) as usize;
    [s[i], s[i + 1], s[i + 2], s[i + 3]]
}

fn st(s: &mut [f32; 32], o: isize, v: V) {
    let i = (o + 24) as usize;
    s[i..i + 4].copy_from_slice(&v);
}

fn zip(a: V, b: V, f: impl Fn(f32, f32) -> f32) -> V {
    std::array::from_fn(|l| f(a[l], b[l]))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct State {
    /// Per channel the native block obj+0x1f0+0x80*ch (offsets -0x60..+0x20 around +0x250).
    s: [[f32; 32]; 2],
}

impl Default for State {
    fn default() -> Self {
        Self { s: [[0.0; 32]; 2] }
    }
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.s.iter().flatten().all(|v| v.is_finite())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    pa: V,
    pb: V,
    lp: V,
    lp2: V,
    mono: bool,
}

impl Engine {
    /// Record order (reader 0x140d00a50, Fx6B): volume, tone, drive, bass, mid, treble, mono.
    pub(crate) fn new(k: [f32; 7], rate: u32) -> Self {
        prepare_table();
        let [vol, tone, drive, bass, mid, treble, mono] = k;
        let one = |x: f32| x.min(1.0);
        // SR setup 0x140abf080.
        let f9 = 6.283 / rate as f32;
        let c12c4 = one(f9 * table(1_481.881_6));
        let c12cc = one(f9 * table(1_381.881_6));
        let c12d8 = one(f9 * table(1_696.881_6));
        let f7 = table(1_170.102_7);
        let p2 = f7 - 1.0;
        let c1cc = if 1.5 <= f7 + f7 { 1.5 } else { f7 + f7 };
        let c1c8 = (f7 + 1.0) * 0.5 * f9;
        let f7 = table(1_030.581_7);
        let f5 = table(1_766.881_6);
        let c1d4 = f7 - 1.0;
        let cap = if f7 < 1.0 { 1.0 } else { 1.5 };
        let f5 = (f9 / ((f7 + 1.0) * 0.5)) * f5;
        let c12dc = if cap <= f5 { cap } else { f5 };
        // Apply 0x140ab96f0 and the bass setter 0x140acf440.
        let f = vol * 0.398;
        let p6 = (1.0 - tone) * f;
        let p4 = f * tone * 0.881;
        let p7 = p4 * c1d4;
        let p0 = table(drive * 262.100_13 * drive + 1_760.077) * 4.0;
        let c12c0 = one(table((570.0 - bass * 400.0) + 1_381.881_6) * f9);
        let c1d0 = table(mid * 400.0 + 1_581.881_6);
        let m = c1d0 * c1c8;
        let c12c8 = if c1cc <= m { c1cc } else { m };
        let c12d0 = one(table(treble * 200.0 + 1_881.881_6) * f9);
        // The ramp adds its zero increments once.
        Self {
            pa: [p0, 0.0, p2, 0.0].map(|v| v + 0.0),
            pb: [p4, 0.0, p6, p7].map(|v| v + 0.0),
            lp: [c12c0, c12c4, c12c8, c12cc],
            lp2: [c12d0, c12d0, c12d8, c12dc],
            mono: mono != 0.0,
        }
    }

    fn sample(&self, s: &mut [f32; 32], x: f32) -> f32 {
        let tab = shaper();
        s[24 - 2] = x + x;
        let mut x7 = ld(s, -4);
        let mut x10 = zip(x7, ld(s, 0), |a, b| a - b);
        st(s, 0, zip(zip(self.lp, x10, |a, b| a * b), ld(s, 0), |a, b| a + b));
        let mut x8 = zip(x10, self.pa, |a, b| a * b);
        let mut x6 = ld(s, -24);
        x6[0] = x8[0];
        st(s, -24, x6);
        let m: [V; 4] = std::array::from_fn(|r| zip(PHASE[r], x6, |a, b| a * b));
        let x2 = [m[0][2] + m[0][0], m[0][3] + m[0][1], m[1][2] + m[1][0], m[1][3] + m[1][1]];
        let x0 = [m[2][2] + m[2][0], m[2][3] + m[2][1], m[3][2] + m[3][0], m[3][3] + m[3][1]];
        let x5 = zip([x2[1], x2[3], x0[1], x0[3]], [x2[0], x2[2], x0[0], x0[2]], |a, b| a + b);
        let x5 = x5.map(|v| v + BIAS);
        let y: V = std::array::from_fn(|l| {
            let i = (x5[l].max(LO).min(HI) - LO) as i32;
            let fr = x5[l] - (i as f32 + LO);
            let i = i as usize * 2;
            fr * tab[i + 1] + tab[i]
        });
        st(s, -20, y);
        let x3 = ld(s, -16);
        let x2 = ld(s, -12);
        x6 = [x6[0], x6[0], x6[1], x6[2]];
        let mut x1 = zip(y, DECIMATE[0], |a, b| a * b);
        x7 = zip(x7, x8, |a, b| a + b);
        let x0 = zip(x3, DECIMATE[1], |a, b| a * b);
        x8 = zip(x8, x8, |a, b| a * b);
        x10 = [x10[1], x10[1], x7[2], x7[2]];
        x1 = zip(x1, x0, |a, b| a + b);
        x1 = zip(x1, zip(x2, DECIMATE[2], |a, b| a * b), |a, b| a + b);
        let x0 = zip(DECIMATE[3], ld(s, -8), |a, b| a * b);
        st(s, -8, x2);
        st(s, -12, x3);
        st(s, -16, y);
        x1 = zip(x1, x0, |a, b| a + b);
        st(s, -24, x6);
        let h = (x1[3] + x1[1]) + (x1[2] + x1[0]);
        let x4 = self.pb.map(|p| h * p);
        x10 = [x10[1], x10[2], x4[2], x4[3]];
        let old = ld(s, 4);
        x10 = zip(zip(zip(x10, old, |a, b| a - b), self.lp2, |a, b| a * b), old, |a, b| a + b);
        st(s, 4, x10);
        st(s, -4, [x10[0], x10[1], x8[0], x8[0]]);
        (x10[2] + x4[0]) + x10[3]
    }

    pub(crate) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        for i in 0..len {
            if self.mono {
                let x = 0.5 * block[1][i] as f32 + 0.5 * block[0][i] as f32;
                let y = f64::from(self.sample(&mut s.s[0], x));
                block[0][i] = y;
                block[1][i] = y;
            } else {
                for c in 0..2 {
                    block[c][i] = f64::from(self.sample(&mut s.s[c], block[c][i] as f32));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kemu vectors (kontra-w5/dst_native.py): ctor, init, apply, wrapper ramp, kernel.
    #[test]
    fn matches_the_native_dstortion() {
        let cases: [(u32, [f32; 7]); 5] = [
            (48_000, [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0]),
            (48_000, [0.8, 0.2, 1.0, 0.1, 0.9, 0.3, 0.0]),
            (48_000, [0.3, 0.9, 0.0, 1.0, 0.0, 1.0, 0.0]),
            (44_100, [0.6, 0.4, 0.7, 0.3, 0.6, 0.8, 1.0]),
            (96_000, [0.5, 0.6, 0.85, 0.7, 0.2, 0.4, 0.0]),
        ];
        super::super::tests::check(include_bytes!("kontakt_dstortion_vectors.f32le"), cases.len(), |n| {
            let (rate, k) = cases[n];
            let unit = Engine::new(k, rate);
            let mut s = State::default();
            Box::new(move |block: &mut Planar, len| unit.process(&mut s, block, len))
        });
    }
}
