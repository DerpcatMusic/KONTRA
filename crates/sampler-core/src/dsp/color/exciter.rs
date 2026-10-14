//! UVI Workstation 4.0.9 Exciter as the color chain's shaper (element process 0x14125c680,
//! Tape kernel 0x14125c870, Tube kernel 0x14125cb60, prepare 0x14125aba0), oversampled
//! 2^k through the shared HIIR stage (20 kHz, 78 dB) like the UVI drives (model
//! uvicolor/exc_model.py, checked against the emulated kernels).
use super::super::hiir::{self, Hiir, HiirState};
use super::super::Planar;
pub use sampler_ir::fx::color::Exciter;

/// `0x141350b50`: `expf(x * ln 10 / 20)`.
fn db(x: f32) -> f32 {
    (x * 0.115_129_25).exp()
}

const C: f32 = 0.951_056_54;
const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    tape: bool,
    gin: f32,
    gout: f32,
    amount: f32,
    mix: f32,
    drive: f32,
    tube_k: f32,
    attack: f32,
    oversampling: usize,
    hiir: Hiir,
}

/// The Tube envelope (reset to 1) and the oversampler memories per channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct State {
    envelope: Option<[f32; 2]>,
    hiir: [HiirState; 2],
}

impl Default for State {
    fn default() -> Self {
        Self { envelope: None, hiir: [HiirState::default(); 2] }
    }
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.envelope.is_none_or(|e| e.iter().all(|v| v.is_finite())) && self.hiir.iter().all(HiirState::finite)
    }
}

impl Engine {
    pub(super) fn new(e: Exciter, rate: u32) -> Self {
        let fs = rate as f32;
        // ponytail: the Amount range lives in encrypted .data (0x14258be20); the documented
        // 0..15 stands in until a runtime dump.
        let (lo, hi) = (0.0f32, 15.0f32);
        let a = ((e.amount - lo) / (hi - lo)) * 10.0;
        let hf = fs * 0.5;
        let p = (hf * 0.005).powf(0.7);
        Self {
            tape: e.mode == 0,
            gin: db(e.input_gain),
            gout: db(e.output_gain),
            amount: e.amount,
            mix: e.mix,
            drive: db(a),
            tube_k: db((e.amount - 7.0) * 0.5),
            attack: 1.0 - ((p * -40.0) / hf).exp(),
            oversampling: usize::from(e.oversampling.min(hiir::STAGES as u8)),
            hiir: Hiir::new(fs, 20_000.0, 78.0),
        }
    }

    pub(super) fn process(&self, state: &mut State, block: &mut Planar, len: usize) {
        const BLOCK: usize = super::super::BLOCK;
        let k = self.oversampling;
        let envelope = state.envelope.get_or_insert([1.0; 2]);
        for ch in 0..2 {
            let mut x = [0f32; BLOCK << hiir::STAGES];
            let mut scratch = [0f32; BLOCK << hiir::STAGES];
            let input: [f32; BLOCK] = std::array::from_fn(|i| block[ch][i] as f32);
            self.hiir.up(&mut state.hiir[ch], &input[..len], k, &mut x, &mut scratch);
            let n = len << k;
            if self.tape {
                self.tape(&mut x[..n]);
            } else {
                self.tube(&mut x[..n], &mut envelope[ch]);
            }
            let mut out = [0f32; BLOCK];
            self.hiir.down(&mut state.hiir[ch], &mut x[..n], k, &mut out);
            for (o, v) in block[ch][..len].iter_mut().zip(&out) {
                *o = f64::from(*v);
            }
        }
    }

    /// Tape: a sine shaper; the 4-wide main loop multiplies by a refined reciprocal of `C`
    /// (rcpps + one Newton step, taken exact), the scalar tail divides, in another order.
    fn tape(&self, x: &mut [f32]) {
        let wet = self.mix * 0.01;
        let dry = 1.0 - wet;
        let (g, d) = (self.gin, self.drive);
        let r0 = 1.0 / C;
        let r = (r0 + r0) - ((r0 * r0) * C);
        let wide = x.len() / 4 * 4;
        for (i, v) in x.iter_mut().enumerate() {
            let u = *v;
            *v = if i < wide {
                let s = ((((d * HALF_PI) * 0.8) * u) * g).sin();
                (((s * r) * wet) + ((u * g) * dry)) * self.gout
            } else {
                let s = ((((g * u) * d) * 0.8) * HALF_PI).sin();
                (((s / C) * wet) + ((g * u) * dry)) * self.gout
            };
        }
    }

    /// Tube: an envelope-normalised cubic, `env` a one-pole follower of `|u|`.
    fn tube(&self, x: &mut [f32], env: &mut f32) {
        let (g, m) = (self.gin, self.mix);
        let lin = ((m * self.amount) * 0.002) + 1.0;
        let scale = m * 0.0002;
        for v in x.iter_mut() {
            let u = g * *v;
            *env += (u.abs() - *env) * self.attack;
            let r = if *env >= 0.001 { 1.0 / *env } else { 1000.0 };
            let t = (u * self.tube_k) * r;
            let h = (((t + 1.0) * t) * t) * (scale * *env);
            *v = (h + (u * lin)) * self.gout;
        }
    }
}
