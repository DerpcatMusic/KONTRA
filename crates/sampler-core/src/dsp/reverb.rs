//! Algorithmic stereo reverb: an 8-line feedback delay network with a
//! Hadamard mix, per-line damping, slow quadrature delay modulation, input
//! diffusion and an optional low shelf. Import profiles own the mapping from
//! their vendor controls to these physical values.

use crate::Error;
use std::f32::consts::TAU;

const LINES: usize = 8;
/// Mutually prime-ish base lengths in ms at size 1, spread over about an octave.
const BASE_MS: [f32; LINES] = [31.7, 37.1, 41.3, 43.9, 53.3, 59.9, 67.1, 73.7];
const ALLPASS_MS: [f32; 2] = [4.77, 3.59];
/// Longest predelay and decay accepted.
pub const MAX_PREDELAY_SECONDS: f64 = 0.25;
pub const MAX_DECAY_SECONDS: f64 = 60.0;
/// Largest `size` (base lengths scale by it) and modulation depth accepted.
pub const MAX_SIZE: f64 = 1.5;
pub const MAX_MODULATION_SECONDS: f64 = 0.0015;
/// Frames between exact LFO values; between them the LFO is interpolated.
const LFO_STEP: usize = 16;

/// Physical reverb settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbSettings {
    /// Time for the tail to fall 60 dB, in seconds.
    pub decay_seconds: f64,
    /// Scale of the line lengths: room size. In (0, `MAX_SIZE`].
    pub size: f64,
    /// Feedback lowpass, Hz.
    pub damping_hz: f64,
    /// Peak delay-time modulation, seconds.
    pub modulation_seconds: f64,
    /// Input allpass coefficient in [0, 0.75].
    pub diffusion: f64,
    pub predelay_seconds: f64,
    /// Input lowpass, Hz.
    pub input_cutoff_hz: f64,
    /// Low-frequency (below about 250 Hz) change of the wet signal in dB;
    /// zero or negative.
    pub low_shelf_db: f64,
    /// Stereo width of the wet signal: 0 mono, 1 as generated.
    pub width: f64,
}

impl ReverbSettings {
    pub(crate) fn valid(&self) -> bool {
        let within = |v: f64, lo: f64, hi: f64| v.is_finite() && v >= lo && v <= hi;
        within(self.decay_seconds, 0.01, MAX_DECAY_SECONDS)
            && within(self.size, 0.05, MAX_SIZE)
            && within(self.damping_hz, 20.0, 192_000.0)
            && within(self.modulation_seconds, 0.0, MAX_MODULATION_SECONDS)
            && within(self.diffusion, 0.0, 0.75)
            && within(self.predelay_seconds, 0.0, MAX_PREDELAY_SECONDS)
            && within(self.input_cutoff_hz, 20.0, 192_000.0)
            && within(self.low_shelf_db, -60.0, 0.0)
            && within(self.width, 0.0, 1.0)
    }

    /// Frames of output after input falls silent until it is under -120 dBFS
    /// from a full-scale peak.
    pub fn tail_frames(&self, rate: u32) -> u32 {
        ((self.predelay_seconds + 2.0 * self.decay_seconds) * f64::from(rate))
            .min(f64::from(u32::MAX)) as u32
    }
}

/// Power-of-two ring; `mask` wraps indices without branches.
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
    #[inline(always)]
    fn read(&self, pos: usize, delay: usize) -> f32 {
        self.buf[pos.wrapping_sub(delay) & self.mask]
    }
    #[inline(always)]
    fn write(&mut self, pos: usize, value: f32) {
        self.buf[pos & self.mask] = value;
    }
}

fn one_pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-TAU * hz.min(rate * 0.49) / rate).exp()
}

/// One reverb's state. All buffers are allocated by [`Reverb::new`].
pub(crate) struct Reverb {
    pos: usize,
    predelay: Line,
    predelay_len: usize,
    allpass: [[Line; 2]; 2],
    allpass_len: [usize; 2],
    diffusion: f32,
    /// One frame of all lines per entry; the length is a power of two.
    lines: Box<[[f32; LINES]]>,
    delay: [f32; LINES],
    feedback: [f32; LINES],
    damp_coef: f32,
    damp_state: [f32; LINES],
    mod_depth: f32,
    lfo_phase: f32,
    lfo_step: f32,
    lfo: [f32; 2],
    input_coef: f32,
    input_state: [f32; 2],
    shelf_coef: f32,
    shelf_gain: f32,
    shelf_state: [f32; 2],
    width: f32,
}

