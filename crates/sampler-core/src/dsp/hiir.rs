//! The shared 2^k polyphase-IIR oversampler stage (UVI Workstation 4.0.9, HIIR design):
//! design 0x14163cc40 -> 0x14163f7c0, upsample 0x14163cdc0, downsample 0x14163cec0. Each 2x
//! stage is two branches of chained first-order allpasses `y = (x - y1) c + x1` (f32); stage
//! `s` is designed for the passband edge `edge / 2^(s+1)` of its output rate. Used by the
//! color chain (UVI drives) and the filter engine (Xpander).

/// Most allpass coefficients per stage (8 at 44.1 kHz, 80 dB).
const MAX: usize = 16;
pub(crate) const STAGES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hiir {
    coefs: [[f32; MAX]; STAGES],
    counts: [usize; STAGES],
}

/// Allpass memories `[x1, y1]` per coefficient, up and down, per stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HiirState {
    up: [[[f32; 2]; MAX]; STAGES],
    down: [[[f32; 2]; MAX]; STAGES],
}

impl Default for HiirState {
    fn default() -> Self {
        Self { up: [[[0.0; 2]; MAX]; STAGES], down: [[[0.0; 2]; MAX]; STAGES] }
    }
}

impl HiirState {
    /// Stage 0's memories `[up, down]`, for engines that keep a 2x stage in their own cells.
    pub(crate) fn first(&mut self) -> [&mut [[f32; 2]; MAX]; 2] {
        [&mut self.up[0], &mut self.down[0]]
    }

    pub(crate) fn finite(&self) -> bool {
        self.up.iter().chain(&self.down).flatten().flatten().all(|v| v.is_finite())
    }
}

/// `0x14163f7c0(t, attenuation)`: the even-count coefficient set for passband edge `t`
/// (fraction of the stage's output rate), in f64 with f32 stores.
fn design(t: f64, attenuation: f64) -> ([f32; MAX], usize) {
    use std::f64::consts::PI;
    let x2 = (PI - t * 2.0 * PI) - t * 2.0 * PI;
    let k = ((PI - x2) * 0.25).tan().powi(2);
    let s = (1.0 - k * k).sqrt().sqrt();
    let e = ((1.0 - s) * 0.5) / (s + 1.0);
    let e4 = e * e * e * e;
    let e5 = e4 * e;
    let q = e5 * e4 * e4 * 150.0 + e5 * e4 * 15.0 + e + e5 + e5;
    let d = 10f64.powf(attenuation * -0.05);
    let a = (d * d) / (1.0 - d * d);
    let mut n = (((a * a * 0.0625).ln() / q.ln() - 1.0) * 0.5).ceil() as usize;
    n += n & 1;
    let n = n.min(MAX);
    let order = (n * 2 + 1) as f64;
    let q4 = q.powf(0.25);
    let mut out = [0f32; MAX];
    for (c, slot) in out[..n].iter_mut().enumerate() {
        let c = (c + 1) as f64;
        let mut num = 0.0;
        for (i, u) in [1.0, 3.0, 5.0, 7.0].into_iter().enumerate() {
            let i = i as f64;
            num += q.powf((i + 1.0) * i) * (-1f64).powf(i) * (u * PI * (1.0 / order) * c).sin();
        }
        let mut den = 0.0;
        for (i, u) in [(1.0, 2.0), (2.0, 4.0), (3.0, 6.0)] {
            den += q.powf(i * i) * (-1f64).powf(i) * (u * PI * (1.0 / order) * c).cos();
        }
        let ww = ((q4 + q4) * num) / (den + den + 1.0);
        let ww = ww * ww;
        let x = ((1.0 - (1.0 / k) * ww) * (1.0 - ww * k)).sqrt() * (1.0 / (ww + 1.0));
        *slot = ((1.0 - x) / (x + 1.0)) as f32;
    }
    (out, n)
}

impl Hiir {
    /// Stages for `rate`: stage `s` at passband edge `edge / rate / 2^(s+1)`.
    pub(crate) fn new(rate: f32, edge: f64, attenuation: f64) -> Self {
        let mut coefs = [[0f32; MAX]; STAGES];
        let mut counts = [0; STAGES];
        for s in 0..STAGES {
            (coefs[s], counts[s]) = design(edge / f64::from(rate) / f64::from(2u32 << s), attenuation);
        }
        Self { coefs, counts }
    }

    #[inline(always)]
    fn step(&self, s: usize, mem: &mut [[f32; 2]; MAX], mut a: f32, mut b: f32) -> (f32, f32) {
        let c = &self.coefs[s];
        for i in (0..self.counts[s]).step_by(2) {
            let t0 = (a - mem[i][1]) * c[i] + mem[i][0];
            let t1 = (b - mem[i + 1][1]) * c[i + 1] + mem[i + 1][0];
            mem[i] = [a, t0];
            mem[i + 1] = [b, t1];
            (a, b) = (t0, t1);
        }
        (a, b)
    }

    /// `input` (n frames) to `out` (`n << k`), through stages `0..k`. `scratch` holds `n << k`.
    pub(crate) fn up(&self, st: &mut HiirState, input: &[f32], k: usize, out: &mut [f32], scratch: &mut [f32]) {
        let n = input.len();
        out[..n].copy_from_slice(input);
        for s in 0..k {
            let len = n << s;
            scratch[..len].copy_from_slice(&out[..len]);
            for i in 0..len {
                let x = scratch[i];
                (out[2 * i], out[2 * i + 1]) = self.step(s, &mut st.up[s], x, x);
            }
        }
    }

    /// `input` (`n << k` frames) down to `out` (n), through stages `0..k` (highest rate first).
    pub(crate) fn down(&self, st: &mut HiirState, input: &mut [f32], k: usize, out: &mut [f32]) {
        let mut len = input.len();
        for s in 0..k {
            for i in 0..len / 2 {
                let (a, b) = self.step(s, &mut st.down[s], input[2 * i + 1], input[2 * i]);
                input[i] = (a + b) * 0.5;
            }
            len /= 2;
        }
        out[..len].copy_from_slice(&input[..len]);
    }
}
