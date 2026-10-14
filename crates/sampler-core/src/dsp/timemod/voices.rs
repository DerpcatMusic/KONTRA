//! The line's voice bank ([`VoiceBank`]): seven modulated voices, each a
//! state-variable-filtered ring read with Catmull-Rom, recirculated through a
//! 7x7 circulant, rotated into the stereo field and low-shelved. Formats
//! configure it (Kontakt Choral 0x5d is one configuration). f64 law of the
//! Kontakt 8.13.1 core: audio 0x14193be50, prepare 0x14193b840, voice SVF
//! 0x14193e160, LFO tick 0x14193bd90; constants cited by address.
use super::super::{Planar, flush};
use sampler_ir::fx::timemod::{BANK_VOICES as VOICES, VoiceBank};
use std::f64::consts::PI;

/// Per voice after the rings: SVF s1, s2, last output, [smoothed m, write
/// position], [LFO1 phase, LFO2 phase]. Then the shelf's s1, s2.
const VOICE_STATE: usize = 5;

/// SVF damping halves (0x147697b60, 0x147697b70; the shelf copy at core +0x1c00
/// is stored from 0x147697b60 by the ctor at 0x141939c46) and the tan series
/// (0x147697b50 = 1/3, 0x147697b40 = 2/15) shared by the voice and shelf filters.
const HALF_K: f64 = 0.707_106_781_186_548_02;
const K: f64 = 1.414_213_562_373_096;
const TAN3: f64 = 1. / 3.;
const TAN5: f64 = 0.133_333_333_333_333_33;

