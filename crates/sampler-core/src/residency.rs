//! Group residency, separate from gain and transient streaming misses.
//!
//! KSP Reference: Group Commands (purge_group/get_purge_state), Callbacks
//! (on async_complete). Kontakt8 SHA 0fe6356e…dae8: BSamplePoolItem vtable
//! 0x144e97bd8 -> 0x14081e0c0; named AttachSample(...SyncedThreadingJobManager).
//! These establish sample-pool/job identities, not a recovered native queue law.
//! Our bounded publication and cancellation policy is an implementation choice.
use crate::{Error, Pcm, Prepared, Runtime, ScriptInstanceId};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering::*},
};

const JOBS: usize = 1024;
pub(super) mod startup;
pub use startup::InitialGroupResidency;

/// First request-admission refusal retained by one residency owner. Invalid or
/// foreign plan handles cannot be attributed to an owner and do not alter it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ResidencyRejectionReason {
    QueueCapacity = 1,
    GroupIndex = 2,
    AsyncIdCapacity = 3,
    CallbackCounterCapacity = 4,
    RevisionCapacity = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResidencyRejection {
    /// Dense runtime group, not a physical source-group index.
    pub group: u32,
    pub reload: bool,
    pub reason: ResidencyRejectionReason,
}

/// Explicit control-side observation of the residency owner retained by a
/// Prepared object. Lifetime totals start at that owner's construction, not at
/// UI publication or a query. Worker phase counts are weakly consistent; this
/// is not an atomic snapshot of the worker thread. No query resets counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResidencyProgress {
    pub managed: bool,
    pub groups_total: u32,
    /// Audio-committed eligibility, not a promise of full sample/page residency.
    pub groups_loaded: u32,
    pub groups_unloaded: u32,
    pub groups_with_pending: u32,
    pub job_capacity: usize,
    pub queued: usize,
    pub in_flight: usize,
    pub worker_ready: usize,
    /// All accepted slots not yet consumed by the audio owner.
    pub pending: usize,
    /// Generation-owned startup completions awaiting callback admission.
    /// Physical work was completed off audio before this generation published.
    pub startup_completions_pending: usize,
    pub accepted: u64,
    pub consumed: u64,
    /// Current-revision successful completions admitted on audio, purge included.
    pub committed: u64,
    /// Current-revision failed completions admitted on audio.
    pub failed: u64,
    /// Completions superseded by a newer group revision, consumed without commit.
    pub stale: u64,
    /// There is no separately observable live cancellation operation today.
    pub canceled: Option<u64>,
    pub rejected: u64,
    pub rejected_queue: u64,
    pub rejected_group: u64,
    pub rejected_async_id: u64,
    pub rejected_callback_counter: u64,
    pub rejected_revision: u64,
    /// The completion stays ready when its async callback cannot be admitted.
    pub completion_admission_failures: u64,
    pub first_rejection: Option<ResidencyRejection>,
    pub counters_saturated: bool,
}

/// Exact dense/physical identity and current eligibility of one valid group.
/// A pending reload is not loaded until the audio completion is admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupResidencyProgress {
    pub group: u32,
    pub source_group: u32,
    pub managed: bool,
    pub loaded: bool,
    /// Members captured when this residency owner was prepared. This does not
    /// enumerate later dynamic sample revisions or promise their physical state.
    pub asset_count: Option<usize>,
    pub pending: usize,
    pub logical_revision: Option<u64>,
    pub latest_requested_reload: Option<bool>,
    /// Ownership mode after a worker rollback may differ from the last request.
    pub retention_owned: Option<bool>,
}

