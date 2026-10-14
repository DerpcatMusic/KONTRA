//! Kontakt Tape Saturator (`NI::KONTAKTFX::Tape`, module 0x42) as the color chain's shaper:
//! process 0x140ae1190, designers 0x140ae1f80 (body 0x140ae0200) and 0x140adff50 (roll-off),
//! drive 0x140ae1ee0, NL table 0x144fc5510 (4098 entries, saved as `tables/tape.f32le`).
//! Base rate: HP 25 Hz + 17 Hz peak; oversampled (x2 at ≤ 50 kHz, x4 with Quality at ≤ 50
//! kHz) through the shared Kaiser stage: LP 13 kHz, bell/shelf, the interpolated table
//! shaper evaluated every other sample, roll-off LPs, 500 Hz / 1.6 kHz post; base rate 40 Hz
//! shelf, 10 Hz HP and the 27.5 Hz warmth LP. Four f32 lanes per stereo pair in the native
//! op order (model kcolor/tape.py, checked against the emulated core).
use super::super::kaiser::{self, Kaiser};
use super::super::Planar;
pub use sampler_ir::fx::color::Tape;

const TABLE_BYTES: &[u8; 4098 * 4] = include_bytes!("tables/tape.f32le");

fn table(i: usize) -> f32 {
    f32::from_le_bytes(TABLE_BYTES[i * 4..i * 4 + 4].try_into().unwrap())
}

type Lanes = [f32; 4];

fn lanes(a: f32, b: f32) -> Lanes {
    [a, a, b, b]
}

/// `f32(tan(arg) * two)`.
fn tanw(arg: f64, two: f64) -> f32 {
    (arg.tan() * two) as f32
}

/// A bilinear second-order low-pass at `w` (prewarped), Q-term `q`, rate `fo`:
/// `(-a1, -a2, b0, b1)`.
fn lowpass(w: f32, q: f32, fo: f32) -> [f32; 4] {
    let (k4, k8) = ((fo * fo) * 4.0, (fo * fo) * 8.0);
    let t = ((fo + fo) * w) * q;
    let w2 = w * w;
    let r = 1.0 / ((t + k4) + w2);
    [-((((w + w) * w) - k8) * r), -(((k4 - t) + w2) * r), w2 * r, ((w + w) * w) * r]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    /// 0x1460..0x14a0: a1, a2, b0, b1, b2.
    pre: [Lanes; 5],
    /// 0x1730..0x17c0.
    os: [Lanes; 10],
    /// 0x1a50..0x1ae0.
    post: [Lanes; 10],
    /// 0x1d70..0x1dc0.
    fin: [Lanes; 6],
    drive: f32,
    warmth: f32,
    resampler: Kaiser,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    pre: [Lanes; 5],
    os: [Lanes; 5],
    post: [Lanes; 5],
    fin: [Lanes; 4],
    shaped: Lanes,
    held: Lanes,
    drive: f32,
    odd: bool,
    up: [kaiser::State; 2],
    down: [kaiser::State; 2],
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.pre.iter().chain(&self.os).chain(&self.post).chain(&self.fin).flatten().all(|v| v.is_finite())
            && self.up.iter().chain(&self.down).all(kaiser::State::finite)
    }
}

