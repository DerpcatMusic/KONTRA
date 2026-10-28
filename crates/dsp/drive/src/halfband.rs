//! A 2x polyphase IIR halfband (Laurent de Soras' HIIR design): two
//! branches of first-order allpasses `y = (x − y₁)·c + x₁`, passband to
//! 20 kHz at 44.1 kHz and up, about 78 dB of stopband (v2 sampler-core
//! dsp/hiir.rs).
//!
//! ponytail: lives here while drive is its only user; moves to
//! `kontra-dsp-core` (03-DSP.md §2 `oversample.rs`) with a second family.

/// Allpass coefficients, at most (8 at 44.1 kHz).
const MAX: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Halfband {
    coefs: [f32; MAX],
    count: usize,
    /// `[x₁, y₁]` per coefficient: up, then down, per channel.
    state: [[[[f32; 2]; MAX]; 2]; 2],
}

impl Halfband {
    /// For a passband to 20 kHz (or 0.45 of the rate, below 44.1 kHz).
    pub(crate) fn new(rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        let edge = 20_000f64.min(0.45 * rate);
        let (coefs, count) = design(edge / rate / 2.0, 78.0);
        Self {
            coefs,
            count,
            state: [[[[0.0; 2]; MAX]; 2]; 2],
        }
    }

    fn step(&self, mem: &mut [[f32; 2]; MAX], mut a: f32, mut b: f32) -> (f32, f32) {
        for (pair, c) in mem
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(self.coefs.as_chunks::<2>().0)
            .take(self.count / 2)
        {
            let [m0, m1] = pair;
            let t0 = (a - m0[1]) * c[0] + m0[0];
            let t1 = (b - m1[1]) * c[1] + m1[0];
            *m0 = [a, t0];
            *m1 = [b, t1];
            (a, b) = (t0, t1);
        }
        (a, b)
    }

    /// One frame at twice the rate: `f` runs on each of the two samples.
    pub(crate) fn run(&mut self, frame: &mut [f32; 2], mut f: impl FnMut(f32) -> f32) {
        let mut state = self.state;
        for (x, [up, down]) in frame.iter_mut().zip(&mut state) {
            let (a, b) = self.step(up, *x, *x);
            let (a, b) = (f(a), f(b));
            let (a, b) = self.step(down, b, a);
            *x = (a + b) * 0.5;
        }
        self.state = state;
    }

    /// Clears memories that have decayed into subnormals.
    pub(crate) fn flush(&mut self) {
        for v in self
            .state
            .as_flattened_mut()
            .as_flattened_mut()
            .as_flattened_mut()
        {
            if v.abs() < 1e-20 {
                *v = 0.0;
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        self.state = [[[[0.0; 2]; MAX]; 2]; 2];
    }
}

/// The even-count coefficient set for passband edge `t` (a fraction of the
/// doubled rate) and `attenuation` dB.
fn design(t: f64, attenuation: f64) -> ([f32; MAX], usize) {
    use std::f64::consts::PI;
    let k = (PI * t).tan().powi(2);
    let s = (1.0 - k * k).sqrt().sqrt();
    let e = ((1.0 - s) * 0.5) / (s + 1.0);
    let e4 = e.powi(4);
    let e5 = e4 * e;
    let q = e5 * e4 * e4 * 150.0 + e5 * e4 * 15.0 + e + 2.0 * e5;
    let d = 10f64.powf(attenuation * -0.05);
    let a = (d * d) / (1.0 - d * d);
    let n = (((a * a * 0.0625).ln() / q.ln() - 1.0) * 0.5)
        .ceil()
        .max(1.0);
    let n = (n as usize).next_multiple_of(2).min(MAX);
    let order = (n * 2 + 1) as f64;
    let q4 = q.powf(0.25);
    let mut out = [0f32; MAX];
    for (c, slot) in out.iter_mut().take(n).enumerate() {
        let c = (c + 1) as f64;
        let num: f64 = (0..4)
            .map(|i| {
                let i = f64::from(i);
                q.powf((i + 1.0) * i) * (-1f64).powf(i) * ((2.0 * i + 1.0) * PI / order * c).sin()
            })
            .sum();
        let den: f64 = (1..4)
            .map(|i| {
                let i = f64::from(i);
                q.powf(i * i) * (-1f64).powf(i) * (2.0 * i * PI / order * c).cos()
            })
            .sum();
        let ww = (2.0 * q4 * num / (2.0 * den + 1.0)).powi(2);
        let x = ((1.0 - ww / k) * (1.0 - ww * k)).sqrt() / (ww + 1.0);
        *slot = ((1.0 - x) / (x + 1.0)) as f32;
    }
    (out, n)
}
