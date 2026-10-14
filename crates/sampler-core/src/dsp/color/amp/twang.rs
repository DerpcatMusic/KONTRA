//! Kontakt Twang (`BEffectTwang`, vtable 0x144f14b18, factory 0x1407c8bc0): init 0x1408de330
//! (tables 0x140addcf0, SR setup 0x140adf330, supply 0x140ade040), reset 0x140adedf0, apply
//! 0x140adf660, kernel 0x140ade440 (process 0x140ade200): per 4x-oversampled step a four-lane
//! tube stage with hum noise, the tone stack, a push-pull power stage with ripple sag, then
//! decimation. The same topology as the AC Box with its own tables and constants.
// ponytail: the four tables (static, 0x144f3d900..0x144f3f900) and the reset's settled operating
// point are dumped under kemu, not re-derived; the reset runs before the knobs are applied.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};
use super::crywah::rcp;
use super::dstortion::zip;

type V = [f32; 4];

const fn bits(v: [u32; 4]) -> V {
    [f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2]), f32::from_bits(v[3])]
}

const HI: f32 = 255.0;
const NOISE: V = bits([0x3716_feb5, 0x379f_6230, 0, 0]);
const K2C0: V = bits([0x4632_cccd, 0x4660_cccd, 0x454e_1eb8, 0x4574_f972]);
const K2D0: V = bits([0x4342_0c4a, 0x4356_872b, 0x4308_b439, 0x42de_353f]);
const K320: V = bits([0x3c75_c28f, 0x3c86_594b, 0, 0]);
const K380: f32 = f32::from_bits(0x45dc_56a1);
const K390: f32 = f32::from_bits(0x427c_ed91);
const K3A0: f32 = f32::from_bits(0xc8c6_b352);
const SAG: f32 = 485.0;
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
/// Table per tube lane in twang.f32le (0 is the power stage's).
const LANE_TABLE: [usize; 4] = [1, 1, 2, 3];

fn tables() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| include_bytes!("twang.f32le").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn shape(x: f32, t: &[f32]) -> f32 {
    let i = x.max(0.0).min(HI) as i32;
    let fr = x - i as f32;
    let i = i as usize * 2;
    fr * t[i + 1] + t[i]
}

fn ld(s: &[f32; 80], o: isize) -> V {
    let i = (o + 36) as usize;
    [s[i], s[i + 1], s[i + 2], s[i + 3]]
}

fn st(s: &mut [f32; 80], o: isize, v: V) {
    let i = (o + 36) as usize;
    s[i..i + 4].copy_from_slice(&v);
}

fn mul(a: V, b: V) -> V {
    zip(a, b, |a, b| a * b)
}

fn add(a: V, b: V) -> V {
    zip(a, b, |a, b| a + b)
}

fn sub(a: V, b: V) -> V {
    zip(a, b, |a, b| a - b)
}

