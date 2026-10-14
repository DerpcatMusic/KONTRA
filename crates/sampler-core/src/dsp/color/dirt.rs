//! Kontakt Dirt (effektrig Dirt core, module 0x67; ctor 0x14193ec90, process 0x141940da0) as
//! the color chain's shaper, inside the effektrig x2 pair. Two drive stages A/B in four f32
//! lanes [A.l, A.r, B.l, B.r]: an input level follower, a TPT low shelf (tone), a 16 kHz TPT
//! low-pass, a diode-capacitor stage solved by Newton-Halley (20 iterations max), blend and
//! compensation, then the serial/parallel routing (fed back through a 1-frame core delay) and
//! the output mix. Laws from setters 0x14193f620/f930/f910/f920/f760/f5e0 and prepare
//! 0x1419404d0 / 0x141942450 (model kcolor/dirt.py, checked against the emulated core).
// ponytail: knobs are constants; the native routing crossfade and smoothers are settled.
use super::super::kaiser::{Effektrig, EffektrigState};
use super::super::Planar;
pub use sampler_ir::fx::color::Dirt;

type Lanes = [f32; 4];

fn map(f: impl Fn(usize) -> f32) -> Lanes {
    std::array::from_fn(f)
}

fn pair(a: f32, b: f32) -> Lanes {
    [a, a, b, b]
}

fn tanpoly(w: f32) -> f32 {
    let t = w * w;
    (((0.333_333_343 * w) * t) + w) + (((0.133_333_34 * w) * t) * t)
}

/// Rs, C and makeup per diode type (0x42200000.., 0x358637bd.., 0x3f6d8699..).
const RS: [f32; 3] = [40.0, 2200.0, 200_000.0];
const CAP: [f32; 3] = [1e-6, 1e-8, 1e-9];
const MAKEUP: [f32; 3] = [0.927_835_047, 1.168_831_11, 1.764_705_9];
/// Routing gains (0x147698090 / 0x147697fd0 / 0x144714b70).
const ROUTING: [Lanes; 3] = [[0.0, 1.0, 0.0, 1.0], [1.0, 0.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0]];

/// Core defaults by index (p15 level 0.75 is never written by the apply).
pub(crate) const DEFAULTS: [f32; 16] = [0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0, 1.0, 0.5, 0.75];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    p: [f32; 16],
    attack: f32,
    release: f32,
    q: Lanes,
    a: Lanes,
    sqrt_a: Lanes,
    g: Lanes,
    d: Lanes,
    a10: Lanes,
    a20: Lanes,
    af0: f32,
    b00: f32,
    drive: Lanes,
    b60: Lanes,
    b70: Lanes,
    b90: Lanes,
    ba0: Lanes,
    bb0: Lanes,
    c9c0: Lanes,
    c9d0: Lanes,
    c9e0: Lanes,
    c960: f32,
    mix: Lanes,
    wet: f32,
    level: f32,
    oversampler: Option<Effektrig>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    envelope: f32,
    shelf: [Lanes; 2],
    lowpass: [Lanes; 2],
    x: Lanes,
    p940: Lanes,
    p930: Lanes,
    /// The routing feedback and the dry path, one or two core frames back.
    feedback: [Lanes; 2],
    dry: [Lanes; 2],
    os: [EffektrigState; 2],
}

impl State {
    pub(super) fn finite(&self) -> bool {
        [self.shelf, self.lowpass, self.feedback, self.dry].iter().flatten().flatten().chain(&self.x).chain(&self.p930).all(|v| v.is_finite())
            && self.envelope.is_finite()
            && self.os.iter().all(EffektrigState::finite)
    }
}

/// Whether the wrapper (exponent 1) oversamples at `rate`.
// ponytail: below ~31 kHz the native wrapper runs x4; these cores stay at x2 there.
pub(super) fn oversampled(rate: u32) -> bool {
    super::super::kaiser::Wrapper::new(1, rate).factor() >= 2
}

