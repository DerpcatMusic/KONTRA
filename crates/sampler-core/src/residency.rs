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
struct Group {
    assets: Box<[Pcm]>,
    loaded: AtomicBool,
    revision: AtomicU64,
}
struct Slot {
    state: AtomicUsize, // 0 free, 1 requested, 2 complete
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
    slots: Box<[Slot]>,
    stop: AtomicBool,
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
        for region in 0..plan.region_count() {
            let pcm = plan.region_asset(region).unwrap();
            let members = match plan.region_groups[region] {
                Some(g) => &mut groups[g as usize],
                None => &mut pinned,
            };
            if !members.iter().any(|p| p.asset_id() == pcm.asset_id()) {
                members.push(pcm.clone());
            }
        }
        for pcm in groups.iter().flatten().chain(&pinned) {
            pcm.residency_acquire();
        }
        let shared = Arc::new(Shared {
            groups: groups
                .into_iter()
                .map(|assets| Group {
                    assets: assets.into(),
                    loaded: AtomicBool::new(true),
                    revision: AtomicU64::new(1),
                })
                .collect(),
            pinned: pinned.into(),
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
                    let group = &worker.groups[slot.group.load(Relaxed)];
                    let revision = slot.revision.load(Relaxed);
                    let reload = slot.reload.load(Relaxed);
                    let mut success = group.revision.load(Acquire) == revision;
                    for pcm in &group.assets {
                        if reload && success {
                            success &= pcm.residency_reload().is_ok();
                        }
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
                    for pcm in &group.assets {
                        // Check after I/O as well: a newer purge owns eligibility and
                        // must not leave a stale head resident when no other owner needs it.
                        while !pcm.residency_collect() && !worker.stop.load(Acquire) {
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
        }
        let next = self.completed.load(Relaxed);
        self.shared.slots[next % JOBS].state.store(0, Release);
        self.completed.store(next.wrapping_add(1), Relaxed);
        self.thread.as_ref().unwrap().thread().unpark();
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
            return Err(Error::InvalidInput);
        }
        if !residency.room() {
            return Err(Error::Capacity);
        }
        let callbacks = generation.callbacks.checked_add(1).ok_or(Error::Capacity)?;
        let id = generation.midi_object.allocate_async_id()?;
        residency.request(group as usize, reload, id, instance)?;
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
    /// Bounded completion drain at a render boundary, also for zero-frame renders.
    pub fn service_residency(&mut self) -> Result<(), Error> {
        for index in 0..self.plans.slots.len() {
            let Some(_) = self.plans.slots[index].value else {
                continue;
            };
            let plan = crate::PlanId(self.plans.id(index));
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
                let callback = self.preflight_async_callback(plan, c.instance)?;
                let generation = self.plans.get_mut(plan.0).unwrap();
                generation.prepared.residency.as_ref().unwrap().consume(c);
                generation.callbacks -= 1;
                self.finish_async_callback(plan, c.id, c.success, callback)?;
            }
        }
        Ok(())
    }
}
