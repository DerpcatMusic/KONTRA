//! Address and scope routing of the arena allocator. One test function, so the process-wide
//! arena is not shared with a concurrent test.

use std::alloc::{GlobalAlloc, Layout};

use luau_alloc_mlua::GLOBAL;
use luau_alloc_mlua::routed::{ArenaScope, arena_stats, reserve, stats};

/// Allocates through the routing allocator. The caller frees with [`free`] and the same layout.
fn alloc(layout: Layout) -> *mut u8 {
    // SAFETY: every layout passed here has a nonzero size.
    unsafe { GLOBAL.alloc(layout) }
}

fn free(ptr: *mut u8, layout: Layout) {
    // SAFETY: `ptr` came from `alloc` with this `layout` and is freed once.
    unsafe { GLOBAL.dealloc(ptr, layout) }
}

#[test]
fn routing_follows_scope_and_address() {
    reserve(16 << 20).expect("reserve");
    let small = Layout::from_size_align(100, 8).expect("layout");

    // Inside a scope, a small allocation is served by the arena.
    let before = stats();
    let p = {
        let _scope = ArenaScope::enter();
        alloc(small)
    };
    assert_eq!(stats().since(before).arena, 1);
    assert_eq!(stats().since(before).system_in_scope, 0);

    // Freed outside any scope, it still returns to the arena: the next scoped allocation of
    // the same class reuses the same address.
    free(p, small);
    let q = {
        let _scope = ArenaScope::enter();
        alloc(small)
    };
    assert_eq!(
        q, p,
        "arena block freed off-scope was not routed back to the arena"
    );
    free(q, small);

    // A block allocated off-scope (System) and freed inside a scope goes to System, and that
    // free is not a violation.
    let sys = alloc(small);
    let before = stats();
    {
        let _scope = ArenaScope::enter();
        free(sys, small);
    }
    assert_eq!(stats().since(before).system_in_scope, 0);

    // A scoped allocation that does not fit spills to System and counts as a violation.
    let big = Layout::from_size_align(32 << 20, 16).expect("layout");
    let spills = arena_stats().spills;
    let before = stats();
    let b = {
        let _scope = ArenaScope::enter();
        alloc(big)
    };
    let d = stats().since(before);
    assert_eq!((d.system, d.system_in_scope), (1, 1));
    assert!(arena_stats().spills > spills);
    free(b, big);

    // realloc of a System block inside a scope moves it to the arena with the bytes intact.
    let grow_from = Layout::from_size_align(64, 8).expect("layout");
    let r = alloc(grow_from);
    // SAFETY: `r` is a live 64-byte block, so writing its 64 bytes is in bounds.
    unsafe { r.write_bytes(0xAB, 64) };
    let before = stats();
    let r2 = {
        let _scope = ArenaScope::enter();
        // SAFETY: `r` is a live block of `grow_from`, and 512 is a valid new size.
        unsafe { GLOBAL.realloc(r, grow_from, 512) }
    };
    assert_eq!(stats().since(before).system_in_scope, 0);
    // SAFETY: `r2` holds at least the 64 copied bytes, and they are read before it is freed.
    let kept = unsafe { std::slice::from_raw_parts(r2, 64) };
    assert!(kept.iter().all(|&byte| byte == 0xAB), "realloc lost bytes");
    free(r2, Layout::from_size_align(512, 8).expect("layout"));

    // An over-aligned in-scope block (640 B at 128 B, seen in the real hosts) is served by the
    // arena at its alignment.
    let aligned = Layout::from_size_align(640, 128).expect("layout");
    let before = stats();
    let a = {
        let _scope = ArenaScope::enter();
        alloc(aligned)
    };
    assert_eq!(stats().since(before).system_in_scope, 0);
    assert_eq!(a as usize % 128, 0, "arena block is not 128-byte aligned");
    free(a, aligned);

    // Cross-thread: a block allocated in a scope on another thread, freed on this thread
    // outside any scope, returns to the arena.
    let addr = std::thread::spawn(move || {
        let _scope = ArenaScope::enter();
        alloc(small) as usize
    })
    .join()
    .expect("thread");
    free(addr as *mut u8, small);
    let again = {
        let _scope = ArenaScope::enter();
        alloc(small)
    };
    assert_eq!(
        again as usize, addr,
        "cross-thread free did not return to the arena"
    );
    free(again, small);
}