/// Prewarped TPT SVF `g` (tan series of `hz * pi / fs`) and `h`. The voice
/// filters (0x14193e160) and the shelf (0x14193b840) group the fifth-order
/// term differently.
fn svf(hz: f64, omega: f64, shelf: bool) -> [f64; 2] {
    let w = hz * omega;
    let w2 = w * w;
    let fifth = if shelf { TAN5 * w * (w2 * w2) } else { TAN5 * w * w2 * w2 };
    let g = (TAN3 * w * w2 + w) + fifth;
    [g, 1. / (((HALF_K * g + g * HALF_K) + 1.) + g * g)]
}

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    feedback: f64,
    /// Rotation `[a, b, c, d]`: L += sL*a + sR*b, R += sL*c + sR*d.
    rotation: [f64; 4],
    gain: f64,
    /// Modulation depth (frames) and base delay (frames).
    depth: f64,
    base: f64,
    increment: [f64; 2],
    offset: f64,
    /// LFO2 scale when it runs, else `None`.
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
pub(crate) struct Bank {
    voices: [Voice; VOICES],
    /// Circulant row `c[(k - j) mod 7]` (product 0x141939270).
    scatter: [f64; VOICES],
    invert: f64,
    input: f64,
    wet: f64,
    dry: f64,
    /// Shelf `g`, `h`, `S - 1`, `k (sqrt S - 1)` (prepare 0x14193b840).
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

impl Bank {
    pub(crate) fn new(bank: &VoiceBank, fs: f64) -> Self {
        let tick = 1. / fs;
        let omega = PI / fs;
        let mut voices = [Voice::default(); VOICES];
        let mut at = 0;
        for (voice, v) in voices.iter_mut().zip(&bank.voices) {
            voice.rotation = v.rotation;
            voice.gain = v.gain;
            voice.feedback = v.feedback;
            voice.increment = [tick * v.lfo_hz, tick * (v.lfo_hz * v.lfo2_ratio)];
            voice.depth = (fs / v.lfo_hz) * v.excursion;
            voice.base = v.delay_s * fs;
            voice.offset = v.offset;
            voice.lfo2 = (v.lfo2 != 0.).then_some(v.lfo2);
            voice.shape = v.shape;
            voice.smoothing = v.smoothing;
            voice.svf = svf(v.filter_hz, omega, false);
            voice.blend = [1. - v.highpass, v.highpass];
            // Ring: the longest read (m <= 1 + 1.5 * lfo2) plus the four-point span.
            let reach = voice.depth * (1. + 1.5 * voice.lfo2.unwrap_or(0.)) + voice.base;
            voice.ring = reach.ceil() as usize + 4;
            voice.at = at;
            at += voice.ring;
        }
        let [g, h] = svf(bank.shelf_hz, omega, true);
        let s = bank.shelf_gain;
        Self {
            voices,
            scatter: bank.scatter,
            invert: bank.invert,
            input: bank.input,
            wet: bank.wet,
            dry: bank.dry,
            shelf: [g, h, s - 1., (s.sqrt() - 1.) * (HALF_K + HALF_K)],
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
    use sampler_ir::fx::timemod::BankVoice;

    /// Seven unrotated voices at 3 ms on a 1 Hz LFO, low-passed at 6 kHz.
    fn bank(gains: [f64; VOICES], feedback: f64, scatter: f64, mix: f64) -> VoiceBank {
        let v = BankVoice { feedback, rotation: [1., 0., 0., 1.], delay_s: 0.003, excursion: 0.01, lfo_hz: 1., lfo2_ratio: 1., filter_hz: 6000., ..BankVoice::default() };
        VoiceBank {
            voices: std::array::from_fn(|k| BankVoice { gain: gains[k], offset: k as f64 / 7., ..v }),
            scatter: [1. - scatter * 0.554, -0.26 * scatter, -0.26 * scatter, 0.45 * scatter, -0.26 * scatter, 0.45 * scatter, 0.45 * scatter],
            invert: 1.,
            input: 1.,
            wet: mix,
            dry: 1. - mix,
            shelf_hz: 50.,
            shelf_gain: 1.,
        }
    }

    fn render(c: &Bank, input: &[[f64; 2]]) -> Vec<[f64; 2]> {
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
    fn impulse_passes_direct_then_the_delayed_filtered_voices() {
        // Frame 0: W = sum of gains * x (the voices' direct inputs), out = W * wet.
        let c = Bank::new(&bank([1., 1., 0., 0., 0., 0., 0.], 0., 0., 1.), 48_000.);
        let mut x = vec![[0.; 2]; 2048];
        x[0] = [1., -1.];
        let y = render(&c, &x);
        assert_eq!(y[0], [2., -2.]);
        // The voices read ~144 frames back (3 ms at 48 kHz, excursion ~ 4.8 frames).
        assert!(y[1..130].iter().all(|f| *f == [0.; 2]), "{:?}", &y[1..8]);
        // Each voice's low-pass passes DC: the delayed tail sums to about the two
        // gains (the moving read stretches it a little).
        let tail: f64 = y[1..].iter().map(|f| f[0]).sum();
        assert!((tail - 2.).abs() < 0.1, "{tail}");
    }

    #[test]
    fn dry_only_is_the_f32_input() {
        let c = Bank::new(&bank([1.; VOICES], 0.8, 1., 0.), 44_100.);
        let x: Vec<[f64; 2]> = (0..600).map(|i| [((i as f64) * 0.1).sin(), ((i as f64) * 0.07).cos()]).collect();
        let y = render(&c, &x);
        assert!(y.iter().zip(&x).all(|(a, b)| *a == [f64::from(b[0] as f32), f64::from(b[1] as f32)]));
    }

    #[test]
    fn full_feedback_and_scatter_stay_bounded() {
        let c = Bank::new(&bank([1.; VOICES], 0.999, 1., 0.1), 44_100.);
        let x: Vec<[f64; 2]> = (0..44_100).map(|i| [((i as f64) * 0.05).sin(), ((i as f64) * 0.031).sin()]).collect();
        let peak = render(&c, &x).iter().flatten().fold(0f64, |m, v| m.max(v.abs()));
        assert!(peak.is_finite() && peak < 50., "{peak}");
    }
}
