//! The Luau heap's arena (04-SCRIPT.md §4, "Real-time safety"; option A of
//! `spikes/luau-alloc-mlua.md`). mlua's Luau allocator calls the Rust global
//! allocator, so [`Routed`] as the global allocator sends every allocation
//! made inside a [`Scope`] (Luau's and the callback's own) to one TLSF arena,
//! and everything else to the system heap. A free goes back by address, to
//! the arena whatever thread frees and whether or not a scope is open.
//!
//! A load scope grows the arena from the system heap; an audio scope never
//! does. An audio allocation that does not fit spills to the system heap and
//! is counted ([`spills`]): a sizing bug, never an abort.
//!
//! ponytail: one spin lock guards the arena, so another thread freeing an
//! arena block can briefly hold up the audio thread; a deferred-free queue
//! drained on the audio thread removes that if it shows up in a profile.

// The global allocator is the one FFI-shaped module here (09-BUILD.md §1.8):
// each unsafe block carries its SAFETY note.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::hint::spin_loop;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rlsf::Tlsf;

/// Largest block `(32 << 25) - 32` bytes, about 1 GiB; second-level classes of 32.
type Heap = Tlsf<'static, u32, u32, 25, 32>;
/// Pools the arena may hold; each load adds at most a few.
const POOLS: usize = 64;
/// Smallest pool added at a time.
const GROW: usize = 4 << 20;
const PAGE: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Load,
    Audio,
}

thread_local! {
    /// This thread's scope. Const-initialised with no destructor, so reading
    /// it never allocates.
    static MODE: Cell<Mode> = const { Cell::new(Mode::Off) };
    /// This thread's audio-scope spills, so one audio thread's count is not
    /// moved by another's.
    static SPILLS: Cell<u64> = const { Cell::new(0) };
}

fn mode() -> Mode {
    MODE.try_with(Cell::get).unwrap_or(Mode::Off)
}

/// Sends this thread's allocations to the arena until dropped. Scopes nest.
#[must_use = "the scope ends when this guard is dropped"]
pub struct Scope(Mode);

impl Scope {
    /// Building a script: the arena grows to fit.
    pub fn load() -> Self {
        Self(MODE.replace(Mode::Load))
    }

    /// Running a callback on the audio thread: the arena never grows.
    pub fn audio() -> Self {
        Self(MODE.replace(Mode::Audio))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        MODE.set(self.0);
    }
}

struct Arena {
    lock: AtomicBool,
    heap: UnsafeCell<Heap>,
    /// `[start, end)` of each pool; the first `count` are published.
    pools: [(AtomicUsize, AtomicUsize); POOLS],
    count: AtomicUsize,
    pooled: AtomicUsize,
    live: AtomicUsize,
}

// SAFETY: `heap` is only reached through `Arena::locked`, which holds `lock`;
// every other field is atomic.
unsafe impl Sync for Arena {}

static ARENA: Arena = Arena {
    lock: AtomicBool::new(false),
    heap: UnsafeCell::new(Tlsf::new()),
    pools: [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; POOLS],
    count: AtomicUsize::new(0),
    pooled: AtomicUsize::new(0),
    live: AtomicUsize::new(0),
};

