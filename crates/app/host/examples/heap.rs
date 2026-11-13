//! Where the anonymous heap goes after a load: a counting allocator tags every
//! live allocation of 512 bytes or more with its load phase and call site, then
//! prints the phases' live bytes and the ten biggest sites. Build with
//! `--profile corpus` so frames have symbols.
//! `cargo run --profile corpus -p kontra-host --features heap-profile --example heap -- <file>...`
//! (The feature leaves Luau on the system heap so this allocator can count it.)

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use kontra_engine::Engine;
use kontra_stream::{Store, StoreConfig};

const MIN: usize = 512;
const FRAMES: usize = 12;
const STAGES: [&str; 3] = ["translate", "prepare", "engine"];

/// Phase `p` is stage `p % 3` of instance `p / 3`.
fn phase_name(p: usize) -> String {
    format!(
        "#{} {}",
        p / 3 + 1,
        STAGES.get(p % 3).copied().unwrap_or("?")
    )
}

static PHASE: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static TRACKING: AtomicUsize = AtomicUsize::new(0);

type Site = (usize, [usize; FRAMES]);
#[derive(Default)]
struct Live {
    blocks: HashMap<usize, (Site, usize)>,
    sites: HashMap<Site, usize>,
}
static LIVE: Mutex<Option<Live>> = Mutex::new(None);
thread_local! { static BUSY: Cell<bool> = const { Cell::new(false) }; }

struct Counting;
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn site() -> Site {
    let mut frames = [0; FRAMES];
    let (mut n, mut skip) = (0, 0);
    // The locked `backtrace::trace` deadlocks against the loader's lock when
    // a thread allocates inside dlopen, so walk without the crate's mutex.
    #[expect(unsafe_code, reason = "trace_unsynchronized: no crate lock")]
    // SAFETY: only this thread's stack is walked, and the example runs no other
    // unsynchronized trace.
    unsafe {
        backtrace::trace_unsynchronized(|f| {
            skip += 1;
            if skip > 3
                && let Some(slot) = frames.get_mut(n)
            {
                *slot = f.ip() as usize;
                n += 1;
            }
            n < FRAMES
        });
    }
    (PHASE.load(Ordering::Relaxed), frames)
}

fn track(ptr: *mut u8, size: usize) {
    TOTAL.fetch_add(size, Ordering::Relaxed);
    if size < MIN || ptr.is_null() || TRACKING.load(Ordering::Relaxed) == 0 {
        return;
    }
    // The bookkeeping below allocates: let those calls through untracked.
    if BUSY.with(|b| b.replace(true)) {
        return;
    }
    let s = site();
    if let Ok(mut live) = LIVE.lock() {
        let live = live.get_or_insert_with(Live::default);
        live.blocks.insert(ptr as usize, (s, size));
        *live.sites.entry(s).or_default() += size;
    }
    BUSY.with(|b| b.set(false));
}

fn untrack(ptr: *mut u8, size: usize) {
    TOTAL.fetch_sub(size, Ordering::Relaxed);
    if size < MIN || BUSY.with(|b| b.replace(true)) {
        return;
    }
    if let Ok(mut live) = LIVE.lock()
        && let Some(live) = live.as_mut()
        && let Some((s, size)) = live.blocks.remove(&(ptr as usize))
        && let Some(total) = live.sites.get_mut(&s)
    {
        *total -= size;
    }
    BUSY.with(|b| b.set(false));
}

// SAFETY: every operation is forwarded to the system allocator with its
// original arguments; the bookkeeping never touches the block.
#[expect(unsafe_code, reason = "the measuring allocator forwards to System")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        track(ptr, layout.size());
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // Not the default (alloc + memset): that would touch every page of a
        // zeroed buffer the program means to leave untouched.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        track(ptr, layout.size());
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        untrack(ptr, layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        untrack(ptr, layout.size());
        let new = unsafe { System.realloc(ptr, layout, size) };
        track(new, size);
        new
    }
}

