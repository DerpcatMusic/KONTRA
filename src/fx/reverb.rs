//! Algorithmic reverb for Kontakt's modern "Reverb" (`BParFXGaloisReverb`).
//!
//! An 8-line feedback delay network with a Hadamard mix, per-line damping and
//! slow delay modulation. Kontakt's algorithm is not public; this reproduces
//! the controls, not the exact sound. Normalized parameter mappings are
//! documented (with confidence) in `audits/EFFECTS.md`.

use super::params;
use std::f32::consts::TAU;

const LINES: usize = 8;
/// Mutually prime-ish base lengths in ms (Hall scale), spread over ~1 octave.
const BASE_MS: [f32; LINES] = [31.7, 37.1, 41.3, 43.9, 53.3, 59.9, 67.1, 73.7];
const MAX_PREDELAY_MS: f32 = 250.0;
const ALLPASS_MS: [f32; 2] = [4.77, 3.59];
/// Tiny DC bias keeping decaying feedback out of subnormal floats (slow on x86).
const ANTI_DENORMAL: f32 = 1e-20;

/// Power-of-two ring buffer; `mask` wraps indices without branches.
struct Line {
    buf: Vec<f32>,
    mask: usize,
}

impl Line {
    fn new(min_len: usize) -> Self {
        let len = (min_len + 2).next_power_of_two();
        Self {
            buf: vec![0.0; len],
            mask: len - 1,
        }
    }

    #[inline]
    fn read(&self, pos: usize, delay: usize) -> f32 {
        self.buf[pos.wrapping_sub(delay) & self.mask]
    }

    /// Linear-interpolated read at a fractional delay.
    #[inline]
    fn read_frac(&self, pos: usize, delay: f32) -> f32 {
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let a = self.read(pos, whole);
        let b = self.read(pos, whole + 1);
        a + (b - a) * frac
    }

    #[inline]
    fn write(&mut self, pos: usize, value: f32) {
        self.buf[pos & self.mask] = value;
    }
}

pub struct Reverb {
    pos: usize,
    predelay: Line,
    predelay_len: usize,
    /// Input diffusion allpasses, `[channel][stage]`.
    allpass: [[Line; 2]; 2],
    allpass_len: [usize; 2],
    diffusion: f32,
    lines: [Line; LINES],
    delay: [f32; LINES],
    feedback: [f32; LINES],
    damp_coef: f32,
    damp_state: [f32; LINES],
    mod_depth: f32,
    lfo_phase: f32,
    lfo_step: f32,
    input_coef: f32,
    input_state: [f32; 2],
    shelf_coef: f32,
    shelf_gain: f32,
    shelf_state: [f32; 2],
    width: f32,
}

/// One-pole lowpass coefficient for cutoff `hz`.
fn one_pole(hz: f32, sample_rate: f32) -> f32 {
    1.0 - (-TAU * hz.min(sample_rate * 0.49) / sample_rate).exp()
}

impl Reverb {
    pub fn new(p: &params::Reverb, sample_rate: f32) -> Self {
        let n = |v: f32| v.clamp(0.0, 1.0);
        let ms = |v: f32| v * 0.001 * sample_rate;
        let hall = p.room_type >= 0.5;
        let scale = (0.5 + n(p.size)) * if hall { 1.0 } else { 0.55 };
        let rt60 = 0.2 * 100f32.powf(n(p.time));
        let mod_depth = ms(n(p.modulation) * 1.5);

        let delay = BASE_MS.map(|base| ms(base * scale));
        let lines = delay.map(|d| Line::new((d + mod_depth) as usize + 2));
        // Each pass through a line of length d must lose 60 dB over rt60.
        let feedback = delay.map(|d| 10f32.powf(-3.0 * d / (rt60 * sample_rate)));
        let predelay_len = ms(n(p.predelay) * MAX_PREDELAY_MS) as usize;
        let allpass_len = ALLPASS_MS.map(|t| ms(t * scale).max(1.0) as usize);

        Self {
            pos: 0,
            predelay: Line::new(ms(MAX_PREDELAY_MS) as usize),
            predelay_len,
            allpass: std::array::from_fn(|_| allpass_len.map(Line::new)),
            allpass_len,
            diffusion: 0.75 * n(p.diffusion),
            lines,
            delay,
            feedback,
            damp_coef: one_pole(18_000.0 * 0.05f32.powf(n(p.damping)), sample_rate),
            damp_state: [0.0; LINES],
            mod_depth,
            lfo_phase: 0.0,
            lfo_step: TAU * 0.7 / sample_rate,
            input_coef: one_pole(20_000.0 * 0.025f32.powf(n(p.high_cut)), sample_rate),
            input_state: [0.0; 2],
            shelf_coef: one_pole(250.0, sample_rate),
            shelf_gain: 10f32.powf(-18.0 * n(p.low_shelf) / 20.0) - 1.0,
            shelf_state: [0.0; 2],
            width: n(p.stereo),
        }
    }