impl Engine {
    #[allow(clippy::excessive_precision, clippy::too_many_lines)]
    pub(super) fn new(t: Tape, rate: u32) -> Self {
        let sr = rate as f32;
        let factor: usize = match (t.quality, sr > 50_000.0, sr > 100_000.0) {
            (_, _, true) | (false, true, _) => 1,
            (false, false, _) | (true, true, false) => 2,
            (true, false, _) => 4,
        };
        let fo = factor as f32 * sr;
        let fod = f64::from(fo);
        let inv = 1.0 / fod;
        let two = fod + fod;
        let srd = f64::from(sr);
        let isr = 1.0 / srd;
        let two_s = 2.0 * srd;
        let (k4, k8) = ((fo * fo) * 4.0, (fo * fo) * 8.0);
        let z = 0.0;

        // HP 25 Hz at the oversampled rate (lanes 0, 1).
        let w = tanw(inv * 78.539_816_339_744_83, two);
        let t1 = ((fo + fo) * w) * 1.414_427_16;
        let den = (k4 + t1) + w * w;
        let a1 = (k8 - (w + w) * w) / den;
        let a2 = -(((k4 - t1) + w * w) / den);
        let b0 = k4 / den;
        let b1 = ((fo * fo) * -8.0) / den;
        // 17 Hz peak at the base rate (lanes 2, 3).
        let v = tanw(isr * 53.407_075_111_026_49, two_s);
        let (s4, s8, sr2) = ((sr * sr) * 4.0, (sr * sr) * 8.0, sr + sr);
        let v2 = v + v;
        let q = v2 * sr;
        let g = q * 1.556_558_85;
        let num1 = v2 * v - s8;
        let d2 = (s4 + v2 * sr2) + v * v;
        let r = 1.0 / d2;
        let n2 = (s4 - v2 * sr2) + v * v;
        let pb1 = num1 * r;
        let pre = [
            [a1, a1, -pb1, -pb1],
            lanes(a2, -(r * n2)),
            lanes(b0, (q * 1.556_558_85 + d2) * r),
            lanes(b1, pb1),
            lanes(b0, (n2 - g) * r),
        ];

        // LP 13 kHz on all lanes.
        let [l1, l2, l3, l4] = lowpass(tanw(inv * 81_681.408_993_334_63, two), 0.555_555_582, fo);
        // Bell at 13 kHz above 100 kHz, else a first-order shelf at 10 kHz (lanes 0, 1).
        let (e1, e2, g0, g1, g2) = if fo > 100_000.0 {
            let w = tanw(81_681.408_993_334_63 / fod, two);
            let (f4, f8) = ((fo * fo) * 4.0, (fo * fo) * 8.0);
            let t5 = (fo * 5.0) * w;
            let w2 = w * w;
            let d = (f4 + t5) + w2;
            let r = 1.0 / d;
            let n2 = (f4 - t5) + w2;
            let gg = ((w + w) * fo) * 1.945_698_5;
            (-((((w + w) * w) - f8) * r), -(r * n2), (gg + d) * r, (((w + w) * w) - f8) * r, (n2 - gg) * r)
        } else {
            let w = tanw(62_831.853_071_795_86 / fod, two);
            let kk = (w * 0.5) / fo;
            let r = 1.0 / (kk + 1.0);
            (-(r * (1.0 - kk)), 0.0, ((kk + 1.0) + 2.981_071_95) * r, ((1.0 - kk) - 2.981_071_95) * r, 0.0)
        };
        let os = [
            [l1; 4],
            [l2; 4],
            [l3; 4],
            [l4; 4],
            [l3; 4],
            lanes(e1, z),
            lanes(e2, z),
            lanes(g0, 1.0),
            lanes(g1, z),
            lanes(g2, z),
        ];

        // Roll-off LPs: (h + 1) 10 kHz (lanes 0, 1), 8 h + 12 kHz (lanes 2, 3).
        let h = t.rolloff;
        let rolloff = |fc: f32, q: f32| lowpass(tanw((f64::from(fc) * std::f64::consts::PI) * inv, two), q, fo);
        let [ra, rb, rc, rd] = rolloff((h + 1.0) * 10_000.0, 1.428_571_46);
        let [sa, sb, sc, sd] = rolloff(h * 8000.0 + 12_000.0, 1.25);
        // Post: s/w at 500 Hz (lanes 0, 1), LP 1.6 kHz Q-term 2.5 (lanes 2, 3).
        let w = tanw(inv * 3_141.592_653_589_793, two);
        let ri = 1.0 / w;
        let [pa1, pa2, pb0, pb1] = lowpass(tanw(inv * 10_053.096_491_487_338, two), 2.5, fo);
        let post = [
            lanes(ra, sa),
            lanes(rb, sb),
            lanes(rc, sc),
            lanes(rd, sd),
            lanes(rc, sc),
            lanes(-(ri * w), pa1),
            lanes(z, pa2),
            lanes((fo + fo) * ri, pb0),
            lanes((fo * -2.0) * ri, pb1),
            lanes(z, pb0),
        ];

        // Base rate: 40 Hz shelf (lanes 0, 1), 10 Hz first-order HP (lanes 2, 3), 27.5 Hz warmth LP.
        let w = tanw(isr * 251.327_412_287_183_45, two_s);
        let r = 1.0 / (sr2 + w);
        let dlt = w - sr2;
        let gw = w * 0.122_018_456;
        let (h_a, h_b0, h_b1) = (-(r * dlt), (gw + (sr2 + w)) * r, (gw + dlt) * r);
        let w = tanw(isr * 62.831_853_071_795_86, two_s);
        let kk = (w * 0.5) / sr;
        let r10 = 1.0 / (kk + 1.0);
        let w = tanw(isr * 172.787_595_947_438_63, two_s);
        let r = 1.0 / (sr2 + w);
        let gl = (w * 27.183_832_2) * r;
        let fin = [
            lanes(h_a, (1.0 - kk) * r10),
            lanes(h_b0, r10 * 0.707_945_764),
            lanes(h_b1, r10 * -0.707_945_764),
            lanes(z, -((w - sr2) * r)),
            lanes(z, gl),
            lanes(z, gl),
        ];
        Self {
            pre,
            os,
            post,
            fin,
            drive: 300.0 * (10f64.powf(48.0 * f64::from(t.gain) / 20.0) as f32),
            warmth: 10f32.powf(0.05 * ((t.warmth * 6.0) - 3.0)),
            resampler: Kaiser::new(factor),
        }
    }

    /// The interpolated table shaper at drive `d`.
    fn shape(v: Lanes, d: Lanes) -> Lanes {
        std::array::from_fn(|k| {
            let d = d[k].max(1e-6);
            let x = v[k] * d;
            let a = x.abs().min(4096.0);
            let i = a as usize;
            let (t0, t1) = (table(i), table(i + 1));
            let y = ((t1 - t0) * (a - i as f32) + t0) * ((1.0 / d) * 1_395.189_2);
            if x >= 0.0 { y } else { -y }
        })
    }

