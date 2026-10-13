//! Kontakt Choral (0x5d) on the timemod line: seven modulated voices, each a
//! state-variable-filtered ring read with Catmull-Rom, recirculated through a
//! 7x7 circulant, rotated into the stereo field and low-shelved. f64 law of
//! the Kontakt 8.13.1 core (audio 0x14193be50); every constant is cited by
//! address. Voice tables: 0x14193cd70. Setters: amount 0x14193a2f0, rate
//! 0x14193b140, delay 0x14193ac00, voices 0x14193a3e0, width 0x14193a4a0,
//! feedback 0x14193a610, scatter 0x14193a620, invert 0x14193a6a0, mix
//! 0x14193a6d0, mode 0x14193a230 / 0x14193e160, prepare 0x14193b840.
use super::super::{Planar, flush};
use std::f64::consts::PI;

const VOICES: usize = 7;
/// Per voice after the rings: SVF s1, s2, last output, [smoothed m, write
/// position], [LFO1 phase, LFO2 phase]. Then the shelf's s1, s2.
const VOICE_STATE: usize = 5;

/// Voice tables (0x14193cd70), f32 bits: G0 delay scale, G1 rate scale, G2
/// LFO1 phase offset, G3 LFO2 rate ratio, G4 LFO2 depth, G5 filter Hz, G6
/// high-pass blend, G7 LFO1 shape, G8 modulation smoothing; seven voices each.
/// Modes 3 and 4 share a table; G8[6] is written after each branch.
const MODES: [[u32; 63]; 4] = [
    [
        0x3b449ba6, 0x3b449ba6, 0x3be56042, 0x3be56042, 0x3c343958, 0x3c343958, 0x3c54fdf4,
        0x3f800000, 0x3f800000, 0x3fb6db62, 0x3fb70a31, 0x3fdcb296, 0x3fdd2395, 0x3f3e075f,
        0x00000000, 0x3f000000, 0x00000000, 0x3f000000, 0x00000000, 0x3f000000, 0x00000000,
        0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x3dcccccd, 0x3dcccccd, 0x3f800000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3c23d70a, 0x3c23d70a, 0x00000000,
        0x459c4000, 0x459c4000, 0x459c4000, 0x459c4000, 0x461c4000, 0x461c4000, 0x42480000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000,
        0x3e4ccccd, 0x3e570a3d, 0x3e570a3d, 0x3e4ccccd, 0x3ecccccd, 0x3ecccccd, 0x00000000,
        0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd, 0x3ecccccd,
    ],
    [
        0x3be56042, 0x3be56042, 0x3be56042, 0x3be56042, 0x3c54fdf4, 0x3c54fdf4, 0x3ced9168,
        0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f3e075f,
        0x00000000, 0x3f000000, 0x3eaaaa3b, 0x3f55551d, 0x3f2aaa3b, 0x3e2aa8eb, 0x00000000,
        0x40833333, 0x40833333, 0x40800000, 0x40800000, 0x4083d70a, 0x4083f8cb, 0x3f800000,
        0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x00000000,
        0x457a0000, 0x457a0000, 0x45bb8000, 0x45bb8000, 0x461c4000, 0x461c4000, 0x42480000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000,
        0x3c23d70a, 0x3c23d70a, 0x3c23d70a, 0x3c54fdf4, 0x3c656042, 0x3c449ba6, 0x00000000,
        0x3e99999a, 0x3ea3d70a, 0x3ea3d70a, 0x3e99999a, 0x3ea8f5c3, 0x3e99999a, 0x3ecccccd,
    ],
    [
        0x3c343958, 0x3c343958, 0x3c343958, 0x3c343958, 0x3c343958, 0x3c343958, 0x3d591687,
        0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, 0x3f3e075f,
        0x00000000, 0x3f000000, 0x3f000000, 0x00000000, 0x3e800000, 0x3f400000, 0x00000000,
        0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x3f800000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
        0x463b8000, 0x463b8000, 0x43160000, 0x43160000, 0x469c4000, 0x469c4000, 0x42480000,
        0x00000000, 0x00000000, 0x3f800000, 0x3f800000, 0x00000000, 0x00000000, 0x3f800000,
        0x3dcccccd, 0x3dcccccd, 0x3dced917, 0x3dced917, 0x3ecccccd, 0x3ecccccd, 0x00000000,
        0x3e4ccccd, 0x3e6147ae, 0x3e6147ae, 0x3e4ccccd, 0x3e4ccccd, 0x3e4ccccd, 0x3f7d70a4,
    ],
    [
        0x3be56042, 0x3be56042, 0x3c343958, 0x3c343958, 0x3c54fdf4, 0x3c54fdf4, 0x3ced9168,
        0x3f800000, 0x3f800000, 0x3fb6db62, 0x3fb6db62, 0x3fdcb296, 0x3fdcb296, 0x3f3e075f,
        0x00000000, 0x00000000, 0x00000000, 0x3e800000, 0x00000000, 0x3e800000, 0x00000000,
        0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x40c33333, 0x3f800000,
        0x00000000, 0x00000000, 0x3e99999a, 0x3e99999a, 0x3f19999a, 0x3f19999a, 0x00000000,
        0x469c4000, 0x469c4000, 0x469c4000, 0x469c4000, 0x469c4000, 0x469c4000, 0x42480000,
        0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000,
        0x00000000, 0x3e4ccccd, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
        0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4, 0x3f7d70a4,
    ],
];

