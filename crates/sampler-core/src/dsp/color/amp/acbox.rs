//! Kontakt AC Box (`BEffectACBox`, vtable 0x144fc0708, ctor 0x140a854e0): init 0x140a99bd0
//! (five wave tables built by 0x140adda10, Catmull-Rom 4x phase rows), SR setup 0x140abdda0
//! (+ 0x140ad1e20), apply 0x140ab8970 with the normal volume 0x140ad0760, brilliant volume
//! 0x140acf560, treble 0x140ad14d0 and bass 0x140acf0d0 laws, ramp chunk 0x140acbe30, kernel
//! 0x140aa75f0: per 4x-oversampled step a four-lane tube stage (table shaper with Miller
//! division), the tone stack, a tremolo-modulated power stage with its sag, then decimation.
// ponytail: the five tables, their bounds and the table-derived constants do not depend on the
// rate or the knobs; they are the native init's output (dumped under kemu), not re-derived.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};
use super::crywah::rcp;
use super::dstortion::zip;

type V = [f32; 4];

const fn bits(v: [u32; 4]) -> V {
    [f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2]), f32::from_bits(v[3])]
}

/// Tube stage tables (lanes 0..3) and the power stage's (4): bounds and table-header constants.
const LO: V = [-134.0, -106.0, -125.0, -128.0];
const HI: V = [121.0, 149.0, 130.0, 127.0];
const LO5: f32 = -118.0;
const HI5: f32 = 137.0;
const K3F90: V = bits([0x4681_3333, 0x4609_1eb8, 0x4659_3333, 0x4654_6666]);
const K3FA0: V = bits([0x3ee6_d800, 0x3eae_3200, 0x3ee1_4800, 0]);
const K4010: V = bits([0x433a_6c82, 0x42a1_78f6, 0, 0]);
const K4020: f32 = f32::from_bits(0x3dd8_44d0);
const K4080: f32 = f32::from_bits(0x45d4_cccd);
const K4090: f32 = f32::from_bits(0x3c83_1000);
const K40A0: f32 = f32::from_bits(0xc742_2e15);
const OUT: f32 = f32::from_bits(0x39df_1ee9);
const SAG: f32 = 1050.0;
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

fn tables() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| include_bytes!("acbox.f32le").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// Index and fraction of `x` in a table spanning lo..=hi.
fn at(x: f32, lo: f32, hi: f32) -> (usize, f32) {
    let i = (x.max(lo).min(hi) - lo) as i32;
    (i as usize * 2, x - (i as f32 + lo))
}

fn ld(s: &[f32; 80], o: isize) -> V {
    let i = (o + 48) as usize;
    [s[i], s[i + 1], s[i + 2], s[i + 3]]
}

fn st(s: &mut [f32; 80], o: isize, v: V) {
    let i = (o + 48) as usize;
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
    /// Per channel the native block around obj+0x290+0x140*ch (float offsets -48..+32).
    s: [[f32; 80]; 2],
}

impl Default for State {
    /// The reset's operating point (slot 5).
    fn default() -> Self {
        let mut s = [0.0; 80];
        for (i, b) in [
            (28, 0x3ace_435f),
            (29, 0x3ae4_c0c1),
            (30, 0x3ac5_8c3f),
            (31, 0x3ac5_8c3f),
            (32, 0x441e_dd71),
            (33, 0x440f_3f0a),
            (34, 0x4425_dfae),
            (35, 0x4425_dfae),
            (36, 0x3ac1_34c5),
            (37, 0x3ac1_34c5),
            (38, 0x3ac1_34c5),
            (39, 0x3ac1_34c5),
            (40, 0x4429_99eb),
            (41, 0x4429_99eb),
            (42, 0x4429_99eb),
            (43, 0x4429_99eb),
            (60, 0x43ba_95c2),
            (68, 0x3f80_0000),
            (70, 0x441e_dd71),
            (71, 0x440f_3f0a),
            (72, 0x4425_dfae),
            (77, 0x4429_99eb),
            (79, 0x3f80_0000),
        ] {
            s[i] = f32::from_bits(b);
        }
        Self { s: [s; 2] }
    }
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.s.iter().flatten().all(|v| v.is_finite())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    /// The ramp's settled lanes (0x3ea0..0x3edc).
    p: [V; 4],
    c3f80: V,
    c4000: V,
    c4030: V,
    c4040: V,
    c4050: V,
    c4060: V,
    c4070: V,
    c40d0: V,
    /// Tremolo LFO step (0x3e94), mains-ripple phase step (0x40e0), supply charge (0x40e8) and
    /// draw (0x40ec).
    lfo: f32,
    ripple: f32,
    charge: f32,
    draw: f32,
    mono: bool,
}

