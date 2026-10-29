//! Option A: a `#[global_allocator]` that sends allocations made inside an [`ArenaScope`] to one
//! fixed arena, and everything else to the system allocator.
//!
//! mlua's Luau allocator calls the Rust global allocator (mlua-0.12.2 src/memory.rs:137), so
//! Luau heap blocks and Rust-side allocations made inside a scope both land in the arena.
//!
//! Frees route by address: a pointer inside the arena region always goes back to the arena,
//! whichever thread frees it and whether or not a scope is active. Everything else goes to
//! `System`. Arena allocations take a spin lock, which the audio thread only contends with when
//! another thread frees an arena block at the same moment.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::hint::spin_loop;
use std::ptr;
use std::sync::atomic::{
    AtomicBool, AtomicUsize,
    Ordering::{Acquire, Relaxed, Release},
};

/// Smallest block is 16 bytes, so every block is 16-byte aligned.
const MIN_SHIFT: u32 = 4;
/// Largest block is 64 MiB. Larger requests go to `System`, and so do alignments above it.
const MAX_SHIFT: u32 = 26;
const CLASSES: usize = (MAX_SHIFT - MIN_SHIFT + 1) as usize;
const ALIGN: usize = 16;

thread_local! {
    /// Set while the current thread runs script code. Const-initialised, so reading it never
    /// allocates and needs no lazy init.
    static IN_SCOPE: Cell<bool> = const { Cell::new(false) };
    /// Per-thread counters for the benchmarks and tests. Plain cells, no atomics.
    static STATS: Cell<Stats> = const { Cell::new(Stats::ZERO) };
    /// Size and alignment of the last scope allocation that went to `System`.
    static LAST_VIOLATION: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// Size and alignment of the last in-scope allocation this thread sent to `System`.
pub fn last_violation() -> (usize, usize) {
    LAST_VIOLATION.with(Cell::get)
}

/// Counters for allocator calls made by the current thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    /// Every `alloc`, `alloc_zeroed` and `realloc` call.
    pub allocs: u64,
    /// Calls served by the arena.
    pub arena: u64,
    /// Calls served by `System`.
    pub system: u64,
    /// Calls served by `System` while a scope was active. Zero is the target on the audio thread.
    pub system_in_scope: u64,
}

impl Stats {
    const ZERO: Self = Self {
        allocs: 0,
        arena: 0,
        system: 0,
        system_in_scope: 0,
    };

    /// Counts accumulated since `earlier`.
    pub fn since(self, earlier: Self) -> Self {
        Self {
            allocs: self.allocs - earlier.allocs,
            arena: self.arena - earlier.arena,
            system: self.system - earlier.system,
            system_in_scope: self.system_in_scope - earlier.system_in_scope,
        }
    }
}

/// The current thread's counters.
pub fn stats() -> Stats {
    STATS.with(Cell::get)
}

fn bump(f: impl FnOnce(&mut Stats)) {
    STATS.with(|c| {
        let mut s = c.get();
        f(&mut s);
        c.set(s);
    });
}

/// Marks the current thread as running script code until dropped. Scopes nest.
#[derive(Debug)]
#[must_use = "the scope ends when this guard is dropped"]
pub struct ArenaScope {
    previous: bool,
}

impl ArenaScope {
    pub fn enter() -> Self {
        Self {
            previous: IN_SCOPE.with(|c| c.replace(true)),
        }
    }
}

impl Drop for ArenaScope {
    fn drop(&mut self) {
        IN_SCOPE.with(|c| c.set(self.previous));
    }
}