impl Arena {
    fn locked<R>(&self, f: impl FnOnce(&mut Heap) -> R) -> R {
        while self
            .lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spin_loop();
        }
        // SAFETY: the lock is held, so this is the only reference to the heap.
        let result = f(unsafe { &mut *self.heap.get() });
        self.lock.store(false, Ordering::Release);
        result
    }

    fn owns(&self, p: *mut u8) -> bool {
        let at = p as usize;
        let count = self.count.load(Ordering::Acquire);
        self.pools.iter().take(count).any(|(start, end)| {
            (start.load(Ordering::Relaxed)..end.load(Ordering::Relaxed)).contains(&at)
        })
    }

    /// Allocates a pool of at least `bytes`, its pages touched now so the
    /// audio thread never faults them in. Runs outside the lock: a load
    /// growing the arena never stalls the audio thread.
    fn prepare(bytes: usize) -> Option<(NonNull<u8>, Layout)> {
        let size = bytes.max(GROW).next_multiple_of(PAGE);
        let layout = Layout::from_size_align(size, PAGE).ok()?;
        // SAFETY: `layout` has a non-zero size.
        let block = NonNull::new(unsafe { System.alloc(layout) })?;
        // Volatile, one write per page: a plain zeroing after `alloc` may be
        // turned into `calloc`, which leaves the pages unmapped.
        for page in (0..size).step_by(PAGE) {
            // SAFETY: `page < size`, inside the block.
            unsafe { ptr::write_volatile(block.as_ptr().add(page), 0) };
        }
        Some((block, layout))
    }

    /// Hands a prepared pool to the heap; false when every pool slot is
    /// taken. The pool is never freed.
    fn publish(&self, heap: &mut Heap, block: NonNull<u8>, size: usize) -> bool {
        let count = self.count.load(Ordering::Relaxed);
        let Some((start, end)) = self.pools.get(count) else {
            return false;
        };
        start.store(block.as_ptr() as usize, Ordering::Relaxed);
        end.store(block.as_ptr() as usize + size, Ordering::Relaxed);
        self.count.store(count + 1, Ordering::Release);
        // SAFETY: the heap owns the block from here on and it is never freed,
        // so it outlives the heap.
        unsafe { heap.insert_free_block_ptr(NonNull::slice_from_raw_parts(block, size)) };
        self.pooled.fetch_add(size, Ordering::Relaxed);
        true
    }

    fn take(&self, heap: &mut Heap, layout: Layout) -> Option<*mut u8> {
        let block = heap.allocate(layout)?;
        self.live.fetch_add(layout.size(), Ordering::Relaxed);
        Some(block.as_ptr())
    }

    fn alloc(&self, layout: Layout, grow: bool) -> *mut u8 {
        if let Some(block) = self.locked(|heap| self.take(heap, layout)) {
            return block;
        }
        // Room for the block rounded up to the next size class (TLSF takes a
        // good fit, up to 1/32 above), its alignment and the pool's headers.
        let room = layout
            .size()
            .saturating_add(layout.size() / 16)
            .saturating_add(layout.align() + PAGE);
        let Some((pool, pool_layout)) = grow.then(|| Self::prepare(room)).flatten() else {
            return ptr::null_mut();
        };
        let (block, published) = self.locked(|heap| {
            if self.publish(heap, pool, pool_layout.size()) {
                (self.take(heap, layout), true)
            } else {
                (None, false)
            }
        });
        if !published {
            // SAFETY: allocated by `prepare` with this layout and not shared.
            unsafe { System.dealloc(pool.as_ptr(), pool_layout) };
        }
        block.unwrap_or(ptr::null_mut())
    }
}

/// The global allocator: the arena inside a [`Scope`], the system heap
/// outside.
pub struct Routed;

