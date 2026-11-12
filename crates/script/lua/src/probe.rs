//! What the Lua callbacks on this thread cost: wall time, arena allocations,
//! Luau GC steps and arena lock waits. Thread-local cells with no
//! destructors, so the allocator may write them; a reader takes the sum since
//! its last read with [`take`].

use std::cell::Cell;
use std::time::Instant;

/// Totals since the last [`take`] on this thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    /// Callback runs (start, resume, control, save) and their wall time.
    pub runs: u64,
    pub run_ns: u64,
    pub run_ns_max: u64,
    /// Allocations inside an audio scope, and their bytes.
    pub allocs: u64,
    pub bytes: u64,
    /// Luau GC steps and their wall time.
    pub gc_steps: u64,
    pub gc_ns: u64,
    pub gc_ns_max: u64,
    /// Arena lock acquisitions that found the lock held, and the wait.
    pub waits: u64,
    pub wait_ns: u64,
    pub wait_ns_max: u64,
}

thread_local! {
    static PROBE: Cell<Probe> = const { Cell::new(Probe {
        runs: 0, run_ns: 0, run_ns_max: 0, allocs: 0, bytes: 0, gc_steps: 0,
        gc_ns: 0, gc_ns_max: 0, waits: 0, wait_ns: 0, wait_ns_max: 0,
    }) };
    /// The start of the GC step in progress.
    static GC: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn add(f: impl FnOnce(&mut Probe)) {
    let _ = PROBE.try_with(|p| {
        let mut v = p.get();
        f(&mut v);
        p.set(v);
    });
}

fn ns(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// This thread's totals since the last call; the counts start again at zero.
pub fn take() -> Probe {
    PROBE
        .try_with(|p| p.replace(Probe::default()))
        .unwrap_or_default()
}

/// Times one callback run until dropped.
pub(crate) struct Run(Instant);

impl Run {
    pub(crate) fn start() -> Self {
        Self(Instant::now())
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let t = ns(self.0);
        add(|p| {
            p.runs += 1;
            p.run_ns += t;
            p.run_ns_max = p.run_ns_max.max(t);
        });
    }
}

pub(crate) fn alloc(bytes: usize) {
    add(|p| {
        p.allocs += 1;
        p.bytes += bytes as u64;
    });
}

pub(crate) fn waited(since: Instant) {
    let t = ns(since);
    add(|p| {
        p.waits += 1;
        p.wait_ns += t;
        p.wait_ns_max = p.wait_ns_max.max(t);
    });
}

/// Luau's GC interrupt fires once as a step starts and once as it ends.
pub(crate) fn gc_edge() {
    let _ = GC.try_with(|g| match g.take() {
        None => g.set(Some(Instant::now())),
        Some(start) => {
            let t = ns(start);
            add(|p| {
                p.gc_steps += 1;
                p.gc_ns += t;
                p.gc_ns_max = p.gc_ns_max.max(t);
            });
        }
    });
}