impl Engine {
    #[allow(clippy::excessive_precision)]
    pub(super) fn new(dirt: Dirt, rate: u32) -> Self {
        let p = dirt.knobs;
        let os = oversampled(rate);
        let sr = rate as f32 * if os { 2.0 } else { 1.0 };
        let a30 = std::f32::consts::PI / sr;
        let g16 = tanpoly(16_000.0 * a30);
        let q = pair(p[6] * p[6] + 1.0, p[7] * p[7] + 1.0);
        let shelf = |v: f32| ((1.0 - v) * 24.0) - 12.0;
        let fv = pair(shelf(p[4]), shelf(p[5]));
        let a = map(|k| ((0.001_656_843_12 * fv[k]) * fv[k]) + ((0.057_564_627_4 * fv[k]) + 1.0));
        let sqrt_a = a.map(f32::sqrt);
        let g = map(|k| {
            let w = (200.0 / sqrt_a[k]) * a30;
            let t = w * w;
            (((0.333_333_343 * w) * t) + w) + (((0.133_333_34 * w) * t) * t)
        });
        let d = map(|k| {
            let kk = 0.707_106_769 * g[k];
            1.0 / (((kk + kk) + 1.0) + g[k] * g[k])
        });
        let drive = pair(((17.5 * p[0]) * p[0]) + 0.5, ((17.5 * p[1]) * p[1]) + 0.5);
        let comp = pair(p[10], p[11]);
        let sq = drive.map(f32::sqrt);
        let b60 = map(|k| if drive[k] < 1.0 { 1.0 / drive[k] } else { (comp[k] / sq[k]) + (1.0 - comp[k]) });
        let blend = pair(p[2], p[3]);
        let t = map(|k| 1.0 - (blend[k] + blend[k]));
        let low = |k: usize| blend[k] < 0.5;
        let ty = |v: f32| ((v * 3.0) as i32).clamp(0, 2) as usize;
        let types = [ty(p[8]), ty(p[8]), ty(p[9]), ty(p[9])];
        let c9c0 = map(|k| {
            let cp = CAP[types[k]];
            (1.0 / ((sr * cp) + (sr * cp))) / RS[types[k]]
        });
        let c9d0 = map(|k| 1.0 / (1.0 + c9c0[k]));
        let c9e0 = map(|k| {
            let cp = CAP[types[k]];
            -(1.0 / ((sr * cp) + (sr * cp))) * c9d0[k]
        });
        let mode = ty(p[12]);
        Self {
            p,
            attack: 1.0 - (-4.605_170_25 / (1e-4 * sr)).exp(),
            release: 1.0 - (-4.605_170_25 / (sr * 0.4)).exp(),
            q,
            a,
            sqrt_a,
            g,
            d,
            a10: a.map(|a| a * a - 1.0),
            a20: a.map(|a| (a - 1.0) * 0.707_106_769),
            af0: g16,
            b00: 1.0 / ((((g16 * 0.707_106_769) + (g16 * 0.707_106_769)) + 1.0) + g16 * g16),
            drive,
            b60,
            b70: types.map(|t| MAKEUP[t]),
            b90: map(|k| if low(k) { blend[k] + blend[k] } else { 1.0 - 0.25 * t[k] }),
            ba0: map(|k| if low(k) { t[k] } else { 0.0 }),
            bb0: map(|k| {
                if low(k) {
                    0.0
                } else {
                    (((if drive[k] < 1.0 { drive[k] } else { sq[k] }) * 0.5) * b60[k]) * t[k]
                }
            }),
            c9c0,
            c9d0,
            c9e0,
            c960: 0.512_820_482 + 0.487_179_458,
            mix: ROUTING[mode],
            wet: if mode == 2 { 1.0 } else { 0.0 },
            level: 10f32.powf((p[15] * 1.200_000_05) - 0.9),
            oversampler: os.then(Effektrig::new),
        }
    }