impl Engine {
    /// Record order (Fx7B): normal volume, brilliant volume, bass, treble, tone cut, tremolo
    /// speed, tremolo depth, mono.
    pub(crate) fn new(k: [f32; 8], rate: u32) -> Self {
        prepare_table();
        let [nv, bv, bass, treble, cut, speed, depth, mono] = k;
        let cap = |c: f32, x: f32| if c <= x { c } else { x };
        // SR setup 0x140abdda0 at the 4x rate.
        let i4 = 1.0 / (rate as f32 * 4.0);
        let w = i4 * 6.283;
        let co = |i: u16| cap(1.0, w * table(f32::from(i) + 0.881_591_8));
        let f5 = -8.64_f32 * 5.0;
        let f4 = -8.64_f32 * 4.982_892;
        // Apply 0x140ab8970.
        let g = |v: f32| table(v * 298.973_5 + 901.026_5);
        let c3f38 = (g(nv) - 0.031_622_775) * 1.032_655_5;
        let c3f30 = (g(bv) - 0.031_622_775) * 1.836_061_4;
        let hb = bv - 0.5;
        let c3f40 = 0.530_955_26 - hb * hb * 2.123_821;
        let t3 = treble * treble * treble;
        let c3f34 = 0.016_410_399 - treble * 0.011_239_999;
        let c3f44 = t3 * 0.083_906_59;
        let c3f48 = bass * c3f34 * 4.4884;
        let c4054 = cap(1.0, table(t3 * 138.5 + 1_813.381_6) * w);
        let c4044 = cap(1.0, w * table((125.0 - bass * 85.0) + 1_381.881_6));
        let c4058 = cap(1.0, w * table((365.0 - bass * 120.0) + 1_381.881_6));
        let tc = (1.0 - cut) * 0.631;
        let tb = cut * 0.631;
        // The ramp adds its zero increments.
        let z = |v: V| v.map(|x| x + 0.0);
        Self {
            p: [z([tc, tc, tb, tb]), z([c3f30, c3f34, c3f38, 0.0]), z([c3f40, c3f44, c3f48, 0.0]), z([depth * 50.0, 0.0, 0.0, 0.0])],
            c3f80: [co(1281), co(1281), co(1306), co(1306)],
            c4000: [co(1956), co(2031), co(2036), co(2031)],
            c4030: [co(1286), co(1336), co(1751), co(1751)],
            c4040: [co(1741), c4044, co(1381), 0.0],
            c4050: [co(1991), c4054, c4058, 1.0],
            c4060: [co(1421), 0.0, co(2001), 0.0],
            c4070: [co(1516), co(1516), 0.0, 0.0],
            c40d0: [
                cap(1.0, w * table(f5 + 1_391.881_6)),
                co(1181),
                cap(1.0, w * table(f5 + 1_171.881_6)),
                cap(1.0, w * table(f5 + 1_101.881_6)),
            ],
            lfo: (speed * 5.0 * 4.0 + 8.0) * i4,
            ripple: 100.0 * i4,
            // 0x140ad1e20.
            charge: i4 * table(f4 + 861.163_33) * 1000.0,
            draw: table(f4 + 861.163_33) * -414.9 * i4,
            mono: mono != 0.0,
        }
    }

