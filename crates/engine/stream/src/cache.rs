//! Fixed pool of streamed pages, shared by every sample of a store.
//!
//! The audio thread reads a slot while the I/O thread may be refilling it for
//! another page, so each slot is a seqlock: the reader decodes, then checks that
//! the generation and owner did not change. A torn read is reported as missing
//! (silence), never as wrong audio. Bytes are relaxed atomics, which compile to
//! plain loads, so the race is sound without `unsafe`.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering, fence};

use crate::fmt::SampleFmt;

pub(crate) struct Cache {
    slots: Box<[Slot]>,
    /// Every slot's bytes, `slot_bytes` each. Allocated zeroed, so the pages
    /// of slots that never fill are never made resident.
    bytes: Box<[AtomicU8]>,
    slot_bytes: usize,
}

struct Slot {
    /// Odd while the I/O thread writes the slot.
    generation: AtomicU32,
    /// Which (sample, page) the bytes belong to; see [`tag`].
    owner: AtomicU64,
    /// Set by a read, cleared by the eviction clock: a second chance.
    touched: AtomicBool,
}

/// Owner tag of a page. Never `u64::MAX`, the empty tag.
pub(crate) fn tag(sample: u32, page: u32) -> u64 {
    (u64::from(sample) << 32) | u64::from(page)
}

const EMPTY: u64 = u64::MAX;

impl Cache {
    pub(crate) fn new(slots: usize, slot_bytes: usize) -> Self {
        let bytes = bytemuck::zeroed_slice_box(slots * slot_bytes);
        let slots = (0..slots)
            .map(|_| Slot {
                generation: AtomicU32::new(0),
                owner: AtomicU64::new(EMPTY),
                touched: AtomicBool::new(false),
            })
            .collect();
        Self {
            slots,
            bytes,
            slot_bytes,
        }
    }

    fn bytes(&self, slot: usize) -> &[AtomicU8] {
        let start = slot * self.slot_bytes;
        self.bytes
            .get(start..start + self.slot_bytes)
            .unwrap_or_default()
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Audio thread. Decodes frames of slot `slot` into `out` if it still holds
    /// `owner`; otherwise leaves `out` silent and returns false.
    pub(crate) fn read(
        &self,
        slot: u32,
        owner: u64,
        fmt: SampleFmt,
        first: usize,
        out: &mut [[f32; 2]],
        mix: Option<&crate::fmt::Mix>,
    ) -> bool {
        let Some(s) = self.slots.get(slot as usize) else {
            return false;
        };
        let bytes = self.bytes(slot as usize);
        let before = s.generation.load(Ordering::Acquire);
        if before & 1 == 1 || s.owner.load(Ordering::Relaxed) != owner {
            out.fill([0.0; 2]);
            return false;
        }
        fmt.decode(
            |i| bytes.get(i).map_or(0, |b| b.load(Ordering::Relaxed)),
            first,
            out,
            mix,
        );
        fence(Ordering::Acquire);
        if s.generation.load(Ordering::Relaxed) != before {
            out.fill([0.0; 2]);
            return false;
        }
        s.touched.store(true, Ordering::Relaxed);
        true
    }

    /// I/O thread. Replaces the slot's bytes and owner.
    pub(crate) fn write(&self, slot: u32, owner: u64, data: &[u8]) {
        let Some(s) = self.slots.get(slot as usize) else {
            return;
        };
        let g = s.generation.load(Ordering::Relaxed);
        s.generation.store(g.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        s.owner.store(owner, Ordering::Relaxed);
        for (cell, value) in self
            .bytes(slot as usize)
            .iter()
            .zip(data.iter().copied().chain(std::iter::repeat(0)))
        {
            cell.store(value, Ordering::Relaxed);
        }
        s.touched.store(true, Ordering::Relaxed);
        s.generation.store(g.wrapping_add(2), Ordering::Release);
    }

    /// I/O thread. Clears and returns the touched bit (clock eviction).
    pub(crate) fn take_touched(&self, slot: u32) -> bool {
        self.slots
            .get(slot as usize)
            .is_some_and(|s| s.touched.swap(false, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::Encoding;

    const FMT: SampleFmt = SampleFmt {
        encoding: Encoding::I16,
        channels: 1,
    };

    #[test]
    fn a_slot_reads_only_for_its_owner() {
        let cache = Cache::new(1, 4);
        cache.write(0, tag(1, 0), &[0x00, 0x40, 0x00, 0xC0]);
        let mut out = [[1.0; 2]; 2];
        assert!(cache.read(0, tag(1, 0), FMT, 0, &mut out, None));
        assert_eq!(out, [[0.5, 0.5], [-0.5, -0.5]]);
        assert!(!cache.read(0, tag(2, 0), FMT, 0, &mut out, None));
        assert_eq!(out, [[0.0; 2]; 2]);
    }

    #[test]
    fn concurrent_rewrites_never_yield_a_mixed_page() {
        // Each owner's page holds one constant value; a torn read would mix two.
        let cache = std::sync::Arc::new(Cache::new(1, 2048));
        let writer = {
            let cache = cache.clone();
            std::thread::spawn(move || {
                for round in 0..2_000u32 {
                    let byte = if round % 2 == 0 { 0x10 } else { 0x20 };
                    cache.write(0, tag(round % 2, 0), &[byte; 2048]);
                }
            })
        };
        let mut out = [[0.0; 2]; 1024];
        let mut good = 0;
        while !writer.is_finished() {
            for owner in 0..2 {
                if cache.read(0, tag(owner, 0), FMT, 0, &mut out, None) {
                    let first = out[0];
                    assert!(out.iter().all(|f| *f == first), "torn page");
                    good += 1;
                }
            }
        }
        writer.join().unwrap();
        assert!(good > 0 || cache.read(0, tag(1, 0), FMT, 0, &mut out, None));
    }
}
