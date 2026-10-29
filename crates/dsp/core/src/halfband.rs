//! A 2x polyphase IIR halfband (Laurent de Soras' HIIR design): two
//! branches of first-order allpasses `y = (x − y₁)·c + x₁`, passband to
//! 20 kHz at 44.1 kHz and up, about 78 dB of stopband (v2 sampler-core
//! dsp/hiir.rs).
//!
//! Drive's shaper and the explicit ladder run inside it.

/// Allpass coefficients, at most (6 at any rate: 0.45 of it below 44.1 kHz).
const MAX: usize = 8;

/// Coefficient pairs at most.
const PAIRS: usize = MAX / 2;

/// Four lanes: branch a left and right, branch b left and right.
type Lanes = [f32; 4];

#[derive(Clone, Debug, PartialEq)]
pub struct Halfband {
    /// Per pair `[c₀, c₀, c₁, c₁]`.
    coefs: [Lanes; PAIRS],
    pairs: usize,
    /// Per pair `[x₁, y₁]`, up then down.
    up: [[Lanes; 2]; PAIRS],
    down: [[Lanes; 2]; PAIRS],
}

/// Every allpass pair one step, all four lanes at once.
fn step(coefs: &[Lanes], mem: &mut [[Lanes; 2]], mut v: Lanes) -> Lanes {
    for (c, [x, y]) in coefs.iter().zip(mem) {
        let mut t = [0.0; 4];
        for (t, (((v, y), c), x)) in t.iter_mut().zip(v.iter().zip(&*y).zip(c).zip(&*x)) {
            *t = (v - y) * c + x;
        }
        (*x, *y, v) = (v, t, t);
    }
    v
}

impl Halfband {
    /// For a passband to 20 kHz (or 0.45 of the rate, below 44.1 kHz).
    #[must_use]
    pub fn new(rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        let edge = 20_000f64.min(0.45 * rate);
        let (c, count) = design(edge / rate / 2.0, 78.0);
        let mut coefs = [[0.0; 4]; PAIRS];
        for (lanes, [c0, c1]) in coefs.iter_mut().zip(c.as_chunks::<2>().0) {
            *lanes = [*c0, *c0, *c1, *c1];
        }
        Self {
            coefs,
            pairs: count / 2,
            up: [[[0.0; 4]; 2]; PAIRS],
            down: [[[0.0; 4]; 2]; PAIRS],
        }
    }

    /// `io` at twice the rate: `f` runs on each frame's two stereo samples,
    /// the earlier first.
    pub fn process(&mut self, io: &mut [[f32; 2]], mut f: impl FnMut([f32; 2]) -> [f32; 2]) {
        let coefs = self.coefs.get(..self.pairs).unwrap_or_default();
        let (up, down) = (&mut self.up[..], &mut self.down[..]);
        for frame in io.iter_mut() {
            let [l, r] = *frame;
            let [a0, a1, b0, b1] = step(coefs, up, [l, r, l, r]);
            let ([a0, a1], [b0, b1]) = (f([a0, a1]), f([b0, b1]));
            let [a0, a1, b0, b1] = step(coefs, down, [b0, b1, a0, a1]);
            *frame = [0.5 * (a0 + b0), 0.5 * (a1 + b1)];
        }
        for v in self
            .up
            .iter_mut()
            .chain(self.down.iter_mut())
            .flatten()
            .flatten()
        {
            if v.abs() < 1e-20 {
                *v = 0.0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.up = [[[0.0; 4]; 2]; PAIRS];
        self.down = [[[0.0; 4]; 2]; PAIRS];
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
