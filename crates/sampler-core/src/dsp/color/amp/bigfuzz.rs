//! Kontakt Big Fuzz (`BEffectBigFuzz`, vtable 0x144fc1878, ctor 0x140a85c00): init
//! 0x140a9ac30, SR setup 0x140abe7f0 (rate-dependent clip bounds via logf), apply 0x140ab9f40
//! with the bass 0x140acf2b0 and treble 0x140ad1650 laws (both re-derive the tone blend),
//! kernel 0x140aa8fd0: a 2-lane table clipper inside a 4x-oversampled one-pole network with a
//! tone blend of its two filtered outputs.
// ponytail: the table and the phase/decimation matrices do not depend on rate or knobs; they
// are the native ctor/init output (dumped under kemu), not re-derived here.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};
use super::dstortion::zip;

type V = [f32; 4];

/// The ctor's clipper gain (+0x2b0), bias (+0x2c0) and feedback (+0x2e0) lanes.
const GAIN: V = [425.978_607, 420.222_137, 0.0, 0.0];
const BIAS: V = [0.5; 4];
const FEEDBACK: [f32; 2] = [-11.220_179_6, -17.782_789_2];
/// Rows +0x310..+0x340 (row k is phase k).
const PHASE: [V; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-0.140_625, 0.867_187_5, 0.226_562_5, -0.023_437_5],
    [-0.125, 0.562_5, 0.562_5, -0.062_5],
    [-0.046_875, 0.226_562_5, 0.867_187_5, -0.070_312_5],
];
/// Rows +0x350..+0x380.
const DECIMATE: [V; 4] = [
    [-0.005_859_375, -0.015_625, -0.017_578_125, 0.0],
    [0.056_640_625, 0.140_625, 0.216_796_875, 0.25],
    [0.216_796_875, 0.140_625, 0.056_640_625, 0.0],
    [-0.017_578_125, -0.015_625, -0.005_859_375, 0.0],
];

fn shaper() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| include_bytes!("bigfuzz.f32le").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
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

