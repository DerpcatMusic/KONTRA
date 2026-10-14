#![allow(dead_code)]
// The eight-line feedback delay network: a Hadamard-coupled bank of
// modulated delay lines with per-line damping, input diffusion, optional
// three-band decay. The general hall/room of [`super::Space`].

use super::common::{Line, one_pole};
use crate::Error;
use super::common::{biased_one_pole32, one_pole32};
use sampler_ir::fx::space::Network;
use std::f32::consts::TAU;

const LINES: usize = 8;
/// Mutually prime-ish base lengths in ms at size 1, spread over about an octave.
const BASE_MS: [f32; LINES] = [31.7, 37.1, 41.3, 43.9, 53.3, 59.9, 67.1, 73.7];
const ALLPASS_MS: [f32; 2] = [4.77, 3.59];
/// Frames between exact LFO values; between them the LFO is interpolated.
const LFO_STEP: usize = 16;

/// Three-band decay: per line, two first-order shelves around the damped
/// loop bring the low and high bands to their own decay times.
struct Bands {
    low_coef: f32,
    high_coef: f32,
    /// Shelf gain minus one, per line.
    low_gain: [f32; LINES],
    high_gain: [f32; LINES],
    low_state: [f32; LINES],
    high_state: [f32; LINES],
}

/// One FDN's state. All buffers are allocated by [`Fdn::new`].
pub(crate) struct Fdn {
    pos: usize,
    predelay: Line,
    predelay_len: usize,
    echo: f32,
    allpass: [[Line; 2]; 2],
    allpass_len: [usize; 2],
    diffusion: f32,
    /// One frame of all lines per entry; the length is a power of two.
    lines: Box<[[f32; LINES]]>,
    delay: [f32; LINES],
    feedback: [f32; LINES],
    damp_coef: f32,
    damp_state: [f32; LINES],
    bands: Option<Box<Bands>>,
    mod_depth: f32,
    lfo_phase: f32,
    lfo_step: f32,
    lfo: [f32; 2],
    input_coef: f32,
    input_state: [f32; 2],
}

impl Fdn {
    pub(crate) fn new(s: &Network, rate: u32) -> Result<Self, Error> {
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let rate = rate as f32;
        let ms = |v: f32| v * 0.001 * rate;
        let size = s.size as f32;
        let longest = ms(BASE_MS[LINES - 1] * size) + (s.modulation_seconds as f32) * rate;
        let allpass_len = ALLPASS_MS.map(|t| ms(t * size).max(1.0) as usize);
        let rt60 = s.decay_seconds as f32;
        let delay = BASE_MS.map(|base| ms(base * size));
        // Each pass through a line of length d loses 60 dB over rt60.
        let loss = |d: f32, rt60: f32| 10f32.powf(-3.0 * d / (rt60 * rate));
        let bands = s.bands.map(|b| {
            let relative = |ratio: f64| delay.map(|d| loss(d, rt60 * ratio as f32) / loss(d, rt60) - 1.0);
            Box::new(Bands {
                low_coef: one_pole(b.low_hz as f32, rate),
                high_coef: one_pole(b.high_hz as f32, rate),
                low_gain: relative(b.low),
                high_gain: relative(b.high),
                low_state: [0.0; LINES],
                high_state: [0.0; LINES],
            })
        });
        let predelay_len = (s.predelay_seconds as f32 * rate) as usize;
        Ok(Self {
            pos: 0,
            predelay: Line::new(predelay_len + 1),
            predelay_len,
            echo: if predelay_len > 0 { s.echo_feedback as f32 } else { 0.0 },
            allpass: std::array::from_fn(|_| allpass_len.map(|n| Line::new(n + 1))),
            allpass_len,
            diffusion: s.diffusion as f32,
            lines: vec![[0.0; LINES]; (longest as usize + 4).next_power_of_two()].into(),
            delay,
            feedback: delay.map(|d| loss(d, rt60)),
            damp_coef: one_pole(s.damping_hz as f32, rate),
            damp_state: [0.0; LINES],
            bands,
            mod_depth: s.modulation_seconds as f32 * rate,
            lfo_phase: 0.0,
            lfo_step: TAU * s.modulation_hz as f32 / rate,
            lfo: [0.0, 1.0],
            input_coef: one_pole(s.input_cutoff_hz as f32, rate),
            input_state: [0.0; 2],
        })
    }