    #[inline(always)]
    fn oversampled(&self, s: &mut State, a: f32, b: f32) -> (f32, f32) {
        let c = &self.os;
        let [x1, x2, y1, y2, o] = s.os;
        let x = [a, b, o[0], o[1]];
        let v: Lanes = std::array::from_fn(|k| (x[k] + x1[k] * c[0][k]) + x2[k] * c[1][k]);
        let u: Lanes = std::array::from_fn(|k| {
            (((c[2][k] * v[k] + x1[k] * c[3][k]) + x2[k] * c[4][k]) + y1[k] * c[5][k]) + y2[k] * c[6][k]
        });
        let zo: Lanes = std::array::from_fn(|k| (c[7][k] * u[k] + y1[k] * c[8][k]) + y2[k] * c[9][k]);
        s.os = [v, x1, u, y1, zo];
        let shaped = if s.odd {
            s.held = zo;
            [s.shaped[0], s.shaped[1]]
        } else {
            let d = s.drive;
            s.shaped = Self::shape([zo[2], zo[3], s.held[2], s.held[3]], [self.drive, self.drive, d, d]);
            [s.shaped[2], s.shaped[3]]
        };
        if s.odd {
            s.drive = self.drive;
        }
        s.odd = !s.odd;
        let c = &self.post;
        let [s1, s2, q1, q2, p] = s.post;
        let x = [shaped[0], shaped[1], p[0], p[1]];
        let w: Lanes = std::array::from_fn(|k| (x[k] + s1[k] * c[0][k]) + s2[k] * c[1][k]);
        let y: Lanes = std::array::from_fn(|k| {
            (((w[k] * c[2][k] + s1[k] * c[3][k]) + s2[k] * c[4][k]) + q1[k] * c[5][k]) + q2[k] * c[6][k]
        });
        let out: Lanes = std::array::from_fn(|k| (q1[k] * c[8][k] + y[k] * c[7][k]) + q2[k] * c[9][k]);
        s.post = [w, s1, y, q1, out];
        (out[2], out[3])
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        let n = self.resampler.factor();
        for i in 0..len {
            let c = &self.pre;
            let [x1, x2, y1, y2, o] = s.pre;
            let x = [block[0][i] as f32, block[1][i] as f32, o[0], o[1]];
            let y: Lanes = std::array::from_fn(|k| {
                (((x1[k] * c[3][k] + x[k] * c[2][k]) + x2[k] * c[4][k]) + c[0][k] * y1[k]) + y2[k] * c[1][k]
            });
            s.pre = [x, x1, y, y1, y];
            let (l, r) = if n == 1 {
                self.oversampled(s, y[2], y[3])
            } else {
                let mut up = [[0f32; kaiser::MAX]; 2];
                self.resampler.up(&mut s.up[0], y[2], &mut up[0][..n]);
                self.resampler.up(&mut s.up[1], y[3], &mut up[1][..n]);
                let mut frame = [[0f32; kaiser::MAX]; 2];
                for p in 0..n {
                    (frame[0][p], frame[1][p]) = self.oversampled(s, up[0][p], up[1][p]);
                }
                (self.resampler.down(&mut s.down[0], &frame[0][..n]), self.resampler.down(&mut s.down[1], &frame[1][..n]))
            };
            let c = &self.fin;
            let [x1, y1, z1, b1] = s.fin;
            let x = [l, r, z1[0], z1[1]];
            let a: Lanes = std::array::from_fn(|k| (x[k] * c[1][k] + x1[k] * c[2][k]) + y1[k] * c[0][k]);
            let b: Lanes = std::array::from_fn(|k| (y1[k] * c[5][k] + a[k] * c[4][k]) + c[3][k] * b1[k]);
            s.fin = [x, a, a, b];
            block[0][i] = f64::from(self.warmth * b[2] + a[2]);
            block[1][i] = f64::from(self.warmth * b[3] + a[3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emulated native core (kcolor/vec_tape.json, 48 kHz, left of a mono input): an
    /// impulse of 0.5 (128 frames) and a 0.5 sine at 200 Hz, frames 1920..2400.
    #[test]
    fn matches_the_native_core() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("tables/kontakt_tape_vectors.json")).unwrap();
        const BLOCK: usize = super::super::super::BLOCK;
        for case in cases.as_array().unwrap() {
            let p: Vec<f32> = case["params"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
            let unit = Engine::new(Tape { gain: p[0], warmth: p[1], rolloff: p[2], quality: p[3] > 0.5 }, 48_000);
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
                    assert!((out[at + i] - n).abs() < 2e-6, "{p:?} {name} {i}: {} vs {n}", out[at + i]);
                }
            };
            check("impulse", &run(&|i| if i == 0 { 0.5 } else { 0.0 }, 128), &case["impulse"], 0);
            let sine = |i: usize| (0.5 * (std::f64::consts::TAU * 200.0 * i as f64 / 48_000.0).sin()) as f32;
            check("sine", &run(&sine, 2400), &case["sine"], 1920);
        }
    }
}
