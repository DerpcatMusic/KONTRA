//! A fixed pool of worker threads for real-time block rendering, and the
//! checked exclusive-access types that let tasks mutate disjoint parts of
//! shared state. All of the unsafe code behind multicore rendering lives here;
//! the APIs are safe and verify their own invariants at run time.
//!
//! - [`Pool::run`] executes `tasks` indexed jobs, each exactly once, on the
//!   calling thread plus the workers, and returns when all are done. It
//!   neither allocates nor takes a lock. Idle workers are parked, not
//!   spinning: waking one is a futex call from the caller.
//! - [`Slab`] and [`Disjoint`] hand out `&mut` access to one unit of a
//!   slice through a shared reference. A unit can be held by one claim at a
//!   time; claiming a held unit panics.

mod snapshot;
pub use snapshot::{Snapshot, SnapshotRead};
mod discard;
pub use discard::discard_f32;
mod claim;
mod pool;

pub use claim::{Claim, Claims, Disjoint, Slab};
pub use pool::Pool;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    #[test]
    fn every_task_runs_once_across_many_runs() {
        let mut pool = Pool::new(3);
        let hits: Vec<AtomicUsize> = (0..37).map(|_| AtomicUsize::new(0)).collect();
        for run in 1..=500 {
            pool.run(hits.len(), &|i, _| {
                hits[i].fetch_add(1, Relaxed);
            });
            assert!(hits.iter().all(|h| h.load(Relaxed) == run));
        }
    }

    #[test]
    fn claims_are_exclusive_and_release_on_drop() {
        let slab = Slab::new(vec![0u32; 8].into_boxed_slice(), 4);
        {
            let mut a = slab.claim(0);
            a[1] = 7;
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| slab.claim(0))).is_err());
            assert_eq!(slab.claim(1).len(), 4);
        }
        assert_eq!(slab.claim(0)[1], 7);
    }

    #[test]
    fn tasks_mutate_disjoint_units() {
        let mut pool = Pool::new(2);
        let slab = Slab::new(vec![0u64; 64].into_boxed_slice(), 1);
        pool.run(64, &|i, _| slab.claim(i)[0] = i as u64 * 2);
        assert!((0..64).all(|i| slab.claim(i)[0] == i as u64 * 2));
    }

    #[test]
    fn a_task_panic_surfaces_and_the_pool_survives() {
        let mut pool = Pool::new(2);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pool.run(8, &|i, _| assert!(i != 3));
        }));
        assert!(result.is_err());
        let count = AtomicUsize::new(0);
        pool.run(8, &|_, _| {
            count.fetch_add(1, Relaxed);
        });
        assert_eq!(count.load(Relaxed), 8);
    }
}