    /// Silence the network.
    pub(crate) fn clear(&mut self) {
        for line in self.allpass.iter_mut().flatten().chain([&mut self.predelay]) {
            line.clear();
        }
        self.lines.fill([0.0; LINES]);
        self.damp_state = [0.0; LINES];
        self.input_state = [0.0; 2];
        if let Some(b) = &mut self.bands {
            b.low_state = [0.0; LINES];
            b.high_state = [0.0; LINES];
        }
    }

    /// Replace `left`/`right` with the untoned wet signal.
    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        sampler_simd::dispatch(
            #[inline(always)]
            || self.run(left, right),
        );
    }

    #[inline(always)]
    fn run(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.chunks_mut(LFO_STEP).zip(right.chunks_mut(LFO_STEP)) {
            let from = self.lfo;
            for _ in 0..left.len() {
                self.lfo_phase += self.lfo_step;
                if self.lfo_phase > TAU {
                    self.lfo_phase -= TAU;
                }
            }
            let (sin, cos) = self.lfo_phase.sin_cos();
            self.lfo = [sin, cos];
            let n = left.len() as f32;
            let slope = [(sin - from[0]) / n, (cos - from[1]) / n];
            for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                let t = (i + 1) as f32;
                let lfo = [from[0] + slope[0] * t, from[1] + slope[1] * t];
                [*l, *r] = self.tick([*l, *r], lfo);
            }
        }
    }

    #[inline(always)]
    fn tick(&mut self, input: [f32; 2], [sin, cos]: [f32; 2]) -> [f32; 2] {
        let pos = self.pos;
        self.pos = pos.wrapping_add(1);
        // The input sums to mono; the network decorrelates the outputs.
        let mut mono = 0.5 * (input[0] + input[1]);
        if self.echo != 0.0 {
            mono += self.echo * self.predelay.read(pos, self.predelay_len);
        }
        self.predelay.write(pos, mono);
        let dry = self.predelay.read(pos, self.predelay_len);
        let mut diffused = [0.0; 2];
        for (ch, out) in diffused.iter_mut().enumerate() {
            let state = &mut self.input_state[ch];
            *state = biased_one_pole32(*state, (dry - *state) * self.input_coef);
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
        let lfo = [sin, cos, -sin, -cos, sin, cos, -sin, -cos];
        let mask = self.lines.len() - 1;
        let taps: [f32; LINES] = std::array::from_fn(|i| {
            let d = self.delay[i] + self.mod_depth * (0.5 + 0.5 * lfo[i]);
            let whole = d as usize;
            let frac = d - whole as f32;
            let a = self.lines[pos.wrapping_sub(whole) & mask][i];
            let b = self.lines[pos.wrapping_sub(whole + 1) & mask][i];
            a + (b - a) * frac
        });
        let mut mix = taps;
        hadamard(&mut mix);
        let row = &mut self.lines[pos & mask];
        for (i, (y, damped)) in row.iter_mut().zip(&mut self.damp_state).enumerate() {
            *damped = biased_one_pole32(
                *damped,
                (mix[i] * self.feedback[i] - *damped) * self.damp_coef,
            );
            let mut out = *damped;
            if let Some(b) = &mut self.bands {
                b.low_state[i] = one_pole32(b.low_state[i], out, b.low_coef);
                out += b.low_state[i] * b.low_gain[i];
                b.high_state[i] = one_pole32(b.high_state[i], out, b.high_coef);
                out += (out - b.high_state[i]) * b.high_gain[i];
            }
            *y = out + diffused[i % 2];
        }
        [
            (taps[0] - taps[2] + taps[4] - taps[6]) * 0.5,
            (taps[1] - taps[3] + taps[5] - taps[7]) * 0.5,
        ]
    }
}

/// In-place orthonormal 8-point Walsh-Hadamard transform.
#[inline(always)]
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

