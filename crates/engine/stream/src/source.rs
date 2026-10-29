//! Where sample bytes come from. Format crates implement [`Source`] (WAV, NCW,
//! monoliths, UFS); the store never knows which format it reads.

use std::io;
use std::ops::Range;

use crate::fmt::{Encoding, SampleFmt};

/// How a codec's blocks decode, for pages kept in memory as stored: NCW
/// blocks of [`ncw::SAMPLES_PER_BLOCK`] frames, one or two channels.
#[derive(Clone, Debug)]
pub struct Packing {
    pub(crate) header: ncw::NcwHeader,
    pub(crate) format: ncw::SampleFormat,
    pub(crate) encoding: Encoding,
}

impl Packing {
    /// A decoded value as the PCM path would play it.
    pub(crate) fn value(&self, v: i32) -> f32 {
        match (self.encoding, self.format) {
            (Encoding::I16, _) => f32::from(v as i16) / 32_768.0,
            (Encoding::I24, _) => v as f32 / 8_388_608.0,
            (Encoding::F32, ncw::SampleFormat::Float) => f32::from_bits(v as u32),
            (Encoding::F32, ncw::SampleFormat::Pcm) => {
                v as f32 / 2f32.powi(i32::from(self.header.bits_per_sample) - 1)
            }
        }
    }
}

/// A readable sample in its stored encoding. Called on loader and I/O threads only.
pub trait Source: Send + Sync {
    /// Fills `out` with frames `first..` in the sample's [`SampleFmt`], packed.
    /// `out.len()` is a whole number of frames; a short sample leaves the tail zeroed.
    fn read(&self, first: u64, out: &mut [u8]) -> io::Result<()>;

    /// How its stored blocks decode, when pinned pages may keep them as
    /// stored (05-STREAM.md §1: heads below Kontakt's footprint).
    fn packing(&self) -> Option<&Packing> {
        None
    }

    /// Its preload is in memory: later reads stream through it in order.
    fn loaded(&self) {}

    /// The stored bytes of blocks `blocks`, and where each block starts in
    /// them (one more entry: the end).
    ///
    /// # Errors
    /// When it has no [`Source::packing`] or the read fails.
    fn read_blocks(&self, blocks: Range<usize>) -> io::Result<(Vec<u8>, Vec<u32>)> {
        let _ = blocks;
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// A sample held in memory, already packed. For tests and synthetic fixtures.
pub struct MemSource {
    bytes: Vec<u8>,
    frame_bytes: usize,
}

impl MemSource {
    #[must_use]
    pub fn new(fmt: SampleFmt, bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            frame_bytes: fmt.bytes_per_frame(),
        }
    }
}

impl Source for MemSource {
    fn read(&self, first: u64, out: &mut [u8]) -> io::Result<()> {
        let start = usize::try_from(first)
            .unwrap_or(usize::MAX)
            .saturating_mul(self.frame_bytes);
        let src = self.bytes.get(start..).unwrap_or_default();
        let n = src.len().min(out.len());
        let (head, tail) = out.split_at_mut(n);
        head.copy_from_slice(src.get(..n).unwrap_or_default());
        tail.fill(0);
        Ok(())
    }
}
