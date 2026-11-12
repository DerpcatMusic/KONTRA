//! The Luau heaps' arenas (04-SCRIPT.md §4, "Real-time safety"; option A of
//! `spikes/luau-alloc-mlua.md`). mlua's Luau allocator calls the Rust global
//! allocator, so [`Routed`] as the global allocator sends every allocation
//! made inside a [`Scope`] (Luau's and the callback's own) to the TLSF arena
//! of the Lua state that opened it, and everything else to the system heap. A
//! free goes back by address, to the arena that owns it, whatever thread
//! frees and whether or not a scope is open.
//!
//! Each Lua state holds its own arena ([`Claim`]), so a script loading on
//! another thread never takes the lock, or the free space, of a state playing
//! on the audio thread. A load scope grows its arena from the system heap; an
//! audio scope never does. An audio allocation that does not fit spills to
//! the system heap and is counted ([`spills`]): a sizing bug, never an abort.
//!
//! ponytail: past [`ARENAS`] states at once, the rest share arena 0 and its
//! lock again (the old single-arena design); raise the count if a host shows it.

// The global allocator is the one FFI-shaped module here (09-BUILD.md §1.8):
// each unsafe block carries its SAFETY note.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::hint::spin_loop;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use rlsf::Tlsf;

/// Largest block `(32 << 25) - 32` bytes, about 1 GiB; second-level classes of 32.
type Heap = Tlsf<'static, u32, u32, 25, 32>;
/// Lua states that each hold an arena of their own; arena 0 is shared by any
/// past that.
pub const ARENAS: usize = 64;
/// Pools are aligned to, and a whole number of, granules, so each granule
/// belongs to one arena; also the smallest pool added at a time.
const GRANULE: usize = 4 << 20;
const SHIFT: u32 = GRANULE.trailing_zeros();
/// Slots of the granule → arena table, kept at most half full: 32 GiB of
/// pools in all.
const SLOTS: usize = 1 << 14;
const PAGE: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Load,
    Audio,
}

thread_local! {
    /// This thread's scope and its arena. Const-initialised with no
    /// destructor, so reading it never allocates.
    static MODE: Cell<(Mode, usize)> = const { Cell::new((Mode::Off, 0)) };
    /// This thread's audio-scope spills, so one audio thread's count is not
    /// moved by another's.
    static SPILLS: Cell<u64> = const { Cell::new(0) };
}

fn mode() -> (Mode, usize) {
    MODE.try_with(Cell::get).unwrap_or((Mode::Off, 0))
}

/// One Lua state's arena, held for the state's life and given back on drop.
/// Drop it after the state (and anything allocated in its scopes).
#[derive(Debug)]
pub struct Claim(usize);

impl Claim {
    /// A free arena, or the shared arena 0 when all [`ARENAS`] are held. An
    /// arena given back keeps its pools for the next state.
    #[must_use]
    pub fn new() -> Self {
        let free = (ARENA.iter().enumerate().skip(1)).find(|(_, a)| {
            a.claimed
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        });
        Self(free.map_or(0, |(n, _)| n))
    }

    /// Building the state: the arena grows to fit.
    pub fn load(&self) -> Scope {
        Scope(MODE.replace((Mode::Load, self.0)))
    }

    /// Running a callback on the audio thread: the arena never grows.
    pub fn audio(&self) -> Scope {
        Scope(MODE.replace((Mode::Audio, self.0)))
    }

    /// Makes sure the arena has at least `bytes` free (by its count; TLSF
    /// fragmentation is not measured). Called after a load, off the audio thread.
    pub fn reserve(&self, bytes: usize) {
        if let Some(arena) = ARENA.get(self.0) {
            arena.reserve(bytes);
        }
    }
}

impl Default for Claim {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Some(arena) = ARENA.get(self.0).filter(|_| self.0 != 0) {
            arena.claimed.store(false, Ordering::Release);
        }
    }
}

/// Sends this thread's allocations to an arena until dropped. Scopes nest.
#[must_use = "the scope ends when this guard is dropped"]
pub struct Scope((Mode, usize));

impl Drop for Scope {
    fn drop(&mut self) {
        MODE.set(self.0);
    }
}

struct Arena {
    lock: AtomicBool,
    heap: UnsafeCell<Heap>,
    pooled: AtomicUsize,
    live: AtomicUsize,
    claimed: AtomicBool,
}

// SAFETY: `heap` is only reached through `Arena::locked`, which holds `lock`;
// every other field is atomic.
unsafe impl Sync for Arena {}

