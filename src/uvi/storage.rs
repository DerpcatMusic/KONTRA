//! Exact resident PCM using the sampler's existing 16/24-bit packing.
//! Storage pairs are not audio channels: scalar interleaved order is unchanged.
//! Decode and verify the source checksum before constructing this storage.

use crate::audio::Pcm;
use anyhow::{Context, Result, ensure};
use std::{fmt, ops::Range};

const MAX_SOURCE_BYTES: usize = 256 << 20;

pub struct Storage {
    pcm: Pcm,
    samples: usize,
}

impl fmt::Debug for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Storage")
            .field("samples", &self.samples)
            .field("resident_bytes", &self.bytes())
            .finish()
    }
}

impl Storage {
    pub fn from_f32(mut values: Vec<f32>) -> Result<Self> {
        ensure!(
            values.len() <= MAX_SOURCE_BYTES / size_of::<f32>(),
            "UVI PCM exceeds storage input limit"
        );
        // One bounded pass validates finiteness and preserves floating -0.
        let mut negative_zero = false;
        for chunk in values.chunks(256) {
            let (finite, negative) =
                chunk
                    .iter()
                    .fold((true, false), |(finite, negative), value| {
                        (
                            finite & value.is_finite(),
                            negative | (value.to_bits() == 0x8000_0000),
                        )
                    });
            ensure!(finite, "Nonfinite UVI PCM");
            negative_zero |= negative;
        }
        let samples = values.len();
        if samples % 2 != 0 {
            values.try_reserve_exact(1)?;
            values.push(0.);
        }
        let (pairs, remainder) = values.as_chunks::<2>();
        debug_assert!(remainder.is_empty());
        let pcm = if negative_zero {
            Pcm::F32(pairs.into())
        } else {
            Pcm::pack(pairs, false)
        };
        Ok(Self { pcm, samples })
    }

    /// Scalar count, excluding the single padding scalar of an odd-length input.
    pub fn len(&self) -> usize {
        self.samples
    }

    pub fn is_empty(&self) -> bool {
        self.samples == 0
    }

    /// Actual PCM allocation, including odd-length padding.
    pub fn bytes(&self) -> usize {
        self.pcm.bytes()
    }

    /// Exact random access; no disk I/O, allocation, locking, or channel folding.
    #[inline]
    pub fn value(&self, index: usize) -> Option<f32> {
        if index >= self.samples {
            return None;
        }
        Some(match &self.pcm {
            Pcm::F32(pairs) => pairs[index / 2][index % 2],
            Pcm::I16(values) => f32::from(values[index]) / 32768.,
            Pcm::I24(high, low) => {
                let integer = (i32::from(high[index]) << 8) | i32::from(low[index]);
                integer as f32 / 8388608.
            }
            Pcm::Packed(_) => {
                let mut pair = [[0.; 2]; 1];
                if !self.pcm.decode(index / 2, &mut pair) {
                    return None;
                }
                pair[0][index % 2]
            }
        })
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        (0..self.samples).map(|index| self.value(index).expect("validated PCM storage bounds"))
    }

    pub fn read(&self, range: Range<usize>, output: &mut [f32]) -> Result<()> {
        ensure!(
            range.start <= range.end && range.end <= self.samples,
            "UVI PCM range outside storage"
        );
        ensure!(
            output.len() == range.end - range.start,
            "UVI PCM output length differs from range"
        );
        for (index, value) in range.zip(output) {
            *value = self.value(index).context("Invalid UVI PCM storage")?;
        }
        Ok(())
    }

    /// Materialize only when an offline consumer needs a contiguous float buffer.
    pub fn to_vec(&self) -> Result<Vec<f32>> {
        let mut output = Vec::new();
        output.try_reserve_exact(self.samples)?;
        output.extend(self.iter());
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_covers_chunk_boundaries_and_preserves_signed_zero() {
        for len in [255usize, 256, 257, 513] {
            let mut values = vec![0.5; len];
            values[len - 1] = -0.0;
            let stored = Storage::from_f32(values.clone()).unwrap();
            assert_eq!(stored.bytes(), len.next_multiple_of(2) * size_of::<f32>());
            assert!(
                stored
                    .iter()
                    .zip(&values)
                    .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
            );
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                values[len - 1] = invalid;
                assert_eq!(
                    Storage::from_f32(values.clone()).unwrap_err().to_string(),
                    "Nonfinite UVI PCM"
                );
            }
        }
    }

    #[test]
    fn packed_pcm_retains_every_channel_and_exact_random_reads() {
        for channels in [1, 2, 6, 10, 12] {
            let original: Vec<f32> = (0..33 * channels)
                .map(|index| (index as i16 - 100) as f32 / 32768.)
                .collect();
            let stored = Storage::from_f32(original.clone()).unwrap();
            assert_eq!(stored.len(), original.len());
            assert_eq!(stored.bytes(), original.len().next_multiple_of(2) * 2);
            assert_eq!(stored.to_vec().unwrap(), original);
            for frame in [32, 1, 17, 0, 31] {
                let range = frame * channels..(frame + 1) * channels;
                let mut output = vec![0.; channels];
                stored.read(range.clone(), &mut output).unwrap();
                assert_eq!(output, original[range]);
            }
            assert!(stored.value(original.len()).is_none());
            let mut untouched = [7.; 1];
            assert!(
                stored
                    .read(original.len()..original.len() + 1, &mut untouched)
                    .is_err()
            );
            assert_eq!(untouched, [7.]);
        }
        let exact_24: Vec<f32> = [-8388608, -1, 0, 1, 8388607]
            .into_iter()
            .map(|value| value as f32 / 8388608.)
            .collect();
        let stored = Storage::from_f32(exact_24.clone()).unwrap();
        assert_eq!(stored.bytes(), exact_24.len().next_multiple_of(2) * 3);
        assert_eq!(stored.to_vec().unwrap(), exact_24);
        for original in [vec![0.12345679, 1.5, -0.4], vec![-0., 0., 0.25]] {
            let stored = Storage::from_f32(original.clone()).unwrap();
            assert!(
                stored
                    .iter()
                    .zip(&original)
                    .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
            );
            assert_eq!(stored.bytes(), original.len().next_multiple_of(2) * 4);
        }
        assert!(Storage::from_f32(vec![f32::NAN]).is_err());
        assert!(Storage::from_f32(vec![f32::INFINITY]).is_err());
        assert!(Storage::from_f32(Vec::new()).unwrap().is_empty());
    }
}
