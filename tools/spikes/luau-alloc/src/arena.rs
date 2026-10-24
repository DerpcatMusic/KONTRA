//! Fixed-capacity size-class allocator that backs one Luau state.
//!
//! Memory is reserved once in [`Arena::new`]. The allocation path only pops a free list or
//! bumps a cursor, so it never calls the system allocator.

use std::alloc::{self, Layout};
use std::ffi::c_void;
use std::ptr;

/// Smallest block is 16 bytes, which keeps every block 16-byte aligned.
const MIN_SHIFT: u32 = 4;
/// Largest block is 64 MiB. Luau doubles a 1e5-entry table's array part to 2 MiB, so a 64 KiB
/// cap failed the heap build in the first run. Larger requests return null and Luau raises a
/// memory error.
const MAX_SHIFT: u32 = 26;
const CLASSES: usize = (MAX_SHIFT - MIN_SHIFT + 1) as usize;
const ALIGN: usize = 16;

#[repr(C)]
struct FreeBlock {
    next: *mut FreeBlock,
}

/// Size-class arena. Power-of-two classes never coalesce, so fragmentation is a known risk.
#[derive(Debug)]
pub struct Arena {
    base: *mut u8,
    layout: Layout,
    bump: usize,
    free: [*mut FreeBlock; CLASSES],
    live: usize,
    pub peak_live: usize,
    pub failures: u64,
}

impl Arena {
    /// Reserves `capacity` bytes. The box keeps the arena at a stable address for Luau.
    ///
    /// # Panics
    /// If `capacity` is zero or the system allocator cannot provide it.
    pub fn new(capacity: usize) -> Box<Self> {
        assert!(capacity > 0, "arena capacity must be nonzero");
        let layout = Layout::from_size_align(capacity, ALIGN).expect("arena layout is valid");
        // SAFETY: `layout` has nonzero size (asserted above), so `alloc` is allowed to be called.
        let base = unsafe { alloc::alloc(layout) };
        assert!(!base.is_null(), "cannot reserve a {capacity} byte arena");
        Box::new(Self {
            base,
            layout,
            bump: 0,
            free: [ptr::null_mut(); CLASSES],
            live: 0,
            peak_live: 0,
            failures: 0,
        })
    }

    /// Bytes handed out by the bump cursor so far, including blocks now on free lists.
    pub fn reserved(&self) -> usize {
        self.bump
    }

    fn acquire(&mut self, class: usize) -> *mut u8 {
        let size = block_size(class);
        let head = self.free[class];
        let p = if head.is_null() {
            if self.bump + size > self.layout.size() {
                self.failures += 1;
                return ptr::null_mut();
            }
            // SAFETY: `bump + size <= capacity`, so the offset stays inside the reserved block.
            let p = unsafe { self.base.add(self.bump) };
            self.bump += size;
            p
        } else {
            // SAFETY: a non-null head is a block this arena wrote in `release` and still owns.
            self.free[class] = unsafe { (*head).next };
            head.cast::<u8>()
        };
        self.live += size;
        self.peak_live = self.peak_live.max(self.live);
        p
    }

    /// Returns a block to its class free list.
    ///
    /// # Safety
    /// `ptr` must be a live block of class `class` that this arena handed out and that Luau
    /// no longer references.
    unsafe fn release(&mut self, ptr: *mut u8, class: usize) {
        let block = ptr.cast::<FreeBlock>();
        // SAFETY: per the contract, `ptr` is a live 16-byte-aligned block of at least 16 bytes
        // that nothing else references, so writing the link is in bounds.
        unsafe { (*block).next = self.free[class] };
        self.free[class] = block;
        self.live -= block_size(class);
    }

    /// Luau `lua_Alloc` semantics: `nsize == 0` frees, a null `ptr` allocates, and otherwise
    /// the block is resized. On failure it returns null and the old block stays valid.
    ///
    /// # Safety
    /// `ptr` must be null or a block this arena returned, whose size is `osize`.
    unsafe fn realloc(&mut self, ptr: *mut u8, osize: usize, nsize: usize) -> *mut u8 {
        if nsize == 0 {
            if !ptr.is_null() {
                // SAFETY: per the contract, `ptr` is a block of size `osize` from this arena.
                unsafe { self.release(ptr, class_for(osize)) };
            }
            return ptr::null_mut();
        }
        let Some(new_class) = class_of(nsize) else {
            self.failures += 1;
            return ptr::null_mut();
        };
        if ptr.is_null() {
            return self.acquire(new_class);
        }
        let old_class = class_for(osize);
        if old_class == new_class {
            return ptr;
        }
        let fresh = self.acquire(new_class);
        if fresh.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: `ptr` holds `osize` valid bytes and `fresh` has room for `nsize` bytes. They
        // cannot overlap because `ptr` is still allocated and `fresh` came from a free list or
        // the untouched bump region.
        unsafe { ptr::copy_nonoverlapping(ptr, fresh, osize.min(nsize)) };
        // SAFETY: `ptr` is a block of class `old_class` that Luau no longer uses.
        unsafe { self.release(ptr, old_class) };
        fresh
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: `base` came from `alloc::alloc` with exactly `self.layout` in `new`, and is
        // freed only here.
        unsafe { alloc::dealloc(self.base, self.layout) };
    }
}

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

/// Class of a size that Luau previously allocated from this arena.
fn class_for(size: usize) -> usize {
    class_of(size).expect("block size outside the arena classes")
}

fn block_size(class: usize) -> usize {
    1 << (class as u32 + MIN_SHIFT)
}

/// `lua_Alloc` callback for a state created with [`Arena`] as `ud`.
///
/// # Safety
/// `ud` must be the `*mut Arena` given to `lua_newstate`, alive and unaliased for the state's
/// whole lifetime. `ptr` must be null or a block this callback returned, with size `osize`.
pub unsafe extern "C" fn luau_alloc(
    ud: *mut c_void,
    ptr: *mut c_void,
    osize: usize,
    nsize: usize,
) -> *mut c_void {
    // SAFETY: per the contract, `ud` is the live arena for this state. Luau does not call the
    // allocator re-entrantly on the same state, so this `&mut` is the only one in use.
    let arena = unsafe { &mut *(ud as *mut Arena) };
    // SAFETY: the caller's contract on `ptr` and `osize` is exactly `realloc`'s contract.
    unsafe { arena.realloc(ptr.cast(), osize, nsize) }.cast()
}
