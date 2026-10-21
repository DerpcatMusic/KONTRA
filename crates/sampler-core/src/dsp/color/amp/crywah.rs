//! Kontakt Cry Wah (`BEffectCryWah`, vtable 0x144fc0c78, ctor 0x140a85de0): pedal law
//! 0x140ad07f0, SR setup 0x140abef20 (bilinear prewarp at 0x6d4 = 21536.875 rad/s), kernel
//! 0x140aaa190: four staggered lanes of one bilinear section each (lane 0 the input's first
//! difference, lanes 2 and 3 the wah band-passes fed one sample behind), mixed by the pedal gains.
// ponytail: the pedal is constant (the 0x1c0 automation buffer and the 32-frame ramp
// 0x140acb7d0 only glide between settings), so the unit starts on its targets.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};

const W: f32 = 21_536.875;

/// The SSE `rcpps` + one Newton step, with an exact reciprocal (as emulated).
pub(super) fn rcp(x: f32) -> f32 {
    let r = 1.0 / x;
    (r + r) - r * r * x
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct State {
    /// Native layout per channel: y1, y2, lane inputs U, last U, two-back d.
    s: [[[f32; 4]; 5]; 2],
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.s.iter().flatten().flatten().all(|v| v.is_finite())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    p: [f32; 4],
    c: [f32; 4],
    fb1: [f32; 4],
    fb2: [f32; 4],
    norm: [f32; 4],
    mono: bool,
}

impl Engine {
    pub(crate) fn new(pedal: f32, mono: bool, rate: u32) -> Self {
        prepare_table();
        let sr = rate as f32;
        let x = (0.5 / sr) * W;
        let (s, c) = (f64::from(x).sin() as f32, f64::from(x).cos() as f32);
        let k = (c / s) * W;
        let c6f0 = k / (k + W);
        let m710 = (k - W) / (k + W);
        let c6f8 = W * k;
        let (kk, ww) = (k * k, W * W);
        // Pedal law 0x140ad07f0: targets p0..p3 (lane gains), q0..q2 (centre, Q, depth).
        let v = pedal;
        let q0 = table(v * 135.849_99 + 1_731.031_6) * 6.283_185_5;
        let q1 = 1.0 / (v * 0.210_74 + 0.102_27);
        let v2 = v * v;
        let f6 = v2 * -153.609_6;
        let p0 = v2 * 9.292_1 - 0.292_1;
        let p2 = table(f6 + 1_031.392_8) - 1.0;
        let q2 = table(f6 + 1_137.428_8) * 0.754_5;
        let d = (v - 1.0) * (v - 1.0);
        let f4 = v2 * v2 * 2.941_5;
        let f3 = d * d * 0.048_911_855;
        let p3 = f4 + f4 + 5.970_372;
        let p1 = 0.231_828_78 - (f3 + f3);
        // Coefficient update (kernel, flag 0x6e0).
        let c6fc = q0 * k;
        let m = [kk, q0 * q0, ww, -kk];
        let a = [(m[2] + m[0]) * q2, (m[0] + m[1]) * q1, (m[3] + m[2]) * q2, (m[1] + m[3]) * q1];
        Self {
            p: [p0, p1, p2, p3],
            c: [c6f0, 0.0, c6f8, c6fc],
            fb1: [m710, 0.0, a[2] * -2.0, a[3] * -2.0],
            fb2: [0.0, 0.0, -a[0] + c6f8, -a[1] + c6fc],
            norm: [rcp(1.0), rcp(1.0), rcp(a[0] + c6f8), rcp(a[1] + c6fc)],
            mono,
        }
    }

    fn sample(&self, st: &mut [[f32; 4]; 5], x: f32) -> f32 {
        let [y1, y2, u, v, d] = st;
        u[0] = x;
        v[0] = x;
        let y: [f32; 4] = std::array::from_fn(|l| (((u[l] - d[l]) * self.c[l] + y1[l] * self.fb1[l]) + self.fb2[l] * y2[l]) * self.norm[l]);
        *y2 = *y1;
        *y1 = y;
        *d = *v;
        *v = *u;
        u[3] *= self.p[1];
        let t: [f32; 4] = std::array::from_fn(|l| self.p[l] * y[l] + u[l]);
        *u = [t[3], t[1], t[0], t[2]];
        t[3]
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

    /// kemu vectors (kontra-w5/wah_native.py): ctor, SR setup, pedal law, wrapper ramp, kernel.
    #[test]
    fn matches_the_native_cry_wah() {
        let cases = [(48_000, 0.0, false), (48_000, 0.5, false), (48_000, 1.0, false), (44_100, 0.3, true), (96_000, 0.8, false)];
        super::super::tests::check(include_bytes!("kontakt_crywah_vectors.f32le"), cases.len(), |n| {
            let (rate, pedal, mono) = cases[n];
            let unit = Engine::new(pedal, mono, rate);
            let mut s = State::default();
            Box::new(move |block: &mut Planar, len| unit.process(&mut s, block, len))
        });
    }
}
