//! Test-only, numeric allocation stacks across loader threads. No sample/script data.
use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
const MIN_BYTES: usize = 4096;
const CAPACITY: usize = 65536;
const DEPTH: usize = 48;
static ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT: AtomicUsize = AtomicUsize::new(0);
static WRITERS: AtomicUsize = AtomicUsize::new(0);
struct Entry {
    ptr: AtomicUsize,
    bytes: AtomicUsize,
    freed: AtomicBool,
    pcs: [AtomicUsize; DEPTH],
}
static ENTRIES: [Entry; CAPACITY] = [const {
    Entry {
        ptr: AtomicUsize::new(0),
        bytes: AtomicUsize::new(0),
        freed: AtomicBool::new(false),
        pcs: [const { AtomicUsize::new(0) }; DEPTH],
    }
}; CAPACITY];
thread_local! { static RECORDING: Cell<bool> = const { Cell::new(false) }; }
unsafe extern "C" {
    fn backtrace(buffer: *mut *mut libc::c_void, size: libc::c_int) -> libc::c_int;
}
pub(super) fn record(ptr: *mut u8, bytes: usize, freed: bool) {
    if ptr.is_null() || bytes < MIN_BYTES || !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    RECORDING.with(|busy| {
        if busy.replace(true) {
            return;
        }
        WRITERS.fetch_add(1, Ordering::SeqCst);
        if !ENABLED.load(Ordering::SeqCst) {
            WRITERS.fetch_sub(1, Ordering::SeqCst);
            busy.set(false);
            return;
        }
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        if let Some(entry) = ENTRIES.get(index) {
            entry.ptr.store(ptr as usize, Ordering::Relaxed);
            entry.bytes.store(bytes, Ordering::Relaxed);
            entry.freed.store(freed, Ordering::Relaxed);
            for pc in &entry.pcs {
                pc.store(0, Ordering::Relaxed);
            }
            if !freed {
                let mut pcs = [std::ptr::null_mut(); DEPTH];
                // SAFETY: libc writes at most DEPTH addresses into this stack buffer.
                let count = unsafe { backtrace(pcs.as_mut_ptr(), DEPTH as i32) };
                for (dst, pc) in entry.pcs.iter().zip(pcs.iter().take(count.max(0) as usize)) {
                    dst.store(*pc as usize, Ordering::Relaxed);
                }
            }
        }
        WRITERS.fetch_sub(1, Ordering::SeqCst);
        busy.set(false);
    });
}
pub(crate) fn start() {
    let mut pcs = [std::ptr::null_mut(); DEPTH];
    // Load the unwinder before recording, avoiding its one-time allocations.
    unsafe {
        backtrace(pcs.as_mut_ptr(), DEPTH as i32);
    }
    NEXT.store(0, Ordering::Relaxed);
    ENABLED.store(true, Ordering::SeqCst);
}
pub(crate) fn finish(path: &std::path::Path) {
    ENABLED.store(false, Ordering::SeqCst);
    while WRITERS.load(Ordering::SeqCst) != 0 {
        std::thread::yield_now();
    }
    let count = NEXT.load(Ordering::Relaxed);
    assert!(count <= CAPACITY, "allocation trace overflow: {count}");
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    writeln!(out, "{}", serde_json::json!({"min_bytes":MIN_BYTES,"maps":std::fs::read_to_string("/proc/self/maps").unwrap(),"events":count})).unwrap();
    for entry in &ENTRIES[..count] {
        writeln!(out, "{}", serde_json::json!({"ptr":entry.ptr.load(Ordering::Relaxed),"bytes":entry.bytes.load(Ordering::Relaxed),"freed":entry.freed.load(Ordering::Relaxed),"pcs":entry.pcs.iter().map(|p|p.load(Ordering::Relaxed)).take_while(|p|*p!=0).collect::<Vec<_>>()})).unwrap();
    }
}
#[test]
#[ignore] // The process-wide recorder is run alone, like probe_load.
fn records_allocations_and_frees_without_recursing() {
    start();
    let memory = vec![0u8; 65536];
    std::hint::black_box(&memory);
    drop(memory);
    ENABLED.store(false, Ordering::SeqCst);
    let count = NEXT.load(Ordering::Relaxed);
    assert_eq!(count, 2);
    assert_eq!(ENTRIES[0].bytes.load(Ordering::Relaxed), 65536);
    assert!(!ENTRIES[0].freed.load(Ordering::Relaxed));
    assert!(ENTRIES[1].freed.load(Ordering::Relaxed));
    assert_eq!(
        ENTRIES[0].ptr.load(Ordering::Relaxed),
        ENTRIES[1].ptr.load(Ordering::Relaxed)
    );
}
