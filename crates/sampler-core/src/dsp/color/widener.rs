//! UVI Workstation 4.0.9 Widener as the color chain's stereo stage (engine 0x141669990,
//! prepare 0x141669740, shelf design 0x141649120): one channel delayed by
//! `|Delay| ms` whole frames (left for negative Delay), then mid/side with the side scaled by
//! `Width + 1` through a first-order high shelf, `L = (S + M) / 2`, `R = (M - S) / 2`
//! (model uvicolor/widener.py, checked against the emulated engine). The delay is a control
//! constant here, so the native's 100 ms delay glide never runs.
use super::super::Planar;
pub use sampler_ir::fx::color::Widener;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    width: f32,
    /// `(channel, frames)` of the delayed side.
    delay: Option<(usize, usize)>,
    b0: f32,
    b1: f32,
    a1: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    z: f32,
    position: usize,
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.z.is_finite()
    }
}

/// 0x141649120 with `lo = 1`, `hi = 10^(dB / 20)`: a first-order high shelf.
fn shelf(freq: f32, fs: f32, gain_db: f32) -> [f32; 3] {
    let w = f64::from(freq / fs).min(0.499);
    let hi = 10f64.powf(f64::from(gain_db) * 0.05);
    let r = 1.0 / if hi > 0.0 { hi } else { 1e-20 };
    let t = (w * std::f64::consts::PI).tan() / r.sqrt();
    let u = t * r;
    let n = 1.0 / (t + 1.0);
    let b0 = n * (u + 1.0) * hi;
    [b0 as f32, (((u - 1.0) / (u + 1.0)) * b0) as f32, ((t - 1.0) * n) as f32]
}

impl Engine {
    pub(super) fn new(w: Widener, rate: u32) -> Self {
        let fs = rate as f32;
        let frames = ((w.delay * 0.001) * fs).abs() as usize;
        let [b0, b1, a1] = shelf(w.shelf_hertz, fs, w.shelf_db);
        Self {
            width: w.width + 1.0,
            delay: (frames > 0 && w.delay != 0.0).then_some((usize::from(w.delay > 0.0), frames)),
            b0,
            b1,
            a1,
        }
    }

    /// Ring frames the delay needs.
    pub(super) fn frames(w: Widener, rate: u32) -> usize {
        ((w.delay * 0.001) * rate as f32).abs() as usize
    }

    pub(super) fn process(&self, state: &mut State, block: &mut Planar, len: usize, ring: &mut [[f64; 2]]) {
        if let Some((ch, frames)) = self.delay {
            let mut p = state.position % frames;
            for v in &mut block[ch][..len] {
                let x = *v as f32;
                *v = ring[p][ch];
                ring[p][ch] = f64::from(x);
                p = (p + 1) % frames;
            }
            state.position = p;
        }
        let [left, right] = block;
        for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
            let (x, y) = (*l as f32, *r as f32);
            let mid = y + x;
            let side = (x - y) * self.width;
            let s = (side * self.b0) + state.z;
            state.z = (side * self.b1) - (s * self.a1);
            *l = f64::from((s + mid) * 0.5);
            *r = f64::from((mid - s) * 0.5);
        }
    }
}
