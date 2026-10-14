//! Kontakt KONTAKTFX polyphase resampler, the stage shared by the Kontakt cores that
//! oversample (Tape Saturator, the Simple/Dual SKF/Monark filters): setup 0x140ae22e0, tap
//! design 0x140ae24c0 / Kaiser-windowed sinc 0x140ae2910 (16 taps, beta 8, Abramowitz-Stegun
//! I0), x2 up 0x140ae2c40 / down 0x140ae3300 (symmetric transposed registers), xL up
//! 0x140ae35d0 / down 0x140ae2f50 (one transposed 16-register block per phase).
//! Up: `y[Ln] = x[n-8]`, `y[Ln+p] = FIR_p(x)`. Down: `y[n] = (x[Ln-8L] + Σ FIR_p) / L`.
//! All in f32 in the native register order (model sv/rs_model.py).

/// Largest factor.
pub(crate) const MAX: usize = 8;

fn big(y: f64) -> f64 {
    let p = (((y * 0.003_923_77 - 0.016_476_33) * y + 0.026_355_37) * y - 0.020_577_06) * y + 0.009_162_81;
    (((p * y - 0.001_575_65) * y + 0.002_253_19) * y + 0.013_285_92) * y + 0.398_942_28
}

fn i0(x: f64) -> f64 {
    if x >= 3.75 {
        (x.exp() / x.sqrt()) * big(3.75 / x)
    } else {
        let y = x * x * 0.071_111_111_111_111_11;
        (((((y * 0.004_581_3 + 0.036_076_8) * y + 0.265_973_2) * y + 1.206_749_2) * y + 3.089_942_4) * y
            + 3.515_622_9)
            * y
            + 1.0
    }
}

/// `0x140ae2910(t, 16, 8)`.
fn tap(t: f32) -> f32 {
    if t == 0.0 {
        return 1.0;
    }
    #[allow(clippy::excessive_precision)]
    let a = f64::from(t) * 3.141_592_653_589_79;
    let d = f64::from((t + t) / 16.0);
    let w = i0((1.0 - d * d).sqrt() * 8.0);
    let den = f64::from(8f32.exp() / 8f32.sqrt()) * big(3.75 / 8.0);
    ((w * (a.sin() / a)) / den) as f32
}

/// The effektrig x2 pair (generic resampler 0x141ee5da0 modes 5/6, process 0x141ee2160,
/// kernel 0x141ee6e00): the same half-sample taps summed in four lanes (up) or in order
/// (down), around the Bite and Dirt cores (model kcolor/os2x.py).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Effektrig {
    h: [f32; 16],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct EffektrigState {
    /// Newest last.
    up: [f32; 16],
    even: [f32; 9],
    odd: [f32; 17],
}

impl EffektrigState {
    pub(crate) fn finite(&self) -> bool {
        self.up.iter().chain(&self.even).chain(&self.odd).all(|v| v.is_finite())
    }
}

impl Effektrig {
    pub(crate) fn new() -> Self {
        Self { h: std::array::from_fn(|k| tap(0.5 + (k as f32 - 8.0))) }
    }

    /// One input to (even, odd) outputs.
    #[inline(always)]
    pub(crate) fn up(&self, s: &mut EffektrigState, x: f32) -> (f32, f32) {
        s.up.copy_within(1.., 0);
        s.up[15] = x;
        let mut lanes = [0f32; 4];
        for k in 0..16 {
            lanes[k & 3] += self.h[k] * s.up[15 - k];
        }
        (s.up[7], ((lanes[3] + lanes[1]) + lanes[2]) + lanes[0])
    }

    /// (even, odd) inputs to one output.
    #[inline(always)]
    pub(crate) fn down(&self, s: &mut EffektrigState, even: f32, odd: f32) -> f32 {
        s.even.copy_within(1.., 0);
        s.even[8] = even;
        s.odd.copy_within(1.., 0);
        s.odd[16] = odd;
        let mut acc = 0f32;
        for k in 0..16 {
            acc += self.h[k] * s.odd[15 - k];
        }
        0.5 * (s.even[0] + acc)
    }
}

/// The effektrig core wrapper's oversampling (ModulatableDSPCoreWrapper, generic prepare
/// slot 3): factor `2^round(log2(2^n 44100 / rate))`, at least 1; x2 runs the effektrig pair,
/// higher factors the polyphase machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wrapper {
    One,
    Two(Effektrig),
    Many(Kaiser),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct WrapperState {
    two: EffektrigState,
    up: State,
    down: State,
}

impl WrapperState {
    pub(crate) fn finite(&self) -> bool {
        self.two.finite() && self.up.finite() && self.down.finite()
    }
}

impl Wrapper {
    pub(crate) fn new(n: i32, rate: u32) -> Self {
        let e = (2f64.powi(n) * 44_100.0 / f64::from(rate)).log2().round().max(0.0) as u32;
        match (1usize << e.min(3)).min(MAX) {
            1 => Self::One,
            2 => Self::Two(Effektrig::new()),
            f => Self::Many(Kaiser::new(f)),
        }
    }

    pub(crate) fn factor(&self) -> usize {
        match self {
            Self::One => 1,
            Self::Two(_) => 2,
            Self::Many(k) => k.factor,
        }
    }