    /// The diode stage: Newton-Halley on `x` for drive `ac8`.
    #[allow(clippy::excessive_precision)]
    #[inline(always)]
    fn solve(&self, mut x: Lanes, ac8: Lanes) -> Lanes {
        const IS: f32 = 2.519_999_99e-9;
        const C990: f32 = 22.075_056_1;
        let q = self.q;
        let c18 = map(|k| (1.0 - self.c9e0[k]) * C990);
        let r = map(|k| 1.0 / (q[k] + 1.0));
        for _ in 0..20 {
            let mut converged = true;
            for k in 0..4 {
                let ee = (ac8[k] - x[k] * c18[k]) * 0.031_25;
                let pp = (1.0 + ee).max(1e-8).min(4.0);
                let nn = (1.0 - q[k] * ee).max(1e-8).min(4.0);
                let (mut p32, mut n32) = (pp, nn);
                for _ in 0..5 {
                    p32 = p32 * p32;
                    n32 = n32 * n32;
                }
                let (p, n) = (p32 * r[k], n32 * r[k]);
                let f65 = (1.0 / pp) * 0.031_25;
                let f59 = ((1.0 / nn) * q[k]) * 0.031_25;
                let p1 = (f65 * 32.0) * p;
                let p2 = (f65 * 31.0) * p1;
                let n1 = (f59 * 32.0) * n;
                let n2 = (f59 * 31.0) * n1;
                let (bd8, bb8) = (c18[k] * c18[k], (c18[k] * c18[k]) * c18[k]);
                let b88 = x[k] - IS * (p - n);
                let ba8 = 1.0 + (IS * c18[k]) * (n1 + p1);
                let d98 = (p2 - n2) * ((-IS) * bd8);
                let b48 = (((f65 * 30.0) * p2) + ((30.0 * f59) * n2)) * (IS * bb8);
                let dc8 = 1.0 / ba8;
                let dd8 = -(b88 * dc8);
                let d88 = 1.0 + (((0.5 * dd8) * d98) * dc8);
                let t1 = (((0.166_666_672 * dd8) * dd8) * b48) * dc8;
                let t2 = (dd8 * d98) * dc8;
                let d78 = 1.0 / ((1.0 + t2) + t1);
                let sel = if d78 > 0.0 { d88 * d78 } else { 1.0 / d88 };
                let dx = dd8 * sel.max(0.5).min(8.0);
                converged &= dx.abs() < 9.999_999_75e-5;
                x[k] += dx;
            }
            if converged {
                break;
            }
        }
        x
    }

