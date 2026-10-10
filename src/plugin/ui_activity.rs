//! Opt-in numeric counters; capture deltas off the audio and paint paths.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Count {
    ReadbackCalls,
    ReadbackBusy,
    ReadbackCells,
    ReadbackChanges,
    PublicationCalls,
    PublicationChanges,
    FaceNew,
    FaceUpdates,
    FaceNoops,
    NativeMaterializations,
    NativeWidgets,
    TypedCalls,
    MeterCalls,
    WatchCalls,
    ReadoutPolls,
    ReadoutChanges,
    WatchWakes,
    AnimationWakes,
    PendingWakes,
    WorkerTicks,
}

pub(crate) const VERSION: u64 = 1;
pub(crate) const FIELDS: [&str; 20] = [
    "readback_calls",
    "readback_busy",
    "readback_cells",
    "readback_changes",
    "publication_calls",
    "publication_changes",
    "face_new",
    "face_updates",
    "face_noops",
    "native_materializations",
    "native_widgets",
    "typed_calls",
    "meter_calls",
    "watch_calls",
    "readout_polls",
    "readout_changes",
    "watch_wakes",
    "animation_wakes",
    "pending_wakes",
    "worker_ticks",
];

pub(crate) struct Activity {
    pub(crate) enabled: bool,
    counts: [AtomicU64; FIELDS.len()],
}

impl Activity {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            counts: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub(crate) fn add(&self, count: Count, amount: u64) {
        if self.enabled {
            self.counts[count as usize].fetch_add(amount, Ordering::Relaxed);
        }
    }

    pub(crate) fn snapshot(&self) -> Option<[u64; FIELDS.len()]> {
        self.enabled
            .then(|| std::array::from_fn(|n| self.counts[n].load(Ordering::Relaxed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_activity_is_absent_and_enabled_counts_are_cumulative() {
        let disabled = Activity::new(false);
        disabled.add(Count::WorkerTicks, 3);
        assert_eq!(disabled.snapshot(), None);
        let enabled = Activity::new(true);
        enabled.add(Count::ReadbackCalls, 2);
        enabled.add(Count::ReadbackCalls, 1);
        enabled.add(Count::ReadbackChanges, 1);
        let counts = enabled.snapshot().unwrap();
        assert_eq!(counts[Count::ReadbackCalls as usize], 3);
        assert_eq!(counts[Count::ReadbackChanges as usize], 1);
        assert_eq!(counts.iter().sum::<u64>(), 4);
        assert_eq!(Count::WorkerTicks as usize + 1, FIELDS.len());
    }

    #[test]
    fn readback_distinguishes_changed_unchanged_and_busy_without_extra_revisions() {
        use crate::support::MutexExt;
        use std::sync::Arc;

        let activity = Arc::new(Activity::new(true));
        let part = super::super::PartShared {
            ui_activity: Some(activity.clone()),
            ..Default::default()
        };
        *part.controls.lock_unpoisoned() = vec![super::super::ControlCell::new(
            sampler_ui_ir::ControlId(1),
            0.25,
        )]
        .into();
        part.refresh_controls(|_| Some(0.75));
        part.refresh_controls(|_| Some(0.75));
        let guard = part.controls.lock_unpoisoned();
        part.refresh_controls(|_| panic!("busy readback must not call the provider"));
        drop(guard);
        let counts = activity.snapshot().unwrap();
        assert_eq!(counts[Count::ReadbackCalls as usize], 3);
        assert_eq!(counts[Count::ReadbackBusy as usize], 1);
        assert_eq!(counts[Count::ReadbackCells as usize], 2);
        assert_eq!(counts[Count::ReadbackChanges as usize], 1);
        assert_eq!(part.scalar_revision.load(Ordering::Acquire), 1);
    }
}