    /// Replaces `left`/`right` with the wet (100%) reverb signal.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let [wl, wr] = self.tick([*l, *r]);
            *l = wl;
            *r = wr;
        }
    }

    #[inline]
    fn tick(&mut self, input: [f32; 2]) -> [f32; 2] {
        let pos = self.pos;
        self.pos = pos.wrapping_add(1);

        // The input is summed to mono; the network decorrelates the outputs.
        self.predelay.write(pos, 0.5 * (input[0] + input[1]));
        let dry = self.predelay.read(pos, self.predelay_len);

        let mut diffused = [0.0; 2];
        for (ch, out) in diffused.iter_mut().enumerate() {
            let state = &mut self.input_state[ch];
            *state += (dry - *state) * self.input_coef + ANTI_DENORMAL;
            let mut x = *state;
            for (line, &len) in self.allpass[ch].iter_mut().zip(&self.allpass_len) {
                let delayed = line.read(pos, len);
                let v = x + self.diffusion * delayed;
                line.write(pos, v);
                x = delayed - self.diffusion * v;
            }
            *out = x;
        }

        // Quadrature LFOs on alternate lines decorrelate modes without pitch wobble.
        self.lfo_phase += self.lfo_step;
        if self.lfo_phase > TAU {
            self.lfo_phase -= TAU;
        }
        let (sin, cos) = self.lfo_phase.sin_cos();
        let mut taps = [0.0; LINES];
        for (i, tap) in taps.iter_mut().enumerate() {
            let lfo = match i % 4 {
                0 => sin,
                1 => cos,
                2 => -sin,
                _ => -cos,
            };
            let d = self.delay[i] + self.mod_depth * (0.5 + 0.5 * lfo);
            *tap = self.lines[i].read_frac(pos, d);
        }

        let mut mix = taps;
        hadamard(&mut mix);
        let lines = self.lines.iter_mut().zip(&mut self.damp_state);
        for (i, ((line, damped), (m, fb))) in lines.zip(mix.iter().zip(&self.feedback)).enumerate()
        {
            *damped += (m * fb - *damped) * self.damp_coef + ANTI_DENORMAL;
            line.write(pos, *damped + diffused[i % 2]);
        }

        let l = (taps[0] - taps[2] + taps[4] - taps[6]) * 0.5;
        let r = (taps[1] - taps[3] + taps[5] - taps[7]) * 0.5;
        let (mid, side) = (0.5 * (l + r), 0.5 * (l - r) * self.width);
        let mut out = [mid + side, mid - side];
        for (ch, v) in out.iter_mut().enumerate() {
            let low = &mut self.shelf_state[ch];
            *low += (*v - *low) * self.shelf_coef;
            *v += *low * self.shelf_gain;
        }
        out
    }
}

/// In-place orthonormal 8-point Walsh-Hadamard transform.
#[inline]
fn hadamard(x: &mut [f32; LINES]) {
    let mut h = 1;
    while h < LINES {
        for i in (0..LINES).step_by(2 * h) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
        }
        h *= 2;
    }
    let norm = (LINES as f32).sqrt().recip();
    for v in x {
        *v *= norm;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(time: f32) -> params::Reverb {
        params::Reverb {
            room_type: 0.0,
            time,
            size: 0.5,
            damping: 0.5,
            modulation: 0.5,
            diffusion: 0.5,
            predelay: 0.0,
            high_cut: 0.0,
            low_shelf: 0.0,
            stereo: 1.0,
        }
    }

    fn energy(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum()
    }

    #[test]
    fn impulse_decays_and_stays_finite() {
        let sr = 48_000.0;
        let mut rv = Reverb::new(&params(0.37), sr);
        let mut l = vec![0.0; 4 * 48_000];
        let mut r = l.clone();
        l[0] = 1.0;
        r[0] = 1.0;
        for (l, r) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            rv.process(l, r);
        }
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        let early = energy(&l[..24_000]);
        let late = energy(&l[3 * 48_000..]);
        assert!(early > 1e-3, "reverb produced no tail: {early}");
        // rt60 ~1.1 s: the fourth second must be far below the first half-second.
        assert!(late < early * 1e-4, "early {early} late {late}");
    }

    #[test]
    fn maximum_time_is_stable() {
        let mut rv = Reverb::new(&params(1.0), 44_100.0);
        let mut l = vec![0.5; 256];
        let mut r = vec![-0.5; 256];
        for _ in 0..2_000 {
            rv.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 100.0));
            l.fill(0.0);
            r.fill(0.0);
        }
    }

    #[test]
    fn hadamard_preserves_energy() {
        let mut x = [1.0, 0.0, 0.5, -2.0, 0.0, 0.25, 0.0, 1.0];
        let before = energy(&x);
        hadamard(&mut x);
        assert!((energy(&x) - before).abs() < 1e-5);
    }
}
