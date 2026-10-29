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

/// Most channels a sample may have.
pub const MAX_CHANNELS: usize = 16;

/// How each source channel (row) reaches the left and right outputs.
pub type Mix = [[f32; 2]; MAX_CHANNELS];

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
    /// With `mix`, channel `c` adds `mix[c]` times its value to the outputs;
    /// without, mono plays on both sides and channels past the second are not
    /// played.
    // ponytail: scalar per-byte decode; SIMD per encoding when C10 shows it in a profile.
    pub(crate) fn decode(
        self,
        byte: impl Fn(usize) -> u8,
        first: usize,
        out: &mut [[f32; 2]],
        mix: Option<&Mix>,
    ) {
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
        let at = |n: usize, c: usize| value((first + n) * frame_bytes + c * width);
        emit(usize::from(self.channels), at, out, mix);
    }
}

/// Writes frame `n` of `out` from `value(n, channel)`. With `mix`, channel
/// `c` adds `mix[c]` times its value to the outputs; without, mono plays on
/// both sides and channels past the second are not played.
pub(crate) fn emit(
    channels: usize,
    value: impl Fn(usize, usize) -> f32,
    out: &mut [[f32; 2]],
    mix: Option<&Mix>,
) {
    if let Some(mix) = mix {
        let rows = mix.iter().take(channels);
        for (n, frame) in out.iter_mut().enumerate() {
            *frame = rows
                .clone()
                .enumerate()
                .fold([0.0; 2], |[l, r], (c, gain)| {
                    let v = value(n, c);
                    [l + v * gain[0], r + v * gain[1]]
                });
        }
        return;
    }
    for (n, frame) in out.iter_mut().enumerate() {
        let left = value(n, 0);
        let right = if channels >= 2 { value(n, 1) } else { left };
        *frame = [left, right];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(fmt: SampleFmt, bytes: &[u8], frames: usize) -> Vec<[f32; 2]> {
        let mut out = vec![[9.0; 2]; frames];
        fmt.decode(|i| bytes.get(i).copied().unwrap_or(0), 0, &mut out, None);
        out
    }

    #[test]
    fn a_mix_sums_each_channel_into_both_outputs_by_its_row() {
        let fmt = SampleFmt {
            encoding: Encoding::I16,
            channels: 3,
        };
        let bytes = [0x00, 0x40, 0x00, 0x20, 0x00, 0xC0]; // 0.5, 0.25, -0.5
        let mut mix = [[0.0; 2]; MAX_CHANNELS];
        mix[0] = [1.0, 0.0];
        mix[2] = [0.5, 1.0];
        let mut out = [[9.0; 2]];
        fmt.decode(
            |i| bytes.get(i).copied().unwrap_or(0),
            0,
            &mut out,
            Some(&mix),
        );
        assert_eq!(out, [[0.25, -0.5]]);
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
