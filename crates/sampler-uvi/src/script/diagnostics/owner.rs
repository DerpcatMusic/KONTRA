//! Opt-in owner-thread clocks. No Lua values, paths, script text or task payloads.
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum OwnerStage {
    Loading,
    InitialSnapshot,
    Events,
    UiRequests,
    Advance,
    UiPublish,
    FindingsPublish,
    ScanSnapshot,
    CommandsCollect,
    CommandsPush,
    Backpressure,
    Park,
    AdvanceScheduled,
    AdvanceData,
    AdvanceSamples,
    DeferredCycle,
    InterfaceExport,
    ValueMirror,
    FaceLock,
    FaceCompare,
    FaceReplace,
    BackpressureUiRequests,
    AuditBarrierEvents,
    AuditBarrierAdvance,
    ExportRegistry,
    ExportTopology,
    ExportWidgets,
    ExportPage,
    ExportValidate,
    ExportWidgetSample,
    ExportValueSample,
    ExportInventorySample,
    ReadyCommandsCollect,
    ReadyCommandsPush,
}
const STAGES: [OwnerStage; 34] = [
    OwnerStage::Loading,
    OwnerStage::InitialSnapshot,
    OwnerStage::Events,
    OwnerStage::UiRequests,
    OwnerStage::Advance,
    OwnerStage::UiPublish,
    OwnerStage::FindingsPublish,
    OwnerStage::ScanSnapshot,
    OwnerStage::CommandsCollect,
    OwnerStage::CommandsPush,
    OwnerStage::Backpressure,
    OwnerStage::Park,
    OwnerStage::AdvanceScheduled,
    OwnerStage::AdvanceData,
    OwnerStage::AdvanceSamples,
    OwnerStage::DeferredCycle,
    OwnerStage::InterfaceExport,
    OwnerStage::ValueMirror,
    OwnerStage::FaceLock,
    OwnerStage::FaceCompare,
    OwnerStage::FaceReplace,
    OwnerStage::BackpressureUiRequests,
    OwnerStage::AuditBarrierEvents,
    OwnerStage::AuditBarrierAdvance,
    OwnerStage::ExportRegistry,
    OwnerStage::ExportTopology,
    OwnerStage::ExportWidgets,
    OwnerStage::ExportPage,
    OwnerStage::ExportValidate,
    OwnerStage::ExportWidgetSample,
    OwnerStage::ExportValueSample,
    OwnerStage::ExportInventorySample,
    OwnerStage::ReadyCommandsCollect,
    OwnerStage::ReadyCommandsPush,
];
impl OwnerStage {
    fn name(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::InitialSnapshot => "initial_snapshot",
            Self::Events => "events",
            Self::UiRequests => "ui_requests",
            Self::Advance => "advance",
            Self::UiPublish => "ui_publish",
            Self::FindingsPublish => "findings_publish",
            Self::ScanSnapshot => "scan_snapshot",
            Self::CommandsCollect => "commands_collect",
            Self::CommandsPush => "commands_push",
            Self::Backpressure => "command_backpressure",
            Self::Park => "park",
            Self::AdvanceScheduled => "advance_scheduled_callbacks",
            Self::AdvanceData => "advance_data_poll",
            Self::AdvanceSamples => "advance_sample_poll",
            Self::DeferredCycle => "deferred_callback_cycle",
            Self::InterfaceExport => "interface_export",
            Self::ValueMirror => "control_value_mirror",
            Self::FaceLock => "face_mutex_wait",
            Self::FaceCompare => "face_equality",
            Self::FaceReplace => "face_replace_and_retire",
            Self::BackpressureUiRequests => "backpressure_ui_requests",
            Self::AuditBarrierEvents => "audit_barrier_events",
            Self::AuditBarrierAdvance => "audit_barrier_advance",
            Self::ExportRegistry => "export_registry",
            Self::ExportTopology => "export_topology",
            Self::ExportWidgets => "export_widgets",
            Self::ExportPage => "export_page",
            Self::ExportValidate => "export_validate",
            Self::ExportWidgetSample => "export_widget_sample",
            Self::ExportValueSample => "export_value_sample",
            Self::ExportInventorySample => "export_inventory_sample",
            Self::ReadyCommandsCollect => "ready_commands_collect",
            Self::ReadyCommandsPush => "ready_commands_push",
        }
    }
    fn scope(self) -> &'static str {
        match self {
            Self::Loading | Self::InitialSnapshot => "startup; excluded from owner-turn sum",
            Self::Backpressure => "nested in commands_push; includes UI requests and retry sleep",
            Self::AdvanceScheduled | Self::AdvanceData | Self::AdvanceSamples => {
                "nested in advance"
            }
            Self::DeferredCycle => "nested in caller; includes deferred callbacks recursively",
            Self::InterfaceExport
            | Self::ValueMirror
            | Self::FaceLock
            | Self::FaceCompare
            | Self::FaceReplace => "nested in ui_publish",
            Self::UiRequests => "owner turn; includes nested audit_barrier work in seeded mode",
            Self::BackpressureUiRequests => "nested in command_backpressure",
            Self::AuditBarrierEvents | Self::AuditBarrierAdvance => {
                "nested in ui_requests only in seeded audit-barrier mode"
            }
            Self::ExportRegistry
            | Self::ExportTopology
            | Self::ExportWidgets
            | Self::ExportPage
            | Self::ExportValidate => {
                "nested in interface_export or initial_snapshot; five export substages are disjoint"
            }
            Self::ExportWidgetSample => {
                "1/128 rotating widget sample; nested in export_widgets; includes value and inventory samples"
            }
            Self::ExportValueSample | Self::ExportInventorySample => {
                "nested in export_widget_sample; sampled value and inventory intervals are disjoint"
            }
            _ => "owner turn; disjoint except explicitly nested stages",
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum OwnerCounter {
    Turns,
    Events,
    UiRequests,
    CommandsCollected,
    CommandsPushed,
    QueueFullRetries,
    SnapshotWidgets,
    SnapshotBindings,
    FaceChanges,
    ScheduledResumes,
    DeferredResumes,
    CoroutineResumes,
    BacklogHighWater,
    ExportSamplePasses,
    ExportBoundValues,
    ExportSampledBoundValues,
    ReadyCommandsCollected,
    ReadyCommandsPushed,
    ExportPlainWidgets,
    ExportMetatableWidgets,
    ExportBoundWidgets,
    ExportIndirectWidgets,
}
const COUNTERS: [OwnerCounter; 22] = [
    OwnerCounter::Turns,
    OwnerCounter::Events,
    OwnerCounter::UiRequests,
    OwnerCounter::CommandsCollected,
    OwnerCounter::CommandsPushed,
    OwnerCounter::QueueFullRetries,
    OwnerCounter::SnapshotWidgets,
    OwnerCounter::SnapshotBindings,
    OwnerCounter::FaceChanges,
    OwnerCounter::ScheduledResumes,
    OwnerCounter::DeferredResumes,
    OwnerCounter::CoroutineResumes,
    OwnerCounter::BacklogHighWater,
    OwnerCounter::ExportSamplePasses,
    OwnerCounter::ExportBoundValues,
    OwnerCounter::ExportSampledBoundValues,
    OwnerCounter::ReadyCommandsCollected,
    OwnerCounter::ReadyCommandsPushed,
    OwnerCounter::ExportPlainWidgets,
    OwnerCounter::ExportMetatableWidgets,
    OwnerCounter::ExportBoundWidgets,
    OwnerCounter::ExportIndirectWidgets,
];
impl OwnerCounter {
    fn name(self) -> &'static str {
        match self {
            Self::Turns => "owner_turns",
            Self::Events => "events_popped",
            Self::UiRequests => "ui_requests_popped",
            Self::CommandsCollected => "commands_collected",
            Self::CommandsPushed => "commands_pushed",
            Self::QueueFullRetries => "queue_full_retries",
            Self::SnapshotWidgets => "snapshot_widgets_total",
            Self::SnapshotBindings => "snapshot_bindings_total",
            Self::FaceChanges => "face_changes",
            Self::ScheduledResumes => "scheduled_resumes",
            Self::DeferredResumes => "deferred_resumes",
            Self::CoroutineResumes => "coroutine_resumes",
            Self::BacklogHighWater => "backlog_high_water",
            Self::ExportSamplePasses => "export_sample_passes",
            Self::ExportBoundValues => "export_bound_value_reads",
            Self::ExportSampledBoundValues => "export_sampled_bound_value_reads",
            Self::ReadyCommandsCollected => "ready_commands_collected",
            Self::ReadyCommandsPushed => "ready_commands_pushed",
            Self::ExportPlainWidgets => "export_plain_scalar_widgets",
            Self::ExportMetatableWidgets => "export_metatable_widgets",
            Self::ExportBoundWidgets => "export_bound_widgets",
            Self::ExportIndirectWidgets => "export_indirect_kind_widgets",
        }
    }
}

