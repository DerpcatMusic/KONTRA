//! Opt-in numeric counters; capture deltas off the audio and paint paths.

use std::{
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// Each entry has one connected caller class; UI spans never time audio work.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Phase {
    UiBuildFrame,
    UiSnapshotReadback,
    UiControlReadback,
    UiControlCapture,
    UiProjectionSync,
    UiAuthoredLayoutSubmission,
    UiNativeScript,
    UiNativeCanvasSubmission,
    WorkerScriptEffects,
}

// Independent from the v1 counter ABI. Triples: count, total_ns, max_ns.
pub(crate) const PHASE_VERSION: u64 = 1;
pub(crate) const PHASES: [(&str, &str); 9] = [
    ("ui_build_frame", "editor_ui"),
    ("ui_snapshot_readback", "editor_ui"),
    ("ui_control_readback", "editor_ui"),
    ("ui_control_capture", "editor_ui"),
    ("ui_projection_sync", "editor_ui"),
    ("ui_authored_layout_submission", "editor_ui"),
    ("ui_native_script", "editor_ui"),
    ("ui_native_canvas_submission", "editor_ui_deferred_canvas"),
    ("worker_script_effects", "serialized_load_worker"),
];
pub(crate) const PHASE_FIELDS: usize = PHASES.len() * 3;

/// A stack-only elapsed-time span, deliberately !Send/!Sync. No clock when disabled.
/// Instant measures elapsed wall time around CPU work, not on-core CPU time.
pub(crate) struct Span<'a> {
    timing: Option<(&'a Activity, Phase, Instant)>,
    _same_thread: PhantomData<Rc<()>>,
}
impl Drop for Span<'_> {
    fn drop(&mut self) {
        if let Some((activity, phase, start)) = self.timing {
            activity.record_phase(phase, start.elapsed());
        }
    }
}

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
    phases: [AtomicU64; PHASE_FIELDS],
}

impl Activity {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            counts: std::array::from_fn(|_| AtomicU64::new(0)),
            phases: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub(crate) fn add(&self, count: Count, amount: u64) {
        if self.enabled {
            self.counts[count as usize].fetch_add(amount, Ordering::Relaxed);
        }
    }

    /// Call only at the named editor/worker seams, never in process/readback providers.
    pub(crate) fn span(&self, phase: Phase) -> Span<'_> {
        Span {
            timing: self.enabled.then(|| (self, phase, Instant::now())),
            _same_thread: PhantomData,
        }
    }

    fn record_phase(&self, phase: Phase, elapsed: Duration) {
        if !self.enabled {
            return;
        }
        let at = phase as usize * 3;
        let ns = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        for (field, amount) in [(at, 1), (at + 1, ns)] {
            let _ = self.phases[field].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_add(amount))
            });
        }
        self.phases[at + 2].fetch_max(ns, Ordering::Relaxed);
    }

    /// Independent cumulative loads, not a transactional snapshot. Read at rest for deltas.
    pub(crate) fn phase_snapshot(&self) -> Option<[u64; PHASE_FIELDS]> {
        self.enabled
            .then(|| std::array::from_fn(|n| self.phases[n].load(Ordering::Relaxed)))
    }

    /// Export worker only: allocation/serialization is not part of instrumented UI work.
    pub(crate) fn phase_report(&self) -> serde_json::Value {
        let Some(values) = self.phase_snapshot() else {
            return serde_json::Value::Null;
        };
        let phases: Vec<_> = PHASES.iter().enumerate().map(|(n, (name, thread))| {
            serde_json::json!({
                "name": name, "thread_role": thread,
                "count": values[n * 3], "total_ns": values[n * 3 + 1],
                "max_ns": values[n * 3 + 2],
            })
        }).collect();
        serde_json::json!({
            "version": PHASE_VERSION, "clock": "elapsed_monotonic_wall_ns",
            "scope": "inclusive_cpu_work_spans_not_gpu_present_or_full_renderer_frame",
            "phases": phases,
        })
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
    fn phase_spans_are_opt_in_bounded_and_aggregate_count_total_max() {
        let disabled = Activity::new(false);
        let span = disabled.span(Phase::UiBuildFrame);
        assert!(span.timing.is_none(), "disabled profiler must not read the clock");
        drop(span);
        assert_eq!(disabled.phase_snapshot(), None);
        assert!(disabled.phase_report().is_null());

        let activity = Activity::new(true);
        // Synthetic exact durations exercise the same sink used by Span::drop.
        activity.record_phase(Phase::UiControlReadback, Duration::from_nanos(7));
        activity.record_phase(Phase::UiControlReadback, Duration::from_nanos(11));
        activity.record_phase(Phase::WorkerScriptEffects, Duration::from_nanos(5));
        let values = activity.phase_snapshot().unwrap();
        let at = Phase::UiControlReadback as usize * 3;
        assert_eq!(&values[at..at + 3], &[2, 18, 11]);
        let worker = Phase::WorkerScriptEffects as usize * 3;
        assert_eq!(&values[worker..worker + 3], &[1, 5, 5]);
        assert_eq!(values.iter().sum::<u64>(), 42);
        // Real guards close on early return and unwind without timing assumptions.
        let early = || { let _span = activity.span(Phase::UiBuildFrame); return; };
        early();
        let _ = std::panic::catch_unwind(|| {
            let _span = activity.span(Phase::UiBuildFrame);
            panic!("synthetic UI failure");
        });
        assert_eq!(activity.phase_snapshot().unwrap()[0], 2);
        assert_eq!(activity.snapshot().unwrap(), [0; FIELDS.len()], "v1 is unchanged");
        assert_eq!(activity.phase_report()["phases"][worker / 3]["thread_role"],
            "serialized_load_worker");
        assert_eq!(Phase::WorkerScriptEffects as usize + 1, PHASES.len());
    }

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
