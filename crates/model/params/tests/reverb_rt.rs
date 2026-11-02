//! The prepared reverb map is accessed directly in an audio callback.
use kontra_params::{ReverbControls, ReverbPar};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: all operations delegate to System with the original arguments.
#[expect(
    unsafe_code,
    reason = "the isolated allocator test delegates to System"
)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn prepared_control_lookup_allocates_nothing() {
    let controls = ReverbControls::from_fn(|p| p as u8);
    ALLOCS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::SeqCst);
    let mut sum = 0u32;
    for _ in 0..1000 {
        for par in ReverbPar::ALL {
            sum += u32::from(*std::hint::black_box(&controls).get(std::hint::black_box(par)));
        }
    }
    COUNTING.store(false, Ordering::SeqCst);
    assert_eq!(sum, 28_000);
    assert_eq!(ALLOCS.load(Ordering::Relaxed), 0);
}
