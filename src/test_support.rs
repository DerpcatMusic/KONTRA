//! Allocation assertions shared by core and plugin unit tests.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

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

#[test]
fn allocation_assertions_count_allocations_and_frees_and_reset_each_window() {
    let layout = Layout::from_size_align(16, 8).unwrap();
    let mut observed = 0;
    assert_eq!(
        allocations(|| {
            let pointer = unsafe { std::alloc::alloc(layout) };
            assert!(!pointer.is_null());
            unsafe {
                std::ptr::write_volatile(pointer, 37);
                observed = std::ptr::read_volatile(pointer);
                std::alloc::dealloc(pointer, layout);
            }
        }),
        2
    );
    assert_eq!(observed, 37);
    assert_eq!(allocations(|| {}), 0);
    let unmeasured = Box::new(41);
    std::hint::black_box(&unmeasured);
    drop(unmeasured);
    assert_eq!(allocations(|| {}), 0);
}

#[test]
fn allocation_assertions_keep_other_threads_out_of_the_window() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    // Force the worker's measured operations inside this thread's window.
    let signal = Arc::new((AtomicBool::new(false), AtomicBool::new(false)));
    let other = signal.clone();
    let worker = std::thread::spawn(move || {
        let layout = Layout::from_size_align(8, 8).unwrap();
        while !other.0.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let count = allocations(|| {
            let pointer = unsafe { std::alloc::alloc(layout) };
            assert!(!pointer.is_null());
            unsafe {
                std::ptr::write_volatile(pointer, 5);
                std::alloc::dealloc(pointer, layout);
            }
        });
        other.1.store(true, Ordering::Release);
        count
    });
    assert_eq!(
        allocations(|| {
            signal.0.store(true, Ordering::Release);
            while !signal.1.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        }),
        0
    );
    assert_eq!(worker.join().unwrap(), 2);
    assert_eq!(allocations(|| {}), 0);
}