#[derive(Default)]
struct Metric {
    calls: AtomicU64,
    wall: AtomicU64,
    wall_max: AtomicU64,
    cpu: AtomicU64,
    cpu_max: AtomicU64,
    cpu_samples: AtomicU64,
    wall_histogram: [AtomicU64; 16],
}
fn bucket(ns: u64) -> usize {
    let us = ns.div_ceil(1000);
    (0..15).find(|i| us <= 1u64 << (i * 2)).unwrap_or(15)
}
fn cpu_now() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
        u64::try_from(t.tv_sec)
            .ok()?
            .checked_mul(1_000_000_000)?
            .checked_add(u64::try_from(t.tv_nsec).ok()?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
fn nanos(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

/// Fixed-size aggregate storage. Snapshots never lock or query the Lua owner.
pub struct OwnerAudit {
    started: Instant,
    active: AtomicU8,
    metrics: [Metric; STAGES.len()],
    counters: [AtomicU64; COUNTERS.len()],
}
impl Default for OwnerAudit {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            active: AtomicU8::new(u8::MAX),
            metrics: std::array::from_fn(|_| Metric::default()),
            counters: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}
impl OwnerAudit {
    /// Rotate the selected declaration-index residue between exports. The
    /// sampling stride is fixed; callbacks and getter order remain unchanged.
    pub(crate) fn export_sample_offset(&self) -> usize {
        (self.counters[OwnerCounter::ExportSamplePasses as usize].fetch_add(1, Ordering::Relaxed)
            & 127) as usize
    }
    /// Scan builds only. Ordinary plugin builds contain neither this option nor clocks.
    pub fn from_env() -> Result<Option<Arc<Self>>, String> {
        match std::env::var("KONTRA_UVI_OWNER_AUDIT").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("0") => Ok(None),
            Ok("1") => Ok(Some(Arc::new(Self::default()))),
            _ => Err("KONTRA_UVI_OWNER_AUDIT must be 0 or 1".into()),
        }
    }
    pub(crate) fn span(&self, stage: OwnerStage) -> OwnerSpan<'_> {
        let previous = self.active.swap(stage as u8, Ordering::Relaxed);
        OwnerSpan {
            audit: self,
            stage,
            previous,
            wall: Instant::now(),
            cpu: cpu_now(),
        }
    }
    pub(crate) fn add(&self, counter: OwnerCounter, n: usize) {
        self.counters[counter as usize].fetch_add(n as u64, Ordering::Relaxed);
    }
    pub(crate) fn max(&self, counter: OwnerCounter, n: usize) {
        self.counters[counter as usize].fetch_max(n as u64, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> OwnerAuditSnapshot {
        let active = self.active.load(Ordering::Relaxed);
        OwnerAuditSnapshot {
            schema: 1,
            elapsed_ns: nanos(self.started),
            active_stage: STAGES.get(usize::from(active)).map(|stage| stage.name()),
            scope: "opt-in wall/caller-thread CPU; completed intervals only; independent concurrent reads; nested stages must not be summed; scheduling/probe costs included",
            exclusive_owner_turn_stages: [
                "events",
                "ui_requests",
                "advance",
                "ui_publish",
                "findings_publish",
                "scan_snapshot",
                "commands_collect",
                "commands_push",
                "park",
                "ready_commands_collect",
                "ready_commands_push",
            ],
            stages: STAGES
                .iter()
                .enumerate()
                .map(|(i, stage)| {
                    let m = &self.metrics[i];
                    let samples = m.cpu_samples.load(Ordering::Relaxed);
                    StageSnapshot {
                        name: stage.name(),
                        scope: stage.scope(),
                        calls: m.calls.load(Ordering::Relaxed),
                        wall_ns: m.wall.load(Ordering::Relaxed),
                        wall_max_ns: m.wall_max.load(Ordering::Relaxed),
                        caller_cpu_ns: (samples > 0).then(|| m.cpu.load(Ordering::Relaxed)),
                        caller_cpu_max_ns: (samples > 0).then(|| m.cpu_max.load(Ordering::Relaxed)),
                        caller_cpu_samples: samples,
                        wall_histogram: std::array::from_fn(|j| {
                            m.wall_histogram[j].load(Ordering::Relaxed)
                        }),
                    }
                })
                .collect(),
            counters: COUNTERS
                .iter()
                .enumerate()
                .map(|(i, c)| CounterSnapshot {
                    name: c.name(),
                    value: self.counters[i].load(Ordering::Relaxed),
                    aggregation: if matches!(c, OwnerCounter::BacklogHighWater) {
                        "maximum"
                    } else {
                        "sum"
                    },
                })
                .collect(),
        }
    }
}
pub struct OwnerSpan<'a> {
    audit: &'a OwnerAudit,
    stage: OwnerStage,
    previous: u8,
    wall: Instant,
    cpu: Option<u64>,
}
impl Drop for OwnerSpan<'_> {
    fn drop(&mut self) {
        let cpu = self
            .cpu
            .zip(cpu_now())
            .map(|(start, end)| end.saturating_sub(start));
        let ns = nanos(self.wall);
        let m = &self.audit.metrics[self.stage as usize];
        m.wall.fetch_add(ns, Ordering::Relaxed);
        m.wall_max.fetch_max(ns, Ordering::Relaxed);
        m.wall_histogram[bucket(ns)].fetch_add(1, Ordering::Relaxed);
        if let Some(cpu) = cpu {
            m.cpu.fetch_add(cpu, Ordering::Relaxed);
            m.cpu_max.fetch_max(cpu, Ordering::Relaxed);
            m.cpu_samples.fetch_add(1, Ordering::Relaxed);
        }
        m.calls.fetch_add(1, Ordering::Release);
        self.audit.active.store(self.previous, Ordering::Relaxed);
    }
}
#[derive(Debug, Serialize)]
pub struct OwnerAuditSnapshot {
    pub schema: u32,
    pub elapsed_ns: u64,
    pub active_stage: Option<&'static str>,
    pub scope: &'static str,
    pub exclusive_owner_turn_stages: [&'static str; 11],
    pub stages: Vec<StageSnapshot>,
    pub counters: Vec<CounterSnapshot>,
}
#[derive(Debug, Serialize)]
pub struct StageSnapshot {
    pub name: &'static str,
    pub scope: &'static str,
    pub calls: u64,
    pub wall_ns: u64,
    pub wall_max_ns: u64,
    pub caller_cpu_ns: Option<u64>,
    pub caller_cpu_max_ns: Option<u64>,
    pub caller_cpu_samples: u64,
    /// Upper bounds in microseconds: 4^i for i0..14; i15 overflows.
    pub wall_histogram: [u64; 16],
}
#[derive(Debug, Serialize)]
pub struct CounterSnapshot {
    pub name: &'static str,
    pub value: u64,
    pub aggregation: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_audit_nested_phases_and_counters_remain_separate_and_fixed() {
        let audit = OwnerAudit::default();
        {
            let _outer = audit.span(OwnerStage::Advance);
            {
                let _inner = audit.span(OwnerStage::AdvanceSamples);
                audit.add(OwnerCounter::DeferredResumes, 3);
            }
            assert_eq!(audit.snapshot().active_stage, Some("advance"));
        }
        audit.max(OwnerCounter::BacklogHighWater, 7);
        audit.max(OwnerCounter::BacklogHighWater, 2);
        let snapshot = audit.snapshot();
        assert_eq!(snapshot.active_stage, None);
        let outer = &snapshot.stages[OwnerStage::Advance as usize];
        let inner = &snapshot.stages[OwnerStage::AdvanceSamples as usize];
        assert_eq!(outer.calls, 1);
        assert_eq!(inner.calls, 1);
        assert!(inner.wall_ns <= outer.wall_ns);
        assert_eq!(
            snapshot.counters[OwnerCounter::DeferredResumes as usize].value,
            3
        );
        assert_eq!(
            snapshot.counters[OwnerCounter::BacklogHighWater as usize].value,
            7
        );
        for stage in &snapshot.stages {
            assert_eq!(stage.calls, stage.wall_histogram.iter().sum::<u64>());
            if cfg!(target_os = "linux") {
                assert_eq!(stage.caller_cpu_samples, stage.calls);
            } else {
                assert_eq!(stage.caller_cpu_ns, None);
            }
        }
        assert_eq!(
            [
                bucket(0),
                bucket(1000),
                bucket(1001),
                bucket(4000),
                bucket(4001),
                bucket(u64::MAX)
            ],
            [0, 0, 1, 1, 2, 15]
        );
    }
}
