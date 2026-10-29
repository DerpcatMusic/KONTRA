//! Global allocation counter: attributes every Rust-side allocation to a benchmark phase.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

/// Forwards to the system allocator and counts calls. Reallocations count as one allocation.
#[derive(Debug)]
pub struct Counting;

#[global_allocator]
static GLOBAL: Counting = Counting;

#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub allocs: u64,
    pub bytes: u64,
}

impl Snapshot {
    /// Counts accumulated since `earlier`.
    pub fn since(self, earlier: Snapshot) -> Snapshot {
        Snapshot {
            allocs: self.allocs.saturating_sub(earlier.allocs),
            bytes: self.bytes.saturating_sub(earlier.bytes),
        }
    }
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        allocs: ALLOCS.load(Relaxed),
        bytes: BYTES.load(Relaxed),
    }
}

// SAFETY: every method forwards its arguments unchanged to `System`, which upholds the
// `GlobalAlloc` contract. The counters are atomics and never allocate, so there is no re-entry.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size() as u64, Relaxed);
        // SAFETY: the caller upholds `alloc`'s contract for `layout`; we pass it through.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` come from a matching `alloc`/`realloc` on `System`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(new_size as u64, Relaxed);
        // SAFETY: the caller upholds `realloc`'s contract; we pass the arguments through.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