/// SVF damping halves (0x147697b60, 0x147697b70; the shelf copy at core +0x1c00
/// is stored from 0x147697b60 by the ctor at 0x141939c46) and the tan series
/// (0x147697b50 = 1/3, 0x147697b40 = 2/15) shared by the voice and shelf filters.
const HALF_K: f64 = 0.707_106_781_186_548_02;
const K: f64 = 1.414_213_562_373_096;
const TAN3: f64 = 1. / 3.;
const TAN5: f64 = 0.133_333_333_333_333_33;

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    /// Feedback gain `g ^ (G0[v] / G0[0])` (0x14193a610).
    feedback: f64,
    /// Rotation `[a, b, c, d]`: L += sL*a + sR*b, R += sL*c + sR*d.
    rotation: [f64; 4],
    gain: f64,
    /// Modulation depth (frames) and base delay (frames).
    depth: f64,
    base: f64,
    increment: [f64; 2],
    offset: f64,
    /// G4 / G3 when LFO2 runs (G4 > 1e-8), else `None`.
    lfo2: Option<f64>,
    shape: f64,
    smoothing: f64,
    /// Voice SVF `g`, `h` and the low / high blend.
    svf: [f64; 2],
    blend: [f64; 2],
    ring: usize,
    at: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Choral {
    voices: [Voice; VOICES],
    /// Circulant row `c[(k - j) mod 7]` (0x14193a620; product 0x141939270).
    scatter: [f64; VOICES],
    invert: f64,
    input: f64,
    wet: f64,
    dry: f64,
    /// Shelf `g`, `h`, `S - 1`, `k (sqrt S - 1)` (0x14193ac00).
    shelf: [f64; 4],
    state: usize,
    frames: usize,
}

/// One-sample triangle 1 - |2 - 4p| (0x141939440, constants 0x144714998, 0x1447149a8).
#[inline(always)]
fn triangle(p: f64) -> f64 {
    1. - (2. - p * 4.).abs()
}

/// The polynomial sine on the triangle (0x1419393d0; 0x147697aa8, 0x147697a90, 0x147697a70).
#[inline(always)]
fn rounded(p: f64) -> f64 {
    let t = triangle(p);
    let t2 = t * t;
    (t * 1.570_796_326_794_900_1 - t * 0.641_592_653_589_793 * t2) + t2 * t2 * t * 0.070_796_326_794_896_6
}

/// The LFO tick (0x14193bd90): value at `offset + phase` wrapped once, then the
/// phase advances by the increment and wraps into [0, 1].
#[inline(always)]
fn tick(phase: &mut f64, offset: f64, increment: f64, shape: fn(f64) -> f64) -> f64 {
    let mut p = offset + *phase;
    if p >= 1. {
        p += -1.;
    }
    let value = shape(p);
    let mut next = increment + *phase;
    if next > 1. {
        next += -1.;
    } else if next < 0. {
        next += 1.;
    }
    *phase = next;
    value
}

