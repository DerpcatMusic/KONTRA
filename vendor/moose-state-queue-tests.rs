use super::{StateLoadQueue, state};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct Counting;
thread_local! {
    static AUDIO: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}
fn count() {
    if AUDIO.get() {
        CALLS.set(CALLS.get() + 1);
    }
}
// SAFETY: forwards each allocation/deallocation unchanged to System.
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

fn state() -> state::DeserializedState {
    state::DeserializedState {
        params: vec![(1, 0.5); 8192],
        extra: Some(vec![7; 256 * 1024]),
        persist: vec![9; 1024 * 1024],
    }
}

#[test]
fn restored_large_state_is_not_freed_by_the_audio_consumer() {
    let queue = StateLoadQueue::new(1);
    drop(queue.force_push(state()));
    let before = CALLS.get();
    AUDIO.set(true);
    let loaded = queue.apply_audio(|state| {
        assert_eq!(state.params.len(), 8192);
        assert_eq!(state.extra.as_ref().unwrap()[0], 7);
        assert_eq!(state.persist.len(), 1024 * 1024);
    });
    AUDIO.set(false);
    assert!(loaded);
    assert_eq!(
        CALLS.get() - before,
        0,
        "state handoff allocated/freed on audio"
    );
    assert_eq!(queue.collect_retired(), 1);
}

#[test]
fn a_restore_published_during_application_keeps_its_ownership_and_order() {
    let queue = StateLoadQueue::new(1);
    drop(queue.force_push(state()));
    assert!(queue.apply_audio(|_| {}));
    // A writer can publish while the preceding consumer is still applying.
    drop(queue.force_push(state()));
    assert!(queue.apply_audio(|_| {
        drop(queue.force_push(state()));
    }));
    assert!(queue.apply_audio(|_| {}));
    assert_eq!(queue.collect_retired(), 2);
    assert!(!queue.apply_audio(|_| {}));
    drop(queue.force_push(state()));
    assert!(queue.apply_audio(|_| {}));
    assert_eq!(queue.collect_retired(), 1);
}

#[test]
fn a_panicking_restore_still_retires_the_blob_on_control() {
    let queue = StateLoadQueue::new(1);
    drop(queue.force_push(state()));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            queue.apply_audio(|_| panic!("authored restore failure"));
        }))
        .is_err()
    );
    assert_eq!(queue.collect_retired(), 1);
}

#[test]
fn rapid_concurrent_recalls_leave_the_audio_consumer_heap_free() {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    };
    let queue = Arc::new(StateLoadQueue::new(1));
    let start = Arc::new(Barrier::new(2));
    let finished = Arc::new(AtomicBool::new(false));
    let writer = {
        let (queue, start, finished) = (queue.clone(), start.clone(), finished.clone());
        std::thread::spawn(move || {
            start.wait();
            for _ in 0..64 {
                drop(queue.force_push(state()));
                std::thread::yield_now();
            }
            finished.store(true, Ordering::Release);
        })
    };
    start.wait();
    let before = CALLS.get();
    AUDIO.set(true);
    while !finished.load(Ordering::Acquire) {
        queue.apply_audio(|state| {
            assert_eq!(state.persist.len(), 1024 * 1024);
        });
    }
    AUDIO.set(false);
    writer.join().unwrap();
    assert_eq!(CALLS.get() - before, 0);
    queue.collect_retired();
    queue.apply_audio(|_| {});
    queue.collect_retired();
}

#[test]
fn rapid_pending_recalls_apply_only_the_newest_blob() {
    let queue = StateLoadQueue::new(1);
    drop(queue.force_push(state()));
    let mut newest = state();
    newest.params[0].1 = 0.75;
    assert!(queue.force_push(newest).is_some());
    assert!(queue.apply_audio(|state| {
        assert_eq!(state.params[0].1, 0.75);
    }));
    assert!(!queue.apply_audio(|_| panic!("displaced state was applied")));
    assert_eq!(queue.collect_retired(), 1);
}
