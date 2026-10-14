//! The one dynamics engine: a frame loop over a chain of stages.
//!
//! A law is a tuple of stages chosen and parameterised once per block
//! (`laws.rs`); `run` is monomorphised per tuple, so the audio loop never
//! branches on the law. Stages talk through [`Frame`]: input conditioning
//! fills `audio`, detectors `level` (and `peak`), gain computers and
//! ballistics rewrite `level` and set `gain`, the delay fills `heard`, and the
//! output stage writes `out`. Values a law keeps in f32 travel as exact f64.
//!
//! The building blocks every law shares live here: the look-ahead [`Delay`],
//! the sliding [`Window`] maximum, the RBJ detector low-pass design, the
//! first-order high-pass design, and the make-up / wet-dry [`Shared`] stage.

use super::{Planar, ProcessorState};
use sampler_ir::fx::dynamics::Link;

#[derive(Clone, Copy, Default)]
pub(super) struct Frame {
    /// The input as read.
    pub x: [f64; 2],
    /// The audio the gain applies to (the input after drive or rounding).
    pub audio: [f64; 2],
    /// Detector level, then whatever the next stage makes of it, per channel.
    pub level: [f64; 2],
    /// Window maximum a gain computer caps against.
    pub peak: f64,
    /// Gain per channel.
    pub gain: [f64; 2],
    /// `audio` through the look-ahead delay.
    pub heard: [f64; 2],
    pub out: [f64; 2],
}

pub(super) trait Stage {
    fn tick(&mut self, f: &mut Frame);
    /// End of frame: feedback detectors read the applied gain here.
    #[inline(always)]
    fn after(&mut self, _: &Frame) {}
    /// Persist the stage's state at the end of the block.
    #[inline(always)]
    fn store(&mut self, _: &mut ProcessorState) {}
}

macro_rules! chain {
    ($($s:ident $i:tt),+) => {
        impl<$($s: Stage),+> Stage for ($($s,)+) {
            #[inline(always)]
            fn tick(&mut self, f: &mut Frame) {
                $(self.$i.tick(f);)+
            }
            #[inline(always)]
            fn after(&mut self, f: &Frame) {
                $(self.$i.after(f);)+
            }
            #[inline(always)]
            fn store(&mut self, state: &mut ProcessorState) {
                $(self.$i.store(state);)+
            }
        }
    };
}
chain!(A 0, B 1, C 2, D 3);
chain!(A 0, B 1, C 2, D 3, E 4);
chain!(A 0, B 1, C 2, D 3, E 4, F 5);
chain!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
chain!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
chain!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);

/// Run `len` frames of `block` through the chain, in place.
pub(super) fn run<S: Stage>(mut s: S, state: &mut ProcessorState, block: &mut Planar, len: usize) {
    let mut f = Frame::default();
    for i in 0..len {
        f.x = [block[0][i], block[1][i]];
        f.audio = f.x;
        s.tick(&mut f);
        s.after(&f);
        [block[0][i], block[1][i]] = f.out;
    }
    s.store(state);
}

/// The stages around a law: stereo link, make-up and wet share.
#[derive(Clone, Copy)]
pub(super) struct Shared {
    pub link: Link,
    pub makeup: f64,
    pub mix: f64,
}

impl Shared {
    /// Factor on the audio the gain `g` applies to: make-up, then wet/dry.
    #[inline(always)]
    pub fn factor(&self, g: f64) -> f64 {
        let wet = g * self.makeup;
        if self.mix == 1. { wet } else { 1. + self.mix * (wet - 1.) }
    }

    pub fn linked(&self) -> bool {
        self.link != Link::Off
    }
}

/// `audio` delayed by the ring's length into `heard` (an empty ring passes
/// it straight through). Cursor in `delay_position`.
pub(super) struct Delay<'a> {
    ring: &'a mut [[f64; 2]],
    at: usize,
}

impl<'a> Delay<'a> {
    pub fn new(ring: &'a mut [[f64; 2]], state: &ProcessorState) -> Self {
        let at = state.delay_position as usize;
        Self { at: if at < ring.len() { at } else { 0 }, ring }
    }
}

impl Stage for Delay<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        if self.ring.is_empty() {
            f.heard = f.audio;
            return;
        }
        f.heard = self.ring[self.at];
        self.ring[self.at] = f.audio;
        self.at += 1;
        if self.at == self.ring.len() {
            self.at = 0;
        }
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        state.delay_position = self.at as u32;
    }
}

