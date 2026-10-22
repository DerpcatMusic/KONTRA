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
/// Whether every stored sample (left channel only when mono, else L/R) passes `ok`.
fn all_samples(frames: &[Frame], mono: bool, ok: impl Fn(f32) -> bool) -> bool {
    if mono {
        frames.iter().all(|f| ok(f[0]))
    } else {
        frames.as_flattened().iter().all(|&s| ok(s))
    }
}
/// The stored samples converted by `convert`, as one exact-size allocation.
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
        let exact = |scale: f32, max: f32| {
            all_samples(frames, mono, |s| {
                let q = s * scale;
                let k = q as i32;
                k as f32 == q
                    && (-max..max).contains(&q)
                    && (k as f32 / scale).to_bits() == s.to_bits()
            })
        };
        let width = if exact(I16, I16) {
            Width::I16
        } else if exact(I24, I24) {
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
        let data = match self.width {
            Width::I16 => Data::I16(stored(frames, mono, |s| (s * I16) as i16)),
            Width::I24 => Data::I24(stored(frames, mono, |s| {
                let [a, b, c, _] = ((s * I24) as i32).to_le_bytes();
                [a, b, c]
            })),
            Width::F32 => Data::F32(stored(frames, mono, |s| s)),
        };
        Packed {
            data,
            mono: self.mono,
            len,
        }
    }
}