impl Reverb {
    pub(crate) fn new(s: &ReverbSettings, rate: u32) -> Result<Self, Error> {
        if rate == 0 || !s.valid() {
            return Err(Error::InvalidInput);
        }
        let rate = rate as f32;
        let ms = |v: f32| v * 0.001 * rate;
        let size = s.size as f32;
        let longest = ms(BASE_MS[LINES - 1] * size) + (s.modulation_seconds as f32) * rate;
        let allpass_len = ALLPASS_MS.map(|t| ms(t * size).max(1.0) as usize);
        let rt60 = s.decay_seconds as f32;
        let delay = BASE_MS.map(|base| ms(base * size));
        Ok(Self {
            pos: 0,
            predelay: Line::new((s.predelay_seconds as f32 * rate) as usize + 1),
            predelay_len: (s.predelay_seconds as f32 * rate) as usize,
            allpass: std::array::from_fn(|_| allpass_len.map(|n| Line::new(n + 1))),
            allpass_len,
            diffusion: s.diffusion as f32,
            lines: vec![[0.0; LINES]; (longest as usize + 4).next_power_of_two()].into(),
            delay,
            // Each pass through a line of length d loses 60 dB over rt60.
            feedback: delay.map(|d| 10f32.powf(-3.0 * d / (rt60 * rate))),
            damp_coef: one_pole(s.damping_hz as f32, rate),
            damp_state: [0.0; LINES],
            mod_depth: s.modulation_seconds as f32 * rate,
            lfo_phase: 0.0,
            lfo_step: TAU * 0.7 / rate,
            lfo: [0.0, 1.0],
            input_coef: one_pole(s.input_cutoff_hz as f32, rate),
            input_state: [0.0; 2],
            shelf_coef: one_pole(250.0, rate),
            shelf_gain: 10f32.powf(s.low_shelf_db as f32 / 20.0) - 1.0,
            shelf_state: [0.0; 2],
            width: s.width as f32,
        })
    }

    /// Silence the network.
    pub(crate) fn clear(&mut self) {
        for line in self
            .allpass
            .iter_mut()
            .flatten()
            .chain([&mut self.predelay])
        {
            line.buf.fill(0.0);
        }
        self.lines.fill([0.0; LINES]);
        self.damp_state = [0.0; LINES];
        self.input_state = [0.0; 2];
        self.shelf_state = [0.0; 2];
    }

    /// Replace `left`/`right` with the wet signal.
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
        self.predelay.write(pos, 0.5 * (input[0] + input[1]));
        let dry = self.predelay.read(pos, self.predelay_len);
        let mut diffused = [0.0; 2];
        for (ch, out) in diffused.iter_mut().enumerate() {
            let state = &mut self.input_state[ch];
            *state = super::kernels::biased_one_pole32(*state, (dry - *state) * self.input_coef);
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
            *damped = super::kernels::biased_one_pole32(*damped, (mix[i] * self.feedback[i] - *damped) * self.damp_coef);
            *y = *damped + diffused[i % 2];
        }
        let l = (taps[0] - taps[2] + taps[4] - taps[6]) * 0.5;
        let r = (taps[1] - taps[3] + taps[5] - taps[7]) * 0.5;
        let (mid, side) = (0.5 * (l + r), 0.5 * (l - r) * self.width);
        let mut out = [mid + side, mid - side];
        for (ch, v) in out.iter_mut().enumerate() {
            let low = &mut self.shelf_state[ch];
            *low = super::kernels::one_pole32(*low, *v, self.shelf_coef);
            *v += *low * self.shelf_gain;
        }
        out
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

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn settings(decay: f64) -> ReverbSettings {
        ReverbSettings {
            decay_seconds: decay,
            size: 0.75,
            damping_hz: 6_000.0,
            modulation_seconds: 0.00075,
            diffusion: 0.375,
            predelay_seconds: 0.0,
            input_cutoff_hz: 20_000.0,
            low_shelf_db: 0.0,
            width: 1.0,
        }
    }

    fn energy(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum()
    }

    #[test]
    fn impulse_decays_and_stays_finite() {
        let mut rv = Reverb::new(&settings(1.1), 48_000).unwrap();
        let mut l = vec![0.0; 4 * 48_000];
        let mut r = l.clone();
        l[0] = 1.0;
        r[0] = 1.0;
        for (l, r) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            rv.process(l, r);
        }
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        let early = energy(&l[..24_000]);
        let late = energy(&l[3 * 48_000..]);
        assert!(early > 1e-3, "no tail: {early}");
        assert!(late < early * 1e-4, "early {early} late {late}");
    }

    #[test]
    fn maximum_time_is_stable_and_rejects_bad_settings() {
        let mut rv = Reverb::new(&settings(MAX_DECAY_SECONDS), 44_100).unwrap();
        let (mut l, mut r) = (vec![0.5; 256], vec![-0.5; 256]);
        for _ in 0..500 {
            rv.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 100.0));
            l.fill(0.0);
            r.fill(0.0);
        }
        let bad = ReverbSettings {
            size: 0.0,
            ..settings(1.0)
        };
        assert!(Reverb::new(&bad, 48_000).is_err());
    }

    #[test]
    fn hadamard_preserves_energy() {
        let mut x = [1.0, 0.0, 0.5, -2.0, 0.0, 0.25, 0.0, 1.0];
        let before = energy(&x);
        hadamard(&mut x);
        assert!((energy(&x) - before).abs() < 1e-5);
    }
}

