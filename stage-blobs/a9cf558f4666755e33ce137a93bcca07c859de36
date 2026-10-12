//! Numeric-only observations; reading a busy owner never locks or waits on Lua.
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum OwnerPhase {
    Loading,
    Events,
    UiRequests,
    Advance,
    Publish,
    Parked,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct OwnerSnapshot {
    pub phase: OwnerPhase,
    pub requested_barriers: u64,
    pub completed_barriers: u64,
    pub requested_clock_ms: f64,
    pub completed_clock_ms: f64,
    pub clock_ms: f64,
    pub work_remaining: u64,
    pub vm_checkpoints: u64,
    pub coroutine_resumes: u64,
    pub work_exhausted: bool,
}

/// Counters are independent observations during work; completion is published
/// only after the owner's UI/fault/scan snapshots. Available for seeded audits.
pub struct ScanProgress {
    phase: AtomicU8,
    requested: AtomicU64,
    completed: AtomicU64,
    requested_clock: AtomicU64,
    completed_clock: AtomicU64,
    clock: AtomicU64,
    work_remaining: AtomicU64,
    vm_checkpoints: AtomicU64,
    resumes: AtomicU64,
    exhausted: AtomicBool,
}

impl ScanProgress {
    pub(crate) fn new(work: u64) -> Self {
        Self {
            phase: AtomicU8::new(OwnerPhase::Loading as u8),
            requested: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            requested_clock: AtomicU64::new(0f64.to_bits()),
            completed_clock: AtomicU64::new(0f64.to_bits()),
            clock: AtomicU64::new(0f64.to_bits()),
            work_remaining: AtomicU64::new(work),
            vm_checkpoints: AtomicU64::new(0),
            resumes: AtomicU64::new(0),
            exhausted: AtomicBool::new(false),
        }
    }
    pub(crate) fn phase(&self, phase: OwnerPhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }
    pub(crate) fn request(&self, ms: f64) {
        self.requested_clock.store(ms.to_bits(), Ordering::Relaxed);
        self.requested.fetch_add(1, Ordering::Release);
    }
    pub(crate) fn complete(&self, ms: f64) {
        self.completed_clock.store(ms.to_bits(), Ordering::Relaxed);
        self.completed
            .store(self.requested.load(Ordering::Acquire), Ordering::Release);
    }
    pub(crate) fn work(&self, left: u64, checkpoints: u64, exhausted: bool) {
        self.work_remaining.store(left, Ordering::Relaxed);
        self.vm_checkpoints.store(checkpoints, Ordering::Relaxed);
        self.exhausted.store(exhausted, Ordering::Relaxed);
    }
    pub(crate) fn resume(&self, ms: f64) {
        self.clock(ms);
        self.resumes.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn clock(&self, ms: f64) {
        self.clock.store(ms.to_bits(), Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> OwnerSnapshot {
        let completed = self.completed.load(Ordering::Acquire);
        OwnerSnapshot {
            phase: match self.phase.load(Ordering::Acquire) {
                0 => OwnerPhase::Loading,
                1 => OwnerPhase::Events,
                2 => OwnerPhase::UiRequests,
                3 => OwnerPhase::Advance,
                4 => OwnerPhase::Publish,
                _ => OwnerPhase::Parked,
            },
            requested_barriers: self.requested.load(Ordering::Acquire),
            completed_barriers: completed,
            requested_clock_ms: f64::from_bits(self.requested_clock.load(Ordering::Relaxed)),
            completed_clock_ms: f64::from_bits(self.completed_clock.load(Ordering::Relaxed)),
            clock_ms: f64::from_bits(self.clock.load(Ordering::Relaxed)),
            work_remaining: self.work_remaining.load(Ordering::Relaxed),
            vm_checkpoints: self.vm_checkpoints.load(Ordering::Relaxed),
            coroutine_resumes: self.resumes.load(Ordering::Relaxed),
            work_exhausted: self.exhausted.load(Ordering::Relaxed),
        }
    }
}