/// Per channel the blocks at +0x390 (interpolator), +0x3a0..+0x3d0 (network) and +0x3e0..
/// (decimator); the second channel's sit at +0x420.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Lane {
    s: V,
    a: V,
    b: V,
    c: V,
    d: V,
    h: [V; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct State {
    l: [Lane; 2],
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.l.iter().all(|l| [l.s, l.a, l.b, l.c, l.d, l.h[0], l.h[1], l.h[2]].iter().flatten().all(|v| v.is_finite()))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    /// Ramp lanes +0x200: blend, blend, volume, blend.
    r: V,
    lo: V,
    hi: V,
    k2d0: V,
    k2f0: V,
    k300: V,
    mono: bool,
}

impl Engine {
    /// Record order (Fx4B): sustain, tone, bass, treble, mono.
    pub(crate) fn new(k: [f32; 5], rate: u32) -> Self {
        prepare_table();
        let [sustain, tone, bass, treble, mono] = k;
        let cap = |x: f32| if 1.0 <= x { 1.0 } else { x };
        // SR setup 0x140abe7f0.
        let inv = 1.0 / (rate as f32 * 4.0);
        let w = inv * 6.283;
        let co = |i: f32| cap(w * table(i + 0.881_591_8));
        let clip = |a: f32, b: f32| {
            let c = cap((rate as f32 * a - b).max(1.0).ln() * 0.130_719_066) * 127.5;
            (0.5 - c, c + 0.5)
        };
        let (lo0, hi0) = clip(0.017_781_600_4, 100.480_003);
        let (lo1, hi1) = clip(0.017_759_999_3, 182.0);
        // Apply 0x140ab9f40: sustain; the tone blend as the bass/treble setters leave it (+0x1f8 = 0.4).
        let vol = (table(sustain * 189.349_899 + 1_010.650_15) - 0.1) * -8.625;
        let u = (1.0 - tone) * -0.4;
        Self {
            r: [u * 3.3113, u * 0.389, vol, (tone * -0.4) * 1.9724],
            lo: [lo0, lo1, 0.0, 0.0],
            hi: [hi0, hi1, 0.0, 0.0],
            k2d0: [inv * 500.0, inv * 600.0, 0.0, 0.0],
            k2f0: {
                let b = cap(table((430.0 - bass * 430.0) + 1_381.881_6) * w);
                [b, b, co(1_526.0), co(1_911.0)]
            },
            k300: [co(1_851.0), co(1_821.0), cap(table(treble * 500.0 + 1_571.881_6) * w), co(1_736.0)],
            mono: mono != 0.0,
        }
    }

    fn sample(&self, l: &mut Lane, x: f32) -> f32 {
        let tab = shaper();
        l.s[0] = x * 2.0;
        let r = self.r;
        let mut o = [0.0f32; 4];
        for k in (0..4).rev() {
            let p = mul(l.s, PHASE[k]);
            l.a[2] = ((p[1] + p[0]) + p[2]) + p[3];
            let x5 = add(mul(sub(l.c, l.d), GAIN), BIAS);
            let i: [i32; 4] = std::array::from_fn(|n| (x5[n].max(self.lo[n]).min(self.hi[n]) + 127.0) as i32);
            let y: V = std::array::from_fn(|n| {
                let fr = x5[n] - (i[n] as f32 - 127.0);
                let j = i[n % 2] as usize * 2;
                fr * tab[j + 1] + tab[j]
            });
            l.d = add(mul(y, self.k2d0), l.d);
            let x3 = sub(l.a, l.b);
            let x2 = add(x3, y);
            l.b = add(mul(x3, self.k2f0), l.b);
            let x2 = mul([x2[0], x2[1], x3[2], l.c[1]], [FEEDBACK[0], FEEDBACK[1], r[2], r[3]]);
            let c = add(mul(sub(x2, l.c), self.k300), l.c);
            l.c = c;
            o[k] = (x3[3] * r[0] + c[3]) + l.a[3] * r[1];
            l.a = [c[2], c[0], c[1], c[1]];
        }
        let [d0, d1, d2, d3] = DECIMATE;
        let z = add(add(add(mul(o, d0), mul(l.h[0], d1)), mul(l.h[1], d2)), mul(d3, l.h[2]));
        l.h = [o, l.h[0], l.h[1]];
        l.s = [l.s[0], l.s[0], l.s[1], l.s[2]];
        let hz = (z[1] + z[3]) + (z[0] + z[2]);
        // The ramp's mix lane (+0x210) stays 1.
        (hz - x) * 1.0 + x
    }

    pub(crate) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        for i in 0..len {
            if self.mono {
                let x = (block[0][i] as f32 + block[1][i] as f32) * 0.5;
                let y = f64::from(self.sample(&mut s.l[0], x));
                block[0][i] = y;
                block[1][i] = y;
            } else {
                for c in 0..2 {
                    block[c][i] = f64::from(self.sample(&mut s.l[c], block[c][i] as f32));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kemu vectors (kontra-w5/bigfuzz_native.py): ctor, init, apply, settled ramp, kernel.
    #[test]
    fn matches_the_native_big_fuzz() {
        let cases: [(u32, [f32; 5]); 5] = [
            (48_000, [0.5, 0.5, 0.5, 0.5, 0.0]),
            (48_000, [0.9, 0.2, 0.8, 0.3, 0.0]),
            (48_000, [0.1, 1.0, 0.0, 1.0, 0.0]),
            (44_100, [0.7, 0.3, 0.6, 0.1, 1.0]),
            (96_000, [1.0, 0.0, 1.0, 0.7, 0.0]),
        ];
        super::super::tests::check(include_bytes!("kontakt_bigfuzz_vectors.f32le"), cases.len(), |n| {
            let (rate, k) = cases[n];
            let unit = Engine::new(k, rate);
            let mut s = State::default();
            Box::new(move |block: &mut Planar, len| unit.process(&mut s, block, len))
        });
    }
}
