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
    if COUNTING.get() {
        CALLS.set(CALLS.get() + 1);
    }
}
// SAFETY: forwards allocation and deallocation unchanged to System.
#[allow(
    unsafe_code,
    reason = "Test-only allocator forwards unchanged to System to count allocation and deallocation"
)]
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

#[allow(dead_code, reason = "not every test binary that includes this module counts heap use")]
pub fn without_heap(f: impl FnOnce()) {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            COUNTING.set(false);
        }
    }
    let before = CALLS.get();
    COUNTING.set(true);
    let guard = Guard;
    f();
    drop(guard);
    assert_eq!(
        CALLS.get() - before,
        0,
        "callback allocated or freed memory"
    );
}