impl Choral {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        [amount, rate, delay, voices, width, feedback, mix]: [f32; 7],
        invert: bool,
        scatter: bool,
        mode: u8,
        fs: f64,
    ) -> Self {
        let table = MODES[usize::from(mode.min(3))].map(|bits| f64::from(f32::from_bits(bits)));
        let group = |g: usize, v: usize| table[g * 7 + v];
        let tick = 1. / fs;
        let omega = PI / fs;
        // Amount 0x14193a2f0: d = (a*a f32) * 0.99 + 0.01, R = 2^(d * 0.025).
        let d = f64::from(amount * amount) * 0.99 + 0.01;
        let ratio = 2f64.powf(d * 0.025);
        // Rate 0x14193b140: r^3 * 7.4 + 0.25 Hz.
        let r = f64::from(rate);
        let speed = r * r * r * 7.4 + 0.25;
        // Delay 0x14193ac00: seconds = G0 * (D * 5 + 0.1).
        let span = f64::from(delay) * 5. + 0.1;
        // Feedback 0x14193a610: g = fb^(1/4) * 0.999; input gain (1 - min(g, .99)^4)^(1/4).
        let g = f64::from(feedback).sqrt().sqrt() * 0.999;
        let clamped = if 0.99 <= g { 0.99 } else { g };
        let c2 = clamped * clamped;
        let input = (1. - c2 * c2).sqrt().sqrt();
        // Voices 0x14193a3e0: pairs 2/3 and 4/5 fade in over the two halves.
        let fade = |k: f64| (f64::from(voices) + f64::from(voices) - k).clamp(0., 1.).sqrt();
        let gains = [1., 1., fade(0.), fade(0.), fade(1.), fade(1.), 0.];
        let sum = 2. + fade(0.) * 2. + fade(1.) * 2.;
        let norm = (sum * 0.2 + 0.6).sqrt() * (1. / sum);
        // Width 0x14193a4a0: pair k turns by -/+ w * pi/4 * (1 - 2k/7); voice 6 stays put.
        let (left_turn, right_turn) = (f64::from(width) * -std::f64::consts::FRAC_PI_4, f64::from(width) * std::f64::consts::FRAC_PI_4);
        let turn = |angle: f64| {
            let (s, c) = angle.sin_cos();
            [c, -s, s, c]
        };
        let mut lines = [Voice::default(); VOICES];
        let mut at = 0;
        let base0 = group(0, 0) * span;
        for (v, voice) in lines.iter_mut().enumerate() {
            let pair = (v / 2 * 2) as f64;
            let reach = 1. - pair * 0.142_857_142_857_142_85;
            voice.rotation = match v {
                6 => [1., -0., 0., 1.],
                _ if v % 2 == 0 => turn(left_turn * reach),
                _ => turn(right_turn * reach),
            };
            voice.gain = gains[v];
            voice.feedback = if v == 0 { g } else { g.powf(group(0, v) * (1. / group(0, 0))) };
            let rate_v = group(1, v) * speed;
            voice.increment = [tick * rate_v, tick * (rate_v * group(3, v))];
            voice.depth = (fs / rate_v) * 0.5 * (ratio - 1.);
            voice.base = group(0, v) * span * fs;
            voice.offset = group(2, v);
            voice.lfo2 = (1e-8 < group(4, v)).then(|| group(4, v) / group(3, v));
            voice.shape = group(7, v);
            voice.smoothing = group(8, v);
            // Voice SVF (0x14193e160): tan series of G5 * pi / fs, k = sqrt 2.
            let w = group(5, v) * omega;
            let w2 = w * w;
            let gv = (TAN3 * w * w2 + w) + TAN5 * w * w2 * w2;
            voice.svf = [gv, 1. / (((HALF_K * gv + HALF_K * gv) + 1.) + gv * gv)];
            voice.blend = [1. - group(6, v), group(6, v)];
            // Ring: the longest read (m <= 1 + 1.5 G4/G3) plus the four-point span.
            let reach = voice.depth * (1. + 1.5 * voice.lfo2.unwrap_or(0.)) + voice.base;
            voice.ring = reach.ceil() as usize + 4;
            voice.at = at;
            at += voice.ring;
        }
        // Shelf 0x14193ac00: S = max(1 - g, 0.2), fc = min(0.25 / T0, 50) Hz warped by S^(1/4).
        let s = if 1. - g <= 0.2 { 0.2 } else { 1. - g };
        let fc = 0.25 / base0;
        let fc = if 50. <= fc { 50. } else { fc };
        let w = fc / s.sqrt().sqrt() * omega;
        let w2 = w * w;
        let gs = (TAN3 * w * w2 + w) + TAN5 * w * (w2 * w2);
        let shelf = [gs, 1. / (((HALF_K * gs + gs * HALF_K) + 1.) + gs * gs), s - 1., (s.sqrt() - 1.) * (HALF_K + HALF_K)];
        let sc = if scatter { 1. } else { 0. };
        let (a, b) = (sc * -0.261_203_874_963_740_98, sc * 0.445_902_906_222_805_98);
        let mix = f64::from(mix);
        Self {
            voices: lines,
            scatter: [1. - sc * 0.554_097_093_777_194_07, a, a, b, a, b, b],
            invert: 1. - f64::from(if invert { 2f32 } else { 0. }),
            input,
            // Output 0x14193be50: mix * input * norm * 0.707 (0x147697a98).
            wet: mix * input * norm * 0.707,
            dry: 1. - mix,
            shelf,
            state: at,
            frames: at + VOICES * VOICE_STATE + 2,
        }
    }

    pub(crate) fn frames(&self) -> usize {
        self.frames
    }

    /// Process `len` frames; returns whether a nonfinite value appeared.
    pub(crate) fn process(&self, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, [dry, wet]: [f64; 2]) -> bool {
        let st = self.state;
        let shelf_at = st + VOICES * VOICE_STATE;
        let [gs, hs, m2, m1] = self.shelf;
        let mut finite = true;
        let [left, right] = block;
        for i in 0..len {
            let x = [f64::from(left[i] as f32), f64::from(right[i] as f32)];
            let y: [[f64; 2]; VOICES] = std::array::from_fn(|v| slab[st + v * VOICE_STATE + 2]);
            // Voice inputs and the rotated sum of the previous outputs.
            let mut inputs = [[0.; 2]; VOICES];
            let mut sum = [0.; 2];
            for (j, voice) in self.voices.iter().enumerate() {
                let mut mixed = [0.; 2];
                for (k, yk) in y.iter().enumerate() {
                    let c = self.scatter[(k + VOICES - j) % VOICES];
                    mixed = [yk[0] * c + mixed[0], yk[1] * c + mixed[1]];
                }
                let gain = self.invert * voice.feedback;
                let u = [gain * mixed[0] + self.input * x[0], gain * mixed[1] + self.input * x[1]];
                inputs[j] = u;
                let s = [self.invert * y[j][0] + u[0], self.invert * y[j][1] + u[1]];
                let [a, b, c, d] = voice.rotation;
                sum[0] += (s[0] * a + s[1] * b) * voice.gain;
                sum[1] += (s[0] * c + s[1] * d) * voice.gain;
            }
            // Low shelf on the sum.
            let (mut band, mut low) = ([0.; 2], [0.; 2]);
            for c in 0..2 {
                let (s1, s2) = (slab[shelf_at][c], slab[shelf_at + 1][c]);
                let bp = ((sum[c] - s2) * gs + s1) * hs;
                let next = s2 + (bp + bp) * gs;
                slab[shelf_at][c] = (bp + bp) - s1;
                slab[shelf_at + 1][c] = next;
                band[c] = bp;
                low[c] = (s2 + next) * 0.5 * m2;
            }
            for (v, voice) in self.voices.iter().enumerate() {
                let at = st + v * VOICE_STATE;
                let [g, h] = voice.svf;
                let w = slab[at + 3][1] as usize;
                let ring = voice.at;
                for c in 0..2 {
                    let (s1, s2, u) = (slab[at][c], slab[at + 1][c], inputs[v][c]);
                    let bp = (s1 + g * (u - s2)) * h;
                    let next = s2 + (bp + bp) * g;
                    slab[at][c] = (bp + bp) - s1;
                    slab[at + 1][c] = next;
                    let lp = (s2 + next) * 0.5;
                    let hp = (u - bp * K) - lp;
                    slab[ring + w][c] = voice.blend[0] * lp + voice.blend[1] * hp;
                }
                let [mut p1, mut p2] = slab[at + 4];
                let l1 = tick(&mut p1, voice.offset, voice.increment[0], triangle);
                let l2 = match voice.lfo2 {
                    Some(scale) => (tick(&mut p2, 0., voice.increment[1], rounded) * 0.5 + 1.) * scale,
                    None => 0.,
                };
                slab[at + 4] = [p1, p2];
                let held = slab[at + 3][0];
                let m = ((2. - (1. - l1) * voice.shape) * (l1 + 1.) * 0.25 + l2) * (1. - voice.smoothing) + held * voice.smoothing;
                let d = voice.depth * m + voice.base;
                let size = voice.ring;
                // The ring is sized for the longest read; the clamp only guards a faulted state.
                let n = (d as usize).clamp(2, size - 3);
                let t = d - d.trunc();
                let t2 = t * t;
                // `n` behind the next write is `n - 1` behind the sample just written.
                let wrap = |k: usize| ring + if k >= size { k - size } else { k };
                let (i1, i0) = (wrap(w + 1 + size - n), wrap(w + 2 + size - n));
                let (i2, i3) = (wrap(w + size - n), wrap(w + size - 1 - n));
                let out: [f64; 2] = std::array::from_fn(|c| {
                    let (p0, p1, p2, p3) = (slab[i0][c], slab[i1][c], slab[i2][c], slab[i3][c]);
                    ((((p0 - p1) - p1) + p2 + p2) - 0.5 * (p1 + p3)) * t2 + t * (p2 - p0) * 0.5 + p1 + ((((p1 + p3) - p0) - p2) * 0.5 + (p1 - p2)) * t2 * t
                });
                slab[at + 2] = out;
                slab[at + 3] = [m, ((w + 1) % size) as f64];
            }
            let out = [
                ((sum[0] + low[0] + band[0] * m1) * self.wet + self.dry * x[0]) as f32,
                ((sum[1] + low[1] + band[1] * m1) * self.wet + self.dry * x[1]) as f32,
            ];
            finite &= out[0].is_finite() && out[1].is_finite();
            left[i] = dry * x[0] + wet * f64::from(out[0]);
            right[i] = dry * x[1] + wet * f64::from(out[1]);
        }
        for frame in &mut slab[st..self.frames] {
            *frame = frame.map(flush);
        }
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::BLOCK;
    use super::*;

    fn law(fields: [f32; 7], mode: u8) -> Choral {
        Choral::new(fields, false, false, mode, 48_000.)
    }

    #[test]
    fn parameter_laws_match_native_vectors() {
        // Delay 0.18, mode 0: B0 = 0.003f * (0.18f * 5 + 0.1) * 48000 (0x14193ac00).
        let c = law([1., 0.5, 0.18, 0., 0., 0.5, 1.], 0);
        assert!((c.voices[0].base - 144.000_006_4).abs() < 1e-6, "{}", c.voices[0].base);
        // Rate 0.5 -> 0.125 * 7.4 + 0.25 = 1.175 Hz (0x14193b140); amount 1 -> 2^0.025 (0x14193a2f0).
        assert!((c.voices[0].increment[0] * 48_000. - 1.175).abs() < 1e-12);
        assert!((c.voices[0].depth - 357.032).abs() < 1e-3, "{}", c.voices[0].depth);
        // Feedback 0.5 (0x14193a610): g = 0.5^(1/4) * 0.999, input (1 - g^4)^(1/4).
        assert!((c.voices[0].feedback - 0.840_055_518_8).abs() < 1e-9);
        assert!((c.input - 0.841_734_796_5).abs() < 1e-9);
        // Voices (0x14193a3e0): 0 -> sqrt(1) / 2, 1 -> sqrt(1.8) / 6.
        assert_eq!(c.wet, 0.5 * c.input * 0.707);
        let full = law([0., 0., 0., 1., 0., 0., 1.], 0);
        assert!((full.wet - 1.8f64.sqrt() / 6. * 0.707).abs() < 1e-15);
        assert_eq!(full.voices.map(|v| v.gain), [1., 1., 1., 1., 1., 1., 0.]);
        // Mode 1 voice 2: LFO2 depth G4 / G3 = 1 / 4, phase offset 0.33333f.
        let m1 = law([0.; 7], 1);
        assert_eq!(m1.voices[2].lfo2, Some(0.25));
        assert_eq!(m1.voices[2].offset, f64::from(f32::from_bits(0x3eaaaa3b)));
    }

    fn render(c: &Choral, input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let mut slab = vec![[0.; 2]; c.frames()];
        let mut out = Vec::new();
        for chunk in input.chunks(BLOCK) {
            let mut block: Planar = [[0.; BLOCK]; 2];
            for (i, f) in chunk.iter().enumerate() {
                (block[0][i], block[1][i]) = (f[0], f[1]);
            }
            assert!(!c.process(&mut slab, &mut block, chunk.len(), [0., 1.]));
            out.extend((0..chunk.len()).map(|i| [block[0][i], block[1][i]]));
        }
        out
    }

    #[test]
    fn impulse_passes_direct_then_two_filtered_voices() {
        // Feedback 0 -> input gain 1, flat shelf; voices 0 -> two voices, norm 0.5.
        // Frame 0: W = s0 + s1 = 2x, out = 2 * 0.5 * 0.707 (0x14193be50).
        let c = law([0., 0.5, 0.18, 0., 0., 0., 1.], 0);
        let mut x = vec![[0.; 2]; 2048];
        x[0] = [1., -1.];
        let y = render(&c, &x);
        assert_eq!(y[0], [f64::from(0.707f32), f64::from(-0.707f32)]);
        // The voices read ~144 frames back (A ~ 3.5 frames at amount 0).
        assert!(y[1..138].iter().all(|f| *f == [0.; 2]), "{:?}", &y[1..8]);
        // Each voice's low-pass passes DC: the delayed tail sums to 2 * 0.5 * 0.707.
        let tail: f64 = y[1..].iter().map(|f| f[0]).sum();
        assert!((tail - 0.707).abs() < 2e-3, "{tail}");
        assert!((y[1..].iter().map(|f| f[1]).sum::<f64>() + tail).abs() < 1e-6);
    }

    #[test]
    fn mix_zero_is_the_f32_input() {
        let c = law([0.7, 0.3, 0.5, 1., 1., 0.8, 0.], 2);
        let x: Vec<[f64; 2]> = (0..600).map(|i| [((i as f64) * 0.1).sin(), ((i as f64) * 0.07).cos()]).collect();
        let y = render(&c, &x);
        assert!(y.iter().zip(&x).all(|(a, b)| *a == [f64::from(b[0] as f32), f64::from(b[1] as f32)]));
    }

    #[test]
    fn every_mode_stays_bounded_with_full_feedback_and_scatter() {
        for mode in 0..4 {
            let c = Choral::new([1.; 7], true, true, mode, 44_100.);
            let x: Vec<[f64; 2]> = (0..44_100).map(|i| [((i as f64) * 0.05).sin(), ((i as f64) * 0.031).sin()]).collect();
            let peak = render(&c, &x).iter().flatten().fold(0f64, |m, v| m.max(v.abs()));
            assert!(peak.is_finite() && peak < 8., "mode {mode}: {peak}");
        }
    }

    /// Kontakt 8.13.1 under tools/re/kemu.py: native ctor 0x141939b10,
    /// prepare 0x14193b840, setters, process 0x14193be50 in 32-frame blocks on
    /// `0.5 sin(0.05 n) + impulse` / `0.5 sin(0.031 n)` (f32). Output f32 bits.
    #[test]
    fn output_matches_native_kemu_vectors() {
        let x: Vec<[f64; 2]> = (0..3000)
            .map(|i| {
                let n = i as f64;
                [f64::from(((n * 0.05).sin() * 0.5 + if i == 0 { 1. } else { 0. }) as f32), f64::from(((n * 0.031).sin() * 0.5) as f32)]
            })
            .collect();
        let cases: [(([f32; 7], bool, bool, u8, f64), [(usize, u32, u32); 6]); 2] = [
            (
                ([0.6, 0.5, 0.18, 0.7, 0.4, 0.5, 0.8], true, true, 1, 48_000.),
                [(0, 0x3f32c0a0, 0), (1, 0x3c7145ad, 0x3c314b45), (150, 0x3ea49034, 0xbebce4dd), (777, 0x3e88e9e0, 0xbe871bd9), (1500, 0xbe883379, 0x3e1418bb), (2999, 0xbdc85a44, 0xbea4ea2d)],
            ),
            (
                ([0.3, 0.8, 0.5, 1.0, 1.0, 0.9, 1.0], false, true, 2, 44_100.),
                [(0, 0x3e820bfa, 0), (1, 0x3bc3deeb, 0x3b80fc5a), (150, 0x3def5bf1, 0xbe077d0a), (777, 0x3dedf5b4, 0xbddeca1a), (1500, 0xbe2cb4be, 0x3e870217), (2999, 0xbefab222, 0xbe207c4d)],
            ),
        ];
        for ((fields, invert, scatter, mode, fs), want) in cases {
            let y = render(&Choral::new(fields, invert, scatter, mode, fs), &x);
            for (i, l, r) in want {
                let got = [y[i][0] as f32, y[i][1] as f32];
                let ulps = |g: f32, w: u32| (g.to_bits() as i64 - i64::from(w)).abs();
                assert!(ulps(got[0], l) <= 1 && ulps(got[1], r) <= 1, "mode {mode} frame {i}: {got:?} vs {:?}", [f32::from_bits(l), f32::from_bits(r)]);
            }
        }
    }
}