/// Whether the current thread is inside an [`ArenaScope`].
pub fn in_scope() -> bool {
    IN_SCOPE.with(Cell::get)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArenaStats {
    /// Bytes handed out by the bump cursor, including blocks now on free lists.
    pub reserved: usize,
    /// Bytes in blocks currently allocated from the arena.
    pub live: usize,
    /// Highest `live` since the last [`reset_peak`].
    pub peak_live: usize,
    /// Scope allocations that did not fit and went to `System`.
    pub spills: u64,
}

/// Shared arena state. Lives in a `static`, so it is `Sync` through the lock below.
#[derive(Debug)]
struct State {
    bump: usize,
    free: [usize; CLASSES],
    live: usize,
    peak_live: usize,
    spills: u64,
}

struct Arena {
    /// Region start, 0 until [`reserve`] succeeds. Written once, read without the lock.
    base: AtomicUsize,
    /// Region size in bytes. Written once, before `base` is published.
    size: AtomicUsize,
    lock: AtomicBool,
    state: UnsafeCell<State>,
}

// SAFETY: `state` is only touched while `lock` is held (see `with`). `base` and `size` are
// atomics. So shared access from any thread is serialised.
unsafe impl Sync for Arena {}

static ARENA: Arena = Arena {
    base: AtomicUsize::new(0),
    size: AtomicUsize::new(0),
    lock: AtomicBool::new(false),
    state: UnsafeCell::new(State {
        bump: 0,
        free: [0; CLASSES],
        live: 0,
        peak_live: 0,
        spills: 0,
    }),
};

impl Arena {
    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        while self
            .lock
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_err()
        {
            spin_loop();
        }
        // SAFETY: the spin lock above is held, so no other thread has access to `state`, and
        // `f` cannot re-enter `with` because it only touches the `State` it is given.
        let r = f(unsafe { &mut *self.state.get() });
        self.lock.store(false, Release);
        r
    }

    /// Whether `ptr` lies inside the reserved region. Lock-free: `base` and `size` never change
    /// after `reserve`.
    fn owns(&self, ptr: *mut u8) -> bool {
        let base = self.base.load(Acquire);
        let addr = ptr as usize;
        base != 0 && addr >= base && addr < base + self.size.load(Relaxed)
    }

    /// Takes a block for `size` bytes at `align`, or `None` if the arena is absent, full, or too
    /// small. A block of class `c` is aligned to its own size, so `align` up to the block size
    /// is met by the class alone.
    fn take(&self, size: usize, align: usize) -> Option<*mut u8> {
        let class = class_of(size.max(align))?;
        let block = block_size(class);
        let base = self.base.load(Acquire);
        if base == 0 {
            return None;
        }
        let capacity = self.size.load(Relaxed);
        self.with(|s| {
            let head = s.free[class];
            let addr = if head != 0 {
                // SAFETY: a non-zero free-list head is a block `give` wrote, inside the region.
                // Its first word is the next link, written by `give` and owned by the arena.
                s.free[class] = unsafe { *(head as *const usize) };
                head
            } else {
                // Bump past the gap that aligns the block. The gap counts toward `reserved`.
                let addr = (base + s.bump).next_multiple_of(block);
                let end = addr + block - base;
                if end > capacity {
                    s.spills += 1;
                    return None;
                }
                s.bump = end;
                addr
            };
            s.live += block;
            s.peak_live = s.peak_live.max(s.live);
            Some(addr as *mut u8)
        })
    }

    /// Returns a block to its class free list.
    ///
    /// # Safety
    /// `ptr` must be a block this arena handed out with the same `size` and `align`, and it must
    /// not be used again.
    unsafe fn give(&self, ptr: *mut u8, size: usize, align: usize) {
        let class = class_of(size.max(align)).expect("arena block has a valid class");
        self.with(|s| {
            // SAFETY: `ptr` is a block of at least 16 bytes inside the region that nothing else
            // references, so its first word is ours to write.
            unsafe { *(ptr as *mut usize) = s.free[class] };
            s.free[class] = ptr as usize;
            s.live -= block_size(class);
        });
    }
}

/// Reserves the arena region. Call once, before the first scope. Returns an error if the region
/// is already reserved or the system allocator cannot provide it.
pub fn reserve(bytes: usize) -> Result<(), &'static str> {
    if ARENA.base.load(Acquire) != 0 {
        return Err("arena already reserved");
    }
    if bytes == 0 {
        return Err("arena size must be nonzero");
    }
    let layout = Layout::from_size_align(bytes, ALIGN).map_err(|_| "bad arena size")?;
    // SAFETY: `layout` has nonzero size, as checked above.
    let base = unsafe { System.alloc(layout) };
    if base.is_null() {
        return Err("cannot reserve arena");
    }
    ARENA.size.store(bytes, Relaxed);
    ARENA.base.store(base as usize, Release);
    Ok(())
}

