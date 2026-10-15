//! Functional test barrier only; the entire module is absent from production.
use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Default)]
pub(super) struct Snapshot {
    pub(super) runs: u64,
    pub(super) completed_lane: Option<usize>,
    pub(super) seen_lanes: usize,
    pub(super) timed_out: bool,
}

#[derive(Default)]
pub(super) struct Witness {
    state: Mutex<Snapshot>,
    completed: Condvar,
}

impl Witness {
    pub(super) fn reset(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.runs += 1;
        state.completed_lane = None;
        // A timeout remains visible even when render() contains several pool runs.
    }

    pub(super) fn before(&self, lane: usize) {
        if lane != 0 {
            return;
        }
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (mut state, _) = self
            .completed
            .wait_timeout_while(state, Duration::from_secs(5), |state| {
                state.completed_lane.is_none() && !state.timed_out
            })
            .unwrap_or_else(|e| e.into_inner());
        if state.completed_lane.is_none() {
            state.timed_out = true;
            self.completed.notify_all();
        }
    }

    pub(super) fn after(&self, lane: usize) {
        if lane == 0 {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.completed_lane = Some(lane);
        state.seen_lanes |= 1 << lane;
        self.completed.notify_all();
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}