    fn sample(&self, s: &mut [f32; 80], x: f32) -> f32 {
        let tab = tables();
        s[0] = x;
        let [r0, r1, r2, r3] = self.p;
        for k in (0..4).rev() {
            // Tube stage: interpolate, Miller-divided grid, table shaper per lane.
            let x1 = mul(PHASE[k], ld(s, -48));
            let x5 = ld(s, 0);
            let u = (x1[3] + x1[1]) + (x1[2] + x1[0]);
            let m = ld(s, -28);
            let x3 = sub([u, m[1], m[2], m[3]], ld(s, -24));
            st(s, -24, add(mul(x3, self.c3f80), ld(s, -24)));
            let x6 = add(add(mul(mul(ld(s, -20), x3), K3F90), K3FA0), mul(K4010, x5));
            let y: V = std::array::from_fn(|l| {
                let (i, fr) = at(x6[l], LO[l], HI[l]);
                let t = &tab[l * 512..];
                fr * t[i + 1] + t[i]
            });
            let old = ld(s, -4);
            let x4 = toward(mul(ld(s, -16), y), old, self.c4000);
            st(s, -4, x4);
            let x6 = toward([y[0], y[1], x4[2], x4[3]], x5, self.c4030);
            st(s, 0, x6);
            // Tone stack.
            let x2 = ld(s, 8);
            let old = ld(s, 4);
            let x3 = sub([x4[0], x4[1], x2[3], x2[3]], old);
            let x6 = mul([x4[2], x4[3], x6[2], x6[3]], r0);
            st(s, 4, add(mul(x3, self.c4040), old));
            let x1 = sub([x3[0], x3[1], x3[1], x4[0]], x2);
            let x3 = mul(x3, r1);
            let x0 = add(mul(self.c4050, x1), x2);
            st(s, 8, x0);
            let x1 = add(mul([x1[0], x1[1], x0[2], x0[2]], r2), x3);
            let x3 = add([x6[2], x6[3], x6[2], x6[3]], x6);
            let x0 = x1.map(|v| x1[2] + v);
            let old = ld(s, 16);
            let x3 = sub(x3, old);
            st(s, -28, [x1[0], x1[0], x0[1], x0[1]]);
            st(s, 16, add(mul(x3, self.c4070), old));
            let x8 = ld(s, 12);
            let fb = x8[0] * K4020;
            let x3 = x3.map(|v| v - fb);
            // Tremolo LFO: a parabola per half period with a flipping sign.
            let mut ph = self.lfo + s[48 + 30];
            s[48 + 30] = ph;
            if ph > 1.0 {
                s[48 + 31] = -s[48 + 31];
                ph -= 2.0;
                s[48 + 30] = ph;
            }
            let mut x7 = ld(s, 24);
            let v = (1.0 - ph * ph) * s[48 + 31] * r3[0];
            // Power stage: lanes 0 and 1 only (2 and 3 never reach the output).
            let lane = |l: usize, e: f32| {
                let x = ((x7[0] * 0.056 + (e + x3[l])) * K4080 + K40A0) * s[48 - 12 + l] + K4090;
                let (i, fr) = at(x, LO5, HI5);
                let t = &tab[4 * 512..];
                (fr * t[i + 1] + t[i]) * s[48 - 8 + l]
            };
            let (a, b) = (lane(0, v), lane(1, v));
            let x4 = toward([b + a, b + a, a - b, a - b], x8, self.c4060);
            st(s, 12, x4);
            // Supply sag: recharged from the rectified ripple, drained by the stage.
            let mut ph = s[48 + 28] + self.ripple;
            if 1.0 <= ph {
                ph -= 2.0;
            }
            s[48 + 28] = ph;
            let s74 = s[48 + 29];
            let drawn = (b + a) * self.draw + s74;
            let q = SAG * (ph * ph) + s74;
            let sv = (SAG - q).max(0.0) * self.charge + drawn;
            s[48 + 29] = sv;
            let x2 = ld(s, 20);
            let x3 = sub(mul([sv, x4[2], x7[0], x2[2]], [1.0, 1.0, 0.96, 0.91]), add(mul(x2, [0.84, 1.0, 1.0, 1.0]), x7));
            let x1 = add(mul(self.c40d0, x3), x2);
            st(s, 20, x1);
            x7[0] += x1[0] * self.c40d0[0];
            st(s, 24, x7);
            let x6 = [sv, x7[0], x1[2], x1[3]];
            let x2 = x6.map(rcp);
            st(s, -16, [x6[2], x6[3], x6[1], x6[1]]);
            st(s, -20, [x2[2], x2[3], x2[1], x2[1]]);
            st(s, -8, [x6[0]; 4]);
            st(s, -12, [x2[0]; 4]);
            s[4 + k] = x3[1] * OUT;
        }
        // 16-tap decimation over the oversampled outputs (offsets -44..-29).
        let (x5, x4, x3) = (ld(s, -44), ld(s, -40), ld(s, -36));
        let x2 = add(add(add(mul(DECIMATE[0], x5), mul(DECIMATE[1], x4)), mul(x3, DECIMATE[2])), mul(ld(s, -32), DECIMATE[3]));
        st(s, -32, x3);
        st(s, -36, x4);
        st(s, -40, x5);
        let i0 = ld(s, -48);
        st(s, -48, [i0[0], i0[0], i0[1], i0[2]]);
        (x2[2] + x2[0]) + (x2[3] + x2[1])
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

    /// kemu vectors (kontra-w5/acbox_native.py): ctor, init, apply, settled ramp, kernel.
    #[test]
    fn matches_the_native_ac_box() {
        let cases: [(u32, [f32; 8]); 5] = [
            (48_000, [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0]),
            (48_000, [0.8, 0.2, 0.1, 0.9, 0.3, 0.7, 0.0, 0.0]),
            (48_000, [0.3, 0.9, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]),
            (44_100, [0.6, 0.7, 0.4, 0.3, 0.6, 0.8, 0.2, 1.0]),
            (96_000, [0.5, 0.85, 0.6, 0.7, 0.2, 0.4, 1.0, 0.0]),
        ];
        super::super::tests::check(include_bytes!("kontakt_acbox_vectors.f32le"), cases.len(), |n| {
            let (rate, k) = cases[n];
            let unit = Engine::new(k, rate);
            let mut s = State::default();
            Box::new(move |block: &mut Planar, len| unit.process(&mut s, block, len))
        });
    }
}
