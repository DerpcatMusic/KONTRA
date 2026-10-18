// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::mem;
use std::vec::Vec;

use symphonia_core::audio::{AudioBuffer, Signal};
use symphonia_core::checksum::Md5;
use symphonia_core::errors::{decode_error, Result};
use symphonia_core::io::Monitor;

// The conversion dispatcher retains the checked width as a type. Invalid
// frame widths cannot reach a buffer allocation, projection, or MD5 mutation.
#[derive(Clone, Copy)]
enum ByteWidth { One, Two, Three, Four }
impl ByteWidth {
    fn from_bits(bps: u32) -> Result<Self> {
        match bps {
            1..=8 => Ok(Self::One),
            9..=16 => Ok(Self::Two),
            17..=24 => Ok(Self::Three),
            25..=32 => Ok(Self::Four),
            _ => decode_error("flac: invalid checksum sample bit width"),
        }
    }
    fn bytes(self) -> usize {
        match self { Self::One => 1, Self::Two => 2, Self::Three => 3, Self::Four => 4 }
    }
}

/// `Validator` computes the MD5 checksum of an audio stream taking into account the peculiarities
/// of FLAC's MD5 validation scheme.
#[derive(Default)]
pub struct Validator {
    state: Md5,
    buf: Vec<u8>,
}

impl Validator {
    /// Processes the audio buffer and updates the state of the validator.
    pub fn update(&mut self, buf: &AudioBuffer<i32>, bps: u32) -> Result<()> {
        // The MD5 checksum is calculated on a buffer containing interleaved audio samples of the
        // correct sample width. While FLAC can encode and decode samples of arbitrary bit widths,
        // the samples in the buffer must be a multiple of 8-bits.
        //
        // Additionally, Symphonia's AudioBuffer's are in planar format, and the FLAC decoder works
        // internally on signed 32-bit samples exclusively.
        //
        // Therefore, to compute the checksum, the audio buffer samples must truncated to the
        // correct bit-width, interlaced, and converted to a little-endian byte buffer. The byte
        // buffer can then be passed to the MD5 algorithm for hashing.

        // Round the sample bit width up to the nearest byte.
        let width = ByteWidth::from_bits(bps)?;
        let n_channels = buf.spec().channels.count();
        let n_frames = buf.frames();
        if n_channels == 0 {
            return decode_error("flac: checksum audio has no channels");
        }
        // No validator-owned state has changed when geometry is refused.
        let buf_len = match n_channels.checked_mul(n_frames)
            .and_then(|n| n.checked_mul(width.bytes())) {
            Some(n) if n <= isize::MAX as usize => n,
            _ => return decode_error("flac: checksum sample byte count overflow"),
        };

        // Ensure the byte buffer length can accomodate all the samples.
        if self.buf.len() < buf_len {
            self.buf.resize(buf_len, 0u8);
        }

        // Populate the hash buffer with samples truncated to the correct width. A &[u8] slice of
        // all the samples in hash buffer will be returned.
        let buf_slice = match width {
            ByteWidth::One => copy_as_i8(buf, &mut self.buf, n_channels, n_frames),
            ByteWidth::Two => copy_as_i16(buf, &mut self.buf, n_channels, n_frames),
            ByteWidth::Three => copy_as_i24(buf, &mut self.buf, n_channels, n_frames),
            ByteWidth::Four => copy_as_i32(buf, &mut self.buf, n_channels, n_frames),
        };

        // Update the MD5 state.
        self.state.process_buf_bytes(buf_slice);
        Ok(())
    }

    /// Get the checksum.
    pub fn md5(&mut self) -> [u8; 16] {
        self.state.md5()
    }
}

fn copy_as_i24<'a>(
    samples: &AudioBuffer<i32>,
    buf: &'a mut [u8],
    n_channels: usize,
    n_frames: usize,
) -> &'a [u8] {
    const SIZE_OF_I24: usize = 24 / 8;

    for ch in 0..n_channels {
        for (out, sample) in
            buf.chunks_exact_mut(SIZE_OF_I24).skip(ch).step_by(n_channels).zip(samples.chan(ch))
        {
            out.copy_from_slice(&sample.to_le_bytes()[0..SIZE_OF_I24]);
        }
    }

    &buf[..n_channels * n_frames * SIZE_OF_I24]
}

macro_rules! copy_as {
    ($name:ident, $type:ty) => {
        fn $name<'a>(
            samples: &AudioBuffer<i32>,
            buf: &'a mut [u8],
            n_channels: usize,
            n_frames: usize,
        ) -> &'a [u8] {
            for ch in 0..n_channels {
                for (out, sample) in buf
                    .chunks_exact_mut(mem::size_of::<$type>())
                    .skip(ch)
                    .step_by(n_channels)
                    .zip(samples.chan(ch))
                {
                    out.copy_from_slice(&(*sample as $type).to_le_bytes());
                }
            }

            &buf[..n_channels * n_frames * mem::size_of::<$type>()]
        }
    };
}

copy_as!(copy_as_i8, i8);
copy_as!(copy_as_i16, i16);
copy_as!(copy_as_i32, i32);

#[cfg(test)]
mod checked_width_tests {
    use super::*;
    use symphonia_core::audio::{Channels, SignalSpec};

    fn samples() -> AudioBuffer<i32> {
        let mut audio = AudioBuffer::new(3, SignalSpec::new(48000,
            Channels::FRONT_LEFT | Channels::FRONT_RIGHT));
        audio.render_reserved(Some(3));
        audio.chan_mut(0).copy_from_slice(&[-1, 0, -1]);
        audio.chan_mut(1).copy_from_slice(&[0, -1, 0]);
        audio
    }

    #[test]
    fn every_valid_bit_width_hashes_the_independent_interleaved_byte_sequence() {
        let audio = samples();
        for bps in 1..=32 {
            let bytes = ((bps + 7) / 8) as usize;
            let mut reference = Vec::new();
            for value in [-1i32, 0, 0, -1, -1, 0] {
                reference.extend_from_slice(&value.to_le_bytes()[..bytes]);
            }
            let mut expected = Md5::default();
            expected.process_buf_bytes(&reference);
            let mut validator = Validator::default();
            validator.update(&audio, bps).unwrap();
            assert_eq!(validator.md5(), expected.md5());
        }
    }

    #[test]
    fn invalid_width_never_mutates_hash_bytes_or_capacity_before_or_after_valid_audio() {
        let audio = samples();
        for warm in [false, true] {
            let mut validator = Validator::default();
            if warm { validator.update(&audio, 24).unwrap(); }
            let before = validator.md5();
            let bytes = validator.buf.clone(); let capacity = validator.buf.capacity();
            for invalid in [0, 33, u32::MAX] {
                assert!(matches!(validator.update(&audio, invalid),
                    Err(symphonia_core::errors::Error::DecodeError(_))));
                assert_eq!(validator.md5(), before);
                assert_eq!(validator.buf, bytes);
                assert_eq!(validator.buf.capacity(), capacity);
            }
            // Rejection did not poison or fabricate a valid checksum update.
            validator.update(&audio, 24).unwrap();
            let mut reference = Validator::default();
            if warm { reference.update(&audio, 24).unwrap(); }
            reference.update(&audio, 24).unwrap();
            assert_eq!(validator.md5(), reference.md5());
        }
    }
}