    /// One input to `factor` outputs.
    #[inline(always)]
    pub(crate) fn up(&self, s: &mut WrapperState, x: f32, out: &mut [f32]) {
        match self {
            Self::One => out[0] = x,
            Self::Two(e) => (out[0], out[1]) = e.up(&mut s.two, x),
            Self::Many(k) => k.up(&mut s.up, x, out),
        }
    }

    /// `factor` inputs to one output.
    #[inline(always)]
    pub(crate) fn down(&self, s: &mut WrapperState, frame: &[f32]) -> f32 {
        match self {
            Self::One => frame[0],
            Self::Two(e) => e.down(&mut s.two, frame[0], frame[1]),
            Self::Many(k) => k.down(&mut s.down, frame),
        }
    }
}

type Block = [f32; 16];

/// One transposed 16-register block (0x140ae35d0 / 0x140ae2f50 lane pattern).
#[inline(always)]
fn block(r: &mut Block, c: &Block, x: f32) -> f32 {
    let o = *r;
    for j in (0..16).step_by(4) {
        r[j] = x * c[j + 1] + o[j + 1];
        r[j + 1] = x * c[j + 2] + o[j + 2];
        r[j + 2] = x * c[j + 3] + o[j + 3];
        r[j + 3] = x * c[j] + o[j];
    }
    let out = r[3];
    (r[3], r[7], r[11], r[15]) = (r[7], r[11], r[15], 0.0);
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Kaiser {
    factor: usize,
    /// Half-sample taps (x2) or per-phase taps (xL), up and down.
    up: [Block; MAX - 1],
    down: [Block; MAX - 1],
    inverse: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct State {
    delay: [f32; 9],
    up: [Block; MAX - 1],
    down: [Block; MAX - 1],
    acc: f32,
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.up.iter().chain(&self.down).flatten().chain(&self.delay).all(|v| v.is_finite()) && self.acc.is_finite()
    }
}

impl Kaiser {
    pub(crate) fn new(factor: usize) -> Self {
        assert!((1..=MAX).contains(&factor));
        let phases = |down: bool| {
            let l = factor as f32;
            let (mut ph, step) = if down { (1.0f32, -1.0 / l) } else { (0.0f32, 1.0 / l) };
            let mut c = [[0f32; 16]; MAX - 1];
            for row in c.iter_mut().take(factor - 1) {
                ph += step;
                *row = std::array::from_fn(|k| tap(ph + (k as f32 - 8.0)));
            }
            c
        };
        Self { factor, up: phases(false), down: phases(true), inverse: 1.0 / factor as f32 }
    }

    pub(crate) fn factor(&self) -> usize {
        self.factor
    }

    /// One input sample to `factor` outputs.
    #[inline(always)]
    pub(crate) fn up(&self, s: &mut State, x: f32, out: &mut [f32]) {
        out[0] = s.delay[0];
        s.delay.copy_within(1..8, 0);
        s.delay[7] = x;
        if self.factor == 2 {
            let h = &self.up[0];
            let p: [f32; 8] = std::array::from_fn(|j| x * h[j]);
            // registers 8..23 of 0x140ae2c40 as r[0..16]
            let r = &mut s.up[0];
            let o = *r;
            r[0..4].copy_from_slice(&[p[1] + o[1], p[2] + o[2], p[3] + o[3], p[0] + o[0]]);
            r[4..8].copy_from_slice(&[p[3] + o[7], p[0] + o[4], p[1] + o[5], p[2] + o[6]]);
            r[8..12].copy_from_slice(&[p[5] + o[9], p[6] + o[10], p[7] + o[11], p[4] + o[8]]);
            r[12..16].copy_from_slice(&[p[7] + o[15], p[4] + o[12], p[5] + o[13], p[6] + o[14]]);
            out[1] = r[3];
            (r[3], r[11], r[12], r[4]) = (r[11], r[12], r[4], 0.0);
        } else {
            for (p, o) in out[1..self.factor].iter_mut().enumerate() {
                *o = block(&mut s.up[p], &self.up[p], x);
            }
        }
    }

    /// `factor` inputs (phase 0 first) to one output.
    #[inline(always)]
    pub(crate) fn down(&self, s: &mut State, frame: &[f32]) -> f32 {
        s.delay.copy_within(0..8, 1);
        s.delay[0] = frame[0];
        let y = (s.delay[8] + s.acc) * self.inverse;
        s.acc = 0.0;
        if self.factor == 2 {
            let h = &self.up[0];
            let x = frame[1];
            let p: [f32; 8] = std::array::from_fn(|j| h[j] * x);
            let r = &mut s.down[0];
            let o = *r;
            r[0..4].copy_from_slice(&[p[1] + o[1], p[2] + o[2], p[3] + o[3], p[0] + o[0]]);
            r[4..8].copy_from_slice(&[p[3] + o[7], p[0] + o[4], p[1] + o[5], p[2] + o[6]]);
            r[8..12].copy_from_slice(&[p[5] + o[9], p[6] + o[10], p[7] + o[11], p[4] + o[8]]);
            r[12..16].copy_from_slice(&[p[7] + o[15], p[4] + o[12], p[5] + o[13], p[6] + o[14]]);
            s.acc = r[3] + s.acc;
            (r[3], r[11], r[12], r[4]) = (r[11], r[12], r[4], 0.0);
        } else {
            for j in 1..self.factor {
                s.acc = block(&mut s.down[j - 1], &self.down[j - 1], frame[j]) + s.acc;
            }
        }
        y
    }
}