    #[allow(clippy::excessive_precision)]
    #[inline(always)]
    fn tick(&self, s: &mut State, l: f32, r: f32, delay: usize) -> (f32, f32) {
        let m = l.abs().max(r.abs());
        let e = s.envelope;
        let e = (m - e) * (if m <= e { self.release } else { self.attack }) + e;
        s.envelope = e;
        let g = if 1.0 < e { 1.0 / e } else { 1.0 };
        let fbv = s.feedback[0];
        let mx = self.mix;
        let clip = |x: f32| if x.abs() < 1.05 { x } else { 1.05f32.copysign(x) };
        let (cl, cr) = (clip(g * l), clip(g * r));
        let inp = [
            (fbv[2] - cl) * mx[0] + cl,
            (fbv[3] - cr) * mx[0] + cr,
            (fbv[0] - cl) * mx[1] + cl,
            (fbv[1] - cr) * mx[1] + cr,
        ];
        let [a80, a90] = s.shelf;
        let v1 = map(|k| (a80[k] + (inp[k] - a90[k]) * self.g[k]) * self.d[k]);
        let s2n = map(|k| a90[k] + self.g[k] * (v1[k] + v1[k]));
        let u = map(|k| ((inp[k] + ((a90[k] + s2n[k]) * 0.5) * self.a10[k]) + self.a20[k] * v1[k]) / self.sqrt_a[k]);
        s.shelf = [map(|k| (v1[k] + v1[k]) - a80[k]), s2n];
        let [b10, b20] = s.lowpass;
        let b1 = map(|k| ((u[k] - b20[k]) * self.af0 + b10[k]) * self.b00);
        let b20n = map(|k| (b1[k] + b1[k]) * self.af0 + b20[k]);
        let lp = map(|k| (b20[k] + b20n[k]) * 0.5);
        s.lowpass = [map(|k| (b1[k] + b1[k]) - b10[k]), b20n];
        let d58 = map(|k| lp[k] * self.drive[k]);
        let d68 = map(|k| ((((d58[k] + s.p940[k]) - s.p930[k]) * self.c9c0[k] + s.p930[k]) * self.c9d0[k]) + self.c9e0[k] * s.x[k]);
        let x = self.solve(s.x, map(|k| d68[k] * 22.075_056_1));
        let d48 = map(|k| (self.c9e0[k] * x[k] + d68[k]) * self.c960);
        s.x = x;
        s.p940 = d58;
        s.p930 = d48;
        let d28 = map(|k| {
            let a88 = (d48[k] * self.b60[k]) * self.b70[k];
            ((self.b90[k] * a88) + (self.ba0[k] * inp[k])) + ((self.bb0[k] * lp[k]) * self.c960)
        });
        let dclip = [cl, cr, cl, cr];
        let a38 = (mx[0] - mx[1]).abs();
        s.feedback[0] = s.feedback[1];
        s.feedback[delay - 1] = map(|k| dclip[k] + a38 * (d28[k] - dclip[k]));
        let d18 = map(|k| d28[k] * (mx[2] * self.level));
        let cf8 = map(|k| d28[2 + (k & 1)] * (mx[3] * self.level));
        let dry = s.dry[0];
        s.dry[0] = s.dry[1];
        s.dry[delay - 1] = [l, r, l, r];
        let wv = self.wet;
        let kk = (self.p[14] - self.p[13]) * wv + self.p[13];
        let out = map(|k| {
            let c9 = d18[k] + cf8[k];
            let bf8 = dry[k] + wv * (d18[k] - dry[k]);
            let o998 = c9 + wv * (cf8[k] - c9);
            bf8 + kk * (o998 - bf8)
        });
        (out[0], out[1])
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        for i in 0..len {
            let (l, r) = (block[0][i] as f32, block[1][i] as f32);
            let (l, r) = if let Some(os) = &self.oversampler {
                let (l0, l1) = os.up(&mut s.os[0], l);
                let (r0, r1) = os.up(&mut s.os[1], r);
                let a = self.tick(s, l0, r0, 2);
                let b = self.tick(s, l1, r1, 2);
                (os.down(&mut s.os[0], a.0, b.0), os.down(&mut s.os[1], a.1, b.1))
            } else {
                self.tick(s, l, r, 1)
            };
            block[0][i] = f64::from(l);
            block[1][i] = f64::from(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emulated native chain (kcolor/vec_dirt.json, 48 kHz, x2 inside): an impulse of
    /// 0.5 (128 frames) and a 0.5 sine at 200 Hz, frames 1920..2400, left channel.
    #[test]
    fn matches_the_native_core() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("tables/kontakt_dirt_vectors.json")).unwrap();
        const BLOCK: usize = super::super::super::BLOCK;
        for case in cases.as_array().unwrap() {
            let mut knobs = DEFAULTS;
            for (k, v) in case["params"].as_object().unwrap() {
                knobs[k.parse::<usize>().unwrap()] = v.as_f64().unwrap() as f32;
            }
            let unit = Engine::new(Dirt { knobs }, 48_000);
            let run = |input: &dyn Fn(usize) -> f32, frames: usize| {
                let mut s = State::default();
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
