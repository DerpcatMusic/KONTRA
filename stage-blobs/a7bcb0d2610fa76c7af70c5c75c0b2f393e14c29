//! Application bytes read during UVI bank preparation and owned PCM-cache reads.
//! Includes OS cache hits; this is not physical disk utilization or DFD activity.
use std::io::{self, Read};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) static READ_BYTES: AtomicU64 = AtomicU64::new(0);

pub(super) struct CountedRead<'a, R> {
    inner: R,
    counter: &'a AtomicU64,
}

impl<R> CountedRead<'static, R> {
    pub(super) fn new(inner: R) -> Self {
        Self { inner, counter: &READ_BYTES }
    }
}

impl<R: Read> Read for CountedRead<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(bytes)?;
        self.counter.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_exact_counts_partial_success_before_eof() {
        let counter = AtomicU64::new(0);
        let mut reader = CountedRead { inner: &b"abc"[..], counter: &counter };
        let mut bytes = [0; 4];
        assert_eq!(reader.read_exact(&mut bytes).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(counter.load(Ordering::Relaxed), 3);
        assert_eq!(&bytes[..3], b"abc");
        assert_eq!(reader.read(&mut bytes).unwrap(), 0);
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn interrupted_and_failed_reads_are_not_counted() {
        struct InterruptedOnce(bool);
        impl Read for InterruptedOnce {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                if !self.0 {
                    self.0 = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                if bytes.is_empty() { return Ok(0); }
                bytes[0] = 42;
                Ok(1)
            }
        }
        let counter = AtomicU64::new(0);
        let mut reader = CountedRead { inner: InterruptedOnce(false), counter: &counter };
        let mut bytes = [0; 2];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [42, 42]);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
        struct Fails;
        impl Read for Fails {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }
        let mut reader = CountedRead { inner: Fails, counter: &counter };
        assert_eq!(reader.read(&mut bytes).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
        let mut reader = CountedRead { inner: io::repeat(0).take(0), counter: &counter };
        assert!(reader.read_exact(&mut bytes).is_err());
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }
}
