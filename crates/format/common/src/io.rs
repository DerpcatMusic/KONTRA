//! Positioned random-access I/O ([`ReadAt`]) and container slice / keystream
//! adapters shared across format readers and header probers.

use std::fs::File;
use std::io;

use kontra_ir as ir;

/// Positioned random-access reads without mutating a shared cursor.
///
/// Allows multi-threaded container readers (`.nkx`, `.nkr`, `.ufs`) and audio
/// header probers to share a single file handle or read from memory slices and
/// transformed container regions uniformly.
pub trait ReadAt {
    /// Reads up to `buf.len()` bytes starting at `offset`, returning the byte count read.
    ///
    /// # Errors
    /// Returns an [`io::Error`] if the underlying read fails.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;

    /// Reads exactly `buf.len()` bytes starting at `offset`.
    ///
    /// # Errors
    /// Returns [`io::ErrorKind::UnexpectedEof`] if fewer than `buf.len()` bytes are available,
    /// or forwards any underlying I/O error.
    fn read_exact_at(&self, mut offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.read_at(offset, buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "failed to fill whole buffer",
                    ));
                }
                Ok(n) => {
                    offset = offset.checked_add(n as u64).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "read offset overflow")
                    })?;
                    let Some(rest) = buf.get_mut(n..) else {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "short read slice overflow",
                        ));
                    };
                    buf = rest;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl<R: ReadAt + ?Sized> ReadAt for &R {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        (**self).read_at(offset, buf)
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        (**self).read_exact_at(offset, buf)
    }
}

impl ReadAt for File {
    #[cfg(unix)]
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        std::os::unix::fs::FileExt::read_at(self, buf, offset)
    }

    #[cfg(windows)]
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        // seek_read changes the cursor. Reopening the same object gives this
        // operation a private cursor without resolving the original path again.
        std::os::windows::fs::FileExt::seek_read(&reopen_for_read(self)?, buf, offset)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn reopen_for_read(file: &File) -> io::Result<File> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReOpenFile(handle: RawHandle, access: u32, share: u32, flags: u32) -> RawHandle;
    }
    const GENERIC_READ: u32 = 0x8000_0000;
    const SHARE_READ_WRITE_DELETE: u32 = 0x7;

    // SAFETY: file keeps the input handle live; ReOpenFile returns a new owned
    // handle or INVALID_HANDLE_VALUE. The new handle is synchronous.
    unsafe {
        let handle = ReOpenFile(
            file.as_raw_handle(),
            GENERIC_READ,
            SHARE_READ_WRITE_DELETE,
            0,
        );
        if handle as isize == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(File::from_raw_handle(handle))
        }
    }
}

impl ReadAt for [u8] {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let Ok(start) = usize::try_from(offset) else {
            return Ok(0);
        };
        let Some(avail) = self.get(start..) else {
            return Ok(0);
        };
        let n = avail.len().min(buf.len());
        if let (Some(dst), Some(src)) = (buf.get_mut(..n), avail.get(..n)) {
            dst.copy_from_slice(src);
        }
        Ok(n)
    }
}

impl ReadAt for Vec<u8> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.as_slice().read_at(offset, buf)
    }
}

/// A bounded `(offset, len)` window over an inner [`ReadAt`] reader.
#[derive(Clone, Debug)]
pub struct SliceReader<R> {
    inner: R,
    offset: u64,
    len: u64,
}

impl<R> SliceReader<R> {
    /// Wraps `inner` exposing bytes `[offset .. offset + len)` as `[0 .. len)`.
    #[must_use]
    pub const fn new(inner: R, offset: u64, len: u64) -> Self {
        Self { inner, offset, len }
    }

    /// Returns the length of this bounded slice in bytes.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Returns `true` when this slice has zero length.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the base offset inside the underlying reader.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

impl<R: ReadAt> ReadAt for SliceReader<R> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let max_read = self.len.saturating_sub(offset);
        let take = usize::try_from(max_read).map_or(buf.len(), |avail| avail.min(buf.len()));
        let abs = self
            .offset
            .checked_add(offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "slice offset overflow"))?;
        let Some(dst) = buf.get_mut(..take) else {
            return Ok(0);
        };
        self.inner.read_at(abs, dst)
    }
}

/// Applies an optional [`ir::Keystream`] transform to bytes read from an inner
/// [`ReadAt`] container region.
///
/// The transform is evaluated at `base_offset + offset` so block-restarted and
/// repeating keystreams decrypt random sub-reads identically to sequential reads.
pub struct TransformedReader<'a, R> {
    inner: R,
    base_offset: u64,
    keystream: Option<&'a ir::Keystream>,
}

impl<'a, R> TransformedReader<'a, R> {
    /// Wraps `inner` (whose offsets start at 0) and applies `keystream` at
    /// `base_offset + offset` on every read.
    #[must_use]
    pub const fn new(inner: R, base_offset: u64, keystream: Option<&'a ir::Keystream>) -> Self {
        Self {
            inner,
            base_offset,
            keystream,
        }
    }
}

impl<R: ReadAt> ReadAt for TransformedReader<'_, R> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read_at(offset, buf)?;
        if let (Some(keystream), Some(read_bytes)) = (self.keystream, buf.get_mut(..n)) {
            let abs = self.base_offset.checked_add(offset).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "keystream offset overflow")
            })?;
            keystream.apply(abs, read_bytes);
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    struct XorByte(u8);

    impl ir::ByteTransform for XorByte {
        fn apply(&self, offset: u64, bytes: &mut [u8]) {
            for (i, b) in bytes.iter_mut().enumerate() {
                *b ^= self.0.wrapping_add((offset + i as u64) as u8);
            }
        }
    }

    #[test]
    fn slice_reader_bounds_reads_to_window() {
        let data: Vec<u8> = (0..32u8).collect();
        let slice = SliceReader::new(data.as_slice(), 10, 6);
        assert_eq!(slice.len(), 6);
        assert!(!slice.is_empty());

        let mut buf = [0u8; 4];
        slice.read_exact_at(1, &mut buf).unwrap();
        assert_eq!(buf, [11, 12, 13, 14]);

        let mut tail = [0u8; 8];
        let n = slice.read_at(4, &mut tail).unwrap();
        assert_eq!(n, 2);
        assert_eq!(&tail[..2], &[14, 15]);

        assert_eq!(
            slice.read_exact_at(4, &mut [0u8; 3]).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn transformed_reader_applies_keystream_at_absolute_offset() {
        let ks = ir::Keystream::new(Arc::new(XorByte(0xA5)));
        let plain: Vec<u8> = (0..64u8).collect();
        let mut cipher = plain.clone();
        ks.apply(100, &mut cipher);

        let reader = TransformedReader::new(cipher.as_slice(), 100, Some(&ks));
        let mut out = [0u8; 16];
        reader.read_exact_at(20, &mut out).unwrap();
        assert_eq!(&out, &plain[20..36]);
    }
}

#[cfg(test)]
#[test]
fn positioned_file_read_leaves_sequential_cursor_unchanged() {
    use std::io::{Read, Seek};

    let path = std::env::temp_dir().join(format!("kontra-read-at-cursor-{}", std::process::id()));
    std::fs::write(&path, b"abcdef").unwrap();
    let mut file = File::open(&path).unwrap();
    file.seek(io::SeekFrom::Start(1)).unwrap();
    let mut buf = [0; 2];
    file.read_exact_at(3, &mut buf).unwrap();
    assert_eq!(&buf, b"de");
    file.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"bc");
    drop(file);
    std::fs::remove_file(path).unwrap();
}
