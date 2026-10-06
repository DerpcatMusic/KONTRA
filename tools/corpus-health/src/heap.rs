//! Counting allocator: live and peak heap bytes for the whole process, and
//! allocation calls made by the calling thread (the render thread's count is
//! the audio-thread allocation measure; stream workers are other threads).
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

fn grow(by: usize) {
    let live = LIVE.fetch_add(by, Ordering::Relaxed) + by;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY: every call forwards to System unchanged; only counters are touched.
#[allow(
    unsafe_code,
    reason = "Allocator instrumentation forwards unchanged to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = CALLS.try_with(|c| c.set(c.get() + 1));
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let _ = CALLS.try_with(|c| c.set(c.get() + 1));
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let _ = CALLS.try_with(|c| c.set(c.get() + 1));
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                grow(new - l.size());
            } else {
                LIVE.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocation calls this thread has made so far.
pub fn calls() -> usize {
    CALLS.with(Cell::get)
}
pub fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}
pub fn peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}
/// Restart peak tracking from the current live size.
pub fn reset_peak() {
    PEAK.store(live(), Ordering::Relaxed);
}
