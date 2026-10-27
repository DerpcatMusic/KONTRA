//! Packed sample encodings, decoded on read (05-STREAM.md §1 "Layout").

/// How one channel value is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// Signed 16-bit little endian.
    I16,
    /// Signed 24-bit little endian, packed in 3 bytes.
    I24,
    /// IEEE float little endian.
    F32,
}

impl Encoding {
    /// Bytes per channel value.
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::I24 => 3,
            Self::F32 => 4,
        }
    }
}

/// The stored layout of a sample: encoding and interleaved channel count, as recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SampleFmt {
    pub encoding: Encoding,
    pub channels: u8,
}

impl SampleFmt {
    #[must_use]
    pub const fn bytes_per_frame(self) -> usize {
        self.encoding.bytes() * self.channels as usize
    }

    /// Decodes frames `first..first + out.len()` of a page into stereo.
    /// `byte(i)` reads byte `i` of the page and returns 0 past its end.
    /// Mono plays on both sides; channels past the second are not played.
    // ponytail: scalar per-byte decode; SIMD per encoding when C10 shows it in a profile.
    pub(crate) fn decode(self, byte: impl Fn(usize) -> u8, first: usize, out: &mut [[f32; 2]]) {
        let width = self.encoding.bytes();
        let frame_bytes = self.bytes_per_frame();
        let value = |at: usize| -> f32 {
            match self.encoding {
                Encoding::I16 => f32::from(i16::from_le_bytes([byte(at), byte(at + 1)])) / 32_768.0,
                Encoding::I24 => {
                    let raw = i32::from_le_bytes([0, byte(at), byte(at + 1), byte(at + 2)]) >> 8;
                    raw as f32 / 8_388_608.0
                }
                Encoding::F32 => {
                    f32::from_le_bytes([byte(at), byte(at + 1), byte(at + 2), byte(at + 3)])
                }
            }
        };
        for (n, frame) in out.iter_mut().enumerate() {
            let at = (first + n) * frame_bytes;
            let left = value(at);
            let right = if self.channels >= 2 {
                value(at + width)
            } else {
                left
            };
            *frame = [left, right];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(fmt: SampleFmt, bytes: &[u8], frames: usize) -> Vec<[f32; 2]> {
        let mut out = vec![[9.0; 2]; frames];
        fmt.decode(|i| bytes.get(i).copied().unwrap_or(0), 0, &mut out);
        out
    }

    #[test]
    fn i16_stereo_decodes_both_sides() {
        let fmt = SampleFmt {
            encoding: Encoding::I16,
            channels: 2,
        };
        let bytes = [0x00, 0x40, 0x00, 0xC0]; // 16384, -16384
        assert_eq!(decode(fmt, &bytes, 1), vec![[0.5, -0.5]]);
    }

    #[test]
    fn i24_mono_plays_on_both_sides_and_keeps_its_sign() {
        let fmt = SampleFmt {
            encoding: Encoding::I24,
            channels: 1,
        };
        let bytes = [0x00, 0x00, 0xC0, 0x00, 0x00, 0x40]; // -4194304, 4194304
        assert_eq!(decode(fmt, &bytes, 2), vec![[-0.5, -0.5], [0.5, 0.5]]);
    }

    #[test]
    fn f32_round_trips_and_reads_past_the_end_as_silence() {
        let fmt = SampleFmt {
            encoding: Encoding::F32,
            channels: 1,
        };
        let bytes = 0.25f32.to_le_bytes();
        assert_eq!(decode(fmt, &bytes, 2), vec![[0.25, 0.25], [0.0, 0.0]]);
    }
}