/// `RssAnon` in MB: what the heap really holds in RAM (zeroed pages that
/// were never written do not count).
fn anon_mb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    (status.lines())
        .find(|l| l.starts_with("RssAnon:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0)
        / 1024
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1e6
}

fn names(frames: &[usize; FRAMES]) -> String {
    let mut out = Vec::new();
    for ip in frames.iter().filter(|ip| **ip != 0) {
        backtrace::resolve(*ip as *mut std::ffi::c_void, |s| {
            let name = s.name().map(|n| n.to_string()).unwrap_or_default();
            let name = name.rsplit_once("::h").map_or(name.as_str(), |(a, _)| a);
            let skip = [
                "alloc::",
                "core::",
                "std::",
                "hashbrown",
                "<alloc",
                "heap::",
            ];
            if out.len() < 6 && !name.is_empty() && !skip.iter().any(|p| name.starts_with(p)) {
                let at = s.filename().map_or(String::new(), |f| {
                    format!(" ({}:{})", f.display(), s.lineno().unwrap_or(0))
                });
                out.push(format!("{name}{at}"));
            }
        });
    }
    out.join(" < ")
}

fn report(label: &str, from: usize) {
    // Reporting allocates: keep it out of the counts, and out of the lock.
    BUSY.with(|b| b.set(true));
    let Ok(mut guard) = LIVE.lock() else {
        return;
    };
    let Some(live) = guard.as_mut() else {
        return;
    };
    let mut by_phase = [0usize; 12];
    for ((phase, _), bytes) in &live.sites {
        if let Some(p) = by_phase.get_mut(*phase) {
            *p += bytes;
        }
    }
    let tracked: usize = by_phase.iter().sum();
    println!(
        "  {label}: heap {:.1} MB live (resident anon {} MB), {:.1} MB in blocks >= {MIN} B; by phase: {}",
        mb(TOTAL.load(Ordering::Relaxed)),
        anon_mb(),
        mb(tracked),
        by_phase
            .iter()
            .enumerate()
            .filter(|(_, b)| **b > 0)
            .map(|(p, b)| format!("{} {:.1}", phase_name(p), mb(*b)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut top: Vec<(&Site, &usize)> = live.sites.iter().filter(|(s, _)| s.0 >= from).collect();
    top.sort_by_key(|(_, b)| std::cmp::Reverse(**b));
    let top: Vec<(Site, usize)> = top.into_iter().take(10).map(|(s, b)| (*s, *b)).collect();
    // Resolving symbols allocates: do it outside the lock and untracked.
    drop(guard);
    for ((phase, frames), bytes) in top {
        println!(
            "    {:7.1} MB [{}] {}",
            mb(bytes),
            phase_name(phase),
            names(&frames)
        );
    }
    BUSY.with(|b| b.set(false));
}

/// Each file is one plugin instance in this process: its own store (sharing
/// pinned pages with the others), heads deferred then read, every instance
/// kept alive. The top sites listed are those of the latest instance.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut alive = Vec::new();
    TRACKING.store(1, Ordering::Relaxed);
    for (n, path) in std::env::args().skip(1).enumerate() {
        println!("{path}");
        let first = 3 * n;
        PHASE.store(first, Ordering::Relaxed);
        let mut store = Store::new(StoreConfig::DEFAULT)?;
        store.share_pages_in_process();
        store.defer_heads(true);
        let ir = kontra_host::instrument::read(path.as_ref())?;
        report("after translate", first);
        PHASE.store(first + 1, Ordering::Relaxed);
        let loaded = kontra_host::instrument::build(ir, 48_000, &mut store);
        println!(
            "  {} zones of {} bytes each",
            loaded.plan.zones.len(),
            std::mem::size_of::<kontra_engine::Zone>(),
        );
        if let Some(heads) = store.take_heads() {
            heads.load(8);
        }
        report("after prepare", first);
        PHASE.store(first + 2, Ordering::Relaxed);
        let engine = Engine::new(std::sync::Arc::new(loaded.plan), store.client(), 48_000);
        report("after engine", first);
        alive.push((store, engine));
    }
    TRACKING.store(0, Ordering::Relaxed);
    Ok(())
}
