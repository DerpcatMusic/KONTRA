//! Deterministic work budget for one callback or initialization.
//!
//! Work is counted in Luau interrupt checkpoints (calls and loop back-edges),
//! so the bound does not depend on scheduler timing. The wall deadline is an
//! observation only; it never refuses work by itself.

use std::{
    cell::Cell,
    time::Instant,
};

/// Raised when a segment spends its checkpoint allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetExhausted;

impl std::fmt::Display for BudgetExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("work budget exceeded")
    }
}

impl From<BudgetExhausted> for mlua::Error {
    fn from(_: BudgetExhausted) -> Self {
        mlua::Error::runtime("work budget exceeded")
    }
}

#[derive(Debug, Default)]
pub struct Budget {
    /// Every checkpoint taken so far (observation).
    pub checkpoints: Cell<u64>,
    /// Checkpoints left in the current segment.
    pub remaining: Cell<u64>,
    /// Set once a segment ran out; cleared by `refill`.
    pub exhausted: Cell<bool>,
    /// Elapsed-time observation for the current segment.
    pub deadline: Cell<Option<Instant>>,
}

impl Budget {
    pub fn new(work: u64, deadline: Option<Instant>) -> Self {
        Self {
            checkpoints: Cell::new(0),
            remaining: Cell::new(work),
            exhausted: Cell::new(false),
            deadline: Cell::new(deadline),
        }
    }

    /// Start a new segment with `work` checkpoints. A segment that was already
    /// exhausted stays exhausted: callers that must not refill check first.
    pub fn refill(&self, work: u64, deadline: Option<Instant>) {
        self.remaining.set(work);
        self.exhausted.set(false);
        self.deadline.set(deadline);
    }

    /// Spend one unit of work. Fails, and records exhaustion, at zero.
    pub fn consume(&self) -> Result<(), BudgetExhausted> {
        match self.remaining.get().checked_sub(1) {
            Some(left) => {
                self.remaining.set(left);
                Ok(())
            }
            None => {
                self.exhausted.set(true);
                Err(BudgetExhausted)
            }
        }
    }

    /// One interrupt checkpoint: counted for observation, then charged.
    pub fn checkpoint(&self) -> Result<(), BudgetExhausted> {
        self.checkpoints.set(self.checkpoints.get().saturating_add(1));
        self.consume()
    }
}