// Updated only by the single audio/request owner, never by the worker. Atomics
// permit control-side observations without introducing a diagnostic lock.
#[derive(Default)]
struct Counters {
    accepted: AtomicU64,
    consumed: AtomicU64,
    committed: AtomicU64,
    failed: AtomicU64,
    stale: AtomicU64,
    rejected: AtomicU64,
    rejected_queue: AtomicU64,
    rejected_group: AtomicU64,
    rejected_async_id: AtomicU64,
    rejected_callback_counter: AtomicU64,
    rejected_revision: AtomicU64,
    completion_admission_failures: AtomicU64,
    first_reason: AtomicUsize,
    first_group: AtomicUsize,
    first_reload: AtomicBool,
    saturated: AtomicBool,
}
impl Counters {
    fn bump(&self, counter: &AtomicU64) {
        let value = counter.load(Relaxed);
        if value == u64::MAX {
            self.saturated.store(true, Relaxed);
        } else {
            counter.store(value + 1, Relaxed);
        }
    }
    fn reject(&self, group: u32, reload: bool, reason: ResidencyRejectionReason) {
        self.bump(&self.rejected);
        self.bump(match reason {
            ResidencyRejectionReason::QueueCapacity => &self.rejected_queue,
            ResidencyRejectionReason::GroupIndex => &self.rejected_group,
            ResidencyRejectionReason::AsyncIdCapacity => &self.rejected_async_id,
            ResidencyRejectionReason::CallbackCounterCapacity => &self.rejected_callback_counter,
            ResidencyRejectionReason::RevisionCapacity => &self.rejected_revision,
        });
        if self.first_reason.load(Relaxed) == 0 {
            self.first_group.store(group as usize, Relaxed);
            self.first_reload.store(reload, Relaxed);
            self.first_reason.store(reason as usize, Release);
        }
    }
    fn first_rejection(&self) -> Option<ResidencyRejection> {
        let reason = match self.first_reason.load(Acquire) {
            1 => ResidencyRejectionReason::QueueCapacity,
            2 => ResidencyRejectionReason::GroupIndex,
            3 => ResidencyRejectionReason::AsyncIdCapacity,
            4 => ResidencyRejectionReason::CallbackCounterCapacity,
            5 => ResidencyRejectionReason::RevisionCapacity,
            _ => return None,
        };
        Some(ResidencyRejection {
            group: self.first_group.load(Relaxed) as u32,
            reload: self.first_reload.load(Relaxed),
            reason,
        })
    }
}

