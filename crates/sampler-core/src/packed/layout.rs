//! Lossless packing analysis without allocating payload storage. The borrowed
//! source cannot change between sizing, budget reservation and encoding.
use super::{Data, Frame, I16, I24, Packed};
#[derive(Clone, Copy)]
enum Width {
    I16,
    I24,
    F32,
}
pub struct Packing<'a> {
    frames: &'a [Frame],
    mono: bool,
    width: Width,
}
/// Whether one stored sample reads back bit-exactly at `scale` (a power of two
/// within the format's range). Multiplying by the reciprocal is exact at a power
/// of two, so it equals the division it replaces. `&` keeps this branch-free.
#[inline(always)]
fn exact(s: f32, scale: f32) -> bool {
    let q = s * scale;
    let k = q as i32;
    let back = k as f32 * scale.recip();
    (k as f32 == q) & (-scale <= q) & (q < scale) & (back.to_bits() == s.to_bits())
}
/// Whether every stored sample (left channel only when mono, else L/R) is exact
/// at `scale`. Chunks are scanned without an early exit so each one vectorizes;
/// a failing chunk ends the scan, which keeps non-matching widths cheap.
#[inline(always)]
fn all_exact(frames: &[Frame], mono: bool, scale: f32) -> bool {
    const CHUNK: usize = 256;
    if mono {
        frames
            .chunks(CHUNK)
            .all(|c| c.iter().fold(true, |ok, f| ok & exact(f[0], scale)))
    } else {
        frames
            .as_flattened()
            .chunks(CHUNK)
            .all(|c| c.iter().fold(true, |ok, &s| ok & exact(s, scale)))
    }
}
/// The stored samples converted by `convert`, as one exact-size allocation.
#[inline(always)]
fn stored<T>(frames: &[Frame], mono: bool, convert: impl Fn(f32) -> T) -> Box<[T]> {
    let mut data = Vec::with_capacity(frames.len() * if mono { 1 } else { 2 });
    if mono {
        data.extend(frames.iter().map(|f| convert(f[0])));
    } else {
        data.extend(frames.as_flattened().iter().map(|&s| convert(s)));
    }
    data.into_boxed_slice()
}
impl<'a> Packing<'a> {
    pub(super) fn analyze(frames: &'a [Frame]) -> Self {
        let mono = frames.iter().all(|[l, r]| l.to_bits() == r.to_bits());
        // `k as f32 == q` is `q.fract() == 0` for in-range q (|q| < 2^23 here),
        // without the libm `truncf` call `fract` makes on the baseline target.
        // Exact widths are a boolean: the wide-ISA build agrees with the baseline.
        let width = if sampler_simd::dispatch(#[inline(always)] || all_exact(frames, mono, I16)) {
            Width::I16
        } else if sampler_simd::dispatch(#[inline(always)] || all_exact(frames, mono, I24)) {
            Width::I24
        } else {
            Width::F32
        };
        Self {
            frames,
            mono,
            width,
        }
    }
    pub fn bytes(&self) -> usize {
        self.frames.len()
            * (if self.mono { 1 } else { 2 })
            * match self.width {
                Width::I16 => 2,
                Width::I24 => 3,
                Width::F32 => 4,
            }
    }
    /// Allocate only after the caller has admitted `bytes()`. Each output
    /// vector reserves its exact sample count; no temporary f32 sample copy.
    pub fn finish(self) -> Packed {
        let len = self.frames.len();
        let (frames, mono) = (self.frames, self.mono);
        let data = sampler_simd::dispatch(#[inline(always)] || match self.width {
            Width::I16 => Data::I16(stored(frames, mono, |s| (s * I16) as i16)),
            Width::I24 => Data::I24(stored(frames, mono, |s| {
                let [a, b, c, _] = ((s * I24) as i32).to_le_bytes();
                [a, b, c]
            })),
            Width::F32 => Data::F32(stored(frames, mono, |s| s)),
        });
        Packed {
            data,
            mono: self.mono,
            len,
        }
    }
}
