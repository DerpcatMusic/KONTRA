//! Allocation accounting shared by both platform test suites.

use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell};

struct Counting;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}
fn count() {
    if COUNTING.with(Cell::get) {
        CALLS.with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: forwards every call unchanged to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocations and frees `f` makes on this thread.
pub(crate) fn allocations(f: impl FnOnce()) -> usize {
    let before = CALLS.with(Cell::get);
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    CALLS.with(Cell::get) - before
}
