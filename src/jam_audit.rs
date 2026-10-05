//! External campaign executable; allocation instrumentation stays out of the plugin library.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::PathBuf,
};
thread_local! {
    static HOT: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[u64; 4]> = const { Cell::new([0; 4]) };
}
struct CountAllocator;
fn count(index: usize) {
    if HOT.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNTS.try_with(|cell| {
            let mut values = cell.get();
            values[index] += 1;
            cell.set(values);
        });
    }
}
// SAFETY: all operations forward the original pointer/layout unchanged to System.
unsafe impl GlobalAlloc for CountAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(1);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(3);
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAllocator = CountAllocator;
fn meter(hot: bool) -> [u64; 4] {
    HOT.with(|cell| cell.set(hot));
    COUNTS.with(Cell::get)
}
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = args
        .windows(2)
        .find(|pair| pair[0] == "--output")
        .map(|pair| PathBuf::from(&pair[1]))
        .ok_or_else(|| anyhow::anyhow!("--output required"))?;
    // SAFETY: no loader, recorder, or other worker exists before the library entry point.
    unsafe {
        std::env::set_var("KONTRA_LOG_DIR", output.join("raw-journal"));
    }
    kontakto::universal_jam(&args, meter)
}