static ARENA: [Arena; ARENAS] = [const {
    Arena {
        lock: AtomicBool::new(false),
        heap: UnsafeCell::new(Tlsf::new()),
        pooled: AtomicUsize::new(0),
        live: AtomicUsize::new(0),
        claimed: AtomicBool::new(false),
    }
}; ARENAS];
/// Each granule of a pool, as `granule << 8 | arena`; 0 is free. Open
/// addressing, never removed (a pool is never freed). The lookup costs the
/// same however many arenas there are: it is on every free in the process.
static OWNERS: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
/// Granules registered, to keep [`OWNERS`] at most half full.
static GRANULES: AtomicUsize = AtomicUsize::new(0);

/// The table's slots in the order granule `g` probes them.
fn slots(g: u64) -> impl Iterator<Item = &'static AtomicU64> {
    let start = (g.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - SLOTS.trailing_zeros())) as usize;
    (0..SLOTS).filter_map(move |i| OWNERS.get((start + i) % SLOTS))
}

fn granule(at: usize) -> u64 {
    (at >> SHIFT) as u64
}

/// The arena whose pool holds `p`.
fn owner(p: *mut u8) -> Option<&'static Arena> {
    let g = granule(p as usize);
    for slot in slots(g) {
        match slot.load(Ordering::Acquire) {
            0 => return None,
            e if e >> 8 == g => return ARENA.get((e & 0xFF) as usize),
            _ => {}
        }
    }
    None
}

impl Arena {
    fn locked<R>(&self, f: impl FnOnce(&mut Heap) -> R) -> R {
        let acquire = || {
            self.lock
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        };
        if !acquire() {
            let since = std::time::Instant::now();
            while !acquire() {
                spin_loop();
            }
            crate::probe::waited(since);
        }
        // SAFETY: the lock is held, so this is the only reference to the heap.
        let result = f(unsafe { &mut *self.heap.get() });
        self.lock.store(false, Ordering::Release);
        result
    }