#[cfg(test)]
mod frozen_kernel_tests {
    use super::*;
    fn frozen_tick(rv: &mut Reverb, input: [f32; 2], [sin, cos]: [f32; 2]) -> [f32; 2] {
        let pos = rv.pos;
        rv.pos = pos.wrapping_add(1);
        // The input sums to mono; the network decorrelates the outputs.
        rv.predelay.write(pos, 0.5 * (input[0] + input[1]));
        let dry = rv.predelay.read(pos, rv.predelay_len);
        let mut diffused = [0.0; 2];
        for (ch, out) in diffused.iter_mut().enumerate() {
            let state = &mut rv.input_state[ch];
            *state += (dry - *state) * rv.input_coef + 1e-20;
            let mut x = *state;
            for (line, &len) in rv.allpass[ch].iter_mut().zip(&rv.allpass_len) {
                let delayed = line.read(pos, len);
                let v = x + rv.diffusion * delayed;
                line.write(pos, v);
                x = delayed - rv.diffusion * v;
            }
            *out = x;
        }
        // Quadrature LFOs on alternate lines decorrelate modes without pitch wobble.
        let lfo = [sin, cos, -sin, -cos, sin, cos, -sin, -cos];
        let mask = rv.lines.len() - 1;
        let taps: [f32; LINES] = std::array::from_fn(|i| {
            let d = rv.delay[i] + rv.mod_depth * (0.5 + 0.5 * lfo[i]);
            let whole = d as usize;
            let frac = d - whole as f32;
            let a = rv.lines[pos.wrapping_sub(whole) & mask][i];
            let b = rv.lines[pos.wrapping_sub(whole + 1) & mask][i];
            a + (b - a) * frac
        });
        let mut mix = taps;
        hadamard(&mut mix);
        let row = &mut rv.lines[pos & mask];
        for (i, (y, damped)) in row.iter_mut().zip(&mut rv.damp_state).enumerate() {
            *damped += (mix[i] * rv.feedback[i] - *damped) * rv.damp_coef + 1e-20;
            *y = *damped + diffused[i % 2];
        }
        let l = (taps[0] - taps[2] + taps[4] - taps[6]) * 0.5;
        let r = (taps[1] - taps[3] + taps[5] - taps[7]) * 0.5;
        let (mid, side) = (0.5 * (l + r), 0.5 * (l - r) * rv.width);
        let mut out = [mid + side, mid - side];
        for (ch, v) in out.iter_mut().enumerate() {
            let low = &mut rv.shelf_state[ch];
            *low += (*v - *low) * rv.shelf_coef;
            *v += *low * rv.shelf_gain;
        }
        out
    }
    #[test]
    fn shared_reverb_matches_frozen_pcm_and_state_bits() {
        for rate in [44100, 48000, 96000] {
            let settings = ReverbSettings { low_shelf_db: -12., size: 0.05, ..super::tests::settings(1.1) };
            let mut actual = Reverb::new(&settings, rate).unwrap();
            let mut expected = Reverb::new(&settings, rate).unwrap();
            for i in 0..8192 {
                let x = if i == 0 { [1., -0.5] } else if i < 2048 { [(i as f32 * 0.137).sin() * 0.2, 0.] } else { [-0., f32::from_bits(1)] };
                let lfo = [(i as f32 * 0.003).sin(), (i as f32 * 0.003).cos()];
                assert_eq!(actual.tick(x, lfo).map(f32::to_bits), frozen_tick(&mut expected, x, lfo).map(f32::to_bits));
                assert_eq!(actual.input_state.map(f32::to_bits), expected.input_state.map(f32::to_bits));
                assert_eq!(actual.damp_state.map(f32::to_bits), expected.damp_state.map(f32::to_bits));
                assert_eq!(actual.shelf_state.map(f32::to_bits), expected.shelf_state.map(f32::to_bits));
                assert_eq!(actual.pos, expected.pos);
            }
            for (a, b) in actual.lines.iter().zip(expected.lines.iter()) { assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits)); }
        }
    }
}
