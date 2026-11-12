//! A steady-state callback neither allocates from the system heap nor waits
//! on a lock another thread holds, while that thread loads other scripts.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kontra_lua::{Callbacks, alloc, compile, probe};
use kontra_vm::{Foreign, HostApi, HostCall, HostValue, Step};

struct Host;

impl HostApi for Host {
    fn call(&mut self, _: HostCall<'_>) -> HostValue {
        HostValue::Int(1)
    }
}

/// Builds a few thousand tables per load, and frees them as it drops.
const LOADER: &str = "local t = {} for i = 1, 20000 do t[i] = {i, tostring(i)} end";

#[test]
fn callbacks_take_no_system_memory_and_wait_on_no_lock_while_another_script_loads() {
    let source = "function onNote(e) local t = {} for i = 1, 200 do t[i] = {i, e.note} end end";
    let mut machine = compile(source, &[], &[]).unwrap().machine();
    let stop = Arc::new(AtomicBool::new(false));
    let loader = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut loads = 0u32;
            while !stop.load(Ordering::Relaxed) || loads < 20 {
                drop(compile(LOADER, &[], &[]));
                loads += 1;
            }
            loads
        })
    };
    // Warm up: the state's first notes size its tables and the arena.
    let mut note = || {
        let co = machine.start(Callbacks::NOTE, 0).unwrap();
        assert_eq!(machine.run(co, &mut Host), Step::Done);
    };
    for _ in 0..50 {
        note();
    }
    let spills = alloc::spills();
    let _ = probe::take();
    for _ in 0..2000 {
        note();
    }
    let seen = probe::take();
    stop.store(true, Ordering::Relaxed);
    let loads = loader.join().unwrap();
    assert!(loads >= 20, "the loader ran {loads} loads");
    assert!(seen.allocs > 0, "the callbacks allocate in Lua: {seen:?}");
    assert_eq!(
        alloc::spills(),
        spills,
        "audio allocations went to the system heap"
    );
    assert_eq!(
        seen.waits, 0,
        "the audio thread waited on another thread: {seen:?}"
    );
}

/// What the routing global allocator adds to an allocation outside any Lua
/// scope (the rest of the plugin), against the system heap direct.
/// `cargo test --release -p kontra-lua --test realtime -- --ignored --nocapture`
#[test]
#[ignore = "a timing, not a check"]
fn routed_pass_through_cost() {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::hint::black_box;
    use std::time::Instant;
    // Live states, as a rack holds: KONTRA_STATES (default 2).
    let states: usize = std::env::var("KONTRA_STATES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2);
    let _states: Vec<_> = (0..states)
        .map(|_| compile("x = {1, 2, 3}", &[], &[]).unwrap())
        .collect();
    let layout = Layout::from_size_align(64, 8).unwrap();
    let time = |f: &dyn Fn()| {
        let t = Instant::now();
        for _ in 0..10_000_000 {
            f();
        }
        t.elapsed().as_nanos() as f64 / 1e7
    };
    // SAFETY: each block is freed with the layout it was allocated with.
    #[expect(unsafe_code, reason = "calls the allocators directly")]
    let (routed, system) = unsafe {
        (
            time(&|| std::alloc::dealloc(black_box(std::alloc::alloc(layout)), layout)),
            time(&|| System.dealloc(black_box(System.alloc(layout)), layout)),
        )
    };
    println!("{states} states, alloc+free of 64 B: routed {routed:.1} ns, system {system:.1} ns");
}