struct Group {
    assets: Box<[Pcm]>,
    families: Box<[crate::SourceStorageFamily]>,
    loaded: AtomicBool,
    revision: AtomicU64,
    pending: AtomicUsize,
    last_requested_reload: AtomicBool,
}
struct Slot {
    state: AtomicUsize, // 0 free, 1 queued, 2 complete, 3 executing
    group: AtomicUsize,
    id: AtomicI32,
    instance: AtomicUsize,
    revision: AtomicU64,
    reload: AtomicBool,
    success: AtomicBool,
}
struct Shared {
    groups: Box<[Group]>,
    pinned: Box<[Pcm]>,
    families: Box<[crate::SourceStorageFamily]>,
    slots: Box<[Slot]>,
    stop: AtomicBool,
    counters: Counters,
}
pub(super) struct Residency {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
    submitted: AtomicUsize,
    completed: AtomicUsize,
}
#[derive(Clone, Copy)]
struct Completion {
    group: usize,
    id: i32,
    instance: ScriptInstanceId,
    revision: u64,
    reload: bool,
    success: bool,
}
impl Residency {
    pub fn new(plan: &Prepared) -> Result<Self, Error> {
        let mut groups: Vec<Vec<Pcm>> = vec![vec![]; plan.group_count as usize];
        let mut pinned = Vec::new();
        let mut families: Vec<crate::SourceStorageFamily> = Vec::new();
        let mut group_families: Vec<Vec<crate::SourceStorageFamily>> = vec![vec![]; plan.group_count as usize];
        let mut referenced = std::collections::HashSet::new();
        for region in 0..plan.region_count() {
            let Some(source) = plan.region_source_storage(region) else {
                continue;
            };
            if let crate::SourceStorageRef::Channels(family) = source {
                referenced.insert(family.id());
                if !families.iter().any(|old| old.id() == family.id()) { families.push(family.clone()); }
                if let Some(group) = plan.region_groups[region] {
                    let owners = &mut group_families[group as usize];
                    if !owners.iter().any(|old| old.id() == family.id()) { owners.push(family.clone()); }
                }
            }
            let members = match plan.region_groups[region] {
                Some(g) => &mut groups[g as usize],
                None => &mut pinned,
            };
            // Logical sources may retain multiple physical assets. Every actual
            // dependency participates in the existing group/pinned owner count;
            // an immutable family pin alone must not strand an aliased purge.
            source.physical_dependencies(|pcm| {
                if !members.iter().any(|p| p.asset_id() == pcm.asset_id()) {
                    members.push(pcm.clone());
                }
            });
        }
        // Unreferenced roots have no group that can manage their cohort. Keep
        // those actual dependencies pinned until the storage root retires.
        // Referenced families belong to their actual group/ungrouped owners.
        for sample in 0..plan.sample_count() {
            if let Some(crate::SourceStorageRef::Channels(source)) = plan.source_storage(sample) {
                if referenced.contains(&source.id()) { continue; }
                if !families.iter().any(|old| old.id() == source.id()) { families.push(source.clone()); }
                source.as_source().physical_dependencies(|pcm| {
                    if !pinned.iter().any(|p| p.asset_id() == pcm.asset_id()) {
                        pinned.push(pcm.clone());
                    }
                });
            }
        }
        for pcm in groups.iter().flatten().chain(&pinned) {
            pcm.residency_acquire();
        }
        let shared = Arc::new(Shared {
            groups: groups
                .into_iter()
                .zip(group_families)
                .map(|(assets, families)| Group {
                    assets: assets.into(),
                    families: families.into(),
                    loaded: AtomicBool::new(true),
                    revision: AtomicU64::new(1),
                    pending: AtomicUsize::new(0),
                    last_requested_reload: AtomicBool::new(true),
                })
                .collect(),
            pinned: pinned.into(),
            families: families.into(),
            slots: (0..JOBS)
                .map(|_| Slot {
                    state: AtomicUsize::new(0),
                    group: AtomicUsize::new(0),
                    id: AtomicI32::new(0),
                    instance: AtomicUsize::new(0),
                    revision: AtomicU64::new(1),
                    reload: AtomicBool::new(false),
                    success: AtomicBool::new(false),
                })
                .collect(),
            stop: AtomicBool::new(false),
            counters: Counters::default(),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("sampler-residency".into())
            .spawn(move || {
                let mut next = 0usize;
                while !worker.stop.load(Acquire) {
                    let slot = &worker.slots[next % JOBS];
                    if slot.state.load(Acquire) != 1 {
                        std::thread::park();
                        continue;
                    }
                    slot.state.store(3, Release);
                    let group = &worker.groups[slot.group.load(Relaxed)];
                    let revision = slot.revision.load(Relaxed);
                    let reload = slot.reload.load(Relaxed);
                    let mut success = group.revision.load(Acquire) == revision;
                    for pcm in &group.assets {
                        if reload && success {
                            success &= pcm.residency_reload().is_ok();
                        }
                    }
                    // Only publish a complete original-channel cohort after
                    // all physical reloads. This is off audio on the existing
                    // worker; a failed middle pack cannot become a partial view.
                    for family in &group.families {
                        if family.refresh_residency().is_err() && reload { success = false; }
                    }
                    // A failed multi-asset reload rolls back its ownership before
                    // completion. Mode and revision share one CAS so a newer
                    // request cannot lose its reference to this older failure.
                    if reload
                        && !success
                        && group
                            .revision
                            .compare_exchange(revision, revision & !1, AcqRel, Acquire)
                            .is_ok()
                    {
                        for pcm in &group.assets {
                            pcm.residency_release();
                        }
                        slot.revision.store(revision & !1, Relaxed);
                    }
                    for family in &group.families {
                        // Failure rollback or a racing purge can revoke the
                        // whole cohort after staging. Release its pins first.
                        let _ = family.refresh_residency();
                    }
                    for pcm in &group.assets {
                        // Check after I/O as well: a newer purge owns eligibility and
                        // must not leave a stale head resident when no other owner needs it.
                        while !pcm.residency_collect() && !worker.stop.load(Acquire) {
                            for family in &group.families { family.collect_residency(); }
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                    }
                    success &= group.revision.load(Acquire) == revision;
                    slot.success.store(success, Relaxed);
                    slot.state.store(2, Release);
                    next = next.wrapping_add(1);
                }
            })
            .map_err(|_| Error::Capacity)?;
        Ok(Self {
            shared,
            thread: Some(thread),
            submitted: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
        })
    }
    pub fn allowed(&self, group: u32) -> bool {
        self.shared
            .groups
            .get(group as usize)
            .is_some_and(|g| g.loaded.load(Acquire))
    }
    fn room(&self) -> bool {
        self.shared.slots[self.submitted.load(Relaxed) % JOBS]
            .state
            .load(Acquire)
            == 0
    }
    fn request(
        &self,
        group: usize,
        reload: bool,
        id: i32,
        instance: ScriptInstanceId,
    ) -> Result<(), Error> {
        let group_state = self.shared.groups.get(group).ok_or(Error::InvalidInput)?;
        if !self.room() {
            return Err(Error::Capacity);
        }
        #[allow(deprecated, reason = "fetch_update supports Rust 1.92")]
        let previous = group_state
            .revision
            .fetch_update(AcqRel, Acquire, |state| {
                (state >> 1)
                    .checked_add(1)
                    .and_then(|revision| revision.checked_mul(2))
                    .map(|revision| revision | u64::from(reload))
            })
            .map_err(|_| Error::Capacity)?;
        group_state.loaded.store(false, Release);
        group_state.last_requested_reload.store(reload, Relaxed);
        group_state.pending.fetch_add(1, Relaxed);
        let revision = ((previous >> 1) + 1) * 2 | u64::from(reload);
        if (previous & 1 != 0) != reload {
            for pcm in &group_state.assets {
                if reload {
                    pcm.residency_acquire();
                } else {
                    pcm.residency_release();
                }
            }
        }
        let next = self.submitted.load(Relaxed);
        let slot = &self.shared.slots[next % JOBS];
        slot.group.store(group, Relaxed);
        slot.id.store(id, Relaxed);
        slot.instance.store(instance.0 as usize, Relaxed);
        slot.revision.store(revision, Relaxed);
        slot.reload.store(reload, Relaxed);
        self.shared.counters.bump(&self.shared.counters.accepted);
        slot.state.store(1, Release);
        self.submitted.store(next.wrapping_add(1), Relaxed);
        self.thread.as_ref().unwrap().thread().unpark();
        Ok(())
    }
    fn peek(&self) -> Option<Completion> {
        let slot = &self.shared.slots[self.completed.load(Relaxed) % JOBS];
        (slot.state.load(Acquire) == 2).then(|| Completion {
            group: slot.group.load(Relaxed),
            id: slot.id.load(Relaxed),
            instance: ScriptInstanceId(slot.instance.load(Relaxed) as u16),
            revision: slot.revision.load(Relaxed),
            reload: slot.reload.load(Relaxed),
            success: slot.success.load(Relaxed),
        })
    }
    fn consume(&self, c: Completion) {
        let g = &self.shared.groups[c.group];
        if g.revision.load(Acquire) == c.revision {
            g.loaded.store(c.reload && c.success, Release);
            self.shared.counters.bump(if c.success {
                &self.shared.counters.committed
            } else {
                &self.shared.counters.failed
            });
        } else {
            self.shared.counters.bump(&self.shared.counters.stale);
        }
        g.pending.fetch_sub(1, Relaxed);
        self.shared.counters.bump(&self.shared.counters.consumed);
        let next = self.completed.load(Relaxed);
        self.shared.slots[next % JOBS].state.store(0, Release);
        self.completed.store(next.wrapping_add(1), Relaxed);
        self.thread.as_ref().unwrap().thread().unpark();
    }

    fn progress(&self) -> ResidencyProgress {
        let c = &self.shared.counters;
        let mut out = ResidencyProgress {
            managed: true,
            groups_total: self.shared.groups.len() as u32,
            job_capacity: JOBS,
            accepted: c.accepted.load(Relaxed),
            consumed: c.consumed.load(Relaxed),
            committed: c.committed.load(Relaxed),
            failed: c.failed.load(Relaxed),
            stale: c.stale.load(Relaxed),
            rejected: c.rejected.load(Relaxed),
            rejected_queue: c.rejected_queue.load(Relaxed),
            rejected_group: c.rejected_group.load(Relaxed),
            rejected_async_id: c.rejected_async_id.load(Relaxed),
            rejected_callback_counter: c.rejected_callback_counter.load(Relaxed),
            rejected_revision: c.rejected_revision.load(Relaxed),
            completion_admission_failures: c.completion_admission_failures.load(Relaxed),
            first_rejection: c.first_rejection(),
            counters_saturated: c.saturated.load(Relaxed),
            ..Default::default()
        };
        for g in &self.shared.groups {
            if g.loaded.load(Acquire) {
                out.groups_loaded += 1;
            } else {
                out.groups_unloaded += 1;
            }
            out.groups_with_pending += u32::from(g.pending.load(Relaxed) != 0);
        }
        for slot in &self.shared.slots {
            match slot.state.load(Acquire) {
                1 => out.queued += 1,
                2 => out.worker_ready += 1,
                3 => out.in_flight += 1,
                _ => {}
            }
        }
        out.pending = out.queued + out.in_flight + out.worker_ready;
        out
    }
}
impl Drop for Shared {
    fn drop(&mut self) {
        for g in &self.groups {
            if g.revision.load(Acquire) & 1 != 0 {
                for p in &g.assets {
                    p.residency_release();
                }
            }
        }
        for p in &self.pinned {
            p.residency_release();
        }
        for family in &self.families { let _ = family.refresh_residency(); }
        for p in self
            .groups
            .iter()
            .flat_map(|g| g.assets.iter())
            .chain(&self.pinned)
        {
            p.residency_collect();
        }
    }
}
impl Drop for Residency {
    fn drop(&mut self) {
        self.shared.stop.store(true, Release);
        self.thread.as_ref().unwrap().thread().unpark();
        // Join belongs to Prepared/control-side destruction, never rendering.
        let _ = self.thread.take().unwrap().join();
    }
}
impl Runtime {
    /// Request at the current audio timestamp; no allocation, I/O or sample destruction.
    /// Reload remains ineligible until completion is admitted on the audio side.
    pub fn request_group_residency(
        &mut self,
        plan: crate::PlanId,
        group: u32,
        reload: bool,
        instance: ScriptInstanceId,
    ) -> Result<i32, Error> {
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let residency = generation
            .prepared
            .residency
            .as_ref()
            .ok_or(Error::InvalidInput)?;
        if group >= generation.prepared.group_count {
            residency
                .shared
                .counters
                .reject(group, reload, ResidencyRejectionReason::GroupIndex);
            return Err(Error::InvalidInput);
        }
        if !residency.room() {
            residency.shared.counters.reject(
                group,
                reload,
                ResidencyRejectionReason::QueueCapacity,
            );
            return Err(Error::Capacity);
        }
        let callbacks = generation.callbacks.checked_add(1).ok_or_else(|| {
            residency.shared.counters.reject(
                group,
                reload,
                ResidencyRejectionReason::CallbackCounterCapacity,
            );
            Error::Capacity
        })?;
        let id = generation
            .midi_object
            .allocate_async_id()
            .inspect_err(|_| {
                residency.shared.counters.reject(
                    group,
                    reload,
                    ResidencyRejectionReason::AsyncIdCapacity,
                );
            })?;
        residency
            .request(group as usize, reload, id, instance)
            .inspect_err(|_| {
                // The single producer checked queue room above; only revision
                // exhaustion can reject this second, mutation-free preflight.
                residency.shared.counters.reject(
                    group,
                    reload,
                    ResidencyRejectionReason::RevisionCapacity,
                );
            })?;
        generation.callbacks = callbacks;
        if !reload {
            for index in 0..self.voices.slots.len() {
                let Some(v) = &self.voices.slots[index].value else {
                    continue;
                };
                let note = self.families.get(v.family.0).unwrap().note;
                if v.group == Some(group) && self.notes.get(note.0).unwrap().plan == plan {
                    self.end_voice(crate::VoiceId(self.voices.id(index)));
                }
            }
        }
        if !reload {
            let residency = self
                .plans
                .get(plan.0)
                .unwrap()
                .prepared
                .residency
                .as_ref()
                .unwrap();
            if let Some(cache) = &mut self.stream_cache {
                for pcm in &residency.shared.groups[group as usize].assets {
                    if pcm.residency_owners() == 0 {
                        cache.invalidate_asset(pcm.asset_id());
                    }
                }
            }
        }
        Ok(id)
    }
    pub fn group_resident(&self, plan: crate::PlanId, group: u32) -> Result<bool, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if group >= generation.prepared.group_count {
            return Err(Error::InvalidInput);
        }
        Ok(generation
            .prepared
            .residency
            .as_ref()
            .is_none_or(|r| r.allowed(group)))
    }
    /// Explicit off-audio diagnostic query: scans at most the owner's group
    /// inventory and its fixed 1024 job slots, allocates nothing and does not
    /// advance work. Worker phase counts may change while they are observed.
    pub fn residency_progress(&self, plan: crate::PlanId) -> Result<ResidencyProgress, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let mut progress = generation.prepared.residency.as_ref().map_or_else(
            || ResidencyProgress {
                groups_total: generation.prepared.group_count,
                groups_loaded: generation.prepared.group_count,
                ..Default::default()
            },
            Residency::progress,
        );
        progress.startup_completions_pending = generation.initial_residency.pending();
        Ok(progress)
    }
    /// Exact O(1), allocation-free group observation. Dense runtime group and
    /// retained physical source identity are reported separately; no fallback
    /// from an invalid index or foreign plan is performed.
    pub fn group_residency_progress(
        &self,
        plan: crate::PlanId,
        group: u32,
    ) -> Result<GroupResidencyProgress, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if group >= generation.prepared.group_count {
            return Err(Error::InvalidInput);
        }
        let source_group = generation
            .prepared
            .group_source_ids
            .get(group as usize)
            .copied()
            .unwrap_or(group);
        let Some(owner) = generation.prepared.residency.as_ref() else {
            return Ok(GroupResidencyProgress {
                group,
                source_group,
                managed: false,
                loaded: true,
                asset_count: None,
                pending: 0,
                logical_revision: None,
                latest_requested_reload: None,
                retention_owned: None,
            });
        };
        let g = &owner.shared.groups[group as usize];
        let revision = g.revision.load(Acquire);
        Ok(GroupResidencyProgress {
            group,
            source_group,
            managed: true,
            loaded: g.loaded.load(Acquire),
            asset_count: Some(g.assets.len()),
            pending: g.pending.load(Relaxed),
            logical_revision: Some(revision >> 1),
            latest_requested_reload: (revision >> 1 != 0)
                .then(|| g.last_requested_reload.load(Relaxed)),
            retention_owned: Some(revision & 1 != 0),
        })
    }
    /// Bounded completion drain at a render boundary, also for zero-frame renders.
    pub fn service_residency(&mut self) -> Result<(), Error> {
        for index in 0..self.plans.slots.len() {
            let Some(_) = self.plans.slots[index].value else {
                continue;
            };
            let plan = crate::PlanId(self.plans.id(index));
            self.service_initial_residency(plan)?;
            for _ in 0..JOBS {
                let next = self
                    .plans
                    .get(plan.0)
                    .unwrap()
                    .prepared
                    .residency
                    .as_ref()
                    .and_then(Residency::peek);
                let Some(c) = next else {
                    break;
                };
                let callback = match self
                    .preflight_async_callback(plan, c.instance)
                    .inspect_err(|_| {
                        let residency = self
                            .plans
                            .get(plan.0)
                            .unwrap()
                            .prepared
                            .residency
                            .as_ref()
                            .unwrap();
                        residency
                            .shared
                            .counters
                            .bump(&residency.shared.counters.completion_admission_failures);
                    }) {
                    Ok(callback) => callback,
                    // Keep the ready completion owned by its worker slot. The
                    // callbacks occupying admission room must resume below in
                    // render_inner before this job can be delivered next block.
                    Err(Error::Capacity) => break,
                    Err(error) => return Err(error),
                };
                let generation = self.plans.get_mut(plan.0).unwrap();
                generation.prepared.residency.as_ref().unwrap().consume(c);
                generation.callbacks -= 1;
                self.finish_async_callback(plan, c.id, c.success, callback)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod logical_source_residency_tests {
    use super::*;
    use crate::{ChannelCount, Envelope, Playback, Region, SourceStorageFamily};
    fn region(sample: usize) -> Region {
        Region { sample, key_low: 60, key_high: 60, root_key: None,
            velocity_low: 0., velocity_high: 1., gain: 1.,
            envelope: Envelope::default(), playback: Playback::default() }
    }
    #[test]
    fn pinned_family_dependencies_keep_an_aliased_stereo_group_purge_collectable() {
        for referenced in [true, false] { check_aliased_family_purge(referenced); }
    }
    fn check_aliased_family_purge(referenced: bool) {
        let packs: Vec<_> = (1..=3).map(|value| Pcm::new(48000,
            vec![[value as f32, -(value as f32)]; 128].into_boxed_slice()).unwrap()).collect();
        let alias = packs[1].clone();
        let family = SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), packs.clone()).unwrap();
        let mut plan = Prepared::new_sources(48000, vec![family.clone().into(), alias.clone().into()],
            vec![region(0), region(0), region(1)], 3).unwrap();
        // The family remains ungrouped; only the independently authored stereo
        // alias is a mutable group. This does not enable wide group mutation.
        plan.group_count = 1;
        plan.region_groups = vec![None, None, Some(0)].into_boxed_slice();
        if !referenced {
            // Unreferenced roots still retain pins until this table retires.
            plan = Prepared::new_sources(48000, vec![family.clone().into(), alias.clone().into()],
                vec![region(1)], 1).unwrap();
            plan.group_count = 1;
            plan.region_groups = vec![Some(0)].into_boxed_slice();
        }
        let owner = Residency::new(&plan).unwrap();
        assert_eq!(owner.shared.pinned.len(), 3);
        assert_eq!(owner.shared.groups[0].assets.len(), 1);
        assert_eq!(packs.iter().map(Pcm::residency_owners).collect::<Vec<_>>(), vec![1, 2, 1]);
        owner.request(0, false, 1, ScriptInstanceId(0)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let completion = loop {
            if let Some(completion) = owner.peek() { break completion; }
            assert!(std::time::Instant::now() < deadline, "aliased purge retained by an unregistered family pin");
            std::thread::yield_now();
        };
        assert!(completion.success);
        owner.consume(completion);
        assert_eq!(alias.residency_owners(), 1);
        assert!(alias.pin_resident().is_some());
        assert_eq!(family.resident_view().sample(2, 127), Some(2.));
        let retained = family.resident_view();
        drop(owner);
        assert!(packs.iter().all(|pcm| pcm.residency_owners() == 0));
        // A reader keeps its exact cohort through owner retirement; new readers
        // observe the unavailable publication rather than a stale pack alias.
        assert_eq!(retained.sample(2, 127), Some(2.));
        assert_eq!(family.resident_view().sample(2, 127), None);
    }
    fn family_owner(members: Vec<Option<u32>>, fail_middle: Arc<AtomicBool>)
        -> (Residency, crate::SourceStorageFamily, Vec<Pcm>) {
        let packs: Vec<_> = (1..=3).map(|n| {
            let data = vec![[n as f32 / 8., -(n as f32) / 8.]; 128];
            let pcm = Pcm::new(48000, data.clone().into_boxed_slice()).unwrap();
            let fail = fail_middle.clone();
            pcm.set_residency_loader(move || {
                if n == 2 && fail.load(Acquire) { return Err(Error::NotReady); }
                Ok(vec![(0, data.clone().into_boxed_slice())])
            }).unwrap();
            pcm
        }).collect();
        let family = crate::SourceStorageFamily::new_resident(crate::ChannelCount::new(6).unwrap(), packs.clone()).unwrap();
        let plan = Prepared::new_sources(48000, vec![family.clone().into()],
            members.iter().map(|_| region(0)).collect(), members.len()).unwrap()
            .with_groups(members.iter().flatten().copied().max().map_or(0, |x| x + 1), members).unwrap();
        // Move the sole existing worker out of the Prepared test fixture.
        let mut plan = plan;
        (plan.residency.take().unwrap(), family, packs)
    }
    fn finish(owner: &Residency) -> Completion {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(completion) = owner.peek() { owner.consume(completion); return completion; }
            assert!(std::time::Instant::now() < deadline, "complete physical cohort did not finish");
            std::thread::yield_now();
        }
    }
    #[test]
    fn family_group_purge_reclaims_every_pack_then_reload_publishes_one_complete_cohort() {
        let (owner, family, packs) = family_owner(vec![Some(0)], Arc::new(AtomicBool::new(false)));
        let identity = family.id();
        assert!(owner.shared.pinned.is_empty());
        assert_eq!(owner.shared.groups[0].assets.len(), 3);
        owner.request(0, false, 1, ScriptInstanceId(0)).unwrap();
        assert!(finish(&owner).success);
        assert!(!owner.allowed(0));
        assert!(family.as_source().resident_view().is_none());
        assert!(packs.iter().all(|p| p.resident_non_head_bytes() == 0 && p.residency_owners() == 0));
        owner.request(0, true, 2, ScriptInstanceId(0)).unwrap();
        assert!(!owner.allowed(0));
        assert!(finish(&owner).success);
        assert!(owner.allowed(0));
        assert_eq!(family.id(), identity);
        let view = family.as_source().resident_view().unwrap();
        for c in 0..6 { assert_eq!(view.sample(c, 127), Some(if c % 2 == 0 { (c / 2 + 1) as f32 / 8. } else { -((c / 2 + 1) as f32) / 8. })); }
        assert!(packs.iter().all(|p| p.residency_owners() == 1));
    }
    #[test]
    fn family_failed_middle_reload_rolls_back_all_packs_and_retries_without_partial_publication() {
        let failure = Arc::new(AtomicBool::new(false));
        let (owner, family, packs) = family_owner(vec![Some(0)], failure.clone());
        owner.request(0, false, 1, ScriptInstanceId(0)).unwrap(); assert!(finish(&owner).success);
        failure.store(true, Release);
        owner.request(0, true, 2, ScriptInstanceId(0)).unwrap(); assert!(!finish(&owner).success);
        assert!(!owner.allowed(0));
        assert!(family.as_source().resident_view().is_none());
        assert!(packs.iter().all(|p| p.residency_owners() == 0 && p.resident_non_head_bytes() == 0));
        failure.store(false, Release);
        owner.request(0, true, 3, ScriptInstanceId(0)).unwrap(); assert!(finish(&owner).success);
        assert!(owner.allowed(0));
        assert!(family.as_source().resident_view().is_some());
        assert!(packs.iter().all(|p| p.residency_owners() == 1));
    }
    #[test]
    fn family_shared_group_and_ungrouped_owners_keep_all_dependencies_until_last_release() {
        for members in [vec![Some(0), Some(1)], vec![Some(0), None]] {
            let (owner, family, packs) = family_owner(members.clone(), Arc::new(AtomicBool::new(false)));
            assert!(packs.iter().all(|p| p.residency_owners() == 2));
            owner.request(0, false, 1, ScriptInstanceId(0)).unwrap(); assert!(finish(&owner).success);
            assert!(family.as_source().resident_view().is_some());
            assert!(packs.iter().all(|p| p.residency_owners() == 1));
            if members[1].is_some() {
                owner.request(1, false, 2, ScriptInstanceId(0)).unwrap(); assert!(finish(&owner).success);
                assert!(family.as_source().resident_view().is_none());
                assert!(packs.iter().all(|p| p.resident_non_head_bytes() == 0));
            }
        }
    }
    #[test]
    fn family_purge_retires_borrowed_cohort_only_after_reader_release() {
        let (owner, family, packs) = family_owner(vec![Some(0)], Arc::new(AtomicBool::new(false)));
        let retained = family.resident_view();
        owner.request(0, false, 1, ScriptInstanceId(0)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while family.as_source().resident_view().is_some() {
            assert!(std::time::Instant::now() < deadline); std::thread::yield_now();
        }
        assert_eq!(retained.sample(4, 127), Some(3. / 8.));
        assert!(owner.peek().is_none(), "worker cannot claim physical retirement while the old cohort is borrowed");
        drop(retained);
        assert!(finish(&owner).success);
        assert!(packs.iter().all(|p| p.resident_non_head_bytes() == 0));
    }
    #[test]
    fn family_stale_purge_completion_cannot_revoke_a_newer_reload_owner() {
        let (owner, family, packs) = family_owner(vec![Some(0)], Arc::new(AtomicBool::new(false)));
        owner.request(0, false, 1, ScriptInstanceId(0)).unwrap();
        owner.request(0, true, 2, ScriptInstanceId(0)).unwrap();
        finish(&owner); assert!(!owner.allowed(0));
        assert!(finish(&owner).success); assert!(owner.allowed(0));
        assert_eq!(owner.progress().stale, 1);
        assert!(family.as_source().resident_view().is_some());
        assert!(packs.iter().all(|p| p.residency_owners() == 1));
    }

    #[test]
    fn retired_family_cohort_is_reclaimed_by_a_surviving_stereo_alias_residency_owner() {
        let (owner, family, packs) = family_owner(vec![Some(0)], Arc::new(AtomicBool::new(false)));
        let alias = packs[1].clone();
        let mut stereo = Prepared::new(48000, vec![alias.clone()], vec![region(0)], 1).unwrap()
            .with_groups(1, vec![Some(0)]).unwrap();
        let surviving = stereo.residency.take().unwrap();
        assert!(surviving.shared.groups[0].families.is_empty());
        let borrowed = family.resident_view();
        drop(owner); // Its worker is gone while the old cohort is still borrowed.
        assert_eq!(borrowed.sample(2, 127), Some(2. / 8.));
        assert!(family.as_source().resident_view().is_none());
        drop(borrowed); // Audio/read drop must do no reclamation, locking, or I/O.
        // Keep family alive externally. The surviving owner knows only Pcm;
        // it must still be able to reclaim retired family pins that retain it.
        surviving.request(0, false, 1, ScriptInstanceId(0)).unwrap();
        assert!(finish(&surviving).success);
        assert_eq!(alias.resident_non_head_bytes(), 0);
        assert!(family.as_source().resident_view().is_none());
    }

    #[test]
    fn current_family_cohort_is_reclaimed_by_surviving_all_pack_stereo_aliases() {
        let (owner, family, packs) = family_owner(vec![Some(0)], Arc::new(AtomicBool::new(false)));
        let mut aliases = Prepared::new(48000, packs.clone(), (0..3).map(region).collect(), 3).unwrap()
            .with_groups(1, vec![Some(0); 3]).unwrap();
        let surviving = aliases.residency.take().unwrap();
        assert!(surviving.shared.groups[0].families.is_empty());
        drop(owner);
        // Every physical pack has a surviving independent alias owner, so A's
        // family still publishes a current cohort after its own worker retires.
        assert!(family.as_source().resident_view().is_some());
        assert!(packs.iter().all(|p| p.residency_owners() == 1));
        surviving.request(0, false, 1, ScriptInstanceId(0)).unwrap();
        assert!(finish(&surviving).success);
        assert!(family.as_source().resident_view().is_none());
        assert!(packs.iter().all(|p| p.resident_non_head_bytes() == 0));
    }

}