// SAFETY: every block comes from `System` or the arena's heap with its
// layout, and goes back to the one that owns its address with that layout.
unsafe impl GlobalAlloc for Routed {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let m = mode();
        if m != Mode::Off {
            let block = ARENA.alloc(layout, m == Mode::Load);
            if !block.is_null() {
                return block;
            }
            let _ = SPILLS.try_with(|n| n.set(n.get() + 1));
        }
        // SAFETY: the caller's layout has a non-zero size.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if mode() == Mode::Off {
            // SAFETY: as `alloc`.
            return unsafe { System.alloc_zeroed(layout) };
        }
        // SAFETY: as `alloc`; a non-null block has `layout.size()` bytes.
        let block = unsafe { self.alloc(layout) };
        if !block.is_null() {
            // SAFETY: see above.
            unsafe { ptr::write_bytes(block, 0, layout.size()) };
        }
        block
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        if let Some(block) = NonNull::new(p).filter(|_| ARENA.owns(p)) {
            ARENA.locked(|heap| {
                // SAFETY: `block` lies in a pool, so the heap allocated it,
                // with `layout`.
                unsafe { heap.deallocate(block, layout.align()) };
            });
            ARENA.live.fetch_sub(layout.size(), Ordering::Relaxed);
        } else {
            // SAFETY: not the arena's, so `System` allocated it with `layout`.
            unsafe { System.dealloc(p, layout) }
        }
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let owned = ARENA.owns(p);
        let m = mode();
        if !owned && m == Mode::Off {
            // SAFETY: the caller's contract, and `System` owns `p`.
            return unsafe { System.realloc(p, layout, new_size) };
        }
        let Ok(new) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        if owned
            && m != Mode::Off
            && let Some(block) = NonNull::new(p)
        {
            // SAFETY: the heap allocated `block` with `layout.align()`, which
            // `new` keeps.
            let moved = ARENA.locked(|heap| unsafe { heap.reallocate(block, new) });
            if let Some(moved) = moved {
                ARENA.live.fetch_add(new_size, Ordering::Relaxed);
                ARENA.live.fetch_sub(layout.size(), Ordering::Relaxed);
                return moved.as_ptr();
            }
        }
        // Between the two heaps, or the arena is full: allocate, copy, free.
        // SAFETY: `new` has a non-zero size (the caller's contract).
        let block = unsafe { self.alloc(new) };
        if !block.is_null() {
            // SAFETY: both blocks hold at least the bytes copied, and differ.
            unsafe {
                ptr::copy_nonoverlapping(p, block, layout.size().min(new_size));
                self.dealloc(p, layout);
            }
        }
        block
    }
}

/// Makes sure the arena has at least `bytes` free (by its count; TLSF
/// fragmentation is not measured). Called after a load, off the audio thread.
pub fn reserve(bytes: usize) {
    let pooled = ARENA.pooled.load(Ordering::Relaxed);
    let live = ARENA.live.load(Ordering::Relaxed);
    if pooled.saturating_sub(live) >= bytes {
        return;
    }
    let Some((pool, layout)) = Arena::prepare(bytes) else {
        return;
    };
    if !ARENA.locked(|heap| ARENA.publish(heap, pool, layout.size())) {
        // SAFETY: allocated by `prepare` with this layout and not shared.
        unsafe { System.dealloc(pool.as_ptr(), layout) };
    }
}

/// This thread's audio-scope allocations that did not fit the arena and went
/// to the system heap. Zero is the target.
pub fn spills() -> u64 {
    SPILLS.try_with(Cell::get).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{ARENA, Scope, reserve, spills};

    #[test]
    fn scoped_blocks_come_from_the_arena_and_go_back_by_address() {
        reserve(1 << 20);
        let (inside, before) = {
            let _arena = Scope::audio();
            let mut v = vec![1u8; 1000];
            // Growing in scope stays in the arena.
            v.resize(100_000, 2);
            (v, spills())
        };
        assert!(ARENA.owns(inside.as_ptr().cast_mut()));
        assert_eq!(inside.get(999..1001), Some(&[1, 2][..]));
        // Growing out of scope moves the bytes to the system heap.
        let mut moved = inside;
        moved.resize(200_000, 3);
        assert!(!ARENA.owns(moved.as_ptr().cast_mut()));
        assert_eq!(moved.get(99_999..100_001), Some(&[2, 3][..]));
        let outside = vec![0u8; 1000];
        assert!(!ARENA.owns(outside.as_ptr().cast_mut()));
        assert_eq!(spills(), before);
    }

    #[test]
    fn a_load_scope_grows_the_arena() {
        let _arena = Scope::load();
        let big = vec![0u8; 40 << 20];
        assert!(ARENA.owns(big.as_ptr().cast_mut()));
    }
}