/// `(x - old) * k + old`
fn toward(x: V, old: V, k: V) -> V {
    add(mul(sub(x, old), k), old)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct State {
    /// Per channel the native block around obj+0x4c0+0x140*ch (float offsets -36..+44).
    s: [[f32; 80]; 2],
    /// The hum noise shift register (+0x68).
    seed: [i32; 2],
}

impl Default for State {
    /// The reset's settled operating point (slot 5).
    fn default() -> Self {
        let mut s = [0.0; 80];
        for (i, b) in [
            (4, 0x3f1e_5c00),
            (5, 0x3f1e_5c00),
            (6, 0x3b8f_d2ff),
            (7, 0x3b8f_e2a4),
            (8, 0xb50f_5bf6),
            (9, 0x3b92_e615),
            (10, 0x3f1e_9362),
            (11, 0x3f1e_929d),
            (12, 0x3b29_21e5),
            (13, 0x3b29_21e5),
            (14, 0x3b1a_d471),
            (15, 0x3b1a_d471),
            (16, 0x43c1_bde0),
            (17, 0x43c1_bde0),
            (18, 0x43d3_a387),
            (19, 0x43d3_a387),
            (20, 0x3b15_5d24),
            (21, 0x3b15_5d24),
            (22, 0x3b15_5d24),
            (23, 0x3b15_5d24),
            (24, 0x43db_623f),
            (25, 0x43db_623f),
            (26, 0x43db_623f),
            (27, 0x43db_623f),
            (28, 0x4383_c711),
            (29, 0x4383_d1aa),
            (30, 0x436c_92d6),
            (31, 0x4360_a98a),
            (32, 0x3fed_e62a),
            (33, 0x4002_006c),
            (36, 0x3b80_0764),
            (37, 0x4383_c41f),
            (38, 0x3b91_7fbc),
            (39, 0x4383_c70d),
            (40, 0xbd12_9977),
            (41, 0x3cbb_ccab),
            (42, 0x4383_81fd),
            (43, 0x3b91_1489),
            (44, 0x436c_913a),
            (45, 0x4360_ae8a),
            (48, 0x3c72_3866),
            (52, 0x3e47_e12c),
            (53, 0x3f80_0000),
            (54, 0x43d3_a387),
            (55, 0x43c1_bde0),
            (56, 0x43da_280f),
            (60, 0x3f05_f4ec),
            (61, 0x43db_623f),
        ] {
            s[i] = f32::from_bits(b);
        }
        Self { s: [s; 2], seed: [1; 2] }
    }
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.s.iter().flatten().all(|v| v.is_finite())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    /// The ramp's settled lanes (0x1e0..0x1fc).
    p: [V; 2],
    c2b0: V,
    c310: V,
    c330: V,
    c340: V,
    c350: V,
    c360: V,
    c3c0: f32,
    c3d0: V,
    /// Mains-ripple phase step (0x3e0), supply charge (0x3e8) and draw (0x3ec).
    ripple: f32,
    charge: f32,
    draw: f32,
    mono: bool,
}

impl Engine {
    /// Record order (Fx4BB): volume, treble, mid, bass, bright, mono.
    pub(crate) fn new(k: [f32; 6], rate: u32) -> Self {
        prepare_table();
        let [vol, treble, mid, bass, bright, mono] = k;
        let cap = |c: f32, x: f32| if c <= x { c } else { x };
        // SR setup 0x140adf330 and 0x140ade040 at the 4x rate.
        let rate = rate as f32;
        let w = 6.283 / (rate * 4.0);
        let i4 = 0.25 / rate;
        let co = |i: u16| cap(1.0, w * table(f32::from(i) + 0.881_591_8));
        // Apply 0x140adf660.
        let v = vol;
        let c1d4 = (((v * 106.84 - 280.18) * v + 248.81) * v - 93.14) * v + 17.67;
        let mid = (table(mid * 348.802_43 + 851.197_57) - table(851.197_57)) * 0.0989;
        let bright = c1d4 * if bright != 0.0 { 0.5 } else { 0.0 };
        let treble = (table(treble * 119.589_41 + 1_080.410_6) - 0.233) * 0.7009;
        let b3 = (table(bass * 219.247_25 + 980.752_75) - 0.0794) * 1.0863;
        let gain = (table(v * 199.315_67 + 1_000.684_3) - 0.1) * 1.111_111_2;
        let pv = (((v * 2489.5 - 4734.5) * v + 3372.5) * v - 1_302.700_1) * v + 2_220.381_6;
        let c354 = cap(1.0, table(1_571.881_6) / (b3 + 0.0001) * w);
        let c35c = cap(1.0, w * table(pv));
        // The ramp adds its zero increments.
        let z = |v: V| v.map(|x| x + 0.0);
        Self {
            p: [z([0.0, mid, bright, treble]), z([f32::from_bits(0xbc20_902e), b3 * 0.5623, 0.0, gain])],
            c2b0: [co(1281), co(1451), co(1556), co(1566)],
            c310: [co(2046), co(2061), co(2011), co(2021)],
            c330: [co(1336), co(1336), 1.0, 1.0],
            c340: [co(1566), co(1451), co(1966), co(1836)],
            c350: [co(2001), c354, 0.0, c35c],
            c360: [co(1451), co(1451), 0.0, 0.0],
            c3c0: cap(1.0, table(1_856.881_6) * 0.000_284_943_32),
            c3d0: [co(1376), 0.0, co(1371), co(1236)],
            ripple: 100.0 * i4,
            charge: table(940.889_65) * 1000.0 * i4,
            draw: table(791.402_83) * -476.4 * i4,
            mono: mono != 0.0,
        }
    }

    fn sample(&self, s: &mut [f32; 80], seed: &mut i32, x: f32) -> f32 {
        let tab = tables();
        s[0] = x;
        let [r0, r1] = self.p;
        for k in (0..4).rev() {
            // Hum noise: a 31-bit shift register's second bit as +-1.
            let n = *seed;
            *seed = (((n >> 25) ^ n) >> 6 & 1) | n.wrapping_add(n);
            let noise = ((*seed & 2) - 1) as f32;
            // Tone stack, then the interpolated input.
            let x0 = mul(PHASE[k], ld(s, -36));
            let x3 = ld(s, -32);
            let x10 = ld(s, -8);
            let x11 = ld(s, 12);
            let x5 = sub([x11[0], x10[0], x3[2], x10[0]], ld(s, 0));
            let x7 = mul(r0, x5);
            st(s, 0, add(mul(x5, self.c340), ld(s, 0)));
            let old = ld(s, 4);
            let x5 = sub([x5[0], x5[1], x10[1], x3[3]], old);
            let x2 = add(mul(x5, self.c350), old);
            st(s, 4, x2);
            let x2 = mul(x2, r1);
            let x4 = ld(s, -4);
            let x0b = x2.map(|v| x7[3] + v);
            let x0b = add([x0b[1], x0b[1], x3[2], x3[2]], x7);
            let u = (x0[3] + x0[1]) + (x0[2] + x0[0]);
            let x5 = [x5[2], x5[2], x0b[1], x0b[2]];
            st(s, -32, x5);
            let x5 = sub(x5, x2);
            // Tube stage: Miller-divided grid, table shaper per lane.
            let x7 = add([u, x2[3], x5[0], x5[0]], NOISE.map(|c| noise * c));
            let old = ld(s, -28);
            let x7 = sub(x7, old);
            st(s, -28, add(mul(x7, self.c2b0), old));
            let x7 = add(mul(mul(sub(x7, x4), ld(s, -24)), K2C0), K2D0);
            let y: V = std::array::from_fn(|l| shape(x7[l], &tab[LANE_TABLE[l] * 512..]));
            let x3 = ld(s, -20);
            let x7 = mul(y, x3);
            let x3 = sub(x3, x7);
            let x2 = toward(x7, x10, self.c310);
            st(s, -8, x2);
            st(s, -4, toward(mul(x3, K320), x4, self.c330));
            let old = ld(s, 8);
            let x2 = sub([x2[2], x2[3], x2[2], x2[3]], old);
            st(s, 8, add(mul(x2, self.c360), old));
            // Push-pull power stage: lanes 0 and 1.
            let mut x8 = ld(s, 20);
            let lane = |l: usize| {
                let x = ((x8[0] * 0.14 + x2[l] * 0.0794) * K380 + K3A0) * s[36 - 16 + l] + K390;
                shape(x, tab) * s[36 - 12 + l]
            };
            let (a, b) = (lane(0), lane(1));
            let d = a - b;
            st(s, 12, toward([d; 4], x11, [self.c3c0, 0.0, 0.0, 0.0]));
            // Supply sag: recharged from the rectified ripple, drained by the stage.
            let mut ph = s[36 + 24] + self.ripple;
            if 1.0 <= ph {
                ph -= 2.0;
            }
            s[36 + 24] = ph;
            let s64 = s[36 + 25];
            let drawn = (b + a) * self.draw + s64;
            let q = SAG * (ph * ph) + s64;
            let sv = (SAG - q).max(0.0) * self.charge + drawn;
            s[36 + 25] = sv;
            let x1 = ld(s, 16);
            let x2 = sub(mul([sv, 1.0, x8[0], x1[2]], [1.0, 1.0, 0.97, 0.916]), add(mul(x1, [0.84, 1.0, 1.0, 1.0]), x8));
            let x2 = add(mul(x2, self.c3d0), x1);
            st(s, 16, x2);
            let x5 = [sv, sv, x2[2], x2[3]];
            let r = x5.map(rcp);
            x8[0] += x2[0] * self.c3d0[0];
            st(s, 20, x8);
            st(s, -20, [x5[3], x5[3], x5[2], x5[2]]);
            st(s, -24, [r[3], r[3], r[2], r[2]]);
            st(s, -12, [sv; 4]);
            st(s, -16, [r[0]; 4]);
            s[36 + 28 + k] = d * 0.001;
        }
        // 16-tap decimation over the oversampled outputs (offsets 28..43).
        let (x5, x4, x3) = (ld(s, 28), ld(s, 32), ld(s, 36));
        let x2 = add(add(add(mul(x4, DECIMATE[1]), mul(x5, DECIMATE[0])), mul(x3, DECIMATE[2])), mul(ld(s, 40), DECIMATE[3]));
        st(s, 40, x3);
        st(s, 36, x4);
        st(s, 32, x5);
        let i0 = ld(s, -36);
        st(s, -36, [i0[0], i0[0], i0[1], i0[2]]);
        (x2[3] + x2[1]) + (x2[2] + x2[0])
    }

    pub(crate) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        for i in 0..len {
            if self.mono {
                let x = 0.5 * block[1][i] as f32 + 0.5 * block[0][i] as f32;
                let y = f64::from(self.sample(&mut s.s[0], &mut s.seed[0], x));
                block[0][i] = y;
                block[1][i] = y;
            } else {
                for c in 0..2 {
                    block[c][i] = f64::from(self.sample(&mut s.s[c], &mut s.seed[c], block[c][i] as f32));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kemu vectors (kontra-w5/twang_native.py): factory, init, reset, apply, kernel.
    #[test]
    fn matches_the_native_twang() {
        let cases: [(u32, [f32; 6]); 5] = [
            (48_000, [0.5, 0.5, 0.5, 0.5, 0.0, 0.0]),
            (48_000, [0.8, 0.2, 0.1, 0.9, 1.0, 0.0]),
            (48_000, [0.3, 0.9, 1.0, 0.0, 0.0, 0.0]),
            (44_100, [0.6, 0.7, 0.4, 0.3, 1.0, 1.0]),
            (96_000, [0.5, 0.85, 0.6, 0.7, 0.0, 0.0]),
        ];
        super::super::tests::check(include_bytes!("kontakt_twang_vectors.f32le"), cases.len(), |n| {
            let (rate, k) = cases[n];
            let unit = Engine::new(k, rate);
            let mut s = State::default();
            Box::new(move |block: &mut Planar, len| unit.process(&mut s, block, len))
        });
    }
}
