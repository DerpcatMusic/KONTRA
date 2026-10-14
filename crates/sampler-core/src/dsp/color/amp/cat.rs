//! Kontakt Cat (`BEffectCat`, vtable 0x144fc0a38, ctor 0x140a85cd0): init 0x140a9ad30 (wave
//! table built by 0x140adda10, Catmull-Rom 4x phase matrices), SR setup 0x140abec70, apply
//! 0x140ab9100 with the distortion 0x140acf610, filter 0x140acf960, bass 0x140acf3b0, balls
//! 0x140acefb0, treble 0x140ad1740 and tone (inline, as 0x140ad1420) laws, kernel 0x140aa9ae0:
//! a staggered-lane one-pole/state-variable tone network around a 4x-oversampled table shaper.
// ponytail: the table and the phase/decimation matrices do not depend on rate or knobs; they
// are the native init's output (dumped under kemu), not re-derived here.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};
use super::dstortion::zip;

type V = [f32; 4];

const LO: f32 = -128.0;
const HI: f32 = 127.0;
/// The table object's level (+0xc60), the first ramp lane.
const LEVEL: f32 = f32::from_bits(0x427d_a357);
const PHASE: [V; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-0.070_312_5, 0.867_187_5, 0.226_562_5, -0.023_437_5],
    [-0.062_5, 0.562_5, 0.562_5, -0.062_5],
    [-0.023_437_5, 0.226_562_5, 0.867_187_5, -0.070_312_5],
];
const DECIMATE: [V; 4] = [
    [-0.005_859_375, -0.015_625, -0.017_578_125, 0.0],
    [0.056_640_625, 0.140_625, 0.216_796_875, 0.25],
    [0.216_796_875, 0.140_625, 0.056_640_625, -0.0],
    [-0.017_578_125, -0.015_625, -0.005_859_375, 0.0],
];

fn shaper() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| include_bytes!("cat.f32le").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn ld(s: &[f32; 36], o: isize) -> V {
    let i = (o + 16) as usize;
    [s[i], s[i + 1], s[i + 2], s[i + 3]]
}

fn st(s: &mut [f32; 36], o: isize, v: V) {
    let i = (o + 16) as usize;
    s[i..i + 4].copy_from_slice(&v);
}

fn mul(a: V, b: V) -> V {
    zip(a, b, |a, b| a * b)
}

