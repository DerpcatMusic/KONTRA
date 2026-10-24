//! Standard test signals. Deterministic: noise uses a fixed-seed LCG so runs are comparable.

use std::f64::consts::TAU;

/// Fixed-seed generator (64-bit LCG, top 24 bits to a float in [-1, 1)).
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1))
    }
    pub fn next_unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f64 / (1u64 << 23) as f64) - 1.0
    }
}

pub fn sine(rate: f64, hz: f64, amp: f64, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (amp * (TAU * hz * n as f64 / rate).sin()) as f32).collect()
}

/// Logarithmic sweep from `lo` to `hi` Hz over `frames`.
pub fn log_sweep(rate: f64, lo: f64, hi: f64, amp: f64, frames: usize) -> Vec<f32> {
    let k = (hi / lo).ln() / frames as f64;
    let mut phase = 0.0;
    (0..frames)
        .map(|n| {
            let f = lo * (k * n as f64).exp();
            phase += TAU * f / rate;
            (amp * phase.sin()) as f32
        })
        .collect()
}

pub fn impulse(frames: usize, amp: f64) -> Vec<f32> {
    let mut v = vec![0.0; frames];
    v[0] = amp as f32;
    v
}

pub fn white(frames: usize, amp: f64, seed: u64) -> Vec<f32> {
    let mut g = Lcg::new(seed);
    (0..frames).map(|_| (amp * g.next_unit()) as f32).collect()
}

/// Paul Kellet's economy pink filter over white noise (about -3 dB/oct, within 0.5 dB over 20 Hz to 20 kHz).
pub fn pink(frames: usize, amp: f64, seed: u64) -> Vec<f32> {
    let mut g = Lcg::new(seed);
    let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    (0..frames)
        .map(|_| {
            let w = g.next_unit();
            b0 = 0.99886 * b0 + w * 0.0555179;
            b1 = 0.99332 * b1 + w * 0.0750759;
            b2 = 0.96900 * b2 + w * 0.1538520;
            b3 = 0.86650 * b3 + w * 0.3104856;
            b4 = 0.55000 * b4 + w * 0.5329522;
            b5 = -0.7616 * b5 - w * 0.0168980;
            let y = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11;
            b6 = w * 0.115926;
            (amp * y) as f32
        })
        .collect()
}

/// Band-limited-ish square and saw at `hz` by summing harmonics below Nyquist (no aliasing source).
pub fn square(rate: f64, hz: f64, amp: f64, frames: usize) -> Vec<f32> {
    additive(rate, hz, amp, frames, |k| if k % 2 == 1 { 4.0 / (std::f64::consts::PI * k as f64) } else { 0.0 })
}

pub fn saw(rate: f64, hz: f64, amp: f64, frames: usize) -> Vec<f32> {
    additive(rate, hz, amp, frames, |k| 2.0 * (if k % 2 == 1 { 1.0 } else { -1.0 }) / (std::f64::consts::PI * k as f64))
}

/// Naive (aliasing) saw: the stress case for nonlinear blocks, unlike `saw`.
pub fn naive_saw(rate: f64, hz: f64, amp: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| {
            let p = (hz * n as f64 / rate).fract();
            (amp * (2.0 * p - 1.0)) as f32
        })
        .collect()
}

fn additive(rate: f64, hz: f64, amp: f64, frames: usize, coeff: impl Fn(usize) -> f64) -> Vec<f32> {
    let harmonics: Vec<(f64, f64)> = (1..)
        .map(|k| (k, k as f64 * hz))
        .take_while(|(_, f)| *f < rate * 0.5)
        .map(|(k, f)| (coeff(k as usize), f))
        .collect();
    (0..frames)
        .map(|n| {
            let t = n as f64 / rate;
            let y: f64 = harmonics.iter().map(|(c, f)| c * (TAU * f * t).sin()).sum();
            (amp * y) as f32
        })
        .collect()
}

/// Exponentially decaying burst down to the subnormal range: the denormal-bait signal.
pub fn denormal_tail(rate: f64, frames: usize) -> Vec<f32> {
    let k = (1e-38f64).ln() / frames as f64;
    (0..frames)
        .map(|n| {
            let env = (k * n as f64).exp();
            (0.5 * env * (TAU * 220.0 * n as f64 / rate).sin()) as f32
        })
        .collect()
}

/// Step from `lo` to `hi` at frame `at`.
pub fn step(lo: f64, hi: f64, at: usize, frames: usize) -> Vec<f32> {
    (0..frames).map(|n| if n < at { lo as f32 } else { hi as f32 }).collect()
}

/// Drum-like loop: decaying kicks and noise snares on a 120 bpm grid, ~1 s.
pub fn drum_loop(rate: f64, frames: usize, seed: u64) -> Vec<f32> {
    let mut out = vec![0.0f32; frames];
    let mut g = Lcg::new(seed);
    let beat = (rate * 0.5) as usize;
    for (i, slot) in (0..frames).step_by(beat).enumerate() {
        let len = (rate * 0.35) as usize;
        for n in 0..len.min(frames - slot) {
            let t = n as f64 / rate;
            let v = if i % 2 == 0 {
                (TAU * (60.0 + 80.0 * (-t * 25.0).exp()) * t).sin() * (-t * 9.0).exp()
            } else {
                g.next_unit() * (-t * 22.0).exp() * 0.7
            };
            out[slot + n] += (0.8 * v) as f32;
        }
    }
    out
}

/// A sustained string-like tone: 12 partials with a slow attack and a 1/k^1.3 rolloff, plus vibrato.
pub fn strings(rate: f64, hz: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| {
            let t = n as f64 / rate;
            let env = (t * 4.0).min(1.0);
            let vib = 1.0 + 0.004 * (TAU * 5.5 * t).sin();
            let y: f64 = (1..=12)
                .filter(|&k| k as f64 * hz * vib < rate * 0.5)
                .map(|k| (TAU * k as f64 * hz * vib * t).sin() / (k as f64).powf(1.3))
                .sum();
            (0.2 * env * y) as f32
        })
        .collect()
}

pub fn silence(frames: usize) -> Vec<f32> {
    vec![0.0; frames]
}
