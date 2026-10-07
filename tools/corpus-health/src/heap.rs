//! Counting allocator. Per thread: live bytes since the last `job_start`, the
//! peak of that, and allocation calls. Jobs run on worker threads, so a job's
//! heap is its thread's; bytes freed by another thread (stream decoders) skew
//! it slightly. A process-wide live count is kept for the whole-run view.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
    static MINE: Cell<i64> = const { Cell::new(0) };
    static MINE_PEAK: Cell<i64> = const { Cell::new(0) };
}

fn grow(by: usize) {
    LIVE.fetch_add(by, Ordering::Relaxed);
    let _ = MINE.try_with(|m| {
        let now = m.get() + by as i64;
        m.set(now);
        let _ = MINE_PEAK.try_with(|p| p.set(p.get().max(now)));
    });
}
fn shrink(by: usize) {
    LIVE.fetch_sub(by, Ordering::Relaxed);
    let _ = MINE.try_with(|m| m.set(m.get() - by as i64));
}
fn call() {
    let _ = CALLS.try_with(|c| c.set(c.get() + 1));
}

// SAFETY: every call forwards to System unchanged; only counters are touched.
#[allow(
    unsafe_code,
    reason = "Allocator instrumentation forwards unchanged to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        call();
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        call();
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        shrink(l.size());
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        call();
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                grow(new - l.size());
            } else {
                shrink(l.size() - new);
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
/// Heap bytes this thread holds since `job_start` (may dip below zero).
pub fn live() -> i64 {
    MINE.with(Cell::get)
}
/// Peak of `live` since `job_start`.
pub fn peak() -> i64 {
    MINE_PEAK.with(Cell::get)
}
/// Begin measuring a job on this thread.
pub fn job_start() {
    MINE.set(0);
    MINE_PEAK.set(0);
}
