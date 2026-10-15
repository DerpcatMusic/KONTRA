//! Control-owned PCM payload storage. A lease follows the physical immutable
//! generation through preparation, publication and retirement, until reclamation.
use crate::Error;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Debug)]
pub struct HeadBudget(Arc<State>);
#[derive(Debug)]
struct State {
    limit: usize,
    held: AtomicUsize,
    peak: AtomicUsize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadBudgetStatus {
    pub limit_bytes: usize,
    pub held_bytes: usize,
    pub peak_bytes: usize,
}
impl HeadBudget {
    /// Construct off audio. Explicitly bound packed heads and ResidentBuffers
    /// share this payload cap. Page pools, source metadata, octave levels and
    /// separately bounded decoder workspaces are outside it.
    pub fn new(limit_bytes: usize) -> Self {
        Self(Arc::new(State {
            limit: limit_bytes,
            held: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }))
    }
    pub fn status(&self) -> HeadBudgetStatus {
        let held = self.0.held.load(Ordering::Acquire);
        HeadBudgetStatus {
            limit_bytes: self.0.limit,
            held_bytes: held,
            peak_bytes: self.0.peak.load(Ordering::Acquire).max(held),
        }
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    #[allow(
        deprecated,
        reason = "fetch_update preserves the declared Rust 1.92 minimum"
    )]
    pub(super) fn reserve(&self, bytes: usize) -> Result<Lease, Error> {
        let previous = self
            .0
            .held
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(bytes).filter(|n| *n <= self.0.limit)
            })
            .map_err(|_| Error::Capacity)?;
        self.0.peak.fetch_max(previous + bytes, Ordering::AcqRel);
        Ok(Lease {
            budget: self.clone(),
            bytes,
        })
    }
}

/// Never cloned or destroyed by render readers. Its owning generation is
/// reclaimed by Snapshot's control-side collection, after the final audio guard.
#[derive(Debug)]
pub(super) struct Lease {
    budget: HeadBudget,
    bytes: usize,
}
impl Lease {
    pub(super) fn belongs_to(&self, budget: &HeadBudget) -> bool {
        self.budget.same(budget)
    }
    pub(super) fn extend(&mut self, bytes: usize) -> Result<(), Error> {
        let mut extra = self.budget.reserve(bytes)?;
        self.bytes = self.bytes.checked_add(bytes).ok_or(Error::Capacity)?;
        extra.bytes = 0;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let previous = self.budget.0.held.fetch_sub(self.bytes, Ordering::AcqRel);
        debug_assert!(previous >= self.bytes);
    }
}