/// Sliding maximum over the last `ring.len()` values, the newest included:
/// a monotonic deque of `[frame, value]` (the UVI Maximizer's `0x141667790`;
/// exact for every law, since a maximum does not depend on how it is found).
/// State: three `aux` slots from `slot` (head, length, frame).
pub(super) struct Window<'a> {
    ring: &'a mut [[f64; 2]],
    head: usize,
    count: usize,
    frame: f64,
    slot: usize,
}

impl<'a> Window<'a> {
    pub fn new(ring: &'a mut [[f64; 2]], state: &ProcessorState, slot: usize) -> Self {
        let a = &state.aux[slot..slot + 3];
        Self { ring, head: a[0] as usize, count: a[1] as usize, frame: a[2], slot }
    }

    #[inline(always)]
    pub fn push(&mut self, v: f64) -> f64 {
        let w = self.ring.len();
        // `head + k` wrapped, k < 2w, without a division.
        let at = |k: usize| if k >= w { k - w } else { k };
        while self.count > 0 && v > self.ring[at(self.head + self.count - 1)][1] {
            self.count -= 1;
        }
        while self.count > 0 && self.ring[self.head][0] + w as f64 <= self.frame {
            self.head = at(self.head + 1);
            self.count -= 1;
        }
        self.ring[at(self.head + self.count)] = [self.frame, v];
        self.count += 1;
        self.frame += 1.;
        self.ring[self.head][1]
    }

    #[inline(always)]
    pub fn store(&self, state: &mut ProcessorState) {
        state.aux[self.slot..self.slot + 3].copy_from_slice(&[self.head as f64, self.count as f64, self.frame]);
    }
}

/// The detector low-pass shared by the Kontakt limiter cores (Pro, Limiter):
/// the RBJ biquad (Q = 1/(2 * 0.91)) at angle `w`, sine and cosine from the
/// setter's polynomials rather than libm. Returns `(b0, b1, a1, a2)` with
/// `b2 = b0` and the feedback terms negated.
pub(super) fn rbj_low_pass(w: f32) -> (f32, f32, f32, f32) {
    let w2 = w * w;
    let sin = (((((w2 * f32::from_bits(0xb2cd_4ca8) + f32::from_bits(0x3638_b94f)) * w2
        - f32::from_bits(0x3950_0c03))
        * w2
        + f32::from_bits(0x3c08_8885))
        * w2
        - f32::from_bits(0x3e2a_aac1))
        * w2
        + 1.)
        * w
        * f32::from_bits(0x3f68_f5c3);
    let inverse = 1. / (sin + 1.);
    let cos = ((((w2 * f32::from_bits(0xb48b_dad9) + f32::from_bits(0x37cf_b5a1)) * w2
        - f32::from_bits(0x3ab6_09bd))
        * w2
        + f32::from_bits(0x3d2a_aa99))
        * w2
        - f32::from_bits(0x3eff_f5e8))
        * w2
        + 1.;
    let one_minus_cos = 1. - cos;
    (one_minus_cos * 0.5 * inverse, one_minus_cos * inverse, -(cos * -2. * inverse), -((1. - sin) * inverse))
}

/// First-order bilinear high-pass at `w` rad/s (Transient Master, Feedback
/// Compressor): `[b0, b1, a1]` for `y = (x b0 + x1 b1) - y1 a1`.
pub(super) fn high_pass_1(rate: u32, w: f32) -> [f32; 3] {
    let t = ((1. / rate as f32) * w).tan();
    let b = 1. / (t + 1.);
    [b, -b, (t - 1.) * b]
}

/// `[f(a0), f(a1)]`, always inlined (`array::map` is not).
#[inline(always)]
pub(super) fn each<T: Copy, U>(a: [T; 2], f: impl Fn(T) -> U) -> [U; 2] {
    [f(a[0]), f(a[1])]
}

/// Input stage: the core reads f32 audio.
pub(super) struct Quantize;

impl Stage for Quantize {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.audio = each(f.x, |v| f64::from(v as f32));
    }
}

/// Input stage: f32 audio times a gain (Kontakt Limiter).
pub(super) struct Scale(pub f32);

impl Stage for Scale {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.audio = each(f.x, |v| f64::from(v as f32 * self.0));
    }
}

/// Input stage: f32 audio driven plus the cores' 1e-18 / 1e-12 offset.
pub(super) struct Drive {
    pub gain: f32,
    pub offset: f32,
}

impl Stage for Drive {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.audio = each(f.x, |v| f64::from(self.gain * v as f32 + self.offset));
    }
}
