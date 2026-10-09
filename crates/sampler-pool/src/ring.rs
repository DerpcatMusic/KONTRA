#![allow(unsafe_code)]
//! Port from v1 0cb7a8a0:src/engine/stream.rs: zeroed per-voice rings.
//! The consumer lends published runs through a mutable borrow: it cannot move
//! the read boundary or reconfigure while those runs remain borrowed.

use std::{
    alloc::Layout,
    marker::PhantomData,
    ops::Range,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering, fence},
    },
};

pub const STREAM_RING_FRAMES: usize = 8192;
type Frame = [f32; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingError {
    Capacity,
    InvalidRange,
    Stale,
    Full,
    SequenceExhausted,
}

struct Rings {
    frames: NonNull<AtomicU64>,
    layout: Layout,
}

impl Rings {
    fn new(slots: usize) -> Result<Self, RingError> {
        let len = slots
            .checked_mul(STREAM_RING_FRAMES)
            .ok_or(RingError::Capacity)?;
        if len == 0 {
            return Err(RingError::Capacity);
        }
        let layout = Layout::array::<AtomicU64>(len).map_err(|_| RingError::Capacity)?;
        // SAFETY: nonzero checked layout; zero bits are valid AtomicU64s.
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        let frames = NonNull::new(ptr.cast()).ok_or(RingError::Capacity)?;
        Ok(Self { frames, layout })
    }
}

impl Drop for Rings {
    fn drop(&mut self) {
        // SAFETY: the same pointer/layout allocated in new; all endpoints have
        // released their Arc before this drop, so no reader or writer remains.
        unsafe { std::alloc::dealloc(self.frames.as_ptr().cast(), self.layout) }
    }
}

// SAFETY: frames are atomic; non-atomic published reads are protected by the
// consumer's mutable borrow and the producer's checked overwrite boundary.
unsafe impl Send for Rings {}
unsafe impl Sync for Rings {}

struct Slot {
    rings: Arc<Rings>,
    index: usize,
    seq: AtomicU64,
    from: AtomicU64,
    read: AtomicU64,
    written: AtomicU64,
    published: AtomicU64,
    active: AtomicBool,
}

impl Slot {
    fn frame(&self, virtual_frame: u64) -> &AtomicU64 {
        let offset = self.index * STREAM_RING_FRAMES + virtual_frame as usize % STREAM_RING_FRAMES;
        // SAFETY: factory checked index and each slot's full ring allocation.
        unsafe { self.rings.frames.add(offset).as_ref() }
    }
}

/// One audio-owned endpoint. Never clone or destroy it on the audio thread.
pub struct RingConsumer {
    slot: Arc<Slot>,
}
/// One worker-owned endpoint. A ring has exactly one producer.
pub struct RingProducer {
    slot: Arc<Slot>,
    seen: u64,
    next: u64,
}

/// Control-side construction. Like v1, unused ring pages are not written here.
pub fn stream_rings(slots: usize) -> Result<(Vec<RingConsumer>, Vec<RingProducer>), RingError> {
    let rings = Arc::new(Rings::new(slots)?);
    let mut consumers = Vec::new();
    let mut producers = Vec::new();
    consumers
        .try_reserve_exact(slots)
        .map_err(|_| RingError::Capacity)?;
    producers
        .try_reserve_exact(slots)
        .map_err(|_| RingError::Capacity)?;
    for index in 0..slots {
        let slot = Arc::new(Slot {
            rings: rings.clone(),
            index,
            seq: AtomicU64::new(0),
            from: AtomicU64::new(0),
            read: AtomicU64::new(0),
            written: AtomicU64::new(0),
            published: AtomicU64::new(0),
            active: AtomicBool::new(false),
        });
        consumers.push(RingConsumer { slot: slot.clone() });
        producers.push(RingProducer {
            slot,
            seen: 0,
            next: 0,
        });
    }
    Ok((consumers, producers))
}

impl RingConsumer {
    /// Port v1 Slot::write's seqlock. Full-width generations avoid tag reuse.
    pub fn configure(&mut self, from: u64, read: u64) -> Result<u64, RingError> {
        if from < read || read.checked_add(STREAM_RING_FRAMES as u64).is_none() {
            return Err(RingError::InvalidRange);
        }
        let seq = self.slot.seq.load(Ordering::Relaxed);
        let next = seq.checked_add(2).ok_or(RingError::SequenceExhausted)?;
        self.slot.seq.store(seq + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.slot.from.store(from, Ordering::Relaxed);
        self.slot.read.store(read, Ordering::Relaxed);
        self.slot.active.store(true, Ordering::Relaxed);
        self.slot.seq.store(next, Ordering::Release);
        Ok(next)
    }

    pub fn stop(&mut self) {
        self.slot.active.store(false, Ordering::Release);
    }

    pub fn published(&self, generation: u64) -> Option<u64> {
        if !self.slot.active.load(Ordering::Acquire)
            || self.slot.seq.load(Ordering::Acquire) != generation
            || self.slot.published.load(Ordering::Acquire) != generation
        {
            return None;
        }
        Some(self.slot.written.load(Ordering::Acquire))
    }

    /// Cannot run while a RingRead still borrows this consumer.
    pub fn release_below(&mut self, frame: u64) -> Result<(), RingError> {
        if frame < self.slot.read.load(Ordering::Relaxed)
            || frame.checked_add(STREAM_RING_FRAMES as u64).is_none()
        {
            return Err(RingError::InvalidRange);
        }
        self.slot.read.store(frame, Ordering::Release);
        Ok(())
    }

    pub fn claim(&mut self, generation: u64, range: Range<u64>) -> Option<RingRead<'_>> {
        if range.start > range.end
            || range.end - range.start > STREAM_RING_FRAMES as u64
            || range.start < self.slot.read.load(Ordering::Relaxed)
            || range.start < self.slot.from.load(Ordering::Relaxed)
            || range.end > self.published(generation)?
        {
            return None;
        }
        Some(RingRead {
            slot: &self.slot,
            range,
            borrow: PhantomData,
        })
    }
}

impl Drop for RingConsumer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl RingProducer {
    /// Port v1 Slot::snapshot. A changed configuration resets the producer.
    pub fn synchronize(&mut self) -> Option<(u64, u64)> {
        let seq = self.slot.seq.load(Ordering::Acquire);
        if seq & 1 != 0 || !self.slot.active.load(Ordering::Acquire) {
            return None;
        }
        let from = self.slot.from.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        if self.slot.seq.load(Ordering::Relaxed) != seq {
            return None;
        }
        if self.seen != seq {
            self.seen = seq;
            self.next = from;
        }
        self.next = self.next.max(self.slot.read.load(Ordering::Acquire));
        Some((seq, self.next))
    }

    pub fn room(&mut self) -> Option<(u64, Range<u64>)> {
        let (generation, from) = self.synchronize()?;
        let end = self
            .slot
            .read
            .load(Ordering::Acquire)
            .checked_add(STREAM_RING_FRAMES as u64)?;
        Some((generation, from..end.max(from)))
    }

    /// Store and publish one contiguous virtual run, never overwriting a read.
    pub fn write(&mut self, generation: u64, from: u64, frames: &[Frame]) -> Result<(), RingError> {
        let (current, expected) = self.synchronize().ok_or(RingError::Stale)?;
        if current != generation {
            return Err(RingError::Stale);
        }
        if from != expected {
            return Err(RingError::InvalidRange);
        }
        let end = from
            .checked_add(frames.len() as u64)
            .ok_or(RingError::InvalidRange)?;
        let limit = self
            .slot
            .read
            .load(Ordering::Acquire)
            .checked_add(STREAM_RING_FRAMES as u64)
            .ok_or(RingError::InvalidRange)?;
        if end > limit {
            return Err(RingError::Full);
        }
        for (i, frame) in frames.iter().enumerate() {
            let bits = u64::from(frame[0].to_bits()) | (u64::from(frame[1].to_bits()) << 32);
            self.slot
                .frame(from + i as u64)
                .store(bits, Ordering::Relaxed);
        }
        if self.slot.seq.load(Ordering::Acquire) != generation {
            return Err(RingError::Stale);
        }
        self.slot.written.store(end, Ordering::Release);
        self.slot.published.store(generation, Ordering::Release);
        self.next = end;
        Ok(())
    }
}

/// Borrowed published frames; releasing this guard permits the consumer to
/// advance the read boundary. Producer writes remain outside this range.
pub struct RingRead<'a> {
    slot: &'a Slot,
    range: Range<u64>,
    borrow: PhantomData<&'a mut RingConsumer>,
}