fn add(a: V, b: V) -> V {
    zip(a, b, |a, b| a + b)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct State {
    /// Per channel the native block obj+0x350+0x90*ch (float offsets -16..+20 around +0x390).
    s: [[f32; 36]; 2],
}

impl Default for State {
    fn default() -> Self {
        Self { s: [[0.0; 36]; 2] }
    }
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.s.iter().flatten().all(|v| v.is_finite())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    p: V,
    a0: V,
    b0: V,
    c0: V,
    mono: bool,
}

impl Engine {
    /// Record order (Fx7B): volume, distortion, filter, bass, balls, treble, tone, mono.
    pub(crate) fn new(k: [f32; 8], rate: u32) -> Self {
        prepare_table();
        let [vol, dist, filt, bass, balls, treble, tone, mono] = k;
        let cap = |c: f32, x: f32| if c <= x { c } else { x };
        // SR setup 0x140abec70.
        let w = 6.283 / rate as f32;
        let c2a8 = cap(1.0, w * table(1_371.881_6));
        let c2ac = cap(1.0, w * table(1_381.881_6));
        // Apply 0x140ab9100: volume.
        let g = (table(vol * 398.631_35 + 801.368_65) - 0.01) * 0.456_413_12;
        // Distortion 0x140acf610.
        let d = dist.clamp(0.0, 1.0).sqrt();
        let f2 = (d * 1.08 - 0.43).max(0.0);
        let c2c4 = 2.0 - (f2 + f2);
        let c1d0 = (f2 * 0.293) * (f2 + 1.3075) + 0.7352;
        let c224 = ((table(d * 597.947 + 1200.0) - 1.0) * c2c4) * LEVEL;
        // Filter 0x140acf960.
        let c2b8 = cap(1.0, table((675.0 - filt * 300.0) + 1_381.881_6) * w);
        // Bass 0x140acf3b0.
        let p = 1.0 - bass;
        let c2a4 = cap(1.0, table(p * p * p * 599.115_72 + 1_356.881_6) * w);
        // Balls 0x140acefb0.
        let c1d8 = table(balls * 80.0 * 9.965_784 + 801.368_65) - 1.0;
        let f3 = (balls * 80.0 - 40.0).max(0.0);
        let c2bc = cap(1.0, table((43.0 - f3) * 5.0 + 1_381.881_6) * w);
        // Treble 0x140ad1740.
        let c2b0 = cap(1.203_904_7, table(treble * 300.0 + 1_731.881_6) * w);
        // Tone (apply tail, as 0x140ad1420).
        let c2b4 = cap(c1d0, table(((tone * 120.0 + 640.0) - d * 335.0) + 1_381.881_6) * w);
        Self {
            // The kernel's per-sample ramp adds its zero increments.
            p: [LEVEL, c224, g, c1d8 * g].map(|v| v + 0.0),
            a0: [0.0, c2a4, c2a8, c2ac],
            b0: [c2b0, c2b4, c2b8, c2bc],
            c0: [f32::from_bits(0x3eeb_8520), c2c4, 1.0, 1.0],
            mono: mono != 0.0,
        }
    }

    fn sample(&self, s: &mut [f32; 36], x: f32) -> f32 {
        let tab = shaper();
        s[16 - 12] = x + x;
        let x5 = ld(s, -12);
        let m20 = ld(s, -8);
        let x6 = zip(x5, m20, |a, b| a - b);
        st(s, -8, add(mul(self.a0, x6), m20));
        let s0 = ld(s, 0);
        let m10 = ld(s, -4);
        let x1 = add(mul(m10, self.c0), [s0[0], s0[1], 0.0, 0.0]);
        let x4 = add(mul(zip([x6[0], x6[1], x6[2], x6[2]], x1, |a, b| a - b), self.b0), m10);
        st(s, -4, x4);
        let x9 = add(mul(x4, self.b0), s0);
        st(s, 0, x9);
        let x6 = mul([x6[1], x4[1], x4[2], x4[3]], self.p);
        let m = ld(s, -16);
        let x8 = mul([0.1, 0.1, m[1], m[1]], [x5[0], x5[0], m[1], m[1]]);
        let x9 = add([x9[0], x9[0], x6[1], x6[3]], [x8[0], x8[0], x6[0], x6[2]]);
        let x5 = [x9[2], m[1], m[2], m[3]];
        let r: [V; 4] = std::array::from_fn(|i| mul(PHASE[i], x5));
        let a = [r[0][2] + r[0][0], r[0][3] + r[0][1], r[1][2] + r[1][0], r[1][3] + r[1][1]];
        let b = [r[2][2] + r[2][0], r[2][3] + r[2][1], r[3][2] + r[3][0], r[3][3] + r[3][1]];
        let v = add([a[1], a[3], b[1], b[3]], [a[0], a[2], b[0], b[2]]);
        let y: V = std::array::from_fn(|l| {
            let i = (v[l].max(LO).min(HI) - LO) as i32;
            let fr = v[l] - (i as f32 + LO);
            let i = i as usize * 2;
            fr * tab[i + 1] + tab[i]
        });
        st(s, 4, y);
        let x3 = ld(s, 8);
        let x2 = ld(s, 12);
        let x1 = add(add(add(mul(y, DECIMATE[0]), mul(DECIMATE[1], x3)), mul(DECIMATE[2], x2)), mul(DECIMATE[3], ld(s, 16)));
        st(s, 16, x2);
        st(s, 12, x3);
        st(s, 8, y);
        st(s, -16, [x5[0], x5[0], x5[1], x5[2]]);
        let h = (x1[3] + x1[1]) + (x1[2] + x1[0]);
        st(s, -12, [x9[0], x9[0], h, x8[2]]);
        x9[3]
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

    /// kemu vectors (kontra-w5/cat_native.py): ctor, init, apply, settled ramp, kernel.
    #[test]
    fn matches_the_native_cat() {
        let cases: [(u32, [f32; 8]); 5] = [
            (48_000, [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0]),
            (48_000, [0.8, 1.0, 0.2, 0.1, 0.9, 0.3, 0.7, 0.0]),
            (48_000, [0.3, 0.0, 0.9, 1.0, 0.0, 1.0, 0.0, 0.0]),
            (44_100, [0.6, 0.7, 0.4, 0.3, 0.6, 0.8, 0.2, 1.0]),
            (96_000, [0.5, 0.85, 0.6, 0.7, 0.2, 0.4, 1.0, 0.0]),
        ];
        super::super::tests::check(include_bytes!("kontakt_cat_vectors.f32le"), cases.len(), |n| {
            let (rate, k) = cases[n];
            let unit = Engine::new(k, rate);
            let mut s = State::default();
            Box::new(move |block: &mut Planar, len| unit.process(&mut s, block, len))
        });
    }
}