pub fn arena_stats() -> ArenaStats {
    ARENA.with(|s| ArenaStats {
        reserved: s.bump,
        live: s.live,
        peak_live: s.peak_live,
        spills: s.spills,
    })
}

/// Sets the peak to the current live size, so the next window measures its own peak.
pub fn reset_peak() {
    ARENA.with(|s| s.peak_live = s.live);
}

/// Class index for a block of at least `size` bytes. `size` is already max(size, align).
fn class_of(size: usize) -> Option<usize> {
    if size > 1 << MAX_SHIFT {
        return None;
    }
    let shift = size
        .max(1 << MIN_SHIFT)
        .next_power_of_two()
        .trailing_zeros();
    Some((shift - MIN_SHIFT) as usize)
}

fn block_size(class: usize) -> usize {
    1 << (class as u32 + MIN_SHIFT)
}

/// The routing global allocator. Install with `#[global_allocator]` in the crate root.
#[derive(Debug)]
pub struct Routed;

impl Routed {
    /// Allocation path shared by `alloc` and the move half of `realloc`.
    fn alloc_routed(&self, layout: Layout) -> *mut u8 {
        let scoped = in_scope();
        bump(|s| s.allocs += 1);
        let arena = if scoped {
            ARENA.take(layout.size(), layout.align())
        } else {
            None
        };
        if let Some(p) = arena {
            bump(|s| s.arena += 1);
            return p;
        }
        bump(|s| {
            s.system += 1;
            s.system_in_scope += u64::from(scoped);
        });
        if scoped {
            LAST_VIOLATION.with(|c| c.set((layout.size(), layout.align())));
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract, and we pass it through.
        unsafe { System.alloc(layout) }
    }
}

// SAFETY: each method returns memory from `System` or from the arena region, both of which meet
// `GlobalAlloc`'s contract. Arena blocks are 16-byte aligned, and the arena never hands out a
// block that is still live. A pointer is routed back to the place it came from by address.
// The thread-local cells and the spin lock never allocate, so the allocator does not re-enter.
unsafe impl GlobalAlloc for Routed {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc_routed(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARENA.owns(ptr) {
            // SAFETY: `owns` says `ptr` is inside the region, so it came from `alloc_routed`
            // with this layout, and the caller frees it once.
            unsafe { ARENA.give(ptr, layout.size(), layout.align()) };
        } else {
            // SAFETY: `ptr` is not in the arena, so it came from `System` with this `layout`.
            unsafe { System.dealloc(ptr, layout) };
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let owned = ARENA.owns(ptr);
        if !owned && !in_scope() && layout.align() <= ALIGN {
            // The common off-scope case stays a plain `System` realloc.
            bump(|s| {
                s.allocs += 1;
                s.system += 1;
            });
            // SAFETY: `ptr` is not in the arena, so it came from `System` with `layout`.
            return unsafe { System.realloc(ptr, layout, new_size) };
        }
        // Otherwise the block moves: allocate on the routed side, copy, free the old block on
        // its own side.
        // SAFETY: `new_size` with the old alignment is a valid layout because `layout` is.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        let fresh = self.alloc_routed(new_layout);
        if !fresh.is_null() {
            // SAFETY: `ptr` holds `layout.size()` valid bytes and `fresh` has room for
            // `new_size`. The two blocks are distinct live allocations, so they cannot overlap.
            unsafe { ptr::copy_nonoverlapping(ptr, fresh, layout.size().min(new_size)) };
            // SAFETY: `ptr` is a live block with `layout`, and the caller gave it up.
            unsafe { self.dealloc(ptr, layout) };
        }
        fresh
    }
}
