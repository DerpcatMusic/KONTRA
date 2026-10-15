//! Source samples kept at their decoded width: 16- or 24-bit integers, mono
//! when both channels match, converted to f32 frames as they are read.

use crate::Frame;
mod layout;
pub use layout::Packing;

const I16: f32 = 32_768.0;
const I24: f32 = 8_388_608.0;

#[derive(Clone, Debug)]
enum Data {
    F32(Box<[f32]>),
    I16(Box<[i16]>),
    I24(Box<[[u8; 3]]>),
}

/// Frames stored in the narrowest form that converts back bit-exactly.
#[derive(Clone, Debug)]
pub struct Packed {
    data: Data,
    mono: bool,
    len: usize,
}

impl Packed {
    /// Pack `frames` losslessly: a frame reads back with identical bits.
    pub fn new(frames: &[Frame]) -> Self {
        Self::prepare(frames).finish()
    }
    /// Analyze lossless width without allocating a payload. Callers can reserve
    /// exact storage before encoding the same immutable borrowed frames.
    pub fn prepare(frames: &[Frame]) -> Packing<'_> { Packing::analyze(frames) }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        let samples = if self.mono { self.len } else { 2 * self.len };
        samples
            * match self.data {
                Data::F32(_) => 4,
                Data::I16(_) => 2,
                Data::I24(_) => 3,
            }
    }

    /// The frames as stored, when they are already stereo f32.
    pub(crate) fn frames(&self) -> Option<&[Frame]> {
        match &self.data {
            Data::F32(s) if !self.mono => Some(s.as_chunks::<2>().0),
            _ => None,
        }
    }

    pub(crate) fn frame(&self, index: usize) -> Frame {
        let mut out = [[0.0; 2]];
        self.copy(index, &mut out);
        out[0]
    }

    /// Convert frames from `start` into `out`.
    #[inline]
    pub(crate) fn copy(&self, start: usize, out: &mut [Frame]) {
        let channels = if self.mono { 1 } else { 2 };
        let range = channels * start..channels * (start + out.len());
        fn fill<T: Copy>(samples: &[T], mono: bool, out: &mut [Frame], f: impl Fn(T) -> f32) {
            if mono {
                for (frame, &s) in out.iter_mut().zip(samples) {
                    let x = f(s);
                    *frame = [x, x];
                }
            } else {
                for (frame, s) in out.iter_mut().zip(samples.as_chunks::<2>().0) {
                    *frame = [f(s[0]), f(s[1])];
                }
            }
        }
        match &self.data {
            Data::F32(s) => fill(&s[range], self.mono, out, |x| x),
            Data::I16(s) => fill(&s[range], self.mono, out, |x| f32::from(x) / I16),
            Data::I24(s) => fill(&s[range], self.mono, out, |[a, b, c]| {
                (i32::from_le_bytes([0, a, b, c]) >> 8) as f32 / I24
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_is_lossless_and_narrowest() {
        let cases: [(Vec<Frame>, usize); 4] = [
            ((-4..4).map(|i| [i as f32 / I16; 2]).collect(), 2 * 8),
            ((-4..4).map(|i| [i as f32 / I16, -1.0]).collect(), 2 * 16),
            ((-4..4).map(|i| [i as f32 / I24, 0.5]).collect(), 3 * 16),
            ((-4..4).map(|i| [i as f32 / 3.0, 0.5]).collect(), 4 * 16),
        ];
        for (frames, bytes) in cases {
            let packed = Packed::new(&frames);
            assert_eq!(packed.bytes(), bytes);
            let mut out = vec![[9.0; 2]; frames.len() - 2];
            packed.copy(1, &mut out);
            for (a, b) in out.iter().zip(&frames[1..]) {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
            }
            assert_eq!(packed.frame(7), frames[7]);
        }
    }
}
