//! Kontakt Fuzz (`BEffectFuzz`, vtable 0x144fc1fe0, ctor 0x140a861a0): init 0x140a9b280, SR
//! setup 0x140abf3b0, apply 0x140abaa80 with the level 0x140ad1980, bass 0x140acf4d0 and treble
//! 0x140ad17c0 laws, kernel 0x140aaad40: a 4x-oversampled table shaper whose input is the
//! band-passed (two one-poles) input fed back through a 4-tap interpolator, dry/wet mixed.
// ponytail: the table and the phase/decimation matrices do not depend on rate or knobs; they
// are the native init's output (dumped under kemu), not re-derived here.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};
use super::dstortion::zip;

type V = [f32; 4];

const LO: f32 = -125.0;
const HI: f32 = 130.0;
const BIAS: f32 = 0.440_002_44;
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
    T.get_or_init(|| include_bytes!("fuzz.f32le").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn mul(a: V, b: V) -> V {
    zip(a, b, |a, b| a * b)
}

fn add(a: V, b: V) -> V {
    zip(a, b, |a, b| a + b)
}

/// Per channel: the interpolator history (+0x320), the gain stage (+0x330), the one-poles
/// (+0x340) and the decimator history (+0x360..); the second channel's blocks sit at +0x390.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Lane {
    s: V,
    a: V,
    b: V,
    h: [V; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct State {
    c: [Lane; 2],
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.c.iter().all(|l| [l.s, l.a, l.b, l.h[0], l.h[1], l.h[2]].iter().flatten().all(|v| v.is_finite()))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    /// Ramp lanes +0x200: level, gain, mix.
    r: V,
    /// One-pole coefficients +0x290.
    k: V,
    mono: bool,
}

impl Engine {
    /// Record order (FFFB): amount, bass, treble, mono.
    pub(crate) fn new(k: [f32; 4], rate: u32) -> Self {
        prepare_table();
        let [amount, bass, treble, mono] = k;
        let cap = |c: f32, x: f32| if c <= x { c } else { x };
        // SR setup 0x140abf3b0.
        let w = 6.283 / rate as f32;
        // Level 0x140ad1980 (called with 1.0).
        let level = (table(1.0 * 338.836_67 + 861.163_33) - 0.019_952_623) * 0.032_261_837;
        // Apply 0x140abaa80: the drive gain, (float)+0x1c8 = 256.
        let gain = (256.0 * 0.0472) * table(amount * 249.144_61 + 1_847.775_9);
        Self {
            r: [level, gain, 1.0, 0.0],
            k: [
                cap(1.0, w * table(1_356.881_6)),
                // Bass 0x140acf4d0, treble 0x140ad17c0.
                cap(1.0, table((520.0 - bass * 570.0) + 1_381.881_6) * w),
                cap(1.0, table(treble * 720.0 * treble + 1_381.881_6) * w),
                cap(1.0, w * table(1_381.881_6)),
            ],
            mono: mono != 0.0,
        }
    }

    fn sample(&self, l: &mut Lane, x: f32) -> f32 {
        let tab = shaper();
        l.s[0] = l.a[1];
        let u: V = std::array::from_fn(|k| {
            let p = mul(l.s, PHASE[k]);
            (p[1] + p[3]) + (p[0] + p[2]) + BIAS
        });
        let y: V = std::array::from_fn(|n| {
            let i = (u[n].max(LO).min(HI) - LO) as i32;
            let fr = u[n] - (i as f32 + LO);
            let i = i as usize * 2;
            fr * tab[i + 1] + tab[i]
        });
        let z = add(add(add(mul(y, DECIMATE[0]), mul(l.h[0], DECIMATE[1])), mul(l.h[1], DECIMATE[2])), mul(DECIMATE[3], l.h[2]));
        l.h = [y, l.h[0], l.h[1]];
        let hz = (z[1] + z[3]) + (z[0] + z[2]);
        l.s = [l.s[0], l.s[0], l.s[1], l.s[2]];
        let e = zip([hz, l.b[2], x * 2.0, l.a[3]], l.b, |a, b| a - b);
        l.b = add(mul(e, self.k), l.b);
        let a1 = l.a[1];
        l.a = mul([e[0], e[1], a1, a1], [self.r[0], self.r[1], a1, a1]);
        (l.a[0] - x) * self.r[2] + x
    }

    pub(crate) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        for i in 0..len {
            if self.mono {
                let x = (block[1][i] as f32 + block[0][i] as f32) * 0.5;
                let y = f64::from(self.sample(&mut s.c[0], x));
                block[0][i] = y;
                block[1][i] = y;
            } else {
                for c in 0..2 {
                    block[c][i] = f64::from(self.sample(&mut s.c[c], block[c][i] as f32));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kemu vectors (kontra-w5/fuzz_native.py): ctor, init, apply, settled ramp, kernel.
    #[test]
    fn matches_the_native_fuzz() {
        let cases: [(u32, [f32; 4]); 5] = [
            (48_000, [0.5, 0.5, 0.5, 0.0]),
            (48_000, [0.9, 0.2, 0.8, 0.0]),
            (48_000, [0.1, 1.0, 0.0, 0.0]),
            (44_100, [0.7, 0.3, 0.6, 1.0]),
            (96_000, [1.0, 0.0, 1.0, 0.0]),
        ];
        super::super::tests::check(include_bytes!("kontakt_fuzz_vectors.f32le"), cases.len(), |n| {
            let (rate, k) = cases[n];
            let unit = Engine::new(k, rate);
            let mut s = State::default();
            Box::new(move |block: &mut Planar, len| unit.process(&mut s, block, len))
        });
    }
}
