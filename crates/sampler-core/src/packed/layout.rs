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
fn samples(frames: &[Frame], mono: bool) -> impl Iterator<Item = f32> + '_ {
    frames
        .iter()
        .flat_map(move |f| f.iter().take(if mono { 1 } else { 2 }).copied())
}
impl<'a> Packing<'a> {
    pub(super) fn analyze(frames: &'a [Frame]) -> Self {
        let mono = frames.iter().all(|[l, r]| l.to_bits() == r.to_bits());
        let exact = |scale: f32, max: f32| {
            samples(frames, mono).all(|s| {
                let q = s * scale;
                q.fract() == 0.
                    && (-max..max).contains(&q)
                    && ((q as i32) as f32 / scale).to_bits() == s.to_bits()
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
        let count = len * if self.mono { 1 } else { 2 };
        let data = match self.width {
            Width::I16 => {
                let mut data = Vec::with_capacity(count);
                data.extend(samples(self.frames, self.mono).map(|s| (s * I16) as i16));
                Data::I16(data.into_boxed_slice())
            }
            Width::I24 => {
                let mut data = Vec::with_capacity(count);
                data.extend(samples(self.frames, self.mono).map(|s| {
                    let [a, b, c, _] = ((s * I24) as i32).to_le_bytes();
                    [a, b, c]
                }));
                Data::I24(data.into_boxed_slice())
            }
            Width::F32 => {
                let mut data = Vec::with_capacity(count);
                data.extend(samples(self.frames, self.mono));
                Data::F32(data.into_boxed_slice())
            }
        };
        Packed {
            data,
            mono: self.mono,
            len,
        }
    }
}