impl RingRead<'_> {
    pub fn frame(&self, at: u64) -> Option<Frame> {
        if !self.range.contains(&at) {
            return None;
        }
        let bits = self.slot.frame(at).load(Ordering::Relaxed);
        Some([
            f32::from_bits(bits as u32),
            f32::from_bits((bits >> 32) as u32),
        ])
    }

    /// Port v1 Slot::runs with a borrow that prevents premature release.
    pub fn runs(&self, range: Range<u64>) -> Option<[&[Frame]; 2]> {
        if !cfg!(target_endian = "little")
            || range.start > range.end
            || range.start < self.range.start
            || range.end > self.range.end
        {
            return None;
        }
        let len = (range.end - range.start) as usize;
        let start = range.start as usize % STREAM_RING_FRAMES;
        let first = len.min(STREAM_RING_FRAMES - start);
        // SAFETY: both runs are within the allocated slot. The guard holds the
        // read boundary below this range, and checked writes start after it.
        // AtomicU64 stores little-endian [f32;2] bits with sufficient alignment.
        unsafe {
            let base = self
                .slot
                .rings
                .frames
                .add(self.slot.index * STREAM_RING_FRAMES)
                .as_ptr()
                .cast::<Frame>();
            Some([
                std::slice::from_raw_parts(base.add(start), first),
                std::slice::from_raw_parts(base, len - first),
            ])
        }
    }

    pub fn copy(&self, range: Range<u64>, out: &mut [Frame]) -> bool {
        if range.start > range.end || range.end - range.start != out.len() as u64 {
            return false;
        }
        if let Some([first, second]) = self.runs(range.clone()) {
            let (head, tail) = out.split_at_mut(first.len());
            head.copy_from_slice(first);
            tail.copy_from_slice(second);
            true
        } else {
            for (at, frame) in range.zip(out) {
                let Some(value) = self.frame(at) else {
                    return false;
                };
                *frame = value;
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_runs_survive_wrap_and_cannot_be_overwritten_before_release() {
        let (mut readers, mut writers) = stream_rings(1).unwrap();
        let reader = &mut readers[0];
        let writer = &mut writers[0];
        let tag = reader.configure(0, 0).unwrap();
        assert!(reader.claim(tag, 0..1).is_none());
        let frames: Vec<_> = (0..STREAM_RING_FRAMES)
            .map(|i| [i as f32, -(i as f32)])
            .collect();
        writer.write(tag, 0, &frames).unwrap();
        {
            let guard = reader.claim(tag, 0..STREAM_RING_FRAMES as u64).unwrap();
            assert_eq!(guard.frame(11), Some([11., -11.]));
            assert_eq!(
                writer.write(tag, STREAM_RING_FRAMES as u64, &[[1.; 2]]),
                Err(RingError::Full)
            );
        }
        reader.release_below(STREAM_RING_FRAMES as u64 - 2).unwrap();
        writer
            .write(tag, STREAM_RING_FRAMES as u64, &[[5., 6.], [7., 8.]])
            .unwrap();
        let guard = reader
            .claim(
                tag,
                STREAM_RING_FRAMES as u64 - 2..STREAM_RING_FRAMES as u64 + 2,
            )
            .unwrap();
        let mut out = [[0.; 2]; 4];
        assert!(guard.copy(
            STREAM_RING_FRAMES as u64 - 2..STREAM_RING_FRAMES as u64 + 2,
            &mut out
        ));
        assert_eq!(out, [[8190., -8190.], [8191., -8191.], [5., 6.], [7., 8.]]);
    }

    #[test]
    fn a_new_offset_never_reads_a_previous_voice_generation() {
        let (mut readers, mut writers) = stream_rings(1).unwrap();
        let first = readers[0].configure(0, 0).unwrap();
        writers[0].write(first, 0, &[[1.; 2]]).unwrap();
        let next = readers[0].configure(9000, 9000).unwrap();
        assert!(readers[0].claim(next, 9000..9001).is_none());
        assert_eq!(
            writers[0].write(first, 1, &[[2.; 2]]),
            Err(RingError::Stale)
        );
        writers[0].write(next, 9000, &[[3., 4.]]).unwrap();
        assert_eq!(
            readers[0].claim(next, 9000..9001).unwrap().frame(9000),
            Some([3., 4.])
        );
        assert!(matches!(stream_rings(usize::MAX), Err(RingError::Capacity)));
    }

    #[test]
    fn publication_and_borrowed_reads_stay_consistent_across_concurrent_wraps() {
        let (mut readers, mut writers) = stream_rings(1).unwrap();
        let tag = readers[0].configure(0, 0).unwrap();
        let mut writer = writers.pop().unwrap();
        let worker = std::thread::spawn(move || {
            for at in (0..65536u64).step_by(64) {
                while writer.room().unwrap().1.end < at + 64 {
                    std::thread::yield_now();
                }
                let frames: [Frame; 64] =
                    std::array::from_fn(|i| [(at + i as u64) as f32, -((at + i as u64) as f32)]);
                writer.write(tag, at, &frames).unwrap();
            }
        });
        let reader = &mut readers[0];
        for at in (0..65536u64).step_by(64) {
            while reader.published(tag).is_none_or(|end| end < at + 64) {
                std::thread::yield_now();
            }
            {
                let guard = reader.claim(tag, at..at + 64).unwrap();
                let mut out = [[0.; 2]; 64];
                assert!(guard.copy(at..at + 64, &mut out));
                for (i, frame) in out.iter().enumerate() {
                    assert_eq!(*frame, [(at + i as u64) as f32, -((at + i as u64) as f32)]);
                }
            }
            reader.release_below(at + 64).unwrap();
        }
        worker.join().unwrap();
    }

    #[test]
    fn a_published_prefix_never_covers_the_resident_gap_before_the_ring_start() {
        let (mut readers, mut writers) = stream_rings(1).unwrap();
        let tag = readers[0].configure(4048, 0).unwrap();
        writers[0].write(tag, 4048, &[[0.5, -0.25]]).unwrap();
        assert!(readers[0].claim(tag, 0..1).is_none());
        assert_eq!(
            readers[0].claim(tag, 4048..4049).unwrap().frame(4048),
            Some([0.5, -0.25])
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unused_rings_leave_their_frame_pages_untouched() {
        unsafe extern "C" {
            fn getpagesize() -> i32;
            fn mincore(addr: *mut std::ffi::c_void, length: usize, vec: *mut u8) -> i32;
        }
        let (readers, _writers) = stream_rings(512).unwrap();
        let rings = &readers[0].slot.rings;
        // SAFETY: getpagesize has no preconditions.
        let page = unsafe { getpagesize() } as usize;
        assert!(page.is_power_of_two());
        let start = rings.frames.as_ptr() as usize;
        let lo = start.next_multiple_of(page);
        let hi = (start + rings.layout.size()) / page * page;
        let mut resident = vec![0u8; (hi - lo) / page];
        // SAFETY: the whole-page range is inside the live ring allocation;
        // mincore writes at most one byte per page into this sized buffer.
        assert_eq!(
            unsafe { mincore(lo as *mut _, hi - lo, resident.as_mut_ptr()) },
            0
        );
        assert!(
            resident.iter().filter(|&&value| value & 1 != 0).count() <= 1,
            "idle zeroed rings must not eagerly touch their PCM pages"
        );
    }
}
