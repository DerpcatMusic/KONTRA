//! Source samples kept at their decoded width: 16- or 24-bit integers, mono
//! when both channels match, converted to f32 frames as they are read.

use crate::Frame;
mod compressed;
use compressed::Compressed;

const I16: f32 = 32_768.0;
const I24: f32 = 8_388_608.0;

#[derive(Clone, Debug)]
enum Data {
    F32(Box<[f32]>),
    Compressed(Compressed),
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
        Self::with_compression(frames, true)
    }

    pub(crate) fn with_compression(frames: &[Frame], compress: bool) -> Self {
        let mono = frames.iter().all(|[l, r]| l.to_bits() == r.to_bits());
        let samples: Vec<f32> = if mono {
            frames.iter().map(|f| f[0]).collect()
        } else {
            frames.as_flattened().to_vec()
        };
        let exact = |scale: f32, max: f32| {
            samples.iter().all(|&s| {
                let q = s * scale;
                q.fract() == 0.0
                    && (-max..max).contains(&q)
                    && ((q as i32) as f32 / scale).to_bits() == s.to_bits()
            })
        };
        let scale = if exact(I16, I16) {
            Some(I16)
        } else if exact(I24, I24) {
            Some(I24)
        } else {
            None
        };
        if compress
            && let Some(scale) = scale
            && let Some(packed) = Compressed::new(frames, scale)
            && packed.bytes() < samples.len() * if scale == I16 { 2 } else { 3 }
        {
            return Self {
                data: Data::Compressed(packed),
                mono: false,
                len: frames.len(),
            };
        }
        let data = if scale == Some(I16) {
            Data::I16(samples.iter().map(|&s| (s * I16) as i16).collect())
        } else if scale == Some(I24) {
            Data::I24(
                samples
                    .iter()
                    .map(|&s| {
                        let [a, b, c, _] = ((s * I24) as i32).to_le_bytes();
                        [a, b, c]
                    })
                    .collect(),
            )
        } else {
            Data::F32(samples.into_boxed_slice())
        };
        Self {
            data,
            mono,
            len: frames.len(),
        }
    }

    pub(crate) fn predictive(&self) -> bool {
        matches!(self.data, Data::Compressed(_))
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        if let Data::Compressed(packed) = &self.data {
            return packed.bytes();
        }
        let samples = if self.mono { self.len } else { 2 * self.len };
        samples
            * match self.data {
                Data::F32(_) => 4,
                Data::Compressed(_) => unreachable!(),
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
            Data::Compressed(packed) => {
                assert!(packed.decode(start, out));
            }
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
    fn predictive_preload_blocks_are_lossless_and_smaller_than_native_pcm() {
        let frames: Vec<Frame> = (0..4097)
            .map(|i| [(i - 2048) as f32 / I24, (1024 - i) as f32 / I24])
            .collect();
        let packed = Packed::new(&frames);
        assert!(
            packed.bytes() < frames.len() * 3,
            "v1 prediction must halve this smooth stereo preload"
        );
        for start in [0, 1, 63, 64, 65, 4030, 4096] {
            let len = (frames.len() - start).min(67);
            let mut output = vec![[0.; 2]; len];
            packed.copy(start, &mut output);
            for (actual, expected) in output.iter().zip(&frames[start..]) {
                assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
            }
        }
    }

    #[test]
    fn predictive_nonzero_residuals_and_native_extremes_round_trip() {
        for scale in [I16, I24] {
            let frames: Vec<Frame> = (0..4097)
                .map(|i| {
                    let q = ((i * i / 64) % 10001) - 5000;
                    [q as f32 / scale, -q as f32 / scale]
                })
                .collect();
            let packed = Packed::new(&frames);
            assert!(packed.predictive());
            let mut output = vec![[0.; 2]; frames.len()];
            packed.copy(0, &mut output);
            assert_eq!(output, frames);
            let extremes = [[-1., (scale - 1.) / scale]; 130];
            let packed = Packed::new(&extremes);
            let mut output = [[0.; 2]; 130];
            packed.copy(0, &mut output);
            assert_eq!(output, extremes);
        }
    }

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
            assert!(packed.bytes() <= bytes);
            assert_eq!(Packed::with_compression(&frames, false).bytes(), bytes);
            let mut out = vec![[9.0; 2]; frames.len() - 2];
            packed.copy(1, &mut out);
            for (a, b) in out.iter().zip(&frames[1..]) {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
            }
            assert_eq!(packed.frame(7), frames[7]);
        }
    }
}
