//! Kontakt Cabinet (`BEffectCabinet`, vtable 0x144f14c20). Type law 0x140ab8c70: 11 cabinets
//! (0x144fbe580 -> 0x144f6b130), each 23 zero/pole pairs plus a gain (0x144f6bb60, 185 f32 per
//! cabinet), bilinear-mapped by 0x140a9e600 (Smith division 0x140816550) at `sr * size / pi`,
//! and a bass/treble shelf section. SR setup 0x140abead0: the "air" room of 8 delay taps
//! (`sr * 0.015 * 1.17^k`) with a one-pole band-pass feedback loop. Kernel 0x140aa9510 runs the
//! 24 sections as two 12-section lane chains, the second one sample behind the first.
// ponytail: the 32-frame control ramps (0x140ac2fa0) and the type-change mute/fade
// (state 0x210) are load-time transients; the controls are constant here, so the unit starts
// on its targets.
use super::super::super::Planar;
use super::super::super::filter::ladder_kernel::{exponential_effektrig as table, prepare as prepare_table};

const ROW: usize = 185;
/// Tap gains 0x149e6c140 (A0, B4, A1, B5, A2, B6, A3, B7) and the feedback mix 0x149e6c168.
const TAPS: [f32; 8] = [1.0, 1.0, -0.730_513_63, -0.730_513_63, 0.533_650_1, 0.533_650_1, -0.389_838_67, -0.389_838_67];
const FEEDBACK: f32 = 0.16;
const INV_PI: f32 = 0.318_309_87;