    /// Allocates a pool of at least `bytes` and a quarter of what the arena
    /// holds, its pages touched now so the audio thread never faults them
    /// in. Runs outside the lock.
    fn prepare(&self, bytes: usize) -> Option<(NonNull<u8>, Layout)> {
        let size = (bytes
            .max(GRANULE)
            .max(self.pooled.load(Ordering::Relaxed) / 4))
        .next_multiple_of(GRANULE);
        let layout = Layout::from_size_align(size, GRANULE).ok()?;
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

    /// Hands a prepared pool to the heap; false when the owner table is
    /// full. The pool is never freed.
    fn publish(&self, heap: &mut Heap, block: NonNull<u8>, size: usize) -> bool {
        let Some(index) = ARENA.iter().position(|a| ptr::eq(a, self)) else {
            return false;
        };
        let count = size >> SHIFT;
        if GRANULES.fetch_add(count, Ordering::Relaxed) + count > SLOTS / 2 {
            GRANULES.fetch_sub(count, Ordering::Relaxed);
            return false;
        }
        let first = granule(block.as_ptr() as usize);
        for g in (first..).take(count) {
            let entry = g << 8 | index as u64;
            // At most half full, so a free slot is always found.
            let _ = slots(g).find(|slot| {
                slot.compare_exchange(0, entry, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
            });
        }
        // SAFETY: the heap owns the block from here on and it is never freed,
        // so it outlives the heap.
        unsafe { heap.insert_free_block_ptr(NonNull::slice_from_raw_parts(block, size)) };
        self.pooled.fetch_add(size, Ordering::Relaxed);
        true
    }

    /// Publishes a prepared pool, or gives it back when no slot is left.
    fn add(&self, pool: NonNull<u8>, layout: Layout, then: Option<Layout>) -> Option<*mut u8> {
        let (block, published) = self.locked(|heap| {
            if self.publish(heap, pool, layout.size()) {
                (then.and_then(|l| self.take(heap, l)), true)
            } else {
                (None, false)
            }
        });
        if !published {
            // SAFETY: allocated by `prepare` with this layout and not shared.
            unsafe { System.dealloc(pool.as_ptr(), layout) };
        }
        block
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
        let Some((pool, pool_layout)) = grow.then(|| self.prepare(room)).flatten() else {
            return ptr::null_mut();
        };
        self.add(pool, pool_layout, Some(layout))
            .unwrap_or(ptr::null_mut())
    }

    fn reserve(&self, bytes: usize) {
        let pooled = self.pooled.load(Ordering::Relaxed);
        let live = self.live.load(Ordering::Relaxed);
        if pooled.saturating_sub(live) >= bytes {
            return;
        }
        if let Some((pool, layout)) = self.prepare(bytes) {
            self.add(pool, layout, None);
        }
    }
}

/// The global allocator: a Lua state's arena inside a [`Scope`], the system
/// heap outside.
pub struct Routed;

// SAFETY: every block comes from `System` or an arena's heap with its layout,
// and goes back to the one that owns its address with that layout.
unsafe impl GlobalAlloc for Routed {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (m, n) = mode();
        if m != Mode::Off {
            if m == Mode::Audio {
                crate::probe::alloc(layout.size());
            }
            if let Some(arena) = ARENA.get(n) {
                let block = arena.alloc(layout, m == Mode::Load);
                if !block.is_null() {
                    return block;
                }
            }
            let _ = SPILLS.try_with(|s| s.set(s.get() + 1));
        }
        // SAFETY: the caller's layout has a non-zero size.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if mode().0 == Mode::Off {
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
        match NonNull::new(p).and_then(|block| Some((block, owner(p)?))) {
            Some((block, arena)) => {
                arena.locked(|heap| {
                    // SAFETY: `block` lies in this arena's pool, so its heap
                    // allocated it, with `layout`.
                    unsafe { heap.deallocate(block, layout.align()) };
                });
                arena.live.fetch_sub(layout.size(), Ordering::Relaxed);
            }
            // SAFETY: no arena's, so `System` allocated it with `layout`.
            None => unsafe { System.dealloc(p, layout) },
        }
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let owned = owner(p);
        let (m, n) = mode();
        if owned.is_none() && m == Mode::Off {
            // SAFETY: the caller's contract, and `System` owns `p`.
            return unsafe { System.realloc(p, layout, new_size) };
        }
        let Ok(new) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        // In place only within the scope's own arena.
        let scoped = ARENA.get(n).filter(|_| m != Mode::Off);
        if let (Some(arena), Some(mine)) = (owned, scoped)
            && ptr::eq(arena, mine)
            && let Some(block) = NonNull::new(p)
        {
            // SAFETY: the heap allocated `block` with `layout.align()`, which
            // `new` keeps.
            let moved = arena.locked(|heap| unsafe { heap.reallocate(block, new) });
            if let Some(moved) = moved {
                if m == Mode::Audio {
                    crate::probe::alloc(new_size);
                }
                arena.live.fetch_add(new_size, Ordering::Relaxed);
                arena.live.fetch_sub(layout.size(), Ordering::Relaxed);
                return moved.as_ptr();
            }
        }
        // Between heaps, or the arena is full: allocate, copy, free.
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

/// This thread's audio-scope allocations that did not fit their arena and
/// went to the system heap. Zero is the target.
pub fn spills() -> u64 {
    SPILLS.try_with(Cell::get).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{Claim, owner, spills};

    #[test]
    fn scoped_blocks_come_from_their_arena_and_go_back_by_address() {
        let arena = Claim::new();
        let other = Claim::new();
        arena.reserve(1 << 20);
        let (inside, before) = {
            let _scope = arena.audio();
            let mut v = vec![1u8; 1000];
            // Growing in scope stays in the arena.
            v.resize(100_000, 2);
            (v, spills())
        };
        let mine = owner(inside.as_ptr().cast_mut()).expect("in an arena");
        assert_eq!(inside.get(999..1001), Some(&[1, 2][..]));
        // Another state's load grows its own arena, not this one.
        let theirs = {
            let _scope = other.load();
            vec![0u8; 1 << 20]
        };
        let theirs_owner = owner(theirs.as_ptr().cast_mut()).expect("in an arena");
        assert!(!std::ptr::eq(mine, theirs_owner));
        // Growing out of scope moves the bytes to the system heap.
        let mut moved = inside;
        moved.resize(200_000, 3);
        assert!(owner(moved.as_ptr().cast_mut()).is_none());
        assert_eq!(moved.get(99_999..100_001), Some(&[2, 3][..]));
        let outside = vec![0u8; 1000];
        assert!(owner(outside.as_ptr().cast_mut()).is_none());
        assert_eq!(spills(), before);
    }

    #[test]
    fn a_load_scope_grows_the_arena() {
        let arena = Claim::new();
        let _scope = arena.load();
        let big = vec![0u8; 40 << 20];
        assert!(owner(big.as_ptr().cast_mut()).is_some());
    }
}
