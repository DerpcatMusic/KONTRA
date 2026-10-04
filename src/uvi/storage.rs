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
    pub(crate) fn cache_tag(&self) -> Result<u8> {
        Ok(match self.pcm {
            Pcm::F32(_) => 1,
            Pcm::I16(_) => 2,
            Pcm::I24(_, _) => 3,
            Pcm::Packed(_) => anyhow::bail!("Unsupported cached PCM"),
        })
    }

    /// Bounded scratch; neither export nor import retains a second full payload.
    pub(crate) fn write_cache(
        &self,
        writer: &mut impl std::io::Write,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<()> {
        use std::io::Write;
        fn words<const N: usize, T>(
            values: &[T],
            writer: &mut impl Write,
            stop: Option<&std::sync::atomic::AtomicBool>,
            encode: impl Fn(&T) -> [u8; N],
        ) -> Result<()> {
            let mut scratch = [0u8; 65536];
            for chunk in values.chunks(scratch.len() / N) {
                super::sample::check_cancel(stop)?;
                for (word, v) in scratch.chunks_exact_mut(N).zip(chunk) {
                    word.copy_from_slice(&encode(v));
                }
                writer.write_all(&scratch[..chunk.len() * N])?;
                super::sample::check_cancel(stop)?;
            }
            Ok(())
        }
        match &self.pcm {
            Pcm::F32(pairs) => words(pairs.as_flattened(), writer, stop, |v| {
                v.to_bits().to_le_bytes()
            }),
            Pcm::I16(values) => words(values, writer, stop, |v| v.to_le_bytes()),
            Pcm::I24(high, low) => {
                words(high, writer, stop, |v| v.to_le_bytes())?;
                for chunk in low.chunks(65536) {
                    super::sample::check_cancel(stop)?;
                    writer.write_all(chunk)?;
                    super::sample::check_cancel(stop)?;
                }
                Ok(())
            }
            Pcm::Packed(_) => anyhow::bail!("Unsupported cached PCM"),
        }
    }

    /// Reconstruct exact packed planes into the existing owned PCM types.
    pub(crate) fn cache_read(
        reader: &mut impl std::io::Read,
        tag: u8,
        samples: usize,
        len: usize,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<Self> {
        use std::io::Read;
        ensure!(
            samples <= MAX_SOURCE_BYTES / 4,
            "Cache decoded sample bound"
        );
        let padded = samples
            .checked_add(samples % 2)
            .context("Cache count overflow")?;
        let width = match tag {
            1 => 4,
            2 => 2,
            3 => 3,
            _ => anyhow::bail!("Unsupported cache storage"),
        };
        ensure!(
            padded.checked_mul(width) == Some(len),
            "Cache packed geometry mismatch"
        );
        let mut scratch = [0u8; 65536];
        let pcm = match tag {
            1 => {
                let mut values = Vec::new();
                values.try_reserve_exact(padded / 2)?;
                for start in (0..padded / 2).step_by(scratch.len() / 8) {
                    let count = (padded / 2 - start).min(scratch.len() / 8);
                    super::sample::check_cancel(stop)?;
                    reader.read_exact(&mut scratch[..count * 8])?;
                    super::sample::check_cancel(stop)?;
                    for word in scratch[..count * 8].chunks_exact(8) {
                        let a = f32::from_bits(u32::from_le_bytes(word[..4].try_into()?));
                        let b = f32::from_bits(u32::from_le_bytes(word[4..].try_into()?));
                        ensure!(a.is_finite() && b.is_finite(), "Cache nonfinite PCM");
                        values.push([a, b]);
                    }
                }
                Pcm::F32(values.into_boxed_slice())
            }
            2 | 3 => {
                let mut high = Vec::new();
                high.try_reserve_exact(padded)?;
                for start in (0..padded).step_by(scratch.len() / 2) {
                    let count = (padded - start).min(scratch.len() / 2);
                    super::sample::check_cancel(stop)?;
                    reader.read_exact(&mut scratch[..count * 2])?;
                    super::sample::check_cancel(stop)?;
                    high.extend(
                        scratch[..count * 2]
                            .chunks_exact(2)
                            .map(|b| i16::from_le_bytes([b[0], b[1]])),
                    );
                }
                if tag == 2 {
                    Pcm::I16(high.into_boxed_slice())
                } else {
                    let mut low = Vec::new();
                    low.try_reserve_exact(padded)?;
                    for start in (0..padded).step_by(scratch.len()) {
                        let count = (padded - start).min(scratch.len());
                        super::sample::check_cancel(stop)?;
                        reader.read_exact(&mut scratch[..count])?;
                        super::sample::check_cancel(stop)?;
                        low.extend_from_slice(&scratch[..count]);
                    }
                    Pcm::I24(high.into_boxed_slice(), low.into_boxed_slice())
                }
            }
            _ => unreachable!(),
        };
        let result = Self { pcm, samples };
        if samples % 2 != 0 {
            let padding = match &result.pcm {
                Pcm::F32(v) => v.last().unwrap()[1].to_bits() == 0,
                Pcm::I16(v) => *v.last().unwrap() == 0,
                Pcm::I24(h, l) => *h.last().unwrap() == 0 && *l.last().unwrap() == 0,
                _ => false,
            };
            ensure!(padding, "Cache odd padding differs");
        }
        Ok(result)
    }

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
    fn owned_cache_planes_roundtrip_and_validate_before_use() {
        use std::{io::Cursor, sync::atomic::AtomicBool};
        for values in [
            vec![0.5, -0.0, 0.25],
            vec![0.5, 0.25, 0.75],
            vec![1.0 / 8388608.0, 0.25, 0.75],
        ] {
            let original = Storage::from_f32(values).unwrap();
            let tag = original.cache_tag().unwrap();
            let mut bytes = Vec::new();
            original.write_cache(&mut bytes, None).unwrap();
            let restored = Storage::cache_read(
                &mut Cursor::new(&bytes),
                tag,
                original.len(),
                bytes.len(),
                None,
            )
            .unwrap();
            assert_eq!(restored.bytes(), original.bytes());
            assert!(
                restored
                    .iter()
                    .zip(original.iter())
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
            assert!(
                Storage::cache_read(
                    &mut Cursor::new(&bytes[..bytes.len() - 1]),
                    tag,
                    original.len(),
                    bytes.len(),
                    None
                )
                .is_err()
            );
            let stop = AtomicBool::new(true);
            assert!(
                Storage::cache_read(
                    &mut Cursor::new(&bytes),
                    tag,
                    original.len(),
                    bytes.len(),
                    Some(&stop)
                )
                .unwrap_err()
                .is::<super::super::sample::LoadCancelled>()
            );
        }
        let mut nonfinite =
            std::io::Cursor::new([f32::NAN.to_bits().to_le_bytes(), 0u32.to_le_bytes()].concat());
        assert!(Storage::cache_read(&mut nonfinite, 1, 2, 8, None).is_err());
        assert!(Storage::cache_read(&mut Cursor::new([0, 0, 1, 0]), 2, 1, 4, None).is_err());
        assert!(
            Storage::cache_read(&mut Cursor::new([]), 2, MAX_SOURCE_BYTES / 4 + 1, 0, None)
                .is_err()
        );
    }

    #[test]
    fn owned_cache_read_preserves_real_io_error_during_stop() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Fail<'a>(&'a AtomicBool);
        impl std::io::Read for Fail<'_> {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.0.store(true, Ordering::Release);
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "authored cache read failure",
                ))
            }
        }
        let stop = AtomicBool::new(false);
        let error = Storage::cache_read(&mut Fail(&stop), 2, 2, 4, Some(&stop)).unwrap_err();
        assert!(!error.is::<super::super::sample::LoadCancelled>());
        assert_eq!(error.to_string(), "authored cache read failure");
    }

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