/// One transposed section `[b1, b2, a1, -a2]`: `y = x + s1`.
type Section = [f32; 4];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct State {
    s: [[[f32; 2]; 24]; 2],
    held: [f32; 2],
    lp1: [f32; 2],
    lp2: [f32; 2],
    at: usize,
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.s.iter().flatten().flatten().chain(&self.held).chain(&self.lp1).chain(&self.lp2).all(|v| v.is_finite())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Engine {
    chain: [Section; 24],
    gains: [f32; 4],
    one_pole: [f32; 2],
    taps: [usize; 8],
    mask: usize,
}

/// `a / b` by Smith's method, as 0x140816550.
fn div([a, b]: [f32; 2], [c, d]: [f32; 2]) -> [f32; 2] {
    if c.is_nan() || d.is_nan() {
        return [f32::NAN; 2];
    }
    if d.abs() < c.abs() {
        let r = d / c;
        let den = r * d + c;
        if den.is_nan() || den == 0.0 {
            return [f32::NAN; 2];
        }
        let inv = 1.0 / den;
        return [(b * r + a) * inv, (b - a * r) * inv];
    }
    if d == 0.0 {
        let inv = 1.0 / c;
        return [inv * a, inv * b];
    }
    let r = c / d;
    let den = r * c + d;
    if den.is_nan() || den == 0.0 {
        return [f32::NAN; 2];
    }
    let inv = 1.0 / den;
    [(a * r + b) * inv, (b * r - a) * inv]
}

/// One zero/pole pair set through the bilinear map at `k` (0x140a9e600); scales `gain`.
fn section(p: &[f32], k: f32, gain: &mut f32) -> Section {
    let (mut p4, p5, mut p6, p7) = (p[4], p[5], p[6], p[7]);
    if 0.0 < p4 {
        p4 = -p4;
    }
    if 0.0 < p6 {
        p6 = -p6;
    }
    let limit = k * -0.000_3;
    if limit < p4 && (p4 != 0.0 || p5 != 0.0) {
        p4 = limit;
    }
    if limit < p6 && (p4 != 0.0 || p5 != 0.0) {
        p6 = limit;
    }
    let z1 = div([p[0] + k, p[1]], [k - p[0], -p[1]]);
    let q1 = div([p4 + k, p5], [k - p4, -p5]);
    let z2 = div([p[2] + k, p[3]], [k - p[2], -p[3]]);
    let q2 = div([p6 + k, p7], [k - p6, -p7]);
    let zeros = [(k - p[0]) * (k - p[2]) - -p[3] * -p[1], -p[1] * (k - p[2]) + -p[3] * (k - p[0])];
    let poles = [(k - p4) * (k - p6) - -p7 * -p5, (k - p6) * -p5 - (k - p4) * p7];
    *gain = div(zeros, poles)[0] * *gain;
    [-(z1[0] + z2[0]), z1[0] * z2[0] - z1[1] * z2[1], q1[0] + q2[0], q1[1] * q2[1] - q1[0] * q2[0]]
}

#[inline(always)]
fn run(st: &mut [f32; 2], c: &Section, x: f32) -> f32 {
    let y = x + st[0];
    st[0] = y * c[2] + x * c[0] + st[1];
    st[1] = x * c[1] + y * c[3];
    y
}

/// The air lines' power-of-two length (0x140abead0): A and B side by side per frame.
pub(crate) fn frames(rate: u32) -> usize {
    let shift = 1 - ((rate as f32 * 0.045_018_62).ln() * -1.442_695) as i32 as i8 as i32;
    1 << (shift & 31)
}

impl Engine {
    /// `kind` 0..=10, `size` 0..1, `air` 0..1, `treble`/`bass` -1..1 (record fields as stored).
    pub(crate) fn new(kind: f32, size: f32, air: f32, treble: f32, bass: f32, rate: u32) -> Self {
        prepare_table();
        let sr = rate as f32;
        let bytes = include_bytes!("cabinet.f32le");
        let kind = (kind as i32).clamp(0, 10) as usize;
        let row: [f32; ROW] = std::array::from_fn(|i| {
            let o = (kind * ROW + i) * 4;
            f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
        });
        let scale = table(size * 61.748_78 + 1_169.125_6);
        let bass = table(bass * 149.486_75 + 1_200.0);
        let treble = table(treble * 149.486_75 + 1_200.0);
        let f20 = sr * INV_PI;
        let k = sr * scale * INV_PI;
        let f14 = -349.230_01 / bass.sqrt();
        let f13 = bass.sqrt() * -349.230_01;
        let f17 = treble.sqrt() * -1_975.5;
        let f19 = f20 - f13;
        let f11 = -1_975.5 / treble.sqrt();
        let f22 = f20 - f11;
        let f18 = f20 - f14;
        let f16 = (f20 + f13) / f19;
        let f11 = (f20 + f11) / f22;
        let f13b = f20 - f17;
        let f17 = (f20 + f17) / f13b;
        let f11c = (f20 + f14) / f18;
        let shelf = [-(f11 + f16), f11 * f16, f17 + f11c, -(f17 * f11c)];
        let mut gain = row[184];
        let sections: [Section; 23] = std::array::from_fn(|s| section(&row[8 * s..], k, &mut gain));
        gain = gain * ((f22 * f19 * treble) / (f13b * f18));
        // Lanes 0/1 run rows 11..0 (even sections), lanes 2/3 the shelf then the odd ones.
        let mut chain = [[0.0; 4]; 24];
        for r in 0..12 {
            chain[r] = sections[22 - 2 * r];
        }
        chain[12] = shelf;
        for r in 1..12 {
            chain[12 + r] = sections[23 - 2 * r];
        }
        let inv = 6.283 / sr;
        let one_pole = [1_681.881_6_f32, 1_806.881_6].map(|i| (table(i) * inv).min(1.0));
        let len = frames(rate);
        let mut taps = [0; 8];
        let mut f = sr * 0.015;
        for slot in [0, 4, 1, 5, 2, 6, 3, 7] {
            taps[slot] = f as i32 as usize;
            f *= 1.17;
        }
        Self { chain, gains: [gain, gain, gain * air * 0.7, gain * air * 0.3], one_pole, taps, mask: len - 1 }
    }

    pub(crate) fn process(&self, s: &mut State, block: &mut Planar, len: usize, ring: &mut [[f64; 2]]) {
        let [c1, c2] = self.one_pole;
        let [g0, g1, g2, g3] = self.gains;
        for i in 0..len {
            let mut out = [0.0f32; 2];
            for (c, o) in out.iter_mut().enumerate() {
                let mut x = block[c][i] as f32;
                for (st, k) in s.s[c][..12].iter_mut().zip(&self.chain[..12]) {
                    x = run(st, k, x);
                }
                let mut y = std::mem::replace(&mut s.held[c], x);
                for (st, k) in s.s[c][12..].iter_mut().zip(&self.chain[12..]) {
                    y = run(st, k, y);
                }
                *o = y;
            }
            let tap = |n: usize, line: usize| ring[(self.taps[n] + s.at) & self.mask][line] as f32;
            let f29 = tap(0, 0) * TAPS[0] + tap(2, 0) * TAPS[4];
            let f48 = tap(4, 1) * TAPS[1] + tap(6, 1) * TAPS[5];
            let f50 = tap(1, 0) * TAPS[2] + tap(3, 0) * TAPS[6];
            let f38 = tap(5, 1) * TAPS[3] + tap(7, 1) * TAPS[7];
            s.at = s.at.wrapping_sub(1) & self.mask;
            let f40 = f50 + f29;
            let f44 = f38 + f48;
            let v = [
                ((f48 + f38 + f29 + f50) * FEEDBACK + out[0] * g2) - s.lp1[0],
                ((f29 + f50 + f48 + f38) * FEEDBACK + out[1] * g3) - s.lp1[1],
            ];
            for l in 0..2 {
                s.lp1[l] = v[l] * c1 + s.lp1[l];
                s.lp2[l] = (v[l] - s.lp2[l]) * c2 + s.lp2[l];
            }
            ring[s.at] = [f64::from(s.lp2[0]), f64::from(s.lp2[1])];
            block[0][i] = f64::from(f40 + out[0] * g0);
            block[1][i] = f64::from(f44 + out[1] * g1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::super::BLOCK;

    /// kemu vectors of the native type law, SR setup and kernel (kontra-w5/cab_native.py):
    /// a stereo impulse then an LCG noise burst, 1024 frames, ramps on their targets.
    #[test]
    fn matches_the_native_cabinet() {
        let raw = include_bytes!("kontakt_cabinet_vectors.f32le");
        let native: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let cases = [
            (48_000, 0.0, 0.5, 0.0, 0.0, 0.0),
            (48_000, 3.0, 0.2, 0.6, 0.4, -0.3),
            (48_000, 7.0, 0.9, 1.0, -1.0, 1.0),
            (44_100, 10.0, 0.5, 0.3, 0.0, 0.0),
            (96_000, 5.0, 0.7, 0.5, 0.25, -0.5),
        ];
        let mut seed: u64 = 12345;
        let input: Vec<[f32; 2]> = (0..1024)
            .map(|i| {
                seed = (seed * 1_103_515_245 + 12345) & 0x7fff_ffff;
                let noise = ((seed as f64 / f64::from(0x7fff_ffff) - 0.5) * 0.5) as f32;
                let burst = (64..192).contains(&i);
                [
                    if i == 0 { 1.0 } else if burst { noise } else { 0.0 },
                    if i == 3 { 0.5 } else if burst { (-f64::from(noise) * 0.7) as f32 } else { 0.0 },
                ]
            })
            .collect();
        for (n, &(rate, kind, size, air, treble, bass)) in cases.iter().enumerate() {
            let unit = Engine::new(kind, size, air, treble, bass, rate);
            let mut ring = vec![[0.0; 2]; frames(rate)];
            let mut s = State::default();
            for start in (0..1024).step_by(BLOCK) {
                let mut block: Planar = [[0.0; BLOCK]; 2];
                for i in 0..BLOCK {
                    block[0][i] = f64::from(input[start + i][0]);
                    block[1][i] = f64::from(input[start + i][1]);
                }
                unit.process(&mut s, &mut block, BLOCK, &mut ring);
                for i in 0..BLOCK {
                    for c in 0..2 {
                        let want = native[(n * 1024 + start + i) * 2 + c];
                        let got = block[c][i] as f32;
                        assert!((got - want).abs() <= 1e-6, "case {n} frame {} ch {c}: {got} vs {want}", start + i);
                    }
                }
            }
        }
    }
}
