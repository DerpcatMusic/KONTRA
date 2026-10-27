//! Where sample bytes come from. Format crates implement [`Source`] (WAV, NCW,
//! monoliths, UFS); the store never knows which format it reads.

use std::io;

use crate::fmt::SampleFmt;

/// A readable sample in its stored encoding. Called on loader and I/O threads only.
pub trait Source: Send + Sync {
    /// Fills `out` with frames `first..` in the sample's [`SampleFmt`], packed.
    /// `out.len()` is a whole number of frames; a short sample leaves the tail zeroed.
    fn read(&self, first: u64, out: &mut [u8]) -> io::Result<()>;
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
